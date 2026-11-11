# QuakeC Studio user guide

QuakeC Studio is an IDE for QuakeC, Quake's game code. It has its own compiler, it runs your
mod in the game as you edit, it has a debugger that stops the running game, and it has an
agent that can change your code and prove the change works. It runs on macOS, Windows and
Linux, offline, with no account.

Read these in order the first time:

1. [Getting started](getting-started.md): from nothing to your own mod running in Quake.
2. [Opening a mod](opening-a-mod.md): an existing mod, its `progs.src` and `quakec.toml`,
   and what the IDE shows of it.
3. [The play loop](play-loop.md): Play, edit, save, and see the change in the running game.
4. [Debugging](debugging.md): breakpoints, stepping, watches, and the entities of the running
   game.
5. [The agent](agent.md): asking the built-in agent to do a task, and reviewing what it did.
6. [MCP](mcp.md): giving the same tools to Claude Code or another agent.
7. [Troubleshooting](troubleshooting.md): when something does not work.

Reference pages:

- [Your Quake game data](../game-data.md): where QuakeC Studio finds the game's files.
- [Settings](../settings.md): every setting.
- [Commands](../commands.md): every command the palette, the CLI, the agent and MCP share.
- [Privacy](../privacy.md): every network request the app can make.

Keys are written for macOS. Off macOS, Cmd is Ctrl.
