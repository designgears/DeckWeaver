use log::info;
use openaction::*;

use super::{DeckAction, Inspector};
use crate::knob_touch::{handle_dial_down, handle_dial_up, handle_touch_press};
use crate::shared::ActionSettings;
use deckweaver_core::ActionType;

pub struct KnobAction;

#[async_trait]
impl DeckAction for KnobAction {
    const UUID: &'static str = "com.designgears.deckweaver.knob";
    const ACTION_TYPE: ActionType = ActionType::Knob;
    const INSPECTOR: Inspector = Inspector::Devices;

    fn render_settings(settings: &ActionSettings) -> ActionSettings {
        settings.with_volume_step(settings.knob_volume_step())
    }

    async fn dial_rotate(&self, settings: &ActionSettings, ticks: i16) -> OpenActionResult<()> {
        let Some(device_id) = settings.device_id.as_deref() else {
            return Ok(());
        };
        let step = settings.knob_volume_step() as i16;
        // Clamped before narrowing: a fast spin (20% x 7 detents) would otherwise wrap past
        // i8::MAX and turn a big volume-up into a volume-down. 100 covers the whole range anyway.
        let delta = (step * ticks).clamp(-100, 100) as i8;
        let core_arc = crate::shared::core();
        let core = core_arc.lock();
        if settings.device_type.as_deref() == Some("source") {
            core.set_source_volume_relative(device_id, settings.is_mix_b(), delta);
        } else {
            core.set_volume_relative(device_id, delta, settings.parse_device_type());
        }
        Ok(())
    }

    async fn dial_down(
        &self,
        instance: &Instance,
        settings: &ActionSettings,
    ) -> OpenActionResult<()> {
        if settings.device_id.is_some() {
            handle_dial_down(instance, settings.clone());
        }
        Ok(())
    }

    async fn dial_up(
        &self,
        instance: &Instance,
        settings: &ActionSettings,
    ) -> OpenActionResult<()> {
        if settings.device_id.is_none() {
            return Ok(());
        }
        handle_dial_up(instance, settings.clone()).await
    }

    async fn touch_tap(
        &self,
        instance: &Instance,
        settings: &ActionSettings,
    ) -> OpenActionResult<()> {
        info!(
            "DeckWeaver touch-strip touched on {} (device_id={:?})",
            instance.instance_id, settings.device_id
        );
        if settings.device_id.is_some() {
            handle_touch_press(instance, settings.clone()).await;
        }
        Ok(())
    }
}
