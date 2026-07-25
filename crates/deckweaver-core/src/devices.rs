use parking_lot::RwLock;
use pipeweaver_ipc::commands::{AudioConfiguration, PhysicalDevice as PipeweaverPhysicalDevice};
use pipeweaver_profile::{
    Devices, PhysicalDeviceDescriptor, PhysicalSourceDevice, PhysicalTargetDevice,
    VirtualSourceDevice, VirtualTargetDevice,
};
use pipeweaver_shared::{DeviceType as PipeweaverDeviceType, Mix, MuteTarget};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceType {
    Source,
    Target,
}

impl DeviceType {
    pub fn source() -> Self {
        DeviceType::Source
    }

    pub fn target() -> Self {
        DeviceType::Target
    }

    pub fn is_source(self) -> bool {
        matches!(self, DeviceType::Source)
    }

    pub fn is_target(self) -> bool {
        matches!(self, DeviceType::Target)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
        pub id: String,
        pub name: String,
        pub device_type: DeviceType,
        pub is_physical: bool,
        pub volume: u8,
        pub is_muted: bool,
        pub color: Option<DeviceColor>,
        pub source_mix_a_volume: Option<u8>,
        pub source_mix_b_volume: Option<u8>,
        pub source_mix_a_muted: Option<bool>,
        pub source_mix_b_muted: Option<bool>,
        pub source_mute_a_all: Option<bool>,
        pub source_mute_b_all: Option<bool>,
        pub source_mute_a_target_count: Option<u8>,
        pub source_mute_b_target_count: Option<u8>,
        pub source_volumes_linked: Option<bool>,
        pub target_mix_b: Option<bool>,
}

impl Device {
    pub fn source_volume_for_mix(&self, mix_b: bool) -> Option<u8> {
        if mix_b {
            self.source_mix_b_volume
        } else {
            self.source_mix_a_volume
        }
    }

    pub fn source_muted_for_mix(&self, mix_b: bool) -> Option<bool> {
        if mix_b {
            self.source_mix_b_muted
        } else {
            self.source_mix_a_muted
        }
    }

