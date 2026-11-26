# qs-update

The update check (P4-19, QA-11, PRIN-9): the app says that a later release exists and how to
get it with what installed this one. It never downloads or installs a release.

- **Layer:** L5 commands. It depends on `serde`, `serde_json`, `tracing` and `ureq`.
- **Spec sections:** none in `docs/spec/`; the request is listed in
  [`docs/privacy.md`](../../docs/privacy.md) and the feed in
  [`docs/releasing.md`](../../docs/releasing.md).
- **Shipped:** yes.

| Part | What it does |
| --- | --- |
| `Version` | A release's version, `YYYY.MM.N`, ordered as the releases were made |
| `Feed`, `Found::check` | The feed, `{"version", "url"}`, read through a `Fetch` once and compared with the version running; a feed whose page is not one of the app's release pages is refused |
| `Https` | The one request: a `GET` of `https://packages.quakecstudio.com/latest.json` with no query, no body and the `User-Agent` `quakec-studio`, given up after ten seconds and read to 4096 bytes at most; a redirect is not followed |
| `Install::channel` | How the app was installed, from files and the environment alone, with no process started |
| `Channel::command`, `Channel::download` | That install's own way to update: its package manager's command, or the release page |
| `State` | When the background check last had an answer and what it found, kept in `update.json` of the app's data directory, so it asks at most once in `CHECK_INTERVAL`, a day |

| Channel | Found by | Update |
| --- | --- | --- |
| `Homebrew` | macOS, `/opt/homebrew/Caskroom/quakec-studio` exists | `brew upgrade --cask quakec-studio` |
| `MacPackage` | macOS otherwise | download |
| `Msi` | Windows, the executable under `ProgramFiles` or `ProgramW6432`, compared without regard to case | `winget upgrade SmokeTurner.QuakeCStudio`, or download |
| `PortableZip` | Windows otherwise | download |
| `Flatpak` | `/.flatpak-info` exists | download |
| `AppImage` | `APPIMAGE` is set | download |
| `Deb` | the executable under `/usr` and not `/usr/local`, `/var/lib/dpkg/info/quakec-studio.list` exists | `sudo apt update && sudo apt install --only-upgrade quakec-studio` |
| `Rpm` | the executable under `/usr` and not `/usr/local`, `/var/lib/rpm` or `/usr/lib/sysimage/rpm` exists | `sudo dnf upgrade quakec-studio`, or `yum` where `/usr/bin/dnf` is missing |
| `Unpackaged` | none of them: a build run from where it was made | download |

The feed is `packaging/latest.json` with the release's version filled in, which the release
workflow writes to quakecstudio/packages on a tag with no `-`. The desktop app
(`apps/desktop/src/update.rs`) asks it when `help.check_for_updates` is run, and as it starts
only once `updates.check` is `daily`.

## Tests

`src/tests.rs`: the order of versions and what is not one; the feed the release writes, a newer,
the same and an older release, and a malformed feed; the request against a local server that
records it (one `GET`, the fixed `User-Agent`, nothing else sent), an unreachable feed and a
redirect, which is not followed; each
channel from a fixture file tree and environment; when a check is due, and the state kept
between runs.
