use crate::action::{ActionConfig, ActionState, ActionType, CachedBaseRender, CachedIcon};
use crate::devices::{apply_patch_op, Device, DeviceType, HardwareDevice, Status};
use crate::render::{
    pixmap_to_rgba, ButtonRenderer, KnobRenderer, RenderParams, SliderRenderer, KNOB_ICON_SIZE,
};
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const RENDER_INTERVAL: Duration = Duration::from_micros(33333);
const DEFAULT_HOST: &str = "localhost";
const RECONNECT_DELAY: Duration = Duration::from_secs(5);
/// Per-device cap on how often a PipeWeaver meter reading is stored (30fps).
const METER_THROTTLE: Duration = Duration::from_millis(33);

/// The release version, stamped in by build.sh/release.sh (scripts/version.sh: the git tag). A plain
/// `cargo build` falls back to the crate version, a 0.0.0 placeholder: versions live in tags.
pub const VERSION: &str = match option_env!("DECKWEAVER_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};
pub const DEFAULT_PORT: u16 = 14565;

#[derive(Debug)]
enum Command {
    SetVolume {
        device_id: String,
        device_type: Option<DeviceType>,
        volume: u8,
    },
    SetVolumeRelative {
        device_id: String,
        device_type: Option<DeviceType>,
        delta: i8,
    },
    ToggleMute {
        device_id: String,
        device_type: Option<DeviceType>,
    },
    SetSourceVolumeRelative {
        device_id: String,
        mix_b: bool,
        delta: i8,
    },
    SetSourceMute {
        device_id: String,
        mix_b: bool,
        muted: bool,
    },
    SetSourceVolumesLinked {
        device_id: String,
        linked: bool,
    },
    SetTargetMute {
        device_id: String,
        muted: bool,
    },
    SetTargetMix {
        device_id: String,
        mix_b: bool,
    },
    AttachPhysicalNode {
        target_id: String,
        node_id: u32,
    },
    RemovePhysicalNode {
        target_id: String,
        index: usize,
    },
    SetRoute {
        source_id: String,
        target_id: String,
        enabled: bool,
    },
}

#[derive(Debug, Clone, Default)]
pub struct PendingUpdate {
    pub image: Option<Vec<u8>>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    image_hash: Option<u64>,
    pub label: Option<String>,
}

pub struct DeckWeaverCore {
    running: Arc<AtomicBool>,
    service_available: Arc<AtomicBool>,
    status: Arc<RwLock<Option<Status>>>,
    actions: Arc<RwLock<HashMap<String, ActionState>>>,
    meter_data: Arc<RwLock<HashMap<String, u8>>>,
    command_tx: Arc<RwLock<Option<tokio::sync::mpsc::Sender<Command>>>>,
    pending_updates: Arc<RwLock<HashMap<String, PendingUpdate>>>,
    /// Per-application volume, independent of PipeWeaver. App actions stay live whether or not
    /// the websocket ever connects.
    pulse: Arc<crate::pulse::PulseBackend>,
    /// Which window has focus, for actions that follow it rather than a fixed app.
    focus: Arc<crate::focus::FocusTracker>,
}

impl DeckWeaverCore {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            service_available: Arc::new(AtomicBool::new(false)),
            status: Arc::new(RwLock::new(None)),
            actions: Arc::new(RwLock::new(HashMap::new())),
            meter_data: Arc::new(RwLock::new(HashMap::new())),
            command_tx: Arc::new(RwLock::new(None)),
            pending_updates: Arc::new(RwLock::new(HashMap::new())),
            pulse: crate::pulse::PulseBackend::new(),
            focus: crate::focus::FocusTracker::new(),
        }
    }

    pub fn start(&mut self) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        self.start_websocket_thread();
        self.start_meter_thread();
        self.start_render_thread();
        tracing::info!("DeckWeaverCore started");
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self.service_available.store(false, Ordering::SeqCst);
        tracing::info!("DeckWeaverCore stopped");
    }

    pub fn register_action(&self, config: ActionConfig) {
        self.actions
            .write()
            .insert(config.action_id.clone(), ActionState::new(config));
    }

    pub fn unregister_action(&self, action_id: &str) {
        self.actions.write().remove(action_id);
    }

    pub fn update_action(&self, action_id: &str, config: ActionConfig) {
        if let Some(state) = self.actions.write().get_mut(action_id) {
            state.config = config;
            state.device = None;
            state.last_render_hash.store(u64::MAX, Ordering::Relaxed);
        }
    }

    pub fn get_pending_updates(&self) -> HashMap<String, PendingUpdate> {
        std::mem::take(&mut *self.pending_updates.write())
    }

    pub fn is_available(&self) -> bool {
        self.service_available.load(Ordering::Relaxed)
    }

    /// True once the sound server is reachable. Independent of [`Self::is_available`]: the app
    /// actions work with PipeWeaver stopped, and the PipeWeaver actions work with no sound server
    /// bound.
    pub fn is_pulse_available(&self) -> bool {
        self.pulse.is_available()
    }

    /// Applications currently playing audio, for the property inspector's picker.
    pub fn get_apps(&self) -> Vec<crate::pulse::AppStream> {
        self.pulse.apps()
    }

    /// Nudge an app's volume by `delta` percentage points. `device_id` is the `app:`-prefixed id
    /// stored on the action; a PipeWeaver id is ignored.
    ///
    /// The focus sentinel is resolved here rather than when the action was configured, so a press
    /// acts on whatever holds focus at that instant.
    pub fn set_app_volume_relative(&self, device_id: &str, delta: i16) -> bool {
        self.with_app_key(device_id, |key| self.pulse.adjust_volume(key, delta))
    }

    /// Set an app's volume to an absolute 0-100.
    pub fn set_app_volume(&self, device_id: &str, volume: u8) -> bool {
        self.with_app_key(device_id, |key| self.pulse.set_volume(key, volume))
    }

    pub fn set_app_mute(&self, device_id: &str, muted: bool) -> bool {
        self.with_app_key(device_id, |key| self.pulse.set_mute(key, muted))
    }

    pub fn toggle_app_mute(&self, device_id: &str) -> bool {
        self.with_app_key(device_id, |key| self.pulse.toggle_mute(key))
    }

    /// True once focus tracking is live. False on desktops with no supported compositor, which is
    /// how the hosts tell the user a focus-following action will not work there.
    pub fn is_focus_tracking_available(&self) -> bool {
        self.focus.is_active()
    }

    /// Name of the app a focus-following action would act on right now, for the config UI.
    pub fn focused_app_name(&self) -> Option<String> {
        self.pulse.focused_app(&self.focus).map(|app| app.name)
    }

    /// Whether this action is bound to an app stream rather than a PipeWeaver device.
    pub fn is_app_device(device_id: &str) -> bool {
        crate::pulse::app_key_from_device_id(device_id).is_some()
    }

    pub fn get_devices(&self) -> Vec<Device> {
        self.with_status(Status::get_all_devices)
            .unwrap_or_default()
    }

    pub fn get_sources(&self) -> Vec<Device> {
        self.with_status(Status::get_sources).unwrap_or_default()
    }

    pub fn get_targets(&self) -> Vec<Device> {
        self.with_status(Status::get_targets).unwrap_or_default()
    }

    pub fn get_physical_sources(&self) -> Vec<Device> {
        self.with_status(Status::get_physical_sources)
            .unwrap_or_default()
    }

    pub fn get_physical_targets(&self) -> Vec<Device> {
        self.with_status(Status::get_physical_targets)
            .unwrap_or_default()
    }

    pub fn get_device(&self, device_id: &str, device_type: Option<DeviceType>) -> Option<Device> {
        self.with_status(|s| s.get_device(device_id, device_type))
            .flatten()
    }

    pub fn get_target_sources(&self, target_id: &str) -> Vec<Device> {
        self.with_status(|s| s.get_target_sources(target_id))
            .unwrap_or_default()
    }

    pub fn get_output_hardware_devices(&self) -> Vec<HardwareDevice> {
        self.with_status(Status::get_output_hardware_devices)
            .unwrap_or_default()
    }

    pub fn get_input_hardware_devices(&self) -> Vec<HardwareDevice> {
        self.with_status(Status::get_input_hardware_devices)
            .unwrap_or_default()
    }

    pub fn get_hardware_device_name(&self, node_id: u32, is_input: bool) -> Option<String> {
        self.with_status(|s| s.get_hardware_device_name(node_id, is_input))
            .flatten()
    }

    /// Whether `source_id` is routed into `target_id`, or `None` before PipeWeaver has reported.
    pub fn is_routed(&self, source_id: &str, target_id: &str) -> Option<bool> {
        self.with_status(|status| status.is_routed(source_id, target_id))
    }

    pub fn set_route(&self, source_id: &str, target_id: &str, enabled: bool) -> bool {
        self.send_command(Command::SetRoute {
            source_id: source_id.to_string(),
            target_id: target_id.to_string(),
            enabled,
        })
    }

    /// Flip the route from `source_id` into `target_id`, based on what PipeWeaver currently
    /// reports so a change made in PipeWeaver's own UI is respected by the next press.
    pub fn toggle_route(&self, source_id: &str, target_id: &str) -> bool {
        self.is_routed(source_id, target_id)
            .is_some_and(|routed| self.set_route(source_id, target_id, !routed))
    }

    pub fn infer_device_type(&self, device_id: &str, prefer_target: bool) -> Option<DeviceType> {
        self.with_status(|status| status.infer_device_type(device_id, prefer_target))
            .flatten()
    }

    pub fn set_volume(&self, device_id: &str, volume: u8, device_type: Option<DeviceType>) -> bool {
        self.send_command(Command::SetVolume {
            device_id: device_id.to_string(),
            device_type,
            volume,
        })
    }

    pub fn set_volume_relative(
        &self,
        device_id: &str,
        delta: i8,
        device_type: Option<DeviceType>,
    ) -> bool {
        self.send_command(Command::SetVolumeRelative {
            device_id: device_id.to_string(),
            device_type,
            delta,
        })
    }

    pub fn toggle_mute(&self, device_id: &str, device_type: Option<DeviceType>) -> bool {
        self.send_command(Command::ToggleMute {
            device_id: device_id.to_string(),
            device_type,
        })
    }

    pub fn set_source_volume_relative(&self, device_id: &str, mix_b: bool, delta: i8) -> bool {
        self.send_command(Command::SetSourceVolumeRelative {
            device_id: device_id.to_string(),
            mix_b,
            delta,
        })
    }

    pub fn set_source_mute(&self, device_id: &str, mix_b: bool, muted: bool) -> bool {
        self.send_command(Command::SetSourceMute {
            device_id: device_id.to_string(),
            mix_b,
            muted,
        })
    }

    pub fn set_target_mute(&self, device_id: &str, muted: bool) -> bool {
        self.send_command(Command::SetTargetMute {
            device_id: device_id.to_string(),
            muted,
        })
    }

    pub fn set_target_mix(&self, device_id: &str, mix_b: bool) -> bool {
        self.send_command(Command::SetTargetMix {
            device_id: device_id.to_string(),
            mix_b,
        })
    }

    pub fn toggle_target_mute(&self, device_id: &str) -> bool {
        self.get_device(device_id, Some(DeviceType::Target))
            .is_some_and(|device| self.set_target_mute(device_id, !device.is_muted))
    }

    pub fn toggle_target_mix(&self, device_id: &str) -> bool {
        self.get_device(device_id, Some(DeviceType::Target))
            .and_then(|device| device.target_mix_b)
            .is_some_and(|mix_b| self.set_target_mix(device_id, !mix_b))
    }

    pub fn toggle_source_volumes_linked(&self, device_id: &str) -> bool {
        self.get_device(device_id, Some(DeviceType::Source))
            .and_then(|device| device.source_volumes_linked)
            .is_some_and(|linked| {
                self.send_command(Command::SetSourceVolumesLinked {
                    device_id: device_id.to_string(),
                    linked: !linked,
                })
            })
    }

    pub fn apply_mute_profile(&self, config: &ActionConfig) -> bool {
        let Some(device_id) = &config.device_id else {
            return false;
        };
        let Some(&muted) = config
            .mute_profile_data
            .get(config.mute_profile_index as usize)
        else {
            return false;
        };
        self.send_profile_mute(config, device_id, muted)
    }

    /// Flip the active mute profile.
    ///
    /// Works from what the server currently reports rather than the value stored in the action,
    /// so a mute made in PipeWeaver's own UI is undone by the next press instead of being
    /// re-applied. Returns the state that was requested so the host can persist it, or `None`
    /// when the device is not known.
    pub fn toggle_mute_profile(&self, config: &ActionConfig) -> Option<bool> {
        let device_id = config.device_id.as_deref()?;
        let is_app = Self::is_app_device(device_id);
        let device = if is_app {
            self.pulse.app_for(device_id, &self.focus)?.to_device()
        } else {
            self.get_device(device_id, config.device_type)?
        };
        let muted = !live_mute_state(config, &device);

        let sent = if is_app {
            self.set_app_mute(device_id, muted)
        } else {
            self.send_profile_mute(config, device_id, muted)
        };
        sent.then_some(muted)
    }

    pub fn switch_output_hardware_device(&self, target_id: &str, node_id: u32) -> bool {
        self.switch_physical_hardware_device(target_id, node_id, false)
    }

    pub fn switch_input_hardware_device(&self, source_id: &str, node_id: u32) -> bool {
        self.switch_physical_hardware_device(source_id, node_id, true)
    }

    pub fn switch_physical_hardware_device(
        &self,
        device_id: &str,
        node_id: u32,
        is_input: bool,
    ) -> bool {
        let Some(attached) = self.with_status(|status| {
            if is_input {
                status.get_attached_input_hardware_devices(device_id)
            } else {
                status.get_attached_output_hardware_devices(device_id)
            }
        }) else {
            return false;
        };
        let already_attached = attached
            .iter()
            .any(|device| device.node_id == Some(node_id));

        let mut success = true;

        if !already_attached {
            success &= self.send_command(Command::AttachPhysicalNode {
                target_id: device_id.to_string(),
                node_id,
            });
        }

        // Everything else attached to this device goes, unmatched descriptors included.
        let mut remove_indices: Vec<usize> = attached
            .iter()
            .filter(|device| device.node_id != Some(node_id))
            .filter_map(|device| device.attachment_index)
            .collect();
        remove_indices.sort_unstable_by(|a, b| b.cmp(a));

        for index in remove_indices {
            success &= self.send_command(Command::RemovePhysicalNode {
                target_id: device_id.to_string(),
                index,
            });
        }

        success
    }

    pub fn get_action_device_name(&self, action_id: &str) -> Option<String> {
        self.actions
            .read()
            .get(action_id)
            .and_then(|s| s.device.as_ref())
            .map(|d| d.name.clone())
    }
}

