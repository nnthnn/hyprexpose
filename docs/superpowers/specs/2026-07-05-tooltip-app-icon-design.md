# Tooltip app icon — design

## Goal

Show the same app icon already drawn on window thumbnails in the hover
tooltip too, next to the app name.

## Where

`draw()`'s hover-tooltip block (`src/render.rs`, the block starting
`// Hover tooltip`). No new module, no new config field — this reuses
`cfg.appearance.show_app_icons`, `cfg.appearance.icon_size` (clamped via the
existing `clamped_icon_size` helper), and the `IconCache` already threaded
into `draw()`.

## Behavior

- Gated on `cfg.appearance.show_app_icons` being true.
- Look up the hovered window's icon via `icon_cache.get(&c.class_name,
  clamped_icon_size(cfg.appearance.icon_size) as u32)`.
- If `None` (no icon resolves for that class), the tooltip renders exactly
  as it does today — no reserved gutter, no layout change.
- If `Some(icon)`:
  - A fixed `gap` of 8px separates the icon from the app-name text.
  - The name row's height becomes `max(nh, icon_size)` (`nh` = the name
    text's pixel height), so a larger icon isn't clipped against a
    smaller font, and a larger font isn't clipped against a smaller icon.
  - The icon is vertically centered within that row, positioned at the
    tooltip's left content padding (`bx + pad`); the name text's x
    position shifts right by `icon_size + gap`.
  - The class and title lines below the name row are **not** indented —
    they stay at the tooltip's normal left padding, unaffected by the
    icon.
  - The tooltip box's content width becomes
    `max(icon_size + gap + name_width, class_width, title_width)`
    (previously just `max(name_width, class_width, title_width)`).
  - The vertical advance after the name row uses the row height
    (`max(nh, icon_size)`) instead of `nh` when computing where the next
    line starts.

## Out of scope

- No new config fields — icon size and the show/hide toggle are shared
  with the thumbnail-corner icons.
- No caching changes — this reuses `IconCache::get`, which already caches
  per class name for the process lifetime.
