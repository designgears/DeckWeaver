use std::collections::HashMap;
use std::time::{Duration, Instant};

use deckweaver_core::ActionType;
use log::debug;
use once_cell::sync::Lazy;
use openaction::Instance;
use parking_lot::Mutex;
use tokio::task::JoinHandle;

use crate::shared::{build_config_for_instance, core, update_instance, ActionSettings};

const MUTE_PROFILE_COUNT: u8 = 2;
const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(275);

struct TouchTapState {
    pending: bool,
    last_tap: Instant,
    settings: ActionSettings,
    flush_task: Option<JoinHandle<()>>,
}

impl Default for TouchTapState {
    fn default() -> Self {
        Self {
            pending: false,
            last_tap: Instant::now(),
            settings: ActionSettings::default(),
            flush_task: None,
        }
    }
}

static TOUCH_TAP_STATES: Lazy<Mutex<HashMap<String, TouchTapState>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub async fn handle_touch_press(instance: &Instance, settings: ActionSettings) {
    let instance_id = instance.instance_id.to_string();
    let now = Instant::now();

    let double_tap = {
        let mut states = TOUCH_TAP_STATES.lock();
        let state = states.entry(instance_id.clone()).or_default();
        let within_window =
            state.pending && now.duration_since(state.last_tap) <= DOUBLE_TAP_WINDOW;

        if within_window {
            state.pending = false;
            if let Some(task) = state.flush_task.take() {
                task.abort();
            }
            state.settings = settings;
            true
        } else {
            if let Some(task) = state.flush_task.take() {
                task.abort();
            }
            state.pending = true;
            state.last_tap = now;
            state.settings = settings;
            false
        }
    };

    if double_tap {
        debug!("DeckWeaver knob double tap on {}", instance_id);
        flush_touch_tap(instance, 2).await;
        return;
    }

    let id = instance_id.clone();
    let task = tokio::spawn(async move {
        tokio::time::sleep(DOUBLE_TAP_WINDOW).await;
        let settings = {
            let mut states = TOUCH_TAP_STATES.lock();
            let Some(state) = states.get_mut(&id) else {
                return;
            };
            if !state.pending {
                return;
            }
            state.pending = false;
            state.flush_task = None;
            state.settings.clone()
        };
        let Some(instance) = openaction::get_instance(id.clone()).await else {
            debug!("DeckWeaver knob tap dropped: instance {} gone", id);
            return;
        };
        debug!("DeckWeaver knob single tap on {}", id);
        flush_touch_tap_with_settings(&instance, settings, 1).await;
    });

    TOUCH_TAP_STATES
        .lock()
        .entry(instance_id)
        .or_default()
        .flush_task = Some(task);
}

async fn flush_touch_tap(instance: &Instance, tap_count: u8) {
    let settings = TOUCH_TAP_STATES
        .lock()
        .get(&instance.instance_id.to_string())
        .map(|state| state.settings.clone())
        .unwrap_or_default();
    flush_touch_tap_with_settings(instance, settings, tap_count).await;
}

async fn flush_touch_tap_with_settings(
    instance: &Instance,
    settings: ActionSettings,
    tap_count: u8,
) {
    if tap_count >= 2 {
        let _ = toggle_knob_mix(instance, settings).await;
    } else {
        let _ = cycle_mute_profile(instance, settings).await;
    }
}

pub async fn cycle_mute_profile(
    instance: &Instance,
    mut settings: ActionSettings,
) -> openaction::OpenActionResult<()> {
    // Only selects which mix the press addresses. Pushing the stored value onto the new mix here
    // would clobber a mute the user made in PipeWeaver's own UI.
    settings.mute_profile_index = (settings.mute_profile_index + 1) % MUTE_PROFILE_COUNT;
    instance.set_settings(&settings).await?;
    update_instance(instance, ActionType::Knob, &settings);
    Ok(())
}

pub async fn toggle_active_profile_mute(
    instance: &Instance,
    mut settings: ActionSettings,
) -> openaction::OpenActionResult<()> {
    let mut data = settings.mute_profile_data();
    let idx = settings.mute_profile_index as usize;
    if idx >= data.len() {
        return Ok(());
    }
    let config = build_config_for_instance(instance, ActionType::Knob, &settings);
    let Some(muted) = core().lock().toggle_mute_profile(&config) else {
        return Ok(());
    };
    data[idx] = muted;
    settings.mute_profile_data = Some(serde_json::to_string(&data).unwrap_or_default());
    instance.set_settings(&settings).await?;
    update_instance(instance, ActionType::Knob, &settings);
    Ok(())
}

pub async fn toggle_knob_mix(
    instance: &Instance,
    mut settings: ActionSettings,
) -> openaction::OpenActionResult<()> {
    let Some(device_id) = settings.device_id.clone() else {
        return Ok(());
    };

    if settings.device_type.as_deref() == Some("source") {
        let mix_b = settings.source_mix.as_deref() == Some("B");
        settings.source_mix = Some(if mix_b { "A" } else { "B" }.to_string());
        instance.set_settings(&settings).await?;
        update_instance(instance, ActionType::Knob, &settings);
        return Ok(());
    }

    core().lock().toggle_target_mix(&device_id);
    Ok(())
}

