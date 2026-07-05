# Window app icons — design

## Goal

Draw each window's app icon in the bottom-right corner of its thumbnail (or
colored-rect fallback) in the workspace overview, so windows are identifiable
at a glance without hovering for the tooltip.

## Icon lookup

New module `src/icons.rs`.

- Detect the user's icon theme: read `gtk-icon-theme-name` from
  `$XDG_CONFIG_HOME/gtk-3.0/settings.ini` (falling back to
  `~/.config/gtk-3.0/settings.ini`); fall back to `"Adwaita"`, then
  `"hicolor"` if not found or unreadable.
- Resolve an icon path via the `freedesktop-icons` crate, trying in order:
  1. the window's raw `class_name`
  2. `clean_app_name(class_name)` lowercased
- Decode the resolved file:
  - PNG/XPM: Cairo's native PNG loader (`ImageSurface::create_from_png`).
  - SVG: rasterize with `resvg`/`usvg`/`tiny-skia` (pure Rust, no
    GTK/librsvg dependency) into an RGBA buffer at `icon_size` (scaled for
    HiDPI-equivalent sharpness), then wrap as a Cairo `ImageSurface`.
- No match at any size/format → no icon drawn for that window (existing
  thumbnail/placeholder rendering is unaffected).

## Caching

`IconCache` = `HashMap<String, Option<Rc<ImageSurface>>>`, keyed by
`class_name`, owned by `AppState`, alive for the process lifetime.

- Lookups happen lazily the first time a class name is seen and are reused
  on every subsequent redraw (icons don't change at runtime).
- A miss is cached as `None` so lookup isn't retried every frame for
  icon-less apps.

## Rendering

In `draw_client` (`src/render.rs`), after the existing thumbnail/placeholder
fill and border:

- If `cfg.appearance.show_app_icons` is true and the thumbnail passes the
  same size gate used for the window label (`rw > 40.0 && rh > 20.0`), draw
  the cached icon (if present) at `icon_size × icon_size`, inset by a small
  fixed margin from the bottom-right corner of the thumbnail rect.
- Draw a subtle dark rounded backing behind the icon first, so it stays
  legible over light or busy thumbnail content.
- Applies uniformly to real thumbnails, `--no-preview` colored rects, and
  Sway (which has no thumbnail protocol and always uses colored rects) —
  icons are most useful exactly where there's no real preview.

`IconCache` is threaded through the same way `thumbnails: &[Thumbnail]`
already is, but as `&mut IconCache` since lookups populate it lazily during
drawing.

## Config

New fields on `AppearanceConfig` (`src/config.rs`), following the existing
pattern (e.g. `window_gap`, `window_border`):

- `show_app_icons: bool` — default `true`
- `icon_size: f64` — default `20.0`

Both get inline doc comments and an entry in `config.example.toml`, and
`show_app_icons` participates in the existing "unknown config key" warning
mechanism the same as every other field.

## New dependencies

- `freedesktop-icons` — icon theme lookup per the freedesktop icon spec.
- `resvg`, `usvg`, `tiny-skia` — pure-Rust SVG rasterization, no C/GTK deps,
  consistent with this project's existing GTK-free Cairo/Pango approach.

## Out of scope (future ideas)

- Parsing `.desktop` files for `StartupWMClass` → `Icon=` mapping for exact
  matches when a window's class name doesn't match its icon name
  (e.g. some Electron/JetBrains apps). Direct class-name lookup is expected
  to cover the large majority of apps since most icon themes name icons
  after the app's WM class by convention.
- Proportional/adaptive icon sizing — fixed `icon_size` was chosen for
  predictability and consistency with other fixed appearance knobs.
