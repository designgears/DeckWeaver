//! Tracks which window has keyboard focus, so an action can follow it instead of being pinned to
//! one application.
//!
//! There is no portable way to ask "what is focused?" on Wayland. The wlr and ext foreign-toplevel
//! protocols would answer it generically, but the compositors that matter either do not expose
//! them to ordinary clients (KWin advertises none of them) or predate them. So this is a set of
//! per-environment backends, picked at startup by [`detect_backend`]:
//!
//! | Backend   | How                                                       |
//! |-----------|-----------------------------------------------------------|
//! | KWin      | injected KWin script reporting over D-Bus                  |
//! | Hyprland  | `hyprctl -j activewindow`                                  |
//! | niri      | `niri msg -j focused-window`                               |
//! | Sway / i3 | `swaymsg -t get_tree` / `i3-msg -t get_tree`, walked       |
//! | X11       | `_NET_ACTIVE_WINDOW` on the root window                     |
//!
//! The compositor-specific ones shell out to the CLI each compositor ships rather than speaking
//! its socket protocol directly. Those protocols are versioned and easy to get subtly wrong, and
//! none of them can be tested from here; the CLI output is the stable, documented contract. The
//! cost is a short-lived process a few times a second, which is cheap next to being wrong.
//!
//! X11 is handled properly through x11rb because it is one well-specified protocol that covers
//! every X11 desktop at once, and needs no external binary.
//!
//! GNOME on Wayland has no supported answer at all — it exposes no focus API without a shell
//! extension — so it detects as unsupported and focus-following actions are hidden there.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

/// Bus name this process owns. The injected script calls into it.
const BUS_NAME: &str = "com.designgears.DeckWeaver";
const OBJECT_PATH: &str = "/Focus";

/// Identifier KWin knows the injected script by, used to replace/unload it.
const SCRIPT_NAME: &str = "deckweaver-focus";

/// Reports `workspace.activeWindow` on every focus change, plus once at startup so a plugin that
/// starts mid-session knows the current window without waiting for the user to alt-tab.
const KWIN_SCRIPT: &str = r#"
function deckweaverReport() {
    var w = workspace.activeWindow;
    if (!w) return;
    callDBus("com.designgears.DeckWeaver", "/Focus", "com.designgears.DeckWeaver.Focus",
             "WindowActivated", "" + w.pid, "" + w.resourceClass, "" + w.caption,
             "" + (w.desktopFileName || ""));
}
workspace.windowActivated.connect(deckweaverReport);
deckweaverReport();
"#;

/// The focused window, as last reported by the compositor.
#[derive(Debug, Clone, Default)]
pub struct FocusedWindow {
    pub pid: u32,
    /// Window class, e.g. "msedge". A fallback match when pid lookup fails.
    pub class: String,
    pub caption: String,
    /// Desktop entry id the window belongs to, e.g. "org.mozilla.firefox". Empty when the
    /// compositor does not report one. The most reliable key for sandboxed apps, whose pid is
    /// from another namespace and whose class need not resemble their binary.
    pub desktop_file: String,
}

pub struct FocusTracker {
    current: Arc<RwLock<Option<FocusedWindow>>>,
    /// Set once a compositor backend is actually reporting.
    active: Arc<AtomicBool>,
    /// Mirrors `current.pid` for lock-free reads on the render path.
    pid: Arc<AtomicU32>,
}

impl FocusTracker {
    /// Start tracking. Never fails: on a desktop with no supported backend the tracker simply
    /// stays inactive and focus-following actions report as unavailable.
    pub fn new() -> Arc<Self> {
        let tracker = Arc::new(Self {
            current: Arc::new(RwLock::new(None)),
            active: Arc::new(AtomicBool::new(false)),
            pid: Arc::new(AtomicU32::new(0)),
        });

        let Some(backend) = detect_backend() else {
            tracing::info!("focus tracking unavailable: no supported compositor detected");
            return tracker;
        };
        tracing::info!("focus tracking using the {} backend", backend.label());

        let current = tracker.current.clone();
        let active = tracker.active.clone();
        let pid = tracker.pid.clone();

        let spawned = std::thread::Builder::new()
            .name("deckweaver-focus".into())
            .spawn(move || match backend {
                // KWin pushes to us over D-Bus, so it needs an async runtime and no polling.
                Backend::KWin => {
                    let runtime = match tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(runtime) => runtime,
                        Err(err) => {
                            tracing::error!("focus tracker runtime failed to start: {err}");
                            return;
                        }
                    };
                    runtime.block_on(serve(current, active, pid));
                }
                other => poll_loop(other, current, active, pid),
            });