impl Default for DeckWeaverCore {
    fn default() -> Self {
        Self::new()
    }
}

impl DeckWeaverCore {
    fn with_status<T>(&self, f: impl FnOnce(&Status) -> T) -> Option<T> {
        self.status.read().as_ref().map(f)
    }

    /// Resolve an app action's device id (following focus if it is the focus sentinel) and run
    /// `f` on the stream key. False when no app currently matches.
    fn with_app_key(&self, device_id: &str, f: impl FnOnce(&str)) -> bool {
        let Some(key) = self.pulse.resolve_key(device_id, &self.focus) else {
            return false;
        };
        f(&key);
        true
    }

    /// Set the mute of the action's active profile: a source's "Mute 1" / "Mute 2" by
    /// `mute_profile_index`, a target's single mute.
    fn send_profile_mute(&self, config: &ActionConfig, device_id: &str, muted: bool) -> bool {
        match config.device_type {
            Some(DeviceType::Source) => {
                self.set_source_mute(device_id, config.mute_profile_index == 1, muted)
            }
            Some(DeviceType::Target) => self.set_target_mute(device_id, muted),
            None => false,
        }
    }

    /// Send command to WebSocket thread. Never blocks: uses try_send so button presses
    /// cannot stall the UI if the WebSocket is slow or reconnecting.
    fn send_command(&self, cmd: Command) -> bool {
        self.command_tx
            .read()
            .as_ref()
            .is_some_and(|tx| tx.try_send(cmd).is_ok())
    }

