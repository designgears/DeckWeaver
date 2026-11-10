use openaction::*;

use super::{DeckAction, Inspector};
use crate::shared::ActionSettings;
use deckweaver_core::ActionType;

pub struct SourceSwitchAction;

#[async_trait]
impl DeckAction for SourceSwitchAction {
    const UUID: &'static str = "com.designgears.deckweaver.sourceswitch";
    const ACTION_TYPE: ActionType = ActionType::Button;
    const INSPECTOR: Inspector = Inspector::Devices;

    fn render_settings(settings: &ActionSettings) -> ActionSettings {
        settings.as_toggle_key()
    }

    async fn key_up(
        &self,
        _instance: &Instance,
        settings: &ActionSettings,
    ) -> OpenActionResult<()> {
        let (Some(target_id), Some(node_id)) = (
            settings.device_id.as_deref(),
            settings.hardware_device_node_id,
        ) else {
            return Ok(());
        };
        crate::shared::core()
            .lock()
            .switch_output_hardware_device(target_id, node_id);
        Ok(())
    }
}
