use crate::devices::Device;
use crate::devices::DeviceType;
use parking_lot::RwLock;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionType {
    Knob,
    Slider,
    Button,
}

#[derive(Debug, Clone)]
pub struct ActionConfig {
    pub action_id: String,
        pub action_type: ActionType,
        pub device_id: Option<String>,
        pub device_type: Option<DeviceType>,
        pub volume_step: i8,
        pub width: u32,
        pub height: u32,
        pub meters_enabled: bool,
        pub meter_invert: bool,
        pub volume_bar_color: Option<(u8, u8, u8, u8)>,
        pub meter_color: Option<(u8, u8, u8, u8)>,
        pub orientation: String,
        pub is_top: bool,
        pub icon_png: Option<Vec<u8>>,
        pub icon_path: Option<String>,
        pub button_overlay: bool,
        pub source_mix_b: bool,
        pub mute_profile_index: u8,
        pub mute_profile_muted: bool,
    pub mute_profile_data: Vec<bool>,
    /// Knob only: draw the volume percentage in the top right of the encoder strip.
    pub show_volume: bool,
    /// Populated by the render loop for app actions, not by the host: the channel the app is
    /// routed to, and the icon its own desktop entry advertises. They live here so the existing
    /// per-frame config clone carries them, and so the render caches key off them.
    pub routed_to: Option<String>,
    pub app_icon_path: Option<String>,
}

