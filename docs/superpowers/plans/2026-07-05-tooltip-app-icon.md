# Tooltip App Icon Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show the hovered window's app icon in the hover tooltip, next to the app name.

**Architecture:** Extend the existing hover-tooltip block in `src/render.rs`'s `draw()` function to look up the hovered window's icon via the already-threaded `IconCache`, and — when found — draw it to the left of the app-name text, widening the tooltip box and the name row's height as needed.

**Tech Stack:** Rust, Cairo (`cairo-rs`) — no new dependencies.

## Global Constraints

- Reuses `cfg.appearance.show_app_icons` and `cfg.appearance.icon_size` (via the existing `clamped_icon_size` helper) — no new config fields.
- If `icon_cache.get(&c.class_name, ...)` returns `None`, the tooltip renders exactly as it does today — no reserved gutter, no layout change.
- Gap between icon and name text: 8px (fixed, not configurable).
- Only the name row gains the icon; the class and title lines below stay at their current left position (not indented).

---

### Task 1: Draw the app icon in the hover tooltip

**Files:**
- Modify: `src/render.rs` (the hover-tooltip block inside `draw()`, currently spanning approximately lines 371-452 — search for the comment `// Hover tooltip`)

**Interfaces:**
- Consumes: `icon_cache: &mut crate::icons::IconCache` (already a local binding in `draw()`, destructured from `DrawParams` — see the top of the `draw()` function body), `cfg.appearance.show_app_icons: bool`, `cfg.appearance.icon_size: f64`, the private `clamped_icon_size(f64) -> f64` helper (already defined in this file, near `icon_rect`).
- Produces: nothing consumed by a later task — this is the only task.

- [ ] **Step 1: Write the failing test**

This change lives inside `draw()`, which is already covered by the existing smoke test `draw_does_not_panic_with_show_app_icons_toggled` in `src/render.rs`'s `#[cfg(test)] mod tests` block. Extend that test to also exercise the hovered-window code path (it currently never sets `hovered_window`, so the tooltip block never runs). Replace the test with this version, which sets `hovered_window: Some(1)` so the tooltip block (including the new icon-drawing code) executes for both `show_app_icons` values:

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
                workspaces: std::slice::from_ref(&ws),
                selected_index: 0,
                thumbnails: &[],
                cfg: &cfg,
                active_window_address: 0,
                hovered_window: Some(1),
                pointer_x: 100.0,
                pointer_y: 100.0,
                filter_match: None,
                filter: "",
                icon_cache: &mut icon_cache,
            });

            assert_eq!(pixels.len(), (800 * 600 * 4) as usize);
        }
    }
```

(The only changes from the existing test are `hovered_window: Some(1)` instead of `None`, and `pointer_x`/`pointer_y` set to `100.0` instead of `0.0` so the tooltip has room to render on-screen — everything else is unchanged.)

- [ ] **Step 2: Run test to verify it currently passes (baseline) then add the size-math unit test that will fail**

First confirm the modified smoke test alone still passes against the *unmodified* tooltip code (it should — this step only proves the hovered-window path is now exercised, not that icon-drawing exists yet):

Run: `cargo test render::tests::draw_does_not_panic_with_show_app_icons_toggled`
Expected: PASS (this test doesn't assert anything about icon presence, so it passes whether or not the icon is drawn — it only proves no panic)

Now add a real failing test for the new layout math. Add this pure helper function test to `src/render.rs`'s `#[cfg(test)] mod tests` block, right after the existing `icon_rect_sits_in_bottom_right_corner_with_margin` test:

```rust
    #[test]
    fn tooltip_name_row_offset_widens_for_icon() {
        // With no icon (icon_size = 0.0), the name text starts right at the
        // tooltip's left padding, and the row height is just the text height.
        let (x_offset, row_h) = tooltip_name_row_metrics(0.0, 8.0, 30.0);
        assert_eq!(x_offset, 0.0);
        assert_eq!(row_h, 30.0);

        // With a 20px icon and an 8px gap, the name text shifts right by
        // icon_size + gap, and the row height is the max of icon size and
        // text height.
        let (x_offset, row_h) = tooltip_name_row_metrics(20.0, 8.0, 30.0);
        assert_eq!(x_offset, 28.0);
        assert_eq!(row_h, 30.0);

        // When the icon is taller than the text, the row height follows the icon.
        let (x_offset, row_h) = tooltip_name_row_metrics(40.0, 8.0, 30.0);
        assert_eq!(x_offset, 48.0);
        assert_eq!(row_h, 40.0);
    }
```

