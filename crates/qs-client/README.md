# qs-client

The preview host's client (P3-26, #240): the server's messages read into entity slots, the
client data, sounds and light styles; the entities relinked and interpolated each frame; the
dynamic lights, temporary entities and beams; the particle system; the view; and the scene
handed to the renderer. The server's messages come from `qs-sim`, byte for byte as id writes
them (NET), so the client knows exactly what id's client knows.

- **Layer:** L4 bridges. It depends on `qc-builtins` and `qc-vm` (L1) and `qs-mdl` (L0);
  its tests build packets with `qs-sim`'s writers.
- **Spec sections:** CLIENT-1 to CLIENT-8 of [`client.md`](../../docs/spec/engine/client.md),
  PART-1 to PART-3 of [`particles.md`](../../docs/spec/engine/particles.md) and VIEW-1 to
  VIEW-4 of [`view.md`](../../docs/spec/engine/view.md), and the rerelease's
  `svc_localsound` and `svc_achievement` (RRE-2.2, RRE-2.3, RRE-2.5); CON-1 to CON-5 of
  [`console.md`](../../docs/spec/engine/console.md) and MENU-1 to MENU-5 of
  [`menu.md`](../../docs/spec/engine/menu.md).
- **Ported from:** id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`:
  `WinQuake/cl_main.c`, `cl_parse.c`, `cl_tent.c`, `r_part.c`, `view.c` and `common.c`
  (the readers), `console.c`, `keys.c` and `menu.c`; and sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`:
  `Quake/cl_parse.c` (the lerp flags), `console.c` (`Con_DrawConsole`, CON-5) and `r_alias.c` (`R_SetupAliasFrame`,
  `R_SetupEntityTransform`), under ADR-0018. Each file's header lists its functions.
- **Shipped:** yes.

## Public shape

- `Client::new(game)`: a client with nothing connected; `Game::MissionPack` for `-rogue` and
  `-hipnotic` (CLIENT-3.2).
- `Client::frame(frametime, packets, models, rand)`: `CL_ReadFromServer` and what follows it
  in a frame: the clock advances, each packet is read (CLIENT-1.4), the entities are relinked
  (CLIENT-5, CLIENT-7), the temporary entities built (CLIENT-6.4), the lights decayed, the
  particles moved (PART-3.1) and the colour shifts updated (VIEW-4). It returns the `Event`s
  the host acts on: prints, sounds to start and stop, static sounds, the new map, the signon
  stages, the CD track. `models` describes each model the server names (`Models`); `rand` is
  the client's `rand ()` (`Random`).
- `Client::scene()`: the `Scene` the renderer draws: the visible entities with their pose and
  move blends, the statics, the eye and the weapon (VIEW-3), the blend over the view
  (VIEW-4.6), the lights and the particles.
- `Client::set_viewangles`, `set_contents`, `set_forwardmove`, `start_pitch_drift`,
  `stop_pitch_drift`, `set_lerp`, `set_view_settings`, `set_gravity`: what the host's input,
  world and settings feed in.
- `Console`: the console's text, notify lines, slide and input line (CON-1 to CON-5);
  `Console::key` edits the line and returns what to run, `draw` and `draw_quakespasm` place it.
- `Menu`: the main, single player, help and quit menus (MENU); `Menu::key` returns the key
  destination and the `MenuEvent`s (sounds, a new game, quit) the host acts on.
- `Reader`: `MSG_Read*` over a message's bytes.
- `Particles`, `ViewState`, `calc_roll`, `calc_bob`, `gamma_table`: the pieces on their own.

## What it is not, yet

The status bar, the scoreboards and the centre print (HUD) and the colormap translations
(CLIENT-1.6) are the next steps of #240; the renderer draws the scene.
