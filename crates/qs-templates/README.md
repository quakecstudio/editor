# qs-templates

New-project templates (P4-17, QA-12, LIC-5): a starter mod that builds and plays at once, made
from id's own QuakeC, fetched under its GPLv2 terms rather than shipped with the app.

- **Layer:** L5 commands. It depends on `qs-commands`, `serde`, `schemars`, `sha2` and `ureq`.
- **Spec sections:** none in `docs/spec/`; the project file is PRJ (`docs/spec/project-file.md`)
  and the sample tests are QCT (`docs/spec/qc-tests.md`) and SCN (`docs/spec/scenario.md`).
- **Shipped:** yes.

| Template | Upstream | Dialect, target |
| --- | --- | --- |
| `id1` | `id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73`, `qcc/v101qc` | vanilla, quake |
| `rerelease` | `id-Software/quake-rerelease-qc@634eefab09a77eb7b5f5ca7078ba3d8784a91142`, `quakec` | fteqcc, rerelease |

Both upstreams are the ones `corpus/manifest.toml` pins, under GPL-2.0-or-later (each file's
header grants version 2 or any later version). `src/files.rs` lists `progs.src` and the files it
names, each with its SHA-256; `create` fetches every file ([`GitHub`] by the pinned commit, or
[`Folder`] from a local copy such as the corpus cache), checks each against its SHA-256 before
writing anything, and then adds:

- `quakec.toml` with the template's dialect and target;
- `.gitignore` excluding `build/`, which holds the build output and the run roots (ADR-0008);
- `tests.qc`, listed last in `progs.src`, with `test_anglemod_wraps`, and
  `tests/start.scenario`, which `project.test` runs (P4-26);
- `COPYING`, the GPLv2 text both upstreams carry (`src/COPYING-GPL-2.0.txt`, the same bytes as
  `qcc/COPYING` and `COPYING.txt`), and `NOTICE.md`, naming the upstream, its commit, its terms
  and what was added.

The command is `project.new` (`template`, `path`); the window's File menu and its no-project
panel offer New id1 Project and New Rerelease Project, and the command line has
`quakec-ide new <id1|rerelease> <dir> [--from <folder>]`.

## Tests

`src/tests.rs`: a project's files and what is added, against synthetic files; a file that is not
the pinned one, a failed fetch, a directory that is not empty or cannot name a mod; the tables
and the GPLv2 text. CI's Test Assets (linux) makes a project of each template from the corpus
copy, builds it, runs `quakec-ide test` over LibreQuake (both tests pass), and checks git tracks
nothing under `build/`.