    pub fn with_selected_source_mix(mut self, mix_b: bool) -> Self {
        if self.device_type == DeviceType::Source {
            if let Some(volume) = self.source_volume_for_mix(mix_b) {
                self.volume = volume;
            }
            if let Some(is_muted) = self.source_muted_for_mix(mix_b) {
                self.is_muted = is_muted;
            }
        }
        self
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DeviceColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl DeviceColor {
    pub fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    pub fn rgba(self) -> (u8, u8, u8, u8) {
        (self.red, self.green, self.blue, 255)
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareDevice {
        pub node_id: Option<u32>,
        pub name: Option<String>,
        pub description: Option<String>,
        pub attachment_index: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Status {
    #[serde(default)]
    pub audio: AudioConfiguration,
    #[serde(skip)]
    device_index: RwLock<Option<HashMap<String, Vec<Device>>>>,
}

// Use pipeweaver types directly - Devices is the devices tree

impl Default for Status {
    fn default() -> Self {
        Self {
            audio: AudioConfiguration::default(),
            device_index: RwLock::new(None),
        }
    }
}

impl Clone for Status {
    fn clone(&self) -> Self {
        Self {
            audio: self.audio.clone(),
            device_index: RwLock::new(None),
        }
    }
}

impl Status {
    fn normalize_volume(raw: u8) -> u8 {
        if raw > 100 {
            ((raw as u16 * 100) / 255) as u8
        } else {
            raw
        }
    }

    pub fn devices_tree(&self) -> &Devices {
        &self.audio.profile.devices
    }

    pub fn get_target_sources(&self, target_id: &str) -> Vec<Device> {
        let mut sources = Vec::new();

        for (source_id, targets) in &self.audio.profile.routes {
            if targets.iter().any(|target| target.to_string() == target_id) {
                if let Some(device) =
                    self.get_device(&source_id.to_string(), Some(DeviceType::Source))
                {
                    sources.push(device);
                }
            }
        }

        sources
    }

    pub fn get_output_hardware_devices(&self) -> Vec<HardwareDevice> {
        self.audio.devices[PipeweaverDeviceType::Target]
            .iter()
            .map(Self::convert_hardware_device)
            .collect()
    }

    pub fn get_input_hardware_devices(&self) -> Vec<HardwareDevice> {
        self.audio.devices[PipeweaverDeviceType::Source]
            .iter()
            .map(Self::convert_hardware_device)
            .collect()
    }

    pub fn get_hardware_device_name(&self, node_id: u32, is_input: bool) -> Option<String> {
        let device_type = if is_input {
            PipeweaverDeviceType::Source
        } else {
            PipeweaverDeviceType::Target
        };
        self.audio.devices[device_type]
            .iter()
            .find(|d| d.node_id == node_id)
            .and_then(|d| d.name.clone().or_else(|| d.description.clone()))
    }

    pub fn get_attached_output_hardware_devices(&self, target_id: &str) -> Vec<HardwareDevice> {
        self.get_attached_physical_hardware_devices_by_id(
            target_id,
            &self.devices_tree().targets.physical_devices,
            &self.audio.devices[PipeweaverDeviceType::Target],
        )
    }

    pub fn get_attached_input_hardware_devices(&self, source_id: &str) -> Vec<HardwareDevice> {
        self.get_attached_source_hardware_devices(
            source_id,
        )
    }

    fn get_attached_source_hardware_devices(
        &self,
        source_id: &str,
    ) -> Vec<HardwareDevice> {
        let Some(device) = self
            .devices_tree()
            .sources
            .physical_devices
            .iter()
            .find(|d| d.description.id.to_string() == source_id)
        else {
            return Vec::new();
        };

        let available = &self.audio.devices[PipeweaverDeviceType::Source];
        device
            .attached_devices
            .iter()
            .enumerate()
            .map(|(index, descriptor)| {
                if let Some(hw) =
                    Self::match_descriptor_to_hardware_device(descriptor, available)
                {
                    let mut converted = Self::convert_hardware_device(hw);
                    converted.attachment_index = Some(index);
                    converted
                } else {
                    HardwareDevice {
                        node_id: None,
                        name: descriptor.name.clone(),
                        description: descriptor.description.clone(),
                        attachment_index: Some(index),
                    }
                }
            })
            .collect()
    }

    fn get_attached_physical_hardware_devices_by_id(
        &self,
        device_id: &str,
        physical_devices: &[pipeweaver_profile::PhysicalTargetDevice],
        available: &[PipeweaverPhysicalDevice],
    ) -> Vec<HardwareDevice> {
        let Some(device) = physical_devices
            .iter()
            .find(|d| d.description.id.to_string() == device_id)
        else {
            return Vec::new();
        };

        device
            .attached_devices
            .iter()
            .enumerate()
            .map(|(index, descriptor)| {
                if let Some(hw) =
                    Self::match_descriptor_to_hardware_device(descriptor, available)
                {
                    let mut converted = Self::convert_hardware_device(hw);
                    converted.attachment_index = Some(index);
                    converted
                } else {
                    HardwareDevice {
                        node_id: None,
                        name: descriptor.name.clone(),
                        description: descriptor.description.clone(),
                        attachment_index: Some(index),
                    }
                }
            })
            .collect()
    }

    pub(crate) fn rebuild_index(&self) {
        let mut index = HashMap::new();
        for device in self.get_all_devices() {
            index
                .entry(device.id.clone())
                .or_insert_with(Vec::new)
                .push(device);
        }
        *self.device_index.write() = Some(index);
    }

    pub fn get_sources(&self) -> Vec<Device> {
        let tree = self.devices_tree();
        let mut devices = Vec::new();

        for raw in &tree.sources.virtual_devices {
            if let Some(device) = Self::convert_virtual_source(raw) {
                devices.push(device);
            }
        }
        for raw in &tree.sources.physical_devices {
            if let Some(device) = Self::convert_physical_source(raw) {
                devices.push(device);
            }
        }

        devices
    }

    pub fn get_targets(&self) -> Vec<Device> {
        let tree = self.devices_tree();
        let mut devices = Vec::new();

        for raw in &tree.targets.virtual_devices {
            if let Some(device) = Self::convert_virtual_target(raw) {
                devices.push(device);
            }
        }
        for raw in &tree.targets.physical_devices {
            if let Some(device) = Self::convert_physical_target(raw) {
                devices.push(device);
            }
        }

        devices
    }

    pub fn get_physical_sources(&self) -> Vec<Device> {
        self.devices_tree()
            .sources
            .physical_devices
            .iter()
            .filter_map(|raw| Self::convert_physical_source(raw))
            .collect()
    }

    pub fn get_physical_targets(&self) -> Vec<Device> {
        self.devices_tree()
            .targets
            .physical_devices
            .iter()
            .filter_map(|raw| Self::convert_physical_target(raw))
            .collect()
    }

    pub fn get_all_devices(&self) -> Vec<Device> {
        let mut devices = self.get_sources();
        devices.extend(self.get_targets());
        devices
    }

    pub fn get_device(&self, device_id: &str, device_type: Option<DeviceType>) -> Option<Device> {
        {
            let index_guard = self.device_index.read();
            if index_guard.is_none() {
                drop(index_guard);
                self.rebuild_index();
            }
        }

        let index_guard = self.device_index.read();
        if let Some(ref index) = *index_guard {
            index.get(device_id).and_then(|devices| {
                devices
                    .iter()
                    .find(|d| device_type.is_none_or(|t| d.device_type == t))
                    .cloned()
            })
        } else {
            self.get_all_devices()
                .into_iter()
                .find(|d| d.id == device_id && device_type.is_none_or(|t| d.device_type == t))
        }
    }

    pub fn infer_device_type(&self, device_id: &str, prefer_target: bool) -> Option<DeviceType> {
        let has_source = self
            .get_device(device_id, Some(DeviceType::Source))
            .is_some();
        let has_target = self
            .get_device(device_id, Some(DeviceType::Target))
            .is_some();

        match (has_source, has_target, prefer_target) {
            (true, false, _) => Some(DeviceType::Source),
            (false, true, _) => Some(DeviceType::Target),
            (true, true, true) => Some(DeviceType::Target),
            (true, true, false) => Some(DeviceType::Source),
            _ => None,
        }
    }

    fn convert_virtual_source(raw: &VirtualSourceDevice) -> Option<Device> {
        Self::convert_source_common(&raw.description, &raw.volumes, &raw.mute_states, false)
    }

    fn convert_physical_source(raw: &PhysicalSourceDevice) -> Option<Device> {
        Self::convert_source_common(&raw.description, &raw.volumes, &raw.mute_states, true)
    }

    fn convert_virtual_target(raw: &VirtualTargetDevice) -> Option<Device> {
        Self::convert_target_common(
            &raw.description,
            &raw.volume,
            &raw.mute_state,
            raw.mix,
            false,
        )
    }

    fn convert_physical_target(raw: &PhysicalTargetDevice) -> Option<Device> {
        Self::convert_target_common(
            &raw.description,
            &raw.volume,
            &raw.mute_state,
            raw.mix,
            true,
        )
    }

    fn convert_source_common(
        description: &pipeweaver_profile::DeviceDescription,
        volumes: &pipeweaver_profile::Volumes,
        mute_states: &pipeweaver_profile::MuteStates,
        is_physical: bool,
    ) -> Option<Device> {
        let id = description.id.to_string();
        let name = description.name.clone();

        let mix_a_volume = Self::normalize_volume(volumes.volume[Mix::A]);
        let mix_b_volume = Self::normalize_volume(volumes.volume[Mix::B]);
        let mix_a_muted = mute_states.mute_state.contains(&MuteTarget::TargetA);
        let mix_b_muted = mute_states.mute_state.contains(&MuteTarget::TargetB);
        let mix_a_target_count = mute_states.mute_targets[MuteTarget::TargetA]
            .len()
            .min(u8::MAX as usize) as u8;
        let mix_b_target_count = mute_states.mute_targets[MuteTarget::TargetB]
            .len()
            .min(u8::MAX as usize) as u8;
        let mix_a_all = mix_a_target_count == 0;
        let mix_b_all = mix_b_target_count == 0;

        let color = Some(DeviceColor {
            red: description.colour.red,
            green: description.colour.green,
            blue: description.colour.blue,
        });

        Some(Device {
            id: id.clone(),
            name: name.clone(),
            device_type: DeviceType::Source,
            is_physical,
            volume: mix_a_volume,
            is_muted: mix_a_muted,
            color,
            source_mix_a_volume: Some(mix_a_volume),
            source_mix_b_volume: Some(mix_b_volume),
            source_mix_a_muted: Some(mix_a_muted),
            source_mix_b_muted: Some(mix_b_muted),
            source_mute_a_all: Some(mix_a_all),
            source_mute_b_all: Some(mix_b_all),
            source_mute_a_target_count: Some(mix_a_target_count),
            source_mute_b_target_count: Some(mix_b_target_count),
            source_volumes_linked: Some(volumes.volumes_linked.is_some()),
            target_mix_b: None,
        })
    }

    fn convert_target_common(
        description: &pipeweaver_profile::DeviceDescription,
        volume: &u8,
        mute_state: &pipeweaver_shared::MuteState,
        mix: Mix,
        is_physical: bool,
    ) -> Option<Device> {
        let id = description.id.to_string();
        let name = description.name.clone();
        let volume = Self::normalize_volume(*volume);

        let is_muted = matches!(mute_state, pipeweaver_shared::MuteState::Muted);

        let color = Some(DeviceColor {
            red: description.colour.red,
            green: description.colour.green,
            blue: description.colour.blue,
        });

        Some(Device {
            id: id.clone(),
            name: name.clone(),
            device_type: DeviceType::Target,
            is_physical,
            volume,
            is_muted,
            color,
            source_mix_a_volume: None,
            source_mix_b_volume: None,
            source_mix_a_muted: None,
            source_mix_b_muted: None,
            source_mute_a_all: None,
            source_mute_b_all: None,
            source_mute_a_target_count: None,
            source_mute_b_target_count: None,
            source_volumes_linked: None,
            target_mix_b: Some(matches!(mix, Mix::B)),
        })
    }

    fn convert_hardware_device(device: &PipeweaverPhysicalDevice) -> HardwareDevice {
        HardwareDevice {
            node_id: Some(device.node_id),
            name: device.name.clone(),
            description: device.description.clone(),
            attachment_index: None,
        }
    }

    fn match_descriptor_to_hardware_device<'a>(
        descriptor: &PhysicalDeviceDescriptor,
        devices: &'a [PipeweaverPhysicalDevice],
    ) -> Option<&'a PipeweaverPhysicalDevice> {
        if let Some(name) = descriptor.name.as_ref() {
            if let Some(device) = devices
                .iter()
                .find(|device| device.name.as_ref() == Some(name))
            {
                return Some(device);
            }
        }

        if let Some(description) = descriptor.description.as_ref() {
            return devices
                .iter()
                .find(|device| device.description.as_ref() == Some(description));
        }

        None
    }
}

pub(crate) fn apply_patch_op(
    doc: &mut serde_json::Value,
    op: &serde_json::Value,
) -> Result<(), String> {
    let operation = op.get("op").and_then(|v| v.as_str()).ok_or("Missing op")?;
    let path = op
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("Missing path")?;

    let (parent, key) = resolve_pointer_parent(doc, path)?;

    match operation {
        "add" => {
            let value = op.get("value").cloned().ok_or("Missing value")?;
            match parent {
                serde_json::Value::Array(arr) => {
                    if key == "-" {
                        arr.push(value);
                    } else {
                        let idx: usize = key.parse().map_err(|_| "Invalid array index")?;
                        if idx > arr.len() {
                            return Err("Array index out of bounds".into());
                        }
                        arr.insert(idx, value);
                    }
                }
                serde_json::Value::Object(obj) => {
                    obj.insert(key, value);
                }
                _ => return Err("Parent is not a container".into()),
            }
        }
        "replace" => {
            let value = op.get("value").cloned().ok_or("Missing value")?;
            match parent {
                serde_json::Value::Array(arr) => {
                    let idx: usize = key.parse().map_err(|_| "Invalid array index")?;
                    if idx >= arr.len() {
                        return Err("Array index out of bounds".into());
                    }
                    arr[idx] = value;
                }
                serde_json::Value::Object(obj) => {
                    if !obj.contains_key(&key) {
                        return Err("Object key not found".into());
                    }
                    obj.insert(key, value);
                }
                _ => return Err("Parent is not a container".into()),
            }
        }
        "remove" => match parent {
            serde_json::Value::Array(arr) => {
                let idx: usize = key.parse().map_err(|_| "Invalid array index")?;
                if idx >= arr.len() {
                    return Err("Array index out of bounds".into());
                }
                arr.remove(idx);
            }
            serde_json::Value::Object(obj) => {
                if obj.remove(&key).is_none() {
                    return Err("Object key not found".into());
                }
            }
            _ => return Err("Parent is not a container".into()),
        },
        _ => {
            tracing::warn!("Unsupported patch operation: {}", operation);
        }
    }

    Ok(())
}

fn resolve_pointer_parent<'a>(
    doc: &'a mut serde_json::Value,
    path: &str,
) -> Result<(&'a mut serde_json::Value, String), String> {
    if !path.starts_with('/') {
        return Err("Path must start with /".into());
    }

    let parts: Vec<String> = path[1..]
        .split('/')
        .map(|p| p.replace("~1", "/").replace("~0", "~"))
        .collect();

    if parts.is_empty() {
        return Err("Empty path".into());
    }

    let key = parts.last().ok_or("Empty path")?.clone();
    let parent_path = &parts[..parts.len() - 1];

    let mut current = doc;
    for part in parent_path {
        current = match current {
            serde_json::Value::Array(arr) => {
                let idx: usize = part.parse().map_err(|_| "Invalid array index")?;
                arr.get_mut(idx).ok_or("Array index out of bounds")?
            }
            serde_json::Value::Object(obj) => obj.get_mut(part).ok_or("Object key not found")?,
            _ => return Err("Cannot traverse non-container".into()),
        };
    }

    Ok((current, key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_device_type() {
        assert_eq!(DeviceType::source(), DeviceType::Source);
        assert_eq!(DeviceType::target(), DeviceType::Target);
    }

    #[test]
    fn test_device_color_rgba() {
        let color = DeviceColor::new(100, 150, 200);
        assert_eq!(color.rgba(), (100, 150, 200, 255));
    }

    #[test]
    fn test_patch_add_array_inserts() {
        let mut doc = json!({ "arr": [1, 3] });
        let op = json!({ "op": "add", "path": "/arr/1", "value": 2 });
        apply_patch_op(&mut doc, &op).unwrap();
        assert_eq!(doc, json!({ "arr": [1, 2, 3] }));
    }

    #[test]
    fn test_patch_replace_array_out_of_bounds_fails() {
        let mut doc = json!({ "arr": [1, 2] });
        let op = json!({ "op": "replace", "path": "/arr/2", "value": 99 });
        assert!(apply_patch_op(&mut doc, &op).is_err());
    }

    #[test]
    fn test_patch_replace_missing_object_key_fails() {
        let mut doc = json!({ "obj": { "a": 1 } });
        let op = json!({ "op": "replace", "path": "/obj/b", "value": 2 });
        assert!(apply_patch_op(&mut doc, &op).is_err());
    }

    #[test]
    fn test_patch_remove_missing_key_fails() {
        let mut doc = json!({ "obj": { "a": 1 } });
        let op = json!({ "op": "remove", "path": "/obj/b" });
        assert!(apply_patch_op(&mut doc, &op).is_err());
    }

    #[test]
    fn test_patch_missing_parent_fails() {
        let mut doc = json!({ "obj": {} });
        let op = json!({ "op": "add", "path": "/obj/missing/value", "value": 5 });
        assert!(apply_patch_op(&mut doc, &op).is_err());
    }
}
