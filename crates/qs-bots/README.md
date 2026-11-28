# qs-bots

The bots the preview host plays coop and deathmatch with (L-87, ENG-18, ADR-0031). A bot is a
client slot with no connection (`qs-sim`'s `addbot`) whose moves come from here, so the mod's
own player QuakeC plays it: bots work with any mod, id1 and LibreQuake included, and the
server stays id-exact.

- **Layer:** L4 bridges (`layers.toml`). It depends on `qs-sim` and `qs-map-gen` (L4), and
  `qc-progs`, `qc-vm` and `qs-bsp` (L0 and L1); its tests compile maps with `qs-map` and
  `qs-map-bsp`.
- **Spec:** [`docs/spec/bots.md`](../../docs/spec/bots.md), BOT-2 to BOT-9, this project's
  design; BOT-1 is `qs-sim`'s.
- **Shipped:** yes, in the preview host (`qs-game`) and `scenario.run` (`qs-workbench`).

## Public shape

- `Bots::graph_of(bsp, deathmatch)`: the map's `Graph`, every place a player reaches from its
  spawns with the playability check's moves (BOT-2); seconds on a large map, so the game makes
  it on a thread of its own.
- `Bots::new(graph)`, `with_weapons` and `with_seed`, then `Bots::drive(world, progs,
  frametime)` before each server frame: a `Mind` for each bot the server brought in, none for
  each it dropped, and each bot's move from its mind (BOT-9.1). `Bots` is a
  `qs_sim::scenario::Driver`, so a scenario plays its bots.
- `Mind::decide(view, graph, weapons)`: one bot's move from what it sees (`View`): the foe it
  fights (BOT-3), the goal it needs most weighed by the route's length (`item_need`,
  `follow_need`, `choose`, BOT-4), its aim at its skill (`Aim`, BOT-5), the weapon it holds
  (`Weapons`, id's by default, BOT-6), its moves along the route (BOT-7), and fire pressed in
  turn to respawn (BOT-3.3). Each bot's random numbers are its own (`BotRng`, BOT-8).
