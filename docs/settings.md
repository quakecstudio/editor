# Settings

<!-- Generated from crates/qs-settings (`qs_settings::reference`); do not edit by hand. -->

The user's settings are in `settings.toml` in the app's config directory; a project's are
in `.quakec-studio/settings.toml` in the project's directory, and win over the user's.
Each file is a TOML table of the settings it sets: `appearance.theme = "light"`.
The `settings.list`, `settings.get`, `settings.set` and `settings.reset` commands read
and change them.

| Key | Default | What it does |
| --- | --- | --- |
| `appearance.theme` | `"dark"` | The colour theme: `dark` or `light`. |
| `log.filter` | `"info"` | Which events the log records, as `RUST_LOG` directives (`info`, `info,qc_sema=debug`); `RUST_LOG` wins when it is set. |
| `game.folder` | `null` | The folder Play reads the game from, in place, as the Quake files guide chose it; the first install found when unset. A folder that no longer holds a game is not used (P3-44). |
| `agent.profile` | `"write"` | The tools the agent is offered (P4-30): `ask` (reading only), `write` (reads and edits run, anything else asks first) or `full` (every tool, unasked). |
| `agent.tool_permissions` | `{}` | Rules for the agent's tool calls over the profile's (P4-30): a `default` of `allow`, `ask` or `deny`, and `tools.<command id>` with its own `default` and `allow`, `ask` and `deny` patterns (`*` and `?`, matched against the call's paths in the project). A `deny` pattern wins, then `ask`, then `allow`. |
| `agent.external` | `[]` | The external agents the agent panel can run over the Agent Client Protocol (P4-14): each a `name`, the `command` that starts it, its `args` and `env`. The panel's Use Claude Code adds Claude Code's. |
| `editor.key_mode` | `"standard"` | How the code editor reads keys (P4-37): `standard`, or `vim` for Vim's normal, insert and visual modes, motions, operators, counts, registers and `.`. |
| `updates.check` | `"off"` | Whether the app looks for a later release by itself (P4-19): `off`, when only Help, Check for Updates looks, or `daily`, when it also looks as it starts, at most once a day. Either way it only says how to update; it installs nothing. |
| `updates.skip_version` | `null` | The release the status bar does not announce, as Skip This Version chose: a version such as `2026.12.1`. A later release is announced again. |
