use crate::config::Config;
use crate::ipc::{ClientInfo, WorkspaceInfo};
use cairo::{Context, Format, ImageSurface};

pub struct Thumbnail {
    pub address: u64,
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
}

fn rounded_rect(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r,     r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r,  0.0,      PI / 2.0);
    cr.arc(x + r,     y + h - r, r,  PI / 2.0, PI);
    cr.arc(x + r,     y + r,     r,  PI,       3.0 * PI / 2.0);
    cr.close_path();
}

fn find_thumb(thumbnails: &[Thumbnail], address: u64) -> Option<&Thumbnail> {
    thumbnails.iter().find(|t| t.address == address)
}

/// Bottom-right-corner placement for a fixed-size icon inset by `margin`
/// from the edges of a `(rx, ry, rw, rh)` rect. Returns `(x, y, w, h)`.
fn icon_rect(rx: f64, ry: f64, rw: f64, rh: f64, icon_size: f64, margin: f64) -> (f64, f64, f64, f64) {
    let x = rx + rw - icon_size - margin;
    let y = ry + rh - icon_size - margin;
    (x, y, icon_size, icon_size)
}

/// Clamp a user-configured icon size to a sane range so a typo (e.g. `5000.0`)
/// can't trigger a huge synchronous SVG rasterization allocation.
fn clamped_icon_size(configured: f64) -> f64 {
    configured.clamp(8.0, 256.0)
}

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

pub(crate) fn clean_app_name(class: &str) -> &str {
    // Strip reversed-domain prefix: "dev.zed.Zed" → "Zed", "org.foo.Bar" → "Bar"
    if class.contains('.') {
        if let Some(last) = class.rsplit('.').next() {
            return last;
        }
    }
    class
}

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
    tab_target: Option<u64>,
    filter: &str,
    window_font: &pango::FontDescription,
    icon_cache: &mut crate::icons::IconCache,
) {
    // Inset each thumbnail by half the gap so adjacent windows have a full gap between them
    let g = cfg.appearance.window_gap / 2.0;
    let rx = off_x + (client.x - min_x) as f64 * scale + g;
    let ry = off_y + (client.y - min_y) as f64 * scale + g;
    let rw = (client.w as f64 * scale - g * 2.0).max(1.0);
    let rh = (client.h as f64 * scale - g * 2.0).max(1.0);

    // Window thumbnail or fallback colored rect
    if let Some(thumb) = find_thumb(thumbnails, client.address) {
        if let Ok(img) = ImageSurface::create_for_data(
            thumb.data.clone(),
            Format::ARgb32,
            thumb.width as i32,
            thumb.height as i32,
            thumb.stride as i32,
        ) {
            cr.save().ok();
            rounded_rect(cr, rx, ry, rw, rh, 4.0);
            cr.clip();
            cr.translate(rx, ry);
            cr.scale(rw / thumb.width as f64, rh / thumb.height as f64);
            cr.set_source_surface(&img, 0.0, 0.0).ok();
            cr.paint().ok();
            cr.restore().ok();
        }
    } else {
        let hash: u32 = client
            .class_name
            .bytes()
            .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
        let r = 0.2 + (hash % 100) as f64 / 200.0;
        let gr = 0.2 + ((hash / 100) % 100) as f64 / 200.0;
        let b = 0.3 + ((hash / 10000) % 100) as f64 / 200.0;
        rounded_rect(cr, rx, ry, rw, rh, 4.0);
        cr.set_source_rgba(r, gr, b, 0.85);
        cr.fill().ok();
    }

    // Default window border (sits in the gap)
    let (wr, wg, wb, wa) = cfg.colors.window_border.rgba();
    rounded_rect(cr, rx, ry, rw, rh, 4.0);
    cr.set_source_rgba(wr, wg, wb, wa);
    cr.set_line_width(1.0);
    cr.stroke().ok();

    // Active-window indicator
    if active_window_address != 0 && client.address == active_window_address {
        let (ar, ag, ab, aa) = cfg.colors.active_window.rgba();
        rounded_rect(cr, rx - 2.0, ry - 2.0, rw + 4.0, rh + 4.0, 5.0);
        cr.set_source_rgba(ar, ag, ab, aa);
        cr.set_line_width(2.5);
        cr.stroke().ok();
    }

    // Filter single-match highlight
    if filter_match == Some(client.address) {
        rounded_rect(cr, rx, ry, rw, rh, 4.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.12);
        cr.fill().ok();
        rounded_rect(cr, rx - 2.0, ry - 2.0, rw + 4.0, rh + 4.0, 6.0);
        cr.set_source_rgba(0.4, 0.8, 1.0, 0.9);
        cr.set_line_width(2.0);
        cr.stroke().ok();
    }

    // Tab-cycle highlight
    if tab_target == Some(client.address) {
        rounded_rect(cr, rx - 2.0, ry - 2.0, rw + 4.0, rh + 4.0, 6.0);
        cr.set_source_rgba(0.3, 1.0, 0.4, 0.95);
        cr.set_line_width(2.5);
        cr.stroke().ok();
    }

    // Hover highlight (skip if this window is already Tab-highlighted, to avoid
    // stacking two near-identical rings when the mouse happens to rest on it)
    if hovered_window == Some(client.address) && tab_target != Some(client.address) {
        let (or_, og, ob, oa) = cfg.colors.hover_overlay.rgba();
        rounded_rect(cr, rx, ry, rw, rh, 4.0);
        cr.set_source_rgba(or_, og, ob, oa);
        cr.fill().ok();
        let (br, bg, bb, ba) = cfg.colors.hover_border.rgba();
        rounded_rect(cr, rx - 1.5, ry - 1.5, rw + 3.0, rh + 3.0, 5.0);
        cr.set_source_rgba(br, bg, bb, ba);
        cr.set_line_width(1.5);
        cr.stroke().ok();
    }

    // Window label (class name or title)
    if rw > 40.0 && rh > 20.0 {
        let layout = pangocairo::functions::create_layout(cr);
        const GENERIC_TITLES: &[&str] = &[
            "New Window", "New Tab", "Window", "window", "Untitled",
        ];
        let initial_ok = !client.initial_title.is_empty()
            && !GENERIC_TITLES.contains(&client.initial_title.as_str());
        let name = if initial_ok {
            client.initial_title.as_str()
        } else if !client.class_name.is_empty() {
            clean_app_name(&client.class_name)
        } else {
            client.title.as_str()
        };
        layout.set_text(name);
        layout.set_font_description(Some(window_font));
        layout.set_width(((rw - 4.0) * pango::SCALE as f64) as i32);
        layout.set_ellipsize(pango::EllipsizeMode::End);
        let (tw, th) = layout.pixel_size();
        let (wr, wg, wb, wa) = cfg.colors.window_label.rgba();
        cr.set_source_rgba(wr, wg, wb, wa);
        cr.move_to(rx + (rw - tw as f64) / 2.0, ry + (rh - th as f64) / 2.0);
        pangocairo::functions::show_layout(cr, &layout);
    }

    // App icon in the bottom-right corner
    if cfg.appearance.show_app_icons && rw > 40.0 && rh > 20.0 && !client.class_name.is_empty() {
        let icon_size = clamped_icon_size(cfg.appearance.icon_size);
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
}

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
    pub tab_target: Option<u64>,
    pub filter: &'a str,
    pub icon_cache: &'a mut crate::icons::IconCache,
}

