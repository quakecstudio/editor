# qs-paths

The single paths module (CONV-16): where QuakeC Studio keeps its configuration, data, caches
and logs on macOS, Windows and Linux. No other crate joins a path onto the home directory.

- **Layer:** L0. It depends on nothing but `std`, so any crate may use it.
- **Decision:** native per-OS directories, [ADR-0011](../../docs/adr/0011-app-directories.md).
- **Spec sections:** none; it implements an ADR, not engine behaviour.
- **Shipped:** yes.

`qs_paths::current()` resolves the directories for the running OS; `qs_paths::resolve()` takes
the platform and an environment lookup, which is how the tests cover every OS on every host.
