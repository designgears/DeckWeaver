use image::{imageops::FilterType, ImageEncoder};
use resvg::{tiny_skia, usvg};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

const DEFAULT_ICON_SIZE: u32 = 200;
const MIN_ICON_SIZE: u32 = 200;

pub fn load_icon_to_png_bytes(path: &str) -> Option<Vec<u8>> {
    load_icon_inner(path, true)
}

/// Same, but leaves a raster icon at its own resolution.
///
/// The renderer resizes to the slot size itself, so pre-scaling here only inserts a second
/// resample. Going 48px up to 200 and straight back down to 72 turns a small icon to mush, and it
/// shows most on a button, where the icon is drawn at twice the size a knob draws it. One resample
/// from the original is always sharper than two.
pub fn load_icon_native_png_bytes(path: &str) -> Option<Vec<u8>> {
    load_icon_inner(path, false)
}

fn load_icon_inner(path: &str, upscale_small: bool) -> Option<Vec<u8>> {
    let path_obj = Path::new(path);

    if !path_obj.exists() {
        return None;
    }

    if path_obj
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("svg"))
        .unwrap_or(false)
    {
        // Vector art has no native pixel size; rasterising at a generous fixed size is real
        // detail rather than invented, so it is left alone.
        return load_svg_to_png(path);
    }

    load_image_to_png(path, upscale_small)
}

pub fn svg_data_to_png_bytes(svg_data: &[u8]) -> Option<Vec<u8>> {
    let opt = usvg::Options::default();
    let tree = match usvg::Tree::from_data(svg_data, &opt) {
        Ok(tree) => tree,
        Err(e) => {
            tracing::warn!("Failed to parse SVG data: {}", e);
            return None;
        }
    };

    let size = tree.size();
    let (target_width, target_height, scale_x, scale_y) =
        if size.width() > 0.0 && size.height() > 0.0 {
            let max_dim = size.width().max(size.height());
            let scale = DEFAULT_ICON_SIZE as f32 / max_dim;
            let tw = (size.width() * scale) as u32;
            let th = (size.height() * scale) as u32;
            let sx = tw as f32 / size.width();
            let sy = th as f32 / size.height();
            (tw, th, sx, sy)
        } else {
            (DEFAULT_ICON_SIZE, DEFAULT_ICON_SIZE, 1.0, 1.0)
        };

    let mut pixmap = match tiny_skia::Pixmap::new(target_width, target_height) {
        Some(pixmap) => pixmap,
        None => {
            tracing::warn!("Failed to create pixmap for SVG");
            return None;
        }
    };

    let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    match pixmap.encode_png() {
        Ok(png_data) => Some(png_data),
        Err(e) => {
            tracing::warn!("Failed to encode SVG as PNG: {}", e);
            None
        }
    }
}

fn load_svg_to_png(path: &str) -> Option<Vec<u8>> {
    let svg_data = match fs::read(path) {
        Ok(data) => data,
        Err(e) => {
            tracing::warn!("Failed to read SVG file {}: {}", path, e);
            return None;
        }
    };
    svg_data_to_png_bytes(&svg_data)
}

fn load_image_to_png(path: &str, upscale_small: bool) -> Option<Vec<u8>> {
    let img = match image::open(path) {
        Ok(img) => img,
        Err(e) => {
            tracing::warn!("Failed to load image {}: {}", path, e);
            return None;
        }
    };

    let rgba_img = img.to_rgba8();
    let (width, height) = rgba_img.dimensions();
    let max_dim = width.max(height);

    let final_img = if upscale_small && max_dim < MIN_ICON_SIZE {
        let scale = MIN_ICON_SIZE as f32 / max_dim as f32;
        let new_width = (width as f32 * scale) as u32;
        let new_height = (height as f32 * scale) as u32;
        image::imageops::resize(&rgba_img, new_width, new_height, FilterType::Triangle)
    } else {
        rgba_img
    };

    let mut png_data = Vec::new();
    {
        let encoder = image::codecs::png::PngEncoder::new(&mut png_data);
        if let Err(e) = encoder.write_image(
            &final_img,
            final_img.width(),
            final_img.height(),
            image::ColorType::Rgba8.into(),
        ) {
            tracing::warn!("Failed to encode image as PNG {}: {}", path, e);
            return None;
        }
    }

    Some(png_data)
}

