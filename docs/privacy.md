# Privacy

QuakeC Studio works offline, with no account and no telemetry (PRIN-9). This page lists every
network request the app can make, and when it makes each one. With no model provider set up,
it makes none unless it is asked to: `cargo xtask offline` proves that in CI.

## No account, no telemetry

- There is no account and no sign-in. Nothing asks for an email address.
- There is no telemetry, no crash reporting and no analytics, and no update check unless you
  ask for one or turn it on ([below](#every-network-request)). The log
  (`qs-log`) is written to the app's own directory on disk and is never sent anywhere.
- Projects, settings, checkpoints and recent projects stay on disk. Settings are in
  `settings.toml` in the app's config directory and `.quakec-studio/settings.toml` in a
  project; checkpoints are git objects in the project.

## Every network request

Three crates can make a request, and all use one HTTP client, `ureq`; no other part of the app
links one (`cargo tree -i ureq`).

| When | What is requested | Crate |
| --- | --- | --- |
| The agent is asked something (a thread or an inline edit), once a provider is set up | The provider's API: `https://api.anthropic.com` for Anthropic, the OpenAI-compatible address entered (`https://api.openai.com/v1` by default), or `http://localhost:11434/v1` for a local Ollama, which stays on the computer. Each request carries the task, the project's context the agent was given or read, and the key. | `qs-models` |
| A project is made from a template (File, New Project, `project.new`, `quakec-ide new`) without a local copy | The template's files at their pinned commit from `https://raw.githubusercontent.com` (id-Software/Quake-Tools for id1, id-Software/quake-rerelease-qc for the rerelease), each checked against its pinned SHA-256. `--from <dir>` reads a local copy instead and makes no request. | `qs-templates` |
| Help, Check for Updates is chosen (`help.check_for_updates`, which the built-in agent may not run); and as the app starts, at most once a day, only once `updates.check = "daily"` is in the settings (it is `off` unless set) | One `GET` of `https://packages.quakecstudio.com/latest.json`, a file naming the latest release's version and its release page. The request has no query and no body, and its `User-Agent` is `quakec-studio` on every install and version: it says nothing of the version running, the system or the install. The app compares the versions itself, and downloads and installs nothing: it shows the command of the package manager that installed it, and its Download and Release Notes buttons open the release's page in the system's browser. | `qs-update` |

Nothing else goes over the network:

- The preview host, the game, the engine bridge, the debugger and the scenarios run on the
  computer, talking over a local socket (a Unix socket, or a named pipe on Windows).
- The MCP server (`quakec-ide mcp`) talks over stdin and stdout, and `--connect` uses the app's
  own local socket (`app.sock`). The language server talks over stdin and stdout.
- A model provider's key is kept in the system keychain (`qs_models::Keychain`), never in a file,
  and is sent only to that provider.
- A help popup's **Read more in the user guide** opens that section of
  `https://www.quakecstudio.com/docs/` in the system's browser, and only when clicked; the app
  hands the address to `open` (macOS), `explorer` (Windows) or `xdg-open` (Linux) and makes no
  request itself.

The build tools in `xtask/` fetch id's sources, the test corpus and the cached specifications
for development and CI. They are not part of the app.

## The check

`cargo xtask offline <id 1.01 codebase> [--game <dir>]` (Linux, in the Test Assets job) runs
each step in a network namespace with no network (`unshare -rn`), under `strace`, which records
every `connect`, `sendto` and `sendmsg`. A step fails if its trace has an internet address
(`AF_INET` or `AF_INET6`) or a name lookup through systemd-resolved. With no provider set up:

- **The control:** `quakec-ide new id1` with no local copy must fail, and its trace must show
  the request, which proves that the trace sees one.
- **The CLI:** `quakec-ide new id1 --from <codebase>`, then `build`, `check` and `test` of the
  mod, and an MCP session with `initialize`, `tools/list` and two tool calls.
- **The desktop app:** a headless session (`apps/desktop/examples/offline.rs`) that opens the
  mod and every file in it, builds, checks, runs the tests, searches, opens the command
  palette, and asks for an agent thread, which does not start. It starts as the app does,
  with no settings, so the update check asks nothing.

Run locally on 2026-11-11 over id 1.01 and LibreQuake:

```
ok    control: new id1 from GitHub: failed, 1 network calls
ok    new id1 from a local copy: succeeded, 0 network calls
ok    build: succeeded, 0 network calls
ok    check: succeeded, 0 network calls
ok    test: succeeded, 0 network calls
ok    mcp: succeeded, 0 network calls
ok    the desktop app's session: succeeded, 0 network calls
```
