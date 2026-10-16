# Your Quake game data

QuakeC Studio does not include Quake's game files, and never copies them: you need your own
copy of the game, and the IDE reads it where it is. This page says what it looks for, where the
stores put it on each operating system, and how to get it onto a Mac.

## What you need

| You have | Files | What you can do |
| --- | --- | --- |
| The original game (1996), registered | `id1/pak0.pak` and `id1/pak1.pak` | Everything: mods for the original game and its mission packs (`hipnotic`, `rogue`) |
| The shareware episode | `id1/pak0.pak` only | Episode 1 only; QuakeC Studio reports `id1/pak1.pak` as missing |
| The 2021 rerelease (Quake Enhanced) | `rerelease/id1/pak0.pak`, `QuakeEX.kpf`, and `hipnotic`, `rogue`, `dopa`, `mg1` | Mods for the rerelease; its `id1` has only its own `pak0.pak` |

Steam and GOG sell both the original game and the rerelease. Steam's install has the rerelease
in its `rerelease` folder; GOG's Quake Enhanced has the rerelease at the top (`QuakeEX.kpf`,
`id1`, `hipnotic`, `rogue`, `dopa`, `mg1`, `mg3`, `ctf`) and the original DOS game in its `dos`
folder (`dos/id1/PAK0.PAK`, `dos/id1/PAK1.PAK`, `dos/hipnotic`, `dos/rogue`), and QuakeC Studio
reports both. The Epic Games Store and Xbox Game Pass sell only the rerelease, so QuakeC Studio reports the original `id1/pak0.pak` and
`id1/pak1.pak` as missing for those installs. Names are matched in any case: Steam's original
install uses `Id1/PAK0.PAK`.

QuakeC Studio checks each file against its published size and MD5 checksum (the table is
`FS_KNOWN_FILES` in [`spec/filesystem.md`](spec/filesystem.md#fs-6-recognising-an-install)).
A file it does not recognise is still used if it is a valid PAK archive: the rerelease's files
change with every update, so an unrecognised rerelease file is normal. A file that is not a
valid archive is reported as damaged.

## Where the stores put it

QuakeC Studio looks in these places when it starts. If your copy is somewhere else, choose its
folder: any folder whose `id1` holds `pak0.pak` is accepted.

| Store | Windows | Linux | macOS |
| --- | --- | --- | --- |
| Steam | Every library listed in Steam's `config/libraryfolders.vdf`, found from the registry (`HKEY_CURRENT_USER\Software\Valve\Steam`, `SteamPath`); the game is in `steamapps/common/Quake` (the folder named by `steamapps/appmanifest_2310.acf`), with the rerelease in its `rerelease` folder | The same, from `~/.steam/steam`, `~/.local/share/Steam` or the Flatpak's `~/.var/app/com.valvesoftware.Steam` | Steam sells Quake for Windows only; see below. A download into `~/Library/Application Support/Steam` is found there |
| GOG | The registry entries GOG's installers write (`HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\GOG.com\Games\1435828198` for Quake, `...\1739637082` for Quake Enhanced, value `path`), and any folder in GOG Galaxy's `C:\Program Files (x86)\GOG Galaxy\Games` or the installers' `C:\GOG Games` holding `goggame-1435828198.info` or `goggame-1739637082.info` | Installed through the Heroic launcher: Heroic's `gog_store/installed.json` in `~/.config/heroic` or the Flatpak's `~/.var/app/com.heroicgameslauncher.hgl/config/heroic` | GOG sells Quake for Windows only; see below |
| Heroic on Windows | `%APPDATA%\heroic\gog_store\installed.json` | | |
| Epic, Game Pass | Choose the game's folder; it is the rerelease | | |

These are the places Ironwail and vkQuake read on Windows and Linux (Steam's registry key and
library files, GOG's registry keys), the Heroic launcher's own files, and GOG Galaxy's default
library folder. QuakeC Studio has not yet been checked against every store on a real machine;
until it has, if your install is not found, choose its folder.

## Getting the game onto a Mac

Both stores sell Quake for Windows only, but the game data is the same on every OS. Two ways to
get it, neither of which needs Windows:

**GOG, with innoextract.** Download the offline backup installer from your GOG library ("Quake"
for the original game, "Quake Enhanced" for the rerelease), not the GOG Galaxy installer. If it
comes in several parts, keep them in one folder. Then:

```bash
brew install innoextract
innoextract --output-dir ~/Games/quake-gog ~/Downloads/setup_quake_*.exe
```

innoextract writes the game into an `app` folder inside the output folder. Choose
`~/Games/quake-gog` (or its `app` folder) in QuakeC Studio.

**Steam, with steamcmd.** steamcmd is Valve's command-line Steam client. Make an empty folder
for the game, then:

```bash
brew install steamcmd
steamcmd
Steam> force_install_dir /Users/<you>/Games/quake-steam
Steam> login <your Steam user name>
Steam> @sSteamCmdForcePlatformType windows
Steam> app_update 2310 validate
Steam> quit
```

`2310` is Quake's Steam app id; the download includes the rerelease in its `rerelease` folder.
steamcmd needs an absolute path (not `~`). Choose the folder in QuakeC Studio; it recognises a
steamcmd download by its `steamapps/appmanifest_2310.acf`.

## What QuakeC Studio does with it

Nothing is written into your install, and nothing is copied out of it. To run your mod, the IDE
makes a run folder inside your project with a link to the install's `id1` (a symbolic link on
macOS and Linux, a junction on Windows, neither of which needs administrator rights) and a link
to your mod's build output, and starts the game there
([ADR-0008](adr/0008-game-directories.md)).