    fn start_websocket_thread(&self) {
        let running = self.running.clone();
        let service_available = self.service_available.clone();
        let status = self.status.clone();
        let command_tx_holder = self.command_tx.clone();

        std::thread::Builder::new()
            .name("deckweaver-ws".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create tokio runtime");

                rt.block_on(async move {
                    let url = format!("ws://{}:{}/api/websocket", DEFAULT_HOST, DEFAULT_PORT);
                    let command_id = AtomicU64::new(0);

                    while running.load(Ordering::SeqCst) {
                        let ws = match connect_async(&url).await {
                            Ok((ws, _)) => ws,
                            Err(e) => {
                                tracing::warn!("WebSocket connection failed: {}", e);
                                service_available.store(false, Ordering::SeqCst);
                                *command_tx_holder.write() = None;
                                tokio::time::sleep(RECONNECT_DELAY).await;
                                continue;
                            }
                        };

                        tracing::info!("Connected to PipeWeaver");
                        service_available.store(true, Ordering::SeqCst);
                        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(32);
                        *command_tx_holder.write() = Some(cmd_tx);
                        let (mut write, mut read) = ws.split();

                        let initial_id = command_id.fetch_add(1, Ordering::SeqCst);
                        let initial_request = serde_json::json!({
                            "id": initial_id,
                            "data": "GetStatus"
                        });
                        if let Ok(json) = serde_json::to_string(&initial_request)
                            && write.send(Message::Text(json.into())).await.is_err()
                        {
                            break;
                        }

                        loop {
                            tokio::select! {
                                msg = read.next() => {
                                    match msg {
                                        Some(Ok(Message::Text(text))) => {
                                            handle_ws_message(&text, &status);
                                        }
                                        Some(Err(e)) => {
                                            tracing::warn!("WebSocket error: {}", e);
                                            break;
                                        }
                                        None => break,
                                        _ => {}
                                    }
                                }
                                cmd = cmd_rx.recv() => {
                                    let Some(cmd) = cmd else { continue };
                                    let id = command_id.fetch_add(1, Ordering::SeqCst);
                                    let Some(json) = build_command(&status, id, cmd) else { continue };
                                    if write.send(Message::Text(json.into())).await.is_err() {
                                        break;
                                    }
                                }
                            }

                            if !running.load(Ordering::SeqCst) {
                                break;
                            }
                        }

                        *command_tx_holder.write() = None;
                        service_available.store(false, Ordering::SeqCst);
                        *status.write() = None;

                        if running.load(Ordering::SeqCst) {
                            tokio::time::sleep(RECONNECT_DELAY).await;
                        }
                    }

                    *command_tx_holder.write() = None;
                    service_available.store(false, Ordering::SeqCst);
                });
            })
            .expect("Failed to spawn websocket thread");
    }

    fn start_meter_thread(&self) {
        let running = self.running.clone();
        let meter_data = self.meter_data.clone();
        let actions = self.actions.clone();

        std::thread::Builder::new()
            .name("deckweaver-meter".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create tokio runtime");

                rt.block_on(async move {
                    let url = format!("ws://{}:{}/api/websocket/meter", DEFAULT_HOST, DEFAULT_PORT);
                    let mut last_update: HashMap<String, Instant> = HashMap::new();

                    while running.load(Ordering::SeqCst) {
                        if !any_meters_enabled(&actions) {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                        let ws = match connect_async(&url).await {
                            Ok((ws, _)) => ws,
                            Err(_) => {
                                tokio::time::sleep(RECONNECT_DELAY).await;
                                continue;
                            }
                        };

                        let (_, mut read) = ws.split();
                        last_update.clear();

                        while any_meters_enabled(&actions) {
                            tokio::select! {
                                msg = read.next() => {
                                    match msg {
                                        Some(Ok(Message::Text(text))) => {
                                            handle_meter_message(&text, &actions, &meter_data, &mut last_update);
                                        }
                                        Some(Err(_)) | None => break,
                                        _ => {}
                                    }
                                }
                                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                                    if !running.load(Ordering::SeqCst) {
                                        break;
                                    }
                                }
                            }
                        }

                        if running.load(Ordering::SeqCst) {
                            tokio::time::sleep(RECONNECT_DELAY).await;
                        }
                    }
                });
            })
            .expect("Failed to spawn meter thread");
    }

    fn start_render_thread(&self) {
        let running = self.running.clone();
        let service_available = self.service_available.clone();
        let status = self.status.clone();
        let actions = self.actions.clone();
        let meter_data = self.meter_data.clone();
        let pending_updates = self.pending_updates.clone();
        let pulse = self.pulse.clone();
        let focus = self.focus.clone();

        std::thread::Builder::new()
            .name("deckweaver-render".into())
            .spawn(move || {
                let mut renderers = Renderers::default();
                // Tracked as a pair because the two backends come and go independently, and a
                // change in either has to force a re-render of the actions that depend on it.
                let mut last_available = (false, false);

                while running.load(Ordering::SeqCst) {
                    let frame_start = Instant::now();
                    let pipeweaver_available = service_available.load(Ordering::Relaxed);
                    let pulse_available = pulse.is_available();
                    let availability_changed =
                        (pipeweaver_available, pulse_available) != last_available;
                    last_available = (pipeweaver_available, pulse_available);

                    let action_ids: Vec<String> = {
                        let guard = actions.read();
                        // Peak monitors only run while an app action is actually showing meters.
                        let wants_app_meters = guard.values().any(|s| {
                            s.config.meters_enabled
                                && s.config
                                    .device_id
                                    .as_deref()
                                    .is_some_and(DeckWeaverCore::is_app_device)
                        });
                        pulse.set_metering(wants_app_meters);
                        guard.keys().cloned().collect()
                    };

                    let mut label_updates = Vec::new();
                    {
                        let status_guard = status.read();
                        let mut actions_guard = actions.write();

                        for action_id in action_ids {
                            let Some(state) = actions_guard.get_mut(&action_id) else {
                                continue;
                            };
                            if let Some(label) = refresh_action(
                                state,
                                status_guard.as_ref(),
                                &meter_data,
                                &pulse,
                                &focus,
                            ) {
                                label_updates.push((action_id, label));
                            }
                        }
                    }
                    queue_labels(&pending_updates, label_updates);

                    let tasks: Vec<RenderTask> = {
                        let guard = actions.read();
                        guard
                            .iter()
                            .filter(|(_, s)| {
                                if availability_changed {
                                    s.last_render_hash.store(u64::MAX, Ordering::Relaxed);
                                }
                                s.needs_render()
                            })
                            .map(|(id, s)| {
                                RenderTask::snapshot(id, s, pipeweaver_available, pulse_available)
                            })
                            .collect()
                    };

                    for task in tasks {
                        if let Some(frame) = render_task(&mut renderers, &actions, &status, &task) {
                            queue_image(&pending_updates, task.action_id, frame);
                        }
                    }

                    let elapsed = frame_start.elapsed();
                    if elapsed < RENDER_INTERVAL {
                        std::thread::sleep(RENDER_INTERVAL - elapsed);
                    }
                }
            })
            .expect("Failed to spawn render thread");
    }
}

