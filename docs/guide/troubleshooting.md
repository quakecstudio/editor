# Troubleshooting

## Play says it cannot find Quake

The **Your Quake files** guide opens when no game is found, or only part of one. It names
what the folder holds and lacks:

- **No Quake files were found.** Press **Choose folder** and pick the folder that holds `id1`.
  [Your Quake game data](../game-data.md) says where each store installs the game.
- **Only the shareware episode is here.** Play works on its maps; the rest of the game needs
  `id1/pak1.pak`, from the registered game.
- **Only the 2021 rerelease is here.** A mod for the original game needs the original game's
  `id1` folder. Steam's install has both; Epic and Game Pass have only the rerelease.

The folder you choose is kept in your settings (`game.folder`) and used the next time.

## The project opens with many errors

A mod written for FTEQCC opened in the vanilla dialect reports FTEQCC's syntax as errors. Add a
`quakec.toml` that names the dialect, or open it from the command line with
`--dialect fteqcc`. The starters already have one.

A warning W0402 says a file name's case differs from the file on disk. It builds on macOS and
Windows but not on Linux, so fix the name in `progs.src`.

## A save does not reach the game

- The toolbar says **The reload failed; see the Console**: the project did not build. The
  Problems tab lists the errors; the game keeps running the code it had.
- A bar says **Reloading would reset**: the change drops or retypes a field or global that
  holds a value. Choose **Reload anyway** or **Restart map**.
- Nothing happens: the game is not running. Reloading happens only while it plays.

## The agent does not start

- **The agent is not set up yet.**: set up a provider ([The agent](agent.md)).
- **No project open.**: open a project first; the agent works on the open one.
- An error from the provider, such as a wrong key or a model name it does not have, is shown
  under the task box. **Settings** opens the provider form again.

## Where the log is

The app writes a log, which stays on your computer:

| OS | Folder |
| --- | --- |
| macOS | `~/Library/Logs/QuakeC Studio` |
| Windows | `%LOCALAPPDATA%\QuakeC Studio\logs` |
| Linux | `~/.local/state/quakec-studio/logs` |

The file is `quakec-studio.log`. A crash writes `crash-<time>-<pid>.log` beside it. Set the
`log.filter` setting, or `RUST_LOG`, to `debug` for more detail.

## Reporting a problem

File an issue at <https://github.com/quakecstudio/editor/issues> with what you did, what
happened, and the log. Do not attach your game's files.