impl ActionConfig {
    pub fn new(action_id: String, action_type: ActionType, width: u32, height: u32) -> Self {
        Self {
            action_id,
            action_type,
            device_id: None,
            device_type: None,
            volume_step: 5,
            width,
            height,
            meters_enabled: true,
            meter_invert: true,
            volume_bar_color: None,
            meter_color: None,
            orientation: "vertical".to_string(),
            is_top: true,
            icon_png: None,
            icon_path: None,
            button_overlay: true,
            source_mix_b: false,
            mute_profile_index: 0,
            mute_profile_muted: false,
            mute_profile_data: vec![false, false],
            show_volume: true,
            routed_to: None,
            app_icon_path: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CachedIcon {
    pub rgba8: image::RgbaImage,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct CachedBaseRender {
    pub pixmap: tiny_skia::Pixmap,
    pub base_hash: u64,
}

#[derive(Debug)]
pub struct ActionState {
    pub config: ActionConfig,
    pub device: Option<Device>,
    pub meter_value: AtomicU8,
    pub last_render_hash: AtomicU64,
    pub last_label: parking_lot::RwLock<Option<String>>,
    pub cached_icon: RwLock<Option<(u64, CachedIcon)>>,
    pub cached_base: RwLock<Option<CachedBaseRender>>,
}

impl ActionState {
    pub fn new(config: ActionConfig) -> Self {
        Self {
            config,
            device: None,
            meter_value: AtomicU8::new(0),
            // Force first frame to render even when device/meter state hashes to 0.
            last_render_hash: AtomicU64::new(u64::MAX),
            last_label: parking_lot::RwLock::new(None),
            cached_icon: RwLock::new(None),
            cached_base: RwLock::new(None),
        }
    }

    pub fn base_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        if let Some(ref device) = self.device {
            // The knob renderer draws the device name into the base image, so a rename has to
            // invalidate the cached base.
            device.name.hash(&mut hasher);
            device.volume.hash(&mut hasher);
            device.is_muted.hash(&mut hasher);
            device.source_mix_a_muted.hash(&mut hasher);
            device.source_mix_b_muted.hash(&mut hasher);
            device.source_volumes_linked.hash(&mut hasher);
            device.target_mix_b.hash(&mut hasher);
            if let Some(color) = &device.color {
                color.red.hash(&mut hasher);
                color.green.hash(&mut hasher);
                color.blue.hash(&mut hasher);
            }
        }
        self.config.volume_bar_color.hash(&mut hasher);
        self.config.meter_color.hash(&mut hasher);
        self.config.meter_invert.hash(&mut hasher);
        self.config.meters_enabled.hash(&mut hasher);
        if let Some(ref icon_png) = self.config.icon_png {
            icon_png.hash(&mut hasher);
        }
        self.config.icon_path.hash(&mut hasher);
        self.config.app_icon_path.hash(&mut hasher);
        self.config.routed_to.hash(&mut hasher);
        if self.config.action_type == crate::action::ActionType::Slider {
            self.config.orientation.hash(&mut hasher);
            self.config.is_top.hash(&mut hasher);
        }
        if self.config.action_type == crate::action::ActionType::Button {
            self.config.button_overlay.hash(&mut hasher);
        }
        if self.config.action_type == crate::action::ActionType::Knob {
            self.config.source_mix_b.hash(&mut hasher);
            self.config.mute_profile_index.hash(&mut hasher);
            self.config.mute_profile_data.hash(&mut hasher);
            self.config.show_volume.hash(&mut hasher);
        }
        hasher.finish()
    }

    pub fn needs_base_rebuild(&self) -> bool {
        let current_hash = self.base_hash();
        let cached = self.cached_base.read();
        cached.as_ref().is_none_or(|c| c.base_hash != current_hash)
    }

    pub fn get_cached_icon(
        &self,
        png_data: Option<&[u8]>,
        icon_path: Option<&str>,
        max_size: f32,
    ) -> Option<CachedIcon> {
        let Some((icon_hash, png_data)) = self.resolve_icon_source(png_data, icon_path) else {
            *self.cached_icon.write() = None;
            return None;
        };

        {
            let cached = self.cached_icon.read();
            if let Some((cached_hash, cached_icon)) = cached.as_ref() {
                if *cached_hash == icon_hash {
                    return Some(cached_icon.clone());
                }
            }
        }

        let Ok(img) = image::load_from_memory(&png_data) else {
            return None;
        };

        let (iw, ih) = (img.width() as f32, img.height() as f32);
        let scale = (max_size / iw).min(max_size / ih).min(1.0);
        let (sw, sh) = ((iw * scale) as u32, (ih * scale) as u32);

        let resized = img
            .resize(sw, sh, image::imageops::FilterType::Triangle)
            .to_rgba8();

        let cached = CachedIcon {
            rgba8: resized,
            width: sw,
            height: sh,
        };

        *self.cached_icon.write() = Some((icon_hash, cached.clone()));

        Some(cached)
    }

    fn resolve_icon_source(
        &self,
        png_data: Option<&[u8]>,
        icon_path: Option<&str>,
    ) -> Option<(u64, Vec<u8>)> {
        let mut hasher = DefaultHasher::new();
        if let Some(png_data) = png_data {
            0u8.hash(&mut hasher);
            png_data.hash(&mut hasher);
            return Some((hasher.finish(), png_data.to_vec()));
        }

        let icon_path = icon_path?;
        1u8.hash(&mut hasher);
        icon_path.hash(&mut hasher);
        let png_data = crate::icon_loader::load_icon_to_png_bytes(icon_path)?;
        Some((hasher.finish(), png_data))
    }

    pub fn label_changed(&self, new_label: Option<&str>) -> bool {
        let mut last = self.last_label.write();
        let changed = last.as_deref() != new_label;
        if changed {
            *last = new_label.map(|s| s.to_string());
        }
        changed
    }

    pub fn get_meter(&self) -> u8 {
        self.meter_value.load(Ordering::Relaxed)
    }

    pub fn set_meter(&self, value: u8) {
        self.meter_value.store(value, Ordering::Relaxed);
    }

    pub fn render_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.config.action_type.hash(&mut hasher);
        self.config.device_id.hash(&mut hasher);
        self.config.device_type.hash(&mut hasher);
        self.config.routed_to.hash(&mut hasher);
        self.config.app_icon_path.hash(&mut hasher);
        self.config.meters_enabled.hash(&mut hasher);
        self.config.orientation.hash(&mut hasher);
        self.config.is_top.hash(&mut hasher);
        self.config.button_overlay.hash(&mut hasher);
        self.config.device_type.hash(&mut hasher);
        if self.config.action_type == crate::action::ActionType::Knob {
            self.config.source_mix_b.hash(&mut hasher);
            self.config.mute_profile_index.hash(&mut hasher);
            self.config.mute_profile_data.hash(&mut hasher);
            self.config.show_volume.hash(&mut hasher);
        }
        self.get_meter().hash(&mut hasher);

        if let Some(ref device) = self.device {
            device.id.hash(&mut hasher);
            device.name.hash(&mut hasher);
            device.volume.hash(&mut hasher);
            device.is_muted.hash(&mut hasher);
            device.source_mix_a_muted.hash(&mut hasher);
            device.source_mix_b_muted.hash(&mut hasher);
            device.source_volumes_linked.hash(&mut hasher);
            device.target_mix_b.hash(&mut hasher);
        } else {
            0u8.hash(&mut hasher);
        }

        hasher.finish()
    }

    pub fn needs_render(&self) -> bool {
        let current = self.render_hash();
        let last = self.last_render_hash.swap(current, Ordering::Relaxed);
        current != last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ActionConfig {
        ActionConfig::new("test-action".to_string(), ActionType::Knob, 200, 100)
    }

    fn device(name: &str) -> Device {
        Device {
            id: "device-1".to_string(),
            name: name.to_string(),
            device_type: DeviceType::Source,
            is_physical: false,
            volume: 50,
            is_muted: false,
            color: None,
            source_mix_a_volume: Some(50),
            source_mix_b_volume: Some(50),
            source_mix_a_muted: Some(false),
            source_mix_b_muted: Some(false),
            source_mute_a_all: None,
            source_mute_b_all: None,
            source_mute_a_target_count: None,
            source_mute_b_target_count: None,
            source_volumes_linked: Some(false),
            target_mix_b: None,
        }
    }

    #[test]
    fn first_frame_requires_render() {
        let state = ActionState::new(config());
        assert!(state.needs_render());
        assert!(!state.needs_render());
    }

    #[test]
    fn meter_change_triggers_render() {
        let state = ActionState::new(config());
        assert!(state.needs_render());
        state.set_meter(17);
        assert!(state.needs_render());
    }

    /// The knob renderer draws the device name, so a rename has to invalidate both the frame
    /// hash and the cached base pixmap — otherwise the old name stays on the strip until some
    /// unrelated field happens to change.
    #[test]
    fn rename_triggers_render_and_base_rebuild() {
        let mut state = ActionState::new(config());
        state.device = Some(device("Chat"));
        assert!(state.needs_render());
        assert!(!state.needs_render());

        let base_hash = state.base_hash();
        *state.cached_base.write() = Some(CachedBaseRender {
            pixmap: tiny_skia::Pixmap::new(200, 100).unwrap(),
            base_hash,
        });
        assert!(!state.needs_base_rebuild());

        state.device = Some(device("Game"));
        assert!(state.needs_render());
        assert!(state.needs_base_rebuild());
    }
}