/// Resolve an icon name (as apps report in `application.icon_name`, or as a `.desktop` entry's
/// `Icon=` key) to a file on disk.
///
/// Deliberately not a full XDG theme implementation: there is no theme inheritance or index.theme
/// parsing here. It walks the standard data directories and takes the best match by size, which is
/// enough for a 72px key and avoids pulling in a dependency for it.
///
/// Flatpak apps are the case worth calling out. Their icons are not in the system theme at all —
/// they live under each installation's `exports/share/icons`, which is why a naive
/// `/usr/share/icons` lookup finds nothing for them. Those roots are searched explicitly.
pub fn find_icon_by_name(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }

    // Some apps report an absolute path rather than a theme name.
    if name.starts_with('/') && std::path::Path::new(name).is_file() {
        return Some(name.to_string());
    }

    // hicolor first, and only fall back to other themes if it has nothing. Decorative themes
    // ship monochrome glyph versions of common apps at tiny sizes; scored head-to-head, one of
    // those scalable glyphs beats a proper 256px application icon, which is the wrong answer for
    // a full-colour key.
    let roots = icon_roots();
    let mut best: Option<(u32, String)> = None;
    for root in &roots {
        collect_best_icon(&root.join("hicolor"), name, &mut best);
    }
    if best.is_none() {
        for root in &roots {
            collect_other_themes(root, name, &mut best);
        }
    }

    // Loose icons that never made it into a theme directory.
    for dir in ["/usr/share/pixmaps", "/usr/local/share/pixmaps"] {
        for ext in ["svg", "png", "xpm"] {
            let candidate = std::path::Path::new(dir).join(format!("{name}.{ext}"));
            if candidate.is_file() {
                let score = if ext == "svg" { 2 } else { 1 };
                if best.as_ref().is_none_or(|(s, _)| score > *s) {
                    best = Some((score, candidate.to_string_lossy().into_owned()));
                }
            }
        }
    }

    best.map(|(_, path)| path)
}

/// True scalable directories sort above any plausible pixel dimension.
const SCALABLE_SCORE: u32 = 100_000;


fn icon_roots() -> Vec<std::path::PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let data_home = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{home}/.local/share"));

    let mut roots = vec![
        std::path::PathBuf::from(format!("{data_home}/icons")),
        std::path::PathBuf::from(format!("{home}/.icons")),
        // Flatpak exports, per-user and system-wide.
        std::path::PathBuf::from(format!("{data_home}/flatpak/exports/share/icons")),
        std::path::PathBuf::from("/var/lib/flatpak/exports/share/icons"),
        std::path::PathBuf::from("/usr/share/icons"),
        std::path::PathBuf::from("/usr/local/share/icons"),
    ];

    // Honour XDG_DATA_DIRS so Nix, Snap and other prefixes are covered too.
    if let Ok(dirs) = std::env::var("XDG_DATA_DIRS") {
        for dir in dirs.split(':').filter(|d| !d.is_empty()) {
            roots.push(std::path::PathBuf::from(dir).join("icons"));
        }
    }

    roots.retain(|root| root.is_dir());
    roots
}

/// Search every theme in a root except hicolor, which gets its own earlier pass.
fn collect_other_themes(root: &std::path::Path, name: &str, best: &mut Option<(u32, String)>) {
    let Ok(themes) = std::fs::read_dir(root) else {
        return;
    };
    for theme in themes.flatten() {
        if theme.file_name().eq_ignore_ascii_case("hicolor") {
            continue;
        }
        let path = theme.path();
        if path.is_dir() {
            search_theme(&path, name, 0, 0, 0, best);
        }
    }
}

