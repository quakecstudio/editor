# Attribution

Every asset in this repository (images, fonts, sounds and Quake formats such as `.mdl`, `.bsp`
and `.wad`: files that cannot carry an SPDX header) has one row below saying who made it, where
it came from and its license. `cargo xtask attribution` fails when an asset has no row, when a
row names a file that is not in the repository, when a path is listed twice, when the author or
source is empty, or when the license is not one of these SPDX identifiers: `GPL-3.0-or-later`,
`GPL-2.0-or-later`, `CC0-1.0`, `CC-BY-4.0`, `CC-BY-SA-4.0`, `MIT`, `Apache-2.0`. Adding another
license is a PR that changes this list and `ASSET_LICENSES` in `xtask/src/attribution.rs`,
after checking that it may be distributed with a GPL-3.0-or-later program.

Quake's game data (`pak0.pak`, `pak1.pak` and anything taken from them) is never committed and
never listed here.

Format: one row per file, the path in backticks relative to the repository root.

| Path | Author | Source | License |
| --- | --- | --- | --- |
| `docs/design/mockups/01-play-loop.png` | QuakeC Studio contributors | Mockup drawn for this project; source in `docs/design/mockups/source/` | GPL-3.0-or-later |
| `docs/design/mockups/02-paused-inspect.png` | QuakeC Studio contributors | Mockup drawn for this project; source in `docs/design/mockups/source/` | GPL-3.0-or-later |
| `docs/design/mockups/03-inline-edit.png` | QuakeC Studio contributors | Mockup drawn for this project; source in `docs/design/mockups/source/` | GPL-3.0-or-later |
| `docs/design/mockups/04-command-palette.png` | QuakeC Studio contributors | Mockup drawn for this project; source in `docs/design/mockups/source/` | GPL-3.0-or-later |
| `docs/design/mockups/05-map-editor.png` | QuakeC Studio contributors | Mockup drawn for this project; source in `docs/design/mockups/source/` | GPL-3.0-or-later |
| `docs/design/mockups/06-theme-and-chrome.png` | QuakeC Studio contributors | Mockup drawn for this project; source in `docs/design/mockups/source/` | GPL-3.0-or-later |
| `docs/design/mockups/source/assets/engine-scene.svg` | QuakeC Studio contributors | Drawing made for the mockups | GPL-3.0-or-later |
| `docs/design/mockups/source/assets/map-3d.svg` | QuakeC Studio contributors | Drawing made for the mockups | GPL-3.0-or-later |
| `docs/design/mockups/source/assets/map-top.svg` | QuakeC Studio contributors | Drawing made for the mockups | GPL-3.0-or-later |