Run: `cargo test render::tests::tooltip_name_row_offset_widens_for_icon`
Expected: FAIL with "cannot find function `tooltip_name_row_metrics` in this scope" (compile error)

- [ ] **Step 3: Implement the pure layout-math helper**

Add this function to `src/render.rs`, right after `clamped_icon_size` (~line 39):

```rust
/// Returns `(name_text_x_offset, name_row_height)` for the tooltip's name
/// row. `icon_size` is `0.0` when no icon will be drawn (the icon column
/// collapses to nothing); otherwise it's the icon's side length, `gap` is
/// the fixed spacing between icon and text, and `text_height` is the name
/// text's own pixel height.
fn tooltip_name_row_metrics(icon_size: f64, gap: f64, text_height: f64) -> (f64, f64) {
    if icon_size <= 0.0 {
        return (0.0, text_height);
    }
    (icon_size + gap, text_height.max(icon_size))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test render::tests::tooltip_name_row_offset_widens_for_icon`
Expected: PASS

- [ ] **Step 5: Wire the icon into the tooltip block**

Find the hover-tooltip block in `draw()` (search for `// Hover tooltip`). Replace the entire block with this version — the only changes are: computing `icon` and the `(icon_x_offset, name_row_h)` pair right after `name_l` is created, using `name_row_h` instead of `nh` when advancing `content_h` and `ty` past the name row, adding the icon's width to `content_w`, and drawing the icon (with its own vertical centering) before the name text:

```rust
    // Hover tooltip
    if let Some(addr) = hovered_window {
        let client = workspaces.iter().flat_map(|ws| ws.clients.iter()).find(|c| c.address == addr);
        if let Some(c) = client {
            const GENERIC_TITLES: &[&str] = &[
                "New Window", "New Tab", "Window", "window", "Untitled",
            ];
            let app = if !c.initial_title.is_empty()
                && !GENERIC_TITLES.contains(&c.initial_title.as_str())
            {
                c.initial_title.as_str()
            } else {
                clean_app_name(&c.class_name)
            };
            let full_class = c.class_name.as_str();
            let show_class = full_class != app && !full_class.is_empty();

            let tf = &cfg.appearance.tooltip_font;
            let name_font  = pango::FontDescription::from_string(&format!("{tf} Bold 13"));
            let class_font = pango::FontDescription::from_string(&format!("{tf} 10"));
            let title_font = pango::FontDescription::from_string(&format!("{tf} 11"));

            let make_layout = |text: &str, font: &pango::FontDescription, max_w: i32| {
                let l = pangocairo::functions::create_layout(&cr);
                l.set_text(text);
                l.set_font_description(Some(font));
                l.set_width(max_w * pango::SCALE);
                l.set_ellipsize(pango::EllipsizeMode::End);
                l
            };

            let max_w = (width as f64 * 0.35) as i32;
            let pad = 12.0_f64;
            let line_gap = 4.0_f64;
            let icon_gap = 8.0_f64;

            let name_l  = make_layout(app, &name_font, max_w);
            let class_l = show_class.then(|| make_layout(full_class, &class_font, max_w));
            let title_l = (!c.title.is_empty()).then(|| make_layout(&c.title, &title_font, max_w));

            let (nw, nh) = name_l.pixel_size();
            let (cw, ch) = class_l.as_ref().map(|l| l.pixel_size()).unwrap_or((0, 0));
            let (tw, th) = title_l.as_ref().map(|l| l.pixel_size()).unwrap_or((0, 0));

            let icon = if cfg.appearance.show_app_icons && !c.class_name.is_empty() {
                let size = clamped_icon_size(cfg.appearance.icon_size);
                icon_cache.get(&c.class_name, size as u32).map(|surface| (surface, size))
            } else {
                None
            };
            let icon_size = icon.as_ref().map(|(_, size)| *size).unwrap_or(0.0);
            let (name_x_offset, name_row_h) = tooltip_name_row_metrics(icon_size, icon_gap, nh as f64);

            let content_w = [name_x_offset + nw as f64, cw as f64, tw as f64]
                .iter()
                .copied()
                .fold(0.0, f64::max);
            let content_h = name_row_h
                + if show_class { ch as f64 + line_gap } else { 0.0 }
                + if title_l.is_some() { th as f64 + line_gap } else { 0.0 };

            let bw = content_w + pad * 2.0;
            let bh = content_h + pad * 2.0;

            let margin = 12.0;
            let bx = (pointer_x + 16.0).min(width as f64 - bw - margin).max(margin);
            let by = (pointer_y - bh - 8.0).max(margin).min(height as f64 - bh - margin);

            rounded_rect(&cr, bx, by, bw, bh, 8.0);
            let (br, bg, bb, ba) = cfg.colors.tooltip_background.rgba();
            cr.set_source_rgba(br, bg, bb, ba);
            cr.fill().ok();

            let mut ty = by + pad;

            if let Some((surface, size)) = &icon {
                let icon_y = ty + (name_row_h - size) / 2.0;
                cr.save().ok();
                cr.translate(bx + pad, icon_y);
                cr.scale(size / surface.width() as f64, size / surface.height() as f64);
                cr.set_source_surface(surface, 0.0, 0.0).ok();
                cr.paint().ok();
                cr.restore().ok();
            }

            let (r, g, b, a) = cfg.colors.tooltip_name.rgba();
            cr.set_source_rgba(r, g, b, a);
            cr.move_to(bx + pad + name_x_offset, ty + (name_row_h - nh as f64) / 2.0);
            pangocairo::functions::show_layout(&cr, &name_l);
            ty += name_row_h + line_gap;

            if let Some(cl) = &class_l {
                let (r, g, b, a) = cfg.colors.tooltip_class.rgba();
                cr.set_source_rgba(r, g, b, a);
                cr.move_to(bx + pad, ty);
                pangocairo::functions::show_layout(&cr, cl);
                ty += ch as f64 + line_gap;
            }
            if let Some(tl) = &title_l {
                let (r, g, b, a) = cfg.colors.tooltip_title.rgba();
                cr.set_source_rgba(r, g, b, a);
                cr.move_to(bx + pad, ty);
                pangocairo::functions::show_layout(&cr, tl);
            }
        }
    }
```

