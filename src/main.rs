mod config;
mod ipc;
mod render;

use config::Config;
use render::Thumbnail;
use ipc::WorkspaceInfo;

use std::collections::VecDeque;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};

use wayland_client::{
    protocol::{
        wl_buffer, wl_compositor, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm,
        wl_shm_pool, wl_surface,
    },
    Connection, Dispatch, EventQueue, QueueHandle, WEnum,
};
use wayland_cursor::CursorTheme;

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};

// Generated Hyprland protocol bindings
#[allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
#[allow(non_upper_case_globals, non_snake_case, unused_imports, missing_docs)]
#[allow(clippy::all)]
pub mod hyprland_toplevel_export {
    use wayland_client;
    use wayland_client::protocol::*;
    use wayland_protocols_wlr::foreign_toplevel::v1::client::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        use wayland_protocols_wlr::foreign_toplevel::v1::client::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/hyprland-toplevel-export-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/hyprland-toplevel-export-v1.xml");
}
use hyprland_toplevel_export::{
    hyprland_toplevel_export_frame_v1, hyprland_toplevel_export_manager_v1,
};
use hyprland_toplevel_export_manager_v1::HyprlandToplevelExportManagerV1;
use hyprland_toplevel_export_frame_v1::HyprlandToplevelExportFrameV1;

// ── signal handling ─────────────────────────────────────────────────────────

static G_QUIT: AtomicBool = AtomicBool::new(false);
static G_TOGGLE: AtomicBool = AtomicBool::new(false);
static G_RELOAD: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn sig_handler(sig: libc::c_int) {
    if sig == libc::SIGUSR1 {
        G_TOGGLE.store(true, Ordering::Relaxed);
    } else if sig == libc::SIGHUP {
        G_RELOAD.store(true, Ordering::Relaxed);
    } else {
        G_QUIT.store(true, Ordering::Relaxed);
    }
}

// ── key mapping (evdev keycodes → XKB keysyms) ──────────────────────────────

const XKB_KEY_ESCAPE:    u32 = 0xff1b;
const XKB_KEY_RETURN:    u32 = 0xff0d;
const XKB_KEY_BACKSPACE: u32 = 0xff08;
const XKB_KEY_LEFT:      u32 = 0xff51;
const XKB_KEY_UP:        u32 = 0xff52;
const XKB_KEY_RIGHT:     u32 = 0xff53;
const XKB_KEY_DOWN:      u32 = 0xff54;
const XKB_KEY_DELETE:    u32 = 0xffff;
const XKB_KEY_1: u32 = 0x0031;
const XKB_KEY_9: u32 = 0x0039;

fn key_name_to_keysym(name: &str) -> Option<u32> {
    match name {
        "Escape"         => Some(XKB_KEY_ESCAPE),
        "Return" | "Enter" => Some(XKB_KEY_RETURN),
        "Left"           => Some(XKB_KEY_LEFT),
        "Up"             => Some(XKB_KEY_UP),
        "Right"          => Some(XKB_KEY_RIGHT),
        "Down"           => Some(XKB_KEY_DOWN),
        "space" | "Space" => Some(0x0020),
        "Delete"          => Some(XKB_KEY_DELETE),
        _ if name.len() == 1 => {
            let c = name.chars().next()?;
            if c.is_ascii_graphic() { Some(c as u32) } else { None }
        }
        _ => None,
    }
}

fn resolve_keys(names: &[String]) -> Vec<u32> {
    names.iter().filter_map(|n| key_name_to_keysym(n)).collect()
}

struct ResolvedKeys {
    close:        Vec<u32>,
    confirm:      Vec<u32>,
    left:         Vec<u32>,
    right:        Vec<u32>,
    up:           Vec<u32>,
    down:         Vec<u32>,
    move_window:  Vec<u32>,
    close_window: Vec<u32>,
}

impl ResolvedKeys {
    fn from_config(kb: &config::KeybindingsConfig) -> Self {
        Self {
            close:        resolve_keys(&kb.close),
            confirm:      resolve_keys(&kb.confirm),
            left:         resolve_keys(&kb.left),
            right:        resolve_keys(&kb.right),
            up:           resolve_keys(&kb.up),
            down:         resolve_keys(&kb.down),
            move_window:  resolve_keys(&kb.move_window),
            close_window: resolve_keys(&kb.close_window),
        }
    }
}

fn keycode_to_keysym(keycode: u32) -> u32 {
    // Compositors send raw evdev keycodes via wl_keyboard.
    // Letter keysyms equal their ASCII value.
    match keycode {
        1   => XKB_KEY_ESCAPE,
        14  => XKB_KEY_BACKSPACE,
        28  => XKB_KEY_RETURN,
        57  => 0x0020, // space
        103 => XKB_KEY_UP,
        105 => XKB_KEY_LEFT,
        106 => XKB_KEY_RIGHT,
        108 => XKB_KEY_DOWN,
        // digits 1-9 (evdev 2-10) and 0 (11)
        2  => 0x31, 3  => 0x32, 4  => 0x33, 5  => 0x34, 6  => 0x35,
        7  => 0x36, 8  => 0x37, 9  => 0x38, 10 => 0x39, 11 => 0x30,
        // top row: q w e r t y u i o p
        16 => 0x71, 17 => 0x77, 18 => 0x65, 19 => 0x72, 20 => 0x74,
        21 => 0x79, 22 => 0x75, 23 => 0x69, 24 => 0x6f, 25 => 0x70,
        // home row: a s d f g h j k l
        30 => 0x61, 31 => 0x73, 32 => 0x64, 33 => 0x66, 34 => 0x67,
        35 => 0x68, 36 => 0x6a, 37 => 0x6b, 38 => 0x6c,
        // bottom row: z x c v b n m
        44 => 0x7a, 45 => 0x78, 46 => 0x63, 47 => 0x76, 48 => 0x62,
        49 => 0x6e, 50 => 0x6d,
        111 => XKB_KEY_DELETE,
        _  => 0,
    }
}

