# Tab-Cycle Windows Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `Tab`/`Shift+Tab` as a global, alt-tab-style way to cycle through every window across all workspaces in the hyprexpose overlay, independent of the existing workspace-grid arrow-key navigation, with Enter focusing the highlighted window.

**Architecture:** Additive `tab_target: Option<u64>` state on `AppState`, populated by a new pure `tab_candidates()` helper (grid order, optionally filter-narrowed) and consumed by `handle_key`'s existing branch structure and by `render.rs`'s per-thumbnail border drawing. No existing state (`selected`, `filter_match`, `hovered_window`) is restructured.

**Tech Stack:** Rust, `wayland-client` (raw evdev keycodes → hand-rolled keysym mapping, no `xkbcommon`), `cairo`/`pango` for rendering, `toml`/`serde` for config.

## Global Constraints

- Ordering is deterministic grid order (workspace order, then `ws.clients` order) — no MRU/`focusHistoryID`, per spec (out of scope).
- Zero behavior change when Tab is never pressed — every new branch must fall through to unchanged existing logic when `tab_target` is `None`.
- Follow existing codebase conventions: hand-rolled evdev-keycode key mapping (no `xkbcommon` dependency), `#[cfg(test)]` unit-test modules co-located with the pure functions they cover (see `find_single_window_match`/`workspace_matches` tests at the bottom of `src/main.rs`), config fields documented in `config.example.toml`.
- Spec: `docs/superpowers/specs/2026-07-05-tab-cycle-windows-design.md`.

## Implementation Note: Shift Tracking (deviation from spec wording)

The spec describes `keys.tab_next` / `keys.tab_prev` as two configurable key-string lists. Investigation during planning found the codebase has **no keyboard-modifier tracking at all** — `wl_keyboard::Event::Modifiers` is never subscribed to, and `key_name_to_keysym` has no syntax for `"Shift+Tab"`. Adding full XKB modifier support is out of scope for this feature.

Instead: a single `keys.tab: Vec<String>` config entry (default `["Tab"]`) plus a new `shift_held: bool` field on `AppState`, updated directly from raw evdev Left/Right-Shift keycodes (42/54) in the `wl_keyboard` `Dispatch` impl — the same "hand-rolled evdev keycode" pattern the file already uses everywhere else. Direction is `shift_held ? prev : next`. This preserves the spec's user-facing behavior (`Tab` = next, `Shift+Tab` = previous, both places the key can be pressed) without introducing a new modifier subsystem.

---

### Task 1: Tab keysym mapping + Shift-key tracking

**Files:**
- Modify: `src/main.rs:70-161` (key mapping constants, `key_name_to_keysym`, `keycode_to_keysym`)
- Modify: `src/main.rs:666-687` (`Dispatch<wl_keyboard::WlKeyboard, ()>` impl)
- Modify: `src/main.rs:197-248` (`AppState` struct + `AppState::new`)
- Test: `src/main.rs` (existing `#[cfg(test)] mod tests` block near the bottom, alongside `workspace_matches`/`find_single_window_match` tests)

**Interfaces:**
- Produces: `XKB_KEY_TAB: u32` constant; `keycode_to_keysym(15) == XKB_KEY_TAB`; `key_name_to_keysym("Tab") == Some(XKB_KEY_TAB)`; `AppState.shift_held: bool` field, `true` while either Shift key is physically held.

- [ ] **Step 1: Write the failing tests for the two pure key-mapping functions**

Add to the existing test module at the bottom of `src/main.rs` (find it via the existing `workspace_matches_by_name` test — add these in the same `mod tests` block):

```rust
    #[test]
    fn keycode_15_maps_to_tab() {
        assert_eq!(keycode_to_keysym(15), XKB_KEY_TAB);
    }

    #[test]
    fn key_name_tab_resolves() {
        assert_eq!(key_name_to_keysym("Tab"), Some(XKB_KEY_TAB));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test keycode_15_maps_to_tab key_name_tab_resolves`
