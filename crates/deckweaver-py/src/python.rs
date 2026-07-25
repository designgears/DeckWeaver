//! PyO3 wrappers around deckweaver-core.

use deckweaver_core::{
    action_dimensions as core_action_dimensions, load_icon_to_png_bytes, ActionConfig as CoreActionConfig,
    ActionType as CoreActionType, ButtonRenderer as CoreButtonRenderer,
    ControllerKind, DeckWeaverCore as CoreEngine, Device as CoreDevice,
    DeviceColor as CoreDeviceColor, DeviceType as CoreDeviceType,
    HardwareDevice as CoreHardwareDevice, KnobRenderer as CoreKnobRenderer,
    SliderRenderer as CoreSliderRenderer, DEFAULT_PORT, VERSION,
};
use pyo3::basic::CompareOp;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};

#[pyclass]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionType {
    Knob,
    Slider,
    Button,
}

#[pymethods]
impl ActionType {
    #[staticmethod]
    fn knob() -> Self {
        ActionType::Knob
    }

    #[staticmethod]
    fn slider() -> Self {
        ActionType::Slider
    }

    #[staticmethod]
    fn button() -> Self {
        ActionType::Button
    }

    fn __repr__(&self) -> &'static str {
        match self {
            ActionType::Knob => "ActionType.Knob",
            ActionType::Slider => "ActionType.Slider",
            ActionType::Button => "ActionType.Button",
        }
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp) -> bool {
        match op {
            CompareOp::Eq => self == other,
            CompareOp::Ne => self != other,
            _ => false,
        }
    }

    fn __hash__(&self) -> u64 {
        match self {
            ActionType::Knob => 0,
            ActionType::Slider => 1,
            ActionType::Button => 2,
        }
    }
}

impl From<ActionType> for CoreActionType {
    fn from(value: ActionType) -> Self {
        match value {
            ActionType::Knob => CoreActionType::Knob,
            ActionType::Slider => CoreActionType::Slider,
            ActionType::Button => CoreActionType::Button,
        }
    }
}

impl From<CoreActionType> for ActionType {
    fn from(value: CoreActionType) -> Self {
        match value {
            CoreActionType::Knob => ActionType::Knob,
            CoreActionType::Slider => ActionType::Slider,
            CoreActionType::Button => ActionType::Button,
        }
    }
}

#[pyclass]
#[derive(Debug, Clone)]
pub struct ActionConfig {
    #[pyo3(get, set)]
    pub action_id: String,
    #[pyo3(get, set)]
    pub action_type: ActionType,
    #[pyo3(get, set)]
    pub device_id: Option<String>,
    #[pyo3(get, set)]
    pub device_type: Option<DeviceType>,
    #[pyo3(get, set)]
    pub volume_step: i8,
    #[pyo3(get, set)]
    pub width: u32,
    #[pyo3(get, set)]
    pub height: u32,
    #[pyo3(get, set)]
    pub meters_enabled: bool,
    #[pyo3(get, set)]
    pub meter_invert: bool,
    #[pyo3(get, set)]
    pub volume_bar_color: Option<(u8, u8, u8, u8)>,
    #[pyo3(get, set)]
    pub meter_color: Option<(u8, u8, u8, u8)>,
    #[pyo3(get, set)]
    pub orientation: String,
    #[pyo3(get, set)]
    pub is_top: bool,
    #[pyo3(get, set)]
    pub icon_png: Option<Vec<u8>>,
    #[pyo3(get, set)]
    pub icon_path: Option<String>,
    #[pyo3(get, set)]
    pub button_overlay: bool,
    #[pyo3(get, set)]
    pub source_mix_b: bool,
    #[pyo3(get, set)]
    pub mute_profile_index: u8,
    #[pyo3(get, set)]
    pub mute_profile_muted: bool,
    #[pyo3(get, set)]
    pub mute_profile_data: Vec<bool>,
    #[pyo3(get, set)]
    pub show_volume: bool,
}

