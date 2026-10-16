# ADR-0010: Use warm graphite with one ember accent, Plex and JetBrains Mono, and native chrome

- **Status:** Accepted
- **Date:** 2026-10-15
- **Decided by:** Justin Plock, 2026-10-15, [approval comment on #199](https://github.com/quakecstudio/editor/issues/199#issuecomment-6202660819)
- **Issue:** [#199](https://github.com/quakecstudio/editor/issues/199)
- **Requirements:** UI-14, UI-15, UI-2, QA-9

## Question

What does QuakeC Studio look like - colour, type, density, icons and window chrome - before the
UI layer is built on it?

## Context

UI-15 asks for the visual design to be set before the UI is built: theme tokens for dark and light,
UI and code typefaces, spacing and density, icons, OS window chrome, and approved mockups of the
main screens. UI-14 sets the feel ("Zed's editor feel and Roblox Studio's play loop: calm, dense,
keyboard-first, native on each OS") and QA-9 requires WCAG AA contrast. The app shell (P2-17,
[#73](https://github.com/quakecstudio/editor/issues/73)) takes its theme, type and spacing from
`docs/design/tokens.toml` and must match the approved mockups; the UI framework spike
([#10](https://github.com/quakecstudio/editor/issues/10)) builds against the same boards.

Two earlier decisions shape the screens: the IDE runs only its own preview host, so the toolbar has
no engine picker ([ADR-0005](0005-default-preview-target.md)), and the product is QuakeC Studio,
with no artwork or trade dress from id or ZeniMax ([ADR-0006](0006-product-name.md)). In v1 the
game runs in its own window docked beside the IDE (ENG-9); the embedded game view (ENG-10) is
post-MVP.

Six boards were drawn in a design canvas and exported to `docs/design/mockups/`: the v1 play loop,
paused inspection with an agent hunk, inline edit, the command palette, the map editor, and theme,
type and window chrome. Justin approved all six on #199.

## Options

| Option | Consequences |
| --- | --- |
| A. Warm graphite surfaces, one ember accent, blue only for focus; IBM Plex Sans and JetBrains Mono; one dense layout; native chrome per OS (decided) | Reads as calm and distinct from stock editor themes; the single accent leaves ember to mean "act here" and lets status colours stand out. Needs a light theme designed to the same tokens and a contrast check, both delivered with this ADR. Two OFL-licensed font families to bundle |
| B. A cool neutral grey theme with a blue accent, close to VS Code's Dark+ or Zed's default | Familiar, but indistinct from the editors it is compared to, and the blue accent collides with the focus colour, so focus and "act here" look the same |
| C. A Quake-styled skin: rust and stone textures, a display face, game-like panels | Strong identity, but low density, textured surfaces that fail contrast, and trade dress close to id's, which ADR-0006 rules out |
| D. Fully custom window chrome on every OS, identical everywhere | One look to build and test, but loses the macOS menu bar and traffic lights and the Windows snap layouts, against UI-14's "native on each OS" |

## Decision

**Option A.** QuakeC Studio uses warm graphite surfaces with a single ember accent (`#EE8B45` dark,
`#A84D16` light); blue (`#6FA8DC` dark) is used only for focus, the keyboard selection and links.
Dark is the default and light is a full theme with the same tokens. The interface is set in IBM
Plex Sans (13 px, 12 px semibold panel headings) and code in JetBrains Mono (13 px on a 20 px line,
ligatures off). There is one dense layout: 24 px rows, a 44 px toolbar, a 4 to 24 px spacing scale.
Icons are outline icons on a 16 px grid in the Lucide set. Window chrome is native per OS: macOS
traffic lights and the system menu bar; on Windows and Linux a client-side title bar with an app
menu button, Windows caption buttons on the right, and Linux buttons following the desktop's
layout. The game runs in the preview host's own docked window, and the toolbar shows that host
with no engine picker.

`docs/design/tokens.toml` holds every value and is the only place the UI layer reads them from;
`docs/design/visual.md` explains them. Justin approved the six boards on #199 on 2026-10-15; that
approval is this decision. Three details were filled in afterwards, within the approved direction:
the light theme's values beyond the sample on the theme board (chosen to pass the contrast check),
Lucide as the icon set the boards' icons follow, and the toolbar and palette change for ADR-0005
described below.

## Consequences

- `docs/design/tokens.toml`, `docs/design/visual.md` and the mockups in `docs/design/mockups/` are
  the reference for every L7 issue; P2-17 ([#73](https://github.com/quakecstudio/editor/issues/73))
  reads its theme from `tokens.toml` and is checked against the mockups.
- `cargo xtask contrast`, a step of `cargo xtask ci`, fails any colour pair in `tokens.toml` below
  WCAG AA (4.5:1) in either theme, and any colour token that is in no contrast rule. A new colour
  is added to `tokens.toml` with its pairs, never hard-coded in the UI.
- Following ADR-0005, the mockups' toolbar shows the preview host as a status, not a picker, and
  the command palette lists "Display the preview host window" instead of choosing an engine. The
  four boards that showed a picker were re-exported from their sources with only that change.
- Field outlines are decorative and do not reach 3:1; fields are identified by label, position and
  fill, and the focused field's `focus` outline is checked. A high-contrast theme is not part of v1.
- IBM Plex Sans and JetBrains Mono (SIL OFL 1.1) and the Lucide icons (ISC) are bundled as separate
  files and recorded in `ATTRIBUTION.md` when the UI adds them; the license review (P4-22) covers
  them.
- Not chosen: a stock-looking cool grey theme (B), a Quake-styled skin (C), identical custom chrome
  on every OS (D), font ligatures for code, and an embedded game view in v1 (ENG-10 stays post-MVP).

## Links

- Issue: [#199](https://github.com/quakecstudio/editor/issues/199); approval:
  [Justin's comment](https://github.com/quakecstudio/editor/issues/199#issuecomment-6202660819).
- Related: [ADR-0005](0005-default-preview-target.md) (only our own preview host, no engine
  picker), [ADR-0006](0006-product-name.md) (the name; no id or ZeniMax trade dress).
- Design: [`docs/design/visual.md`](../design/visual.md), [`docs/design/tokens.toml`](../design/tokens.toml),
  [`docs/design/mockups/`](../design/mockups/).
- [WCAG 2.2, success criterion 1.4.3 Contrast (Minimum)](https://www.w3.org/TR/WCAG22/#contrast-minimum)
  and the [relative luminance definition](https://www.w3.org/TR/WCAG22/#dfn-relative-luminance).
- [IBM Plex](https://github.com/IBM/plex) and [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono)
  (SIL OFL 1.1); [Lucide](https://lucide.dev) (ISC).
