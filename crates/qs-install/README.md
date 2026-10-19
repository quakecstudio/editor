# qs-install

Finds the player's Quake game data, checks it, and links it into a run root. `find_installs`
looks where Steam, GOG and the Heroic launcher put the game on the running OS, and
`examine_folder` accepts any folder the player chooses that holds `id1/pak0.pak`, recognising
steamcmd downloads and GOG installers extracted with innoextract (the way onto a Mac, where both
stores sell Quake for Windows only). Each `Install` reports its original and rerelease base
directories, their `id1` and their other game directories; GOG's Quake Enhanced has the
original game in its `dos` folder (FS-6.6). `Install::validate` identifies each
`pak0.pak` and `pak1.pak` by its published size and MD5 checksum and reports the editions
present (shareware, registered, rerelease) and what is missing. `link_dir` makes the directory
links of the run roots in [ADR-0008](../../docs/adr/0008-game-directories.md): a symbolic link
on macOS and Linux, a junction on Windows.

The game data is read where it is and never copied or written (LIC-4, PLAT-8). Where each store
puts the files, and how to get them onto a Mac, is [`docs/game-data.md`](../../docs/game-data.md).

- **Layer:** L0 formats. It depends on `qs-paths` for the stores' folders, `qs-vfs` for the
  host filesystem and `qs-pak` to read an archive no published checksum matches; `serde_json`
  reads Heroic's list of installed games, and on Windows the `junction` crate (MIT) makes
  junctions, so no `unsafe` code is needed. The Windows registry is read with `reg query`.
- **Spec sections:** FS-6 in [`filesystem.md`](../../docs/spec/filesystem.md).
- **Spec statements:** the layouts of FS-5.1 and the name matching of FS-3.7.
- **Shipped:** yes.

## Tests

Every store layout on every OS is tested on every host: the finder takes the platform, its
store folders, a filesystem and a registry, and the tests give it trees and registries held in
memory. The fixtures are directory layouts and small stand-in PAK archives; the checksum tests
build table rows from the stand-ins' own sizes and checksums, so no game data is needed or
committed. The directory-link tests run on the real filesystem of each OS in CI: symbolic links
on macOS and Linux, junctions on Windows, and on all three a broken link, a link to the wrong
folder, a missing install and a real folder in the way.

## Checking a real install

The `find` example searches this machine, or examines the folders given, and prints every
install it finds with what `validate` makes of it:

```bash
cargo run --release -p qs-install --example find [-- <folder>...]
```

On LibreQuake's `lite` release (the `librequake-lite` test asset), 2026-10-16, it reported a
`Folder` install with `id1/pak0.pak` (353 entries, 43,948,916 bytes, MD5
`a70be459469774794ce6bd1b44d5a213`) and `id1/pak1.pak` (60 entries, 5,856,792 bytes, MD5
`0f3af10286bce6dad742b84bee86ea68`), both unrecognised and no edition, as FS-6.3 and FS-6.4
expect for files that are not id's. macOS's `md5` gives the same two checksums. The runs on
real Steam and GOG installs are [#277](https://github.com/quakecstudio/editor/issues/277).
