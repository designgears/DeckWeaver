use openaction::*;

use super::{DeckAction, Inspector, start_hold};
use crate::shared::ActionSettings;
use deckweaver_core::ActionType;

pub struct SliderAction;

#[async_trait]
impl DeckAction for SliderAction {
    const UUID: &'static str = "com.designgears.deckweaver.slider";
    const ACTION_TYPE: ActionType = ActionType::Slider;
    const INSPECTOR: Inspector = Inspector::Devices;

    fn render_settings(settings: &ActionSettings) -> ActionSettings {
        settings.with_volume_step(settings.slider_volume_step())
    }

    /// Steps on press and repeats while held; see [`crate::key_repeat`].
    async fn key_down(
        &self,
        instance: &Instance,
        settings: &ActionSettings,
    ) -> OpenActionResult<()> {
        let device_type = settings.parse_device_type();
        start_hold(
            instance,
            settings,
            settings.slider_volume_step(),
            move |core, id, step| {
                core.set_volume_relative(id, step, device_type);
            },
        );
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
