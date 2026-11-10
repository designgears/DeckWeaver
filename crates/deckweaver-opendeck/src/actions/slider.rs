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
        settings.volume_step = Some(settings.slider_volume_step());
        register_instance(instance, ActionType::Slider, &settings, width, height);
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
        settings.volume_step = Some(settings.slider_volume_step());
        update_instance(instance, ActionType::Slider, &settings);
        Ok(())
    }

    /// Steps on press and repeats while held; see [`crate::key_repeat`].
    async fn key_down(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let Some(device_id) = settings.device_id.clone() else {
            return Ok(());
        };
        let step = settings.slider_volume_step();
        let device_type = settings.parse_device_type();
        let interval = settings.hold_repeat_interval();
        crate::key_repeat::start(instance.instance_id.to_string(), interval, move || {
            crate::shared::core()
                .lock()
                .set_volume_relative(&device_id, step, device_type);
        });
        Ok(())
    }

    async fn key_up(
        &self,
        instance: &Instance,
        _settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        crate::key_repeat::stop(&instance.instance_id.to_string());
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