#[pymethods]
impl ActionConfig {
    #[new]
    #[pyo3(signature = (action_id, action_type, width=200, height=100))]
    fn new(action_id: String, action_type: ActionType, width: u32, height: u32) -> Self {
        let core = CoreActionConfig::new(action_id, action_type.into(), width, height);
        Self::from_core(core)
    }

    fn apply_knob_settings(
        &mut self,
        source_mix_b: bool,
        mute_profile_index: u8,
        mute_profile_data: Vec<bool>,
        show_volume: bool,
    ) {
        let mut core = self.to_core();
        core.apply_knob_settings(
            source_mix_b,
            mute_profile_index,
            mute_profile_data,
            show_volume,
        );
        *self = Self::from_core(core);
    }
}

impl ActionConfig {
    fn from_core(core: CoreActionConfig) -> Self {
        Self {
            action_id: core.action_id,
            action_type: core.action_type.into(),
            device_id: core.device_id,
            device_type: core.device_type.map(DeviceType::from_core),
            volume_step: core.volume_step,
            width: core.width,
            height: core.height,
            meters_enabled: core.meters_enabled,
            meter_invert: core.meter_invert,
            volume_bar_color: core.volume_bar_color,
            meter_color: core.meter_color,
            orientation: core.orientation,
            is_top: core.is_top,
            icon_png: core.icon_png,
            icon_path: core.icon_path,
            button_overlay: core.button_overlay,
            source_mix_b: core.source_mix_b,
            mute_profile_index: core.mute_profile_index,
            mute_profile_muted: core.mute_profile_muted,
            mute_profile_data: core.mute_profile_data,
            show_volume: core.show_volume,
        }
    }

    fn to_core(&self) -> CoreActionConfig {
        CoreActionConfig {
            action_id: self.action_id.clone(),
            action_type: self.action_type.into(),
            device_id: self.device_id.clone(),
            device_type: self.device_type.map(|dt| dt.into_core()),
            volume_step: self.volume_step,
            width: self.width,
            height: self.height,
            meters_enabled: self.meters_enabled,
            meter_invert: self.meter_invert,
            volume_bar_color: self.volume_bar_color,
            meter_color: self.meter_color,
            orientation: self.orientation.clone(),
            is_top: self.is_top,
            icon_png: self.icon_png.clone(),
            icon_path: self.icon_path.clone(),
            button_overlay: self.button_overlay,
            source_mix_b: self.source_mix_b,
            mute_profile_index: self.mute_profile_index,
            mute_profile_muted: self.mute_profile_muted,
            mute_profile_data: self.mute_profile_data.clone(),
            show_volume: self.show_volume,
        }
    }
}

#[pyclass]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    Source,
    Target,
}

#[pymethods]
impl DeviceType {
    #[staticmethod]
    fn source() -> Self {
        DeviceType::Source
    }

    #[staticmethod]
    fn target() -> Self {
        DeviceType::Target
    }

    fn __repr__(&self) -> &'static str {
        match self {
            DeviceType::Source => "DeviceType.Source",
            DeviceType::Target => "DeviceType.Target",
        }
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp) -> bool {
        match op {
            CompareOp::Eq => self == other,
            CompareOp::Ne => self != other,
            _ => false,
        }
    }

    fn __hash__(&self) -> u64 {
        match self {
            DeviceType::Source => 0,
            DeviceType::Target => 1,
        }
    }

    fn is_source(&self) -> bool {
        matches!(self, DeviceType::Source)
    }

    fn is_target(&self) -> bool {
        matches!(self, DeviceType::Target)
    }
}

impl DeviceType {
    fn from_core(value: CoreDeviceType) -> Self {
        match value {
            CoreDeviceType::Source => DeviceType::Source,
            CoreDeviceType::Target => DeviceType::Target,
        }
    }

    fn into_core(self) -> CoreDeviceType {
        match self {
            DeviceType::Source => CoreDeviceType::Source,
            DeviceType::Target => CoreDeviceType::Target,
        }
    }
}

#[pyclass]
#[derive(Debug, Clone)]
pub struct DeviceColor {
    #[pyo3(get)]
    pub red: u8,
    #[pyo3(get)]
    pub green: u8,
    #[pyo3(get)]
    pub blue: u8,
}

