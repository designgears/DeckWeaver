use openaction::*;

use crate::shared::{
    dimensions_for_instance, register_instance, unregister_instance, update_instance,
    ActionSettings,
};
use deckweaver_core::ActionType;

pub struct ButtonAction;

#[async_trait]
impl Action for ButtonAction {
    const UUID: &'static str = "com.designgears.deckweaver.button";
    type Settings = ActionSettings;

    async fn will_appear(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let (width, height) = dimensions_for_instance(instance, ActionType::Button);
        let mut settings = settings.clone();
        settings.volume_step = Some(settings.normalized_volume_step(true, 5, -20, 20));
        register_instance(instance, ActionType::Button, &settings, width, height);
        Ok(())
    }

    async fn will_disappear(
        &self,
        instance: &Instance,
        _settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        crate::key_repeat::stop(&instance.instance_id.to_string());
        unregister_instance(instance);
        Ok(())
    }

    async fn did_receive_settings(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let mut settings = settings.clone();
        settings.volume_step = Some(settings.normalized_volume_step(true, 5, -20, 20));
        update_instance(instance, ActionType::Button, &settings);
        Ok(())
    }

    /// Volume steps happen on press and repeat while the key is held; see [`crate::key_repeat`].
    async fn key_down(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let Some(device_id) = settings.device_id.clone() else {
            return Ok(());
        };
        let step = settings.normalized_volume_step(true, 5, -20, 20);
        if step == 0 {
            return Ok(());
        }
        let device_type = parse_device_type(settings);
        let interval = settings.hold_repeat_interval();
        crate::key_repeat::start(instance.instance_id.to_string(), interval, move || {
            crate::shared::core()
                .lock()
                .set_volume_relative(&device_id, step, device_type);
        });
        Ok(())
    }

    /// A step of 0 makes the key a mute toggle, which fires once on release.
    async fn key_up(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        crate::key_repeat::stop(&instance.instance_id.to_string());
        let Some(device_id) = settings.device_id.as_deref() else {
            return Ok(());
        };
        if settings.normalized_volume_step(true, 5, -20, 20) == 0 {
            crate::shared::core()
                .lock()
                .toggle_mute(device_id, parse_device_type(settings));
        }
        Ok(())
    }

    async fn property_inspector_did_appear(
        &self,
        instance: &Instance,
        _settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        crate::shared::send_devices(instance).await
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

fn parse_device_type(settings: &ActionSettings) -> Option<deckweaver_core::DeviceType> {
    settings.device_type.as_deref().map(|t| match t {
        "source" => deckweaver_core::DeviceType::Source,
        _ => deckweaver_core::DeviceType::Target,
    })
}
