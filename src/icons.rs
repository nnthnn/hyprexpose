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

fn detect_icon_theme() -> String {
    let candidates = [
        std::env::var_os("XDG_CONFIG_HOME").map(|p| Path::new(&p).join("gtk-3.0/settings.ini")),
        dirs_home_config("gtk-3.0/settings.ini"),
    ];
    for path in candidates.into_iter().flatten() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Some(theme) = parse_gtk_icon_theme(&text) {
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

// Not yet constructed outside tests; Task 5's `render.rs`/`main.rs`
// integration will consume this.
#[allow(dead_code)]
pub struct IconCache {
    theme: String,
    cache: HashMap<String, Option<ImageSurface>>,
}

// Not yet called outside tests; Task 5's `render.rs`/`main.rs` integration
// will consume these.
#[allow(dead_code)]
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
        let candidates = icon_name_candidates(class_name);
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
}
