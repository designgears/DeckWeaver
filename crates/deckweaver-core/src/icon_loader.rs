use image::{imageops::FilterType, ImageEncoder};
use resvg::{tiny_skia, usvg};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

const DEFAULT_ICON_SIZE: u32 = 200;
const MIN_ICON_SIZE: u32 = 200;

pub fn load_icon_to_png_bytes(path: &str) -> Option<Vec<u8>> {
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
        return load_svg_to_png(path);
    }

    load_image_to_png(path)
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

fn load_image_to_png(path: &str) -> Option<Vec<u8>> {
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

    let final_img = if max_dim < MIN_ICON_SIZE {
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
) -> Option<String> {
    // The render loop asks once per frame per action; walking the icon and application
    // directories at that rate would be gratuitous. Results are stable for a session.
    static CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<String, Option<String>>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let cache_key = format!(
        "{}\u{1}{}\u{1}{}",
        icon_name.unwrap_or_default(),
        app_name,
        binary.unwrap_or_default()
    );

    if let Ok(guard) = cache.lock()
        && let Some(hit) = guard.get(&cache_key)
    {
        return hit.clone();
    }

    let resolved = icon_name
        .and_then(find_icon_by_name)
        .or_else(|| icon_from_desktop_entry(app_name, binary))
        // Last resort: some apps do use their binary as the icon name.
        .or_else(|| binary.and_then(find_icon_by_name));

    if let Ok(mut guard) = cache.lock() {
        guard.insert(cache_key, resolved.clone());
    }
    resolved
}

/// Find the `.desktop` entry belonging to this app and resolve its `Icon=`.
fn icon_from_desktop_entry(app_name: &str, binary: Option<&str>) -> Option<String> {
    let (_, icon) = best_desktop_entry(app_name, binary)?;
    find_icon_by_name(&icon)
}

/// Desktop entry id for an app, e.g. "org.mozilla.firefox".
///
/// This is the strongest identity a sandboxed app has. Its pid is from another namespace and its
/// binary name need not resemble anything the compositor reports, but the desktop entry is the
/// one name both sides agree on: the compositor knows which entry launched a window, and the
/// entry is what we can match audio properties against.
pub fn find_desktop_id_for_app(app_name: &str, binary: Option<&str>) -> Option<String> {
    best_desktop_entry(app_name, binary).map(|(id, _)| id)
}

/// Best-matching desktop entry for an app, as (id, icon name).
///
/// Cached: the focus matcher asks for this on every rendered frame, and the answer is a directory
/// scan that does not change during a session.
fn best_desktop_entry(app_name: &str, binary: Option<&str>) -> Option<(String, String)> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<HashMap<String, Option<(String, String)>>>,
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

fn scan_desktop_entries(app_name: &str, binary: Option<&str>) -> Option<(String, String)> {
    let app_name = app_name.trim().to_lowercase();
    let binary = binary.map(|b| b.trim().to_lowercase());
    // "firefox-bin" is the process; "firefox" is what the desktop entry calls itself. Trying the
    // stripped form as well costs nothing and covers the whole -bin/-browser family.
    let binary_stem = binary.as_deref().and_then(|b| {
        b.strip_suffix("-bin")
            .or_else(|| b.strip_suffix("-browser"))
            .map(|s| s.to_string())
    });

    let mut best: Option<(u32, String, String)> = None;
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
            let matches_class = wm_class.as_deref().is_some_and(|class| {
                Some(class) == binary.as_deref()
                    || Some(class) == binary_stem.as_deref()
                    || class == app_name
            });

            let score = if matches_class {
                3
            } else if name.as_deref() == Some(app_name.as_str()) {
                2
            } else if Some(stem.as_str()) == binary.as_deref()
                || Some(stem.as_str()) == binary_stem.as_deref()
            {
                1
            } else {
                continue;
            };

            if best.as_ref().is_none_or(|(s, _, _)| score > *s) {
                best = Some((score, stem, icon));
            }
        }
    }

    best.map(|(_, stem, icon)| (stem, icon))
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

    #[test]
    fn missing_and_empty_keys_are_none() {
        assert!(desktop_key(ENTRY, "NoSuchKey").is_none());
        assert!(desktop_key("[Desktop Entry]\nIcon=\n", "Icon").is_none());
    }
}
