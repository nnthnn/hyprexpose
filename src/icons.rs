use cairo::{Format, ImageSurface};
use std::collections::HashMap;
use std::path::Path;

/// Parses `gtk-icon-theme-name = "..."` out of a GTK settings.ini file. Scans
/// the whole text for a matching key, without regard to `[Settings]` section
/// boundaries.
pub(crate) fn parse_gtk_icon_theme(ini_text: &str) -> Option<String> {
    for line in ini_text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("gtk-icon-theme-name") {
            let rest = rest.trim_start();
            if let Some(value) = rest.strip_prefix('=') {
                let value = value.trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

/// Parses `IconThemeName`'s `value="..."` attribute out of an XFCE
/// `xsettings.xml` xfconf channel file. Scans line by line for a
/// `name="IconThemeName"` property and pulls its `value="..."` attribute.
pub(crate) fn parse_xfconf_icon_theme(xml_text: &str) -> Option<String> {
    for line in xml_text.lines() {
        if !line.contains("name=\"IconThemeName\"") {
            continue;
        }
        let key = "value=\"";
        if let Some(start) = line.find(key).map(|i| i + key.len()) {
            let rest = &line[start..];
            if let Some(end) = rest.find('"') {
                let value = &rest[..end];
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

/// Ordered list of icon names to try when looking up an app's icon: the raw
/// window class, then the cleaned (reversed-domain-stripped), lowercased name.
pub(crate) fn icon_name_candidates(class_name: &str) -> Vec<String> {
    let cleaned = crate::render::clean_app_name(class_name).to_lowercase();
    let mut candidates = vec![class_name.to_string()];
    if cleaned != class_name {
        candidates.push(cleaned);
    }
    candidates
}

/// Converts premultiplied RGBA bytes (as produced by tiny-skia) into Cairo's
/// ARgb32 byte order. Both formats are already premultiplied; only a byte
/// swap of R and B is needed. On little-endian machines, Cairo's ARgb32 is
/// stored as `[B, G, R, A]` per pixel, while tiny-skia provides `[R, G, B, A]`.
pub(crate) fn premultiplied_rgba_to_cairo_argb(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for px in rgba.chunks_exact(4) {
        let [r, g, b, a] = [px[0], px[1], px[2], px[3]];
        out.extend_from_slice(&[b, g, r, a]);
    }
    out
}

fn decode_png(path: &Path) -> Option<ImageSurface> {
    let mut file = std::fs::File::open(path).ok()?;
    ImageSurface::create_from_png(&mut file).ok()
}

fn decode_svg(path: &Path, size: u32) -> Option<ImageSurface> {
    let data = std::fs::read(path).ok()?;
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(&data, &opt).ok()?;

    let tree_size = tree.size();
    if tree_size.width() <= 0.0 || tree_size.height() <= 0.0 {
        return None;
    }

    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    let scale_x = size as f32 / tree_size.width();
    let scale_y = size as f32 / tree_size.height();
    let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let argb = premultiplied_rgba_to_cairo_argb(pixmap.data());
    let stride = size as i32 * 4;
    ImageSurface::create_for_data(argb, Format::ARgb32, size as i32, size as i32, stride).ok()
}

/// Parses the `Icon=` value out of a `.desktop` file's `[Desktop Entry]`
/// section only — ignoring any `Icon=` in later sections such as
/// `[Desktop Action ...]`.
pub(crate) fn parse_desktop_icon(desktop_text: &str) -> Option<String> {
    let mut in_desktop_entry = false;
    for line in desktop_text.lines() {
        let line = line.trim();
        if let Some(section) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_desktop_entry = section == "Desktop Entry";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }
        if let Some(value) = line.strip_prefix("Icon=") {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// XDG application directories to search for a `.desktop` file, in priority
/// order: `$XDG_DATA_HOME/applications` (default `~/.local/share/applications`),
/// then each entry of `$XDG_DATA_DIRS/applications` (default
/// `/usr/local/share/applications:/usr/share/applications`).
fn app_data_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();

    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".local/share")));
    if let Some(home) = data_home {
        dirs.push(home.join("applications"));
    }

    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    for dir in data_dirs.split(':').filter(|d| !d.is_empty()) {
        dirs.push(Path::new(dir).join("applications"));
    }

    dirs
}

/// Looks up a `.desktop` file named after any of `candidates` (tried in
/// order, across all `dirs`) and returns its declared `Icon=` name, if any.
fn find_desktop_icon_name(dirs: &[std::path::PathBuf], candidates: &[String]) -> Option<String> {
    for dir in dirs {
        for candidate in candidates {
            let path = dir.join(format!("{candidate}.desktop"));
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Some(icon) = parse_desktop_icon(&text) {
                    return Some(icon);
                }
            }
        }
    }
    None
}

fn detect_icon_theme() -> String {
    let gtk_paths = [
        std::env::var_os("XDG_CONFIG_HOME").map(|p| Path::new(&p).join("gtk-3.0/settings.ini")),
        dirs_home_config("gtk-3.0/settings.ini"),
    ];
    for path in gtk_paths.into_iter().flatten() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Some(theme) = parse_gtk_icon_theme(&text) {
                return theme;
            }
        }
    }

    // XFCE (and other xfconf-based desktops) store the active icon theme in
    // xfconf's xsettings channel file rather than gtk-3.0/settings.ini.
    let xfconf_paths = [
        std::env::var_os("XDG_CONFIG_HOME")
            .map(|p| Path::new(&p).join("xfce4/xfconf/xfce-perchannel-xml/xsettings.xml")),
        dirs_home_config("xfce4/xfconf/xfce-perchannel-xml/xsettings.xml"),
    ];
    for path in xfconf_paths.into_iter().flatten() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Some(theme) = parse_xfconf_icon_theme(&text) {
                return theme;
            }
        }
    }

    "Adwaita".to_string()
}