// ── capture frame state ──────────────────────────────────────────────────────

#[derive(Default)]
struct CaptureFrameData {
    format: u32,
    width: u32,
    height: u32,
    stride: u32,
    buffer_done: bool,
    frame_ready: bool,
    frame_failed: bool,
}

enum CapturePhase {
    Phase1,
    Phase2 {
        wl_buf: wl_buffer::WlBuffer,
        mmap_ptr: *mut libc::c_void,
        size: usize,
        width: u32,
        height: u32,
        stride: u32,
    },
}

struct ActiveCapture {
    frame: HyprlandToplevelExportFrameV1,
    address: u64,
    phase: CapturePhase,
    started: std::time::Instant,
}

// ── application state ────────────────────────────────────────────────────────

struct AppState {
    // Wayland globals
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    seat: Option<wl_seat::WlSeat>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    toplevel_export: Option<HyprlandToplevelExportManagerV1>,

    // Per-show Wayland objects (recreated on each show)
    wl_surf: Option<wl_surface::WlSurface>,
    layer_surface: Option<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>,
    wl_buf: Option<wl_buffer::WlBuffer>,

    // Surface / overlay state
    width: u32,
    height: u32,
    configured: bool,
    visible: bool,
    should_close: bool,
    needs_redraw: bool,

    // App state
    workspaces: Vec<WorkspaceInfo>,
    thumbnails: Vec<Thumbnail>,
    selected: usize,
    no_preview: bool,
    active_window_address: u64,
    config: Config,
    keys: ResolvedKeys,

    // Type-to-filter
    filter: String,
    filter_match: Option<u64>,

    // Mouse support (only active when --allow-mouse is set).
    allow_mouse: bool,
    pointer_x: f64,
    pointer_y: f64,
    hovered_window: Option<u64>,
    cursor_surface: Option<wl_surface::WlSurface>,
    cursor_theme: Option<CursorTheme>,
    cursor_enter_serial: u32,
    cursor_is_pointer: bool,

    // Async thumbnail capture
    pending_capture: Option<CaptureFrameData>,
    capture_queue: VecDeque<u64>,
    active_capture: Option<ActiveCapture>,

    // Window to focus after the overlay surface is fully destroyed: (workspace_id, address, cx, cy)
    pending_focus: Option<(i32, u64, i32, i32)>,
}

fn find_single_window_match(workspaces: &[WorkspaceInfo], filter: &str) -> Option<u64> {
    let f = filter.to_lowercase();
    let mut found = None;
    for ws in workspaces {
        for c in &ws.clients {
            if c.class_name.to_lowercase().contains(&f) || c.title.to_lowercase().contains(&f) {
                if found.is_some() { return None; } // more than one
                found = Some(c.address);
            }
        }
    }
    found
}

fn workspace_matches(ws: &WorkspaceInfo, filter: &str) -> bool {
    ws.name.to_lowercase().contains(filter)
        || ws.clients.iter().any(|c| {
            c.class_name.to_lowercase().contains(filter)
                || c.title.to_lowercase().contains(filter)
        })
}

impl AppState {
    fn new(no_preview: bool, allow_mouse: bool, config: Config) -> Self {
        // CLI flags override config values (OR semantics: flag=true forces on)
        let no_preview = no_preview || config.behavior.no_preview;
        let allow_mouse = allow_mouse || config.behavior.allow_mouse;
        Self {
            compositor: None,
            shm: None,
            seat: None,
            layer_shell: None,
            toplevel_export: None,
            wl_surf: None,
            layer_surface: None,
            wl_buf: None,
            width: 0,
            height: 0,
            configured: false,
            visible: false,
            should_close: false,
            needs_redraw: false,
            workspaces: Vec::new(),
            thumbnails: Vec::new(),
            selected: 0,
            no_preview,
            active_window_address: 0,
            keys: ResolvedKeys::from_config(&config.keybindings),
            config,
            filter: String::new(),
            filter_match: None,
            allow_mouse,
            pointer_x: 0.0,
            pointer_y: 0.0,
            hovered_window: None,
            cursor_surface: None,
            cursor_theme: None,
            cursor_enter_serial: 0,
            cursor_is_pointer: false,
            pending_capture: None,
            capture_queue: VecDeque::new(),
            active_capture: None,
            pending_focus: None,
        }
    }

    fn init_cursor(&mut self, conn: &Connection, qh: &QueueHandle<Self>) {
        if self.cursor_surface.is_some() {
            return;
        }
        let compositor = self.compositor.clone();
        let shm = self.shm.clone();
        if let (Some(compositor), Some(shm)) = (compositor, shm) {
            self.cursor_surface = Some(compositor.create_surface(qh, ()));
            self.cursor_theme = CursorTheme::load(conn, shm, 24).ok();
        }
    }