#[pymethods]
impl DeviceColor {
    #[new]
    fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    fn rgba(&self) -> (u8, u8, u8, u8) {
        (self.red, self.green, self.blue, 255)
    }
}

#[pyclass]
#[derive(Debug, Clone)]
pub struct Device {
    #[pyo3(get)]
    pub id: String,
    #[pyo3(get)]
    pub name: String,
    #[pyo3(get)]
    pub device_type: DeviceType,
    #[pyo3(get)]
    pub is_physical: bool,
    #[pyo3(get)]
    pub volume: u8,
    #[pyo3(get)]
    pub is_muted: bool,
    #[pyo3(get)]
    pub color: Option<DeviceColor>,
    #[pyo3(get)]
    pub source_mix_a_volume: Option<u8>,
    #[pyo3(get)]
    pub source_mix_b_volume: Option<u8>,
    #[pyo3(get)]
    pub source_mix_a_muted: Option<bool>,
    #[pyo3(get)]
    pub source_mix_b_muted: Option<bool>,
    #[pyo3(get)]
    pub source_mute_a_all: Option<bool>,
    #[pyo3(get)]
    pub source_mute_b_all: Option<bool>,
    #[pyo3(get)]
    pub source_mute_a_target_count: Option<u8>,
    #[pyo3(get)]
    pub source_mute_b_target_count: Option<u8>,
    #[pyo3(get)]
    pub source_volumes_linked: Option<bool>,
    #[pyo3(get)]
    pub target_mix_b: Option<bool>,
}

impl Device {
    fn from_core(device: CoreDevice) -> Self {
        Self {
            id: device.id,
            name: device.name,
            device_type: match device.device_type {
                CoreDeviceType::Source => DeviceType::Source,
                CoreDeviceType::Target => DeviceType::Target,
            },
            is_physical: device.is_physical,
            volume: device.volume,
            is_muted: device.is_muted,
            color: device.color.map(|c| DeviceColor {
                red: c.red,
                green: c.green,
                blue: c.blue,
            }),
            source_mix_a_volume: device.source_mix_a_volume,
            source_mix_b_volume: device.source_mix_b_volume,
            source_mix_a_muted: device.source_mix_a_muted,
            source_mix_b_muted: device.source_mix_b_muted,
            source_mute_a_all: device.source_mute_a_all,
            source_mute_b_all: device.source_mute_b_all,
            source_mute_a_target_count: device.source_mute_a_target_count,
            source_mute_b_target_count: device.source_mute_b_target_count,
            source_volumes_linked: device.source_volumes_linked,
            target_mix_b: device.target_mix_b,
        }
    }
}

#[pyclass]
#[derive(Debug, Clone)]
pub struct HardwareDevice {
    #[pyo3(get)]
    pub node_id: Option<u32>,
    #[pyo3(get)]
    pub name: Option<String>,
    #[pyo3(get)]
    pub description: Option<String>,
    #[pyo3(get)]
    pub attachment_index: Option<usize>,
}

impl HardwareDevice {
    fn from_core(device: CoreHardwareDevice) -> Self {
        Self {
            node_id: device.node_id,
            name: device.name,
            description: device.description,
            attachment_index: device.attachment_index,
        }
    }
}

#[pyclass]
pub struct DeckWeaverCore {
    inner: CoreEngine,
}

#[pymethods]
impl DeckWeaverCore {
    #[new]
    fn new() -> Self {
        Self {
            inner: CoreEngine::new(),
        }
    }

    fn start(&mut self) {
        self.inner.start();
    }

    fn stop(&mut self) {
        self.inner.stop();
    }

    fn register_action(&self, config: ActionConfig) {
        self.inner.register_action(config.to_core());
    }

    fn unregister_action(&self, action_id: &str) {
        self.inner.unregister_action(action_id);
    }

    fn update_action(&self, action_id: &str, config: ActionConfig) {
        self.inner.update_action(action_id, config.to_core());
    }