/// Walk one theme directory looking for `<name>.{svg,png}`, keeping the best match.
///
/// Themes disagree about directory order: hicolor uses `<size>/<category>/`, while others (the
/// `char-*` themes shipped here, for one) use `<category>/<size>/`. So the size is taken from
/// whichever path component parses as one, rather than from a fixed depth.
fn collect_best_icon(theme_dir: &std::path::Path, name: &str, best: &mut Option<(u32, String)>) {
    if theme_dir.is_dir() {
        search_theme(theme_dir, name, 0, 0, 0, best);
    }
}

/// Recurse a theme looking for the icon, carrying the best size seen along the path.
fn search_theme(
    dir: &std::path::Path,
    name: &str,
    theme_bonus: u32,
    size_so_far: u32,
    depth: u32,
    best: &mut Option<(u32, String)>,
) {
    // hicolor is <size>/<category>/icon.png; nothing legitimate nests deeper than this.
    if depth > 3 {
        return;
    }

    for ext in ["svg", "png"] {
        let candidate = dir.join(format!("{name}.{ext}"));
        if candidate.is_file() {
            // An svg only breaks ties. Awarding it an automatic win let a 16px monochrome
            // scalable icon beat a proper 256px app icon.
            let bonus = if ext == "svg" { 1 } else { 0 };
            let score = size_so_far + theme_bonus + bonus;
            if best.as_ref().is_none_or(|(s, _)| score > *s) {
                *best = Some((score, candidate.to_string_lossy().into_owned()));
            }
        }
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // Take the size from whichever component carries one, so both directory layouts work.
        let size = size_score(&entry.file_name().to_string_lossy());
        search_theme(
            &path,
            name,
            theme_bonus,
            size_so_far.max(size),
            depth + 1,
            best,
        );
    }
}

/// Rank a theme size directory. "scalable" wins; "48x48" scores 48; anything else scores 0.
fn size_score(dir_name: &str) -> u32 {
    if dir_name.eq_ignore_ascii_case("scalable") {
        return SCALABLE_SCORE;
    }
    dir_name
        .split(['x', 'X'])
        .next()
        .and_then(|n| n.parse::<u32>().ok())
        // Oversized icons cost more to decode than a 72px key can use, so they are not preferred.
        .map(|px| if px > 512 { 0 } else { px })
        .unwrap_or(0)
}

#[cfg(test)]
mod icon_name_tests {
    use super::*;

    #[test]
    fn scalable_outranks_any_raster_size() {
        assert!(size_score("scalable") > size_score("512x512"));
        assert!(size_score("256x256") > size_score("48x48"));
    }

    #[test]
    fn unparseable_size_dirs_score_zero() {
        assert_eq!(size_score("symbolic"), 0);
        assert_eq!(size_score(""), 0);
    }

    #[test]
    fn absurd_sizes_are_not_preferred() {
        assert_eq!(size_score("1024x1024"), 0);
    }

    #[test]
    fn empty_name_resolves_to_nothing() {
        assert!(find_icon_by_name("").is_none());
        assert!(find_icon_by_name("   ").is_none());
    }
}