    fn apply_cursor(&mut self, pointer: &wl_pointer::WlPointer, is_pointer: bool) {
        let surf = match self.cursor_surface.clone() {
            Some(s) => s,
            None => return,
        };
        let serial = self.cursor_enter_serial;
        let theme = match self.cursor_theme.as_mut() {
            Some(t) => t,
            None => return,
        };
        let fallbacks: &[&str] = if is_pointer {
            &["pointer", "hand2"]
        } else {
            &["default", "left_ptr"]
        };
        let mut found = false;
        for name in fallbacks {
            if let Some(cursor) = theme.get_cursor(name) {
                let image = &cursor[0];
                let (hx, hy) = image.hotspot();
                surf.attach(Some(&**image), 0, 0);
                surf.damage_buffer(0, 0, i32::MAX, i32::MAX);
                surf.commit();
                pointer.set_cursor(serial, Some(&surf), hx as i32, hy as i32);
                found = true;
                break;
            }
        }
        if !found {
            pointer.set_cursor(serial, None, 0, 0);
        }
    }

    /// Returns true if the overlay should be closed.
    fn handle_key(&mut self, keysym: u32) -> bool {
        let n = self.workspaces.len();
        if n == 0 { return true; }
        let cols = ((n as f64).sqrt().ceil() as usize).max(1);
        let k = keysym;

        // Backspace: pop filter char
        if k == XKB_KEY_BACKSPACE {
            if self.filter.pop().is_some() {
                self.update_filter_state();
                self.needs_redraw = true;
            }
            return false;
        }

        // Escape: clear filter if active, else close
        if self.keys.close.contains(&k) {
            if !self.filter.is_empty() {
                self.filter.clear();
                self.filter_match = None;
                self.needs_redraw = true;
                return false;
            }
            return true;
        }

        // Confirm: if single window match, focus it directly; else activate workspace
        if self.keys.confirm.contains(&k) {
            if let Some(addr) = self.filter_match {
                let client = self.workspaces.iter()
                    .flat_map(|ws| ws.clients.iter())
                    .find(|c| c.address == addr);
                if let Some(c) = client {
                    self.pending_focus = Some((c.workspace_id, addr, c.x + c.w / 2, c.y + c.h / 2));
                    return true;
                }
            }
            if !self.filter.is_empty() {
                let f = self.filter.to_lowercase();
                if let Some(i) = self.workspaces.iter().position(|ws| workspace_matches(ws, &f)) {
                    self.selected = i;
                }
            }
            self.activate_selected_workspace();
            return true;
        }

        // Number keys: direct jump (only without filter)
        if self.filter.is_empty() && (XKB_KEY_1..=XKB_KEY_9).contains(&k) {
            let idx = (k - XKB_KEY_1) as usize;
            if idx < n { ipc::switch_workspace(&self.workspaces[idx]); }
            return true;
        }

        // Arrow keys always navigate; hjkl only when filter is empty
        let is_arrow = matches!(k, XKB_KEY_LEFT | XKB_KEY_RIGHT | XKB_KEY_UP | XKB_KEY_DOWN);
        if is_arrow || self.filter.is_empty() {
            if self.keys.right.contains(&k) && self.selected + 1 < n {
                self.selected += 1; self.needs_redraw = true; return false;
            }
            if self.keys.left.contains(&k) && self.selected > 0 {
                self.selected -= 1; self.needs_redraw = true; return false;
            }
            if self.keys.down.contains(&k) && self.selected + cols < n {
                self.selected += cols; self.needs_redraw = true; return false;
            }
            if self.keys.up.contains(&k) && self.selected >= cols {
                self.selected -= cols; self.needs_redraw = true; return false;
            }
        }

        // Move window (only without filter)
        if self.filter.is_empty() && self.keys.move_window.contains(&k) && self.move_active_window_to_selected() {
            return true;
        }

        // Close window: target hovered window first, then single filter match
        if self.keys.close_window.contains(&k) {
            let addr = self.hovered_window.or(self.filter_match);
            if let Some(addr) = addr {
                ipc::close_window(addr);
                for ws in &mut self.workspaces {
                    ws.clients.retain(|c| c.address != addr);
                }
                if self.hovered_window == Some(addr) { self.hovered_window = None; }
                self.update_filter_state();
                self.needs_redraw = true;
            }
            return false;
        }

        // Printable char: append to filter
        if let Some(c) = char::from_u32(k).filter(|c| c.is_ascii_graphic() || *c == ' ') {
            self.filter.push(c);
            self.update_filter_state();
            self.needs_redraw = true;
        }

        false
    }

    fn update_filter_state(&mut self) {
        let f = self.filter.to_lowercase();
        self.filter_match = if f.is_empty() {
            None
        } else {
            find_single_window_match(&self.workspaces, &f)
        };
        if let Some(i) = self.workspaces.iter().position(|ws| workspace_matches(ws, &f)) {
            self.selected = i;
        }
    }

    fn activate_selected_workspace(&self) {
        if self.selected < self.workspaces.len() {
            ipc::switch_workspace(&self.workspaces[self.selected]);
        }
    }

    /// Move the active window to the selected workspace. Returns true on success
    /// (overlay should close).
    fn move_active_window_to_selected(&self) -> bool {
        if self.active_window_address == 0 || self.selected >= self.workspaces.len() {
            return false;
        }
        let target = &self.workspaces[self.selected];
        ipc::move_window_to_workspace(self.active_window_address, target);
        if self.config.behavior.switch_on_move {
            ipc::switch_workspace(target);
        }
        true
    }

