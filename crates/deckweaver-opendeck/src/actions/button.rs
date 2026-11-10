use openaction::*;

use super::{DeckAction, Inspector, start_hold};
use crate::shared::ActionSettings;
use deckweaver_core::ActionType;

pub struct ButtonAction;

#[async_trait]
impl DeckAction for ButtonAction {
    const UUID: &'static str = "com.designgears.deckweaver.button";
    const ACTION_TYPE: ActionType = ActionType::Button;
    const INSPECTOR: Inspector = Inspector::Devices;

    fn render_settings(settings: &ActionSettings) -> ActionSettings {
        settings.with_volume_step(settings.key_volume_step())
    }

    /// Volume steps happen on press and repeat while the key is held; see [`crate::key_repeat`].
    async fn key_down(
        &self,
        instance: &Instance,
        settings: &ActionSettings,
    ) -> OpenActionResult<()> {
        let device_type = settings.parse_device_type();
        start_hold(
            instance,
            settings,
            settings.key_volume_step(),
            move |core, id, step| {
                core.set_volume_relative(id, step, device_type);
            },
        );
        Ok(())
    }

    /// A step of 0 makes the key a mute toggle, which fires once on release.
    async fn key_up(&self, instance: &Instance, settings: &ActionSettings) -> OpenActionResult<()> {
        crate::key_repeat::stop(&instance.instance_id.to_string());
        let Some(device_id) = settings.device_id.as_deref() else {
            return Ok(());
        };
        if settings.key_volume_step() == 0 {
            crate::shared::core()
                .lock()
                .toggle_mute(device_id, settings.parse_device_type());
        }
        Ok(())
    }
}
