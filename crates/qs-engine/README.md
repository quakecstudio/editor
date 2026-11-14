# qs-engine

The IDE's client for the preview host, the only engine it runs (ADR-0005, P3-06): the run
roots ADR-0008 launches every engine in, finding and starting the host, and the bridge
protocol, [`bridge-protocol.md`](../../docs/spec/bridge-protocol.md), spoken to it.

- **Layer:** L4 bridges. It depends on `qs-bridge` for the protocol, `qs-install` for the
  installs and their links, and `interprocess` for the socket or named pipe.
- **Spec sections:** BRIDGE-1.1 (the socket), BRIDGE-1.2 to BRIDGE-1.4 through `qs-bridge`,
  BRIDGE-2 (`initialize`, the capabilities, `shutdown`), BRIDGE-3 (the events) and BRIDGE-4
  (the commands); the run roots are ADR-0008's.
- **Shipped:** yes.

## Public shape

- `prepare_run_root(project, flavour, base, mod)`: makes or repairs
  `<project>/build/run/original` or `rerelease`, holding `id1`, a directory link to the
  install's, and `<mod>`, a directory link to `<project>/build/<mod>`; the rerelease's also
  links `QuakeEX.kpf` on macOS and Linux and passes it with `-kpf` on Windows. Nothing is
  written inside the install. A `RunRoot` whose `game` is `None` plays `id1` alone.
- `locate_host(configured, app)`: the host at the path the user set, else beside the app;
  never downloaded.
- `LaunchSpec::command()` and `LaunchSpec::launch(name)`: the host started with `-basedir`, `-game`,
  `-kpf`, `-bridge`, the extra words and the map, and its bridge connected and initialized; a
  host that stops as it starts is `LaunchError::Exited`.
- `Client`: `initialize`, `offers`, and a method per command (`pause`, `resume`, `step`,
  `inspect`, `console`, `screenshot`, `set_field`, `pick`, `spawn`, `remove`,
  `edit_entity`, `spawn_player_at`, `reload`, `set_breakpoint`, `clear_breakpoint`, `evaluate`,
  `shutdown`); a command the
  host's capabilities leave out is refused before it is sent (BRIDGE-2.6). Events are kept
  in order for `take_events` and `wait_event`.

## Tests

`cargo test -p qs-engine` runs the client against a fake host over a real socket, and the
run roots on fixture installs, including a missing, broken or wrongly targeted link being
repaired. `cargo xtask bridge-client <game root>` runs the client against the real preview
host, `--headless`, launched through a run root; CI's Test Assets jobs run it on LibreQuake
on Linux, macOS and Windows.
