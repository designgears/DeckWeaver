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
        update_instance(instance, ActionType::Button, &settings);
        Ok(())
    }

    async fn key_up(
        &self,
        _instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let Some(device_id) = settings.device_id.as_deref() else {
            return Ok(());
        };
        let step = settings.normalized_volume_step(true, 5, -20, 20);
        let core_arc = crate::shared::core();
        let core = core_arc.lock();
        if step == 0 {
            core.toggle_mute(
                device_id,
                settings.device_type.as_deref().map(|t| match t {
                    "source" => deckweaver_core::DeviceType::Source,
                    _ => deckweaver_core::DeviceType::Target,
                }),
            );
        } else {
            core.set_volume_relative(
                device_id,
                step,
                settings.device_type.as_deref().map(|t| match t {
                    "source" => deckweaver_core::DeviceType::Source,
                    _ => deckweaver_core::DeviceType::Target,
                }),
            );
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
