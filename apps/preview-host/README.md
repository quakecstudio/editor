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

`-basedir`, `-game`, `-kpf` and `-bridge` take one dash, as the engines and ADR-0008 write
them; `-game` is a single directory name. `-kpf` reads the rerelease's localization from that
archive instead of the game root. The map defaults to `start`.

W, A, S and D move, Space jumps, Q and E swim, Shift runs, the mouse button fires, 1 to 8
change weapons and dragging looks around. `--fly` is a free camera, `--benchmark <seconds>`
measures the frame rate (P2-152), `--position <x> <y>` opens the window on a chosen display,
and `--help` lists every option.

The bridge offers `inspect`, `pause`, `resume`, `step` (a whole frame), `field/set`,
`entity/spawn`, `entity/edit`, `entity/remove`, `player/spawn_at` and `console`, and sends
`progs/loaded`, `map/loaded`, `edict/spawned`, `edict/changed`, `edict/removed`, `qc/error`,
`state/changed` and `console/line`. The commands' conformance tests are in `src/game/tests.rs`.
