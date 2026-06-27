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

fn clean_app_name(class: &str) -> &str {
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
    filter: &str,
    window_font: &pango::FontDescription,
) {
    let rx = off_x + (client.x - min_x) as f64 * scale;
    let ry = off_y + (client.y - min_y) as f64 * scale;
    let rw = client.w as f64 * scale;
    let rh = client.h as f64 * scale;

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
        let g = 0.2 + ((hash / 100) % 100) as f64 / 200.0;
        let b = 0.3 + ((hash / 10000) % 100) as f64 / 200.0;
        rounded_rect(cr, rx, ry, rw, rh, 4.0);
        cr.set_source_rgba(r, g, b, 0.85);
        cr.fill().ok();
    }

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

    // Hover highlight
    if hovered_window == Some(client.address) {
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
        let name = if !client.initial_title.is_empty() {
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
    pub filter: &'a str,
}

pub fn draw(p: DrawParams<'_>) -> Vec<u8> {
    let DrawParams { width, height, workspaces, selected_index, thumbnails, cfg, active_window_address, hovered_window, pointer_x, pointer_y, filter_match, filter } = p;
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
    let cols = ((n as f64).sqrt().ceil() as usize).max(1);
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
            draw_client(&cr, cfg, client, min_x, min_y, scale, off_x, off_y, thumbnails, active_window_address, hovered_window, filter_match, filter, &window_font);
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
            let app = if !c.initial_title.is_empty() { c.initial_title.as_str() }
                      else { clean_app_name(&c.class_name) };
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

            let name_l  = make_layout(app, &name_font, max_w);
            let class_l = show_class.then(|| make_layout(full_class, &class_font, max_w));
            let title_l = (!c.title.is_empty()).then(|| make_layout(&c.title, &title_font, max_w));

            let (nw, nh) = name_l.pixel_size();
            let (cw, ch) = class_l.as_ref().map(|l| l.pixel_size()).unwrap_or((0, 0));
            let (tw, th) = title_l.as_ref().map(|l| l.pixel_size()).unwrap_or((0, 0));

            let content_w = [nw, cw, tw].iter().copied().max().unwrap_or(0) as f64;
            let content_h = nh as f64
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
            let (r, g, b, a) = cfg.colors.tooltip_name.rgba();
            cr.set_source_rgba(r, g, b, a);
            cr.move_to(bx + pad, ty);
            pangocairo::functions::show_layout(&cr, &name_l);
            ty += nh as f64 + line_gap;

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
        let layout = pangocairo::functions::create_layout(&cr);
        let text = format!("  {}  ", filter);
        layout.set_text(&text);
        let bar_font = pango::FontDescription::from_string("Sans 14");
        layout.set_font_description(Some(&bar_font));
        let (tw, th) = layout.pixel_size();
        let bw = tw as f64 + 24.0;
        let bh = th as f64 + 16.0;
        let bx = (width as f64 - bw) / 2.0;
        let by = 24.0;
        rounded_rect(&cr, bx, by, bw, bh, bh / 2.0);
        cr.set_source_rgba(0.08, 0.08, 0.12, 0.92);
        cr.fill().ok();
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
        cr.move_to(bx + (bw - tw as f64) / 2.0, by + (bh - th as f64) / 2.0);
        pangocairo::functions::show_layout(&cr, &layout);
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
}
