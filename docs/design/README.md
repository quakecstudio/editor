# Design

| File | What it is |
| --- | --- |
| [`design-doc.md`](design-doc.md) | The design document (revision 36, 2026-10-15): goals, licensing, architecture, foundations, compiler, engine bridge, agent, UI, map editor, quality, MVP scope, risks |
| [`mockups/`](mockups/) | The approved mockups of the main screens (approved on [#199](https://github.com/quakecstudio/editor/issues/199#issuecomment-6202660819)) |

The plan in [`../../plan/`](../../plan/README.md) turns the design document into requirements and issues. When the two disagree, fix one of them in the same PR; `plan/check.py` only checks that every requirement is covered, not that the prose still matches.

## Mockups

These are design targets for the UI layer (L7), not screenshots. The app is built to match them once the UI framework is chosen ([#10](https://github.com/quakecstudio/editor/issues/10)) and the app shell exists ([#73](https://github.com/quakecstudio/editor/issues/73)).

| Image | Shows | Requirements |
| --- | --- | --- |
| [`01-play-loop.png`](mockups/01-play-loop.png) | v1 play loop: code beside the engine, which runs in its own window docked to the right; live entities, read-only properties while playing, a finished agent task, the console, a next-edit suggestion | UI-2, UI-3, UI-5, UI-7, ENG-9, ENG-13 |
| [`02-paused-inspect.png`](mockups/02-paused-inspect.png) | Paused at a breakpoint: editable properties, inline values, call stack, watches, breakpoints, "Ask about this entity" and a hunk to accept or reject | UI-8, UI-9, UI-12, UI-13, AGT-3, ENG-4, ENG-14 |
| [`03-inline-edit.png`](mockups/03-inline-edit.png) | Cmd/Ctrl+K inline edit shown as a diff in place | UI-4, UI-6 |
| [`04-command-palette.png`](mockups/04-command-palette.png) | The command palette over the registry, with command ids and shortcuts | UI-10, PRIN-3 |
| [`05-map-editor.png`](mockups/05-map-editor.png) | Map editor: 3D and top views, map tools, play from here, background full compile, typed entity keys from QuakeC, an unknown-key diagnostic, the texture browser | MAP-1 to MAP-5, MAP-7, MAP-13, MAP-14 |
| [`06-theme-and-chrome.png`](mockups/06-theme-and-chrome.png) | Theme tokens for dark and light with contrast ratios, code colours, type, spacing, and window chrome on macOS, Windows and Linux | UI-14, UI-15, QA-9 |

The embedded game view (ENG-10) is post-MVP and is deliberately not in the v1 mockups.

### Design language

- **Feel:** Zed's editor (calm, dense, keyboard-first) with Roblox Studio's play loop (UI-14).
- **Colour:** warm graphite surfaces with one ember accent (`#EE8B45` dark, `#A84D16` light); blue (`#6FA8DC`) only for focus and selection. Every text pair meets WCAG AA; the ratios are on the theme board.
- **Type:** IBM Plex Sans for the interface (13 px, 12 px semibold for panel headings), JetBrains Mono for code (13 px on a 20 px line, ligatures off so QuakeC reads literally).
- **Code colours:** keywords copper, types teal, functions gold (builtins in italic), constants and numbers lilac, strings green, vectors teal, comments grey italic.

The tokens move into `docs/design/tokens.toml`, with a CI contrast check, as part of [#199](https://github.com/quakecstudio/editor/issues/199).

### Sources

`mockups/source/` holds each board as a `.dc.html` file and the SVG drawings they use. They open in a browser as plain HTML (the `support.js` they reference is the design canvas's runtime and is not needed to view them). The editable originals are in the "QuakeC Studio mockups" design canvas on claude.ai, which is private to Justin until shared.

To regenerate the PNGs after editing a source file, open it in a browser at the board's size (1920x1080 for the play loop, 1440x660 for the theme board, 1440x900 for the rest) and take a screenshot.