fn dirs_home_config(suffix: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|home| Path::new(&home).join(".config").join(suffix))
}

fn resolve_icon(candidates: &[String], theme: &str, size: u32) -> Option<ImageSurface> {
    for name in candidates {
        let path = freedesktop_icons::lookup(name)
            .with_theme(theme)
            .with_size(size as u16)
            .with_cache()
            .find()
            .or_else(|| {
                freedesktop_icons::lookup(name)
                    .with_theme("hicolor")
                    .with_size(size as u16)
                    .with_cache()
                    .find()
            });
        let Some(path) = path else { continue };
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let surface = if ext.eq_ignore_ascii_case("svg") {
            decode_svg(&path, size)
        } else {
            decode_png(&path)
        };
        if surface.is_some() {
            return surface;
        }
    }
    None
}

pub struct IconCache {
    theme: String,
    cache: HashMap<String, Option<ImageSurface>>,
}

impl IconCache {
    pub fn new() -> Self {
        Self {
            theme: detect_icon_theme(),
            cache: HashMap::new(),
        }
    }

    /// Returns the cached icon surface for `class_name` at `size` px,
    /// looking it up (and caching the result, including misses) on first access.
    pub fn get(&mut self, class_name: &str, size: u32) -> Option<ImageSurface> {
        if let Some(cached) = self.cache.get(class_name) {
            return cached.clone();
        }
        let mut candidates = icon_name_candidates(class_name);
        // An app's .desktop file Icon= is the authoritative icon name and
        // often differs from its window class (e.g. Thunar's class is
        // "Thunar" but its Icon= is "org.xfce.thunar") — try it first.
        if let Some(desktop_icon) = find_desktop_icon_name(&app_data_dirs(), &candidates) {
            if !candidates.contains(&desktop_icon) {
                candidates.insert(0, desktop_icon);
            }
        }
        let surface = resolve_icon(&candidates, &self.theme, size);
        self.cache.insert(class_name.to_string(), surface.clone());
        surface
    }
}

impl Default for IconCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_icon_theme_from_settings_ini() {
        let ini = "[Settings]\ngtk-theme-name=Adwaita\ngtk-icon-theme-name=Papirus-Dark\ngtk-font-name=Sans 10\n";
        assert_eq!(parse_gtk_icon_theme(ini), Some("Papirus-Dark".to_string()));
    }

    #[test]
    fn parses_icon_theme_with_spaces_around_equals() {
        let ini = "[Settings]\ngtk-icon-theme-name = Yaru\n";
        assert_eq!(parse_gtk_icon_theme(ini), Some("Yaru".to_string()));
    }

    #[test]
    fn skips_lookalike_line_without_equals_and_finds_real_key() {
        let ini = "gtk-icon-theme-name-extra=foo\ngtk-icon-theme-name=Papirus\n";
        assert_eq!(parse_gtk_icon_theme(ini), Some("Papirus".to_string()));
    }

    #[test]
    fn missing_icon_theme_key_returns_none() {
        let ini = "[Settings]\ngtk-theme-name=Adwaita\n";
        assert_eq!(parse_gtk_icon_theme(ini), None);
    }

    #[test]
    fn empty_text_returns_none() {
        assert_eq!(parse_gtk_icon_theme(""), None);
    }

    #[test]
    fn parses_icon_theme_from_xfconf_xsettings_xml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<channel name="xsettings" version="1.0">
  <property name="Net" type="empty">
    <property name="ThemeName" type="string" value="Adwaita-dark"/>
    <property name="IconThemeName" type="string" value="Papirus-Dark"/>
  </property>
