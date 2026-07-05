# Window App Icons Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Draw each window's app icon in the bottom-right corner of its thumbnail (or colored-rect fallback) in the workspace overview.

**Architecture:** A new `src/icons.rs` module resolves an app icon file via the freedesktop icon-theme spec (using the `freedesktop-icons` crate) keyed by window class name, decodes it (PNG via Cairo, SVG via `resvg`/`usvg`/`tiny-skia`) into a Cairo `ImageSurface`, and caches the result for the process lifetime in an `IconCache` owned by `AppState`. `render::draw_client` draws the cached icon over a small rounded backing in the bottom-right corner of each window rect, gated by a new `show_app_icons` config flag and a minimum-size threshold.

**Tech Stack:** Rust, Cairo (`cairo-rs`), `freedesktop-icons` (icon theme lookup), `resvg` + `usvg` + `tiny-skia` (pure-Rust SVG rasterization, no GTK/librsvg dependency).

## Global Constraints

- No GTK or other C-library dependencies beyond what's already used (Cairo, Pango, Wayland) — SVG rendering must stay pure-Rust (`resvg`/`usvg`/`tiny-skia`), per the spec.
- New config fields go in `AppearanceConfig` with defaults: `show_app_icons = true`, `icon_size = 20.0`, following the existing style (inline doc comment above each field, entry in `config.example.toml`, entry in `warn_unknown_keys`'s `SECTIONS` list).
- Icon lookup tries, in order: raw `class_name`, then `clean_app_name(class_name)` lowercased. No match at any step → no icon drawn, existing rendering unaffected.
- `IconCache` caches both hits and misses (a `None` entry) keyed by `class_name`, for the process lifetime — icons are never re-looked-up once resolved (or found absent) for a given class.
- Icon theme is detected once via `gtk-3.0/settings.ini`'s `gtk-icon-theme-name`, falling back to `"Adwaita"`, then `"hicolor"`.

---

### Task 1: Config fields for app icons

**Files:**
- Modify: `src/config.rs` (`AppearanceConfig` struct ~line 58-102, `warn_unknown_keys` `SECTIONS` ~line 298-318)
- Modify: `config.example.toml` (appearance section, ~line 36-37)

**Interfaces:**
- Produces: `AppearanceConfig.show_app_icons: bool` (default `true`), `AppearanceConfig.icon_size: f64` (default `20.0`) — consumed by Task 5's rendering code as `cfg.appearance.show_app_icons` / `cfg.appearance.icon_size`.

- [ ] **Step 1: Write the failing test**

Add to the bottom of `src/config.rs` (there's no existing `#[cfg(test)] mod tests` in this file — create one):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_defaults_include_app_icons() {
        let cfg = AppearanceConfig::default();
        assert!(cfg.show_app_icons);
        assert_eq!(cfg.icon_size, 20.0);
    }

    #[test]
    fn appearance_parses_app_icon_overrides() {
        let toml_str = r#"
            show_app_icons = false
            icon_size = 32.0
        "#;
        let cfg: AppearanceConfig = toml::from_str(toml_str).unwrap();
        assert!(!cfg.show_app_icons);
        assert_eq!(cfg.icon_size, 32.0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib config::tests`
Expected: FAIL with "no field `show_app_icons` on type `AppearanceConfig`" (compile error)

- [ ] **Step 3: Add the fields**

In `src/config.rs`, add to the `AppearanceConfig` struct (after the `tooltip_font` field, ~line 82-83):

```rust
    /// Base font family used for the hover tooltip (size suffixes are appended automatically).
    pub tooltip_font: String,
    /// Draw each window's app icon in the bottom-right corner of its thumbnail.
    pub show_app_icons: bool,
    /// Size (px) of the app icon drawn on each window thumbnail.
    pub icon_size: f64,
```

And in `impl Default for AppearanceConfig` (~line 85-102), add after `tooltip_font: "Sans".into(),`:

```rust
            tooltip_font: "Sans".into(),
            show_app_icons: true,
            icon_size: 20.0,
```

In `warn_unknown_keys`'s `SECTIONS` (~line 300-304), add the two new keys to the `"appearance"` list:

```rust
            ("appearance", &[
                "font", "label_font", "tooltip_font",
                "card_padding", "card_radius", "max_card_width", "max_card_height",
                "label_height", "thumb_padding", "select_border", "window_gap", "columns",
                "show_app_icons", "icon_size",
            ]),
```

In `config.example.toml`, add after the `window_gap` line (~line 37):

```toml
window_gap    = 4.0    # gap between adjacent window thumbnails (px); border sits in this space

# Draw each window's app icon in the bottom-right corner of its thumbnail.
show_app_icons = true

# Size (px) of the app icon drawn on each window thumbnail.
icon_size = 20.0
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib config::tests`
Expected: PASS (2 tests)

- [ ] **Step 5: Commit**

```bash
git add src/config.rs config.example.toml
git commit -m "Add show_app_icons and icon_size config options"
```

---

### Task 2: Icon theme detection and name-candidate lookup logic

**Files:**
- Create: `src/icons.rs`
- Modify: `src/main.rs` (add `mod icons;` after `mod config;` ~line 1)
- Modify: `src/render.rs` (make `clean_app_name` visible to `icons.rs`, ~line 27)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: `pub(crate) fn parse_gtk_icon_theme(ini_text: &str) -> Option<String>` and `pub(crate) fn icon_name_candidates(class_name: &str) -> Vec<String>` in `src/icons.rs` — consumed by Task 4's `IconCache::get`. Also widens `render::clean_app_name` to `pub(crate) fn clean_app_name(class: &str) -> &str` so `icons.rs` can call `crate::render::clean_app_name`.

- [ ] **Step 1: Write the failing tests**

Create `src/icons.rs`:

```rust
/// Parses `gtk-icon-theme-name = "..."` out of a GTK settings.ini's `[Settings]` section.
pub(crate) fn parse_gtk_icon_theme(ini_text: &str) -> Option<String> {
    todo!()
}

/// Ordered list of icon names to try when looking up an app's icon: the raw
/// window class, then the cleaned (reversed-domain-stripped), lowercased name.
pub(crate) fn icon_name_candidates(class_name: &str) -> Vec<String> {
    todo!()
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
}
```

Add `mod icons;` to `src/main.rs` right after `mod config;` (line 1):

```rust
mod config;
mod icons;
mod ipc;
mod render;
```

In `src/render.rs`, change the `clean_app_name` function signature (~line 27) from private to crate-visible:

```rust
pub(crate) fn clean_app_name(class: &str) -> &str {
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib icons::tests`
Expected: FAIL — `todo!()` panics (or compile passes but tests panic with "not yet implemented")

- [ ] **Step 3: Implement the functions**

Replace the `todo!()` bodies in `src/icons.rs`:

```rust
pub(crate) fn parse_gtk_icon_theme(ini_text: &str) -> Option<String> {
    for line in ini_text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("gtk-icon-theme-name") {
            let rest = rest.trim_start();
            let value = rest.strip_prefix('=')?.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

pub(crate) fn icon_name_candidates(class_name: &str) -> Vec<String> {
    let cleaned = crate::render::clean_app_name(class_name).to_lowercase();
    let mut candidates = vec![class_name.to_string()];
    if cleaned != class_name {
        candidates.push(cleaned);
    }
    candidates
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib icons::tests`
Expected: PASS (6 tests)

- [ ] **Step 5: Run the full test suite to make sure nothing else broke**

Run: `cargo test --lib`
Expected: PASS (all existing `render::tests` and new `config::tests`/`icons::tests` pass)

- [ ] **Step 6: Commit**

```bash
git add src/icons.rs src/main.rs src/render.rs
git commit -m "Add icon theme detection and name-candidate lookup logic"
```

---

### Task 3: RGBA-to-Cairo byte conversion helper

**Files:**
- Modify: `src/icons.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `pub(crate) fn premultiplied_rgba_to_cairo_argb(rgba: &[u8]) -> Vec<u8>` in `src/icons.rs` — consumed by Task 4's SVG decoding path.

**Context:** `tiny-skia::Pixmap::data()` returns premultiplied RGBA bytes (`[R, G, B, A]` per pixel). Cairo's `Format::ARgb32` is also premultiplied but stored as native-endian 32-bit ARGB, which on little-endian machines (all supported targets here) means in-memory byte order `[B, G, R, A]` per pixel. Both are already premultiplied, so only a byte swap (R and B) is needed — no re-premultiplication.

- [ ] **Step 1: Write the failing test**

Add to `src/icons.rs`'s existing `tests` module:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib icons::tests`
Expected: FAIL — `premultiplied_rgba_to_cairo_argb` not defined (compile error)

- [ ] **Step 3: Implement the function**

Add to `src/icons.rs` (outside the tests module):

```rust
pub(crate) fn premultiplied_rgba_to_cairo_argb(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for px in rgba.chunks_exact(4) {
        let [r, g, b, a] = [px[0], px[1], px[2], px[3]];
        out.extend_from_slice(&[b, g, r, a]);
    }
    out
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib icons::tests`
Expected: PASS (8 tests)

- [ ] **Step 5: Commit**

```bash
git add src/icons.rs
git commit -m "Add premultiplied RGBA-to-Cairo-ARGB byte conversion helper"
```

---

### Task 4: Icon file resolution, decoding, and IconCache

**Files:**
- Modify: `Cargo.toml` (add dependencies)
- Modify: `src/icons.rs` (add `decode_png`, `decode_svg`, `IconCache`)

**Interfaces:**
- Consumes: `icon_name_candidates` and `premultiplied_rgba_to_cairo_argb` from Tasks 2/3; `crate::render::clean_app_name` (already `pub(crate)` from Task 2).
- Produces: `pub struct IconCache` with `pub fn new() -> Self` and `pub fn get(&mut self, class_name: &str, size: u32) -> Option<cairo::ImageSurface>` — consumed by Task 5's `render.rs` integration and `main.rs`'s `AppState`.

- [ ] **Step 1: Add dependencies**

Run:

```bash
cargo add freedesktop-icons@0.4.0
cargo add resvg@0.47.0
cargo add usvg@0.47.0
cargo add tiny-skia@0.12.0
```

Verify `Cargo.toml`'s `[dependencies]` section now includes all four.

- [ ] **Step 2: Write the failing test for SVG decoding**

Add to `src/icons.rs`'s `tests` module (needs `std::io::Write` for the tempfile):

```rust
    #[test]
    fn decodes_svg_to_requested_pixel_size() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">
            <rect width="24" height="24" fill="#ff0000"/>
        </svg>"#;
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
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib icons::tests`
Expected: FAIL — `decode_svg` not defined (compile error)

- [ ] **Step 4: Implement decode_png, decode_svg, and IconCache**

Add to `src/icons.rs` (outside the tests module), along with necessary imports at the top of the file:

```rust
use cairo::{Format, ImageSurface};
use std::collections::HashMap;
use std::path::Path;

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
            .with_size(size)
            .with_cache()
            .find()
            .or_else(|| {
                freedesktop_icons::lookup(name)
                    .with_theme("hicolor")
                    .with_size(size)
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
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib icons::tests`
Expected: PASS (10 tests)

- [ ] **Step 6: Run the full test suite and clippy**

Run: `cargo test --lib && cargo clippy --all-targets -- -D warnings`
Expected: all tests pass, no clippy warnings

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/icons.rs
git commit -m "Add icon file resolution, PNG/SVG decoding, and IconCache"
```

---

### Task 5: Render app icons on window thumbnails

**Files:**
- Modify: `src/render.rs` (`draw_client` ~line 37-169, `DrawParams`/`draw` ~line 171-333)
- Modify: `src/main.rs` (`AppState` struct ~line 197-230, `AppState::new` ~line 273-, draw call site ~line 917-930)

**Interfaces:**
- Consumes: `icons::IconCache` (`get(&mut self, class_name: &str, size: u32) -> Option<cairo::ImageSurface>`) from Task 4; `cfg.appearance.show_app_icons: bool` and `cfg.appearance.icon_size: f64` from Task 1.
- Produces: nothing further consumed by later tasks (this is the last task).

- [ ] **Step 1: Write the failing test for the corner-placement math**

Add to `src/render.rs`'s existing `tests` module (at the bottom of the file):

```rust
    #[test]
    fn icon_rect_sits_in_bottom_right_corner_with_margin() {
        let (x, y, w, h) = icon_rect(10.0, 20.0, 100.0, 60.0, 24.0, 4.0);
        // rx=10, ry=20, rw=100, rh=60, icon_size=24, margin=4
        assert_eq!(w, 24.0);
        assert_eq!(h, 24.0);
        assert_eq!(x, 10.0 + 100.0 - 24.0 - 4.0); // 82.0
        assert_eq!(y, 20.0 + 60.0 - 24.0 - 4.0);  // 52.0
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib render::tests`
Expected: FAIL — `icon_rect` not defined (compile error)

- [ ] **Step 3: Implement icon_rect and wire icon drawing into draw_client**

Add this pure helper to `src/render.rs`, near `rounded_rect` (~line 13-21):

```rust
/// Bottom-right-corner placement for a fixed-size icon inset by `margin`
/// from the edges of a `(rx, ry, rw, rh)` rect. Returns `(x, y, w, h)`.
fn icon_rect(rx: f64, ry: f64, rw: f64, rh: f64, icon_size: f64, margin: f64) -> (f64, f64, f64, f64) {
    let x = rx + rw - icon_size - margin;
    let y = ry + rh - icon_size - margin;
    (x, y, icon_size, icon_size)
}
```

Add `icon_cache: &mut crate::icons::IconCache` as a new parameter to `draw_client` (~line 38-53), and draw the icon at the end of the function (after the filter-dimming block, ~line 158-168):

```rust
#[allow(clippy::too_many_arguments)]
fn draw_client(
    cr: &Context,
    cfg: &Config,
    client: &ClientInfo,
    min_x: i32,
    min_y: i32,
    scale: f64,
    off_x: f64,
    off_y: f64,
    thumbnails: &[Thumbnail],
    active_window_address: u64,
    hovered_window: Option<u64>,
    filter_match: Option<u64>,
    filter: &str,
    window_font: &pango::FontDescription,
    icon_cache: &mut crate::icons::IconCache,
) {
```

(this only adds the trailing `icon_cache` parameter — every other line of the existing signature and body stays as-is until the new block below)

```rust
    // Dim window if it doesn't match the active filter
    if !filter.is_empty() {
        let f = filter.to_lowercase();
        let matches = client.class_name.to_lowercase().contains(&f)
            || client.title.to_lowercase().contains(&f);
        if !matches {
            rounded_rect(cr, rx, ry, rw, rh, 4.0);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.6);
            cr.fill().ok();
        }
    }

    // App icon in the bottom-right corner
    if cfg.appearance.show_app_icons && rw > 40.0 && rh > 20.0 && !client.class_name.is_empty() {
        let icon_size = cfg.appearance.icon_size;
        if let Some(icon) = icon_cache.get(&client.class_name, icon_size as u32) {
            let margin = 4.0;
            let (ix, iy, iw, ih) = icon_rect(rx, ry, rw, rh, icon_size, margin);
            let backing_pad = 2.0;
            rounded_rect(cr, ix - backing_pad, iy - backing_pad, iw + backing_pad * 2.0, ih + backing_pad * 2.0, 4.0);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.45);
            cr.fill().ok();

            cr.save().ok();
            cr.translate(ix, iy);
            cr.scale(iw / icon.width() as f64, ih / icon.height() as f64);
            cr.set_source_surface(&icon, 0.0, 0.0).ok();
            cr.paint().ok();
            cr.restore().ok();
        }
    }
}
```

Update the two things that construct/call `DrawParams`/`draw_client`:

In `DrawParams` (~line 171-184), add a new field:

```rust
pub struct DrawParams<'a> {
    pub width: u32,
    pub height: u32,
    pub workspaces: &'a [WorkspaceInfo],
    pub selected_index: usize,
    pub thumbnails: &'a [Thumbnail],
    pub cfg: &'a Config,
    pub active_window_address: u64,
    pub hovered_window: Option<u64>,
    pub pointer_x: f64,
    pub pointer_y: f64,
    pub filter_match: Option<u64>,
    pub filter: &'a str,
    pub icon_cache: &'a mut crate::icons::IconCache,
}
```

In `draw` (~line 186-187), destructure the new field and thread it through:

```rust
pub fn draw(p: DrawParams<'_>) -> Vec<u8> {
    let DrawParams { width, height, workspaces, selected_index, thumbnails, cfg, active_window_address, hovered_window, pointer_x, pointer_y, filter_match, filter, icon_cache } = p;
```

And at the client-drawing call site (~line 315-317):

```rust
        for client in &ws.clients {
            draw_client(&cr, cfg, client, min_x, min_y, scale, off_x, off_y, thumbnails, active_window_address, hovered_window, filter_match, filter, &window_font, icon_cache);
        }
```

- [ ] **Step 4: Wire IconCache into AppState and the draw call site in main.rs**

In `src/main.rs`, add a field to `AppState` (~line 220, next to `thumbnails: Vec<Thumbnail>,`):

```rust
    workspaces: Vec<WorkspaceInfo>,
    thumbnails: Vec<Thumbnail>,
    icon_cache: icons::IconCache,
```

Initialize it in `AppState::new` (~line 293, next to `thumbnails: Vec::new(),`):

```rust
            thumbnails: Vec::new(),
            icon_cache: icons::IconCache::new(),
```

Update the `render::draw` call site (~line 917-930) to pass it in:

```rust
    let pixels = render::draw(render::DrawParams {
        width: state.width,
        height: state.height,
        workspaces: workspaces_to_draw,
        selected_index: state.selected,
        thumbnails: &state.thumbnails,
        cfg: &state.config,
        active_window_address: state.active_window_address,
        hovered_window: state.hovered_window,
        pointer_x: state.pointer_x,
        pointer_y: state.pointer_y,
        filter_match: state.filter_match,
        filter: &state.filter,
        icon_cache: &mut state.icon_cache,
    });
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib render::tests`
Expected: PASS (all `render::tests`, including `icon_rect_sits_in_bottom_right_corner_with_margin`)

- [ ] **Step 6: Write a smoke test that draw() doesn't panic with icons on or off**

Add to `src/render.rs`'s `tests` module:

```rust
    #[test]
    fn draw_does_not_panic_with_show_app_icons_toggled() {
        use crate::ipc::{ClientInfo, WorkspaceInfo};

        let client = ClientInfo {
            class_name: "firefox".to_string(),
            title: "Mozilla Firefox".to_string(),
            initial_title: String::new(),
            address: 1,
            workspace_id: 1,
            x: 0,
            y: 0,
            w: 800,
            h: 600,
        };
        let ws = WorkspaceInfo {
            id: 1,
            name: String::new(),
            monitor_id: 0,
            clients: vec![client],
        };

        for show_icons in [true, false] {
            let mut cfg = Config::default();
            cfg.appearance.show_app_icons = show_icons;
            let mut icon_cache = crate::icons::IconCache::new();

            let pixels = draw(DrawParams {
                width: 800,
                height: 600,
                workspaces: &[ws.clone()],
                selected_index: 0,
                thumbnails: &[],
                cfg: &cfg,
                active_window_address: 0,
                hovered_window: None,
                pointer_x: 0.0,
                pointer_y: 0.0,
                filter_match: None,
                filter: "",
                icon_cache: &mut icon_cache,
            });

            assert_eq!(pixels.len(), (800 * 600 * 4) as usize);
        }
    }
```

- [ ] **Step 7: Run test to verify it passes**

Run: `cargo test --lib`
Expected: PASS (all tests across `config`, `icons`, `render` modules)

- [ ] **Step 8: Run clippy and build in release mode**

Run: `cargo clippy --all-targets -- -D warnings && cargo build --release`
Expected: no warnings, clean release build

- [ ] **Step 9: Manual verification**

Run the built binary against a live Hyprland or Sway session (`target/release/hyprexpose &`, then `pkill -SIGUSR1 hyprexpose`), open a few different apps (e.g. a terminal, a browser, a GTK app), and confirm:
- Each window thumbnail shows its app icon in the bottom-right corner.
- Icons look correct for at least one PNG-icon app and one SVG-icon app.
- Setting `show_app_icons = false` in `~/.config/hyprexpose/config.toml` hides all icons.
- An app with no resolvable icon (e.g. a custom `class_name` not in any icon theme) still renders its thumbnail/placeholder normally with no icon and no crash.

- [ ] **Step 10: Commit**

```bash
git add src/render.rs src/main.rs
git commit -m "Draw app icons in the bottom-right corner of window thumbnails"
```
