//! The plugin's actions.
//!
//! Every action shares the same lifecycle: register with the core on appear, re-register on new
//! settings, unregister on disappear, and feed its property inspector a device or app list. Only
//! the input gestures differ, so each action implements [`DeckAction`] (its identity plus the
//! gestures it uses) and is registered wrapped in [`Deck`], which supplies the lifecycle.

mod app;
mod button;
mod knob;
mod physical_source_switch;
mod route_toggle;
mod slider;
mod source_switch;

pub use app::{AppButtonAction, AppKnobAction, AppSliderAction};
pub use button::ButtonAction;
pub use knob::KnobAction;
pub use physical_source_switch::PhysicalSourceSwitchAction;
pub use route_toggle::RouteToggleAction;
pub use slider::SliderAction;
pub use source_switch::SourceSwitchAction;

use deckweaver_core::{ActionType, DeckWeaverCore};
use openaction::*;

use crate::shared::{self, ActionSettings};

/// What an action's property inspector lists.
pub enum Inspector {
    /// PipeWeaver sources/targets and hardware devices.
    Devices,
    /// Applications playing audio.
    Apps,
}

/// The parts of an action that differ between actions. Gestures default to doing nothing.
// async_trait marks the default bodies #[must_use] on top of the boxed future's own must_use.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait DeckAction: Send + Sync + 'static {
    const UUID: &'static str;
    const ACTION_TYPE: ActionType;
    const INSPECTOR: Inspector;

    /// The settings handed to the renderer, with `volume_step` resolved to what this action
    /// actually does with it.
    fn render_settings(settings: &ActionSettings) -> ActionSettings;

    async fn key_down(
        &self,
        _instance: &Instance,
        _settings: &ActionSettings,
    ) -> OpenActionResult<()> {
        Ok(())
    }

    async fn key_up(
        &self,
        _instance: &Instance,
        _settings: &ActionSettings,
    ) -> OpenActionResult<()> {
        Ok(())
    }

    async fn dial_rotate(&self, _settings: &ActionSettings, _ticks: i16) -> OpenActionResult<()> {
        Ok(())
    }

    async fn dial_down(
        &self,
        _instance: &Instance,
        _settings: &ActionSettings,
    ) -> OpenActionResult<()> {
        Ok(())
    }

    async fn dial_up(
        &self,
        _instance: &Instance,
        _settings: &ActionSettings,
    ) -> OpenActionResult<()> {
        Ok(())
    }

    async fn touch_tap(
        &self,
        _instance: &Instance,
        _settings: &ActionSettings,
    ) -> OpenActionResult<()> {
        Ok(())
    }
}

/// Adapts a [`DeckAction`] to OpenAction's [`Action`], adding the shared lifecycle.
pub struct Deck<A>(pub A);

#[async_trait]
impl<A: DeckAction> Action for Deck<A> {
    const UUID: &'static str = A::UUID;
    type Settings = ActionSettings;

    async fn will_appear(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        shared::register_instance(instance, A::ACTION_TYPE, &A::render_settings(settings));
        Ok(())
    }

    async fn will_disappear(
        &self,
        instance: &Instance,
        _settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        // A key removed mid-hold never gets its key_up. A no-op for actions that never repeat.
        crate::key_repeat::stop(&instance.instance_id.to_string());
        shared::unregister_instance(instance);
        Ok(())
    }

    async fn did_receive_settings(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        shared::update_instance(instance, A::ACTION_TYPE, &A::render_settings(settings));
        Ok(())
    }

    async fn property_inspector_did_appear(
        &self,
        instance: &Instance,
        _settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        match A::INSPECTOR {
            Inspector::Devices => shared::send_devices(instance).await,
            Inspector::Apps => shared::send_apps(instance).await,
        }
    }

    async fn send_to_plugin(
        &self,
        instance: &Instance,
        _settings: &Self::Settings,
        payload: &serde_json::Value,
    ) -> OpenActionResult<()> {
        shared::handle_pi_message(instance, payload).await
    }

    async fn key_down(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        self.0.key_down(instance, settings).await
    }

    async fn key_up(&self, instance: &Instance, settings: &Self::Settings) -> OpenActionResult<()> {
        self.0.key_up(instance, settings).await
    }

    async fn dial_rotate(
        &self,
        _instance: &Instance,
        settings: &Self::Settings,
        ticks: i16,
        _pressed: bool,
    ) -> OpenActionResult<()> {
        self.0.dial_rotate(settings, ticks).await
    }

    async fn dial_down(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        self.0.dial_down(instance, settings).await
    }

    async fn dial_up(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        self.0.dial_up(instance, settings).await
    }

    async fn touch_tap(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
        _position: (u16, u16),
        _hold: bool,
    ) -> OpenActionResult<()> {
        self.0.touch_tap(instance, settings).await
    }
}

/// Press-and-hold for the volume keys: apply `step` now and keep applying it while the key is
/// held (see [`crate::key_repeat`]). A step of 0 is a mute toggle, which fires once on release
/// instead, so it never starts a hold. Does nothing for an unbound key.
fn start_hold<F>(instance: &Instance, settings: &ActionSettings, step: i8, apply: F)
where
    F: Fn(&DeckWeaverCore, &str, i8) + Send + Sync + 'static,
{
    let Some(device_id) = settings.device_id.clone() else {
        return;
    };
    if step == 0 {
        return;
    }
    crate::key_repeat::start(
        instance.instance_id.to_string(),
        settings.hold_repeat_interval(),
        move || apply(&shared::core().lock(), &device_id, step),
    );
}