/// Resolve an icon for an application, given whatever the sound server told us about it.
///
/// `application.icon_name` is the happy path, but plenty of apps never set it — every Flatpak
/// checked here included. For those, the `.desktop` entry is the authority: it is what the app
/// ships specifically to say "this is my name and this is my icon". Flatpak Firefox is the
/// worked example — it reports name "Firefox" and binary "firefox-bin", sets no icon name, and
/// exports its icon as `org.mozilla.firefox`, a string that appears nowhere in its audio
/// properties. Only the desktop entry connects the two.
pub fn find_icon_for_app(
    icon_name: Option<&str>,
    app_name: &str,
    binary: Option<&str>,
    steam_app_id: Option<&str>,
) -> Option<String> {
    // The render loop asks once per frame per action; walking the icon and application
    // directories at that rate would be gratuitous. Results are stable for a session.
    static CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<String, Option<String>>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let cache_key = format!(
        "{}\u{1}{}\u{1}{}\u{1}{}",
        icon_name.unwrap_or_default(),
        app_name,
        binary.unwrap_or_default(),
        steam_app_id.unwrap_or_default()
    );

    if let Ok(guard) = cache.lock()
        && let Some(hit) = guard.get(&cache_key)
    {
        return hit.clone();
    }

    // The desktop entry is tried first: an app that lies about its name lies about its icon too
    // (Vesktop advertises "chromium-browser"), and the entry is the one thing it ships to describe
    // itself accurately. `application.icon_name` is the fallback for apps with no entry at all.
    let resolved = icon_from_desktop_entry(app_name, binary)
        .or_else(|| steam_app_id.and_then(steam_artwork))
        .or_else(|| icon_name.and_then(find_icon_by_name))
        // Last resort: some apps do use their binary as the icon name.
        .or_else(|| binary.and_then(find_icon_by_name));

    if let Ok(mut guard) = cache.lock() {
        guard.insert(cache_key, resolved.clone());
    }
    resolved
}

/// Artwork Steam keeps for an installed app, best first.
///
/// Steam games ship no desktop entry and set no `application.icon_name`, so without this they
/// render iconless. Three sources, in order of how well they read on a key:
///
/// 1. `steam_icon_<appid>` in the icon theme — only exists when the user made a desktop
///    shortcut, but then it is a proper square icon at up to 256px.
/// 2. The library capsule (portrait box art). Not square, but the renderer letterboxes it, and
///    at 300x450 it stays sharp where the actual icon would not.
/// 3. The icon Steam itself shows in lists — typically 32px, legible but soft when scaled up.
fn steam_artwork(app_id: &str) -> Option<String> {
    if let Some(icon) = find_icon_by_name(&format!("steam_icon_{app_id}")) {
        return Some(icon);
    }

    let mut icon_fallback = None;
    for root in steam_roots() {
        let cache = root.join("appcache/librarycache");

        // Current layout: one directory per app, art in hash-named subdirectories with stable
        // filenames, and the list icon as a hash-named jpg at the top level.
        let app_dir = cache.join(app_id);
        if let Ok(entries) = std::fs::read_dir(&app_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let capsule = path.join("library_capsule.jpg");
                    if capsule.is_file() {
                        return Some(capsule.to_string_lossy().into_owned());
                    }
                } else if icon_fallback.is_none()
                    && path.extension().is_some_and(|e| e == "jpg")
                {
                    icon_fallback = Some(path.to_string_lossy().into_owned());
                }
            }
        }

        // Pre-2024 layout: flat files named by appid.
        for name in [
            format!("{app_id}_library_600x900.jpg"),
            format!("{app_id}_icon.jpg"),
        ] {
            let path = cache.join(name);
            if path.is_file() {
                return Some(path.to_string_lossy().into_owned());
            }
        }
    }
    icon_fallback
}

/// Steam installation roots worth checking: native, the classic `~/.steam` link, and Flatpak.
fn steam_roots() -> Vec<std::path::PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let data_home =
        std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{home}/.local/share"));

    let mut roots = vec![
        std::path::PathBuf::from(format!("{data_home}/Steam")),
        std::path::PathBuf::from(format!("{home}/.steam/steam")),
        std::path::PathBuf::from(format!(
            "{home}/.var/app/com.valvesoftware.Steam/.local/share/Steam"
        )),
    ];
    roots.retain(|root| root.is_dir());
    roots
}

/// Find the `.desktop` entry belonging to this app and resolve its `Icon=`.
fn icon_from_desktop_entry(app_name: &str, binary: Option<&str>) -> Option<String> {
    let entry = best_desktop_entry(app_name, binary)?;
    find_icon_by_name(&entry.icon)
}

