# qs-paths

The single paths module (CONV-16): where QuakeC Studio keeps its configuration, data, caches
and logs on macOS, Windows and Linux. No other crate joins a path onto the home directory.

- **Layer:** L0. It depends on nothing but `std` and `serde_json`, so any crate may use it.
- **Decision:** native per-OS directories, [ADR-0011](../../docs/adr/0011-app-directories.md).
- **Spec sections:** none; it implements an ADR, not engine behaviour.
- **Shipped:** yes.

`qs_paths::current()` resolves the directories for the running OS; `Platform::dirs()` takes
an environment lookup, which is how the tests cover every OS on every host.

`qs_paths::store_dirs()` (and `current_store_dirs()`) give the folders where game stores and
launchers keep their files on each OS: the Steam client folders, GOG Galaxy's and the offline
installers' libraries, and the Heroic launcher's configuration. `qs-install` looks for Quake
there, so it never joins a path onto the home directory itself.

`qs_paths::font_dirs()` (and `current_font_dirs()`) give the folders where each OS keeps its
installed fonts: macOS's system, local and user font folders, Windows' `Fonts` folder and its
per-user one, and on Linux the `fonts` folder of each XDG data directory and `~/.fonts`. The
desktop app's font fallback reads them for a character its bundled fonts lack.

`qs_paths::lexical()` works out a path's `.` and `..` by its text, and
`qs_paths::arguments` finds and makes absolute the paths a command's JSON arguments name (any
key `path` or ending in `_path`), for the agent's permissions and the MCP server's roots.

`AppDirs::app_socket()` is where the running app answers other processes' commands
(AGT-7), which `quakec-ide mcp --connect` reaches: `app.sock` in the cache directory on macOS
and Linux, and on Windows, which has no socket files, a named pipe named after the cache
directory, so each user has their own.