        if let Err(err) = spawned {
            tracing::error!("failed to spawn focus tracker: {err}");
        }

        tracker
    }

    /// True once a compositor backend is reporting focus changes.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    pub fn focused(&self) -> Option<FocusedWindow> {
        self.current.read().clone()
    }

    /// Pid of the process owning the focused window, or `None` when nothing is being tracked.
    pub fn focused_pid(&self) -> Option<u32> {
        match self.pid.load(Ordering::Relaxed) {
            0 => None,
            pid => Some(pid),
        }
    }

    pub fn focused_class(&self) -> Option<String> {
        self.current.read().as_ref().map(|w| w.class.clone())
    }

    /// Desktop entry id of the focused window, when the compositor reports one.
    pub fn focused_desktop_file(&self) -> Option<String> {
        self.current
            .read()
            .as_ref()
            .map(|w| w.desktop_file.clone())
            .filter(|id| !id.is_empty())
    }
}

/// Which mechanism can answer "what is focused?" in this session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    KWin,
    Hyprland,
    Niri,
    Sway,
    I3,
    X11,
}

impl Backend {
    fn label(self) -> &'static str {
        match self {
            Backend::KWin => "KWin",
            Backend::Hyprland => "Hyprland",
            Backend::Niri => "niri",
            Backend::Sway => "Sway",
            Backend::I3 => "i3",
            Backend::X11 => "X11",
        }
    }
}

/// Pick a backend for this session.
///
/// Wayland compositors are identified by the socket they export, which is more reliable than
/// `XDG_CURRENT_DESKTOP` — that is set by the session file and can be missing, renamed, or
/// inherited by a nested session. X11 comes last: it is the fallback that works under any X11
/// window manager, and on a Wayland session `DISPLAY` is usually still set by XWayland, where it
/// would only ever see X11 clients.
pub fn detect_backend() -> Option<Backend> {
    detect_backend_from(|var| std::env::var(var).ok())
}

/// Detection against an arbitrary environment, so the precedence rules can be tested without
/// mutating the process environment.
fn detect_backend_from(get: impl Fn(&str) -> Option<String>) -> Option<Backend> {
    let has = |var: &str| get(var).is_some_and(|v| !v.is_empty());

    if has("HYPRLAND_INSTANCE_SIGNATURE") {
        return Some(Backend::Hyprland);
    }
    if has("NIRI_SOCKET") {
        return Some(Backend::Niri);
    }
    if has("SWAYSOCK") {
        return Some(Backend::Sway);
    }
    if get("XDG_CURRENT_DESKTOP")
        .map(|d| d.to_ascii_lowercase().contains("kde"))
        .unwrap_or(false)
    {
        return Some(Backend::KWin);
    }
    if has("I3SOCK") {
        return Some(Backend::I3);
    }
    // Only trust X11 when this is actually an X11 session; under XWayland the root window knows
    // nothing about native Wayland windows, so it would report stale or empty focus forever.
    let session_is_x11 = get("XDG_SESSION_TYPE")
        .map(|s| s.eq_ignore_ascii_case("x11"))
        .unwrap_or(false);
    if session_is_x11 && has("DISPLAY") {
        return Some(Backend::X11);
    }
    None
}

struct FocusService {
    current: Arc<RwLock<Option<FocusedWindow>>>,
    pid: Arc<AtomicU32>,
}

