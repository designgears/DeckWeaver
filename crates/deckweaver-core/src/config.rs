use crate::action::ActionConfig;

impl ActionConfig {
    pub fn apply_knob_settings(
        &mut self,
        source_mix_b: bool,
        mute_profile_index: u8,
        mute_profile_data: Vec<bool>,
        show_volume: bool,
    ) {
        self.source_mix_b = source_mix_b;
        self.mute_profile_index = mute_profile_index;
        self.mute_profile_data = mute_profile_data;
        self.show_volume = show_volume;
        self.mute_profile_muted = self
            .mute_profile_data
            .get(mute_profile_index as usize)
            .copied()
            .unwrap_or(false);
    }

    pub fn toggle_mute_profile(&mut self) -> bool {
        let idx = self.mute_profile_index as usize;
        if idx >= self.mute_profile_data.len() {
            return false;
        }
        self.mute_profile_data[idx] = !self.mute_profile_data[idx];
        self.mute_profile_muted = self.mute_profile_data[idx];
        true
    }
}

pub fn mute_profile_muted(mute_profile_index: u8, mute_profile_data: &[bool]) -> bool {
    mute_profile_data
        .get(mute_profile_index as usize)
        .copied()
        .unwrap_or(false)
}

#[allow(clippy::too_many_arguments)]
pub fn populate_common_fields(
    config: &mut ActionConfig,
    device_id: Option<String>,
    device_type: Option<crate::devices::DeviceType>,
    volume_step: i8,
    meters_enabled: bool,
    meter_invert: bool,
    volume_bar_color: Option<(u8, u8, u8, u8)>,
    meter_color: Option<(u8, u8, u8, u8)>,
    icon_path: Option<String>,
    orientation: Option<String>,
) {
    config.device_id = device_id;
    config.device_type = device_type;
    config.volume_step = volume_step;
    config.meters_enabled = meters_enabled;
    config.meter_invert = meter_invert;
    config.volume_bar_color = volume_bar_color;
    config.meter_color = meter_color;
    config.icon_path = icon_path;
    config.orientation = orientation.unwrap_or_else(|| "vertical".to_string());
    config.is_top = volume_step > 0;
}
