use openaction::*;

use super::{DeckAction, Inspector};
use crate::shared::ActionSettings;
use deckweaver_core::ActionType;

/// Turns the route from one source into one target on and off, e.g. to monitor a microphone in
/// headphones only when wanted.
pub struct RouteToggleAction;

#[async_trait]
impl DeckAction for RouteToggleAction {
    const UUID: &'static str = "com.designgears.deckweaver.routetoggle";
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
        let (Some(source_id), Some(target_id)) = (
            settings.device_id.as_deref(),
            settings.route_target_id.as_deref(),
        ) else {
            return Ok(());
        };
        crate::shared::core()
            .lock()
            .toggle_route(source_id, target_id);
        Ok(())
    }
}