Expected: compile error — `XKB_KEY_TAB` not found in this scope.

- [ ] **Step 3: Add the `XKB_KEY_TAB` constant and wire it into both mapping functions**

In `src/main.rs`, next to the other `XKB_KEY_*` constants (around line 77):

```rust
const XKB_KEY_DELETE:    u32 = 0xffff;
const XKB_KEY_TAB:       u32 = 0xff09;
const XKB_KEY_1: u32 = 0x0031;
const XKB_KEY_9: u32 = 0x0039;
```

In `key_name_to_keysym` (around line 81-97), add a case:

```rust
fn key_name_to_keysym(name: &str) -> Option<u32> {
    match name {
        "Escape"         => Some(XKB_KEY_ESCAPE),
        "Return" | "Enter" => Some(XKB_KEY_RETURN),
        "Left"           => Some(XKB_KEY_LEFT),
        "Up"             => Some(XKB_KEY_UP),
        "Right"          => Some(XKB_KEY_RIGHT),
        "Down"           => Some(XKB_KEY_DOWN),
        "Tab"            => Some(XKB_KEY_TAB),
        "space" | "Space" => Some(0x0020),
        "Delete"          => Some(XKB_KEY_DELETE),
        _ if name.len() == 1 => {
            let c = name.chars().next()?;
            if c.is_ascii_graphic() { Some(c as u32) } else { None }
        }
        _ => None,
    }
}
```

In `keycode_to_keysym` (around line 129-161), add evdev keycode 15 (`KEY_TAB`):

```rust
fn keycode_to_keysym(keycode: u32) -> u32 {
    match keycode {
        1   => XKB_KEY_ESCAPE,
        14  => XKB_KEY_BACKSPACE,
        15  => XKB_KEY_TAB,
        28  => XKB_KEY_RETURN,
        // ...unchanged rest of the match...
```

(Leave every other arm of the match exactly as-is — only insert the new `15 => XKB_KEY_TAB,` line.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test keycode_15_maps_to_tab key_name_tab_resolves`
Expected: both PASS.

- [ ] **Step 5: Add `shift_held` state and evdev Shift-keycode tracking**

Add evdev keycode constants near the top of the key-mapping section (around line 70, alongside the existing `const BTN_LEFT/BTN_RIGHT` style constants — those live at line 22-23, this can go right after the `XKB_KEY_*` block):

```rust
const KEYCODE_LEFTSHIFT:  u32 = 42;
const KEYCODE_RIGHTSHIFT: u32 = 54;
```

Add a field to `AppState` (`src/main.rs:197-248`), next to `filter`/`filter_match` (around line 227-229):

```rust
    // Type-to-filter
    filter: String,
    filter_match: Option<u64>,

    // Tab-cycle
    shift_held: bool,
    tab_target: Option<u64>,
```

Initialize both in `AppState::new` (around line 299-300, right after `filter_match: None,`):

```rust
            filter: String::new(),
            filter_match: None,
            shift_held: false,
            tab_target: None,
```

Update the `Dispatch<wl_keyboard::WlKeyboard, ()>` impl (`src/main.rs:666-687`) to track Shift press/release before the existing Pressed-only dispatch to `handle_key`:

```rust
impl Dispatch<wl_keyboard::WlKeyboard, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Key { key, state: WEnum::Value(key_state), .. } = event {
            if key == KEYCODE_LEFTSHIFT || key == KEYCODE_RIGHTSHIFT {
                state.shift_held = key_state == wl_keyboard::KeyState::Pressed;
                return;
            }
            if key_state == wl_keyboard::KeyState::Pressed {
                let keysym = keycode_to_keysym(key);
                if keysym != 0 && state.handle_key(keysym) {
                    state.should_close = true;
                }
            }
        }
    }
}
```

- [ ] **Step 6: Build to confirm everything compiles**

Run: `cargo build`
Expected: builds with no errors (new fields are unused warnings at most, resolved by later tasks).

- [ ] **Step 7: Commit**

```bash
git add src/main.rs
git commit -m "Add Tab keysym mapping and Shift-key tracking"
```

---

### Task 2: `tab` keybinding config field

**Files:**
- Modify: `src/config.rs:206-249` (`KeybindingsConfig`, defaults)
- Modify: `src/config.rs:298-338` (`warn_unknown_keys` known-keys list)
- Modify: `src/main.rs:103-127` (`ResolvedKeys`)
- Modify: `config.example.toml` (keybindings section + comment)
- Test: `src/config.rs` (new `#[cfg(test)] mod tests` block — none exists yet in this file)

