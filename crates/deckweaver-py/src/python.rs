//! PyO3 wrappers around deckweaver-core.

//!
//! main.py is the only consumer, so this exposes just what it uses; the two change together.

use deckweaver_core::{
    action_dimensions as core_action_dimensions, ActionConfig as CoreActionConfig,
    ActionType as CoreActionType, ControllerKind, DeckWeaverCore as CoreEngine,
    Device as CoreDevice, DeviceType as CoreDeviceType, HardwareDevice as CoreHardwareDevice,
    VERSION,
};
use pyo3::basic::CompareOp;
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

/// A render/behaviour config for one action. Wraps the core config directly, so a field added to
/// the core needs no copying here; Python only gets setters for the fields main.py fills in.
#[pyclass]
#[derive(Debug, Clone)]
pub struct ActionConfig {
    inner: CoreActionConfig,
}

#[pymethods]
impl ActionConfig {
    #[new]
    #[pyo3(signature = (action_id, action_type, width=200, height=100))]
    fn new(action_id: String, action_type: ActionType, width: u32, height: u32) -> Self {
        Self {
            inner: CoreActionConfig::new(action_id, action_type.into(), width, height),
        }
    }

    fn apply_knob_settings(
        &mut self,
        source_mix_b: bool,
        mute_profile_index: u8,
        mute_profile_data: Vec<bool>,
        show_volume: bool,
    ) {
        self.inner.apply_knob_settings(
            source_mix_b,
            mute_profile_index,
            mute_profile_data,
            show_volume,
        );
    }

    #[setter]
    fn set_device_id(&mut self, value: Option<String>) {
        self.inner.device_id = value;
    }

    #[setter]
    fn set_device_type(&mut self, value: Option<DeviceType>) {
        self.inner.device_type = value.map(DeviceType::into_core);
    }

    #[setter]
    fn set_volume_step(&mut self, value: i8) {
        self.inner.volume_step = value;
    }

    #[setter]
    fn set_meters_enabled(&mut self, value: bool) {
        self.inner.meters_enabled = value;
    }

    #[setter]
    fn set_meter_invert(&mut self, value: bool) {
        self.inner.meter_invert = value;
    }

    #[setter]
    fn set_volume_bar_color(&mut self, value: Option<(u8, u8, u8, u8)>) {
        self.inner.volume_bar_color = value;
    }

    #[setter]
    fn set_meter_color(&mut self, value: Option<(u8, u8, u8, u8)>) {
        self.inner.meter_color = value;
    }

    #[setter]
    fn set_orientation(&mut self, value: String) {
        self.inner.orientation = value;
    }

    #[setter]
    fn set_is_top(&mut self, value: bool) {
        self.inner.is_top = value;
    }

    #[setter]
    fn set_icon_path(&mut self, value: Option<String>) {
        self.inner.icon_path = value;
    }

    #[setter]
    fn set_button_overlay(&mut self, value: bool) {
        self.inner.button_overlay = value;
    }