#[zbus::interface(name = "com.designgears.DeckWeaver.Focus")]
impl FocusService {
    /// Called by the injected KWin script. Arguments arrive as strings because KWin's `callDBus`
    /// cannot express integer signatures.
    async fn window_activated(
        &self,
        pid: String,
        class: String,
        caption: String,
        desktop_file: String,
    ) {
        let pid: u32 = pid.trim().parse().unwrap_or(0);
        self.pid.store(pid, Ordering::Relaxed);
        *self.current.write() = Some(FocusedWindow {
            pid,
            class,
            caption,
            // KWin reports this with a trailing ".desktop" on some versions; normalise it so it
            // compares equal to a desktop entry's file stem.
            desktop_file: desktop_file
                .trim()
                .trim_end_matches(".desktop")
                .to_string(),
        });
    }
}

async fn serve(
    current: Arc<RwLock<Option<FocusedWindow>>>,
    active: Arc<AtomicBool>,
    pid: Arc<AtomicU32>,
) {
    let service = FocusService {
        current,
        pid: pid.clone(),
    };

    let connection = match zbus::connection::Builder::session()
        .and_then(|b| b.name(BUS_NAME))
        .and_then(|b| b.serve_at(OBJECT_PATH, service))
        .map(|b| b.build())
    {
        Ok(build) => match build.await {
            Ok(connection) => connection,
            Err(err) => {
                tracing::warn!("focus tracker could not take {BUS_NAME}: {err}");
                return;
            }
        },
        Err(err) => {
            tracing::warn!("focus tracker D-Bus setup failed: {err}");
            return;
        }
    };

    if let Err(err) = inject_kwin_script(&connection).await {
        tracing::warn!("could not install the KWin focus script: {err}");
        return;
    }

    active.store(true, Ordering::Relaxed);
    tracing::info!("focus tracking active via KWin");

    // Nothing else to drive: the script calls in as focus changes. Park the runtime so the
    // connection stays alive for the life of the process.
    std::future::pending::<()>().await;
}

/// Load the reporting script into KWin and start it.
///
/// `loadScript` is idempotent by name in the sense that a second load with the same name replaces
/// the first, so a plugin restart does not stack duplicate scripts.
async fn inject_kwin_script(connection: &zbus::Connection) -> zbus::Result<()> {
    // KWin's loadScript takes a path, not source, so the script has to exist on disk. Keep it in
    // the runtime dir: it is per-session, cleaned up by the system, and never mistaken for
    // configuration the user is expected to manage.
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let path = std::path::Path::new(&dir).join("deckweaver-focus.js");
    std::fs::write(&path, KWIN_SCRIPT)
        .map_err(|err| zbus::Error::Failure(format!("writing {}: {err}", path.display())))?;

    let scripting = zbus::Proxy::new(
        connection,
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
    )
    .await?;

    // Replace any script left behind by a previous run before loading the new one.
    let _: Result<bool, _> = scripting.call("unloadScript", &(SCRIPT_NAME)).await;

    let _: i32 = scripting
        .call(
            "loadScript",
            &(path.to_string_lossy().to_string(), SCRIPT_NAME.to_string()),
        )
        .await?;

    // Loaded scripts stay dormant until the engine is started.
    let _: () = scripting.call("start", &()).await?;

    Ok(())
}

/// Whether `pid` is `ancestor`, or descends from it.
///
/// Chromium-family apps play audio from a child process while the window belongs to the parent —
/// verified on this machine, where the Edge sink input's pid was a direct child of the pid owning
/// the window. Comparing pids directly would never match those apps, which is most browsers.
pub fn is_same_or_descendant(pid: u32, ancestor: u32) -> bool {
    if pid == ancestor {
        return true;
    }

    let mut current = pid;
    // Deep enough for real process trees, bounded so a malformed /proc cannot spin here.
    for _ in 0..24 {
        match parent_pid(current) {
            Some(parent) if parent == ancestor => return true,
            // pid 1 has no useful parent; stop rather than walking into init.
            Some(parent) if parent > 1 => current = parent,
            _ => return false,
        }
    }
    false
}