**Interfaces:**
- Consumes: nothing new.
- Produces: `KeybindingsConfig.tab: Vec<String>` (default `["Tab"]`); `ResolvedKeys.tab: Vec<u32>`.

- [ ] **Step 1: Write the failing test for the config default**

Add at the bottom of `src/config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_keybinding_defaults_to_tab_key() {
        let cfg = KeybindingsConfig::default();
        assert_eq!(cfg.tab, vec!["Tab".to_string()]);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test tab_keybinding_defaults_to_tab_key`
Expected: compile error — no field `tab` on `KeybindingsConfig`.

- [ ] **Step 3: Add the `tab` field to `KeybindingsConfig`**

In `src/config.rs`, add a default function next to `default_close_window` (around line 213):

```rust
fn default_close_window()  -> Vec<String> { vec!["Delete".into()] }
fn default_tab()           -> Vec<String> { vec!["Tab".into()] }
```

Add the field to the struct (around line 215-234):

```rust
#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct KeybindingsConfig {
    #[serde(default = "default_close",        deserialize_with = "deserialize_key_list")]
    pub close: Vec<String>,
    #[serde(default = "default_confirm",      deserialize_with = "deserialize_key_list")]
    pub confirm: Vec<String>,
    #[serde(default = "default_left",         deserialize_with = "deserialize_key_list")]
    pub left: Vec<String>,
    #[serde(default = "default_right",        deserialize_with = "deserialize_key_list")]
    pub right: Vec<String>,
    #[serde(default = "default_up",           deserialize_with = "deserialize_key_list")]
    pub up: Vec<String>,
    #[serde(default = "default_down",         deserialize_with = "deserialize_key_list")]
    pub down: Vec<String>,
    #[serde(default = "default_move_window",  deserialize_with = "deserialize_key_list")]
    pub move_window: Vec<String>,
    #[serde(default = "default_close_window", deserialize_with = "deserialize_key_list")]
    pub close_window: Vec<String>,
    #[serde(default = "default_tab",          deserialize_with = "deserialize_key_list")]
    pub tab: Vec<String>,
}
```

And to its `Default` impl (around line 236-249):

```rust
impl Default for KeybindingsConfig {
    fn default() -> Self {
        Self {
            close:        default_close(),
            confirm:      default_confirm(),
            left:         default_left(),
            right:        default_right(),
            up:           default_up(),
            down:         default_down(),
            move_window:  default_move_window(),
            close_window: default_close_window(),
            tab:          default_tab(),
        }
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test tab_keybinding_defaults_to_tab_key`
Expected: PASS.

- [ ] **Step 5: Add `tab` to the unknown-keys warning list**

In `src/config.rs`, in `warn_unknown_keys`'s `SECTIONS` (around line 314-317):

```rust
            ("keybindings", &[
                "close", "confirm", "left", "right", "up", "down",
                "move_window", "close_window", "tab",
            ]),
```

- [ ] **Step 6: Resolve the new key list in `ResolvedKeys` (`src/main.rs`)**

In `ResolvedKeys` struct (`src/main.rs:103-112`):