    fn get_pending_updates<'a>(&self, py: Python<'a>) -> PyResult<Bound<'a, PyDict>> {
        let updates = self.inner.get_pending_updates();
        let dict = PyDict::new(py);
        for (action_id, update) in updates {
            let entry = PyDict::new(py);
            if let Some(bytes) = update.image {
                entry.set_item("image", PyBytes::new(py, &bytes))?;
                if let Some(w) = update.width {
                    entry.set_item("width", w)?;
                }
                if let Some(h) = update.height {
                    entry.set_item("height", h)?;
                }
            } else {
                entry.set_item("image", py.None())?;
            }
            if let Some(label) = update.label {
                entry.set_item("label", label)?;
            } else {
                entry.set_item("label", py.None())?;
            }
            dict.set_item(action_id, entry)?;
        }
        Ok(dict)
    }

    fn is_available(&self) -> bool {
        self.inner.is_available()
    }

    fn get_devices(&self) -> Vec<Device> {
        self.inner
            .get_devices()
            .into_iter()
            .map(Device::from_core)
            .collect()
    }

    #[pyo3(name = "get_sources")]
    fn py_get_sources(&self) -> Vec<Device> {
        self.inner
            .get_sources()
            .into_iter()
            .map(Device::from_core)
            .collect()
    }

    #[pyo3(name = "get_targets")]
    fn py_get_targets(&self) -> Vec<Device> {
        self.inner
            .get_targets()
            .into_iter()
            .map(Device::from_core)
            .collect()
    }

    #[pyo3(name = "get_physical_sources")]
    fn py_get_physical_sources(&self) -> Vec<Device> {
        self.inner
            .get_physical_sources()
            .into_iter()
            .map(Device::from_core)
            .collect()
    }

    #[pyo3(name = "get_physical_targets")]
    fn py_get_physical_targets(&self) -> Vec<Device> {
        self.inner
            .get_physical_targets()
            .into_iter()
            .map(Device::from_core)
            .collect()
    }

    #[pyo3(signature = (device_id, device_type=None))]
    fn get_device(&self, device_id: &str, device_type: Option<DeviceType>) -> Option<Device> {
        self.inner
            .get_device(device_id, device_type.map(|dt| dt.into_core()))
            .map(Device::from_core)
    }

    #[pyo3(name = "get_target_sources")]
    fn py_get_target_sources(&self, target_id: &str) -> Vec<Device> {
        self.inner
            .get_target_sources(target_id)
            .into_iter()
            .map(Device::from_core)
            .collect()
    }

    #[pyo3(name = "get_output_hardware_devices")]
    fn py_get_output_hardware_devices(&self) -> Vec<HardwareDevice> {
        self.inner
            .get_output_hardware_devices()
            .into_iter()
            .map(HardwareDevice::from_core)
            .collect()
    }

    #[pyo3(name = "get_input_hardware_devices")]
    fn py_get_input_hardware_devices(&self) -> Vec<HardwareDevice> {
        self.inner
            .get_input_hardware_devices()
            .into_iter()
            .map(HardwareDevice::from_core)
            .collect()
    }

    fn get_hardware_device_name(&self, node_id: u32, is_input: bool) -> Option<String> {
        self.inner.get_hardware_device_name(node_id, is_input)
    }

    fn infer_device_type(&self, device_id: &str, prefer_target: bool) -> Option<DeviceType> {
        self.inner
            .infer_device_type(device_id, prefer_target)
            .map(DeviceType::from_core)
    }

    #[pyo3(signature = (device_id, volume, device_type=None))]
    fn set_volume(&self, device_id: &str, volume: u8, device_type: Option<DeviceType>) -> bool {
        self.inner.set_volume(
            device_id,
            volume,
            device_type.map(|dt| dt.into_core()),
        )
    }

    #[pyo3(signature = (device_id, delta, device_type=None))]
    fn set_volume_relative(
        &self,
        device_id: &str,
        delta: i8,
        device_type: Option<DeviceType>,
    ) -> bool {
        self.inner.set_volume_relative(
            device_id,
            delta,
            device_type.map(|dt| dt.into_core()),
        )
    }

    #[pyo3(signature = (device_id, device_type=None))]
    fn toggle_mute(&self, device_id: &str, device_type: Option<DeviceType>) -> bool {
        self.inner
            .toggle_mute(device_id, device_type.map(|dt| dt.into_core()))
    }

    fn set_source_volume_relative(&self, device_id: &str, mix_b: bool, delta: i8) -> bool {
        self.inner
            .set_source_volume_relative(device_id, mix_b, delta)
    }

    fn set_source_mute(&self, device_id: &str, mix_b: bool, muted: bool) -> bool {
        self.inner.set_source_mute(device_id, mix_b, muted)
    }

    fn set_target_mute(&self, device_id: &str, muted: bool) -> bool {
        self.inner.set_target_mute(device_id, muted)
    }

    fn set_target_mix(&self, device_id: &str, mix_b: bool) -> bool {
        self.inner.set_target_mix(device_id, mix_b)
    }

    fn toggle_target_mute(&self, device_id: &str) -> bool {
        self.inner.toggle_target_mute(device_id)
    }

    fn toggle_target_mix(&self, device_id: &str) -> bool {
        self.inner.toggle_target_mix(device_id)
    }

    fn toggle_source_volumes_linked(&self, device_id: &str) -> bool {
        self.inner.toggle_source_volumes_linked(device_id)
    }

    fn apply_mute_profile(&self, config: &ActionConfig) -> bool {
        self.inner.apply_mute_profile(&config.to_core())
    }

    fn switch_output_hardware_device(&self, target_id: &str, node_id: u32) -> bool {
        self.inner.switch_output_hardware_device(target_id, node_id)
    }

    fn switch_input_hardware_device(&self, source_id: &str, node_id: u32) -> bool {
        self.inner.switch_input_hardware_device(source_id, node_id)
    }

    fn get_action_device_name(&self, action_id: &str) -> Option<String> {
        self.inner.get_action_device_name(action_id)
    }
}