impl Drop for DeckWeaverCore {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Raw RGBA bytes plus their width and height, as every renderer returns them.
type Frame = (Vec<u8>, u32, u32);

fn any_meters_enabled(actions: &RwLock<HashMap<String, ActionState>>) -> bool {
    actions.read().values().any(|s| s.config.meters_enabled)
}

/// Store one PipeWeaver meter reading, if an action is metering that device, at most once per
/// `METER_THROTTLE` per device.
fn handle_meter_message(
    text: &str,
    actions: &RwLock<HashMap<String, ActionState>>,
    meter_data: &RwLock<HashMap<String, u8>>,
    last_update: &mut HashMap<String, Instant>,
) {
    let data = match serde_json::from_str::<Value>(text) {
        Ok(data) => data,
        Err(e) => {
            tracing::warn!(
                "Failed to parse meter message as JSON: {} (message: {})",
                e,
                text.chars().take(200).collect::<String>()
            );
            return;
        }
    };
    let (Some(id), Some(percent)) = (
        data.get("id").and_then(|v| v.as_str()),
        data.get("percent").and_then(|v| v.as_u64()),
    ) else {
        return;
    };

    let device_needs_meters = actions
        .read()
        .values()
        .any(|s| s.config.meters_enabled && s.config.device_id.as_deref() == Some(id));
    if !device_needs_meters {
        return;
    }

    let now = Instant::now();
    let should_update = last_update
        .get(id)
        .is_none_or(|t| now.duration_since(*t) >= METER_THROTTLE);
    if should_update {
        last_update.insert(id.to_string(), now);
        meter_data.write().insert(id.to_string(), percent as u8);
    }
}

/// Bring one action's device and meter up to date with the live backends, returning its label
/// when that has changed since the last frame.
fn refresh_action(
    state: &mut ActionState,
    status: Option<&Status>,
    meter_data: &RwLock<HashMap<String, u8>>,
    pulse: &crate::pulse::PulseBackend,
    focus: &crate::focus::FocusTracker,
) -> Option<String> {
    let device_id = state.config.device_id.as_deref()?;
    let is_app = DeckWeaverCore::is_app_device(device_id);

    // App actions resolve against the sound server instead of the PipeWeaver status, keyed off
    // the `app:` prefix in the device id.
    state.device = if is_app {
        let app = pulse.app_for(device_id, focus);
        // Routing and icon are properties of whichever app is resolved right now, which changes
        // under a focus-following action, so they are refreshed every frame rather than at
        // configure time.
        state.config.routed_to = app.as_ref().and_then(|a| a.routed_to.clone());
        state.config.app_icon_path = app.as_ref().and_then(|a| {
            crate::icon_loader::find_icon_for_app(
                a.icon_name.as_deref(),
                &a.name,
                Some(&a.key),
                a.steam_app_id.as_deref(),
            )
        });
        app.map(|a| a.to_device())
    } else {
        let config = &state.config;
        status.and_then(|st| {
            let device = st.get_device(device_id, config.device_type).or_else(|| {
                if config.action_type == ActionType::Slider {
                    st.get_device(device_id, None)
                } else {
                    None
                }
            })?;
            let mut device = device.with_selected_source_mix(config.source_mix_b);
            // A route toggle reads "muted" while the route is off.
            if let Some(target_id) = config.route_target_id.as_deref() {
                device.is_muted = !st.is_routed(&device.id, target_id);
            }
            Some(device)
        })
    };

    // PipeWeaver devices take their level from the PipeWeaver meter feed; app streams take
    // theirs from the PulseAudio peak monitors, resolved through the same app the key is
    // currently controlling.
    if !state.config.meters_enabled {
        state.set_meter(0);
    } else if is_app {
        let level = state
            .device
            .as_ref()
            .and_then(|d| crate::pulse::app_key_from_device_id(&d.id))
            .map(|key| pulse.peak(key))
            .unwrap_or(0);
        state.set_meter(level);
    } else if let Some(&meter) = meter_data.read().get(device_id) {
        state.set_meter(meter);
    }

    // A route toggle's label names both ends so the key says what it connects. Only the label:
    // the device keeps its own name, which is also what the host shows as the selected source.
    let device = state.device.as_ref()?;
    let label = status
        .zip(state.config.route_target_id.as_deref())
        .and_then(|(st, target_id)| st.get_device(target_id, Some(DeviceType::Target)))
        .map(|target| format!("{}\n→ {}", device.name, target.name))
        .unwrap_or_else(|| device.name.clone());
    state.label_changed(Some(&label)).then_some(label)
}

fn queue_labels(
    pending_updates: &RwLock<HashMap<String, PendingUpdate>>,
    label_updates: Vec<(String, String)>,
) {
    if label_updates.is_empty() {
        return;
    }
    let mut updates = pending_updates.write();
    for (action_id, label) in label_updates {
        if updates.get(&action_id).and_then(|u| u.label.as_ref()) != Some(&label) {
            updates.entry(action_id).or_default().label = Some(label);
        }
    }
}

fn queue_image(
    pending_updates: &RwLock<HashMap<String, PendingUpdate>>,
    action_id: String,
    (bytes, width, height): Frame,
) {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    let image_hash = hasher.finish();

    let mut updates = pending_updates.write();
    if updates.get(&action_id).and_then(|u| u.image_hash) != Some(image_hash) {
        let update = updates.entry(action_id).or_default();
        update.image = Some(bytes);
        update.width = Some(width);
        update.height = Some(height);
        update.image_hash = Some(image_hash);
    }
}

/// Everything one action needs to draw a frame, snapshotted under the actions lock so the
/// drawing itself runs without holding it.
struct RenderTask {
    action_id: String,
    config: ActionConfig,
    device: Option<Device>,
    meter: u8,
    cached_icon: Option<CachedIcon>,
    cached_knob_base: Option<CachedBaseRender>,
    /// A metering knob whose cached base is stale and has to be redrawn this frame.
    needs_knob_base_rebuild: bool,
    is_app_action: bool,
    available: bool,
}

impl RenderTask {
    fn snapshot(
        action_id: &str,
        s: &ActionState,
        pipeweaver_available: bool,
        pulse_available: bool,
    ) -> Self {
        let max_icon_size = match s.config.action_type {
            ActionType::Knob => KNOB_ICON_SIZE,
            _ => (s.config.width as f32) * 0.5,
        };
        // Explicit user choice first, the app's own icon only as a fallback, so setting a custom
        // icon always overrides it.
        let icon_path = s
            .config
            .icon_path
            .as_deref()
            .or(s.config.app_icon_path.as_deref());
        let cached_icon = s.get_cached_icon(s.config.icon_png.as_deref(), icon_path, max_icon_size);

        let uses_knob_meter_cache =
            s.config.action_type == ActionType::Knob && s.config.meters_enabled;
        let needs_knob_base_rebuild = uses_knob_meter_cache && s.needs_base_rebuild();
        let cached_knob_base = if uses_knob_meter_cache && !needs_knob_base_rebuild {
            s.cached_base.read().clone()
        } else {
            None
        };

        // Each action is gated on the backend it actually talks to, so a dead PipeWeaver
        // connection no longer blanks the app actions (and vice versa).
        let is_app_action = s
            .config
            .device_id
            .as_deref()
            .is_some_and(DeckWeaverCore::is_app_device);
        let available = if is_app_action {
            pulse_available
        } else {
            pipeweaver_available
        };

        // Tint the bar with the app's own icon, so a key reads as that app at a glance. Only when
        // nothing more specific applies: an explicit volume_bar_color still wins in
        // `accent_color`, and a PipeWeaver device keeps whatever colour PipeWeaver assigned it.
        let mut device = s.device.clone();
        if is_app_action
            && let Some(dev) = device.as_mut()
            && dev.color.is_none()
            && let Some((red, green, blue)) = cached_icon.as_ref().and_then(|icon| icon.accent)
        {
            dev.color = Some(crate::devices::DeviceColor { red, green, blue });
        }

        Self {
            action_id: action_id.to_string(),
            config: s.config.clone(),
            device,
            meter: s.get_meter(),
            cached_icon,
            cached_knob_base,
            needs_knob_base_rebuild,
            is_app_action,
            available,
        }
    }
}

/// Redraw a metering knob's base (everything but the meter) and store it on the action.
fn rebuild_knob_base(
    renderers: &mut Renderers,
    actions: &RwLock<HashMap<String, ActionState>>,
    task: &RenderTask,
) -> Option<CachedBaseRender> {
    let device = task.device.as_ref()?;
    let config = &task.config;
    let params = knob_render_params(config, device, 0);
    let pixmap = renderers.knob(config.width, config.height).render_base(
        &params,
        config.icon_png.clone(),
        task.cached_icon.as_ref(),
    )?;

    let guard = actions.read();
    let state = guard.get(&task.action_id)?;
    let cached = CachedBaseRender {
        pixmap,
        base_hash: state.base_hash(),
    };
    *state.cached_base.write() = Some(cached.clone());
    Some(cached)
}

fn render_task(
    renderers: &mut Renderers,
    actions: &RwLock<HashMap<String, ActionState>>,
    status: &RwLock<Option<Status>>,
    task: &RenderTask,
) -> Option<Frame> {
    let config = &task.config;
    let rebuilt_base;
    let cached_knob_base = if task.needs_knob_base_rebuild {
        rebuilt_base = rebuild_knob_base(renderers, actions, task);
        rebuilt_base.as_ref()
    } else {
        task.cached_knob_base.as_ref()
    };

    if !task.available {
        if task.is_app_action {
            // No sound server is a real fault, but the app actions should not suddenly sprout a
            // red cross across the deck for it either.
            renderers.render_idle(config, "No audio server")
        } else {
            renderers.render_unavailable(config)
        }
    } else if let Some(ref dev) = task.device {
        renderers.render_with_cached(
            config,
            dev,
            task.meter,
            task.cached_icon.as_ref(),
            cached_knob_base,
        )
    } else if config.device_id.is_some()
        // An app that is not currently playing has no stream to control, and the sound server is
        // already known good here, so this is a settled "nothing to show" rather than a state
        // still being fetched.
        && (task.is_app_action || status.read().is_some())
    {
        if task.is_app_action {
            // Nothing playing is the resting state of a focus-following knob, not a fault, so
            // it says so rather than showing an error.
            renderers.render_idle(config, "No sound playing")
        } else {
            renderers.render_unavailable(config)
        }
    } else {
        renderers.render_loading(config)
    }
}

fn handle_ws_message(text: &str, status: &RwLock<Option<Status>>) {
    let Ok(response) = serde_json::from_str::<Value>(text) else {
        let preview: String = text.chars().take(200).collect();
        tracing::warn!(
            "Failed to parse WebSocket message as JSON (message: {})",
            preview
        );
        return;
    };

    if let Some(status_data) = response.get("data").and_then(|d| d.get("Status")) {
        if let Ok(new_status) = serde_json::from_value::<Status>(status_data.clone()) {
            new_status.rebuild_index();
            *status.write() = Some(new_status);
        } else {
            tracing::warn!("Failed to deserialize Status");
        }
    }

    if let Some(patch) = response
        .get("data")
        .and_then(|d| d.get("Patch"))
        .and_then(|p| p.as_array())
    {
        apply_patch(status, patch);
    }
}

fn apply_patch(status: &RwLock<Option<Status>>, patch: &[Value]) {
    let mut guard = status.write();
    let Some(current) = guard.as_mut() else {
        tracing::warn!("Cannot apply patch: status is None");
        return;
    };

    let Ok(mut value) = serde_json::to_value(&*current) else {
        tracing::error!("Failed to serialize status for patch");
        return;
    };

    for op in patch {
        if let Err(e) = apply_patch_op(&mut value, op) {
            tracing::warn!("Failed to apply patch operation: {}", e);
        }
    }

    if let Ok(new_status) = serde_json::from_value::<Status>(value) {
        new_status.rebuild_index();
        *current = new_status;
    } else {
        tracing::error!("Failed to deserialize status after patch");
    }
}

fn device_is_source(config: &ActionConfig, device: &Device) -> bool {
    match config.device_type {
        Some(DeviceType::Source) => true,
        Some(DeviceType::Target) => false,
        None => device.device_type == DeviceType::Source,
    }
}

fn device_color(device: &Device) -> Option<(u8, u8, u8)> {
    device
        .color
        .as_ref()
        .map(|color| (color.red, color.green, color.blue))
}

/// What the server currently says about the active mute profile.
///
/// A PipeWeaver source has two mute profiles (its "Mute 1" / "Mute 2" buttons, `TargetA` and
/// `TargetB` on the wire), each muting whichever targets it is configured for; the `mix_a` /
/// `mix_b` field names on [`Device`] are historical and refer to those, not to the volume mixes.
/// A target has a single mute, and so does an app stream (which presents as a source without
/// per-profile flags), so every profile reads the same flag there.
fn live_mute_state(config: &ActionConfig, device: &Device) -> bool {
    device
        .source_muted_for_mix(config.mute_profile_index == 1)
        .unwrap_or(device.is_muted)
}

fn knob_render_params(config: &ActionConfig, device: &Device, meter_value: u8) -> RenderParams {
    let is_source = device_is_source(config, device);

    RenderParams {
        name: device.name.clone(),
        volume: device.volume,
        is_muted: device.is_muted,
        is_source,
        meter_value,
        device_color: device_color(device),
        volume_bar_color: config.volume_bar_color,
        meter_color: config.meter_color,
        meter_invert: config.meter_invert,
        meters_enabled: config.meters_enabled,
        mix_b_active: if is_source {
            config.source_mix_b
        } else {
            device.target_mix_b.unwrap_or(false)
        },
        source_volumes_linked: device.source_volumes_linked.unwrap_or(false),
        routed_to: config.routed_to.clone(),
        mute_profile: config.mute_profile_index,
        // The stored profile value is only what was last set from this key. Muting in PipeWeaver's
        // own UI (or pavucontrol, for an app) leaves it stale, so the dial dims off what the server
        // currently reports instead.
        mute_profile_muted: live_mute_state(config, device),
        show_volume: config.show_volume,
    }
}

fn slider_render_params(config: &ActionConfig, device: &Device, meter_value: u8) -> RenderParams {
    RenderParams {
        name: device.name.clone(),
        volume: device.volume,
        is_muted: false,
        is_source: device_is_source(config, device),
        meter_value,
        device_color: device_color(device),
        volume_bar_color: config.volume_bar_color,
        meter_color: config.meter_color,
        meter_invert: config.meter_invert,
        meters_enabled: config.meters_enabled,
        mix_b_active: false,
        source_volumes_linked: false,
        // Slider halves have no status row to put it in.
        routed_to: None,
        mute_profile: 0,
        mute_profile_muted: false,
        show_volume: false,
    }
}

/// The device a plain volume/mute command addresses: the configured type first, then whatever
/// device carries that id.
fn lookup_device(
    status: &RwLock<Option<Status>>,
    device_id: &str,
    device_type: Option<DeviceType>,
) -> Option<Device> {
    let guard = status.read();
    let status = guard.as_ref()?;
    status
        .get_device(device_id, device_type)
        .or_else(|| status.get_device(device_id, None))
}

/// The device of exactly `device_type`, for commands that only exist for one of the two.
fn typed_device(
    status: &RwLock<Option<Status>>,
    device_id: &str,
    device_type: DeviceType,
) -> Option<Device> {
    status
        .read()
        .as_ref()?
        .get_device(device_id, Some(device_type))
}

fn nudge_volume(volume: u8, delta: i8) -> u8 {
    (volume as i16 + delta as i16).clamp(0, 100) as u8
}

/// Mix A volume for a source, the only volume for a target.
fn set_volume_json(device_type: DeviceType, device_id: String, volume: u8) -> Value {
    match device_type {
        DeviceType::Source => {
            serde_json::json!({"Pipewire": {"SetSourceVolume": [device_id, "A", volume]}})
        }
        DeviceType::Target => {
            serde_json::json!({"Pipewire": {"SetTargetVolume": [device_id, volume]}})
        }
    }
}

fn build_command(status: &RwLock<Option<Status>>, id: u64, cmd: Command) -> Option<String> {
    let data = match cmd {
        Command::SetVolume {
            device_id,
            device_type,
            volume,
        } => {
            let device = lookup_device(status, &device_id, device_type)?;
            set_volume_json(device.device_type, device_id, volume)
        }
        Command::SetVolumeRelative {
            device_id,
            device_type,
            delta,
        } => {
            let device = lookup_device(status, &device_id, device_type)?;
            set_volume_json(
                device.device_type,
                device_id,
                nudge_volume(device.volume, delta),
            )
        }
        Command::ToggleMute {
            device_id,
            device_type,
        } => {
            let device = lookup_device(status, &device_id, device_type)?;
            match device.device_type {
                DeviceType::Source => {
                    if device.is_muted {
                        serde_json::json!({"Pipewire": {"DelSourceMuteTarget": [device_id, "TargetA"]}})
                    } else {
                        serde_json::json!({"Pipewire": {"AddSourceMuteTarget": [device_id, "TargetA"]}})
                    }
                }
                DeviceType::Target => {
                    let state = if device.is_muted { "Unmuted" } else { "Muted" };
                    serde_json::json!({"Pipewire": {"SetTargetMuteState": [device_id, state]}})
                }
            }
        }
        Command::SetSourceVolumeRelative {
            device_id,
            mix_b,
            delta,
        } => {
            let device = typed_device(status, &device_id, DeviceType::Source)?;
            let new_volume = nudge_volume(device.source_volume_for_mix(mix_b)?, delta);
            let mix = if mix_b { "B" } else { "A" };
            serde_json::json!({"Pipewire": {"SetSourceVolume": [device_id, mix, new_volume]}})
        }
        Command::SetSourceMute {
            device_id,
            mix_b,
            muted,
        } => {
            typed_device(status, &device_id, DeviceType::Source)?;
            let target = if mix_b { "TargetB" } else { "TargetA" };
            if muted {
                serde_json::json!({"Pipewire": {"AddSourceMuteTarget": [device_id, target]}})
            } else {
                serde_json::json!({"Pipewire": {"DelSourceMuteTarget": [device_id, target]}})
            }
        }
        Command::SetSourceVolumesLinked { device_id, linked } => {
            typed_device(status, &device_id, DeviceType::Source)?;
            serde_json::json!({"Pipewire": {"SetSourceVolumeLinked": [device_id, linked]}})
        }
        Command::SetTargetMute { device_id, muted } => {
            typed_device(status, &device_id, DeviceType::Target)?;
            let state = if muted { "Muted" } else { "Unmuted" };
            serde_json::json!({"Pipewire": {"SetTargetMuteState": [device_id, state]}})
        }
        Command::SetTargetMix { device_id, mix_b } => {
            typed_device(status, &device_id, DeviceType::Target)?;
            let mix = if mix_b { "B" } else { "A" };
            serde_json::json!({"Pipewire": {"SetTargetMix": [device_id, mix]}})
        }
        Command::AttachPhysicalNode { target_id, node_id } => {
            serde_json::json!({"Pipewire": {"AttachPhysicalNode": [target_id, node_id]}})
        }
        Command::RemovePhysicalNode { target_id, index } => {
            serde_json::json!({"Pipewire": {"RemovePhysicalNode": [target_id, index]}})
        }
        Command::SetRoute {
            source_id,
            target_id,
            enabled,
        } => {
            serde_json::json!({"Pipewire": {"SetRoute": [source_id, target_id, enabled]}})
        }
    };

    serde_json::to_string(&serde_json::json!({ "id": id, "data": data })).ok()
}

/// One renderer per size, created on first use.
#[derive(Default)]
struct Renderers {
    knobs: HashMap<(u32, u32), KnobRenderer>,
    sliders: HashMap<u32, SliderRenderer>,
    buttons: HashMap<u32, ButtonRenderer>,
}

/// Run `$body` with `$r` bound to the renderer for `$config`'s action type and size.
macro_rules! with_renderer {
    ($renderers:expr, $config:expr, |$r:ident| $body:expr) => {
        match $config.action_type {
            ActionType::Knob => {
                let $r = $renderers.knob($config.width, $config.height);
                $body
            }
            ActionType::Slider => {
                let $r = $renderers.slider($config.width);
                $body
            }
            ActionType::Button => {
                let $r = $renderers.button($config.width);
                $body
            }
        }
    };
}

impl Renderers {
    fn knob(&mut self, width: u32, height: u32) -> &mut KnobRenderer {
        self.knobs
            .entry((width, height))
            .or_insert_with(|| KnobRenderer::new(width, height))
    }

