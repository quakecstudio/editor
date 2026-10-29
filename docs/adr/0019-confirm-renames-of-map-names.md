# ADR-0019: Ask the modder to confirm each edit of a rename that maps may depend on

- **Status:** Accepted
- **Date:** 2026-10-21
- **Decided by:** Justin Plock, 2026-10-21, choosing option C, "Ask per edit"
- **Issue:** [#70](https://github.com/quakecstudio/editor/issues/70)
- **Requirements:** LANG-7, GOAL-1

## Question

When a rename changes a function or field that a `.map` file may name, does the language server
make the rename, refuse it, or ask the modder to confirm the edits?

## Context

The language server (P2-14, [#70](https://github.com/quakecstudio/editor/issues/70)) checks a
rename by resolving the edited modules again and refusing any change of meaning. That check sees
only the QuakeC source. Map data binds names the source cannot show: the engine calls the function
named by an entity's `classname` (EDICT-8.6), sets the field named by each key (EDICT-8.2), and
reads a function-valued key by the function's name (EDICT-8.2, `docs/spec/edicts.md`). No file in
the project says which maps name what, so after renaming `monster_ogre` or `.wait`, every map that
uses the old name silently stops spawning or setting it.

Names the engine itself binds are a different case and are always refused, whatever is decided
here: system globals and fields, whose place before `end_sys_globals` or `end_sys_fields` the
header CRC checks (EDICT-2.2, EDICT-3.2), and the fields the engine reads by name (EDICT-3.4).

LSP 3.18 has a way to ask: a `ChangeAnnotation` with `needsConfirmation`, "A flag which indicates
that user confirmation is needed before applying the change" (`specs/lsp/3.18/types/textEdit.md`),
named by each `AnnotatedTextEdit`. An annotated edit goes only in a `TextDocumentEdit`, whose
support "is guarded by the client capability `workspace.workspaceEdit.changeAnnotationSupport`. If
a client doesn't signal the capability, servers shouldn't send `AnnotatedTextEdit` literals back
to the client" (`types/textDocumentEdit.md`). For a rename, `honorsChangeAnnotations` says
"Whether the client honors the change annotations in text edits and resource operations returned
via the rename request's workspace edit by, for example, presenting the workspace edit in the user
interface and asking for confirmation" (`language/rename.md`). The specification does not say what
a server should do instead when a client cannot confirm.

## Options

| Option | Consequences |
| --- | --- |
| A. Rename, and warn with `window/showMessage` | Every client gets the rename. The modder learns of the risk only after the edits are made, and a warning is easy to miss |
| B. Refuse every rename of a function or field | Safe, but the most common renames in QuakeC (functions and fields) need doing by hand, in every client |
| C. Ask per edit: annotate the edits with a change annotation that needs confirmation (chosen) | The modder sees and confirms the edits in a client that honors change annotations (VS Code does). A client that does not cannot ask, so for it the rename is refused, as option B |

Option A was what the first version of #553 did; it was not kept because the modder is told only
after the fact. Option B is kept for clients that cannot ask, and dropped for those that can,
since they can show the modder what will change.

## Decision

A rename of a function or field (`qc_ide::Caution`), other than a builtin, which the engine binds
by number or engine name (QC-13.3), is answered with `documentChanges`: every edit of the rename
is an `AnnotatedTextEdit` naming the one annotation `map-name`, whose `needsConfirmation` is true,
whose label names the renamed name, and whose description says that maps naming it are not
changed. Any other rename is answered with plain `changes`, unannotated. A client "honors" the
annotations when it sends `workspace.workspaceEdit.documentChanges` true,
`workspace.workspaceEdit.changeAnnotationSupport` and `textDocument.rename.honorsChangeAnnotations`
true. To any other client, since the specification is silent, the rename of such a name is refused
with `RequestFailed` and a message saying why and that the name can be renamed by hand: our
decision, the safe choice, so a rename is never made without the modder having been asked.

## Consequences

- `apps/lsp` (P2-14, #553) sends the annotated edits and refuses for clients that cannot confirm;
  `qc-ide` reports which renames need confirmation (`Rename::caution`) and refuses the engine's
  own names.
- The desktop editor, when it renames through `qc-ide` (P2-17), shows the same confirmation in its
  own interface.
- Neovim and other clients without `honorsChangeAnnotations` cannot rename functions and fields
  through the server.

## Links

- Issue [#70](https://github.com/quakecstudio/editor/issues/70) and pull request
  [#553](https://github.com/quakecstudio/editor/pull/553).
- Sources: `specs/lsp/3.18/types/textEdit.md`, `types/textDocumentEdit.md`,
  `types/workspaceEdit.md` and `language/rename.md`; `docs/spec/edicts.md` EDICT-2.2, EDICT-3.2,
  EDICT-3.4, EDICT-8.2 and EDICT-8.6.