#[pyclass]
pub struct KnobRenderer {
    inner: CoreKnobRenderer,
}

#[pymethods]
impl KnobRenderer {
    #[new]
    #[pyo3(signature = (width=200, height=100))]
    fn new(width: u32, height: u32) -> Self {
        Self {
            inner: CoreKnobRenderer::new(width, height),
        }
    }
}

#[pyclass]
pub struct SliderRenderer {
    inner: CoreSliderRenderer,
}

#[pymethods]
impl SliderRenderer {
    #[new]
    #[pyo3(signature = (button_size=72))]
    fn new(button_size: u32) -> Self {
        Self {
            inner: CoreSliderRenderer::new(button_size),
        }
    }
}

#[pyclass]
pub struct ButtonRenderer {
    inner: CoreButtonRenderer,
}

#[pymethods]
impl ButtonRenderer {
    #[new]
    #[pyo3(signature = (button_size=72))]
    fn new(button_size: u32) -> Self {
        Self {
            inner: CoreButtonRenderer::new(button_size),
        }
    }
}

#[pyfunction]
#[pyo3(signature = (action_type, is_encoder=false))]
pub fn action_dimensions(action_type: ActionType, is_encoder: bool) -> (u32, u32) {
    let controller = if is_encoder {
        ControllerKind::Encoder
    } else {
        ControllerKind::Keypad
    };
    core_action_dimensions(action_type.into(), controller)
}

#[pyfunction]
pub fn load_icon_to_png(path: &str) -> PyResult<Option<Vec<u8>>> {
    Ok(load_icon_to_png_bytes(path))
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<DeckWeaverCore>()?;
    m.add_class::<ActionConfig>()?;
    m.add_class::<ActionType>()?;
    m.add_class::<Device>()?;
    m.add_class::<DeviceColor>()?;
    m.add_class::<DeviceType>()?;
    m.add_class::<HardwareDevice>()?;
    m.add_class::<KnobRenderer>()?;
    m.add_class::<SliderRenderer>()?;
    m.add_class::<ButtonRenderer>()?;
    m.add_function(wrap_pyfunction!(action_dimensions, m)?)?;
    m.add_function(wrap_pyfunction!(load_icon_to_png, m)?)?;
    m.add("VERSION", VERSION)?;
    m.add("DEFAULT_PORT", DEFAULT_PORT)?;
    Ok(())
}
