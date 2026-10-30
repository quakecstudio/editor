# qs-paths

The single paths module (CONV-16): where QuakeC Studio keeps its configuration, data, caches
and logs on macOS, Windows and Linux. No other crate joins a path onto the home directory.

- **Layer:** L0. It depends on nothing but `std`, so any crate may use it.
- **Decision:** native per-OS directories, [ADR-0011](../../docs/adr/0011-app-directories.md).
- **Spec sections:** none; it implements an ADR, not engine behaviour.
- **Shipped:** yes.

`qs_paths::current()` resolves the directories for the running OS; `qs_paths::resolve()` takes
the platform and an environment lookup, which is how the tests cover every OS on every host.

`qs_paths::store_dirs()` (and `current_store_dirs()`) give the folders where game stores and
launchers keep their files on each OS: the Steam client folders, GOG Galaxy's and the offline
installers' libraries, and the Heroic launcher's configuration. `qs-install` looks for Quake
there, so it never joins a path onto the home directory itself.

`qs_paths::font_dirs()` (and `current_font_dirs()`) give the folders where each OS keeps its
installed fonts: macOS's system, local and user font folders, Windows' `Fonts` folder and its
per-user one, and on Linux the `fonts` folder of each XDG data directory and `~/.fonts`. The
desktop app's font fallback reads them for a character its bundled fonts lack.
