# qs-sim

The preview host's server simulation (P3-26, #238): a map's world loaded from its BSP, its
entities spawned through the game's progs on our QuakeC VM, and server frames run as id's
server runs them. Collision, monster movement, physics and the player all follow the engine.

- **Layer:** L4 bridges. It depends on `qc-builtins`, `qc-edict` and `qc-vm` (L1),
  and `qc-progs`, `qc-types`, `qs-bsp` and `qs-mdl` (L0). Its tests also compile QuakeC with
  `qc-sema` and `qc-codegen`.
- **Spec sections:** WORLD-1 to WORLD-8 of [`world.md`](../../docs/spec/engine/world.md),
  MOVE-1 to MOVE-5 of [`move.md`](../../docs/spec/engine/move.md), PHYS-1 to PHYS-10 of
  [`physics.md`](../../docs/spec/engine/physics.md), and the spawning of EDICT-8.
- **Ported from:** id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`:
  `WinQuake/world.c`, `sv_move.c`, `sv_phys.c`, `sv_user.c` and `sv_main.c`
  (`SV_SpawnServer`), with parts of `pr_cmds.c`, `pr_edict.c`, `model.c`, `common.c`,
  `mathlib.c` and `view.c`. Each file's header lists its functions.
- **Shipped:** yes.

## Public shape

- `World::load(progs, map, bsp, files)`: the world's fields and globals, then the entities of
  the entity lump spawned for single player; `files` reads the other models the progs
  precaches.
- `World::connect(progs)`: the player enters the game (PHYS-10.1).
- `World::set_command(cmd)` and `World::frame(progs, frametime)`: the player's `UserCmd`, then
  a server frame of that length: the command, then every edict's physics (PHYS-1, PHYS-9).
- `World::entities()`, `World::player_view()`, `World::take_set_angle()`,
  `World::take_sounds()` and `World::time()`: what a client draws and plays.

## What it is not, yet

The message builtins keep only their lengths, so stats, centre prints and temporary entities
never reach a client. `SV_SetIdealPitch`, intermission, changing level and save games are not
ported. There is no oracle differential for physics or player movement: their behaviour is
specified from id's source and tested against those statements.

## Trying it

```text
cargo run -p qs-sim --example entities -- <maps/x.bsp> <progs.dat> [frames]
```

lists the models a map's entities spawn with. With `QS_PLAY` set, it connects a player and
prints its eye each frame.
