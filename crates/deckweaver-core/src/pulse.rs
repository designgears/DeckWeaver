//! Per-application volume control over the PulseAudio API.
//!
//! This is deliberately independent of the PipeWeaver websocket: the app actions have to keep
//! working on machines that never run PipeWeaver at all. PulseAudio is the widest target — real
//! PulseAudio speaks it, and PipeWire speaks it through pipewire-pulse — so one backend covers
//! both without a compile-time choice.
//!
//! libpulse's context is a bag of raw pointers and is neither `Send` nor `Sync`, so it is pinned
//! to a single worker thread. Everything the rest of the crate touches goes through the snapshot
//! ([`AppStream`] values behind an `RwLock`) or the command channel.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::{Context, FlagSet as ContextFlagSet, State};
use libpulse_binding::mainloop::standard::{IterateResult, Mainloop};
use libpulse_binding::volume::{ChannelVolumes, Volume};
use parking_lot::RwLock;

use crate::devices::{Device, DeviceType};

/// Device ids for app streams carry this prefix so the render pipeline can tell them apart from
/// PipeWeaver device ids without a second field threaded through every action config.
pub const APP_DEVICE_PREFIX: &str = "app:";

/// Sentinel key meaning "whichever app owns the focused window" rather than a fixed app.
pub const FOCUSED_APP_KEY: &str = "@focused";

/// Device id an action stores to follow the focused window.
pub const FOCUSED_DEVICE_ID: &str = "app:@focused";

/// True when this device id follows focus rather than naming a fixed app.
pub fn is_focused_device_id(device_id: &str) -> bool {
    app_key_from_device_id(device_id) == Some(FOCUSED_APP_KEY)
}

/// How often the worker re-reads the sink input list. Fast enough that volume changes made
/// elsewhere (pavucontrol, media keys) show up promptly, cheap enough to ignore — the list is a
/// handful of entries and the query is local IPC.
const POLL_INTERVAL: Duration = Duration::from_millis(150);

/// Mainloop iteration cadence. Keeps command latency low without spinning a core.
const TICK: Duration = Duration::from_millis(10);

/// Retry cadence when no sound server is reachable.
const RECONNECT_INTERVAL: Duration = Duration::from_secs(2);

/// Sinks are created and renamed far less often than streams appear, so they get a slower poll.
const SINK_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// How long to keep trusting a local change the server has not echoed back yet. Only an escape
/// hatch: a change normally settles on the first poll after it lands. Without a cap, a value the
/// server refuses to accept would stick on screen forever.
const PENDING_TIMEOUT: Duration = Duration::from_secs(1);

/// One application's audio, aggregated across every stream it owns.
///
/// A browser typically opens a stream per tab; treating them separately would give you a key that
/// controls whichever tab happened to sort first. Grouping by [`AppStream::key`] means one action
/// controls the app as a whole.
#[derive(Debug, Clone)]
pub struct AppStream {
    /// Stable identity across restarts — the process binary where available, else the app name.
    /// This is what an action stores, since sink input indices are recycled constantly.
    pub key: String,
    /// Display name, e.g. "Microsoft Edge".
    pub name: String,
    /// Every live sink input belonging to this app.
    pub indices: Vec<u32>,
    /// Process ids behind those streams. Used to match an app to the focused window, which
    /// reports the pid of the process that owns the window.
    pub pids: Vec<u32>,
    /// Channel count of the first stream, needed to build a `ChannelVolumes` when setting volume.
    channels: u8,
    /// 0-100. Clamped to 100 to match the range the renderers and the rest of the crate use.
    pub volume: u8,
    /// True only when every stream of this app is muted.
    pub is_muted: bool,
    /// `application.icon_name`, when the app sets one. Usable as an XDG icon theme lookup.
    pub icon_name: Option<String>,
    /// Human-readable name of the sink this app plays into — the PipeWeaver channel when one is
    /// in use. `None` while the sink list has not been read yet.
    pub routed_to: Option<String>,
}

impl AppStream {
    /// Device id an action stores to bind itself to this app.
    pub fn device_id(&self) -> String {
        format!("{APP_DEVICE_PREFIX}{}", self.key)
    }

    /// Present the app as a [`Device`] so the existing renderers work on it unchanged. The
    /// PipeWeaver-specific fields have no meaning for an app stream and stay `None`.
    pub fn to_device(&self) -> Device {
        Device {
            id: self.device_id(),
            name: self.name.clone(),
            // Apps feed audio in, which is what the source styling in the renderers is for.
            device_type: DeviceType::Source,
            is_physical: false,
            volume: self.volume,
            is_muted: self.is_muted,
            color: None,
            source_mix_a_volume: None,
            source_mix_b_volume: None,
            source_mix_a_muted: None,
            source_mix_b_muted: None,
            source_mute_a_all: None,
            source_mute_b_all: None,
            source_mute_a_target_count: None,
            source_mute_b_target_count: None,
            source_volumes_linked: None,
            target_mix_b: None,
        }
    }
}

/// Whether a reported pid actually belongs to the app that reported it.
///
/// Sandboxed apps report a pid from *their* namespace, not the host's: Flatpak Firefox reports
/// pid 2, which on the host is the kernel thread daemon. Matching a focused window against that
/// would be nonsense, so a pid is only kept when the process wearing it on this host looks like
/// the binary the stream claims. When it does not, the pid is dropped and focus matching falls
/// back to the window class.
fn pid_matches_binary(pid: u32, binary: Option<&str>) -> bool {
    let Some(binary) = binary else {
        // Nothing to check against; trust it rather than throwing away a usable pid.
        return true;
    };
    let Ok(comm) = std::fs::read_to_string(format!("/proc/{pid}/comm")) else {
        return false;
    };
    let comm = comm.trim().to_lowercase();
    let binary = binary.to_lowercase();
    // /proc/<pid>/comm is capped at 15 characters, so compare on that prefix.
    let cut = comm.len().min(binary.len()).min(15);
    cut > 0 && comm[..cut] == binary[..cut]
}

