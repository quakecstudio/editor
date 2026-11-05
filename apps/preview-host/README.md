# preview-host

The game in its own window, served to the IDE over the bridge (ADR-0004, P3-02). Layer: apps.

It loads a map from the user's install through the game filesystem (`qs-vfs`), runs the
server in `qs-sim`, reads the server's messages with `qs-client` and draws the scene and HUD
with `qs-render`. With `-bridge PATH` it waits for the IDE on that Unix socket and serves
`docs/spec/bridge-protocol.md` (BRIDGE-1 to BRIDGE-7) between two server frames.

```bash
cargo run --release -p preview-host -- -basedir <game root> [-game <dir>] [-kpf <QuakeEX.kpf>] \
    [-bridge <socket>] [map]
```

clap reads the command line. `-basedir`, `-game`, `-kpf` and `-bridge` are written with one
dash because ADR-0008 launches every engine, this one included, with `-basedir <run root>
-game <mod>`; the host gives those four words their second dash before clap reads them, so
`--basedir` works too, and every other option takes two. `-game` is a single directory name. `-kpf` reads the rerelease's localization from that
archive instead of the game root. The map defaults to `start`.

W, A, S and D move, Space jumps, Q and E swim, Shift runs, the mouse button fires, 1 to 8
change weapons, and a click in the window captures the mouse to look around (Escape frees it). `--fly` is a free camera, `--benchmark <seconds>`
measures the frame rate (P2-152), `--position <x> <y>` opens the window on a chosen display,
and `--help` lists every option. `--headless` (with `-bridge`) opens no window and no sound
device: it runs frames of 1/72 of a second and serves the bridge between them, leaving
`screenshot` out of its capabilities, for `cargo xtask bridge-client`.

The bridge offers `inspect`, `reload` (the progs loaded again over the running map, its globals
and fields migrated by name and type, WORLD-10), `pause`, `resume`, `step` (a frame, or a QuakeC statement, `in`,
`over` or `out`), `breakpoint/set`, `breakpoint/clear`, `evaluate`, `field/set`,
`entity/spawn`, `entity/edit`, `entity/remove`, `player/spawn_at`, `pick`, `screenshot` and
`console`, and sends `progs/loaded`, `map/loaded`, `edict/spawned`, `edict/changed`,
`edict/removed`, `qc/error`, `breakpoint/hit`, `state/changed` and `console/line`. The QuakeC
debugger stops a frame before a statement and goes on by replaying the frame from a copy of
the world taken before it (WORLD-9); `progs.lno` beside the progs gives it files and lines.
`console` runs id's console (PHYS-13): its commands, aliases and settings, the commands it
forwards to the server, and `map`, `changelevel`, `restart`, `save`, `load` and `quit`, which
the host carries out, saving to `<basedir>/<game>/<name>.sav`. The commands' conformance tests
are in `src/game/tests.rs`.
