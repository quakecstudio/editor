# qs-sim

The preview host's server simulation (P3-26, #238): a map's world loaded from its BSP, its
entities spawned through the game's progs on our QuakeC VM, and server frames run as id's
server runs them. Collision, monster movement, physics and the player all follow the engine.

- **Layer:** L4 bridges. It depends on `qc-builtins`, `qc-edict` and `qc-vm` (L1),
  and `qc-progs`, `qc-types`, `qs-bsp` and `qs-mdl` (L0). Its tests also compile QuakeC with
  `qc-sema` and `qc-codegen`; its examples use `qs-vfs` and `clap`.
- **Spec sections:** WORLD-1 to WORLD-8 of [`world.md`](../../docs/spec/engine/world.md),
  MOVE-1 to MOVE-5 of [`move.md`](../../docs/spec/engine/move.md), PHYS-1 to PHYS-13 of
  [`physics.md`](../../docs/spec/engine/physics.md), WORLD-9 (the debugger), NET-1 to NET-5 of
  [`net.md`](../../docs/spec/engine/net.md), and the spawning of EDICT-8; the rerelease's
  localization (RRE-1) is `qc-builtins`', and its local sounds (RRE-2.1, RRE-2.4) and named builtins
  (RRE-4) are here.
- **Ported from:** id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`:
  `WinQuake/world.c`, `sv_move.c`, `sv_phys.c`, `sv_user.c`, `sv_main.c` (`SV_SpawnServer`
  and the message writers) and `host_cmd.c` (the signon commands, `Host_Changelevel_f`, `Host_Savegame_f`, `Host_Loadgame_f`), `cmd.c` (the command buffer, aliases and `Cmd_ForwardToServer`), `cvar.c` (`Cvar_Command`), with parts of
  `pr_cmds.c`, `pr_edict.c` (the edict prints), `pr_exec.c` (`PR_Profile_f`), `model.c`, `common.c`, `mathlib.c` and `view.c`. Each file's
  header lists its functions.
- **Shipped:** yes.

## Public shape

- `World::load(progs, map, bsp, files)`: the world's fields and globals, then the entities of
  the entity lump spawned for single player; `files` reads the other models the progs
  precaches.
- `World::settle(progs)`: the two settle frames and the entity baselines (EDICT-8.9,
  NET-2.1); a host calls it once after `load`.
- `World::connect(progs)`: the player enters the game (PHYS-10.1) and is sent the serverinfo
  and the signon sequence as one reliable message (NET-2).
- `World::set_command(cmd)` and `World::frame(progs, frametime)`: the player's `UserCmd`, then
  a server frame of that length: the command, then every edict's physics (PHYS-1, PHYS-9).
- `World::take_messages()`: the packets the server sent the client since the last call, in
  order: each frame's datagram of time, client data, entity updates and effects, and the
  reliable messages the builtins and the frame wrote (NET-1 to NET-5), byte for byte as id
  writes them. `messages.rs` holds the writers (`Message`) and the fat PVS.
- `World::entities()`, `World::player_view()`, `World::take_set_angle()`,
  `World::take_sounds()` and `World::time()`: what a client draws and plays without reading
  the messages, kept for the viewer until the client crate reads them.
- `World::set_standard_quake(standard)`: `-rogue` and `-hipnotic` send the weapon as a bit
  index (NET-4.4).
- `World::take_host_command()` and `World::save_spawn_parms(progs)`: the level `changelevel`
  asked for, and `SV_SaveSpawnparms`' `SpawnParms`, which `LoadOptions::carried` gives the
  next world (PHYS-10.4).
- `World::save_game(progs)`, `SaveGame::read(text)` and `World::restore(progs, save)`:
  `Host_Savegame_f`'s file, and `Host_Loadgame_f` reading it back over the map freshly spawned
  (PHYS-10.5).
- `World::console(text)`, `World::execute_console(progs)`, `World::take_console_output()` and
  `World::take_host_command()`: id's console (PHYS-13), run in each frame before the server
  frame as `_Host_Frame` runs `Cbuf_Execute`; what it printed, a line each; and the `map`,
  `changelevel`, `restart`, `save`, `load` or `quit` it left for the host (PHYS-13.6).
- `debug::Debugger` with `World::set_debugger` and `World::evaluate(progs, text)`: a frame
  stopped before a QuakeC statement, the steps and breakpoints that replay it from a clone of
  the world (WORLD-9.1 to WORLD-9.4), and watch expressions (WORLD-9.5).
- `examples/sweep.rs`: every map of an install run, saved, loaded and run again, the record
  of P3-24's acceptance (`--basedir <root> [--game <dir>] [--seconds <s>]`).
- `LoadOptions`, the host's part of `World::load`: the rerelease's keyed strings, parsed from
  `localization/loc_english.txt` by `localization::Localization::loc_load_file` (RRE-1.7),
  given to the mock engine before the map spawns; with a file loaded the print builtins and
  `WriteString` send their text localized (RRE-1.9), with none as id's engine sends it
  (RRE-1.6).
- `rerelease::patch_builtins`: QuakeSpasm's `PR_PatchRereleaseBuiltins` on a progs' bytes
  before it is read (RRE-4.1). It returns the `rerelease::BuiltinTable` for
  `LoadOptions::builtins`: the rerelease's, with its builtins 79 to 92 (RRE-4.2), when it
  patched any, else id's.