/// Decide what a focus-following action controls, given what focus points at right now and what
/// it last settled on.
///
/// A remembered app is only held while it still has a stream. Once it stops playing there is
/// nothing to control, but the memory is kept: if it starts up again while you are focused
/// elsewhere, it picks up where it left off rather than needing you to click back into it.
fn sticky_focus(
    live: Option<AppStream>,
    remembered: &mut Option<String>,
    apps: &[AppStream],
) -> Option<AppStream> {
    if let Some(app) = live {
        *remembered = Some(app.key.clone());
        return Some(app);
    }
    let key = remembered.as_deref()?;
    apps.iter().find(|app| app.key == key).cloned()
}

/// Strip the key out of an `app:`-prefixed device id.
pub fn app_key_from_device_id(device_id: &str) -> Option<&str> {
    device_id.strip_prefix(APP_DEVICE_PREFIX)
}

enum Command {
    SetVolume { key: String, volume: u8 },
    AdjustVolume { key: String, delta: i16 },
    SetMute { key: String, muted: bool },
    ToggleMute { key: String },
    Stop,
}

pub struct PulseBackend {
    snapshot: Arc<RwLock<Vec<AppStream>>>,
    /// Key of the last app a focus-following action settled on. See [`PulseBackend::focused_app`].
    last_focused: RwLock<Option<String>>,
    available: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    command_tx: Sender<Command>,
}

impl PulseBackend {
    /// Spawn the worker and start tracking sink inputs. Never fails: if no sound server is
    /// reachable the backend simply reports unavailable and keeps retrying.
    pub fn new() -> Arc<Self> {
        let (command_tx, command_rx) = mpsc::channel();
        let backend = Arc::new(Self {
            snapshot: Arc::new(RwLock::new(Vec::new())),
            last_focused: RwLock::new(None),
            available: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(true)),
            command_tx,
        });

        let snapshot = backend.snapshot.clone();
        let available = backend.available.clone();
        let running = backend.running.clone();

        let spawned = std::thread::Builder::new()
            .name("deckweaver-pulse".into())
            .spawn(move || worker(snapshot, available, running, command_rx));

        if let Err(err) = spawned {
            tracing::error!("failed to spawn PulseAudio worker: {err}");
            backend.running.store(false, Ordering::SeqCst);
        }

        backend
    }

    /// True once a sound server connection is established.
    pub fn is_available(&self) -> bool {
        self.available.load(Ordering::Relaxed)
    }

    /// Every app currently playing audio, sorted by display name.
    pub fn apps(&self) -> Vec<AppStream> {
        self.snapshot.read().clone()
    }

    pub fn get(&self, key: &str) -> Option<AppStream> {
        self.snapshot.read().iter().find(|a| a.key == key).cloned()
    }

    /// The app a focus-following action should control right now.
    ///
    /// Sticky: most windows you focus are not playing anything — a terminal, an editor, a file
    /// manager — and going inert every time you tab away from the thing making noise would make
    /// the action useless in practice. So focusing a silent window keeps whatever was last
    /// resolved, and control only moves when you focus something that *is* playing.
    pub fn focused_app(&self, focus: &crate::focus::FocusTracker) -> Option<AppStream> {
        let live = self.focused_app_live(focus);
        let apps = self.snapshot.read();
        let mut remembered = self.last_focused.write();
        sticky_focus(live, &mut remembered, &apps)
    }

    /// The app owning the focused window, with no stickiness applied.
    ///
    /// Matches on pid first, walking the process tree so a browser whose audio comes from a child
    /// process still resolves. Falls back to the window class, which covers apps that report a
    /// different pid for audio than for their window (some sandboxed and Flatpak apps do).
    fn focused_app_live(&self, focus: &crate::focus::FocusTracker) -> Option<AppStream> {
        let apps = self.snapshot.read();

        if let Some(pid) = focus.focused_pid() {
            let matched = apps.iter().find(|app| {
                app.pids
                    .iter()
                    .any(|&stream_pid| crate::focus::is_same_or_descendant(stream_pid, pid))
            });
            if let Some(app) = matched {
                return Some(app.clone());
            }
        }

        // Desktop entry identity, when the compositor reports it. This is the one key a sandboxed
        // app shares with the compositor: its pid is from another namespace and its binary name
        // ("firefox-bin") need not resemble what the compositor calls it, but both sides agree on
        // "org.mozilla.firefox". Tried before the fuzzy class comparison because it is exact.
        if let Some(desktop_id) = focus.focused_desktop_file() {
            let matched = apps.iter().find(|app| {
                crate::icon_loader::find_desktop_id_for_app(&app.name, Some(&app.key))
                    .is_some_and(|id| id.eq_ignore_ascii_case(&desktop_id))
            });
            if let Some(app) = matched {
                return Some(app.clone());
            }
        }

        let class = focus.focused_class()?;
        if class.is_empty() {
            return None;
        }
        let class = class.to_lowercase();
        // Sandboxed apps land here rather than on the pid path, and they are exactly the ones
        // whose identifiers disagree: Flatpak Firefox is binary "firefox-bin", name "Firefox",
        // and a window class of either "firefox" or "org.mozilla.firefox" depending on how it
        // was launched. Compare against every form rather than betting on one.
        let class_tail = class.rsplit('.').next().unwrap_or(&class);
        apps.iter()
            .find(|app| {
                let key = app.key.as_str();
                let key_stem = key.strip_suffix("-bin").unwrap_or(key);
                let name = app.name.to_lowercase();
                key == class
                    || key_stem == class
                    || name == class
                    || key_stem == class_tail
                    || name == class_tail
            })
            .cloned()
    }

    /// Resolve an `app:`-prefixed device id to a [`Device`], or `None` when that app is not
    /// currently playing anything. Handles the focus-following sentinel.
    pub fn device_for(&self, device_id: &str, focus: &crate::focus::FocusTracker) -> Option<Device> {
        self.app_for(device_id, focus).map(|app| app.to_device())
    }

    /// The app an action's device id points at right now.
    pub fn app_for(
        &self,
        device_id: &str,
        focus: &crate::focus::FocusTracker,
    ) -> Option<AppStream> {
        let key = app_key_from_device_id(device_id)?;
        if key == FOCUSED_APP_KEY {
            self.focused_app(focus)
        } else {
            self.get(key)
        }
    }

    pub fn set_volume(&self, key: &str, volume: u8) {
        self.send(Command::SetVolume {
            key: key.to_string(),
            volume: volume.min(100),
        });
    }

    pub fn adjust_volume(&self, key: &str, delta: i16) {
        self.send(Command::AdjustVolume {
            key: key.to_string(),
            delta,
        });
    }

    pub fn set_mute(&self, key: &str, muted: bool) {
        self.send(Command::SetMute {
            key: key.to_string(),
            muted,
        });
    }

    pub fn toggle_mute(&self, key: &str) {
        self.send(Command::ToggleMute {
            key: key.to_string(),
        });
    }

    /// Concrete app key an action's device id acts on right now. Resolves the focus sentinel, so
    /// a press lands on whatever is focused at that moment rather than on a stale target.
    pub fn resolve_key(
        &self,
        device_id: &str,
        focus: &crate::focus::FocusTracker,
    ) -> Option<String> {
        self.app_for(device_id, focus).map(|app| app.key)
    }

    fn send(&self, command: Command) {
        if self.command_tx.send(command).is_err() {
            tracing::debug!("PulseAudio worker is gone; dropping command");
        }
    }
}

