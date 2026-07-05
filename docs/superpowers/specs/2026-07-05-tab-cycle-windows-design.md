# Tab-cycle windows (design)

## Goal

Add `Tab`/`Shift+Tab` as a global, alt-tab-style way to cycle through windows across all workspaces in the overlay, independent of the existing workspace-grid arrow-key navigation. Pressing Enter while a window is Tab-highlighted focuses that specific window.

## Non-goals

- No most-recently-used (MRU) ordering. Hyprland's `focusHistoryID` is not currently parsed anywhere in `src/ipc.rs`/`src/ipc/hyprland.rs`, and adding it is out of scope. Ordering is deterministic grid order.
- No change to the existing workspace-selection model (`AppState.selected`, arrow-key navigation, number-key jump, mouse hover/click). Tab-cycling is an additive, parallel selection concept, not a replacement.
- No unification with `filter_match`/`find_single_window_match`. Those remain untouched; Tab-cycling reads the same filter predicate but stores its own state.

## State

Add one field to `AppState` (`src/main.rs`):

```rust
tab_target: Option<u64>, // address of the currently Tab-highlighted window; None = inactive
```

Initialized to `None` in `AppState::new` and reset to `None` whenever the overlay is shown again (same lifecycle as other transient selection state).

## Candidate list

A new free function alongside `find_single_window_match`/`workspace_matches` (`src/main.rs`), e.g. `fn tab_candidates(workspaces: &[WorkspaceInfo], filter: &str) -> Vec<u64>`, computed fresh on every Tab/Shift+Tab press (not cached):

- If `filter.is_empty()`: flatten all windows in grid order — iterate `workspaces` in order, and within each workspace, `ws.clients` in order.
- If the filter is non-empty: same flatten, filtered to windows matching the filter predicate already used by `workspace_matches`/`find_single_window_match` (`class_name`/`title` case-insensitive substring match).
- If the resulting list is empty, Tab/Shift+Tab is a no-op.

Being a pure function of `(workspaces, filter)`, this follows the existing pattern of `find_single_window_match`/`workspace_matches` and is unit-testable the same way (see Testing).

## Key handling (`AppState::handle_key`, `src/main.rs`)

New keysym constant for `Tab` (`XKB_KEY_TAB`), and a `keys.tab_next` / `keys.tab_prev` pair added to `ResolvedKeys` (`src/config.rs`) following the existing pattern used for `left`/`right`/`up`/`down`/`confirm`/etc. Defaults: `Tab` for next, `Shift+Tab` for previous. Both configurable via `KeybindingsConfig`.

Branches, in priority order:

1. **Escape** (`keys.close`): if `tab_target.is_some()`, clear it (`tab_target = None`), mark `needs_redraw`, `return false` (stay open). This check runs *before* the existing filter-clear check, so the Escape chain becomes: 1st press clears Tab-highlight → 2nd press clears filter (if any) → 3rd press closes overlay. This extends the existing filter-clear-then-close chain by one step; no existing branch changes.
2. **Confirm/Enter** (`keys.confirm`): if `tab_target` is `Some(addr)`, resolve the client the same way the existing `filter_match` branch does (search `self.workspaces` for matching `address`), set `pending_focus = Some((c.workspace_id, addr, cx, cy))`, `return true`. This check runs *before* the existing `filter_match` check, which is otherwise unchanged and still runs when `tab_target` is `None`.
3. **Tab / Shift+Tab** (`keys.tab_next` / `keys.tab_prev`): build the candidate list (see above). If empty, no-op. Otherwise find `tab_target`'s position in the list via `position()`. If found, advance by +1 (Tab) or -1 (Shift+Tab) with wraparound (`(i + 1) % len`, or `i.checked_sub(1).unwrap_or(len - 1)`). If `tab_target` is `None` or not found in the (possibly filter-narrowed) list, start at index 0 for Tab or `len - 1` for Shift+Tab. Set `tab_target` to the resulting address, mark `needs_redraw`.
4. **Arrow keys** (`left`/`right`/`up`/`down`): before their existing logic runs, set `tab_target = None`.
5. **Mouse motion** (pointer motion handler, `src/main.rs`): before existing `hovered_window` logic runs, set `tab_target = None`.
6. **Close window** (`keys.close_window`): after removing the closed window from `self.workspaces`, if `tab_target == Some(addr)` (the address just closed), set `tab_target = None` — mirrors the existing `hovered_window` clear in this branch.

## Filter interaction edge case

If the filter text changes (typed or backspaced) while `tab_target` is set, the candidate list is recomputed from scratch on the *next* Tab press. If `tab_target`'s address is no longer in the new list, treat it as not-found: the next Tab/Shift+Tab starts fresh at index 0 / last, rather than erroring or panicking.

## Rendering (`src/render.rs`)

Draw a new, visually distinct border around the thumbnail whose `client.address == tab_target` (when `Some`), alongside the existing per-cell workspace-selection border and the existing hover/filter-match border. All three can in principle be visible at once (e.g., Tab-highlight on one window, mouse hovering a different one) since they are independent pieces of state — this is expected and matches "any other input clears Tab-highlight" only for arrow keys/mouse/Escape, not for the borders' render logic itself.

## Testing

- `src/main.rs` already has a `#[cfg(test)]` unit-test module covering pure helpers (`workspace_matches`, `find_single_window_match`) using small `workspace()`/`client()` test builders. Add equivalent unit tests for `tab_candidates` following the same style: empty filter returns all windows in grid order across multiple workspaces, non-empty filter narrows correctly, no-match filter returns an empty list.
- `handle_key` itself (and the rest of `AppState`) is not unit-tested today — it depends on live Wayland/IPC state — so Tab/Shift+Tab/Enter/Escape wiring is verified manually via the `verify`/`run` skill: launch the overlay, confirm Tab/Shift+Tab cycles the highlight across workspace boundaries in grid order, confirm Enter focuses the highlighted window, confirm arrow keys/mouse/Escape clear the highlight as specified, confirm filtered Tab-cycling narrows to matches, confirm closing the Tab-highlighted window clears the highlight without panicking.