pub fn draw(p: DrawParams<'_>) -> Vec<u8> {
    let DrawParams { width, height, workspaces, selected_index, thumbnails, cfg, active_window_address, hovered_window, pointer_x, pointer_y, filter_match, tab_target, filter, icon_cache } = p;
    let stride = width * 4;
    let size = (stride * height) as usize;
    let buf = vec![0u8; size];

    let surface = match ImageSurface::create_for_data(
        buf, Format::ARgb32, width as i32, height as i32, stride as i32,
    ) {
        Ok(s) => s,
        Err(_) => return vec![0u8; size],
    };
    let cr = match Context::new(&surface) {
        Ok(c) => c,
        Err(_) => return vec![0u8; size],
    };

    // Dimmed background
    let (br, bg, bb, ba) = cfg.colors.background.rgba();
    cr.set_operator(cairo::Operator::Source);
    cr.set_source_rgba(br, bg, bb, ba);
    cr.paint().ok();
    cr.set_operator(cairo::Operator::Over);

    if workspaces.is_empty() {
        drop(cr);
        return surface.take_data().map(|d| d.to_vec()).unwrap_or_default();
    }

    let n = workspaces.len();
    let cols = if cfg.appearance.columns > 0 { cfg.appearance.columns.min(n) }
               else { ((n as f64).sqrt().ceil() as usize).max(1) };
    let rows = n.div_ceil(cols);
    let pad = cfg.appearance.card_padding;

    let card_w = ((width as f64 - pad * (cols + 1) as f64) / cols as f64)
        .min(cfg.appearance.max_card_width);
    let card_h = ((height as f64 - pad * (rows + 1) as f64) / rows as f64)
        .min(cfg.appearance.max_card_height);

    let grid_w = cols as f64 * card_w + (cols - 1) as f64 * pad;
    let grid_h = rows as f64 * card_h + (rows - 1) as f64 * pad;
    let ox = (width as f64 - grid_w) / 2.0;
    let oy = (height as f64 - grid_h) / 2.0;

    let label_font = pango::FontDescription::from_string(&cfg.appearance.label_font);
    let window_font = pango::FontDescription::from_string(&cfg.appearance.font);
    let empty_font = window_font.clone();

    for (i, ws) in workspaces.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let cx = ox + col as f64 * (card_w + pad);
        let cy = oy + row as f64 * (card_h + pad);
        let r = cfg.appearance.card_radius;
        let select_border_w = cfg.appearance.select_border;

        // Selection highlight
        if i == selected_index {
            let (sr, sg, sb, sa) = cfg.colors.selection.rgba();
            rounded_rect(&cr, cx - select_border_w, cy - select_border_w, card_w + 2.0 * select_border_w, card_h + 2.0 * select_border_w, r + 2.0);
            cr.set_source_rgba(sr, sg, sb, sa);
            cr.fill().ok();
        }

        // Card background
        let (cr_c, cg, cb, ca) = cfg.colors.card.rgba();
        rounded_rect(&cr, cx, cy, card_w, card_h, r);
        cr.set_source_rgba(cr_c, cg, cb, ca);
        cr.fill().ok();

        // Workspace label
        {
            let layout = pangocairo::functions::create_layout(&cr);
            // Named workspaces without a number (sway: num == -1) show just the name.
            let mut label = if ws.id >= 1 { ws.id.to_string() } else { String::new() };
            if !ws.name.is_empty() && ws.name != label {
                if !label.is_empty() {
                    label.push(' ');
                }
                label.push_str(&ws.name);
            }
            if label.is_empty() {
                label = ws.id.to_string();
            }
            layout.set_text(&label);
            layout.set_font_description(Some(&label_font));
            let (tw, _) = layout.pixel_size();
            let (lr, lg, lb, la) = cfg.colors.label.rgba();
            let alpha = if i == selected_index { la } else { la * 0.25 };
            cr.set_source_rgba(lr, lg, lb, alpha);
            cr.move_to(cx + (card_w - tw as f64) / 2.0, cy + 6.0);
            pangocairo::functions::show_layout(&cr, &layout);
        }

        let lh = cfg.appearance.label_height;
        let tp = cfg.appearance.thumb_padding;
        let win_x = cx + tp;
        let win_y = cy + lh;
        let win_w = card_w - 2.0 * tp;
        let win_h = card_h - lh - tp;

        if ws.clients.is_empty() {
            let layout = pangocairo::functions::create_layout(&cr);
            layout.set_text("(empty)");
            layout.set_font_description(Some(&empty_font));
            let (tw, th) = layout.pixel_size();
            let (er, eg, eb, ea) = cfg.colors.empty_label.rgba();
            cr.set_source_rgba(er, eg, eb, ea);
            cr.move_to(cx + (card_w - tw as f64) / 2.0, win_y + (win_h - th as f64) / 2.0);
            pangocairo::functions::show_layout(&cr, &layout);
            continue;
        }

        // Scale all client rects to fit the window area
        let (min_x, min_y, max_x, max_y) = ws.clients.iter().fold(
            (i32::MAX, i32::MAX, i32::MIN, i32::MIN),
            |(mix, miy, mxx, mxy), c| {
                (mix.min(c.x), miy.min(c.y), mxx.max(c.x + c.w), mxy.max(c.y + c.h))
            },
        );

        let ws_w = (max_x - min_x).max(1) as f64;
        let ws_h = (max_y - min_y).max(1) as f64;
        let scale = (win_w / ws_w).min(win_h / ws_h) * 0.9;

        let off_x = win_x + (win_w - ws_w * scale) / 2.0;
        let off_y = win_y + (win_h - ws_h * scale) / 2.0;

        for client in &ws.clients {
            draw_client(&cr, cfg, client, min_x, min_y, scale, off_x, off_y, thumbnails, active_window_address, hovered_window, filter_match, tab_target, filter, &window_font, icon_cache);
        }

        // Dim card if it doesn't match the active filter
        if !filter.is_empty() {
            let f = filter.to_lowercase();
            let matches = ws.name.to_lowercase().contains(&f)
                || ws.clients.iter().any(|c| {
                    c.class_name.to_lowercase().contains(&f)
                        || c.title.to_lowercase().contains(&f)
                });
            if !matches {
                rounded_rect(&cr, cx, cy, card_w, card_h, r);
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.65);
                cr.fill().ok();
            }
        }
    }

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

    // Filter bar
    if !filter.is_empty() {
        let f = filter.to_lowercase();
        let matched_ws: Vec<&str> = workspaces.iter()
            .filter(|ws| {
                ws.name.to_lowercase().contains(&f)
                    || ws.clients.iter().any(|c| {
                        c.class_name.to_lowercase().contains(&f)
                            || c.title.to_lowercase().contains(&f)
                    })
            })
            .map(|ws| ws.name.as_str())
            .collect();

        let bar_font = pango::FontDescription::from_string("Sans 14");

        let filter_layout = pangocairo::functions::create_layout(&cr);
        filter_layout.set_text(filter);
        filter_layout.set_font_description(Some(&bar_font));
        let (ftw, fth) = filter_layout.pixel_size();

        let ws_suffix = if !matched_ws.is_empty() {
            format!("  ·  {}", matched_ws.join(", "))
        } else {
            String::new()
        };
        let ws_layout = if !ws_suffix.is_empty() {
            let l = pangocairo::functions::create_layout(&cr);
            l.set_text(&ws_suffix);
            l.set_font_description(Some(&bar_font));
            Some(l)
        } else {
            None
        };
        let wstw = ws_layout.as_ref().map(|l| l.pixel_size().0).unwrap_or(0);

        let pad_x = 20.0;
        let pad_y = 10.0;
        let bw = ftw as f64 + wstw as f64 + pad_x * 2.0;
        let bh = fth as f64 + pad_y * 2.0;
        let bx = (width as f64 - bw) / 2.0;
        let by = 24.0;

        rounded_rect(&cr, bx, by, bw, bh, bh / 2.0);
        cr.set_source_rgba(0.08, 0.08, 0.12, 0.92);
        cr.fill().ok();

        cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
        cr.move_to(bx + pad_x, by + pad_y);
        pangocairo::functions::show_layout(&cr, &filter_layout);

        if let Some(wsl) = &ws_layout {
            cr.set_source_rgba(0.7, 0.75, 0.85, 0.6);
            cr.move_to(bx + pad_x + ftw as f64, by + pad_y);
            pangocairo::functions::show_layout(&cr, wsl);
        }
    }

    drop(cr);
    surface.take_data().map(|d| d.to_vec()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_app_name_strips_reversed_domain() {
        assert_eq!(clean_app_name("dev.zed.Zed"), "Zed");
        assert_eq!(clean_app_name("org.gnome.Nautilus"), "Nautilus");
        assert_eq!(clean_app_name("com.mitchellh.ghostty"), "ghostty");
    }

    #[test]
    fn clean_app_name_passthrough_when_no_dot() {
        assert_eq!(clean_app_name("firefox"), "firefox");
        assert_eq!(clean_app_name(""), "");
        assert_eq!(clean_app_name("Alacritty"), "Alacritty");
    }

    #[test]
    fn clean_app_name_trailing_dot_returns_empty() {
        assert_eq!(clean_app_name("foo."), "");
    }

    #[test]
    fn icon_rect_sits_in_bottom_right_corner_with_margin() {
        let (x, y, w, h) = icon_rect(10.0, 20.0, 100.0, 60.0, 24.0, 4.0);
        // rx=10, ry=20, rw=100, rh=60, icon_size=24, margin=4
        assert_eq!(w, 24.0);
        assert_eq!(h, 24.0);
        assert_eq!(x, 10.0 + 100.0 - 24.0 - 4.0); // 82.0
        assert_eq!(y, 20.0 + 60.0 - 24.0 - 4.0);  // 52.0
    }

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

    #[test]
    fn clamped_icon_size_caps_huge_values() {
        assert_eq!(clamped_icon_size(5000.0), 256.0);
    }

    #[test]
    fn clamped_icon_size_floors_negative_or_tiny_values() {
        assert_eq!(clamped_icon_size(-10.0), 8.0);
        assert_eq!(clamped_icon_size(0.0), 8.0);
    }

    #[test]
    fn clamped_icon_size_passes_through_sane_values() {
        assert_eq!(clamped_icon_size(20.0), 20.0);
    }

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
                tab_target: None,
                filter: "",
                icon_cache: &mut icon_cache,
            });

            assert_eq!(pixels.len(), (800 * 600 * 4) as usize);
        }
    }
}