impl Drop for PulseBackend {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        let _ = self.command_tx.send(Command::Stop);
    }
}

/// Owns the mainloop and context for the whole process lifetime, reconnecting whenever the sound
/// server goes away.
fn worker(
    snapshot: Arc<RwLock<Vec<AppStream>>>,
    available: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    command_rx: Receiver<Command>,
) {
    while running.load(Ordering::SeqCst) {
        match connect() {
            Some((mainloop, context)) => {
                available.store(true, Ordering::Relaxed);
                tracing::info!("connected to PulseAudio");
                run_session(mainloop, context, &snapshot, &running, &command_rx);
                available.store(false, Ordering::Relaxed);
                snapshot.write().clear();
                tracing::info!("lost PulseAudio connection");
            }
            None => {
                available.store(false, Ordering::Relaxed);
                // Drain anything queued while disconnected so commands don't replay stale on
                // reconnect.
                while command_rx.try_recv().is_ok() {}
                std::thread::sleep(RECONNECT_INTERVAL);
            }
        }
    }
}

/// Bring up a mainloop and context, iterating until the context is ready or fails.
fn connect() -> Option<(Mainloop, Context)> {
    let mut mainloop = Mainloop::new()?;
    let mut context = Context::new(&mainloop, "DeckWeaver")?;

    if context
        .connect(None, ContextFlagSet::NOFLAGS, None)
        .is_err()
    {
        return None;
    }

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match mainloop.iterate(false) {
            IterateResult::Quit(_) | IterateResult::Err(_) => return None,
            IterateResult::Success(_) => {}
        }
        match context.get_state() {
            State::Ready => return Some((mainloop, context)),
            State::Failed | State::Terminated => return None,
            _ => {
                if Instant::now() > deadline {
                    return None;
                }
                std::thread::sleep(TICK);
            }
        }
    }
}

fn run_session(
    mut mainloop: Mainloop,
    mut context: Context,
    snapshot: &Arc<RwLock<Vec<AppStream>>>,
    running: &Arc<AtomicBool>,
    command_rx: &Receiver<Command>,
) {
    // The introspection callback fires on the mainloop, so results land here rather than being
    // returned. Rc/RefCell keeps it single-threaded and avoids a lock on the hot path.
    let poll_result: Rc<RwLock<Option<Vec<RawStream>>>> = Rc::new(RwLock::new(None));
    // Local changes the server has not confirmed yet. See `reconcile`.
    let mut pending: HashMap<String, PendingChange> = HashMap::new();
    // Apps the user has set a level on, and the streams already carrying it.
    let mut managed: HashMap<String, Managed> = HashMap::new();
    // Sink index -> display name. Sinks change far less often than streams, so this is refreshed
    // on its own slower cadence.
    let sink_result: Rc<RwLock<Option<HashMap<u32, String>>>> = Rc::new(RwLock::new(None));
    let mut sinks: HashMap<u32, String> = HashMap::new();
    let mut last_sink_poll = Instant::now() - SINK_POLL_INTERVAL;
    let mut sink_inflight = false;
    let mut last_poll = Instant::now() - POLL_INTERVAL;
    let mut inflight = false;

    loop {
        if !running.load(Ordering::SeqCst) {
            return;
        }

        match mainloop.iterate(false) {
            IterateResult::Quit(_) | IterateResult::Err(_) => return,
            IterateResult::Success(_) => {}
        }

        if context.get_state() != State::Ready {
            return;
        }

        if let Some(map) = sink_result.write().take() {
            sink_inflight = false;
            sinks = map;
        }

        if !sink_inflight && last_sink_poll.elapsed() >= SINK_POLL_INTERVAL {
            last_sink_poll = Instant::now();
            sink_inflight = true;
            let target = sink_result.clone();
            let mut collected: HashMap<u32, String> = HashMap::new();
            context
                .introspect()
                .get_sink_info_list(move |result| match result {
                    ListResult::Item(info) => {
                        collected.insert(info.index, sink_display_name(info));
                    }
                    ListResult::End => *target.write() = Some(std::mem::take(&mut collected)),
                    ListResult::Error => *target.write() = None,
                });
        }

        // Publish whatever the last query produced.
        if let Some(raw) = poll_result.write().take() {
            inflight = false;
            let mut apps = aggregate_with_sinks(raw, &sinks);
            reconcile(&mut apps, &mut pending);
            adopt_new_streams(&mut context, &mut apps, &mut managed, &mut pending);
            *snapshot.write() = apps;
        }

        for command in command_rx.try_iter() {
            match command {
                Command::Stop => return,
                other => apply(&mut context, snapshot, &mut pending, &mut managed, other),
            }
        }

        if !inflight && last_poll.elapsed() >= POLL_INTERVAL {
            last_poll = Instant::now();
            inflight = true;
            let sink = poll_result.clone();
            let mut collected = Vec::new();
            context
                .introspect()
                .get_sink_input_info_list(move |result| match result {
                    ListResult::Item(info) => collected.push(RawStream::from_info(info)),
                    ListResult::End => *sink.write() = Some(std::mem::take(&mut collected)),
                    // A failed query leaves the previous snapshot in place rather than blanking
                    // every action; the next poll will retry.
                    ListResult::Error => *sink.write() = None,
                });
        }

        std::thread::sleep(TICK);
    }
}

