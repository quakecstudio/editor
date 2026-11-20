# qs-map-edit

Map editing (P6-40): the open map and the map commands that need no QuakeC, moved out of
`qs-workbench` so the map editor stands on the map formats alone.

- **Layer:** L5 commands. It depends on `qs-commands` (L5), `qs-map-bsp` (L4, for the report a
  compile gives the map view), `qs-map`, `qs-bsp`, `qs-wad`, `qs-vfs` and `qs-diff` (L0),
  `image`, `schemars`, `serde`, `serde_json` and `tracing`. `layers.toml`'s `[forbidden]` table
  keeps it from depending directly on any `qc-` crate, `qs-workbench` or `qs-engine`, and
  `cargo xtask layers` checks it.
- **Decision:** plan requirement ARCH-1 and the P6-40 refactor (#941); not engine behaviour.
- **Spec sections:** its tests cite MAP-6, MAP-9 to MAP-13 and MAP-15
  ([`map.md`](../../docs/spec/map.md)), WAD-6 to WAD-8 and BSP-19.
- **Shipped:** yes.

## Prior art

TrenchBroom keeps the map document and its editing apart from the game's code: a game
configuration and the entity definitions are what it is told of the game. This crate does the
same through a seam: it is told the project's entity classes and the running game's state, and
never reads QuakeC.

## Public shape

- `MapStore` and `MapDocument`: the open map, read from a `.map` or a shipped `.bsp`'s entity
  lump (`map.open`), edited through `qs-map`'s lossless document so a saved map differs only
  where an edit changed it (MAP-7), and saved as a `.map`, a `.ent` beside a BSP, or the BSP
  with its entity lump rewritten (`map.save`).
- The map commands, registered by `register_map_edit`: entity edits (`map.select`,
  `map.place_entity`, `map.move_entity`, `map.rotate_entity`, `map.delete_entity`, `map.duplicate_entity`, `map.set_key`,
  `map.remove_key`), brushes and their tools (`map.brush_*`), shapes, layers and groups,
  links, prefabs, the outline, the issues list, staged edits for review, face UVs and texture
  alignment, the textures the map names, `texture.import`, backups (`backup`), and
  `map.resolve_disk_change` and `map.compare_with_disk` after another program wrote the map. `register_map_game`
  adds the commands only a context with a game offers.
- The seam (`HasMap`): a context gives the map store (`map_store`), the project
  (`MapProject`: its folder, an entity class, the targeting keys and a check of a map's
  entities) and the running game (`LiveMap`: whether it plays the map, taking a `LiveChange`,
  and its folders). `qs-workbench` answers it from the server QuakeC and the preview.
- `MapEditError`: why a map command failed; `qs-workbench` carries it unchanged.
- `testing`, with the `testing` feature: the map context and map fixtures the crate's tests
  use, which the workbench's map compile tests share.
