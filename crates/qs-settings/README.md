# qs-settings

QuakeC Studio's settings (QA-10): the user's settings file, each project's settings file, the
schema of both, and the commands that read and change them.

- **Layer:** L5 commands. It depends on `qs-commands`, `schemars`, `serde`, `serde_json` and
  `toml`.
- **Decision:** plan requirement QA-10, P2-22 (#78); not engine behaviour.
- **Spec sections:** none; it implements a plan requirement, not engine behaviour.
- **Shipped:** yes.

## Prior art

The model is **Zed's settings**, which VS Code shares: a setting has a default, the user's file
may set it, and a project's file may set it again for that project, which wins. Zed keeps the
user's in `~/.config/zed/settings.json` and a project's in `.zed/settings.json`; here they are
`settings.toml` in the app's config directory (`qs_paths::AppDirs::config`) and
`.quakec-studio/settings.toml` in the project's directory, in TOML like `quakec.toml`.

## Settings

Each setting is a `Setting`, with its key, default and summary. `docs/settings.md` is generated
from them by `qs_settings::reference`, and a test fails when it is out of date. A file holds only
what it sets; an unknown key or a value of the wrong type is an error, so a typo is not ignored.

## Commands

`qs_settings::register` adds four commands to a registry whose context implements
`HasSettings`:

| Id | What it does |
| --- | --- |
| `settings.list` | Every setting with its default, its value and where the value comes from. |
| `settings.get` | One setting's value and where it comes from. |
| `settings.set` | Sets a setting in the user's file, or with `"scope": "project"` the project's, and writes the file. |
| `settings.reset` | Removes a setting from the user's or the project's file and writes the file. |

`settings.set` and `settings.reset` are undoable when they change the file. The settings UI is
these commands, through the palette like every other command; the settings screen of the app
(L7) calls them too.
