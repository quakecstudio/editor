# Accessibility audit against Apple's Human Interface Guidelines

P7-12 (QA-9). Each guideline of Apple's Human Interface Guidelines, Accessibility
(https://developer.apple.com/design/human-interface-guidelines/accessibility, fetched 2026-11-15,
page last changed June 9, 2025), mapped to what the macOS app does and the test or check that
proves it. Guideline titles are quoted as the page writes them. Where the page sets a number, the
number is quoted too.

Windows and Linux are checked by hand later (L-16): the settings below are read only on macOS,
and the app keeps its standard look elsewhere.

## Vision

| Guideline | What the app does | Proof |
| --- | --- | --- |
| "Support larger text sizes." "Ideally, give people the option to enlarge text by at least 200 percent" | `view.zoom` draws the whole interface, text and icons, from 50 to 300 percent. Cmd+Plus, Cmd+Minus and Cmd+0 and View > Zoom In, Zoom Out and Actual Size step it by 10 percent, and the layout file keeps it. A side panel takes at most 30 percent of the window and the bottom panel 45 percent, the right panel scrolls, and tab rows wrap, so at 200 percent every control can still be reached. | `qs_layout::zoom::tests`; `tests::the_interface_zooms_and_the_layout_keeps_it`; snapshots `shell-zoom-200` and `map-editor-zoom-200` |
| "Use recommended defaults for custom type sizes." macOS: "13 pt" default, "10 pt" minimum | Body text is 13 pt; the smallest style, captions and panel headings, is 12 pt. | `docs/design/tokens.toml` `[type_scale]` |
| "Strive to meet color contrast minimum standards." "Up to 17 pts ... 4.5:1" | Every text and icon pair the UI draws reaches 4.5:1 in the light and dark themes. | `cargo xtask contrast` in CI |
| "ensure it at least provides a higher contrast color scheme when the system setting Increase Contrast is turned on" and "check the minimum contrast in both light and dark appearances" | The app follows the system's light or dark appearance. With Increase contrast on, it draws the `dark_contrast` or `light_contrast` theme, where every pair reaches 7:1 and borders are visible. | `cargo xtask contrast` (7:1 floor on those themes); `tests::the_look_follows_the_display_settings`; `tests::the_increased_palettes_have_more_contrast`; snapshots `shell-light`, `shell-contrast` and `shell-light-contrast` |
| "Prefer system-defined colors." | Not followed: the theme is the approved design's own (ADR-0010, `docs/design/visual.md`). Its increased-contrast themes stand in for the system colours' accessible variants. | `docs/design/tokens.toml` |
| "Convey information with more than color alone." | Problems carry an error or warning icon and a code. The status bar's counts have icons and names. An unsaved file is marked `M`. The Git gutter marks added lines with a solid bar, changed lines with a dashed bar and removed lines with a square. A missing texture is named in words. The map views mark the selection in colour, and the outline and Properties name it as well. | `qs-editor-view` `tests::paint::the_gutter_marks_changes_against_head`; `tests::every_panel_tab_and_tool_has_help` (counts named) |
| "Describe your app's interface and content for VoiceOver." | Every area has a role and a name. Every button, field, checkbox, radio button, combo box, number field, link and tab in the window, the map editor and each of its tools and tabs, and each bottom tab has a name. A button drawn as an icon is named by what it does. The editor exposes its text runs, caret and selection (P2-23). | `tests::every_area_and_control_has_a_name`; `tests::every_map_editor_control_has_a_name`; `qs-editor-view` AccessKit tests; the VoiceOver pass below |

## Hearing

| Guideline | What the app does | Proof |
| --- | --- | --- |
| "Support text-based ways to enjoy audio and video." | The IDE plays no audio or video of its own. The game's sound is Quake's, and the game's console prints its messages in the Console tab. | Not applicable |
| "Use haptics in addition to audio cues." / "Augment audio cues with visual cues." | The IDE gives no audio cues. Every outcome is shown in text: the status bar, the toolbar's reload status, Problems. | Not applicable |

## Mobility

| Guideline | What the app does | Proof |
| --- | --- | --- |
| "Offer sufficiently sized controls." macOS: "28x28 pt" default, "20x20 pt" minimum | Controls are 30 pt wide and 24 pt tall at least (`[size] control` and `row`), and help buttons are 20 by 20 pt. | `docs/design/tokens.toml` `[size]`; `help::HELP_SIZE` |
| "Consider spacing between controls as important as size." | Item spacing is the spacing scale's steps (`[space]`); toolbars separate their groups. | `docs/design/visual.md`, Spacing and density |
| "Support simple gestures for common interactions." / "Offer alternatives to gestures." | The map views take drags and the keyboard alike. Arrows and Page Up and Page Down move the selection, Alt with Left or Right turns it, and W, A, S, D, Q and X fly. Every command is in the palette. | `tests::the_map_view_takes_trenchbrooms_keys`; `tests::every_registered_command_is_reachable_from_the_palette` |
| "Let people use Voice Control" / "Support mobility-related assistive technologies." | Voice Control and Switch Control act on the accessibility tree's names, the same names VoiceOver reads. | As for VoiceOver |
| "Integrate with Siri and Shortcuts" | Not done: the CLI (`quakec-ide run <command>`) can be called from Shortcuts' Run Shell Script. | `docs/commands.md` |

## Speech

| Guideline | What the app does | Proof |
| --- | --- | --- |
| "Let people use the keyboard alone to navigate and interact with your app." | Cmd+1 to Cmd+4 give the keyboard to the editor or a panel, and Tab moves through its controls. Every command is in the palette (Cmd+Shift+P), and no system shortcut is overridden. | `tests::focus_commands_reach_every_panel`; `tests::every_registered_command_is_reachable_from_the_palette` |
| "Support Switch Control." | As for Voice Control. | As for VoiceOver |

## Cognitive

| Guideline | What the app does | Proof |
| --- | --- | --- |
| "Keep actions simple and intuitive." | Each panel, tab row and dialog has a help button explaining it and linking to the guide (P7-11). | `tests::every_panel_tab_and_tool_has_help` |
| "Minimize use of time-boxed interface elements." | Nothing in the app dismisses itself on a timer. Notices stay until the next action, and popups close on a click. | Code read: no timed dismissal in `apps/desktop/src` |
| "Let people control audio and video playback." | The game runs only after Play, and Pause and Stop are on the toolbar. | The user guide's play loop page, The toolbar |
| "Allow people to opt out of flashing lights in video playback." | The IDE plays no video. The game's own flashes are Quake's. | Not applicable |
| "Be cautious with fast-moving and blinking animations." "When this setting is active, ensure your app or game responds by reducing automatic and repetitive animations" | Nothing animates on its own; the code editor's caret does not blink. With Reduce motion on, egui's animation time and scroll animation are zero and a text field's caret stops blinking. | `tests::the_look_follows_the_display_settings` |
| "Optimize your app's UI for Assistive Access." | An iOS and iPadOS feature; not applicable on macOS. | Not applicable |

## Checks a person makes

- **VoiceOver pass:** with VoiceOver on, open a project and a map, and move through each panel with VO+arrow keys and Tab. Check each control is read by its name and role, and record the result with Accessibility Inspector's audit on the reference Mac. This needs a person at the Mac (P7-12's verification).
- **Increase contrast and Reduce motion:** turn each on in System Settings > Accessibility > Display and switch back to the app. The look changes when the window gains focus.
