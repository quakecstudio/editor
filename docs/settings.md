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