/// Desktop entry id for an app, e.g. "org.mozilla.firefox".
///
/// This is the strongest identity a sandboxed app has. Its pid is from another namespace and its
/// binary name need not resemble anything the compositor reports, but the desktop entry is the
/// one name both sides agree on: the compositor knows which entry launched a window, and the
/// entry is what we can match audio properties against.
pub fn find_desktop_id_for_app(app_name: &str, binary: Option<&str>) -> Option<String> {
    best_desktop_entry(app_name, binary).map(|entry| entry.id)
}

/// Display name from an app\'s desktop entry, which is what the app calls itself to the user.
///
/// Preferred over `application.name` because Electron apps routinely report the toolkit rather
/// than themselves — Vesktop shows up as "Chromium".
pub fn find_desktop_name_for_app(app_name: &str, binary: Option<&str>) -> Option<String> {
    best_desktop_entry(app_name, binary).and_then(|entry| entry.name)
}

/// Best-matching desktop entry for an app, as (id, icon name).
///
/// Cached: the focus matcher asks for this on every rendered frame, and the answer is a directory
/// scan that does not change during a session.
fn best_desktop_entry(app_name: &str, binary: Option<&str>) -> Option<DesktopEntry> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<HashMap<String, Option<DesktopEntry>>>,
    > = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let cache_key = format!("{app_name}\u{1}{}", binary.unwrap_or_default());

    if let Ok(guard) = cache.lock()
        && let Some(hit) = guard.get(&cache_key)
    {
        return hit.clone();
    }
    let found = scan_desktop_entries(app_name, binary);
    if let Ok(mut guard) = cache.lock() {
        guard.insert(cache_key, found.clone());
    }
    found
}

/// A matched desktop entry: its id (file stem), icon name, and display name.
#[derive(Clone)]
struct DesktopEntry {
    id: String,
    icon: String,
    name: Option<String>,
}

fn scan_desktop_entries(app_name: &str, binary: Option<&str>) -> Option<DesktopEntry> {
    let app_name = app_name.trim().to_lowercase();
    let binary = binary.map(|b| b.trim().to_lowercase());
    // "firefox-bin" is the process; "firefox" is what the desktop entry calls itself. Trying the
    // stripped form as well costs nothing and covers the whole -bin/-browser family.
    let binary_stem = binary.as_deref().and_then(|b| {
        b.strip_suffix("-bin")
            .or_else(|| b.strip_suffix("-browser"))
            .map(|s| s.to_string())
    });

    let mut best: Option<(u32, DesktopEntry)> = None;
    for dir in desktop_entry_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "desktop") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };

            let Some(icon) = desktop_key(&text, "Icon") else {
                continue;
            };
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let name = desktop_key(&text, "Name").map(|n| n.to_lowercase());
            let wm_class = desktop_key(&text, "StartupWMClass").map(|w| w.to_lowercase());

            // Ranked strongest signal first: the window class an app declares is the most
            // deliberate, its display name next, the filename last.
            //
            // Every comparison requires the entry's side to be present. Comparing two `Option`s
            // directly made `None == None` a match, so any entry without a StartupWMClass matched
            // any app without one — which is most of both, and handed out the first icon it found
            // to everything.
            let Some(score) = entry_score(
                &stem,
                name.as_deref(),
                wm_class.as_deref(),
                &app_name,
                binary.as_deref(),
                binary_stem.as_deref(),
            ) else {
                continue;
            };

            if best.as_ref().is_none_or(|(s, _)| score > *s) {
                best = Some((
                    score,
                    DesktopEntry {
                        id: stem,
                        icon,
                        // Keep the original casing for display; `name` above is lowercased for
                        // comparison only.
                        name: desktop_key(&text, "Name"),
                    },
                ));
            }
        }
    }

    best.map(|(_, entry)| entry)
}

