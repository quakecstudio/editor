# apps

The binaries: the desktop app, the CLI and the LSP server, named by ADR-0006. Each is a
workspace member (`apps/*`). Release builds use `panic = "unwind"` so the command guard can
catch a panic in one command and keep the session alive (PRIN-7, `docs/conventions.md`).
