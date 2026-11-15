# TrenchBroom parity audit

MAP-30 asks that the map editor match or exceed TrenchBroom on every feature its manual lists.
This audit walks
[TrenchBroom's manual](https://github.com/TrenchBroom/TrenchBroom/blob/8f329f11e65e3d21b355a42bcae199cb3e4e0812/app/resources/documentation/manual/index.md)
at `TrenchBroom/TrenchBroom@8f329f11e65e3d21b355a42bcae199cb3e4e0812`, section by section, and
maps each feature to ours with the test that proves it (P7-08,
[#823](https://github.com/quakecstudio/editor/issues/823)).

Each row says one of:

- **Same:** we do what the manual describes.
- **Beyond:** we do it and more, said how.
- **Gap:** we lack it or do it worse; the gaps are listed at the end and closed before #823 does.
- **Not for Quake:** the feature serves another game (Quake 2 or 3, Half-Life) or another
  editor's packaging, so it does not apply to a Quake map editor.

Tests are named `crate-or-app path::test`; `desktop` is `apps/desktop/src`, `map-edit` is
`crates/qs-map-edit/src`, `map` is `crates/qs-map/src`.

## Introduction: Features

| TrenchBroom | Ours | Test | Verdict |
| --- | --- | --- | --- |
| Editing in 3D and in up to three 2D views | One 3D view, or 3D beside top, front and side (`view.map_panes`) | `qs-layout` map view tests | Same |
| High performance renderer for huge maps | Textured and lit 3D view on wgpu; the benchmark below | `qs-bench textured_view_e1m1`; the benchmark | See benchmark |
| Unlimited undo and redo | Every `map.*` edit is a session command with undo and redo | `map-edit tests::every_entity_edit_undoes_and_the_map_saves` | Same |
| Macro-like command repetition | `edit.repeat` repeats the last map edit | `desktop tests` (edit.repeat) | Same |
| Linked groups | Read, edited and written as TrenchBroom writes them | `map-edit groups::tests::an_edit_in_a_linked_group_changes_every_copy_in_one_step` | Same |
| Issue browser with quick fixes | `map.issues`, `map.fix`; issues from the mod's own QuakeC | `map-edit issues::tests::a_misspelled_key_is_renamed` | Beyond: checks keys and classes against the mod's QuakeC |
| Run external compilers and launch engines | Compile profiles with our own bsp, vis and light, tools, copy and launch; Play from here | `desktop tests::a_profile_compiles_from_the_map_view`; `qs-bench play_from_here_e1m1` | Beyond: compilers built in; Play from here starts at the camera |
| Point file support | The leak trail from our qbsp, drawn and flown | `desktop tests::a_leak_is_found_and_flown_along` | Same |
| Portal file support | Show portals (`map.portals`) | `qs-workbench map_compile::tests::map_portals_reads_the_last_compiles_portals` | Same |
| Automatic backups | `map.backup`, a backup each minute with unsaved edits, restore | `map-edit backup::tests::backups_are_numbered_and_the_oldest_removed` | Same |
| Free and cross platform | GPL-3.0-or-later; macOS, Windows, Linux | CI's three platforms | Same |
| Vertex editing with edge and face splitting, several vertices together | `map.brush_vertex`, `map.brush_vertices`, `map.brush_split_edge`, `map.brush_split_face` | `map-edit brush_tools::tests::vertices_move_and_split` | Same |
| Clipping tool with two and three points | `map.brush_clip` by a plane | `map-edit brushes::tests::a_brush_is_clipped` | Same: the command takes three points; the tool draws two on the ground |
| Scale and shear tools | `map.brush_scale`, `map.brush_shear` | `map-edit brush_tools::tests::brushes_scale_and_shear` | Same |
| CSG merge, subtract, hollow | And intersect | `map-edit brushes::tests::a_brush_is_subtracted`, `brush_tools::tests::brushes_hollow_and_intersect` | Same |
| UV view | The UV editor (`map.face_uv`) | `map-edit uv::tests::fit_uses_the_wad_s_size_and_undoes` | Same |
| Alignment lock for all brush edits | Textures locked on move | `map-edit brushes::tests::a_brush_moves_with_its_textures_locked` | Same |
| Multiple material collections | Every WAD of the `wad` key, the project's, the game's | `map-edit textures::tests::the_first_wad_holding_a_name_wins` | Same |
| Entity browser with drag and drop | The Entities browser; drag to place | `desktop tests::entities_are_placed_and_linked_without_typing` | Same |
| FGD, ENT and DEF entity definitions | The mod's own QuakeC, and FGD files | `qc-sema entities::tests` | Beyond: read from the code the game runs |
| Mod support | The project is the mod | `qs-workbench` project tests | Same |
| Entity link visualisation | Links for the selection or every entity, from the keys the QuakeC compares with `targetname` | `desktop tests::links_follow_the_keys_the_code_fires_by` | Beyond: a mod's own target keys link |
| 3D models in the editor | The model's first frame as a wireframe | `desktop map_scene` model test | Gap G1: TrenchBroom draws models textured |
| Smart entity property editors | Spawnflags, angle, colour, choices, targets | `map-edit links::tests::the_spawnflags_named_by_the_code_come_with_the_keys` | Same |

## Getting started

| TrenchBroom | Ours | Test | Verdict |
| --- | --- | --- | --- |
| Main window: menu, toolbar, editing area, inspector, info panel | Menu, toolbar, Map tab, Properties panel, bottom panel | `desktop snapshots` | Same |
| One to four viewports; 2D top, front and side | `view.map_panes`: one or four | `qs-layout` map view tests | Gap G7: no two- or three-pane layout |
| Compass in each viewport | X, Y and Z in each pane's lower left, each with its letter | `desktop map_panes::tests::the_compass_turns_with_the_view` | Same |
| Viewport focus highlight | The pane under the pointer takes input, with no highlight | none | Gap G7 |
| Map bounds shown as a guide | TrenchBroom's Quake soft bounds, -4096 to 4096, in the 2D views | `desktop tests::the_2d_views_draw_the_soft_map_bounds` | Same; not editable per map |
| Look by right-drag; move with keys; speed on the wheel | Right-drag looks, WASD flies | `desktop tests::the_camera_starts_behind_the_player_start` | Same: WASD flies, the wheel travels |
| Orbit about a clicked point | Alt+right-drag orbits about the brush point under the pointer, else 256 units ahead | `desktop tests::alt_right_drag_orbits_and_right_drag_looks`, `qs-render camera::tests::orbiting_keeps_the_centre_where_it_was_seen`, `map_scene::tests::a_ray_s_hit_is_its_first_brush` | Same |
| Focus on selection | Choosing an outline row frames the entity | `desktop tests::focus_frames_the_selection_and_escape_clears_it`, `map_scene::tests::the_selection_s_box_holds_its_entities_and_brushes` | Same: Cmd+U frames every selected entity and brush |
| Move camera to a position | The camera's x, y and z in the Properties panel, always shown | `desktop tests::focus_frames_the_selection_and_escape_clears_it` | Beyond: the position is shown as it moves |
| 2D views pan and zoom; linked 2D views | Pan and zoom in each 2D pane | `desktop map_panes` tests | Gap G7: 2D views are not linked |
| FOV setting and temporary zoom | `view.map_camera` field of view, 50 to 150 as TrenchBroom's slider; Shift+wheel zooms, Reset zoom puts it back | `desktop tests::the_camera_settings_reach_the_3d_view`, `qs-layout map_camera::tests` | Same; the reset is a button, as Ctrl+Alt+Z undoes here |

## Selection

| TrenchBroom | Ours | Test | Verdict |
| --- | --- | --- | --- |
| Click selects; Ctrl+click adds | Click selects; Shift+click adds | `desktop tests` | Same |
| Selection drilling with the wheel in 3D | None | none | Gap G8 |
| 2D click picks by the candidates under the pointer | First brush the ray enters | none | Gap G8 |
| Select all, select touching, select inside, select tall | Cmd+A, Cmd+T, Cmd+E, Cmd+Shift+E, as TrenchBroom's (the selecting brushes deleted) | `desktop map_select::tests`, `desktop tests::the_selection_keys_select_all_invert_siblings_and_touching` | Same |
| Select siblings, invert selection, deselect all | Select siblings in Properties (Ctrl+B is Compile here), Cmd+Alt+A, Cmd+Shift+A and Escape | as above | Same |
| Select brush faces with Shift+click; all faces by double click | The UV editor's face pick | `desktop map_uv` tests | Same: the UV editor picks faces |
| Search box with plain text and queries | `map.search` by class, key, texture and box | `map-edit issues::tests::search_by_class_key_texture_and_box` | Gap G19: plain fields, not TrenchBroom's query language |

## Editing

| TrenchBroom | Ours | Test | Verdict |
| --- | --- | --- | --- |
| Mods editor (`_tb_mod`) | The project and the game folder | none | Not for Quake as TrenchBroom does it: the project is the mod |
| Entity definition files (`_tb_def`) | The mod's QuakeC, FGD files | `qs-workbench entity_classes::tests` | Beyond |
| Wad files with a smart editor; drop a wad on the window | The `wad` key, Import texture | `map-edit texture_import::tests::an_import_extends_the_key_the_map_uses` | Gap G16: a WAD dropped on the window is not added |
| Tools: modal and permanent | Toolbar: Select, Brush, Clip, Vertex, Move, Measure, Shape | `desktop map_tools` tests | Same |
| Cancel a drag or a tool with Escape | Escape drops a drag, then picked corners, then the tool, then the selection | `desktop map_tools::tests::escape_cancels_the_drag_then_the_corners_then_the_tool`, `desktop tests::escape_in_the_map_view_clears_the_selection` | Same |
| Axis restriction while moving | None | none | Gap G11 |
| Grid from 0.125 to 256, set from the menu or Alt+Ctrl+wheel | `view.map_grid`, the toolbar's Grid picker, TrenchBroom's + - and 1 to 9 | `desktop tests::the_grid_keys_set_the_step_the_selection_moves_by`, `qs-layout map_grid::tests` | Same |
| Map view context menu: group, layers, select all of a class, reveal material, create entities | Entities browser context menu only | none | Gap G12 |
| Simple shapes by drag in 3D and 2D | Brush tool | `map-edit brushes::tests::a_box_is_created_on_the_grid` | Same |
| Complex shape tool: points to a convex brush | Shape tool: cylinders, cones, arches, spheroids, rocks, curves | `map-edit shapes::tests::a_shape_is_added_in_one_undoable_step` | Gap G13: no hull of placed points |
| Point entities by drag or context menu | Drag from the Entities browser | `desktop tests` | Same |
| Brush entities from selected brushes | None | none | Gap G13 |
| Duplicate | `map.duplicate`, with link numbers moved on | `map-edit links::tests::a_duplicate_numbers_its_names_onward` | Beyond |
| Copy and paste | Cmd+C writes the selection's map text, Cmd+V pastes at the pointer, Cmd+Alt+V where it was (`map.copy`, `map.paste`) | `desktop tests::the_map_view_copies_and_pastes_map_text`, `map-edit prefabs::tests::a_copy_pastes_in_place_or_at_a_point` | Same |
| Move with mouse or keys | `map.move`, `map.brush_move` | `map-edit brushes::tests::a_brush_moves_with_its_textures_locked` | Same |
| Rotate tool and rotate by 90 degrees | `map.rotate` | `map-edit brush_tools::tests::a_selection_turns_about_the_vertical`, `desktop tests::selected_brushes_turn_and_flip_from_properties` | Same: Alt+Left and Right turn the selection a right angle, Turn in Properties by any angle, 15 at first (`map.turn`) |
| Flip | Cmd+F across the view, Cmd+Alt+F up and down, Flip x, y, z in Properties (`map.flip`) | `map-edit brush_tools::tests::a_selection_flips_along_each_axis`, `desktop tests::the_map_view_takes_trenchbrooms_keys` | Same |
| Scale and shear | `map.brush_scale`, `map.brush_shear` | `map-edit brush_tools::tests::brushes_scale_and_shear` | Same |
| Delete | `map.delete`, `map.brush_delete` | `map-edit brushes::tests::brushes_are_merged_and_deleted` | Same |
| Extrude, stamp, move faces | `map.brush_extrude` | `map-edit brush_tools::tests::a_face_extrudes` | Gap G13: no stamping or moving faces |
| Clip points, snapping, matching plane | `map.brush_clip` | `map-edit brushes::tests::a_brush_is_clipped` | Gap G13: no snapping to vertices or matching a face's plane |
| Sweep | `map.brush_sweep` | `map-edit brush_tools::tests::a_brush_sweeps` | Same |
| Vertex, edge and face tools | `map.brush_vertex`, `map.brush_split_edge`, `map.brush_split_face`, `map.brush_chamfer` | `map-edit brush_tools::tests::vertices_move_and_split`, `an_edge_chamfers` | Beyond: chamfer |
| UV lock | Textures locked on move | `map-edit brushes::tests::a_brush_moves_with_its_textures_locked` | Same |
| CSG convex merge, subtraction, hollow, intersection | All four | as above | Same |
| Special brush and face types (clip, skip, hint, trigger) | Drawn as edges in the textured view; issues for misuse | `desktop map_shading::tests::the_textured_view_leaves_out_unseen_textures` | Same |
| Material browser | Textures tab with uses and filter | `map-edit textures::tests::every_texture_of_the_wad_and_the_map_is_listed_with_its_uses` | Same |
| Filter material collections | Filter by name | `desktop` texture browser tests | Gap G16: not by WAD |
| Projection modes (paraxial, parallel) | Standard and Valve 220 read and written | `map uv::tests` | Same |
| Material for new brushes | The map's last face's texture | `map-edit brushes::tests::a_box_is_created_on_the_grid` | Gap G16: not the browser's chosen texture |
| Assign materials | `map.brush_texture` | `map-edit` tests | Same |
| Replace materials | None | none | Gap G16 |
| Align, justify and fit | `map.face_uv`, `map.face_align` | `map-edit uv::tests::alignment_carries_across_the_corner` | Same |
| Face attribute editor | The UV editor's fields | `desktop map_uv` tests | Same |
| Alignment keyboard shortcuts | None | none | Gap G16 |
| UV editor | The UV editor | `desktop map_uv` tests | Same |
| Default entity properties | The keys the class reads, with their defaults | `map-edit links::tests::the_spawnflags_named_by_the_code_come_with_the_keys` | Beyond: from the QuakeC |
| Edit properties, several entities at once | `map.set_key` with `also` | `map-edit links::tests::a_key_is_set_on_several_entities_at_once` | Same |
| Smart property editors | Spawnflags, angle, colour, choices, targets | as above | Same |
| Linking entities | Pick and Alt-drag | `map-edit links::tests::linking_names_the_target_when_it_has_no_name` | Same |
| Link visualisation | Links | as above | Beyond |
| Undo collation and transactions | One undo step per command | `map-edit tests::every_entity_edit_undoes_and_the_map_saves` | Same |

## Keeping an overview

| TrenchBroom | Ours | Test | Verdict |
| --- | --- | --- | --- |
| Filtering and rendering options | Show by skill, coop and deathmatch from the QuakeC; shading Lines, Textured, Lit | `desktop tests::the_map_filter_is_found_again_after_an_edit` | Beyond: what the game spawns, and a lit view |
| Hide and isolate | `map.hide`, `map.isolate`, `map.show_all` | `map-edit groups::tests::a_selection_is_hidden_isolated_and_locked` | Same |
| Locking | Any selection, not only layers | as above | Beyond |
| Groups: create, open, close, ungroup | Read and moved; `map.move_to` | `map-edit containers::tests::a_selection_is_grouped_and_ungrouped`, `desktop tests::groups_and_layers_are_made_from_the_view` | Same: Cmd+G, Cmd+Shift+G, Group and Ungroup in Properties (`map.group`, `map.ungroup`) |
| Linked groups: create, separate, extract, protected properties | Edits copied, protected keys kept | `map-edit containers::tests::layers_are_added_and_groups_linked_and_separated`, `desktop tests::groups_and_layers_are_made_from_the_view` | Gap G17: create (Cmd+Shift+D) and separate made; extracting into a new linked group is not |
| Layers: create, hide, lock, omit from export, current layer | `map.set_layer`, `map.layers`, `map.export` | `map-edit groups::tests::a_locked_layer_refuses_edits_and_an_export_leaves_it_out`, `desktop tests::groups_and_layers_are_made_from_the_view` | Gap G17: Add layer made (`map.add_layer`); no current layer |

## Preferences

| TrenchBroom | Ours | Test | Verdict |
| --- | --- | --- | --- |
| Game configuration: game path | Install detection, the game folder setting | `qs-install` tests | Same |
| View layout and rendering | `view.map_panes`, shading | `qs-layout` tests | Same |
| Mouse input preferences | Look, pan and fly speeds, inverted look and wheel (`view.map_camera`) | `desktop tests::the_camera_settings_reach_the_3d_view` | Same |
| Keyboard shortcuts, rebindable | TrenchBroom's defaults | `desktop keymap` tests | Same: a keymap file rebinds |

## Advanced topics

| TrenchBroom | Ours | Test | Verdict |
| --- | --- | --- | --- |
| Automatic updates | None: the app makes no network request without a provider (P4-16) | `cargo xtask offline` | Decision for Justin: an update check breaks P4-16 |
| Command repetition | `edit.repeat` | as above | Same |
| Issue browser | `map.issues`, `map.fix` | `map-edit issues::tests` | Beyond |
| Compile profiles: export map, run tool, launch engine, copy, rename, delete | `bsp`, `vis`, `light`, `tool`, `copy`, `launch` | `desktop tests::a_profile_compiles_from_the_map_view` | Gap G20: no rename or delete task |
| Launching game engines | Play and Play from here in our engine | `qs-bench play_from_here_e1m1` | Beyond |
| Expression language in profiles and definitions | `${MAP}` and five more variables | `qs-workbench map_compile` tests | Same for profiles; definitions are QuakeC |
| Query language | `map.search` | as above | Gap G19 |
| Automatic backups | `map.backup` | as above | Same |
| Display models for entities from definitions | Models from the QuakeC's `setmodel` | `desktop map_scene` tests | Same: the model the QuakeC sets |
| Point and portal files | Leak trail and portals | as above | Same |
| Game configuration files | Not needed: the game is Quake and the classes come from the QuakeC | none | Not for Quake |

## Beyond TrenchBroom

What we do that the manual does not list:

- Entity classes, keys, spawnflags and links read from the mod's own QuakeC.
- A lit view of the compiled map with coloured light, and a light's reach in its colour.
- QuakeC editing in the same window, with the compiler's diagnostics, and a debugger on the
  running game.
- Play from here in our own engine, and the game following the map as it is edited.
- The agent, building and reviewing geometry with the same commands.
- Lock on any selection.

## Gaps

To close before #823 does:

| Gap | What TrenchBroom's manual has that we lack |
| --- | --- |
| G1 | Entity models drawn textured, not as wireframes |
| G7 | Two- and three-pane layouts, a highlight on the focused pane, linked 2D views |
| G8 | Selection drilling; 2D picking by candidates |
| G11 | Axis restriction while moving |
| G12 | The map view's context menu |
| G13 | Brush entities from selected brushes and back to the world; a convex brush from placed points; stamping and moving faces; clip snapping and matching a face's plane |
| G16 | Replace a texture; the browser's chosen texture for new brushes; filter by WAD; a WAD dropped on the window; alignment shortcuts |
| G17 | Extracting objects into a new linked group; the current layer new objects go into |
| G19 | TrenchBroom's query language in the search |
| G20 | Rename and delete tasks in compile profiles |

Automatic updates are left to Justin: an update check would make a network request, which P4-16
rules out without a provider.

## Benchmark

TrenchBroom's side, measured on the reference Mac on 2026-11-15, is on #823: 2.9 s from launch to
`lq-e2m5.map` loaded, 611 MiB resident, 119.9 FPS while orbiting (display-capped at 120 Hz).

Ours is measured the same way once the reference Mac's screen is unlocked: with it locked, the app
gets no window to draw in, so neither its load time nor its frame time is real.