/// Rank how well a desktop entry identifies an app, or `None` if it does not match at all.
///
/// Everything derived from the binary outranks everything derived from the name the app reports.
/// Electron and Chromium-embedding apps report the toolkit rather than themselves — Vesktop calls
/// itself "Chromium" and advertises "chromium-browser" as its icon — so trusting the reported name
/// hands them Chromium\'s entry, and with it Chromium\'s name and icon. The binary ("vesktop") is
/// the part they get right.
///
/// Comparisons require the entry\'s side to be present: comparing two `Option`s made `None == None`
/// a match, so any entry without a StartupWMClass once matched any app without one.
fn entry_score(
    stem: &str,
    name: Option<&str>,
    wm_class: Option<&str>,
    app_name: &str,
    binary: Option<&str>,
    binary_stem: Option<&str>,
) -> Option<u32> {
    let matches_binary =
        |value: &str| Some(value) == binary || Some(value) == binary_stem;

    if wm_class.is_some_and(matches_binary) {
        return Some(4);
    }
    if matches_binary(stem) {
        return Some(3);
    }
    if wm_class == Some(app_name) {
        return Some(2);
    }
    if name == Some(app_name) {
        return Some(1);
    }
    None
}

/// First value for `key` in a desktop entry, ignoring localised variants (`Name[de]=`).
fn desktop_key(text: &str, key: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find_map(|line| {
            let rest = line.strip_prefix(key)?;
            let value = rest.strip_prefix('=')?;
            Some(value.trim().to_string())
        })
        .filter(|value| !value.is_empty())
}

fn desktop_entry_dirs() -> Vec<std::path::PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let data_home =
        std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{home}/.local/share"));

    let mut dirs = vec![
        std::path::PathBuf::from(format!("{data_home}/applications")),
        // Flatpak exports, per-user and system-wide.
        std::path::PathBuf::from(format!("{data_home}/flatpak/exports/share/applications")),
        std::path::PathBuf::from("/var/lib/flatpak/exports/share/applications"),
        std::path::PathBuf::from("/usr/share/applications"),
        std::path::PathBuf::from("/usr/local/share/applications"),
    ];
    if let Ok(extra) = std::env::var("XDG_DATA_DIRS") {
        for dir in extra.split(':').filter(|d| !d.is_empty()) {
            dirs.push(std::path::PathBuf::from(dir).join("applications"));
        }
    }
    dirs.retain(|dir| dir.is_dir());
    dirs
}

#[cfg(test)]
mod desktop_entry_tests {
    use super::*;

    const ENTRY: &str = "\
[Desktop Entry]
Name=Firefox
Name[de]=Feuerfuchs
Exec=/usr/bin/flatpak run org.mozilla.firefox
Icon=org.mozilla.firefox
StartupWMClass=firefox
";

    #[test]
    fn reads_plain_keys() {
        assert_eq!(desktop_key(ENTRY, "Icon").as_deref(), Some("org.mozilla.firefox"));
        assert_eq!(desktop_key(ENTRY, "StartupWMClass").as_deref(), Some("firefox"));
    }

    /// `Name=` must win over `Name[de]=`, which would otherwise be picked up by a prefix match.
    #[test]
    fn localised_variants_do_not_shadow_the_plain_key() {
        assert_eq!(desktop_key(ENTRY, "Name").as_deref(), Some("Firefox"));
    }

    /// The reported bug: Vesktop calls itself "Chromium", so a name-first ranking gave it
    /// Chromium\'s entry — and therefore Chromium\'s name and icon.
    #[test]
    fn the_binary_outranks_a_reported_name() {
        let vesktop = entry_score("vesktop", Some("vesktop"), Some("vesktop"), "chromium", Some("vesktop"), None);
        let chromium = entry_score("chromium", Some("chromium"), Some("chromium"), "chromium", Some("vesktop"), None);
        assert!(vesktop > chromium, "the app\'s own binary must win over the name it reports");
    }