/// One sink input as read off the wire, before per-app grouping.
struct RawStream {
    key: String,
    name: String,
    index: u32,
    sink: u32,
    pid: Option<u32>,
    channels: u8,
    volume: u8,
    muted: bool,
    icon_name: Option<String>,
}

impl RawStream {
    fn from_info(info: &libpulse_binding::context::introspect::SinkInputInfo) -> Self {
        let props = &info.proplist;
        let binary = props
            .get_str("application.process.binary")
            .map(|b| clean_binary(&b));
        let app_name = props.get_str("application.name");

        // What the app calls itself is often the toolkit rather than the product: Vesktop reports
        // "Chromium". Its desktop entry says "Vesktop", so that is preferred wherever one is
        // found, with the reported name as the fallback for apps that ship no entry.
        let reported = app_name
            .clone()
            .or_else(|| binary.clone())
            .or_else(|| info.name.as_ref().map(|n| n.to_string()))
            .unwrap_or_else(|| format!("Stream {}", info.index));
        let name = crate::icon_loader::find_desktop_name_for_app(&reported, binary.as_deref())
            .unwrap_or(reported);

        // Prefer the binary: an app's display name can change with what it is playing (browsers
        // put the tab title in it on some sites), while the binary stays put.
        let key = binary
            .clone()
            .or_else(|| app_name.clone())
            .unwrap_or_else(|| name.clone())
            .to_lowercase();

        Self {
            key,
            name,
            index: info.index,
            sink: info.sink,
            pid: props
                .get_str("application.process.id")
                .and_then(|p| p.trim().parse().ok())
                .filter(|pid| pid_matches_binary(*pid, binary.as_deref())),
            channels: info.volume.len(),
            volume: volume_to_percent(info.volume.avg()),
            muted: info.mute,
            icon_name: props.get_str("application.icon_name"),
        }
    }
}

/// PipeWire reports the binary of an updated-in-place app as `"msedge (deleted)"`. Left alone that
/// forks one app into two keys the moment it is patched, silently orphaning a configured action.
fn clean_binary(binary: &str) -> String {
    binary
        .trim()
        .trim_end_matches(" (deleted)")
        .trim()
        .to_string()
}

/// Label for the channel an app plays into.
///
/// PipeWeaver names its virtual sinks `pipeweaver_<channel>`, which is an implementation detail
/// rather than something worth putting on a 200px strip — so that form is unwrapped to just the
/// channel ("pipeweaver_voice_chat" -> "Voice Chat"). Anything else falls back to the sink's own
/// description, which is what a hardware device carries.
fn sink_display_name(info: &libpulse_binding::context::introspect::SinkInfo) -> String {
    let name = info.name.as_deref().unwrap_or_default();
    if let Some(channel) = name.strip_prefix("pipeweaver_") {
        return title_case_words(channel);
    }
    info.description
        .as_deref()
        .filter(|d| !d.is_empty())
        .map(|d| d.to_string())
        .unwrap_or_else(|| name.to_string())
}