    fn workspace_at(&self, x: f64, y: f64) -> Option<usize> {
        let n = self.workspaces.len();
        if n == 0 || self.width == 0 || self.height == 0 {
            return None;
        }

        let cols = ((n as f64).sqrt().ceil() as usize).max(1);
        let rows = n.div_ceil(cols);
        let pad = self.config.appearance.card_padding;

        let card_w = ((self.width as f64 - pad * (cols + 1) as f64) / cols as f64)
            .min(self.config.appearance.max_card_width);
        let card_h = ((self.height as f64 - pad * (rows + 1) as f64) / rows as f64)
            .min(self.config.appearance.max_card_height);

        let grid_w = cols as f64 * card_w + (cols.saturating_sub(1)) as f64 * pad;
        let grid_h = rows as f64 * card_h + (rows.saturating_sub(1)) as f64 * pad;
        let ox = (self.width as f64 - grid_w) / 2.0;
        let oy = (self.height as f64 - grid_h) / 2.0;

        for i in 0..n {
            let col = i % cols;
            let row = i / cols;
            let cx = ox + col as f64 * (card_w + pad);
            let cy = oy + row as f64 * (card_h + pad);
            if x >= cx && x <= cx + card_w && y >= cy && y <= cy + card_h {
                return Some(i);
            }
        }
        None
    }

    fn select_workspace_at(&mut self, x: f64, y: f64) {
        if let Some(i) = self.workspace_at(x, y) {
            if self.selected != i {
                self.selected = i;
                self.needs_redraw = true;
            }
        }
    }

    // Returns the address of the window under (x, y), or None if the click
    // landed on the card background or outside all cards.
    fn window_at(&self, x: f64, y: f64) -> Option<u64> {
        let n = self.workspaces.len();
        if n == 0 || self.width == 0 || self.height == 0 {
            return None;
        }
        let cols = ((n as f64).sqrt().ceil() as usize).max(1);
        let rows = n.div_ceil(cols);
        let pad = self.config.appearance.card_padding;
        let card_w = ((self.width as f64 - pad * (cols + 1) as f64) / cols as f64)
            .min(self.config.appearance.max_card_width);
        let card_h = ((self.height as f64 - pad * (rows + 1) as f64) / rows as f64)
            .min(self.config.appearance.max_card_height);
        let grid_w = cols as f64 * card_w + (cols.saturating_sub(1)) as f64 * pad;
        let grid_h = rows as f64 * card_h + (rows.saturating_sub(1)) as f64 * pad;
        let ox = (self.width as f64 - grid_w) / 2.0;
        let oy = (self.height as f64 - grid_h) / 2.0;
        let lh = self.config.appearance.label_height;
        let tp = self.config.appearance.thumb_padding;

        for (i, ws) in self.workspaces.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let cx = ox + col as f64 * (card_w + pad);
            let cy = oy + row as f64 * (card_h + pad);
            if x < cx || x > cx + card_w || y < cy || y > cy + card_h {
                continue;
            }
            // Inside this card — hit-test individual windows
            let win_x = cx + tp;
            let win_y = cy + lh;
            let win_w = card_w - 2.0 * tp;
            let win_h = card_h - lh - tp;
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
                let rx = off_x + (client.x - min_x) as f64 * scale;
                let ry = off_y + (client.y - min_y) as f64 * scale;
                let rw = client.w as f64 * scale;
                let rh = client.h as f64 * scale;
                if x >= rx && x <= rx + rw && y >= ry && y <= ry + rh {
                    return Some(client.address);
                }
            }
            return None;
        }
        None
    }
}

// ── wayland Dispatch impls ───────────────────────────────────────────────────

impl Dispatch<wl_registry::WlRegistry, ()> for AppState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global { name, interface, version } = event else { return };
        match interface.as_str() {
            "wl_compositor" => {
                state.compositor = Some(registry.bind(name, version.min(4), qh, ()));
            }
            "wl_shm" => {
                state.shm = Some(registry.bind(name, 1, qh, ()));
            }
            "wl_seat" => {
                state.seat = Some(registry.bind(name, version.min(7), qh, ()));
            }
            "zwlr_layer_shell_v1" => {
                state.layer_shell = Some(registry.bind(name, version.min(4), qh, ()));
            }
            "hyprland_toplevel_export_manager_v1" => {
                state.toplevel_export = Some(registry.bind(name, 1, qh, ()));
            }
            _ => {}
        }
    }
}

wayland_client::delegate_noop!(AppState: ignore wl_compositor::WlCompositor);
wayland_client::delegate_noop!(AppState: ignore wl_shm_pool::WlShmPool);
wayland_client::delegate_noop!(AppState: ignore zwlr_layer_shell_v1::ZwlrLayerShellV1);
wayland_client::delegate_noop!(AppState: ignore HyprlandToplevelExportManagerV1);