Notes on this diff for whoever implements it:
- `icon_cache` is already in scope inside `draw()` (it's one of the fields destructured from `DrawParams` at the top of the function, alongside `width`, `height`, `workspaces`, etc. — confirm this by checking the `let DrawParams { ... } = p;` line near the top of `draw()`).
- The name text is now vertically centered within `name_row_h` (`ty + (name_row_h - nh as f64) / 2.0`) instead of drawn flush at `ty`, so it stays visually centered whether or not an icon (and its possibly-taller row) is present.
- When `icon` is `None` (icons disabled, empty class name, or no icon resolved for this class), `icon_size` is `0.0`, `tooltip_name_row_metrics` returns `(0.0, nh as f64)`, and the block behaves identically to the pre-existing code — same `bx`/`by`/`bw`/`bh` math, same text position.

- [ ] **Step 6: Run tests to verify everything passes**

Run: `cargo test` (this is a bin-only crate — `cargo test --lib` will NOT work)
Expected: PASS, all tests including `render::tests::draw_does_not_panic_with_show_app_icons_toggled` and `render::tests::tooltip_name_row_offset_widens_for_icon`

- [ ] **Step 7: Run clippy and rustfmt**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings

Run: `rustfmt --check src/render.rs`
Expected: no diff (if there is a diff, run `rustfmt src/render.rs` to apply it, then re-run the test suite to confirm nothing broke)

- [ ] **Step 8: Manual verification**

This step cannot be performed by an agent without a live Wayland display — note it as a concern in the report instead of attempting it. The controller (or the user) should verify manually: run the built binary against a live Hyprland or Sway session, hover over a window whose app icon is known to resolve (e.g. a terminal or browser), and confirm the icon appears to the left of the app name in the tooltip, vertically centered, with the class/title lines unaffected. Then hover over a window with no resolvable icon and confirm the tooltip looks exactly as it did before this change.

- [ ] **Step 9: Commit**

```bash
git add src/render.rs
git commit -m "Show app icon in the hover tooltip next to the app name"
```