fn title_case_words(raw: &str) -> String {
    raw.split(['_', '-'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn volume_to_percent(volume: Volume) -> u8 {
    let normal = Volume::NORMAL.0 as f64;
    ((volume.0 as f64 / normal) * 100.0).round().clamp(0.0, 100.0) as u8
}

fn percent_to_volume(percent: u8) -> Volume {
    let normal = Volume::NORMAL.0 as f64;
    Volume(((percent.min(100) as f64 / 100.0) * normal).round() as u32)
}

/// Collapse per-stream rows into one row per app.
fn aggregate_with_sinks(raw: Vec<RawStream>, sinks: &HashMap<u32, String>) -> Vec<AppStream> {
    let mut by_key: HashMap<String, AppStream> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for stream in raw {
        match by_key.get_mut(&stream.key) {
            Some(existing) => {
                existing.indices.push(stream.index);
                if let Some(pid) = stream.pid {
                    existing.pids.push(pid);
                }
                // Loudest stream wins, so the reading matches what you actually hear.
                existing.volume = existing.volume.max(stream.volume);
                // Only call an app muted when nothing it owns is audible.
                existing.is_muted = existing.is_muted && stream.muted;
                if existing.icon_name.is_none() {
                    existing.icon_name = stream.icon_name;
                }
            }
            None => {
                order.push(stream.key.clone());
                by_key.insert(
                    stream.key.clone(),
                    AppStream {
                        key: stream.key,
                        name: stream.name,
                        indices: vec![stream.index],
                        pids: stream.pid.into_iter().collect(),
                        channels: stream.channels,
                        volume: stream.volume,
                        is_muted: stream.muted,
                        icon_name: stream.icon_name,
                        routed_to: sinks.get(&stream.sink).cloned(),
                    },
                );
            }
        }
    }

    let mut apps: Vec<AppStream> = order
        .into_iter()
        .filter_map(|key| by_key.remove(&key))
        .collect();
    // Stable, predictable ordering for the property inspector lists.
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps
}

/// An app whose volume the user has set through us, and the streams we have already applied it to.
///
/// Browsers replace their sink input on every page load, and their audio backends explicitly set
/// the new stream to their own internal level — full scale — right after creating it, which
/// overrides PulseAudio's own stream-restore. Without adopting those newcomers, reloading a page
/// snaps the app back to 100%: audibly, and on the key.
struct Managed {
    volume: u8,
    muted: bool,
    /// Stream indices already carrying our value.
    known: HashSet<u32>,
}

/// Apply the user's chosen level to streams that have appeared since we last looked.
///
/// Only apps the user has actually adjusted through us are managed, so this never touches the
/// volume of something they have not asked us to control. When no new streams appeared, the
/// target follows whatever the streams currently say, so changing volume in pavucontrol updates
/// the target rather than being fought over.
fn adopt_new_streams(
    context: &mut Context,
    apps: &mut [AppStream],
    managed: &mut HashMap<String, Managed>,
    pending: &mut HashMap<String, PendingChange>,
) {
    let mut introspect = context.introspect();

    for app in apps.iter_mut() {
        let Some(entry) = managed.get_mut(&app.key) else {
            continue;
        };
        let fresh = plan_adoption(app, entry);
        if fresh.is_empty() {
            continue;
        }

        let mut channel_volumes = ChannelVolumes::default();
        channel_volumes.set(app.channels.max(1), percent_to_volume(entry.volume));
        for index in fresh {
            introspect.set_sink_input_volume(index, &channel_volumes, None);
            introspect.set_sink_input_mute(index, entry.muted, None);
        }

        // Hold the target until the server confirms it, exactly as a user-initiated change does.
        // Without this, the next poll lands before the write does, sees the stream still at the
        // browser's own level, and — finding no *new* streams that time — accepts it as an
        // external change. The value the user set is then overwritten by whatever the reload
        // happened to start at.
        pending.insert(
            app.key.clone(),
            PendingChange {
                volume: entry.volume,
                muted: entry.muted,
                since: Instant::now(),
            },
        );
    }

    // Deliberately not pruning apps that currently have no streams. A page load destroys the old
    // stream before creating the new one, and an app that vanishes for even one poll would lose
    // the level the user set — so the new stream would come up at whatever the app or
    // stream-restore chose. Entries are only ever created for apps the user has adjusted, so
    // keeping them for the session costs nothing.
}

/// Decide which of an app's streams need the user's level pushed onto them.
///
/// Split out from the I/O so the rule can be tested without a sound server. Returns the stream
/// indices to write to, and rewrites the reported volume when there are any.
fn plan_adoption(app: &mut AppStream, entry: &mut Managed) -> Vec<u32> {
    let current: HashSet<u32> = app.indices.iter().copied().collect();
    let fresh: Vec<u32> = current.difference(&entry.known).copied().collect();

    if fresh.is_empty() {
        // Nothing new: trust the streams, and let an external change become the new target. This
        // is what keeps pavucontrol and media keys working instead of being fought over.
        entry.volume = app.volume;
        entry.muted = app.is_muted;
        entry.known = current;
        return Vec::new();
    }

    // Report the target straight away rather than the newcomer's full-scale value, so the key
    // never flashes 100% in the frame between the stream appearing and the change landing.
    app.volume = entry.volume;
    app.is_muted = entry.muted;
    entry.known = current;
    fresh
}

/// A change made locally that the server has not echoed back yet.
struct PendingChange {
    volume: u8,
    muted: bool,
    since: Instant,
}

/// Hold locally-applied values until the server confirms them.
///
/// A poll issued *before* a command lands *after* it, carrying pre-command state. Publishing that
/// verbatim rewinds the value on screen for a poll interval, which reads as the bar jumping — and
/// worse, the next relative adjustment is then computed from the rewound number, so turning a dial
/// quietly drops ticks. Keeping the local value until the server agrees fixes both: the snapshot
/// only ever moves forward, and deltas always accumulate from what the user last saw.
fn reconcile(apps: &mut [AppStream], pending: &mut HashMap<String, PendingChange>) {
    if pending.is_empty() {
        return;
    }

    let mut settled: Vec<String> = Vec::new();
    for app in apps.iter_mut() {
        let Some(change) = pending.get(&app.key) else {
            continue;
        };

        if (app.volume, app.is_muted) == (change.volume, change.muted) {
            // Server agrees; stop holding and let polls drive again.
            settled.push(app.key.clone());
        } else if change.since.elapsed() > PENDING_TIMEOUT {
            // Never converged — trust the server rather than showing a value forever.
            settled.push(app.key.clone());
        } else {
            app.volume = change.volume;
            app.is_muted = change.muted;
        }
    }

    for key in settled {
        pending.remove(&key);
    }
    // Drop anything for an app that has since stopped playing.
    pending.retain(|key, _| apps.iter().any(|app| &app.key == key));
}

/// Run a command against every stream the app owns, then fold the result into the snapshot right
/// away. Waiting for the next poll to reflect it would make a knob feel like it lagged a frame
/// behind the dial.
fn apply(
    context: &mut Context,
    snapshot: &Arc<RwLock<Vec<AppStream>>>,
    pending: &mut HashMap<String, PendingChange>,
    managed: &mut HashMap<String, Managed>,
    command: Command,
) {
    let (key, target_volume, target_mute) = {
        let apps = snapshot.read();
        let key = match &command {
            Command::SetVolume { key, .. }
            | Command::AdjustVolume { key, .. }
            | Command::SetMute { key, .. }
            | Command::ToggleMute { key } => key.clone(),
            Command::Stop => return,
        };
        let Some(app) = apps.iter().find(|a| a.key == key) else {
            return;
        };

        match &command {
            Command::SetVolume { volume, .. } => (app.key.clone(), Some(*volume), None),
            Command::AdjustVolume { delta, .. } => {
                let next = (app.volume as i16 + delta).clamp(0, 100) as u8;
                (app.key.clone(), Some(next), None)
            }
            Command::SetMute { muted, .. } => (app.key.clone(), None, Some(*muted)),
            Command::ToggleMute { .. } => (app.key.clone(), None, Some(!app.is_muted)),
            Command::Stop => return,
        }
    };

    let (indices, channels) = {
        let apps = snapshot.read();
        match apps.iter().find(|a| a.key == key) {
            Some(app) => (app.indices.clone(), app.channels),
            None => return,
        }
    };

    let mut introspect = context.introspect();

    if let Some(volume) = target_volume {
        let mut channel_volumes = ChannelVolumes::default();
        channel_volumes.set(channels.max(1), percent_to_volume(volume));
        for index in &indices {
            introspect.set_sink_input_volume(*index, &channel_volumes, None);
        }
    }

    if let Some(muted) = target_mute {
        for index in &indices {
            introspect.set_sink_input_mute(*index, muted, None);
        }
    }

    let mut apps = snapshot.write();
    if let Some(app) = apps.iter_mut().find(|a| a.key == key) {
        if let Some(volume) = target_volume {
            app.volume = volume;
        }
        if let Some(muted) = target_mute {
            app.is_muted = muted;
        }
        // From now on this app's level is the user's choice, and any stream it opens later gets
        // it too.
        managed.insert(
            key.clone(),
            Managed {
                volume: app.volume,
                muted: app.is_muted,
                known: indices.iter().copied().collect(),
            },
        );

        // Record what we expect to see so an in-flight poll cannot rewind it.
        pending.insert(
            key,
            PendingChange {
                volume: app.volume,
                muted: app.is_muted,
                since: Instant::now(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(key: &str, name: &str, index: u32, volume: u8, muted: bool) -> RawStream {
        RawStream {
            key: key.to_string(),
            name: name.to_string(),
            index,
            sink: 1,
            pid: Some(1000 + index),
            channels: 2,
            volume,
            muted,
            icon_name: None,
        }
    }

    #[test]
    fn percent_round_trips_through_volume() {
        for percent in [0u8, 1, 25, 50, 99, 100] {
            assert_eq!(volume_to_percent(percent_to_volume(percent)), percent);
        }
    }

    /// PipeWire tags the binary of an app that was updated in place, which would otherwise split
    /// one app into two keys and orphan whatever action was already bound to it.
    #[test]
    fn deleted_suffix_is_stripped_from_binary() {
        assert_eq!(clean_binary("msedge (deleted)"), "msedge");
        assert_eq!(clean_binary("  msedge  "), "msedge");
        assert_eq!(clean_binary("msedge"), "msedge");
    }

    /// A browser opens a stream per tab; all of them have to fold into one controllable app.
    #[test]
    fn streams_group_by_key() {
        let apps = aggregate(vec![
            raw("msedge", "Microsoft Edge", 1, 40, false),
            raw("msedge", "Microsoft Edge", 2, 70, false),
        ]);

        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].indices, vec![1, 2]);
        // Loudest stream is the one you hear, so that is the honest reading.
        assert_eq!(apps[0].volume, 70);
    }

    /// One unmuted tab still makes noise, so the app is not muted.
    #[test]
    fn app_is_muted_only_when_every_stream_is() {
        let partly = aggregate(vec![
            raw("msedge", "Microsoft Edge", 1, 50, true),
            raw("msedge", "Microsoft Edge", 2, 50, false),
        ]);
        assert!(!partly[0].is_muted);

        let fully = aggregate(vec![
            raw("msedge", "Microsoft Edge", 1, 50, true),
            raw("msedge", "Microsoft Edge", 2, 50, true),
        ]);
        assert!(fully[0].is_muted);
    }

    #[test]
    fn apps_sort_by_name_case_insensitively() {
        let apps = aggregate(vec![
            raw("zoom", "Zoom", 1, 50, false),
            raw("firefox", "firefox", 2, 50, false),
            raw("ardour", "Ardour", 3, 50, false),
        ]);
        let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["Ardour", "firefox", "Zoom"]);
    }

    /// Tests do not care about sink naming; they exercise grouping and reconciliation.
    fn aggregate(raw: Vec<RawStream>) -> Vec<AppStream> {
        aggregate_with_sinks(raw, &HashMap::new())
    }

    fn pending_change(volume: u8, muted: bool, age: Duration) -> PendingChange {
        PendingChange {
            volume,
            muted,
            since: Instant::now() - age,
        }
    }

    /// A poll issued before a command lands after it, carrying the pre-command volume. Publishing
    /// that rewinds the bar for a poll interval and makes the next relative adjustment compute off
    /// the stale number, silently dropping dial ticks.
    #[test]
    fn stale_poll_does_not_rewind_a_local_change() {
        let mut apps = aggregate(vec![raw("msedge", "Microsoft Edge", 1, 100, false)]);
        let mut pending = HashMap::from([(
            "msedge".to_string(),
            pending_change(80, false, Duration::ZERO),
        )]);

        reconcile(&mut apps, &mut pending);

        assert_eq!(apps[0].volume, 80, "stale poll must not win");
        assert!(pending.contains_key("msedge"), "still waiting on the server");
    }

    /// Once the server echoes the value back, polls have to drive again — otherwise a change made
    /// elsewhere (pavucontrol, media keys) would never show up.
    #[test]
    fn confirmed_change_releases_the_hold() {
        let mut apps = aggregate(vec![raw("msedge", "Microsoft Edge", 1, 80, false)]);
        let mut pending = HashMap::from([(
            "msedge".to_string(),
            pending_change(80, false, Duration::ZERO),
        )]);

        reconcile(&mut apps, &mut pending);

        assert_eq!(apps[0].volume, 80);
        assert!(pending.is_empty(), "hold should be released once confirmed");
    }

    /// If the server never accepts the value, showing it forever would be worse than being wrong
    /// for one poll.
    #[test]
    fn unconfirmed_change_expires() {
        let mut apps = aggregate(vec![raw("msedge", "Microsoft Edge", 1, 100, false)]);
        let mut pending = HashMap::from([(
            "msedge".to_string(),
            pending_change(80, false, PENDING_TIMEOUT + Duration::from_millis(50)),
        )]);

        reconcile(&mut apps, &mut pending);

        assert_eq!(apps[0].volume, 100, "server wins after the timeout");
        assert!(pending.is_empty());
    }

    #[test]
    fn mute_is_held_the_same_way() {
        let mut apps = aggregate(vec![raw("msedge", "Microsoft Edge", 1, 50, false)]);
        let mut pending = HashMap::from([(
            "msedge".to_string(),
            pending_change(50, true, Duration::ZERO),
        )]);

        reconcile(&mut apps, &mut pending);

        assert!(apps[0].is_muted, "local mute must not be rewound");
    }

    /// An app that stopped playing has nothing left to confirm, so its hold must not leak.
    #[test]
    fn hold_is_dropped_when_the_app_goes_away() {
        let mut apps = aggregate(vec![raw("zoom", "Zoom", 1, 50, false)]);
        let mut pending = HashMap::from([(
            "msedge".to_string(),
            pending_change(80, false, Duration::ZERO),
        )]);

        reconcile(&mut apps, &mut pending);

        assert!(pending.is_empty(), "hold for a departed app should not leak");
    }

    /// PipeWeaver's sink names are an implementation detail; the strip should read as the channel
    /// the user configured, not as "pipeweaver_voice_chat".
    #[test]
    fn pipeweaver_sinks_render_as_channel_names() {
        assert_eq!(title_case_words("voice_chat"), "Voice Chat");
        assert_eq!(title_case_words("system"), "System");
        assert_eq!(title_case_words("browser"), "Browser");
        assert_eq!(title_case_words("sfx"), "Sfx");
    }

    #[test]
    fn title_casing_tolerates_odd_separators() {
        assert_eq!(title_case_words("game__audio"), "Game Audio");
        assert_eq!(title_case_words("music-player"), "Music Player");
        assert_eq!(title_case_words(""), "");
    }

    #[test]
    fn routing_is_attached_from_the_sink_map() {
        let sinks = HashMap::from([(1u32, "Browser".to_string())]);
        let apps = aggregate_with_sinks(vec![raw("msedge", "Microsoft Edge", 1, 50, false)], &sinks);
        assert_eq!(apps[0].routed_to.as_deref(), Some("Browser"));
    }

    /// The sink list is read on its own slower cadence, so the first frames can arrive before it.
    #[test]
    fn unknown_sink_leaves_routing_unset() {
        let apps = aggregate_with_sinks(
            vec![raw("msedge", "Microsoft Edge", 1, 50, false)],
            &HashMap::new(),
        );
        assert!(apps[0].routed_to.is_none());
    }

    /// Focusing something that is playing takes control, and is remembered.
    #[test]
    fn focusing_a_playing_app_takes_control() {
        let apps = aggregate(vec![raw("msedge", "Microsoft Edge", 1, 50, false)]);
        let mut remembered = None;

        let got = sticky_focus(Some(apps[0].clone()), &mut remembered, &apps);

        assert_eq!(got.map(|a| a.key), Some("msedge".to_string()));
        assert_eq!(remembered.as_deref(), Some("msedge"));
    }

    /// The point of the whole thing: tabbing to a terminal must not drop control of the browser.
    #[test]
    fn focusing_a_silent_window_keeps_the_last_app() {
        let apps = aggregate(vec![raw("msedge", "Microsoft Edge", 1, 50, false)]);
        let mut remembered = Some("msedge".to_string());

        let got = sticky_focus(None, &mut remembered, &apps);

        assert_eq!(got.map(|a| a.key), Some("msedge".to_string()));
    }

    /// Control still moves when you focus a different app that is actually playing.
    #[test]
    fn focusing_another_playing_app_moves_control() {
        let apps = aggregate(vec![
            raw("msedge", "Microsoft Edge", 1, 50, false),
            raw("firefox-bin", "Firefox", 2, 30, false),
        ]);
        let firefox = apps.iter().find(|a| a.key == "firefox-bin").unwrap().clone();
        let mut remembered = Some("msedge".to_string());

        let got = sticky_focus(Some(firefox), &mut remembered, &apps);

        assert_eq!(got.map(|a| a.key), Some("firefox-bin".to_string()));
        assert_eq!(remembered.as_deref(), Some("firefox-bin"));
    }

    /// A remembered app that stopped playing has no stream to control.
    #[test]
    fn remembered_app_that_stopped_playing_yields_nothing() {
        let apps = aggregate(vec![raw("firefox-bin", "Firefox", 1, 30, false)]);
        let mut remembered = Some("msedge".to_string());

        assert!(sticky_focus(None, &mut remembered, &apps).is_none());
        // Kept, so it resumes control if it starts playing again.
        assert_eq!(remembered.as_deref(), Some("msedge"));
    }

    #[test]
    fn remembered_app_resumes_when_it_plays_again() {
        let mut remembered = Some("msedge".to_string());
        let silent: Vec<AppStream> = Vec::new();
        assert!(sticky_focus(None, &mut remembered, &silent).is_none());

        let apps = aggregate(vec![raw("msedge", "Microsoft Edge", 1, 50, false)]);
        assert_eq!(
            sticky_focus(None, &mut remembered, &apps).map(|a| a.key),
            Some("msedge".to_string())
        );
    }

    #[test]
    fn nothing_remembered_and_nothing_focused_is_none() {
        let apps = aggregate(vec![raw("msedge", "Microsoft Edge", 1, 50, false)]);
        let mut remembered = None;
        assert!(sticky_focus(None, &mut remembered, &apps).is_none());
    }

    fn managed(volume: u8, muted: bool, known: &[u32]) -> Managed {
        Managed {
            volume,
            muted,
            known: known.iter().copied().collect(),
        }
    }

    /// The reported bug: a browser replaces its stream on page load and sets the new one to full
    /// scale, which would otherwise snap the app back to 100% both audibly and on the key.
    #[test]
    fn a_replacement_stream_is_pulled_back_to_the_chosen_level() {
        let mut apps = aggregate(vec![raw("zen-bin", "Zen", 7, 100, false)]);
        let mut entry = managed(35, false, &[3]);

        let fresh = plan_adoption(&mut apps[0], &mut entry);

        assert_eq!(fresh, vec![7], "the new stream must be written to");
        assert_eq!(apps[0].volume, 35, "and must not be reported at full scale");
    }

    /// A second tab starting while one is already playing gets the same treatment.
    #[test]
    fn an_additional_stream_is_adopted_without_disturbing_the_first() {
        let mut apps = aggregate(vec![
            raw("zen-bin", "Zen", 1, 35, false),
            raw("zen-bin", "Zen", 2, 100, false),
        ]);
        let mut entry = managed(35, false, &[1]);

        let fresh = plan_adoption(&mut apps[0], &mut entry);

        assert_eq!(fresh, vec![2]);
        assert_eq!(apps[0].volume, 35);
    }

    /// Changing volume in pavucontrol must stick rather than being pulled back, or the two fight.
    #[test]
    fn an_external_change_becomes_the_new_target() {
        let mut apps = aggregate(vec![raw("zen-bin", "Zen", 1, 80, false)]);
        let mut entry = managed(35, false, &[1]);

        let fresh = plan_adoption(&mut apps[0], &mut entry);

        assert!(fresh.is_empty(), "nothing new, so nothing to write");
        assert_eq!(apps[0].volume, 80, "the external value is respected");
        assert_eq!(entry.volume, 80, "and becomes what new streams inherit");
    }

    #[test]
    fn mute_is_adopted_alongside_volume() {
        let mut apps = aggregate(vec![raw("zen-bin", "Zen", 9, 100, false)]);
        let mut entry = managed(35, true, &[1]);

        let fresh = plan_adoption(&mut apps[0], &mut entry);

        assert_eq!(fresh, vec![9]);
        assert!(apps[0].is_muted, "a replacement stream must not unmute the app");
    }

    /// Indices are recycled constantly; a stale one must not make a genuinely new stream look
    /// already-adopted.
    #[test]
    fn dead_indices_are_pruned_from_the_known_set() {
        let mut apps = aggregate(vec![raw("zen-bin", "Zen", 5, 35, false)]);
        let mut entry = managed(35, false, &[1, 2, 3, 5]);

        plan_adoption(&mut apps[0], &mut entry);

        assert_eq!(entry.known, [5].into_iter().collect::<HashSet<u32>>());
    }

    /// The regression behind "it comes back lower": after adopting a replacement stream, the very
    /// next poll can arrive before the write lands. That poll sees no *new* streams, so the
    /// external-change rule would accept the browser's own level as the user's new target.
    /// Holding the value through `pending` until the server confirms is what prevents it.
    #[test]
    fn an_unlanded_adoption_is_not_mistaken_for_an_external_change() {
        let mut entry = managed(65, false, &[1]);

        // Poll 1: the replacement stream shows up at full scale and gets adopted.
        let mut apps = aggregate(vec![raw("zen-bin", "Zen", 9, 100, false)]);
        assert_eq!(plan_adoption(&mut apps[0], &mut entry), vec![9]);
        let mut pending = HashMap::from([(
            "zen-bin".to_string(),
            pending_change(entry.volume, entry.muted, Duration::ZERO),
        )]);

        // Poll 2: the write has not landed, so the stream still reads full scale.
        let mut apps = aggregate(vec![raw("zen-bin", "Zen", 9, 100, false)]);
        reconcile(&mut apps, &mut pending);
        let fresh = plan_adoption(&mut apps[0], &mut entry);

        assert!(fresh.is_empty(), "the stream is no longer new");
        assert_eq!(apps[0].volume, 65, "reconcile must hold the adopted value");
        assert_eq!(entry.volume, 65, "and the target must not drift to the browser's level");
    }

    /// A page load destroys the old stream before making the new one. If the app going quiet for a
    /// single poll dropped its entry, the replacement would come up at whatever the app chose.
    #[test]
    fn a_gap_with_no_streams_does_not_lose_the_chosen_level() {
        let mut entry = managed(65, false, &[1]);

        // The app is briefly absent entirely, then returns on a new stream at full scale.
        let mut apps = aggregate(vec![raw("zen-bin", "Zen", 4, 100, false)]);
        let fresh = plan_adoption(&mut apps[0], &mut entry);

        assert_eq!(fresh, vec![4]);
        assert_eq!(apps[0].volume, 65, "the level set before the reload still applies");
    }

    #[test]
    fn device_id_round_trips() {
        let apps = aggregate(vec![raw("msedge", "Microsoft Edge", 1, 50, false)]);
        let id = apps[0].device_id();
        assert_eq!(id, "app:msedge");
        assert_eq!(app_key_from_device_id(&id), Some("msedge"));
        assert_eq!(app_key_from_device_id("some-pipeweaver-uuid"), None);
    }

    /// The renderers read these fields directly, so the mapping has to survive refactors.
    #[test]
    fn device_mapping_carries_volume_and_mute() {
        let apps = aggregate(vec![raw("msedge", "Microsoft Edge", 1, 42, true)]);
        let device = apps[0].to_device();
        assert_eq!(device.name, "Microsoft Edge");
        assert_eq!(device.volume, 42);
        assert!(device.is_muted);
        assert_eq!(device.device_type, DeviceType::Source);
    }
}
