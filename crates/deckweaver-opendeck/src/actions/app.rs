//! Per-application volume actions.
//!
//! These talk to the sound server directly rather than to PipeWeaver, so they work on a machine
//! that has never run it. They are grouped in one module because the three only differ in which
//! input gesture they expose; everything below the gesture is shared.

use openaction::*;

use super::{DeckAction, Inspector, start_hold};
use crate::shared::ActionSettings;
use deckweaver_core::ActionType;

fn toggle_mute(settings: &ActionSettings) {
    if let Some(device_id) = settings.device_id.as_deref() {
        crate::shared::core().lock().toggle_app_mute(device_id);
    }
}

/// Step once on press and keep stepping while the key is held. A step of 0 is a mute toggle,
/// handled on release instead, so it never repeats.
fn start_app_hold(instance: &Instance, settings: &ActionSettings, step: i8) {
    start_hold(instance, settings, step, |core, id, step| {
        core.set_app_volume_relative(id, step as i16);
    });
}

pub struct AppKnobAction;

#[async_trait]
impl DeckAction for AppKnobAction {
    const UUID: &'static str = "com.designgears.deckweaver.appknob";
    const ACTION_TYPE: ActionType = ActionType::Knob;
    const INSPECTOR: Inspector = Inspector::Apps;

    fn render_settings(settings: &ActionSettings) -> ActionSettings {
        settings.with_volume_step(settings.knob_volume_step())
    }

    /// Rotation always changes volume: the knob step is never 0, so the mute-on-zero convention
    /// of the app button deliberately does not apply here.
    async fn dial_rotate(&self, settings: &ActionSettings, ticks: i16) -> OpenActionResult<()> {
        if let Some(device_id) = settings.device_id.as_deref() {
            let step = settings.knob_volume_step() as i16;
            crate::shared::core()
                .lock()
                .set_app_volume_relative(device_id, step * ticks);
        }
        Ok(())
    }

    async fn dial_up(
        &self,
        _instance: &Instance,
        settings: &ActionSettings,
    ) -> OpenActionResult<()> {
        toggle_mute(settings);
        Ok(())
    }

    async fn touch_tap(
        &self,
        _instance: &Instance,
        settings: &ActionSettings,
    ) -> OpenActionResult<()> {
        toggle_mute(settings);
        Ok(())
    }
}

pub struct AppButtonAction;

#[async_trait]
impl DeckAction for AppButtonAction {
    const UUID: &'static str = "com.designgears.deckweaver.appbutton";
    const ACTION_TYPE: ActionType = ActionType::Button;
    const INSPECTOR: Inspector = Inspector::Apps;

    fn render_settings(settings: &ActionSettings) -> ActionSettings {
        settings.with_volume_step(settings.key_volume_step())
    }

    async fn key_down(
        &self,
        instance: &Instance,
        settings: &ActionSettings,
    ) -> OpenActionResult<()> {
        start_app_hold(instance, settings, settings.key_volume_step());
        Ok(())
    }

    /// A step of 0 makes the key a mute toggle, which fires once on release, matching how the
    /// PipeWeaver button action reads its step.
    async fn key_up(&self, instance: &Instance, settings: &ActionSettings) -> OpenActionResult<()> {
        crate::key_repeat::stop(&instance.instance_id.to_string());
        if settings.key_volume_step() == 0 {
            toggle_mute(settings);
        }
        Ok(())
    }
}

pub struct AppSliderAction;

#[async_trait]
impl DeckAction for AppSliderAction {
    const UUID: &'static str = "com.designgears.deckweaver.appslider";
    const ACTION_TYPE: ActionType = ActionType::Slider;
    const INSPECTOR: Inspector = Inspector::Apps;

    fn render_settings(settings: &ActionSettings) -> ActionSettings {
        settings.with_volume_step(settings.slider_volume_step())
    }

    async fn key_down(
        &self,
        instance: &Instance,
        settings: &ActionSettings,
    ) -> OpenActionResult<()> {
        start_app_hold(instance, settings, settings.slider_volume_step());
        Ok(())
    }

    async fn key_up(
        &self,
        instance: &Instance,
        _settings: &ActionSettings,
    ) -> OpenActionResult<()> {
        crate::key_repeat::stop(&instance.instance_id.to_string());
        Ok(())
    }
}
