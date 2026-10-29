# QuakeC for VS Code

Starts [`quakec-lsp`](../../apps/lsp/README.md) for `.qc` and `.qh` files, with a TextMate
grammar for comments, literals, keywords, types, directives and `$` frame commands. Names
are coloured by the server's semantic tokens. Not published to any marketplace until the
legal review (P4-22) signs off.

Needs Node 22 or later and pnpm for the build; the extension runs on VS Code 1.91 or later.

## Install from this directory

```sh
pnpm install
pnpm run build                         # dist/extension.cjs, one bundled file
code --extensionDevelopmentPath="$PWD" <your mod's QuakeC directory>
```

`--extensionDevelopmentPath` loads the extension into that window only. A `.vsix` to install
for good comes with publishing, after P4-22.

## Settings

| Setting              | Default         | Meaning                                                                  |
| -------------------- | --------------- | ------------------------------------------------------------------------ |
| `quakec.server.path` | `quakec-lsp`    | The server binary: a path, or a name on `PATH`                           |
| `quakec.dialect`     | unset (vanilla) | `vanilla`, `fteqcc` or `gmqcc`, for a project with no `quakec.toml`      |
| `quakec.target`      | unset           | `quake`, `quakespasm`, `ironwail`, `vkquake`, `rerelease`, `fte` or `dp` |
| `quakec.autoproto`   | `false`         | FTEQCC's `-Fautoproto`                                                   |

The three project settings are sent as `initializationOptions`; changing any setting starts
the server again. For the rerelease, set `quakec.dialect` to `fteqcc` and `quakec.target` to
`rerelease`.

## Checks and the recorded test

```sh
pnpm run check      # tsc --noEmit, oxlint, oxfmt --check
QUAKEC_RERELEASE=<rerelease dir> QUAKEC_LSP=<path to quakec-lsp> pnpm test
```

`pnpm test` downloads the current stable VS Code into `.vscode-test/`, opens the rerelease
with this extension and no others, sets the rerelease's dialect and target, and runs
`vscode.executeDefinitionProvider`, the provider behind Go to Definition (F12), on the
`T_Damage` call in `weapons.qc`. It prints and checks
`definition T_Damage weapons.qc:71:3: defs.qc:806:68`.
