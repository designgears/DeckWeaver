use std::sync::Arc;
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use deckweaver_core::{
    ActionConfig, ActionType, ControllerKind, DeckWeaverCore, DeviceType as CoreDeviceType,
    action_dimensions, populate_common_fields,
};
use image::codecs::png::PngEncoder;
use image::{ColorType, ImageEncoder, RgbaImage};
use once_cell::sync::OnceCell;
use openaction::{Instance, get_instance};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;

static CORE: OnceCell<Arc<Mutex<DeckWeaverCore>>> = OnceCell::new();

pub fn core() -> Arc<Mutex<DeckWeaverCore>> {
    CORE.get_or_init(|| {
        let mut engine = DeckWeaverCore::new();
        engine.start();
        let shared = Arc::new(Mutex::new(engine));
        spawn_update_loop(shared.clone());
        shared
    })
    .clone()
}

fn controller_kind(instance: &Instance) -> ControllerKind {
    if instance.controller.eq_ignore_ascii_case("Encoder") {
        ControllerKind::Encoder
    } else {
        ControllerKind::Keypad
    }
}

/// Dimensions used by the shared renderer and sent to OpenDeck via setImage.
pub fn dimensions_for_instance(instance: &Instance, action_type: ActionType) -> (u32, u32) {
    action_dimensions(action_type, controller_kind(instance))
}

fn spawn_update_loop(core: Arc<Mutex<DeckWeaverCore>>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(33)).await;
            let updates = core.lock().get_pending_updates();
            for (action_id, update) in updates {
                let Some(instance) = get_instance(action_id.clone()).await else {
                    continue;
                };

                if let Some(bytes) = update.image
                    && let Some(width) = update.width
                    && let Some(height) = update.height
                    && let Some(data_uri) = rgba_to_data_uri(&bytes, width, height)
                {
                    // We're drawing, so what are we drawing to?
                    if controller_kind(&instance) == ControllerKind::Encoder {
                        // We're drawing to an encoder, send the image to the layout
                        let feedback = json!({"img": data_uri.clone()});
                        let _ = instance.set_feedback(&feedback).await;

                        // setFeedback only reaches the LCD strip: OpenDeck's UI draws encoder
                        // slots with the same square-canvas path as keypad keys, off the state
                        // image, and never reads the parsed layout. Without this the UI is stuck
                        // on the manifest's actionDefaultImage while the hardware animates.
                        // Harmless for the strip itself: OpenDeck only falls back to these bytes
                        // when an action has no encoder config, and while it does feed a differing
                        // state image to the renderer as an icon override, that override is
                        // dropped unless the layout declares an "icon" pixmap item (ours doesn't).
                        let _ = instance.set_image(Some(data_uri), None).await;
                    } else {
                        // We're drawing to a button
                        let _ = instance.set_image(Some(data_uri), None).await;
                    }
                }

                // If the label needs updating, do it now. Safe to send unconditionally: a title
                // only reaches the strip if the layout declares a "title" text item (ours is a
                // lone pixmap), and only reaches the UI if the state sets ShowTitle (ours is
                // false), so it never lands on top of the dial the knob renderer already drew.
                if let Some(label) = update.label {
                    let _ = instance
                        .set_title(Some(label.chars().take(25).collect::<String>()), None)
                        .await;
                }
            }
        }
    });
}