/// Read PPid out of /proc/<pid>/status.
fn parent_pid(pid: u32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("PPid:"))
        .and_then(|value| value.trim().parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    #[test]
    fn wayland_compositors_are_detected_by_their_socket() {
        assert_eq!(
            detect_backend_from(env(&[("HYPRLAND_INSTANCE_SIGNATURE", "abc")])),
            Some(Backend::Hyprland)
        );
        assert_eq!(
            detect_backend_from(env(&[("NIRI_SOCKET", "/run/niri.sock")])),
            Some(Backend::Niri)
        );
        assert_eq!(
            detect_backend_from(env(&[("SWAYSOCK", "/run/sway.sock")])),
            Some(Backend::Sway)
        );
        assert_eq!(
            detect_backend_from(env(&[("XDG_CURRENT_DESKTOP", "KDE")])),
            Some(Backend::KWin)
        );
    }

    /// XWayland sets DISPLAY inside a Wayland session, where the X11 root window can only ever see
    /// X11 clients. Picking X11 there would report stale focus forever.
    #[test]
    fn x11_is_only_chosen_for_a_real_x11_session() {
        assert_eq!(
            detect_backend_from(env(&[("DISPLAY", ":0"), ("XDG_SESSION_TYPE", "wayland")])),
            None
        );
        assert_eq!(
            detect_backend_from(env(&[("DISPLAY", ":0"), ("XDG_SESSION_TYPE", "x11")])),
            Some(Backend::X11)
        );
    }

    /// A compositor socket must win over the DISPLAY that XWayland also exports.
    #[test]
    fn compositor_socket_beats_display() {
        assert_eq!(
            detect_backend_from(env(&[
                ("SWAYSOCK", "/run/sway.sock"),
                ("DISPLAY", ":0"),
                ("XDG_SESSION_TYPE", "x11"),
            ])),
            Some(Backend::Sway)
        );
    }

    #[test]
    fn empty_variables_do_not_count_as_present() {
        assert_eq!(detect_backend_from(env(&[("SWAYSOCK", "")])), None);
    }

    #[test]
    fn gnome_wayland_has_no_backend() {
        assert_eq!(
            detect_backend_from(env(&[
                ("XDG_CURRENT_DESKTOP", "GNOME"),
                ("XDG_SESSION_TYPE", "wayland"),
            ])),
            None
        );
    }

    #[test]
    fn hyprland_output_parses() {
        let window = parse_hyprland(
            r#"{"address":"0x1","pid":4242,"class":"firefox","title":"Some Page"}"#,
        )
        .expect("focused window");
        assert_eq!(window.pid, 4242);
        assert_eq!(window.class, "firefox");
        assert_eq!(window.caption, "Some Page");
    }

    /// Hyprland answers with an empty object when the workspace has no focused window.
    #[test]
    fn hyprland_empty_response_is_none() {
        assert!(parse_hyprland("{}").is_none());
        assert!(parse_hyprland("not json").is_none());
    }

    #[test]
    fn niri_output_parses() {
        let window =
            parse_niri(r#"{"id":3,"pid":777,"app_id":"org.mozilla.firefox","title":"Page"}"#)
                .expect("focused window");
        assert_eq!(window.pid, 777);
        assert_eq!(window.class, "org.mozilla.firefox");
    }

    #[test]
    fn niri_null_response_is_none() {
        assert!(parse_niri("null").is_none());
    }

    /// sway reports a tree; the focused leaf has to be dug out of it.
    #[test]
    fn sway_tree_yields_the_focused_leaf() {
        let tree = r#"{
            "type":"root","focused":false,"nodes":[
              {"type":"output","focused":false,"nodes":[
                {"type":"con","focused":false,"nodes":[
                  {"type":"con","focused":false,"pid":10,"app_id":"alacritty","name":"term","nodes":[]},
                  {"type":"con","focused":true,"pid":20,"app_id":"firefox","name":"Page","nodes":[]}
                ]}
              ]}
            ]}"#;
        let window = parse_i3_tree(tree).expect("focused window");
        assert_eq!(window.pid, 20);
        assert_eq!(window.class, "firefox");
        assert_eq!(window.caption, "Page");
    }

    /// i3 has no app_id — that is a Wayland concept — and reports window_properties instead.
    #[test]
    fn i3_tree_falls_back_to_window_properties() {
        let tree = r#"{
            "type":"root","focused":false,"nodes":[
              {"type":"con","focused":true,"pid":55,"name":"Page","nodes":[],
               "window_properties":{"class":"Firefox","instance":"Navigator"}}
            ]}"#;
        let window = parse_i3_tree(tree).expect("focused window");
        assert_eq!(window.pid, 55);
        assert_eq!(window.class, "Firefox");
    }

    /// A focused container is not a window, so it must not be mistaken for one.
    #[test]
    fn focused_container_is_skipped_for_its_focused_child() {
        let tree = r#"{
            "type":"root","focused":true,"nodes":[
              {"type":"con","focused":true,"pid":99,"app_id":"mpv","name":"Video","nodes":[]}
            ]}"#;
        let window = parse_i3_tree(tree).expect("focused window");
        assert_eq!(window.pid, 99, "should descend to the leaf, not stop at the root");
    }

    #[test]
    fn tree_with_nothing_focused_is_none() {
        assert!(parse_i3_tree(r#"{"type":"root","focused":false,"nodes":[]}"#).is_none());
    }

    #[test]
    fn a_pid_is_its_own_ancestor() {
        let me = std::process::id();
        assert!(is_same_or_descendant(me, me));
    }

    /// The case that matters: a browser's audio process is a child of the window's process.
    #[test]
    fn child_is_a_descendant_of_its_parent() {
        let me = std::process::id();
        let parent = parent_pid(me).expect("this process has a parent");
        assert!(
            is_same_or_descendant(me, parent),
            "current process should be seen as descending from its parent"
        );
    }

    #[test]
    fn unrelated_pids_do_not_match() {
        let me = std::process::id();
        // pid 1 never descends from this test process.
        assert!(!is_same_or_descendant(1, me));
    }

    #[test]
    fn missing_pid_is_not_a_descendant() {
        // Well above the default pid_max, so it cannot exist.
        assert!(!is_same_or_descendant(u32::MAX - 1, std::process::id()));
    }
}


/// How often the polling backends ask what is focused. Fast enough to feel immediate on a key
/// press, slow enough that spawning a helper process is not a burden.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(300);

/// Drive a backend that has to be asked rather than one that reports on its own.
fn poll_loop(
    backend: Backend,
    current: Arc<RwLock<Option<FocusedWindow>>>,
    active: Arc<AtomicBool>,
    pid: Arc<AtomicU32>,
) {
    // Only claim to be tracking once something has actually answered. A compositor's socket can
    // be set in the environment while its CLI is missing from the image, and reporting "active"
    // in that case would offer the user an option that never resolves.
    let mut ever_answered = false;

    loop {
        if let Some(window) = probe(backend) {
            if !ever_answered {
                ever_answered = true;
                active.store(true, Ordering::Relaxed);
            }
            pid.store(window.pid, Ordering::Relaxed);
            *current.write() = Some(window);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn probe(backend: Backend) -> Option<FocusedWindow> {
    match backend {
        Backend::Hyprland => parse_hyprland(&run(&["hyprctl", "-j", "activewindow"])?),
        Backend::Niri => parse_niri(&run(&["niri", "msg", "-j", "focused-window"])?),
        Backend::Sway => parse_i3_tree(&run(&["swaymsg", "-t", "get_tree"])?),
        Backend::I3 => parse_i3_tree(&run(&["i3-msg", "-t", "get_tree"])?),
        Backend::X11 => probe_x11(),
        // Pushes to us instead; never polled.
        Backend::KWin => None,
    }
}

fn run(argv: &[&str]) -> Option<String> {
    let output = std::process::Command::new(argv[0])
        .args(&argv[1..])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// `hyprctl -j activewindow` -> `{"pid":123,"class":"firefox","title":"..."}`
fn parse_hyprland(json: &str) -> Option<FocusedWindow> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let pid = value.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let class = value
        .get("class")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    // Hyprland answers with an empty object when nothing is focused.
    if pid == 0 && class.is_empty() {
        return None;
    }
    Some(FocusedWindow {
        pid,
        class: class.to_string(),
        caption: value
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        desktop_file: String::new(),
    })
}

/// `niri msg -j focused-window` -> `{"pid":123,"app_id":"firefox","title":"..."}`
fn parse_niri(json: &str) -> Option<FocusedWindow> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    if value.is_null() {
        return None;
    }
    let pid = value.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let class = value
        .get("app_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if pid == 0 && class.is_empty() {
        return None;
    }
    Some(FocusedWindow {
        pid,
        class: class.to_string(),
        caption: value
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        desktop_file: String::new(),
    })
}

/// Walk a sway/i3 tree for the node holding focus.
///
/// Both report the whole tree rather than the focused window, so the node with `"focused": true`
/// has to be found. i3 has no `app_id` (that is a Wayland concept) and reports `window_properties`
/// instead, so both shapes are accepted.
fn parse_i3_tree(json: &str) -> Option<FocusedWindow> {
    let root: serde_json::Value = serde_json::from_str(json).ok()?;
    let node = find_focused_node(&root)?;

    let class = node
        .get("app_id")
        .and_then(|v| v.as_str())
        .or_else(|| {
            node.get("window_properties")
                .and_then(|p| p.get("class"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or_default();

    Some(FocusedWindow {
        pid: node.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        class: class.to_string(),
        caption: node
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        desktop_file: String::new(),
    })
}

fn find_focused_node(node: &serde_json::Value) -> Option<&serde_json::Value> {
    // Only leaves are real windows; a focused container would have no app to control.
    let is_leaf = node
        .get("nodes")
        .and_then(|n| n.as_array())
        .is_none_or(|n| n.is_empty());
    if node.get("focused").and_then(|v| v.as_bool()) == Some(true) && is_leaf {
        return Some(node);
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(children) = node.get(key).and_then(|n| n.as_array()) {
            for child in children {
                if let Some(found) = find_focused_node(child) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Read `_NET_ACTIVE_WINDOW` from the root window, then that window's pid and class.
///
/// A fresh connection per poll rather than a cached one: the X server may restart under us, and a
/// connection every 300ms costs far less than the reconnect handling a long-lived one would need.
fn probe_x11() -> Option<FocusedWindow> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

    let (conn, screen_num) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots.get(screen_num)?.root;

    let atom = |name: &str| -> Option<u32> {
        conn.intern_atom(false, name.as_bytes())
            .ok()?
            .reply()
            .ok()
            .map(|r| r.atom)
    };

    let active_atom = atom("_NET_ACTIVE_WINDOW")?;
    let reply = conn
        .get_property(false, root, active_atom, AtomEnum::WINDOW, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    let window = reply.value32()?.next()?;
    if window == 0 {
        return None;
    }

    let pid = atom("_NET_WM_PID")
        .and_then(|pid_atom| {
            conn.get_property(false, window, pid_atom, AtomEnum::CARDINAL, 0, 1)
                .ok()?
                .reply()
                .ok()
        })
        .and_then(|r| r.value32()?.next())
        .unwrap_or(0);

    // WM_CLASS is two NUL-separated strings, instance then class; the second is the useful one.
    let class = conn
        .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| {
            let parts: Vec<&[u8]> = r.value.split(|b| *b == 0).collect();
            parts
                .get(1)
                .or_else(|| parts.first())
                .map(|p| String::from_utf8_lossy(p).into_owned())
                .unwrap_or_default()
        })
        .unwrap_or_default();

    let caption = conn
        .get_property(false, window, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 256)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| String::from_utf8_lossy(&r.value).into_owned())
        .unwrap_or_default();

    if pid == 0 && class.is_empty() {
        return None;
    }
    Some(FocusedWindow {
        pid,
        class,
        caption,
        desktop_file: String::new(),
    })
}
