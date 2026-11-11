# A Steam library for the Flatpak's install test

The release's Flatpak install test (`.github/workflows/release.yml`, P4-18) checks that Steam's
library is found from inside the sandbox. It makes a Steam folder at
`~/.local/share/Steam` holding LibreQuake as Quake, with these two files in its `steamapps`:
`libraryfolders.vdf`, which lists the folder as a library holding app 2310, Quake, with
`@STEAM@` replaced by the folder; and `appmanifest_2310.acf`, which names Quake's install
folder. They are what `qs-install` reads (FS-5).
