use log::info;
use openaction::*;

use crate::knob_touch::{handle_dial_down, handle_dial_up, handle_touch_press};
use crate::shared::{
    dimensions_for_instance, register_instance, unregister_instance, update_instance,
    ActionSettings,
};
use deckweaver_core::ActionType;

pub struct KnobAction;

#[async_trait]
impl Action for KnobAction {
    const UUID: &'static str = "com.designgears.deckweaver.knob";
    type Settings = ActionSettings;

    async fn will_appear(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        let (width, height) = dimensions_for_instance(instance, ActionType::Knob);
        let mut settings = settings.clone();
        settings.volume_step = settings.normalized_volume_step(false, 5, 1, 20);
        register_instance(instance, ActionType::Knob, &settings, width, height);
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
        settings.volume_step = settings.normalized_volume_step(false, 5, 1, 20);
        update_instance(instance, ActionType::Knob, &settings);
        Ok(())
    }

    async fn dial_rotate(
        &self,
        _instance: &Instance,
        settings: &Self::Settings,
        ticks: i16,
        _pressed: bool,
    ) -> OpenActionResult<()> {
        let Some(device_id) = settings.device_id.as_deref() else {
            return Ok(());
        };
        let step = settings.normalized_volume_step(false, 5, 1, 20) as i16;
        let delta = step * ticks;
        let core_arc = crate::shared::core();
        let core = core_arc.lock();
        let is_source = settings.device_type.as_deref() == Some("source");
        if is_source {
            let mix_b = settings.source_mix.as_deref() == Some("B");
            core.set_source_volume_relative(device_id, mix_b, delta as i8);
        } else {
            core.set_volume_relative(
                device_id,
                delta as i8,
                settings.device_type.as_deref().map(|t| match t {
                    "source" => deckweaver_core::DeviceType::Source,
                    _ => deckweaver_core::DeviceType::Target,
                }),
            );
        }
        Ok(())
    }

    async fn dial_down(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        if settings.device_id.is_none() {
            return Ok(());
        }
        handle_dial_down(instance, settings.clone());
        Ok(())
    }

    async fn dial_up(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        if settings.device_id.is_none() {
            return Ok(());
        }
        handle_dial_up(instance, settings.clone()).await
    }

    async fn touch_tap(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
        _position: (u16, u16),
        _hold: bool,
    ) -> OpenActionResult<()> {
        info!(
            "DeckWeaver touch-strip touched on {} (device_id={:?})",
            instance.instance_id, settings.device_id
        );
        if settings.device_id.is_none() {
            return Ok(());
        }
        handle_touch_press(instance, settings.clone()).await;
        Ok(())
    }

    async fn send_to_plugin(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
        payload: &serde_json::Value,
    ) -> OpenActionResult<()> {
        crate::shared::handle_pi_message(instance, ActionType::Knob, settings, payload).await
    }

    async fn property_inspector_did_appear(
        &self,
        instance: &Instance,
        _settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        crate::shared::send_devices(instance).await
    }
}
