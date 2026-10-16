# ADR-0011: Use each OS's native application directories

- **Status:** Accepted
- **Date:** 2026-10-15
- **Decided by:** P0-01 implementer, under the plan's instruction to record the choice
- **Issue:** [#1](https://github.com/quakecstudio/editor/issues/1)
- **Requirements:** CONV-16

## Question

Does QuakeC Studio keep its configuration, data, caches and logs in XDG directories on every
platform, as vouch does, or in each operating system's native locations?

## Decision

**Native per OS.** The `qs-paths` crate is the only code that decides where files live, and it
resolves:

| Kind | macOS | Windows | Linux and other Unix |
| --- | --- | --- | --- |
| config | `~/Library/Application Support/QuakeC Studio` | `%APPDATA%\QuakeC Studio` | `$XDG_CONFIG_HOME/quakec-studio`, default `~/.config/quakec-studio` |
| data | `~/Library/Application Support/QuakeC Studio` | `%LOCALAPPDATA%\QuakeC Studio\data` | `$XDG_DATA_HOME/quakec-studio`, default `~/.local/share/quakec-studio` |
| cache | `~/Library/Caches/QuakeC Studio` | `%LOCALAPPDATA%\QuakeC Studio\cache` | `$XDG_CACHE_HOME/quakec-studio`, default `~/.cache/quakec-studio` |
| logs | `~/Library/Logs/QuakeC Studio` | `%LOCALAPPDATA%\QuakeC Studio\logs` | `$XDG_STATE_HOME/quakec-studio/logs`, default `~/.local/state/quakec-studio/logs` |

On Linux an empty or relative `XDG_*` variable is ignored, as the XDG Base Directory
specification requires. A missing, empty or relative `HOME`, `APPDATA` or `LOCALAPPDATA` is an
error, never a silent fallback to the working directory. The directory name follows the
product name; if ADR-0006 renames the product, the two constants in `qs-paths` change.

## Why

- **It is a desktop app.** vouch is a command-line tool, where XDG on macOS is common and
  expected. Mac users expect a GUI app's settings under `~/Library/Application Support`, its
  caches under `~/Library/Caches` (which backup tools and "clear caches" utilities know to
  skip) and its logs under `~/Library/Logs`, where Console.app shows them. Windows users expect
  `%APPDATA%`.
- **Roaming config, local data on Windows.** Settings are small and worth following the user
  between machines; indexes, checkpoints and caches are large and machine-specific, so they
  go under `%LOCALAPPDATA%`.
- **No dependency.** The rules are four environment variables per platform. `qs-paths`
  implements them as a pure function of the platform and an environment lookup, so every
  platform's rules are tested on every CI host, which a crate that asks the running OS
  (`dirs`, `directories`) cannot do.

## Alternatives considered

| Option | Why not |
| --- | --- |
| XDG everywhere, as in vouch | Unexpected for a Mac or Windows desktop app; Console.app, Time Machine exclusions and Windows roaming profiles would not treat the files as users expect |
| The `dirs` or `directories` crate | Same layout, but adds a dependency and can only be tested on the host OS. Windows' `SHGetKnownFolderPath` would also honour folder redirection that `%APPDATA%` already reflects |
| Native on macOS and Windows, but honour `XDG_*` variables there when set | Two sources of truth on one platform; a stray variable set for a command-line tool would move the app's settings |

## Consequences

- No crate builds a path from the home directory itself; it asks `qs-paths`
  (`docs/conventions.md`, CONV-16).
- Resolving a directory never creates it; the code that writes there creates it.
- The game-install directories (Steam, GOG, the user's Quake folder) are a different question,
  answered by ADR-0008.
