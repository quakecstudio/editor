# Cached public specifications

`specs/` holds the text of the public specifications QuakeC Studio implements, so that
anyone stating what a specification requires can quote it instead of remembering it
([`.claude/rules/specs-are-source-of-truth.md`](../.claude/rules/specs-are-source-of-truth.md),
CONV-12). Grep here before fetching from the web.

This is not where engine behaviour is described: that is [`docs/spec/`](../docs/spec/README.md),
written from id's source. Third-party source code (id's engine and tools, FTEQCC,
ericw-tools) is never cached here; builds fetch it by commit and checksum.

## What is cached

| Directory | Specification | Origin | License |
| --- | --- | --- | --- |
| `lsp/3.18/` | Language Server Protocol 3.18, every page and the meta model, plus in `includes/` the shared pages the specification includes that a server implements: the lifecycle messages, `window/showMessage` and the URI type | [microsoft/language-server-protocol](https://github.com/microsoft/language-server-protocol), branch `gh-pages`, `_specifications/lsp/3.18/`, and `_includes/messages/3.18/` and `_includes/types/` for `includes/` | CC-BY-4.0 |
| `mcp/2026-07-28/` | Model Context Protocol 2026-07-28, every specification page, plus `schema/schema.ts` and `schema/schema.json` | [modelcontextprotocol/modelcontextprotocol](https://github.com/modelcontextprotocol/modelcontextprotocol), branch `main`, `docs/specification/2026-07-28/` and `schema/2026-07-28/` | Apache-2.0 (specification), MIT for older contributions |
| `acp/v1/` | Agent Client Protocol v1 (stable), every protocol page, plus `schema/schema.json` and `schema/meta.json` | [agentclientprotocol/agent-client-protocol](https://github.com/agentclientprotocol/agent-client-protocol), branch `main`, `docs/protocol/v1/` and `schema/v1/` | Apache-2.0 |

Left out on purpose: generated `schema.mdx` pages (the schema files they render are cached),
images, examples, and draft versions. Add a draft only when an issue implements it.

## The manifest

[`manifest.tsv`](manifest.tsv) lists every cached file, one row each, tab-separated:

| Column | Meaning |
| --- | --- |
| `path` | The cached file, from the repository root |
| `spec` | The specification and version it belongs to |
| `url` | Where it was fetched from |
| `sha256` | SHA-256 of the origin bytes |
| `bytes` | Size of the origin bytes |
| `fetched` | UTC date of the fetch that last changed the file |

Every file is byte-identical to its origin, so `shasum -a 256 <path>` equals the `sha256`
column, and a quote from the file is a quote from the origin. The files carry their
upstream's license, not ours, so `cargo xtask licenses` does not scan the files the manifest
lists. `.gitattributes` marks `specs/` as binary-safe and the pre-commit hooks exclude it, so
nothing rewrites a byte.

## Refreshing

```bash
scripts/refresh-specs.sh           # fetch every row, rewrite the files and rows that changed
scripts/refresh-specs.sh --check   # report rows whose origin changed, write nothing
scripts/refresh-specs.sh --verify  # offline: every file matches its row and every file has a row
```

To cache a new file, append a row with `path`, `spec` and `url`, put `-` in the other
three columns, and run the script with no arguments. To move to a new version of a
specification, add the new version's rows, refresh, and remove the old version's rows and
files in the same PR once nothing cites them.

A refresh commits as one PR whose diff is the spec text that changed upstream; review it
for changes to anything the code relies on.
