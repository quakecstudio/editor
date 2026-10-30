# Attribution

Every asset in this repository (images, fonts, sounds and Quake formats such as `.mdl`, `.bsp`
and `.wad`: files that cannot carry an SPDX header) has one row below saying who made it, where
it came from and its license. `cargo xtask attribution` fails when an asset has no row, when a
row names a file that is not in the repository, when a path is listed twice, when the author or
source is empty, or when the license is not one of these SPDX identifiers: `GPL-3.0-or-later`,
`GPL-2.0-or-later`, `CC0-1.0`, `CC-BY-4.0`, `CC-BY-SA-4.0`, `MIT`, `Apache-2.0`, `OFL-1.1`,
`ISC`, or several of them joined with ` AND ` for a file whose parts differ. Adding another
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
| `apps/desktop/assets/fonts/IBMPlexSans-Regular.ttf` | IBM Corp. | IBM/plex release `@ibm/plex-sans@1.1.0`, `ibm-plex-sans.zip` (sha256 `fb365d910566e6d199cc2c15579a7dd9a267128e18431a394ed81f1970c69200`); license in `apps/desktop/assets/fonts/IBMPlexSans-OFL.txt` | OFL-1.1 |
| `apps/desktop/assets/fonts/IBMPlexSans-SemiBold.ttf` | IBM Corp. | IBM/plex release `@ibm/plex-sans@1.1.0`, `ibm-plex-sans.zip`; license in `apps/desktop/assets/fonts/IBMPlexSans-OFL.txt` | OFL-1.1 |
| `apps/desktop/assets/fonts/JetBrainsMono-Regular.ttf` | The JetBrains Mono Project Authors | JetBrains/JetBrainsMono release `v2.304`, `JetBrainsMono-2.304.zip` (sha256 `6f6376c6ed2960ea8a963cd7387ec9d76e3f629125bc33d1fdcd7eb7012f7bbf`); license in `apps/desktop/assets/fonts/JetBrainsMono-OFL.txt` | OFL-1.1 |
| `apps/desktop/assets/fonts/NotoSansSC-Regular.otf` | Adobe (the Noto CJK fonts, for Google) | notofonts/noto-cjk at `523d033d6cb47f4a80c58a35753646f5c3608a78` (tag `Sans2.004`), `Sans/SubsetOTF/SC/NotoSansSC-Regular.otf` (sha256 `faa6c9df652116dde789d351359f3d7e5d2285a2b2a1f04a2d7244df706d5ea9`); license in `apps/desktop/assets/fonts/NotoSansCJK-OFL.txt` | OFL-1.1 |
| `apps/desktop/assets/fonts/lucide.ttf` | Lucide Icons and Contributors; Cole Bemis for the icons derived from Feather | lucide-icons/lucide release `1.57.0`, `lucide-font-1.57.0.zip` (sha256 `297023825c613e950345aa2c1b941788538497b7be0fc88d5b0dca23f85bacee`); license in `apps/desktop/assets/fonts/Lucide-LICENSE.txt` | ISC AND MIT |
| `apps/desktop/snapshots/map-view.png` | QuakeC Studio contributors | Rendered by the app's map view test (`apps/desktop/src/map_view/tests.rs`) | GPL-3.0-or-later |
| `apps/desktop/snapshots/palette.png` | QuakeC Studio contributors | Rendered by the app's snapshot harness (`apps/desktop/src/snapshots.rs`) | GPL-3.0-or-later |
| `apps/desktop/snapshots/palette-prompt.png` | QuakeC Studio contributors | Rendered by the app's snapshot harness (`apps/desktop/src/snapshots.rs`) | GPL-3.0-or-later |
| `apps/desktop/snapshots/project.png` | QuakeC Studio contributors | Rendered by the app's snapshot harness (`apps/desktop/src/snapshots.rs`) | GPL-3.0-or-later |
| `apps/desktop/snapshots/shell.png` | QuakeC Studio contributors | Rendered by the app's snapshot harness (`apps/desktop/src/snapshots.rs`) | GPL-3.0-or-later |
| `apps/desktop/snapshots/shell-editor-only.png` | QuakeC Studio contributors | Rendered by the app's snapshot harness (`apps/desktop/src/snapshots.rs`) | GPL-3.0-or-later |
| `apps/desktop/snapshots/shell-left-hidden.png` | QuakeC Studio contributors | Rendered by the app's snapshot harness (`apps/desktop/src/snapshots.rs`) | GPL-3.0-or-later |
| `apps/desktop/snapshots/shell-resized.png` | QuakeC Studio contributors | Rendered by the app's snapshot harness (`apps/desktop/src/snapshots.rs`) | GPL-3.0-or-later |