fn rgba_to_data_uri(rgba: &[u8], width: u32, height: u32) -> Option<String> {
    let image = RgbaImage::from_raw(width, height, rgba.to_vec())?;
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(image.as_raw(), width, height, ColorType::Rgba8.into())
        .ok()?;
    Some(format!("data:image/png;base64,{}", STANDARD.encode(png)))
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct ActionSettings {
    pub device_id: Option<String>,
    pub device_type: Option<String>,
    pub volume_step: i8,
    pub meters_enabled: bool,
    pub meter_invert_color: bool,
    pub meter_color: Option<Vec<u8>>,
    pub volume_bar_color: Option<Vec<u8>>,
    pub icon_fa: Option<String>,
    pub icon_path: Option<String>,
    pub source_mix: Option<String>,
    pub mute_profile_index: u8,
    pub mute_profile_data: Option<String>,
    pub orientation: Option<String>,
    pub hardware_device_node_id: Option<u32>,
    pub hardware_device_description: Option<String>,
    /// Knob only. `None` means on: an action whose property inspector has never been opened
    /// should match the renderer's own default rather than silently hiding the readout. Kept as
    /// an `Option` so the derived `Default` — used as a fallback on the touch-tap path — agrees
    /// with what serde produces for absent settings.
    pub show_volume: Option<bool>,
}

impl ActionSettings {
    pub fn normalized_volume_step(&self, signed: bool, default: i8, min: i8, max: i8) -> i8 {
        let mut value = if self.volume_step == 0 {
            default
        } else {
            self.volume_step
        };
        if !signed {
            value = value.abs();
        }
        value.clamp(min, max)
    }

    fn parse_device_type(&self) -> Option<CoreDeviceType> {
        match self.device_type.as_deref() {
            Some("source") => Some(CoreDeviceType::Source),
            Some("target") => Some(CoreDeviceType::Target),
            _ => None,
        }
    }

    fn color_tuple(value: &Option<Vec<u8>>) -> Option<(u8, u8, u8, u8)> {
        let color = value.as_ref()?;
        match color.len() {
            3 => Some((color[0], color[1], color[2], 255)),
            n if n >= 4 => Some((color[0], color[1], color[2], color[3])),
            _ => None,
        }
    }

    pub fn mute_profile_data(&self) -> Vec<bool> {
        let Some(raw) = self.mute_profile_data.as_ref() else {
            return vec![false, false];
        };
        serde_json::from_str::<Vec<bool>>(raw).unwrap_or_else(|_| vec![false, false])
    }
}

fn build_config(
    action_id: String,
    action_type: ActionType,
    settings: &ActionSettings,
    width: u32,
    height: u32,
) -> ActionConfig {
    let mut config = ActionConfig::new(action_id, action_type, width, height);
    let icon_path = if settings
        .icon_fa
        .as_ref()
        .is_some_and(|slug| !slug.is_empty())
    {
        None
    } else {
        settings.icon_path.clone()
    };
    populate_common_fields(
        &mut config,
        settings.device_id.clone(),
        settings.parse_device_type(),
        settings.volume_step,
        settings.meters_enabled,
        settings.meter_invert_color,
        ActionSettings::color_tuple(&settings.volume_bar_color),
        ActionSettings::color_tuple(&settings.meter_color),
        icon_path,
        settings.orientation.clone(),
    );

    if let Some(slug) = settings.icon_fa.as_ref().filter(|slug| !slug.is_empty()) {
        config.icon_png = crate::fa_icons::fa_icon_to_png(slug);
    }

    if action_type == ActionType::Knob {
        config.apply_knob_settings(
            settings.source_mix.as_deref() == Some("B"),
            settings.mute_profile_index,
            settings.mute_profile_data(),
            settings.show_volume.unwrap_or(true),
        );
    }

    config
}

pub fn build_config_for_instance(
    instance: &Instance,
    action_type: ActionType,
    settings: &ActionSettings,
) -> ActionConfig {
    let (width, height) = dimensions_for_instance(instance, action_type);
    build_config(
        instance.instance_id.to_string(),
        action_type,
        settings,
        width,
        height,
    )
}

pub fn register_instance(
    instance: &Instance,
    action_type: ActionType,
    settings: &ActionSettings,
    width: u32,
    height: u32,
) {
    let action_id = instance.instance_id.to_string();
    let mut config = build_config(action_id.clone(), action_type, settings, width, height);

    if config.device_type.is_none() {
        if let Some(device_id) = config.device_id.as_deref() {
            let core_arc = core();
            config.device_type = core_arc
                .lock()
                .infer_device_type(device_id, action_type == ActionType::Button);
        }
    }

    core().lock().register_action(config);
}

pub fn update_instance(instance: &Instance, action_type: ActionType, settings: &ActionSettings) {
    let (width, height) = dimensions_for_instance(instance, action_type);
    let action_id = instance.instance_id.to_string();
    let config = build_config(action_id, action_type, settings, width, height);
    core()
        .lock()
        .update_action(&instance.instance_id.to_string(), config);
}

pub async fn send_devices(instance: &Instance) -> openaction::OpenActionResult<()> {
    let payload = {
        let core_arc = core();
        let core = core_arc.lock();
        serde_json::json!({
            "event": "devices",
            "available": core.is_available(),
            "sources": core.get_sources(),
            "targets": core.get_targets(),
            "outputHardware": core.get_output_hardware_devices(),
            "inputHardware": core.get_input_hardware_devices(),
        })
    };
    instance.send_to_property_inspector(payload).await
}

/// Applications currently playing audio, for the app actions' picker.
///
/// Built by hand rather than deriving `Serialize` on `AppStream` so the wire shape stays a
/// deliberate choice: the picker needs a stable id to store and a name to show, and nothing else
/// the backend happens to track.
pub async fn send_apps(instance: &Instance) -> openaction::OpenActionResult<()> {
    let payload = {
        let core_arc = core();
        let core = core_arc.lock();
        let apps: Vec<serde_json::Value> = core
            .get_apps()
            .into_iter()
            .map(|app| {
                serde_json::json!({
                    "id": app.device_id(),
                    "key": app.key,
                    "name": app.name,
                    "volume": app.volume,
                    "muted": app.is_muted,
                    "iconName": app.icon_name,
                    "routedTo": app.routed_to,
                })
            })
            .collect();

        serde_json::json!({
            "event": "apps",
            "available": core.is_pulse_available(),
            "apps": apps,
            // Lets the picker offer "focused application" only where it can actually work, and
            // show what it currently resolves to.
            "focusId": deckweaver_core::FOCUSED_DEVICE_ID,
            "focusAvailable": core.is_focus_tracking_available(),
            "focusedApp": core.focused_app_name(),
        })
    };
    instance.send_to_property_inspector(payload).await
}

pub fn unregister_instance(instance: &Instance) {
    core()
        .lock()
        .unregister_action(&instance.instance_id.to_string());
}

pub async fn handle_pi_message(
    instance: &Instance,
    _action_type: ActionType,
    _settings: &ActionSettings,
    payload: &serde_json::Value,
) -> openaction::OpenActionResult<()> {
    match payload.get("event").and_then(|v| v.as_str()) {
        Some("refreshDevices") => send_devices(instance).await,
        Some("refreshApps") => send_apps(instance).await,
        _ => Ok(()),
    }
}