```rust
struct ResolvedKeys {
    close:        Vec<u32>,
    confirm:      Vec<u32>,
    left:         Vec<u32>,
    right:        Vec<u32>,
    up:           Vec<u32>,
    down:         Vec<u32>,
    move_window:  Vec<u32>,
    close_window: Vec<u32>,
    tab:          Vec<u32>,
}
```

In `ResolvedKeys::from_config` (`src/main.rs:114-127`):

```rust
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
            tab:          resolve_keys(&kb.tab),
        }
    }
}
```

- [ ] **Step 7: Document the new key in `config.example.toml`**

In `config.example.toml`, update the keybindings section comment and add the entry:

```toml
# ── Keybindings ───────────────────────────────────────────────────────────────
# Each action accepts a single key name or an array of key names.
# Supported names: "Escape", "Return", "Left", "Right", "Up", "Down", "Tab", "space",
# or any single printable character ("h", "j", "k", "l", "m", etc.).
# Number keys 1–9 always jump directly to that workspace and cannot be remapped.

[keybindings]
close       = "Escape"
confirm     = "Return"
left        = ["Left", "h"]
right       = ["Right", "l"]
up          = ["Up", "k"]
down        = ["Down", "j"]
move_window   = "m"
close_window  = "Delete"   # close the hovered window (or the single filter match)
tab           = "Tab"      # cycle-highlight windows across all workspaces; hold Shift to reverse
```

- [ ] **Step 8: Build to confirm everything compiles**

Run: `cargo build`
Expected: builds with no errors.

- [ ] **Step 9: Commit**

```bash
git add src/config.rs src/main.rs config.example.toml
git commit -m "Add configurable tab keybinding"
```

---

### Task 3: `tab_candidates` pure function

**Files:**
- Modify: `src/main.rs` (add function near `find_single_window_match`/`workspace_matches`, around line 250-270)
- Test: `src/main.rs` (existing `#[cfg(test)] mod tests` block)

**Interfaces:**
- Consumes: `WorkspaceInfo` (from `crate::ipc`, already imported), same shape used by `find_single_window_match`.
- Produces: `fn tab_candidates(workspaces: &[WorkspaceInfo], filter: &str) -> Vec<u64>` — every window address in grid order (workspace order, then `ws.clients` order), narrowed to filter-matches when `filter` is non-empty (case-insensitive substring on `class_name`/`title`, same predicate as `workspace_matches`).

- [ ] **Step 1: Write the failing tests**

Add to the existing test module at the bottom of `src/main.rs`, using the existing `workspace()`/`client()` test builders (already used by the `find_single_window_match` tests above them):

```rust
    #[test]
    fn tab_candidates_empty_filter_returns_all_in_grid_order() {
        let workspaces = vec![
            workspace("1", vec![client(10, "foot", "term"), client(11, "firefox", "web")]),
            workspace("2", vec![client(20, "zed", "editor")]),
        ];
        assert_eq!(tab_candidates(&workspaces, ""), vec![10, 11, 20]);
    }

    #[test]
    fn tab_candidates_filter_narrows_to_matches() {
        let workspaces = vec![
            workspace("1", vec![client(10, "foot", "term"), client(11, "firefox", "web")]),
            workspace("2", vec![client(20, "firefox", "docs")]),
        ];
        assert_eq!(tab_candidates(&workspaces, "firefox"), vec![11, 20]);
    }

    #[test]
    fn tab_candidates_no_matches_returns_empty() {
        let workspaces = vec![workspace("1", vec![client(10, "foot", "term")])];
        assert_eq!(tab_candidates(&workspaces, "zed"), Vec::<u64>::new());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test tab_candidates`
Expected: compile error — `tab_candidates` not found.

- [ ] **Step 3: Implement `tab_candidates`**

Add next to `workspace_matches` (around line 264-270 of `src/main.rs`):

