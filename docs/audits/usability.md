# Usability audit

UI-24, L-80, L-83. Every panel and window of the app checked against the rules of
[`docs/design/usability.md`](../design/usability.md), on 2026-11-25 and again on 2026-11-27,
from the app's own snapshots (`apps/desktop/snapshots/`) and its code. The first table lists each panel and window by the
title its help button gives it (`Topic`), plus the parts that have no help button;
`help::tests::the_usability_audit_covers_every_panel` checks that every help topic has a row.
The second table lists each problem found, the rule it breaks, the fix and the UI test that
drives the fixed control as a modder would.

## Panels and windows

| Panel or window | Checked | Problems |
| --- | --- | --- |
| Toolbar | Play, Pause, Step, Restart map, the preview host's place, Pop Out, Full Screen, Search commands, the panel buttons | U8 |
| Project | The file tree, Open Folder, New Project; empty state "No project open." | None |
| Symbols | The symbol list and its filter | None |
| Entities | The running game's entities and their filter; empty state while the game is stopped | None |
| Properties | The running game's selected entity: its fields, typed or picked while paused; Open spawn function, Ask about this | U11, U15 |
| Map | The map outline: layers, groups and entities, its filter | None |
| Map properties | The map editor's Properties panel: compile, leaks, camera, review, layers and the selected entity's keys | U1, U2, U3, U4, U5, U6, U7, U12, U16, U17 |
| Agent | The task box, threads and their search, the provider form | None |
| Outline | The open file's symbols | None |
| Git | Branch picker, new branch, changes, commit message | None |
| Problems | The list, Open as excerpts; empty state "No problems." | None |
| Console | The game's console, its input and filter | U13 |
| Debugger | Call stack, watches, breakpoints, the console | U13, U14 |
| Terminal | Terminal tabs and the input line | None |
| Editor | The code editor, its find and replace bar, inline edit, rename, the settings file | U18 |
| Search | The project search and replace, its options | None |
| Game | The Game tab | None |
| Map view | The tool bar (select, brush, clip, vertex, move, ...), shading, the pane, grid and entity filter pickers, links | None |
| Textures | The texture browser, its filter and Import texture | None |
| Entity classes | The entity browser and its filter | None |
| Prefabs | The prefab browser, the prefab name and Save | None |
| Issues | Check map, the issue list and its fixes, the map search | None |
| Logic | Fire, Check logic, the trace and its scrubber, the issue list | U9 |
| Balance | Count, the kind of game, the table a column a skill, each row's name selecting its entities and each skill's heading the view filter | None |
| Your Quake files | The window that finds the game's files | None |
| Import texture | The import preview: the name, the colour matching, Import and Cancel | U10 |
| Status bar | The panel buttons, the problem counts, the preview host's status | None |
| Command line | The command line and its offers | None |
| Command palette | The palette and its prompts, whose choices are pickers | None |
| New project | Which game the mod is for, as radio buttons with what each starts from | None |
| Welcome | The three numbered steps, each with its button | None |

Checked and kept as they are, with the reason:

- **Free text where the set is open.** A new branch name, a commit message, a prefab's name, a
  search query, the agent's task, and the agent's model id (a local Ollama model or an
  OpenAI-compatible server's has no list the app can know) are typed, each with a hint saying
  what to type (rule 1). A watch's expression is free too, but its names are not: U14 offers
  them.
- **A running game's float, vector and string fields** are typed while paused: such a field
  takes any value of its type, as a debugger's watch does in Zed and VS Code (rule 1). Its
  entity and function fields hold a known set and offer it (U15).
- **A class's own spawnflags** keep the code's names (`DOOR_START_OPEN`): they are the only
  names the QuakeC gives them (rule 4).
- **Icon-only controls of the side panels and the map tool bar** each name their action on
  hover and to a screen reader (`Icon::tool`, `Icon::toggle`); the accessibility audit's
  `tests::every_area_and_control_has_a_name` and `tests::every_map_editor_control_has_a_name`
  hold every control to a name (rules 3 and 7).

## Problems found and fixed

| | Control | Rule | Fix | Test |
| --- | --- | --- | --- | --- |
| U1 | Map properties: a targeting key already set (`target`, `killtarget`, ...) was a free text box; the map's names were offered only when a key was being added | 1, 2 | The field offers the map's targetnames as it is typed, each with the classnames of the entities that have it, takes a new name typed, and marks a name no entity has; the eyedropper stays beside it | `tests::a_target_is_picked_from_the_names_in_the_map` |
| U2 | Map properties: a new key was a free text box, with the keys the class reads as a row of buttons above it under "Keys its class reads:", which did not say what clicking one did | 1, 2, 5, 7 | One key field offers the keys the class reads and the entity does not set, each with its type, default and the class's comment, and fills in the default when one is taken; any other key is still typed and added | `tests::a_new_key_is_offered_as_it_is_typed` |
| U3 | Map properties: a new targeting key's value had a second list, Targetnames, beside the value field | 5 | The value field itself offers the map's targetnames with their classnames | `tests::a_target_is_picked_from_the_names_in_the_map` |
| U4 | Map properties: `classname` was a free text box | 1, 2 | It offers the project's classes, each a point or a brush entity | `tests::the_properties_panel_says_what_each_control_is` |
| U5 | Map properties: keeping an entity off a skill or out of deathmatch meant typing 256, 512, 1024 or 2048 into `spawnflags`; a class that tests no flag showed no checkbox at all | 1, 4 | The engine's flags (EDICT-8.4) follow the class's own as checkboxes named Not in easy, Not in normal, Not in hard or nightmare and Not in deathmatch, for every class but `worldspawn` (`project.entities` and `map.read_entity` list them too) | `tests::the_properties_panel_says_what_each_control_is` |
| U6 | Map properties: the compile profile picker showed "Full" with no label | 4, 7 | Labelled "Compile with" | `tests::the_properties_panel_says_what_each_control_is` |
| U7 | Map properties: the camera position was three bare numbers | 2 | Each field shows its axis: `X -320`, `Y 0`, `Z 88` | `tests::the_properties_panel_says_what_each_control_is` |
| U8 | Toolbar: Pop Out and Full Screen were drawn as plain text, the same as the label beside them | 7 | Drawn framed, as buttons | `snapshots::pop_out_is_a_button_that_says_what_it_does`; the goldens |
| U9 | Logic: the trace's scrubber was labelled "step" | 4 | Labelled "events shown" | `tests::fire_shows_what_the_quakec_did` |
| U10 | Import texture: Nearest colour and Dither had no label saying what they choose | 7 | Labelled "Colours" | `tests::a_dropped_png_is_imported_into_the_browser` |
| U11 | Properties (running game): the lock shown while the game runs had no name for a screen reader, since a plain label takes none from `icons::mark` | 3 | `Icon::sign` draws an icon that only shows a state, named on hover and to a screen reader; the lock, the grid and filter signs of the map tool bar and the new mark of U1 use it | `entities::tests::the_lock_says_why_fields_are_read_only` |
| U12 | Map properties: Fire beside the selected entity's class said only "Fire" on hover, while the Logic tab's Fire and the view's menu said what it does | 3 | It says on hover that it runs the entity's use or touch and changes nothing | `tests::the_properties_panel_says_what_each_control_is` |
| U13 | Console: the command box was free text, though the engine's commands and settings are a fixed list | 1, 2 | The box offers them as typed, each said to be a command or a setting with its default (`sv_gravity   setting, 800 by default`), and after `map` or `changelevel` the game's levels; one taken is put in the box for its words, and Enter runs the line | `debugger::tests::a_console_command_is_taken_from_the_offers`, `debugger::tests::a_level_is_offered_after_map` |
| U14 | Debugger: Add watch was free text, though what a watch reads where the game stopped is known | 1, 2 | The box offers the locals, parameters and globals visible on the line the game stopped on, and the fields of `self`, each with its type (`preview.watch_names`); any other expression is still added on Enter | `debugger::tests::a_watch_is_taken_from_the_names_in_scope` |
| U15 | Properties (running game): an entity or function field was typed while paused, its number or function's name remembered | 1, 2 | An entity field offers the edicts in use as the panel writes them (`monster_army #7`), a function field the project's functions with their files | `entities::tests::entity_and_function_fields_offer_what_they_may_hold` |
| U16 | Map properties: a key whose values the class's comment numbers (`"sounds"`, `1) stone`) was a number typed, its meaning only in the comment | 1, 2 | The field offers the numbered values with their names, and shows the name of the one set beside it | `tests::a_numbered_choice_and_a_sound_are_picked` |
| U17 | Map properties: a key the code loads a level, model or sound by was free text, and a name no file has was found only when the game failed to load it | 1, 3 | The field offers the project's and the game's files of that kind (MAP-8.11), each said to be in the project or the game, with the case the file has, and once the game's files were searched marks a name no file has | `tests::a_numbered_choice_and_a_sound_are_picked` |
| U18 | Editor: the settings file was plain TOML, its keys and values only in `docs/settings.md` | 1, 7 | Completion in the file offers its tables, the keys of the table the caret is in with what each does, and a key's values with what each does, from the settings' schema | `editor::tests::the_settings_file_completes_from_its_schema` |
