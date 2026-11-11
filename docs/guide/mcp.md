# MCP: using QuakeC Studio from another agent

`quakec-ide mcp` serves the agent's tools over the Model Context Protocol, so Claude Code or any
other MCP client can build, test, play and edit your mod with the IDE's own commands. It is
the same set of tools the built-in agent has, and no more. [`docs/mcp.md`](../mcp.md) is the
full reference.

## With Claude Code

```bash
claude mcp add quakec -- quakec-ide mcp /path/to/your/mod
```

Add `--scope project` to share it with everyone who opens the repository; Claude Code then
writes it to the repository's `.mcp.json`. Add `--dialect fteqcc` for an FTEQCC mod with no
`quakec.toml`, and `--game <folder>` when the game is not where `quakec-ide` finds it.

## With another client

A client that starts a stdio server takes the same command:

```json
{
  "mcpServers": {
    "quakec": {
      "command": "quakec-ide",
      "args": ["mcp", "/path/to/your/mod"]
    }
  }
}
```

## Working in the open window

With `--connect`, the tools run in the QuakeC Studio window you have open instead of a session
of the server's own. An edit the agent stages then shows in the window's Proposed changes, a
build is the window's build, and a game it plays is the window's game. Open the app with the
project first.

```bash
claude mcp add quakec -- quakec-ide mcp /path/to/your/mod --connect
```

## What it may touch

The tools read and write only under the project, plus each `--read` and `--write` folder.
`--read-only` lets nothing build, edit or play. An edit is staged, as the built-in agent's
are, and reaches disk only when it is accepted and saved.