</channel>
"#;
        assert_eq!(
            parse_xfconf_icon_theme(xml),
            Some("Papirus-Dark".to_string())
        );
    }

    #[test]
    fn missing_xfconf_icon_theme_property_returns_none() {
        let xml = r#"<channel name="xsettings" version="1.0"><property name="Net" type="empty"/></channel>"#;
        assert_eq!(parse_xfconf_icon_theme(xml), None);
    }

    #[test]
    fn parses_icon_from_desktop_entry_section() {
        let desktop = "[Desktop Entry]\nType=Application\nName=Thunar\nIcon=org.xfce.thunar\nExec=thunar %U\n";
        assert_eq!(
            parse_desktop_icon(desktop),
            Some("org.xfce.thunar".to_string())
        );
    }

    #[test]
    fn ignores_icon_key_outside_desktop_entry_section() {
        // "Icon=" in a [Desktop Action ...] section shouldn't be picked up
        // over the main entry's icon, and a file with no [Desktop Entry]
        // Icon= at all should return None.
        let desktop = "[Desktop Action new-window]\nIcon=window-new\nExec=thunar\n";
        assert_eq!(parse_desktop_icon(desktop), None);
    }

    #[test]
    fn desktop_entry_icon_takes_priority_over_later_action_icon() {
        let desktop = "[Desktop Entry]\nIcon=org.xfce.thunar\n\n[Desktop Action new-window]\nIcon=window-new\n";
        assert_eq!(
            parse_desktop_icon(desktop),
            Some("org.xfce.thunar".to_string())
        );
    }

    #[test]
    fn missing_icon_key_returns_none() {
        let desktop = "[Desktop Entry]\nName=Thunar\nExec=thunar %U\n";
        assert_eq!(parse_desktop_icon(desktop), None);
    }

    #[test]
    fn candidates_for_reversed_domain_class_name() {
        assert_eq!(
            icon_name_candidates("dev.zed.Zed"),
            vec!["dev.zed.Zed", "zed"]
        );
    }

    #[test]
    fn candidates_dedup_when_class_already_clean_and_lowercase() {
        // "alacritty" cleaned is still "alacritty" and already lowercase —
        // candidates should not contain the exact same string twice.
        assert_eq!(icon_name_candidates("alacritty"), vec!["alacritty"]);
    }

    #[test]
    fn swaps_r_and_b_keeps_g_and_a() {
        // Two pixels: opaque red, then translucent green.
        let rgba = [255u8, 0, 0, 255, 0, 128, 0, 64];
        let argb = premultiplied_rgba_to_cairo_argb(&rgba);
        assert_eq!(argb, vec![0, 0, 255, 255, 0, 128, 0, 64]);
    }

    #[test]
    fn empty_input_returns_empty_output() {
        assert_eq!(premultiplied_rgba_to_cairo_argb(&[]), Vec::<u8>::new());
    }

    #[test]
    fn decodes_svg_to_requested_pixel_size() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">
            <rect width="24" height="24" fill="#ff0000"/>
        </svg>"##;
        let mut path = std::env::temp_dir();
        path.push(format!("hyprexpose-icon-test-{}.svg", std::process::id()));
        std::fs::write(&path, svg).unwrap();

        let surface = decode_svg(&path, 20).expect("decode_svg should succeed");
        assert_eq!(surface.width(), 20);
        assert_eq!(surface.height(), 20);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn decode_svg_returns_none_for_missing_file() {
        let path = std::path::Path::new("/nonexistent/hyprexpose-icon-test.svg");
        assert!(decode_svg(path, 20).is_none());
    }

    #[test]
    fn find_desktop_icon_name_matches_by_filename_and_reads_icon() {
        let dir =
            std::env::temp_dir().join(format!("hyprexpose-desktop-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("thunar.desktop"),
            "[Desktop Entry]\nName=Thunar\nIcon=org.xfce.thunar\n",
        )
        .unwrap();

        let dirs = vec![dir.clone()];
        let candidates = vec!["Thunar".to_string(), "thunar".to_string()];
        assert_eq!(
            find_desktop_icon_name(&dirs, &candidates),
            Some("org.xfce.thunar".to_string())
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn find_desktop_icon_name_returns_none_when_no_file_matches() {
        let dirs = vec![std::path::PathBuf::from("/nonexistent/hyprexpose-apps")];
        let candidates = vec!["nope".to_string()];
        assert_eq!(find_desktop_icon_name(&dirs, &candidates), None);
    }
}