impl Dispatch<wl_shm::WlShm, ()> for AppState {
    fn event(_: &mut Self, _: &wl_shm::WlShm, _: wl_shm::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_buffer::WlBuffer, ()> for AppState {
    fn event(_: &mut Self, _: &wl_buffer::WlBuffer, _: wl_buffer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_surface::WlSurface, ()> for AppState {
    fn event(_: &mut Self, _: &wl_surface::WlSurface, _: wl_surface::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_seat::WlSeat, ()> for AppState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(caps) } = event {
            if caps.contains(wl_seat::Capability::Keyboard) {
                seat.get_keyboard(qh, ());
            }
            if state.allow_mouse && caps.contains(wl_seat::Capability::Pointer) {
                seat.get_pointer(qh, ());
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Key {
            key,
            state: WEnum::Value(wl_keyboard::KeyState::Pressed),
            ..
        } = event
        {
            let keysym = keycode_to_keysym(key);
            if keysym != 0 && state.handle_key(keysym) {
                state.should_close = true;
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for AppState {
    fn event(
        state: &mut Self,
        pointer: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if !state.allow_mouse {
            return;
        }
        match event {
            wl_pointer::Event::Enter { serial, surface_x, surface_y, .. } => {
                state.pointer_x = surface_x;
                state.pointer_y = surface_y;
                state.cursor_enter_serial = serial;
                state.cursor_is_pointer = false;
                state.select_workspace_at(surface_x, surface_y);
                state.init_cursor(conn, qh);
                let win = state.window_at(surface_x, surface_y);
                state.apply_cursor(pointer, win.is_some());
                state.cursor_is_pointer = win.is_some();
                if state.hovered_window != win {
                    state.hovered_window = win;
                    state.needs_redraw = true;
                }
            }
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                state.pointer_x = surface_x;
                state.pointer_y = surface_y;
                state.select_workspace_at(surface_x, surface_y);
                let win = state.window_at(surface_x, surface_y);
                if win.is_some() != state.cursor_is_pointer {
                    state.apply_cursor(pointer, win.is_some());
                    state.cursor_is_pointer = win.is_some();
                }
                if state.hovered_window != win {
                    state.hovered_window = win;
                    state.needs_redraw = true;
                }
            }
            wl_pointer::Event::Button {
                button,
                state: WEnum::Value(wl_pointer::ButtonState::Pressed),
                ..
            } => {
                if state.workspace_at(state.pointer_x, state.pointer_y).is_none() {
                    state.should_close = true;
                    return;
                }
                match button {
                    BTN_LEFT => {
                        let win = state.window_at(state.pointer_x, state.pointer_y);
                        if let Some(addr) = win {
                            let client = state.workspaces.iter()
                                .flat_map(|ws| ws.clients.iter())
                                .find(|c| c.address == addr);
                            if let Some(c) = client {
                                state.pending_focus = Some((c.workspace_id, addr, c.x + c.w / 2, c.y + c.h / 2));
                            }
                        } else {
                            state.activate_selected_workspace();
                        }
                        state.should_close = true;
                    }
                    BTN_RIGHT if state.move_active_window_to_selected() => {
                        state.should_close = true;
                    }
                    _ => {}
                }
            }
            wl_pointer::Event::Leave { serial, .. } => {
                pointer.set_cursor(serial, None, 0, 0);
                state.cursor_is_pointer = false;
                if state.hovered_window.is_some() {
                    state.hovered_window = None;
                    state.needs_redraw = true;
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for AppState {
    fn event(
        state: &mut Self,
        layer_surface: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                layer_surface.ack_configure(serial);
                state.width = width;
                state.height = height;
                state.configured = true;
            }
            zwlr_layer_surface_v1::Event::Closed => {
                state.should_close = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<HyprlandToplevelExportFrameV1, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &HyprlandToplevelExportFrameV1,
        event: hyprland_toplevel_export_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use hyprland_toplevel_export_frame_v1::Event;
        let Some(cap) = state.pending_capture.as_mut() else { return };
        match event {
            Event::Buffer { format, width, height, stride } => {
                // Convert WEnum<wl_shm::Format> to u32
                cap.format = format.into();
                cap.width = width;
                cap.height = height;
                cap.stride = stride;
            }
            Event::BufferDone => cap.buffer_done = true,
            Event::Ready { .. } => cap.frame_ready = true,
            Event::Failed => cap.frame_failed = true,
            _ => {}
        }
    }
}

// ── Wayland surface management ───────────────────────────────────────────────

fn create_shm_fd(size: usize) -> Option<OwnedFd> {
    let name = c"hyprexpose";
    let raw = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    if raw < 0 {
        return None;
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    if unsafe { libc::ftruncate(raw, size as libc::off_t) } < 0 {
        return None;
    }
    Some(fd)
}

fn show(state: &mut AppState, qh: &QueueHandle<AppState>) -> bool {
    let (compositor, layer_shell) = match (&state.compositor, &state.layer_shell) {
        (Some(c), Some(l)) => (c, l),
        _ => return false,
    };

    state.should_close = false;
    state.configured = false;

    let wl_surf = compositor.create_surface(qh, ());
    let layer_surface = layer_shell.get_layer_surface(
        &wl_surf,
        None,
        zwlr_layer_shell_v1::Layer::Top,
        "hyprexpose".to_owned(),
        qh,
        (),
    );

    use zwlr_layer_surface_v1::Anchor;
    layer_surface.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
    layer_surface.set_exclusive_zone(-1);
    layer_surface.set_keyboard_interactivity(
        zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive,
    );

    wl_surf.commit();

    state.wl_surf = Some(wl_surf);
    state.layer_surface = Some(layer_surface);
    true
}

fn hide(state: &mut AppState) {
    // Cancel any in-flight thumbnail capture
    state.capture_queue.clear();
    state.pending_capture = None;
    if let Some(cap) = state.active_capture.take() {
        if let CapturePhase::Phase2 { wl_buf, mmap_ptr, size, .. } = cap.phase {
            unsafe { libc::munmap(mmap_ptr, size) };
            wl_buf.destroy();
        }
        cap.frame.destroy();
    }

    if let Some(buf) = state.wl_buf.take() {
        buf.destroy();
    }
    if let Some(ls) = state.layer_surface.take() {
        ls.destroy();
    }
    if let Some(surf) = state.wl_surf.take() {
        surf.destroy();
    }
    state.configured = false;
    state.visible = false;
    state.filter.clear();
    state.workspaces.clear();
    state.thumbnails.clear();
}

fn redraw(state: &mut AppState, qh: &QueueHandle<AppState>) {
    if !state.configured || state.width == 0 || state.height == 0 {
        return;
    }

    let visible_workspaces: Vec<_>;
    let workspaces_to_draw: &[_] = if state.config.behavior.show_empty_workspaces {
        &state.workspaces
    } else {
        visible_workspaces = state.workspaces.iter()
            .filter(|ws| !ws.clients.is_empty())
            .cloned()
            .collect();
        &visible_workspaces
    };

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
    });

    let stride = state.width * 4;
    let size = (stride * state.height) as usize;

    let Some(fd) = create_shm_fd(size) else { return };
    let raw_fd = fd.as_fd().as_raw_fd();

    let written = unsafe { libc::write(raw_fd, pixels.as_ptr() as *const libc::c_void, size) };
    if written < 0 {
        return;
    }

    let Some(shm) = &state.shm else { return };
    let pool = shm.create_pool(fd.as_fd(), size as i32, qh, ());
    drop(fd); // compositor mapped it; safe to close our end

    let buf = pool.create_buffer(
        0,
        state.width as i32,
        state.height as i32,
        stride as i32,
        wl_shm::Format::Argb8888,
        qh,
        (),
    );
    pool.destroy();

    if let Some(surf) = &state.wl_surf {
        surf.attach(Some(&buf), 0, 0);
        surf.damage_buffer(0, 0, state.width as i32, state.height as i32);
        surf.commit();
    }

    if let Some(old) = state.wl_buf.replace(buf) {
        old.destroy();
    }
}

// ── async thumbnail capture ──────────────────────────────────────────────────

// Advance the capture state machine by one step. Call once per main-loop
// iteration while the overlay is visible.
fn advance_capture(conn: &Connection, state: &mut AppState, qh: &QueueHandle<AppState>) {
    const TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

    let active = match state.active_capture.take() {
        Some(a) => a,
        None => {
            // Start the next capture if the queue is non-empty.
            if state.no_preview || state.toplevel_export.is_none() {
                return;
            }
            let address = match state.capture_queue.pop_front() {
                Some(a) => a,
                None => return,
            };
            let frame = state.toplevel_export.as_ref().unwrap()
                .capture_toplevel(0, address as u32, qh, ());
            state.pending_capture = Some(CaptureFrameData::default());
            conn.flush().ok();
            state.active_capture = Some(ActiveCapture {
                frame,
                address,
                phase: CapturePhase::Phase1,
                started: std::time::Instant::now(),
            });
            return;
        }
    };

    let timed_out = active.started.elapsed() > TIMEOUT;

    match active.phase {
        CapturePhase::Phase1 => {
            let cap = match state.pending_capture.as_ref() {
                Some(c) => c,
                None => { active.frame.destroy(); return; }
            };

            if timed_out || cap.frame_failed {
                active.frame.destroy();
                state.pending_capture = None;
                return;
            }

            if !cap.buffer_done {
                state.active_capture = Some(active);
                return;
            }

            // buffer_done — transition to phase 2
            let cap = state.pending_capture.take().unwrap();
            if cap.width == 0 || cap.height == 0 {
                active.frame.destroy();
                return;
            }

            let size = (cap.stride * cap.height) as usize;
            let raw_fd = unsafe {
                libc::memfd_create(c"hyprexpose-cap".as_ptr(), libc::MFD_CLOEXEC)
            };
            if raw_fd < 0 { active.frame.destroy(); return; }
            unsafe { libc::ftruncate(raw_fd, size as libc::off_t) };

            let mmap_ptr = unsafe {
                libc::mmap(std::ptr::null_mut(), size,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED, raw_fd, 0)
            };
            if mmap_ptr == libc::MAP_FAILED {
                unsafe { libc::close(raw_fd) };
                active.frame.destroy();
                return;
            }

            let owned_fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
            let Some(shm) = &state.shm else {
                unsafe { libc::munmap(mmap_ptr, size) };
                active.frame.destroy();
                return;
            };

            let wl_fmt = wl_shm::Format::try_from(cap.format)
                .unwrap_or(wl_shm::Format::Argb8888);
            let pool = shm.create_pool(owned_fd.as_fd(), size as i32, qh, ());
            let wl_buf = pool.create_buffer(
                0, cap.width as i32, cap.height as i32, cap.stride as i32, wl_fmt, qh, ());
            pool.destroy();
            drop(owned_fd);

            state.pending_capture = Some(CaptureFrameData {
                width: cap.width, height: cap.height,
                stride: cap.stride, format: cap.format,
                ..Default::default()
            });
            active.frame.copy(&wl_buf, 1);
            conn.flush().ok();

            state.active_capture = Some(ActiveCapture {
                frame: active.frame,
                address: active.address,
                phase: CapturePhase::Phase2 {
                    wl_buf, mmap_ptr, size,
                    width: cap.width, height: cap.height, stride: cap.stride,
                },
                started: active.started,
            });
        }

        CapturePhase::Phase2 { wl_buf, mmap_ptr, size, width, height, stride } => {
            let cap = state.pending_capture.as_ref();
            let ready = cap.is_some_and(|c| c.frame_ready);
            let failed = timed_out || cap.is_none_or(|c| c.frame_failed);

            if !ready && !failed {
                state.active_capture = Some(ActiveCapture {
                    frame: active.frame,
                    address: active.address,
                    phase: CapturePhase::Phase2 { wl_buf, mmap_ptr, size, width, height, stride },
                    started: active.started,
                });
                return;
            }

            state.pending_capture = None;

            if ready {
                let pixels = unsafe {
                    std::slice::from_raw_parts(mmap_ptr as *const u8, size).to_vec()
                };
                state.thumbnails.push(Thumbnail {
                    address: active.address,
                    data: pixels,
                    width, height, stride,
                });
                state.needs_redraw = true;
            }

            unsafe { libc::munmap(mmap_ptr, size) };
            wl_buf.destroy();
            active.frame.destroy();
        }
    }
}

// ── main ─────────────────────────────────────────────────────────────────────

fn try_lock_instance() -> Option<std::fs::File> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let path = format!("{runtime_dir}/hyprexpose.lock");
    let file = std::fs::OpenOptions::new().create(true).write(true).open(&path).ok()?;
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret == 0 { Some(file) } else { None }
}

fn main() {
    let _lock = match try_lock_instance() {
        Some(f) => f,
        None => std::process::exit(0),
    };

    let mut no_preview = false;
    let mut allow_mouse = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--no-preview" => no_preview = true,
            "--allow-mouse" => allow_mouse = true,
            _ => {
                eprintln!("Usage: hyprexpose [--no-preview] [--allow-mouse]");
                std::process::exit(1);
            }
        }
    }

    // Install signal handlers and block them outside ppoll to eliminate the race
    // where a signal fires between the G_TOGGLE check and the poll call.
    let unblocked_mask = unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = sig_handler as *const () as libc::sighandler_t;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGUSR1, &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGHUP,  &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGINT,  &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());

        // Block these signals normally; ppoll atomically unblocks them during
        // the wait so they can never land in the gap before poll starts.
        let mut block_mask: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut block_mask);
        libc::sigaddset(&mut block_mask, libc::SIGUSR1);
        libc::sigaddset(&mut block_mask, libc::SIGHUP);
        libc::sigaddset(&mut block_mask, libc::SIGINT);
        libc::sigaddset(&mut block_mask, libc::SIGTERM);
        libc::sigprocmask(libc::SIG_BLOCK, &block_mask, std::ptr::null_mut());

        let mut empty: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut empty);
        empty
    };

    let conn = Connection::connect_to_env().expect("failed to connect to Wayland display");
    let mut eq: EventQueue<AppState> = conn.new_event_queue();
    let qh = eq.handle();

    let config = Config::load();
    let mut state = AppState::new(no_preview, allow_mouse, config);

    // Register registry listener and perform two roundtrips to discover all globals
    conn.display().get_registry(&qh, ());
    eq.roundtrip(&mut state).expect("Wayland roundtrip failed");
    eq.roundtrip(&mut state).expect("Wayland roundtrip failed");

    if state.compositor.is_none() || state.shm.is_none() || state.layer_shell.is_none() {
        eprintln!("hyprexpose: missing required Wayland globals");
        std::process::exit(1);
    }

    if state.toplevel_export.is_none() && !state.no_preview {
        eprintln!(
            "hyprexpose: hyprland-toplevel-export protocol unavailable; \
             window previews disabled (colored rectangles will be used)"
        );
    }

    eprintln!(
        "hyprexpose: daemon running on {} (SIGUSR1 to toggle, SIGHUP to reload config)",
        ipc::compositor().name()
    );

    let display_fd = conn.as_fd().as_raw_fd();

    // Watch the config file's parent directory for writes/renames so we can
    // auto-reload when the file changes. Editors often do atomic saves by
    // writing a temp file then renaming it, so IN_MOVED_TO is essential.
    let config_path = config::config_path();
    let config_filename = config_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config.toml")
        .to_owned();
    let inotify_fd = unsafe {
        let fd = libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC);
        if fd >= 0 {
            if let Some(dir) = config_path.parent() {
                if let Ok(cpath) = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes()) {
                    libc::inotify_add_watch(
                        fd,
                        cpath.as_ptr(),
                        libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO,
                    );
                }
            }
        }
        fd
    };

    'main: loop {
        if G_QUIT.load(Ordering::Relaxed) {
            break;
        }

        if G_RELOAD.swap(false, Ordering::Relaxed) {
            let new_config = Config::load();
            state.keys = ResolvedKeys::from_config(&new_config.keybindings);
            state.config = new_config;
            if state.visible { state.needs_redraw = true; }
            eprintln!("hyprexpose: config reloaded");
        }

        if G_TOGGLE.swap(false, Ordering::Relaxed) {
            if state.visible {
                hide(&mut state);
                conn.flush().ok();
            } else if show(&mut state, &qh) {
                conn.flush().ok();
                eq.roundtrip(&mut state).ok();
                if state.configured {
                    // Fetch workspace/window data (fast IPC calls)
                    state.active_window_address = ipc::get_active_window_address();
                    state.workspaces = ipc::get_workspaces();
                    state.thumbnails.clear();
                    let active = ipc::get_active_workspace_name();
                    state.selected = state.workspaces.iter()
                        .position(|ws| ws.name == active).unwrap_or(0);
                    state.visible = true;

                    // Enqueue thumbnail captures; overlay draws immediately
                    // with colored rectangles and redraws as thumbnails arrive.
                    state.capture_queue.extend(
                        state.workspaces.iter()
                            .flat_map(|ws| ws.clients.iter().map(|c| c.address))
                    );

                    redraw(&mut state, &qh);
                    conn.flush().ok();
                }
            }
        }

        if state.should_close && state.visible {
            hide(&mut state);
            conn.flush().ok();
            state.should_close = false;
        }

        if let Some((ws_id, addr, cx, cy)) = state.pending_focus.take() {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(100));
                ipc::focus_window(ws_id, addr, cx, cy);
            });
        }

