use openaction::*;

use crate::shared::{
    dimensions_for_instance, register_instance, unregister_instance, update_instance,
    ActionSettings,
};
use deckweaver_core::ActionType;

pub struct SourceSwitchAction;

#[async_trait]
impl Action for SourceSwitchAction {
    const UUID: &'static str = "com.designgears.deckweaver.sourceswitch";
    type Settings = ActionSettings;

    async fn will_appear(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let (width, height) = dimensions_for_instance(instance, ActionType::Button);
        register_instance(instance, ActionType::Button, settings, width, height);
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
        update_instance(instance, ActionType::Button, settings);
        Ok(())
    }

    async fn key_up(
        &self,
        _instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let (Some(target_id), Some(node_id)) =
            (settings.device_id.as_deref(), settings.hardware_device_node_id)
        else {
            return Ok(());
        };
        crate::shared::core()
            .lock()
            .switch_output_hardware_device(target_id, node_id);
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