    /// Flatpak Firefox is binary "firefox-bin" against a "firefox" entry.
    #[test]
    fn a_bin_suffixed_binary_still_matches_its_entry() {
        assert_eq!(
            entry_score("firefox", Some("firefox"), None, "firefox", Some("firefox-bin"), Some("firefox")),
            Some(3)
        );
    }

    #[test]
    fn an_unrelated_entry_does_not_match() {
        assert_eq!(
            entry_score("gimp", Some("gimp"), Some("gimp"), "chromium", Some("vesktop"), None),
            None
        );
    }

    /// Two absent fields must not be treated as agreeing with each other.
    #[test]
    fn absent_fields_do_not_match_each_other() {
        assert_eq!(entry_score("gimp", None, None, "chromium", None, None), None);
    }

    #[test]
    fn missing_and_empty_keys_are_none() {
        assert!(desktop_key(ENTRY, "NoSuchKey").is_none());
        assert!(desktop_key("[Desktop Entry]\nIcon=\n", "Icon").is_none());
    }
}

/// Pick a colour from an icon to tint the volume bar with.
///
/// Not a mean of all pixels: averaging an icon returns mud, because icons are mostly transparent
/// padding plus black or white detail. Pixels are bucketed by coarse colour, and the heaviest
/// bucket wins — so the result is the icon's brand colour rather than whatever covers the most
/// area. A white glyph on a coloured field gives the field's colour, not white.
///
/// Weighting is by *chroma*, not saturation. Saturation is chroma over brightness, which rates a
/// near-black deep colour just as highly as a vivid one — Steam's navy scores 0.87 saturation
/// while being far too dark to read on the bar. Chroma rises with both colourfulness and
/// brightness, so the vivid part of an icon outvotes its shadows and the colour picked is one
/// that needs no artificial lifting to be legible.
pub fn dominant_accent(image: &image::RgbaImage) -> Option<(u8, u8, u8)> {
    // Two passes: the first ignores washed-out pixels entirely, and if an icon turns out to be
    // wholly desaturated (a greyscale logo) the second accepts them so it still gets a tint.
    for min_saturation in [0.35_f32, 0.12, 0.0] {
        if let Some(colour) = accent_pass(image, min_saturation) {
            return Some(colour);
        }
    }
    None
}

fn accent_pass(image: &image::RgbaImage, min_saturation: f32) -> Option<(u8, u8, u8)> {
    // 4 bits per channel: coarse enough that shading variations of one colour land together,
    // fine enough to keep genuinely different colours apart.
    let mut buckets: HashMap<(u8, u8, u8), (f32, f64, f64, f64)> = HashMap::new();

    for pixel in image.pixels() {
        let [r, g, b, a] = pixel.0;
        if a < 128 {
            continue;
        }
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        // Too dark to tell a colour from, or too pale to read against the bar's track.
        if max < 40 {
            continue;
        }
        let chroma = (max - min) as f32;
        // Saturation still decides *whether* a pixel counts as coloured at all; it is the right
        // test for "is this grey?" even though it is the wrong weight.
        let saturation = chroma / max as f32;
        if saturation < min_saturation {
            continue;
        }

        let key = (r >> 4, g >> 4, b >> 4);
        let entry = buckets.entry(key).or_insert((0.0, 0.0, 0.0, 0.0));
        // Chroma favours bright vivid pixels over dark ones of the same hue. The floor keeps the
        // greyscale fallback pass working, where every pixel has zero chroma by definition.
        let weight = (chroma / 255.0).max(0.02);
        entry.0 += weight;
        entry.1 += r as f64 * weight as f64;
        entry.2 += g as f64 * weight as f64;
        entry.3 += b as f64 * weight as f64;
    }

    let (_, (weight, r, g, b)) = buckets
        .into_iter()
        .max_by(|a, b| a.1 .0.total_cmp(&b.1 .0))?;
    if weight <= 0.0 {
        return None;
    }

    Some(brighten_for_bar((
        (r / weight as f64) as u8,
        (g / weight as f64) as u8,
        (b / weight as f64) as u8,
    )))
}