    fn slider(&mut self, width: u32) -> &mut SliderRenderer {
        self.sliders
            .entry(width)
            .or_insert_with(|| SliderRenderer::new(width))
    }

    fn button(&mut self, width: u32) -> &mut ButtonRenderer {
        self.buttons
            .entry(width)
            .or_insert_with(|| ButtonRenderer::new(width))
    }

    /// Dimmed "nothing to control" state, used for app actions in place of the fault cross.
    fn render_idle(&mut self, config: &ActionConfig, message: &str) -> Option<Frame> {
        with_renderer!(self, config, |r| r.render_idle_internal(message))
    }

    fn render_unavailable(&mut self, config: &ActionConfig) -> Option<Frame> {
        with_renderer!(self, config, |r| r.render_unavailable_internal())
    }

    fn render_loading(&mut self, config: &ActionConfig) -> Option<Frame> {
        with_renderer!(self, config, |r| r.render_loading_internal())
    }

    fn render_with_cached(
        &mut self,
        config: &ActionConfig,
        device: &Device,
        meter_value: u8,
        cached_icon: Option<&CachedIcon>,
        cached_knob_base: Option<&CachedBaseRender>,
    ) -> Option<Frame> {
        match config.action_type {
            ActionType::Knob => {
                let params = knob_render_params(config, device, meter_value);
                let knob = self.knob(config.width, config.height);
                if let Some(cached_base) = cached_knob_base {
                    let mut pixmap = cached_base.pixmap.clone();
                    knob.render_meter_overlay(&mut pixmap, &params);
                    pixmap_to_rgba(&pixmap)
                } else if let Some(cached) = cached_icon {
                    knob.render_internal_png_with_cached(&params, Some(cached))
                } else {
                    knob.render_internal_png(&params, config.icon_png.clone())
                }
            }
            ActionType::Slider => {
                let params = slider_render_params(config, device, meter_value);
                self.slider(config.width).render_internal_png(
                    &params,
                    config.is_top,
                    config.orientation == "horizontal",
                )
            }
            ActionType::Button => {
                // A zero step is a mute toggle rather than a volume key, so it gets no +/- symbol.
                let is_plus = (config.volume_step != 0).then_some(config.volume_step > 0);
                let button = self.button(config.width);
                if let Some(cached) = cached_icon {
                    button.render_internal_png_with_cached(
                        is_plus,
                        Some(cached),
                        device.is_muted,
                        config.button_overlay,
                    )
                } else {
                    button.render_internal_png(
                        is_plus,
                        config.icon_png.clone(),
                        device.is_muted,
                        config.button_overlay,
                    )
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_route_serializes_to_the_pipeweaver_command() {
        let status = Arc::new(RwLock::new(None));
        let message = build_command(
            &status,
            7,
            Command::SetRoute {
                source_id: "mic".into(),
                target_id: "phones".into(),
                enabled: true,
            },
        )
        .expect("SetRoute needs no status");
        let value: Value = serde_json::from_str(&message).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"id": 7, "data": {"Pipewire": {"SetRoute": ["mic", "phones", true]}}})
        );
    }
}