```rust
fn tab_candidates(workspaces: &[WorkspaceInfo], filter: &str) -> Vec<u64> {
    let f = filter.to_lowercase();
    workspaces
        .iter()
        .flat_map(|ws| ws.clients.iter())
        .filter(|c| {
            f.is_empty()
                || c.class_name.to_lowercase().contains(&f)
                || c.title.to_lowercase().contains(&f)
        })
        .map(|c| c.address)
        .collect()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test tab_candidates`
Expected: all three PASS.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "Add tab_candidates helper for grid-ordered window cycling"
```

---

### Task 4: Wire Tab-cycling into `handle_key`

**Files:**
- Modify: `src/main.rs:362-459` (`AppState::handle_key`)
- Modify: `src/main.rs:689-` (pointer `Enter`/`Motion` handlers — mouse clears `tab_target`)
- Modify: `src/main.rs:873-899` (`hide()` — reset `tab_target` on overlay close)

**Interfaces:**
- Consumes: `tab_candidates` (Task 3), `self.keys.tab: Vec<u32>` (Task 2), `self.shift_held: bool` and `self.tab_target: Option<u64>` (Task 1), `XKB_KEY_TAB` (Task 1).
- Produces: `self.tab_target` transitions correctly for every input path; later tasks (rendering) read `self.tab_target` directly.

- [ ] **Step 1: Add the Escape branch — clear `tab_target` first, before the existing filter-clear check**

In `handle_key` (`src/main.rs:362-459`), the current Escape branch is:

```rust
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
```

Change it to:

```rust
        // Escape: clear Tab-highlight first, then filter, else close
        if self.keys.close.contains(&k) {
            if self.tab_target.is_some() {
                self.tab_target = None;
                self.needs_redraw = true;
                return false;
            }
            if !self.filter.is_empty() {
                self.filter.clear();
                self.filter_match = None;
                self.needs_redraw = true;
                return false;
            }
            return true;
        }