    /// Route toggle only: the target whose route from `device_id` the key flips.
    #[setter]
    fn set_route_target_id(&mut self, value: Option<String>) {
        self.inner.route_target_id = value;
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

/// An application currently playing audio, aggregated across all of its streams.
#[pyclass]
#[derive(Debug, Clone)]
pub struct AppStream {
    /// Device id to store on an action, e.g. "app:msedge".
    #[pyo3(get)]
    pub id: String,
    /// Stable identity across restarts, normally the process binary.
    #[pyo3(get)]
    pub key: String,
    #[pyo3(get)]
    pub name: String,
    #[pyo3(get)]
    pub volume: u8,
    #[pyo3(get)]
    pub is_muted: bool,
    /// `application.icon_name`, suitable for an XDG icon theme lookup.
    #[pyo3(get)]
    pub icon_name: Option<String>,
    /// Channel this app plays into, e.g. the PipeWeaver target.
    #[pyo3(get)]
    pub routed_to: Option<String>,
    /// Host pids behind this app's streams. Empty when the app is sandboxed and reported a pid
    /// from its own namespace, which is discarded as unusable.
    #[pyo3(get)]
    pub pids: Vec<u32>,
}

#[pymethods]
impl AppStream {
    fn __repr__(&self) -> String {
        format!(
            "AppStream(id={:?}, name={:?}, volume={}, is_muted={})",
            self.id, self.name, self.volume, self.is_muted
        )
    }
}

impl AppStream {
    fn from_core(app: deckweaver_core::AppStream) -> Self {
        Self {
            id: app.device_id(),
            key: app.key,
            name: app.name,
            volume: app.volume,
            is_muted: app.is_muted,
            icon_name: app.icon_name,
            routed_to: app.routed_to,
            pids: app.pids,
        }
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
            device_type: DeviceType::from_core(device.device_type),
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

fn devices(list: Vec<CoreDevice>) -> Vec<Device> {
    list.into_iter().map(Device::from_core).collect()
}

fn hardware_devices(list: Vec<CoreHardwareDevice>) -> Vec<HardwareDevice> {
    list.into_iter().map(HardwareDevice::from_core).collect()
}

/// The engine, exposing only what main.py calls.
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
        self.inner.register_action(config.inner);
    }

    fn unregister_action(&self, action_id: &str) {
        self.inner.unregister_action(action_id);
    }

    fn update_action(&self, action_id: &str, config: ActionConfig) {
        self.inner.update_action(action_id, config.inner);
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
            entry.set_item("label", update.label)?;
            dict.set_item(action_id, entry)?;
        }
        Ok(dict)
    }

    fn is_available(&self) -> bool {
        self.inner.is_available()
    }

    fn get_devices(&self) -> Vec<Device> {
        devices(self.inner.get_devices())
    }

    fn get_sources(&self) -> Vec<Device> {
        devices(self.inner.get_sources())
    }

    fn get_targets(&self) -> Vec<Device> {
        devices(self.inner.get_targets())
    }

    fn get_physical_sources(&self) -> Vec<Device> {
        devices(self.inner.get_physical_sources())
    }

    fn get_physical_targets(&self) -> Vec<Device> {
        devices(self.inner.get_physical_targets())
    }

    #[pyo3(signature = (device_id, device_type=None))]
    fn get_device(&self, device_id: &str, device_type: Option<DeviceType>) -> Option<Device> {
        self.inner
            .get_device(device_id, device_type.map(DeviceType::into_core))
            .map(Device::from_core)
    }

    fn get_output_hardware_devices(&self) -> Vec<HardwareDevice> {
        hardware_devices(self.inner.get_output_hardware_devices())
    }

    fn get_input_hardware_devices(&self) -> Vec<HardwareDevice> {
        hardware_devices(self.inner.get_input_hardware_devices())
    }

    fn infer_device_type(&self, device_id: &str, prefer_target: bool) -> Option<DeviceType> {
        self.inner
            .infer_device_type(device_id, prefer_target)
            .map(DeviceType::from_core)
    }

    #[pyo3(signature = (device_id, delta, device_type=None))]
    fn set_volume_relative(
        &self,
        device_id: &str,
        delta: i8,
        device_type: Option<DeviceType>,
    ) -> bool {
        self.inner
            .set_volume_relative(device_id, delta, device_type.map(DeviceType::into_core))
    }

    #[pyo3(signature = (device_id, device_type=None))]
    fn toggle_mute(&self, device_id: &str, device_type: Option<DeviceType>) -> bool {
        self.inner
            .toggle_mute(device_id, device_type.map(DeviceType::into_core))
    }

    /// True once the sound server is reachable. Independent of `is_available`, which reports the
    /// PipeWeaver connection: app actions work without PipeWeaver running.
    fn is_pulse_available(&self) -> bool {
        self.inner.is_pulse_available()
    }

    /// Applications currently playing audio, sorted by name.
    fn get_apps(&self) -> Vec<AppStream> {
        self.inner
            .get_apps()
            .into_iter()
            .map(AppStream::from_core)
            .collect()
    }

    /// Nudge an app's volume by `delta` percentage points. `device_id` is the `app:`-prefixed id;
    /// returns False if it is not one.
    fn set_app_volume_relative(&self, device_id: &str, delta: i16) -> bool {
        self.inner.set_app_volume_relative(device_id, delta)
    }

    fn toggle_app_mute(&self, device_id: &str) -> bool {
        self.inner.toggle_app_mute(device_id)
    }

    /// True once focus tracking is live. False where no supported compositor was found, which is
    /// how the config UI knows to hide the focus-following option.
    fn is_focus_tracking_available(&self) -> bool {
        self.inner.is_focus_tracking_available()
    }

    /// Name of the app a focus-following action would act on right now, or None when the focused
    /// window's app is not playing anything.
    fn focused_app_name(&self) -> Option<String> {
        self.inner.focused_app_name()
    }

    fn set_source_volume_relative(&self, device_id: &str, mix_b: bool, delta: i8) -> bool {
        self.inner
            .set_source_volume_relative(device_id, mix_b, delta)
    }

    fn set_target_mix(&self, device_id: &str, mix_b: bool) -> bool {
        self.inner.set_target_mix(device_id, mix_b)
    }

    fn toggle_target_mix(&self, device_id: &str) -> bool {
        self.inner.toggle_target_mix(device_id)
    }

    fn toggle_source_volumes_linked(&self, device_id: &str) -> bool {
        self.inner.toggle_source_volumes_linked(device_id)
    }

    fn toggle_mute_profile(&self, config: &ActionConfig) -> Option<bool> {
        self.inner.toggle_mute_profile(&config.inner)
    }

    fn toggle_route(&self, source_id: &str, target_id: &str) -> bool {
        self.inner.toggle_route(source_id, target_id)
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

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<DeckWeaverCore>()?;
    m.add_class::<ActionConfig>()?;
    m.add_class::<ActionType>()?;
    m.add_class::<AppStream>()?;
    m.add_class::<Device>()?;
    m.add_class::<DeviceColor>()?;
    m.add_class::<DeviceType>()?;
    m.add_class::<HardwareDevice>()?;
    m.add_function(wrap_pyfunction!(action_dimensions, m)?)?;
    m.add("VERSION", VERSION)?;
    m.add("FOCUSED_DEVICE_ID", deckweaver_core::FOCUSED_DEVICE_ID)?;
    Ok(())
}
