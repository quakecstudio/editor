# qc-entities

What a map's entities mean to a QuakeC module, read through `qc-sema`'s semantic model and its
intermediate layer:

- `EntityClass::of_module`, `EntityClass` and `Fgd` (`entities`, P5-03, MAP-1): the entity
  classes a module's spawn functions define (EDICT-8.5, EDICT-8.6), each with the keys it
  reads from `self`, their defaults and what its comment says of each, its spawnflags, its
  `/*QUAKED*/` comment, model and size
  (BUILTIN-9.2, BUILTIN-9.4); and their export as an FGD file. `cargo xtask entity-corpus`
  holds id 1.01's and the rerelease's to golden files.
- `EntitySchema::of_module`, `EntitySchema::check` and `MapDiagnostic` (`map_check`, P5-04,
  MAP-8): a map's entities checked against the progs as the engine reads them (EDICT-8.2,
  EDICT-8.5): classnames naming no function and keys naming no field, each with the closest
  name, values their field cannot read, and dangling targets and targetnames.

- **Layer:** L3 semantics. It depends on `qc-sema` for the module it reads, on `qc-progs` for
  the opcodes of the intermediate layer and on `qs-map` for the map.
- **Spec sections:** MAP-8 in [`map.md`](../../docs/spec/map.md), read with EDICT-8 in
  [`edicts.md`](../../docs/spec/edicts.md).
- **Shipped:** yes.