```

- [ ] **Step 2: Add the Confirm/Enter branch — focus `tab_target` first, before the existing `filter_match` check**

The current Confirm branch is:

```rust
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
```

Change it to:

```rust
        // Confirm: Tab-highlighted window first, else single filter match, else activate workspace
        if self.keys.confirm.contains(&k) {
            let target_addr = self.tab_target.or(self.filter_match);
            if let Some(addr) = target_addr {
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
```

(`self.tab_target.or(self.filter_match)` preserves existing behavior when `tab_target` is `None`, and takes priority when it's `Some`, matching the spec's "tab_target first" ordering.)

- [ ] **Step 3: Add the Tab/Shift+Tab branch, and clear `tab_target` on arrow-key input**

The current arrow-key branch is:

```rust
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
```

Add the Tab branch immediately before it, and clear `tab_target` inside the existing arrow branch:

```rust
        // Tab / Shift+Tab: cycle the highlighted window across all workspaces
        // (or across filter-matching windows, if a filter is active)
        if self.keys.tab.contains(&k) {
            let candidates = tab_candidates(&self.workspaces, &self.filter);
            if !candidates.is_empty() {
                let reverse = self.shift_held;
                let cur = self.tab_target.and_then(|addr| candidates.iter().position(|&a| a == addr));
                let next_index = match (cur, reverse) {
                    (Some(i), false) => (i + 1) % candidates.len(),
                    (Some(i), true)  => if i == 0 { candidates.len() - 1 } else { i - 1 },
                    (None, false)    => 0,
                    (None, true)     => candidates.len() - 1,
                };
                self.tab_target = Some(candidates[next_index]);
                self.needs_redraw = true;
            }
            return false;
        }

        // Arrow keys always navigate; hjkl only when filter is empty
        let is_arrow = matches!(k, XKB_KEY_LEFT | XKB_KEY_RIGHT | XKB_KEY_UP | XKB_KEY_DOWN);
        if is_arrow || self.filter.is_empty() {
            if self.keys.right.contains(&k) && self.selected + 1 < n {
                self.tab_target = None;
                self.selected += 1; self.needs_redraw = true; return false;
            }
            if self.keys.left.contains(&k) && self.selected > 0 {
                self.tab_target = None;
                self.selected -= 1; self.needs_redraw = true; return false;
            }
            if self.keys.down.contains(&k) && self.selected + cols < n {
                self.tab_target = None;
                self.selected += cols; self.needs_redraw = true; return false;
            }
            if self.keys.up.contains(&k) && self.selected >= cols {
                self.tab_target = None;
                self.selected -= cols; self.needs_redraw = true; return false;
            }
        }
```

- [ ] **Step 4: Clear `tab_target` when the highlighted window is closed**

The current close-window branch (`src/main.rs:439-451`) is:

```rust
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
```

Add a `tab_target` clear alongside the existing `hovered_window` clear:

```rust
        // Close window: target hovered window first, then single filter match
        if self.keys.close_window.contains(&k) {
            let addr = self.hovered_window.or(self.filter_match);
            if let Some(addr) = addr {
                ipc::close_window(addr);
                for ws in &mut self.workspaces {
                    ws.clients.retain(|c| c.address != addr);
                }
                if self.hovered_window == Some(addr) { self.hovered_window = None; }
                if self.tab_target == Some(addr) { self.tab_target = None; }
                self.update_filter_state();
                self.needs_redraw = true;
            }
            return false;
        }
```

- [ ] **Step 5: Clear `tab_target` on mouse motion**

In the `Dispatch<wl_pointer::WlPointer, ()>` impl (`src/main.rs:689-` onward), both `Enter` and `Motion` arms call `state.select_workspace_at(...)`. Add a `tab_target` clear at the top of each arm, right after the existing `state.pointer_x`/`state.pointer_y` assignments:

```rust
            wl_pointer::Event::Enter { serial, surface_x, surface_y, .. } => {
                state.pointer_x = surface_x;
                state.pointer_y = surface_y;
                state.cursor_enter_serial = serial;
                state.cursor_is_pointer = false;
                if state.tab_target.is_some() { state.tab_target = None; state.needs_redraw = true; }
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
                if state.tab_target.is_some() { state.tab_target = None; state.needs_redraw = true; }
                state.select_workspace_at(surface_x, surface_y);
                let win = state.window_at(surface_x, surface_y);
                if win.is_some() != state.cursor_is_pointer {
                    state.apply_cursor(pointer, win.is_some());
                    state.cursor_is_pointer = win.is_some();
                }
                // ...unchanged rest of this arm...
```

(Only add the two new `if state.tab_target.is_some() { ... }` lines shown above; leave the rest of both arms exactly as they are today.)

- [ ] **Step 6: Reset `tab_target` when the overlay is hidden**

In `hide()` (`src/main.rs:873-899`), add the reset alongside the existing `filter`/`workspaces`/`thumbnails` resets:

```rust
    state.configured = false;
    state.visible = false;
    state.filter.clear();
    state.tab_target = None;
    state.workspaces.clear();
    state.thumbnails.clear();
```

- [ ] **Step 7: Build to confirm everything compiles**

Run: `cargo build`
Expected: builds with no errors.

- [ ] **Step 8: Run the full unit test suite to confirm no regressions**

Run: `cargo test`
Expected: all tests PASS (existing `workspace_matches`/`find_single_window_match` tests plus the new ones from Tasks 1-3).

- [ ] **Step 9: Commit**

```bash
git add src/main.rs
git commit -m "Wire Tab-cycle highlight into key and pointer handling"
```

---

### Task 5: Render the Tab-highlight border

**Files:**
- Modify: `src/render.rs:37-169` (`draw_client`)
- Modify: `src/render.rs:171-184` (`DrawParams`)
- Modify: `src/render.rs:315-317` (`draw_client` call site inside `draw`)
- Modify: `src/main.rs` (`redraw()`, the `render::draw(render::DrawParams { ... })` call — find via `render::DrawParams {`)

**Interfaces:**
- Consumes: `AppState.tab_target: Option<u64>` (Task 4).
- Produces: a visible border distinct from the selection/hover/filter-match borders, drawn around the thumbnail whose address matches `tab_target`.

- [ ] **Step 1: Add a `tab_target` parameter to `draw_client` and draw its border**

Change the `draw_client` signature (`src/render.rs:37-53`) to add one parameter, and add a new border block after the existing "Filter single-match highlight" block (after line 117, before "Hover highlight"):

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
    tab_target: Option<u64>,
    filter: &str,
    window_font: &pango::FontDescription,
) {
```

After the existing filter-match block:

```rust
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
```

- [ ] **Step 2: Thread `tab_target` through `DrawParams` and the call site**

Add the field to `DrawParams` (`src/render.rs:171-184`):

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
    pub tab_target: Option<u64>,
    pub filter: &'a str,
}
```

Update the destructure and the `draw_client` call inside `draw()` (`src/render.rs:186-187` and `315-317`):

```rust
pub fn draw(p: DrawParams<'_>) -> Vec<u8> {
    let DrawParams { width, height, workspaces, selected_index, thumbnails, cfg, active_window_address, hovered_window, pointer_x, pointer_y, filter_match, tab_target, filter } = p;
```

```rust
        for client in &ws.clients {
            draw_client(&cr, cfg, client, min_x, min_y, scale, off_x, off_y, thumbnails, active_window_address, hovered_window, filter_match, tab_target, filter, &window_font);
        }
```

- [ ] **Step 3: Pass `state.tab_target` from `redraw()` in `src/main.rs`**

Find the `render::draw(render::DrawParams { ... })` call inside `redraw()` (`src/main.rs`, starts around line 917) and add `tab_target: state.tab_target,` alongside the existing `filter_match: state.filter_match,` line.

- [ ] **Step 4: Build to confirm everything compiles**

Run: `cargo build`
Expected: builds with no errors.

- [ ] **Step 5: Commit**

```bash
git add src/render.rs src/main.rs
git commit -m "Render Tab-cycle highlight border on the targeted window"
```

---

### Task 6: End-to-end manual verification

**Files:** none (verification only)

- [ ] **Step 1: Build in release mode**

Run: `cargo build --release`
Expected: builds with no errors.

- [ ] **Step 2: Run the full test suite one more time**

Run: `cargo test`
Expected: all tests PASS.

- [ ] **Step 3: Launch the overlay in a live Hyprland session and manually verify each spec scenario**

Use this project's `run`/`verify` skill (or the app's existing show/toggle mechanism — `SIGUSR1` per `src/main.rs`'s signal handler) to open the overlay with at least 2 workspaces, each with 2+ windows, and confirm:

- Tab cycles the highlight forward through every window in grid order, wrapping from the last window back to the first.
- Shift+Tab cycles backward, wrapping from the first window to the last.
- Tab-cycling crosses workspace boundaries (moving from the last window of workspace 1 to the first window of workspace 2).
- Pressing Enter while a window is Tab-highlighted focuses that specific window and closes the overlay.
- Pressing an arrow key clears the Tab-highlight (border disappears) and resumes normal workspace-grid navigation.
- Moving the mouse clears the Tab-highlight.
- Pressing Escape once clears an active Tab-highlight and keeps the overlay open; pressing Escape again (with filter text present) clears the filter; a third Escape closes the overlay.
- Typing a filter, then pressing Tab, cycles only through windows matching that filter.
- Closing the Tab-highlighted window (via the close-window key) clears the highlight without a crash.
- Reopening the overlay after closing it starts with no Tab-highlight active.

- [ ] **Step 4: Report results**

If any scenario fails, fix the relevant task's code and re-run this task's checklist from Step 1. Once all scenarios pass, this feature is complete — no commit needed for this task (verification only).
