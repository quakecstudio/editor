# Security policy

## Supported versions

QuakeC Studio has not had a release yet. Until it does, only `main` receives fixes. After the
first release, the latest release and `main` do.

## Reporting a vulnerability

Report vulnerabilities privately through GitHub's
[private vulnerability reporting](https://github.com/quakecstudio/editor/security/advisories/new)
for this repository. Do not open a public issue.

Include what is affected (the app, the compiler, the language server, the agent or MCP server,
the preview host), the version or commit, how to reproduce it and what an attacker gains. You
should get an acknowledgement within a week.

Areas where a report is especially useful:

- the compiler, language server or map compiler crashing or misbehaving on a crafted
  `.qc`, `progs.src`, `progs.dat`, `.map`, `.bsp`, PAK or WAD file;
- the agent or MCP server acting outside the project it was given, or leaking model-provider
  API keys;
- the preview host bridge accepting commands from anything other than the IDE.
