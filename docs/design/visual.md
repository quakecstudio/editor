# Visual design

The look of QuakeC Studio: colour, type, spacing, density, icons, motion and window chrome (UI-14,
UI-15, QA-9). The direction and the alternatives not taken are in
[ADR-0010](../adr/0010-visual-design.md); the approved mockups are in [`mockups/`](mockups/) and
listed in the [design README](README.md#mockups).

[`tokens.toml`](tokens.toml) is the single source the UI layer reads its theme from. The tables
below explain it; when they disagree, `tokens.toml` is right and this page is fixed.

## Direction

- **Feel:** Zed's editor (calm, dense, keyboard-first) with Roblox Studio's play loop (UI-14). The
  code and the running game get the space; chrome stays quiet.
- **Colour:** warm graphite surfaces with one ember accent. Ember marks the one thing to act on
  (Play, Accept, the selected entity); blue is only for focus and the keyboard selection. Status
  colours (ok, warn, error) appear only where there is a status.
- **Dark is the default;** light is a full theme with the same tokens, not an afterthought.
- **Native on each OS:** the window chrome follows each platform's conventions (below).

## Colour

Two themes, `dark` and `light`, each defining every token. Values are `#RRGGBB`.

### Surfaces

| Token | Dark | Light | Used for |
| --- | --- | --- | --- |
| `surface.chrome` | `#161514` | `#F1EEEA` | Title bar and status bar |
| `surface.toolbar` | `#1D1B1A` | `#F4F2EE` | The play toolbar |
| `surface.tabs` | `#1E1D1B` | `#F1EEEA` | The tab strip above the editor |
| `surface.editor` | `#1A1918` | `#FCFBF9` | Editor, console, text inputs |
| `surface.panel` | `#211F1E` | `#F4F2EE` | Side and bottom panels |
| `surface.raised` | `#2A2826` | `#ECE8E2` | Popovers, the command palette, chips, hovered rows |
| `surface.viewport` | `#121110` | `#E4DFD8` | Map editor 3D and top views |
| `surface.current_line` | `#222120` | `#F6F4F0` | The caret's line in the editor |

### Highlighted rows

| Token | Dark | Light | Used for |
| --- | --- | --- | --- |
| `select.focus` | `#2E3640` | `#EEF3F8` | The keyboard selection in trees and lists; the editor selection |
| `select.accent` | `#3A2A1E` | `#F8E9DC` | The selected entity, with a 2 px `accent` bar on its left edge |
| `diff.added` | `#24331F` | `#EFF6E9` | Added lines in an inline edit or an agent hunk |
| `debug.paused_line` | `#3A3122` | `#FBF3DF` | The line the debugger is paused on, the pressed Paused button |
| `status.error_bg` | `#3A2220` | `#FAE6E2` | An inline diagnostic box |

### Text and icons

| Token | Dark | Light | Used for |
| --- | --- | --- | --- |
| `text` | `#E8E3DC` | `#22201D` | Body text, values, active tabs |
| `text.code` | `#D9D2C7` | `#2E2B27` | Plain code and agent prose |
| `text.muted` | `#ABA49A` | `#5C564F` | Labels, field names, inactive tabs, secondary text |
| `text.faint` | `#8E877D` | `#6E6860` | Line numbers and counts, only on chrome, toolbar, tabs, editor and panel surfaces and the current line |
| `text.on_accent` | `#1A1918` | `#FFFFFF` | Text on an `accent` fill (Play, Accept) |
| `text.on_select` | `#F2E9DE` | `#3A2414` | Text on `select.accent` |
| `text.on_diff` | `#C7E0B6` | `#2C4A1C` | Code on `diff.added` in a hunk |
| `text.on_error` | `#F2D2CC` | `#6E1F17` | Text in an inline diagnostic |
| `accent` | `#EE8B45` | `#A84D16` | Ember: primary actions, the selected entity, active-tab underline, fuzzy-match highlights |
| `focus` | `#6FA8DC` | `#24609A` | Focus rings, the focused field's outline, links |
| `focus.hover` | `#9CC4E8` | `#1B4C7A` | A hovered link |
| `status.ok` | `#8CC47E` | `#346B27` | Success: a clean compile, a passed scenario, a finished agent step |
| `status.warn` | `#E2B04A` | `#7F5708` | Warnings, modified files, the paused state |
| `status.error` | `#EC7A6C` | `#AA3529` | Errors and unknown keys |

### Code

QuakeC is coloured by syntax category ([`tokens.toml`](tokens.toml) `[syntax]`):

| Category | Token | Dark | Light | Style |
| --- | --- | --- | --- | --- |
| Keyword (`local`, `if`, `return`) | `code.keyword` | `#E0956A` | `#A84F1C` | |
| Type (`entity`, `void`, `float`) | `code.type` | `#86BFD0` | `#21708A` | |
| Function | `code.function` | `#E8C77E` | `#8A6512` | |
| Builtin (`spawn`, `sound`) | `code.function` | `#E8C77E` | `#8A6512` | italic |
| Constant and number (`MOVETYPE_BOUNCE`, `0.7`) | `code.constant` | `#CDA4DE` | `#8547A0` | |
| String | `code.string` | `#A9CB82` | `#4F7A2A` | |
| Vector literal (`'0 0 16'`) | `code.type` | `#86BFD0` | `#21708A` | |
| Comment | `code.comment` | `#938C82` | `#6E6860` | italic |
| Everything else | `text.code` | `#D9D2C7` | `#2E2B27` | |

### Lines

`border` (`#33302D` / `#E0DCD5`) divides panels and outlines cards; `border.strong` (`#423F3B` /
`#CFC9C0`) outlines secondary buttons and key caps; `border.window` (`#3A3734` / `#D6D1C9`) outlines
the docked engine window and the main window where the OS draws none; `border.warn` (`#5A4A2A` /
`#E3CC94`) outlines the pressed Paused button.

### Contrast

Every pair the UI draws is listed under `[[contrast]]` in `tokens.toml` and must reach WCAG 2.2 AA,
4.5:1, in both themes, for text and icons alike (QA-9). `cargo xtask contrast` checks it as a
step of `cargo xtask ci` and fails on any pair below 4.5:1, on a rule that asks for less, on a
colour token that is in no rule, and on a theme missing a token. Its output gives the lowest ratio
of each rule; at the time of writing the lowest pair is 4.53:1 in both themes.

Rules the UI layer follows so that only checked pairs are drawn:

- `text.faint` and comments are never drawn on `surface.raised` or a highlighted row; there they
  are drawn in `text.muted`.
- `status.error` is never drawn on `select.focus`; a selected problem row keeps its icon and
  message in `text`.
- Code colours are drawn on the editor, panel and current-line surfaces, the editor selection, added
  lines and the paused line, and nowhere else.
- Borders are decorative. A field is identified by its label, position and `surface.editor` fill;
  the focused field gets a `focus` outline, which is checked.

## Type

| Role | Font | Size and line | Weight |
| --- | --- | --- | --- |
| Titles | IBM Plex Sans | 20 px | 600 |
| Interface | IBM Plex Sans | 13 px | 400 |
| Panel headings | IBM Plex Sans | 12 px | 600 |
| Captions, status bar, hints | IBM Plex Sans | 12 px | 400 |
| Code | JetBrains Mono | 13 px on a 20 px line | 400 |
| Small code (ids, keys, values in panels) | JetBrains Mono | 12 px on an 18 px line | 400 |

Ligatures are off so QuakeC reads literally (`!=` stays two characters). Both families are under
the SIL Open Font License 1.1 and ship as separate font files; they are recorded in
`ATTRIBUTION.md` when they are added. Fallbacks: `system-ui`, Segoe UI, Cantarell, then
`sans-serif`; SF Mono, Cascadia Mono, DejaVu Sans Mono, then `monospace`.

## Spacing and density

One density, tuned for a laptop screen showing code, game and panels at once.

- **Spacing scale:** 4, 8, 12, 16, 24 px. Panel padding is 12 px; gaps inside a control are 8 px.
- **Heights:** list and tree rows 24 px, code lines 20 px, panel headers 30 px, controls 30 px,
  status bar 26 px, tab strip 36 px, title bar 38 px, toolbar 44 px.
- **Radius:** 4 px key caps, 5 px inputs, 6 px buttons and rows, 8 px cards and popovers, 10 px
  windows.
- **Elevation:** surfaces are flat and separated by `border`. Popovers (the command palette, menus,
  autocomplete) cast a 0 8 24 shadow at 40% black; dialogs and the docked engine window 0 24 60 at
  60%.

## Icons

Outline icons on a 16 px grid with a 1.5 px stroke and round joins, drawn in the colour of the
text beside them (`text`, `text.muted`, or a status colour). The mockups follow
[Lucide](https://lucide.dev) (ISC license), which is the icon set: play, pause, step, rotate-ccw for
Restart map, box for entities, star for agent actions, lock for read-only, check, alert-triangle.
Icons the set lacks (map tools, QuakeC kinds) are drawn to the same grid and stroke. Every icon-only
button has an accessible name (QA-9). The icon files are recorded in `ATTRIBUTION.md` when they are
added.

## Motion

Short and functional: 100 ms for hover and press feedback, 160 ms ease-out for popovers, panels
and the inline-edit diff. Nothing animates on its own while the game runs, and every duration is
zero when the OS asks for reduced motion.

## Window chrome

| OS | Title bar | Menus |
| --- | --- | --- |
| macOS | Native traffic lights on the left of a 38 px unified title bar; the project name and branch beside them | The system menu bar |
| Windows | Client-side title bar: an app menu button on the left, the project name, and the caption buttons (minimise, maximise, close) on the right, 44 px wide, with the OS snap layouts on the maximise button | The app menu button |
| Linux | Client-side title bar: an app menu button on the left, the project name; window buttons follow the desktop's button layout (close only, on the right, by default on GNOME) | The app menu button |

The game runs in its own docked window in v1 (ENG-9): the preview host's window, with its own
native title bar, placed beside the IDE and following its focus. The toolbar shows that the
preview host is running and where it is docked; there is no engine picker, because the IDE runs only
its own preview host ([ADR-0005](../adr/0005-default-preview-target.md)). The embedded game view
(ENG-10) is post-MVP and in none of the v1 mockups.

## Layout coverage

Every panel named in UI-2 appears in at least one approved mockup, with real QuakeC and entity
fields:

| UI-2 panel | Mockups |
| --- | --- |
| Toolbar: Play, Pause, Step, Restart map | 01, 02, 03 (and the map editor's Play from here, 05) |
| Project | 01, 03, 05 (collapsed in 02) |
| Entities | 01, 02 (the map's entities in 05) |
| Editor | 01, 02, 03 |
| Game | 01, as the docked engine window (ENG-9) |
| Properties | 01 (read-only while playing), 02 (editable while paused), 05 (typed entity keys) |
| Agent | 01 (a finished task), 02 (a hunk to accept or reject), 03 (inline edit) |
| Problems | 01 and 02 as a tab with its count; 05 as an inline diagnostic and the status-bar count |
| Console | 01 |
| Debugger | 02 (call stack, variables and watches, breakpoints) |
