//! Per-application volume actions.
//!
//! These talk to the sound server directly rather than to PipeWeaver, so they work on a machine
//! that has never run it. They are grouped in one module because the three only differ in which
//! input gesture they expose; everything below the gesture is shared.

use openaction::*;

use crate::shared::{
    dimensions_for_instance, register_instance, unregister_instance, ActionSettings,
};
use deckweaver_core::ActionType;

/// Apply a press/rotate to the bound app. A step of zero means the key is a mute toggle, matching
/// how the PipeWeaver button action reads its step.
fn apply_step(settings: &ActionSettings, step: i8, ticks: i16) {
    let Some(device_id) = settings.device_id.as_deref() else {
        return;
    };
    let core_arc = crate::shared::core();
    let core = core_arc.lock();

    if step == 0 {
        core.toggle_app_mute(device_id);
    } else {
        core.set_app_volume_relative(device_id, step as i16 * ticks);
    }
}

pub struct AppKnobAction;

#[async_trait]
impl Action for AppKnobAction {
    const UUID: &'static str = "com.designgears.deckweaver.appknob";
    type Settings = ActionSettings;

    async fn will_appear(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let (width, height) = dimensions_for_instance(instance, ActionType::Knob);
        let mut settings = settings.clone();
        settings.volume_step = settings.normalized_volume_step(false, 5, 1, 20);
        register_instance(instance, ActionType::Knob, &settings, width, height);
        Ok(())
    }

    async fn will_disappear(
        &self,
        instance: &Instance,
        _settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        unregister_instance(instance);
        Ok(())
    }

    async fn did_receive_settings(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let mut settings = settings.clone();
        settings.volume_step = settings.normalized_volume_step(false, 5, 1, 20);
        crate::shared::update_instance(instance, ActionType::Knob, &settings);
        Ok(())
    }

    async fn dial_rotate(
        &self,
        _instance: &Instance,
        settings: &Self::Settings,
        ticks: i16,
        _pressed: bool,
    ) -> OpenActionResult<()> {
        // Rotation always changes volume; a zero step here would make the dial inert, so the
        // mute-on-zero convention deliberately does not apply.
        let step = settings.normalized_volume_step(false, 5, 1, 20);
        apply_step(settings, step.max(1), ticks);
        Ok(())
    }

    async fn dial_up(
        &self,
        _instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let Some(device_id) = settings.device_id.as_deref() else {
            return Ok(());
        };
        crate::shared::core().lock().toggle_app_mute(device_id);
        Ok(())
    }

    async fn touch_tap(
        &self,
        _instance: &Instance,
        settings: &Self::Settings,
        _position: (u16, u16),
        _hold: bool,
    ) -> OpenActionResult<()> {
        let Some(device_id) = settings.device_id.as_deref() else {
            return Ok(());
        };
        crate::shared::core().lock().toggle_app_mute(device_id);
        Ok(())
    }

    async fn property_inspector_did_appear(
        &self,
        instance: &Instance,
        _settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        crate::shared::send_apps(instance).await
    }

    async fn send_to_plugin(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
        payload: &serde_json::Value,
    ) -> OpenActionResult<()> {
        crate::shared::handle_pi_message(instance, ActionType::Knob, settings, payload).await
    }
}

pub struct AppButtonAction;

#[async_trait]
impl Action for AppButtonAction {
    const UUID: &'static str = "com.designgears.deckweaver.appbutton";
    type Settings = ActionSettings;

    async fn will_appear(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let (width, height) = dimensions_for_instance(instance, ActionType::Button);
        let mut settings = settings.clone();
        settings.volume_step = settings.normalized_volume_step(true, 5, -20, 20);
        register_instance(instance, ActionType::Button, &settings, width, height);
        Ok(())
    }

    async fn will_disappear(
        &self,
        instance: &Instance,
        _settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        unregister_instance(instance);
        Ok(())
    }

    async fn did_receive_settings(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let mut settings = settings.clone();
        settings.volume_step = settings.normalized_volume_step(true, 5, -20, 20);
        crate::shared::update_instance(instance, ActionType::Button, &settings);
        Ok(())
    }

    async fn key_up(
        &self,
        _instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let step = settings.normalized_volume_step(true, 5, -20, 20);
        apply_step(settings, step, 1);
        Ok(())
    }

    async fn property_inspector_did_appear(
        &self,
        instance: &Instance,
        _settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        crate::shared::send_apps(instance).await
    }

    async fn send_to_plugin(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
        payload: &serde_json::Value,
    ) -> OpenActionResult<()> {
        crate::shared::handle_pi_message(instance, ActionType::Button, settings, payload).await
    }
}

pub struct AppSliderAction;

#[async_trait]
impl Action for AppSliderAction {
    const UUID: &'static str = "com.designgears.deckweaver.appslider";
    type Settings = ActionSettings;

    async fn will_appear(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let (width, height) = dimensions_for_instance(instance, ActionType::Slider);
        let mut settings = settings.clone();
        settings.volume_step = settings.normalized_volume_step(true, 5, -20, 20);
        register_instance(instance, ActionType::Slider, &settings, width, height);
        Ok(())
    }

    async fn will_disappear(
        &self,
        instance: &Instance,
        _settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        unregister_instance(instance);
        Ok(())
    }

    async fn did_receive_settings(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let mut settings = settings.clone();
        settings.volume_step = settings.normalized_volume_step(true, 5, -20, 20);
        crate::shared::update_instance(instance, ActionType::Slider, &settings);
        Ok(())
    }

    async fn key_up(
        &self,
        _instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let step = settings.normalized_volume_step(true, 5, -20, 20);
        apply_step(settings, step, 1);
        Ok(())
    }

    async fn property_inspector_did_appear(
        &self,
        instance: &Instance,
        _settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        crate::shared::send_apps(instance).await
    }

    async fn send_to_plugin(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
        payload: &serde_json::Value,
    ) -> OpenActionResult<()> {
        crate::shared::handle_pi_message(instance, ActionType::Slider, settings, payload).await
    }
}