        if state.needs_redraw && state.visible {
            state.needs_redraw = false;
            redraw(&mut state, &qh);
            conn.flush().ok();
        }

        // Advance the async thumbnail capture state machine.
        if state.visible {
            advance_capture(&conn, &mut state, &qh);
        }

        conn.flush().ok();

        // Poll for events using prepare_read. Use a short timeout while
        // captures are pending so we can check for per-capture timeouts.
        let guard = match eq.prepare_read() {
            Some(g) => g,
            None => {
                eq.dispatch_pending(&mut state).ok();
                continue;
            }
        };

        let capturing = state.active_capture.is_some() || !state.capture_queue.is_empty();
        let poll_timeout = if capturing {
            &libc::timespec { tv_sec: 0, tv_nsec: 50_000_000 } as *const libc::timespec
        } else {
            std::ptr::null()
        };

        let mut pollfds = [
            libc::pollfd { fd: display_fd,  events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: inotify_fd,  events: libc::POLLIN, revents: 0 },
        ];
        let nfds = if inotify_fd >= 0 { 2u64 } else { 1u64 };

        let ret = unsafe { libc::ppoll(pollfds.as_mut_ptr(), nfds, poll_timeout, &unblocked_mask) };

        if ret < 0 {
            drop(guard);
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                // signal interrupted poll — loop back to check flags
                eq.dispatch_pending(&mut state).ok();
                continue;
            }
            break 'main;
        }

        // Check inotify before Wayland so a config change doesn't race a redraw
        if inotify_fd >= 0 && pollfds[1].revents & libc::POLLIN != 0 {
            let mut buf = [0u8; 4096];
            let n = unsafe {
                libc::read(inotify_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
            };
            if n > 0 {
                let ev_size = std::mem::size_of::<libc::inotify_event>();
                let mut offset = 0usize;
                while offset + ev_size <= n as usize {
                    let ev = unsafe { &*(buf.as_ptr().add(offset) as *const libc::inotify_event) };
                    let name_len = ev.len as usize;
                    if name_len > 0 {
                        let name_bytes = &buf[offset + ev_size..offset + ev_size + name_len];
                        let nul = name_bytes.iter().position(|&b| b == 0).unwrap_or(name_len);
                        if let Ok(name) = std::str::from_utf8(&name_bytes[..nul]) {
                            if name == config_filename {
                                G_RELOAD.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    offset += ev_size + name_len;
                }
            }
        }

        if pollfds[0].revents & libc::POLLIN != 0 {
            guard.read().ok();
        } else {
            drop(guard);
        }

        eq.dispatch_pending(&mut state).ok();
    }

    hide(&mut state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::{ClientInfo, WorkspaceInfo};

    fn client(address: u64, class: &str, title: &str) -> ClientInfo {
        ClientInfo {
            address,
            class_name: class.into(),
            title: title.into(),
            initial_title: String::new(),
            workspace_id: 1,
            x: 0, y: 0, w: 100, h: 100,
        }
    }

    fn workspace(name: &str, clients: Vec<ClientInfo>) -> WorkspaceInfo {
        WorkspaceInfo { id: 1, name: name.into(), monitor_id: 0, clients }
    }

    // ── workspace_matches ────────────────────────────────────────────────────

    #[test]
    fn workspace_matches_by_name() {
        let ws = workspace("web", vec![]);
        assert!(workspace_matches(&ws, "web"));
        assert!(workspace_matches(&ws, "we"));
        assert!(!workspace_matches(&ws, "code"));
    }

    #[test]
    fn workspace_matches_by_client_class() {
        let ws = workspace("1", vec![client(1, "firefox", "New Tab")]);
        assert!(workspace_matches(&ws, "fire"));
        assert!(!workspace_matches(&ws, "zed"));
    }

    #[test]
    fn workspace_matches_by_client_title() {
        let ws = workspace("1", vec![client(1, "foot", "nvim — main.rs")]);
        assert!(workspace_matches(&ws, "nvim"));
        assert!(!workspace_matches(&ws, "emacs"));
    }

    #[test]
    fn workspace_matches_empty_filter_always_true() {
        let ws = workspace("1", vec![]);
        assert!(workspace_matches(&ws, ""));
    }

    // ── find_single_window_match ─────────────────────────────────────────────

    #[test]
    fn single_match_returns_address() {
        let workspaces = vec![workspace("1", vec![
            client(10, "firefox", "GitHub"),
            client(20, "foot", "nvim"),
        ])];
        assert_eq!(find_single_window_match(&workspaces, "fire"), Some(10));
    }

    #[test]
    fn multiple_matches_returns_none() {
        let workspaces = vec![workspace("1", vec![
            client(10, "firefox", "GitHub"),
            client(20, "foot", "Firefox bookmarks"),
        ])];
        assert_eq!(find_single_window_match(&workspaces, "firefox"), None);
    }

    #[test]
    fn no_matches_returns_none() {
        let workspaces = vec![workspace("1", vec![client(10, "foot", "nvim")])];
        assert_eq!(find_single_window_match(&workspaces, "zed"), None);
    }

    #[test]
    fn single_match_across_workspaces() {
        let workspaces = vec![
            workspace("1", vec![client(10, "foot", "nvim")]),
            workspace("2", vec![client(20, "firefox", "GitHub")]),
        ];
        assert_eq!(find_single_window_match(&workspaces, "github"), Some(20));
    }

    #[test]
    fn match_is_case_insensitive() {
        let workspaces = vec![workspace("1", vec![client(10, "Alacritty", "bash")])];
        assert_eq!(find_single_window_match(&workspaces, "ALACRITTY"), Some(10));
    }
}