/// Lift a colour that would disappear against the bar's dark track.
///
/// Icons carry plenty of deep colours that are legible on a white page but turn to mud against the
/// bar's dark track — Steam's navy is the obvious one. Anything below the floor is scaled up with
/// its hue preserved, so it stays recognisably the app's colour while becoming readable.
fn brighten_for_bar((r, g, b): (u8, u8, u8)) -> (u8, u8, u8) {
    const FLOOR: u16 = 155;
    let max = r.max(g).max(b) as u16;
    if max >= FLOOR || max == 0 {
        return (r, g, b);
    }
    let scale = FLOOR as f32 / max as f32;
    let lift = |c: u8| ((c as f32 * scale).round() as u16).min(255) as u8;
    (lift(r), lift(g), lift(b))
}

#[cfg(test)]
mod accent_tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn image_from(pixels: &[(u8, u8, u8, u8)]) -> RgbaImage {
        let mut img = RgbaImage::new(pixels.len() as u32, 1);
        for (x, p) in pixels.iter().enumerate() {
            img.put_pixel(x as u32, 0, Rgba([p.0, p.1, p.2, p.3]));
        }
        img
    }

    /// The shape most app icons have: a coloured mark on transparent padding.
    #[test]
    fn a_coloured_mark_on_transparency_wins() {
        let img = image_from(&[
            (0, 0, 0, 0),
            (0, 0, 0, 0),
            (220, 40, 40, 255),
            (220, 40, 40, 255),
        ]);
        let (r, g, b) = dominant_accent(&img).expect("an accent");
        assert!(r > 180 && g < 90 && b < 90, "expected red, got {r},{g},{b}");
    }

    /// A white glyph over a coloured field must yield the field, not the glyph — averaging or
    /// counting by area would return white here.
    #[test]
    fn a_white_glyph_does_not_beat_the_colour_behind_it() {
        let mut pixels = vec![(255, 255, 255, 255); 6];
        pixels.extend(vec![(30, 100, 220, 255); 4]);
        let (r, g, b) = dominant_accent(&image_from(&pixels)).expect("an accent");
        assert!(b > 150 && r < 100, "expected blue, got {r},{g},{b}");
    }

    /// Given a dark and a bright shade of the same hue, the bright one should be sampled. Picking
    /// the dark one and lifting it afterwards shifts the hue and is less faithful to the icon.
    /// Saturation-weighting got this backwards: Steam's navy rates 0.87 saturation.
    #[test]
    fn a_bright_shade_beats_a_dark_shade_of_the_same_hue() {
        // Deliberately more dark pixels than bright, so area alone would pick the dark shade.
        let mut pixels = vec![(14, 48, 110, 255); 6];
        pixels.extend(vec![(60, 150, 240, 255); 4]);
        let (_, g, b) = dominant_accent(&image_from(&pixels)).expect("an accent");
        assert!(
            g > 100 && b > 200,
            "expected the bright blue to win, got g={g} b={b}"
        );
    }

    /// A deep colour that is genuinely the only one present still has to be lifted, or it vanishes
    /// against the bar's dark track.
    #[test]
    fn very_dark_colours_are_lifted_to_stay_visible() {
        let img = image_from(&[(0, 0, 60, 255); 4]);
        let (_, _, b) = dominant_accent(&img).expect("an accent");
        assert!(b >= 150, "expected the blue to be lifted clear of the track, got {b}");
    }

    /// A greyscale logo should still tint rather than falling back to the default.
    #[test]
    fn a_greyscale_icon_still_produces_something() {
        let img = image_from(&[(180, 180, 180, 255); 4]);
        assert!(dominant_accent(&img).is_some());
    }

    #[test]
    fn a_fully_transparent_icon_has_no_accent() {
        let img = image_from(&[(255, 0, 0, 0); 4]);
        assert!(dominant_accent(&img).is_none());
    }
}
