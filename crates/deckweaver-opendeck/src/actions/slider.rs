use openaction::*;

use crate::shared::{
    dimensions_for_instance, register_instance, unregister_instance, update_instance,
    ActionSettings,
};
use deckweaver_core::ActionType;

pub struct SliderAction;

#[async_trait]
impl Action for SliderAction {
    const UUID: &'static str = "com.designgears.deckweaver.slider";
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
        update_instance(instance, ActionType::Slider, &settings);
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
        core_arc.lock().set_volume_relative(
            device_id,
            step,
            settings.device_type.as_deref().map(|t| match t {
                "source" => deckweaver_core::DeviceType::Source,
                _ => deckweaver_core::DeviceType::Target,
            }),
        );
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
        crate::shared::handle_pi_message(instance, ActionType::Slider, settings, payload).await
    }
}
