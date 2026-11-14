# Audit: the public API boundary of the L0 to L5 library crates

- **Issue:** #655 (`P2-154`)
- **Date:** 2026-11-02
- **Rules:** ARCH-1 (`docs/layers.md`) and [`docs/conventions.md`, "Type design (CONV-17)"](../conventions.md#type-design-conv-17)
- **Tool:** `cargo xtask api` lists every public item of each crate in `docs/api/<crate>.txt`;
  `cargo xtask api --check` in `cargo xtask ci` fails when a list differs from the source

Every library crate in layers `L0` to `L5` of `layers.toml` (32 crates; the `tests` and
`tooling` layers and `apps` excluded) had its public items listed by `cargo xtask api` and each
item's dependents found by name over every other workspace crate's source (`crates/*`,
`apps/*`, `xtask`, `fuzz`, their tests included). An item with a dependent is kept public. An
item with none is narrowed to `pub(crate)` unless a public signature names it, in which case
the compiler's `private_interfaces` lint keeps it public, or unless the crate is one whose
whole public API is a deliberate model for dependents that do not exist yet (the preview host,
ADR-0004; the map editor), in which case the crate's policy below keeps it. An item that only
the crate's own tests read is `pub(crate)` under `#[cfg(test)]`. Nothing listed here changes
behaviour: every test asserts what it did, and `cargo xtask ci` passes.

In all: 2193 public items at the roots after the audit, 100 narrowed or removed by it.

## How to read it

Each crate has a table of its public items at the root as `cargo xtask api` lists them after
this audit: the kind and path, the workspace crates that name the item, and the disposition.
A method is `fn Type::method`; a struct's public fields, an enum's variants and the traits a
type implements are listed in `docs/api/<crate>.txt` and take their type's disposition unless
"IDs, indices and counts" below says otherwise. The items that left a crate's root in this
audit follow its table.

## IDs, indices and counts in public fields

The acceptance criterion: no public field a dependent can use to break an invariant the crate
keeps, and no public constructor that forges an ID the crate hands out.

| Field or constructor | Disposition |
| --- | --- |
| `qc-edict` `qc_edict::system::SysVar.slot: usize` | kept public: a slot of the system table (EDICT-*), a constant of the spec, not a handle the store hands out |
| `qc-preproc` `qc_preproc::Expansion.parent: Option<ExpansionId>` | the struct is now `pub(crate)` |
| `qc-preproc` `qc_preproc::PpToken.expansion: Option<ExpansionId>` | kept public and read-only in effect: an `ExpansionId` has a private field and no public constructor, so a dependent can copy one from a token but not make one; the preprocessor never reads an id back from a dependent |
| `qc-preproc` `qc_preproc::source::FileSpan.file: FileId` | kept public: a `FileId` the dependent itself chose for `set_file`; the preprocessor resolves it through its own source map and reports a missing one |
| `qc-project` `qc_project::graph::GraphFile.parent: Option<GraphIndex>` | kept public and read-only in effect: a `GraphIndex` has a private field and no public constructor; dependents read graphs, never hand one back |
| `qc-sema` `qc_sema::expand::ModuleInput.compiled: Vec<FileId>` | as `FileSpan::file`: ids the dependent chose |
| `qc-sema` `qc_sema::expand::ModuleInput.names: BTreeMap<FileId...>` | as `FileSpan::file`: ids the dependent chose |
| `qc-sema` `qc_sema::expand::ModuleInput.prelude: Vec<FileId>` | as `FileSpan::file`: ids the dependent chose |
| `qc-sema` `qc_sema::resolve::FileDiagnostic.file: FileId` | as `FileSpan::file` |
| `qc-sema` `qc_sema::symbol::Definition.member_of: Option<DefId>` | as `Reference::def` |
| `qc-sema` `qc_sema::symbol::Definition.owner: Option<DefId>` | as `Reference::def` |
| `qc-sema` `qc_sema::symbol::Location.file: FileId` | as `FileSpan::file` |
| `qc-sema` `qc_sema::symbol::Reference.def: DefId` | kept public: a `DefId` has a private field and no public constructor; `ModuleIndex::def` answers `None` for an id outside its table |
| `qs-bsp` `qs_bsp::lit::FoundLit.path_id: PathId` | kept public: a `PathId` is an enumeration of the lit file paths (BSP-*), with no field to set |
| `qs-sim` `qs_sim::collision::Body.owner: Option<usize>` | kept public: an edict number or plane index into the simulation the dependent itself built; the simulation checks every index it reads (SIM-*), and the preview host reads these records |
| `qs-sim` `qs_sim::collision::hull::BrushModel.set: usize` | kept public: an edict number or plane index into the simulation the dependent itself built; the simulation checks every index it reads (SIM-*), and the preview host reads these records |
| `qs-sim` `qs_sim::collision::hull::ClipNode.plane: usize` | kept public: an edict number or plane index into the simulation the dependent itself built; the simulation checks every index it reads (SIM-*), and the preview host reads these records |
| `qs-sim` `qs_sim::collision::trace::Trace.ent: Option<usize>` | kept public: an edict number or plane index into the simulation the dependent itself built; the simulation checks every index it reads (SIM-*), and the preview host reads these records |
| `qs-sim` `qs_sim::physics::Sound.entity: usize` | kept public: an edict number or plane index into the simulation the dependent itself built; the simulation checks every index it reads (SIM-*), and the preview host reads these records |
| `qs-sim` `qs_sim::spawn::FieldDef.slot: usize` | kept public: an edict number or plane index into the simulation the dependent itself built; the simulation checks every index it reads (SIM-*), and the preview host reads these records |
| `qs-sim` `qs_sim::world::Entity.number: usize` | kept public: an edict number or plane index into the simulation the dependent itself built; the simulation checks every index it reads (SIM-*), and the preview host reads these records |
| `qs-workbench` `qs_workbench::TreeEntry.depth: usize` | kept public: a byte offset or count the command carries, checked by the workbench where it reads it |
| `qs-workbench` `qs_workbench::commands::AtParams.offset: usize` | kept public: a byte offset or count the command carries, checked by the workbench where it reads it |
| `qs-workbench` `qs_workbench::commands::OpenFileParams.offset: Option<usize>` | kept public: a byte offset or count the command carries, checked by the workbench where it reads it |
| `qs-workbench` `qs_workbench::commands::Place.end: usize` | kept public: a byte offset or count the command carries, checked by the workbench where it reads it |
| `qs-workbench` `qs_workbench::commands::Place.start: usize` | kept public: a byte offset or count the command carries, checked by the workbench where it reads it |
| `qs-workbench` `qs_workbench::commands::ProjectOpened.files: usize` | kept public: a byte offset or count the command carries, checked by the workbench where it reads it |
| `qs-workbench` `qs_workbench::commands::RenameParams.offset: usize` | kept public: a byte offset or count the command carries, checked by the workbench where it reads it |
| `qc_edict::EdictNumber::new(index: usize)` | kept public: an edict number is an index into the store the dependent itself built from the progs, and every store method answers `None` or an error for a number outside it (EDICT-*); qc-builtins, qc-vm, qs-sim and the differential make them from the engine's own counts |
| `qc_sema::FileId::new`, `qc_preproc::source::FileId` | kept public: the file id is a key the dependent chooses when it gives the `Db` a file (`set_file`), not an id the crate hands out; a lookup of an unknown id answers `None` |
| `qs_commands::CommandId::new` | kept public: an id is a name the dependent registers, validated by `IdError` |
| `qc_lexer::LineIndex::new` | kept public: built from the dependent's own text |
| `qs_bsp::LumpId`, `qs_bsp::lit::PathId` | kept public: enumerations of the format, with no field to set |
| `qc_sema::DefId`, `qc_sema::ModuleIndex`, `qc_preproc::ExpansionId`, `qc_project::GraphIndex` | kept public with private fields and no public constructor (P2-142 for qc-sema; this audit checked the rest) |

## Crate policies

| Crate | Policy for an item no dependent names |
| --- | --- |
| `qc-builtins` (L1) | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `qc-diag` (L0) | kept public: the diagnostic vocabulary every language crate and the IDE share |
| `qc-edict` (L1) | kept public: the edict store the VM and the preview host share (EDICT-*) |
| `qc-lno` (L0) | kept public: the .lno format's model (LNO-*), read by the VM and the debugger to come |
| `qc-progs` (L0) | kept public: the progs.dat format's model, every record and named constant of the spec (PROGS-*), read by the VM, the compiler and the preview host |
| `qc-trace` (L1) | kept public: the trace format's codec (TRACE-*) the oracles and the differential exchange |
| `qc-types` (L0) | kept public: the progs value vocabulary (TYPE-*) every engine crate and the compiler share |
| `qc-vm` (L1) | kept public: the VM the preview host and the differentials run (VM-*) |
| `qs-bsp` (L0) | kept public: the BSP format's model (BSP-*), read by the preview host's renderer and the map editor (ADR-0004) |
| `qs-install` (L0) | kept public: the install detection the app's first run drives (INST-*) |
| `qs-log` (L0) | kept public: the logging policy the app and the LSP server install (LOG-*) |
| `qs-map` (L0) | kept public: the .map format's model (MAP-*), read by the map editor and compiler to come |
| `qs-mdl` (L0) | kept public: the MDL format's model (MDL-*), read by the preview host's renderer (ADR-0004) |
| `qs-pak` (L0) | kept public: the PAK format's model (PAK-*), read by the game filesystem and the preview host |
| `qs-paths` (L0) | kept public: the app directories every binary resolves (PATHS-*) |
| `qs-porting-example` (L1) | kept public: the worked porting example docs/porting.md walks through |
| `qs-sim` (L4) | kept public: the engine model the preview host runs (SIM-*, ADR-0004) |
| `qs-vfs` (L0) | kept public: the game filesystem the preview host and the app read (VFS-*) |
| `qs-wad` (L0) | kept public: the WAD2 format's model (WAD-*), read by the preview host and the map editor |
| `qs-wav` (L0) | kept public: the WAV reader (WAV-*), read by the preview host's sound |
| the language crates `qc-lexer`, `qc-preproc`, `qc-syntax`, `qc-project`, `qc-sema`, `qc-engines`, `qc-ide`, `qc-codegen` and the L5 crates `qs-commands`, `qs-layout`, `qs-settings`, `qs-workbench` | narrowed to `pub(crate)`, or `pub(crate)` under `cfg(test)` when only the crate's tests read it; kept public only where a public signature names it |


## `qc-builtins` (L1)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qc_builtins::mock::BUILTIN_CHECKCLIENT_INTERVAL` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `const qc_builtins::mock::BUILTIN_CMD_TEXT_SIZE` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `const qc_builtins::mock::BUILTIN_FL_FLY` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `const qc_builtins::mock::BUILTIN_FL_NOTARGET` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `const qc_builtins::mock::BUILTIN_FL_ONGROUND` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `const qc_builtins::mock::BUILTIN_FL_SWIM` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `const qc_builtins::mock::BUILTIN_SIGNON_SIZE` | `xtask` | kept public: used by `xtask` |
| `const qc_builtins::mock::DIFF_RAND_INCREMENT` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `const qc_builtins::mock::DIFF_RAND_MULTIPLIER` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `const qc_builtins::mock::LIMIT_MAX_DATAGRAM` | `qc-progs`, `xtask` | kept public: used by `qc-progs`, `xtask` |
| `const qc_builtins::mock::LIMIT_MAX_LIGHTSTYLES` | `qc-progs` | kept public: used by `qc-progs` |
| `const qc_builtins::mock::LIMIT_MAX_MODELS` | `qc-progs` | kept public: used by `qc-progs` |
| `const qc_builtins::mock::LIMIT_MAX_MSGLEN` | `qc-progs` | kept public: used by `qc-progs` |
| `const qc_builtins::mock::LIMIT_MAX_SOUNDS` | `qc-progs` | kept public: used by `qc-progs` |
| `const qc_builtins::mock::VKQUAKE_MAX_MODELS` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `const qc_builtins::mock::VKQUAKE_MAX_SOUNDS` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `const qc_builtins::table::BUILTINS` | `qc-engines`, `qs-sim` | kept public: used by `qc-engines`, `qs-sim` |
| `const qc_builtins::table::BUILTIN_MAX_NUMBER` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `enum qc_builtins::error::BuiltinError` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `enum qc_builtins::error::FatalError` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `enum qc_builtins::error::RuntimeError` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `enum qc_builtins::mock::EngineProfile` | `qs-differential` | kept public: used by `qs-differential` |
| `enum qc_builtins::mock::MessageDest` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `enum qc_builtins::mock::Precache` | `qc-codegen`, `xtask` | kept public: used by `qc-codegen`, `xtask` |
| `enum qc_builtins::table::Class` | `qc-preproc`, `qc-sema`, `qc-syntax`, `qc-diag`, `qc-lexer`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qc-syntax`, `qc-diag`, `qc-lexer`, `xtask` |
| `fn qc_builtins::dispatch::MockBuiltins::engine` | `qs-differential`, `qs-sim` | kept public: used by `qs-differential`, `qs-sim` |
| `fn qc_builtins::dispatch::MockBuiltins::engine_mut` | `qs-differential`, `qs-sim` | kept public: used by `qs-differential`, `qs-sim` |
| `fn qc_builtins::dispatch::MockBuiltins::with_engine` | `qs-differential`, `qs-sim` | kept public: used by `qs-differential`, `qs-sim` |
| `fn qc_builtins::error::BuiltinError::message` | `qc-preproc`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-bsp`, `qs-differential`, `qs-render`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm` | kept public: used by `qc-preproc`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-bsp`, `qs-differential`, `qs-render`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm` |
| `fn qc_builtins::math::anglemod` | `qc-sema`, `qs-render`, `qc-engines`, `qs-sim`, `xtask` | kept public: used by `qc-sema`, `qs-render`, `qc-engines`, `qs-sim`, `xtask` |
| `fn qc_builtins::math::ceil` | `qc-sema`, `qs-bsp`, `qs-editor-view`, `qc-engines`, `xtask` | kept public: used by `qc-sema`, `qs-bsp`, `qs-editor-view`, `qc-engines`, `xtask` |
| `fn qc_builtins::math::fabs` | `qc-syntax`, `qc-engines`, `qs-tests`, `xtask` | kept public: used by `qc-syntax`, `qc-engines`, `qs-tests`, `xtask` |
| `fn qc_builtins::math::floor` | `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-bsp`, `qs-render`, `qs-map`, `qc-engines`, `qs-sim`, `desktop`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-bsp`, `qs-render`, `qs-map`, `qc-engines`, `qs-sim`, `desktop`, `xtask` |
| `fn qc_builtins::math::ftos` | `qc-sema`, `qs-differential`, `qc-engines`, `qc-vm`, `xtask` | kept public: used by `qc-sema`, `qs-differential`, `qc-engines`, `qc-vm`, `xtask` |
| `fn qc_builtins::math::makevectors` | `qc-syntax`, `qc-engines`, `qs-tests`, `qc-vm` | kept public: used by `qc-syntax`, `qc-engines`, `qs-tests`, `qc-vm` |
| `fn qc_builtins::math::normalize` | `qc-ide`, `qc-preproc`, `qc-sema`, `qs-workbench`, `qs-editor-view`, `qc-engines`, `qs-sim`, `qc-project`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qs-workbench`, `qs-editor-view`, `qc-engines`, `qs-sim`, `qc-project`, `lsp`, `xtask` |
| `fn qc_builtins::math::random` | `qc-codegen`, `qc-sema`, `qs-differential`, `qc-progs`, `qc-engines`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-codegen`, `qc-sema`, `qs-differential`, `qc-progs`, `qc-engines`, `qs-sim`, `xtask`, `fuzz` |
| `fn qc_builtins::math::rint` | `qc-syntax`, `qc-engines`, `desktop`, `xtask` | kept public: used by `qc-syntax`, `qc-engines`, `desktop`, `xtask` |
| `fn qc_builtins::math::vectoangles` | `qc-engines` | kept public: used by `qc-engines` |
| `fn qc_builtins::math::vectoyaw` | `qc-engines` | kept public: used by `qc-engines` |
| `fn qc_builtins::math::vlen` | `qc-engines`, `xtask` | kept public: used by `qc-engines`, `xtask` |
| `fn qc_builtins::math::vtos` | `qs-differential`, `qc-engines`, `qc-vm`, `xtask` | kept public: used by `qs-differential`, `qc-engines`, `qc-vm`, `xtask` |
| `fn qc_builtins::mock::Cvars::create` | `qs-log`, `qs-differential`, `qs-sim` | kept public: used by `qs-log`, `qs-differential`, `qs-sim` |
| `fn qc_builtins::mock::Cvars::set` | `qc-preproc`, `qc-sema`, `qs-vfs`, `qs-workbench`, `qs-differential`, `qs-map`, `qs-tests`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qs-vfs`, `qs-workbench`, `qs-differential`, `qs-map`, `qs-tests`, `xtask` |
| `fn qc_builtins::mock::Cvars::text` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-differential`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-differential`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_builtins::mock::EngineProfile::builtin_limit` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `fn qc_builtins::mock::EngineProfile::precache_limit` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `fn qc_builtins::mock::EngineStrings::make` | `qs-differential`, `qs-sim` | kept public: used by `qs-differential`, `qs-sim` |
| `fn qc_builtins::mock::EngineStrings::set_temp` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `fn qc_builtins::mock::EngineStrings::take_temp_change` | `qs-differential` | kept public: used by `qs-differential` |
| `fn qc_builtins::mock::EngineStrings::text` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-differential`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-differential`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_builtins::mock::EngineStrings::unzone` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `fn qc_builtins::mock::EngineStrings::zone` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `fn qc_builtins::mock::MockEngine::append_command` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `fn qc_builtins::mock::MockEngine::append_message` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `fn qc_builtins::mock::MockEngine::client_active` | `qs-differential` | kept public: used by `qs-differential` |
| `fn qc_builtins::mock::MockEngine::model_bounds_for` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `fn qc_builtins::mock::MockEngine::new` | `qs-differential`, `qs-sim` | kept public: used by `qs-differential`, `qs-sim` |
| `fn qc_builtins::mock::MockEngine::precache` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `fn qc_builtins::mock::MockEngine::precache_index` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `fn qc_builtins::mock::MockEngine::set_lightstyle` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `fn qc_builtins::mock::Rng::new` | `qs-differential`, `qs-sim`, `fuzz` | kept public: used by `qs-differential`, `qs-sim`, `fuzz` |
| `fn qc_builtins::mock::Rng::next_rand` | `qs-sim` | kept public: used by `qs-sim` |
| `fn qc_builtins::table::lookup` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-pak`, `qc-edict`, `qc-lexer`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-sim`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-pak`, `qc-edict`, `qc-lexer`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-sim`, `xtask` |
| `mod qc_builtins::math` | `qc-preproc`, `qc-sema`, `qs-sim`, `qc-vm` | kept public: used by `qc-preproc`, `qc-sema`, `qs-sim`, `qc-vm` |
| `mod qc_builtins::mock` | `qs-differential`, `qs-sim`, `qc-vm`, `xtask` | kept public: used by `qs-differential`, `qs-sim`, `qc-vm`, `xtask` |
| `mod qc_builtins::table` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qs-map`, `qc-progs`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `qc-vm`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qs-map`, `qc-progs`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `qc-vm`, `xtask` |
| `struct qc_builtins::dispatch::MockBuiltins` | `qs-differential`, `qs-tests`, `qs-sim` | kept public: used by `qs-differential`, `qs-tests`, `qs-sim` |
| `struct qc_builtins::mock::Buffers` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `struct qc_builtins::mock::CheckClient` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `struct qc_builtins::mock::Cvars` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `struct qc_builtins::mock::EngineStrings` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `struct qc_builtins::mock::MockEngine` | `qs-differential`, `qs-sim` | kept public: used by `qs-differential`, `qs-sim` |
| `struct qc_builtins::mock::ModelBounds` | `qs-differential`, `qs-sim` | kept public: used by `qs-differential`, `qs-sim` |
| `struct qc_builtins::mock::Overflow` | `qc-preproc` | kept public: used by `qc-preproc` |
| `struct qc_builtins::mock::Rng` | `qs-differential`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qs-differential`, `qs-sim`, `xtask`, `fuzz` |
| `struct qc_builtins::table::Builtin` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qc-engines`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qc-engines`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask` |
| `use qc_builtins::BUILTIN_MAX_NUMBER = table::BUILTIN_MAX_NUMBER` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `use qc_builtins::Builtin = table::Builtin` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qc-engines`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qc-engines`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask` |
| `use qc_builtins::BuiltinError = error::BuiltinError` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `use qc_builtins::Class = table::Class` | `qc-preproc`, `qc-sema`, `qc-syntax`, `qc-diag`, `qc-lexer`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qc-syntax`, `qc-diag`, `qc-lexer`, `xtask` |
| `use qc_builtins::EngineProfile = mock::EngineProfile` | `qs-differential` | kept public: used by `qs-differential` |
| `use qc_builtins::FatalError = error::FatalError` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `use qc_builtins::MockBuiltins = dispatch::MockBuiltins` | `qs-differential`, `qs-tests`, `qs-sim` | kept public: used by `qs-differential`, `qs-tests`, `qs-sim` |
| `use qc_builtins::MockEngine = mock::MockEngine` | `qs-differential`, `qs-sim` | kept public: used by `qs-differential`, `qs-sim` |
| `use qc_builtins::RuntimeError = error::RuntimeError` | none | kept public: the builtins and the mock engine the differentials drive (BUILTIN-*) |
| `use qc_builtins::lookup = table::lookup` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-pak`, `qc-edict`, `qc-lexer`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-sim`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-pak`, `qc-edict`, `qc-lexer`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-sim`, `xtask` |

Items that left the root (4): `const qc_builtins::math::BUILTIN_RANDOM_MASK`, `const qc_builtins::table::BUILTIN_FIXME_MESSAGE`, `fn qc_builtins::math::q_atof`, `fn qc_builtins::table::is_fixme`. Each is `pub(crate)` now, under `#[cfg(test)]` where only the crate's tests read it; `qc_sema::nav::ModuleIndex::references_at` is removed.

## `qc-codegen` (L4)

| Item | Used by | Disposition |
| --- | --- | --- |
| `enum qc_codegen::CompileError` | `xtask` | kept public: used by `xtask` |
| `enum qc_codegen::Internal` | `xtask` | kept public: used by `xtask` |
| `fn qc_codegen::compile` | `qc-ide`, `qc-preproc`, `qc-sema`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-commands`, `qs-sim`, `qc-project`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-commands`, `qs-sim`, `qc-project`, `xtask` |
| `fn qc_codegen::compile_module` | `qs-differential`, `qs-bench`, `qs-sim`, `xtask` | kept public: used by `qs-differential`, `qs-bench`, `qs-sim`, `xtask` |
| `struct qc_codegen::Compiled` | `qc-preproc`, `qc-project`, `xtask` | kept public: used by `qc-preproc`, `qc-project`, `xtask` |
| `struct qc_codegen::Unit` | `qc-builtins`, `qs-sim`, `qc-vm`, `xtask` | kept public: used by `qc-builtins`, `qs-sim`, `qc-vm`, `xtask` |
| `struct qc_codegen::precache::Precache` | `qc-builtins`, `xtask` | kept public: used by `qc-builtins`, `xtask` |
| `struct qc_codegen::precache::Precaches` | none | kept public: named in a public signature a dependent uses |
| `use qc_codegen::Precache = precache::Precache` | `qc-builtins`, `xtask` | kept public: used by `qc-builtins`, `xtask` |

Items that left the root (1): `use qc_codegen::Precaches = precache::Precaches`. Each is `pub(crate)` now, under `#[cfg(test)]` where only the crate's tests read it; `qc_sema::nav::ModuleIndex::references_at` is removed.

## `qc-diag` (L0)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qc_diag::code::Code::ALL` | `qc-ide`, `qc-preproc`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `enum qc_diag::code::Code` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qs-layout`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qs-layout`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `enum qc_diag::code::Severity` | `qc-ide`, `qc-codegen`, `qs-workbench`, `qc-lexer`, `qc-engines`, `desktop`, `lsp` | kept public: used by `qc-ide`, `qc-codegen`, `qs-workbench`, `qc-lexer`, `qc-engines`, `desktop`, `lsp` |
| `fn qc_diag::Diagnostic::is_error` | `qc-ide`, `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qc-engines`, `qc-project`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qc-engines`, `qc-project`, `xtask` |
| `fn qc_diag::Diagnostic::severity` | `qc-codegen`, `qc-engines`, `desktop`, `lsp` | kept public: used by `qc-codegen`, `qc-engines`, `desktop`, `lsp` |
| `fn qc_diag::Span::contains` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_diag::Span::covers` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_diag::Span::is_empty` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_diag::Span::len` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-porting-example`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-porting-example`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_diag::Span::new` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qc-engines`, `lsp` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qc-engines`, `lsp` |
| `fn qc_diag::Span::range` | `qc-ide`, `qc-preproc`, `qc-sema`, `qs-pak`, `qc-syntax`, `qc-lexer`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qs-pak`, `qc-syntax`, `qc-lexer`, `lsp`, `xtask`, `fuzz` |
| `fn qc_diag::Span::to` | `qc-preproc`, `qc-sema` | kept public: used by `qc-preproc`, `qc-sema` |
| `fn qc_diag::code::Code::as_str` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qc-syntax`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qc-syntax`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask` |
| `fn qc_diag::code::Code::is_error` | `qc-ide`, `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qc-engines`, `qc-project`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qc-engines`, `qc-project`, `xtask` |
| `fn qc_diag::code::Code::severity` | `qc-codegen`, `qc-engines`, `desktop`, `lsp` | kept public: used by `qc-codegen`, `qc-engines`, `desktop`, `lsp` |
| `struct qc_diag::Diagnostic` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qs-map`, `qs-tests`, `qc-project`, `lsp` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qs-map`, `qs-tests`, `qc-project`, `lsp` |
| `struct qc_diag::Span` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qc-engines`, `lsp` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qc-engines`, `lsp` |
| `use qc_diag::Code = code::Code` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qs-layout`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qs-layout`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `use qc_diag::Severity = code::Severity` | `qc-ide`, `qc-codegen`, `qs-workbench`, `qc-lexer`, `qc-engines`, `desktop`, `lsp` | kept public: used by `qc-ide`, `qc-codegen`, `qs-workbench`, `qc-lexer`, `qc-engines`, `desktop`, `lsp` |

## `qc-edict` (L1)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qc_edict::EDICT_HEADER_SIZE` | `qs-differential`, `qc-vm` | kept public: used by `qs-differential`, `qc-vm` |
| `const qc_edict::EDICT_SYS_GLOBALS_END` | `qc-builtins` | kept public: used by `qc-builtins` |
| `const qc_edict::number::EdictNumber::WORLD` | `qc-types`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-map`, `qs-sim`, `qc-vm`, `desktop`, `xtask` | kept public: used by `qc-types`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-map`, `qs-sim`, `qc-vm`, `desktop`, `xtask` |
| `enum qc_edict::layout::LayoutError` | `qs-layout`, `qc-vm` | kept public: used by `qs-layout`, `qc-vm` |
| `enum qc_edict::store::EdictError` | `qc-vm` | kept public: used by `qc-vm` |
| `fn qc_edict::layout::EdictLayout::edict_number` | `qs-sim` | kept public: used by `qs-sim` |
| `fn qc_edict::layout::EdictLayout::edict_size` | `qc-trace`, `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm`, `xtask` | kept public: used by `qc-trace`, `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm`, `xtask` |
| `fn qc_edict::layout::EdictLayout::entity_value` | `qs-differential`, `qs-sim`, `qc-vm` | kept public: used by `qs-differential`, `qs-sim`, `qc-vm` |
| `fn qc_edict::layout::EdictLayout::entityfields` | `qc-trace`, `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm`, `xtask` | kept public: used by `qc-trace`, `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm`, `xtask` |
| `fn qc_edict::layout::EdictLayout::for_progs` | `qc-vm` | kept public: used by `qc-vm` |
| `fn qc_edict::layout::EdictLayout::new` | `qc-vm` | kept public: used by `qc-vm` |
| `fn qc_edict::layout::EdictLayout::pointer_slot` | `qc-vm` | kept public: used by `qc-vm` |
| `fn qc_edict::layout::EdictLayout::pointer_value` | `qc-vm` | kept public: used by `qc-vm` |
| `fn qc_edict::number::EdictNumber::from_i32` | `qc-builtins`, `qc-vm` | kept public: used by `qc-builtins`, `qc-vm` |
| `fn qc_edict::number::EdictNumber::index` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qs-differential`, `qs-bench`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qs-differential`, `qs-bench`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask`, `fuzz` |
| `fn qc_edict::number::EdictNumber::new` | `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm` | kept public: used by `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm` |
| `fn qc_edict::number::EdictNumber::to_i32` | `qc-builtins` | kept public: used by `qc-builtins` |
| `fn qc_edict::store::EdictStore::alloc` | `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm` | kept public: used by `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm` |
| `fn qc_edict::store::EdictStore::clear_edict` | `qc-builtins` | kept public: used by `qc-builtins` |
| `fn qc_edict::store::EdictStore::edict_num` | `qs-differential`, `qc-builtins`, `qs-sim` | kept public: used by `qs-differential`, `qc-builtins`, `qs-sim` |
| `fn qc_edict::store::EdictStore::field` | `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm`, `xtask` |
| `fn qc_edict::store::EdictStore::fields` | `qc-trace`, `qc-syntax`, `qs-differential`, `qc-progs`, `qc-vm`, `xtask` | kept public: used by `qc-trace`, `qc-syntax`, `qs-differential`, `qc-progs`, `qc-vm`, `xtask` |
| `fn qc_edict::store::EdictStore::fields_mut` | `qs-differential` | kept public: used by `qs-differential` |
| `fn qc_edict::store::EdictStore::free` | `qc-sema`, `qs-differential`, `qc-builtins`, `qs-sim` | kept public: used by `qc-sema`, `qs-differential`, `qc-builtins`, `qs-sim` |
| `fn qc_edict::store::EdictStore::is_client` | `qc-builtins` | kept public: used by `qc-builtins` |
| `fn qc_edict::store::EdictStore::is_free` | `qs-differential`, `qc-builtins`, `qs-sim` | kept public: used by `qs-differential`, `qc-builtins`, `qs-sim` |
| `fn qc_edict::store::EdictStore::layout` | `qc-sema`, `qs-differential`, `qc-builtins`, `qc-progs`, `qs-sim`, `qc-vm`, `desktop` | kept public: used by `qc-sema`, `qs-differential`, `qc-builtins`, `qc-progs`, `qs-sim`, `qc-vm`, `desktop` |
| `fn qc_edict::store::EdictStore::maxclients` | `qs-differential`, `qc-builtins` | kept public: used by `qs-differential`, `qc-builtins` |
| `fn qc_edict::store::EdictStore::new` | `qc-vm` | kept public: used by `qc-vm` |
| `fn qc_edict::store::EdictStore::num_edicts` | `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm` | kept public: used by `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm` |
| `fn qc_edict::store::EdictStore::num_for_edict` | `qc-vm` | kept public: used by `qc-vm` |
| `fn qc_edict::store::EdictStore::set_field` | `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm` | kept public: used by `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm` |
| `fn qc_edict::system::field_by_name` | `qc-builtins`, `qs-sim` | kept public: used by `qc-builtins`, `qs-sim` |
| `fn qc_edict::system::global_by_name` | `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim` | kept public: used by `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim` |
| `mod qc_edict::system` | `qc-ide`, `qc-preproc`, `qs-wav`, `qc-types`, `qs-settings`, `qs-vfs`, `qs-log`, `qs-differential`, `qs-install`, `qc-builtins`, `qs-layout`, `qc-progs`, `qs-paths`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wav`, `qc-types`, `qs-settings`, `qs-vfs`, `qs-log`, `qs-differential`, `qs-install`, `qc-builtins`, `qs-layout`, `qc-progs`, `qs-paths`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `struct qc_edict::layout::EdictLayout` | `qc-vm` | kept public: used by `qc-vm` |
| `struct qc_edict::number::EdictNumber` | `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm` | kept public: used by `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm` |
| `struct qc_edict::store::EdictStore` | `qc-vm` | kept public: used by `qc-vm` |
| `struct qc_edict::system::SysVar` | none | kept public: the edict store the VM and the preview host share (EDICT-*) |
| `use qc_edict::EdictError = store::EdictError` | `qc-vm` | kept public: used by `qc-vm` |
| `use qc_edict::EdictLayout = layout::EdictLayout` | `qc-vm` | kept public: used by `qc-vm` |
| `use qc_edict::EdictNumber = number::EdictNumber` | `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm` | kept public: used by `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm` |
| `use qc_edict::EdictStore = store::EdictStore` | `qc-vm` | kept public: used by `qc-vm` |
| `use qc_edict::LayoutError = layout::LayoutError` | `qs-layout`, `qc-vm` | kept public: used by `qs-layout`, `qc-vm` |

Items that left the root (9): `const qc_edict::EDICT_FREED_NEXTTHINK`, `const qc_edict::EDICT_MAX_CLIENTS`, `const qc_edict::EDICT_MAX_FIELDS`, `const qc_edict::EDICT_REUSE_DELAY`, `const qc_edict::EDICT_REUSE_GRACE`, `const qc_edict::EDICT_SYS_FIELDS_END`, `fn qc_edict::store::EdictStore::freetime`, `fn qc_edict::system::field_at_slot`, `fn qc_edict::system::global_at_slot`. Each is `pub(crate)` now, under `#[cfg(test)]` where only the crate's tests read it; `qc_sema::nav::ModuleIndex::references_at` is removed.

## `qc-engines` (L3)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qc_engines::Engine::ALL` | `qc-ide`, `qc-preproc`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `const qc_engines::tables::SOURCES` | `xtask` | kept public: used by `xtask` |
| `enum qc_engines::BuiltinKey` | `xtask` | kept public: used by `xtask` |
| `enum qc_engines::Engine` | `qc-ide`, `qc-trace`, `qs-differential`, `qc-vm`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-trace`, `qs-differential`, `qc-vm`, `xtask`, `fuzz` |
| `enum qc_engines::check::BuiltinRef` | `qc-ide`, `lsp` | kept public: used by `qc-ide`, `lsp` |
| `fn qc_engines::Engine::builtin` | `qc-syntax`, `xtask` | kept public: used by `qc-syntax`, `xtask` |
| `fn qc_engines::Engine::extensions` | `qs-log` | kept public: used by `qs-log` |
| `fn qc_engines::Engine::from_target` | `qc-ide` | kept public: used by `qc-ide` |
| `fn qc_engines::Engine::has_table` | `qc-ide` | kept public: used by `qc-ide` |
| `fn qc_engines::Engine::id` | `qs-editor-view`, `qs-commands`, `desktop` | kept public: used by `qs-editor-view`, `qs-commands`, `desktop` |
| `fn qc_engines::Engine::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_engines::check::BuiltinRef::lookup` | `qc-ide`, `qc-sema` | kept public: used by `qc-ide`, `qc-sema` |
| `fn qc_engines::check::declared_builtins` | `qc-ide` | kept public: used by `qc-ide` |
| `fn qc_engines::check::missing_builtins` | `qc-ide`, `fuzz` | kept public: used by `qc-ide`, `fuzz` |
| `mod qc_engines::check` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-vm`, `desktop`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-vm`, `desktop`, `xtask`, `fuzz` |
| `struct qc_engines::Builtin` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qc-builtins`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qc-builtins`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask` |
| `struct qc_engines::Source` | `qc-preproc`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qs-log`, `qs-install`, `qc-builtins`, `qc-project`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qs-log`, `qs-install`, `qc-builtins`, `qc-project`, `desktop`, `xtask` |
| `use qc_engines::BuiltinRef = check::BuiltinRef` | `qc-ide`, `lsp` | kept public: used by `qc-ide`, `lsp` |
| `use qc_engines::SOURCES = tables::SOURCES` | `xtask` | kept public: used by `xtask` |
| `use qc_engines::declared_builtins = check::declared_builtins` | `qc-ide` | kept public: used by `qc-ide` |
| `use qc_engines::missing_builtins = check::missing_builtins` | `qc-ide`, `fuzz` | kept public: used by `qc-ide`, `fuzz` |

Items that left the root (3): `fn qc_engines::Engine::builtin_named`, `fn qc_engines::Engine::builtins`, `fn qc_engines::Engine::from_id`. Each is `pub(crate)` now, under `#[cfg(test)]` where only the crate's tests read it; `qc_sema::nav::ModuleIndex::references_at` is removed.

## `qc-ide` (L3)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qc_ide::highlight::Modifier::ALL` | `qc-preproc`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `enum qc_ide::highlight::HighlightKind` | `qs-workbench`, `desktop`, `lsp` | kept public: used by `qs-workbench`, `desktop`, `lsp` |
| `enum qc_ide::highlight::Modifier` | `qs-workbench`, `desktop`, `lsp` | kept public: used by `qs-workbench`, `desktop`, `lsp` |
| `enum qc_ide::highlight::SyntaxClass` | `qs-workbench`, `desktop` | kept public: used by `qs-workbench`, `desktop` |
| `enum qc_ide::rename::Caution` | `qs-workbench` | kept public: used by `qs-workbench` |
| `enum qc_ide::rename::EngineBinding` | none | kept public: named in a public signature a dependent uses |
| `enum qc_ide::rename::RenameError` | `qs-workbench`, `lsp` | kept public: used by `qs-workbench`, `lsp` |
| `enum qc_ide::symbols::SymbolKind` | `qs-workbench`, `desktop` | kept public: used by `qs-workbench`, `desktop` |
| `fn qc_ide::completion::AnalysisHost::completions` | `qs-workbench`, `lsp` | kept public: used by `qs-workbench`, `lsp` |
| `fn qc_ide::diagnostics::AnalysisHost::all_diagnostics` | `qs-workbench`, `lsp` | kept public: used by `qs-workbench`, `lsp` |
| `fn qc_ide::diagnostics::AnalysisHost::diagnostics` | `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-tests`, `xtask`, `fuzz` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-tests`, `xtask`, `fuzz` |
| `fn qc_ide::highlight::AnalysisHost::highlights` | `qs-workbench`, `desktop`, `lsp` | kept public: used by `qs-workbench`, `desktop`, `lsp` |
| `fn qc_ide::highlight::AnalysisHost::syntax_classes` | `qs-workbench` | kept public: used by `qs-workbench` |
| `fn qc_ide::highlight::HighlightKind::of` | `qs-sim`, `xtask` | kept public: used by `qs-sim`, `xtask` |
| `fn qc_ide::highlight::Modifiers::contains` | `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_ide::highlight::Modifiers::iter` | `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-porting-example`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-porting-example`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_ide::highlight::Modifiers::with` | `qc-preproc`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-install`, `qs-sim`, `qc-vm` | kept public: used by `qc-preproc`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-install`, `qs-sim`, `qc-vm` |
| `fn qc_ide::highlight::SyntaxClass::of` | `qs-sim`, `xtask` | kept public: used by `qs-sim`, `xtask` |
| `fn qc_ide::hints::AnalysisHost::parameter_hints` | `lsp` | kept public: used by `lsp` |
| `fn qc_ide::host::AnalysisHost::files` | `qc-preproc`, `qs-vfs`, `qs-workbench` | kept public: used by `qc-preproc`, `qs-vfs`, `qs-workbench` |
| `fn qc_ide::host::AnalysisHost::new` | `qs-workbench`, `lsp` | kept public: used by `qs-workbench`, `lsp` |
| `fn qc_ide::host::AnalysisHost::open` | `qc-preproc`, `qs-vfs`, `qs-workbench`, `qs-log`, `qs-differential`, `qs-install`, `qc-project`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `qs-vfs`, `qs-workbench`, `qs-log`, `qs-differential`, `qs-install`, `qc-project`, `desktop`, `lsp`, `xtask` |
| `fn qc_ide::host::AnalysisHost::reload` | `lsp` | kept public: used by `lsp` |
| `fn qc_ide::host::AnalysisHost::remove_overlay` | `lsp` | kept public: used by `lsp` |
| `fn qc_ide::host::AnalysisHost::set_config` | `qs-workbench`, `lsp` | kept public: used by `qs-workbench`, `lsp` |
| `fn qc_ide::host::AnalysisHost::set_overlay` | `qs-workbench`, `lsp` | kept public: used by `qs-workbench`, `lsp` |
| `fn qc_ide::host::AnalysisHost::text` | `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_ide::hover::AnalysisHost::hover` | `qs-workbench`, `lsp` | kept public: used by `qs-workbench`, `lsp` |
| `fn qc_ide::hover::BuiltinInfo::on_target` | `lsp` | kept public: used by `lsp` |
| `fn qc_ide::navigation::AnalysisHost::definition` | `qc-sema`, `qs-workbench`, `lsp` | kept public: used by `qc-sema`, `qs-workbench`, `lsp` |
| `fn qc_ide::navigation::AnalysisHost::references` | `qc-sema`, `qs-workbench`, `lsp` | kept public: used by `qc-sema`, `qs-workbench`, `lsp` |
| `fn qc_ide::rename::AnalysisHost::prepare_rename` | `lsp` | kept public: used by `lsp` |
| `fn qc_ide::rename::AnalysisHost::rename` | `qc-preproc`, `qs-workbench`, `lsp` | kept public: used by `qc-preproc`, `qs-workbench`, `lsp` |
| `fn qc_ide::rename::Caution::name` | `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_ide::symbols::AnalysisHost::document_symbols` | `qs-workbench` | kept public: used by `qs-workbench` |
| `fn qc_ide::symbols::AnalysisHost::workspace_symbols` | `qs-workbench` | kept public: used by `qs-workbench` |
| `struct qc_ide::FileRange` | `qs-workbench`, `lsp` | kept public: used by `qs-workbench`, `lsp` |
| `struct qc_ide::completion::Completion` | `qs-workbench`, `desktop`, `lsp`, `xtask` | kept public: used by `qs-workbench`, `desktop`, `lsp`, `xtask` |
| `struct qc_ide::highlight::Highlight` | `qs-workbench`, `desktop` | kept public: used by `qs-workbench`, `desktop` |
| `struct qc_ide::highlight::Modifiers` | `qs-workbench`, `qs-editor-view`, `desktop` | kept public: used by `qs-workbench`, `qs-editor-view`, `desktop` |
| `struct qc_ide::hints::ParameterHint` | none | kept public: named in a public signature a dependent uses |
| `struct qc_ide::host::AnalysisHost` | `qs-workbench`, `lsp` | kept public: used by `qs-workbench`, `lsp` |
| `struct qc_ide::host::Config` | `qs-workbench`, `desktop`, `lsp`, `xtask` | kept public: used by `qs-workbench`, `desktop`, `lsp`, `xtask` |
| `struct qc_ide::hover::BuiltinInfo` | `lsp` | kept public: used by `lsp` |
| `struct qc_ide::hover::Hover` | `qs-workbench`, `desktop`, `lsp`, `xtask` | kept public: used by `qs-workbench`, `desktop`, `lsp`, `xtask` |
| `struct qc_ide::rename::FileEdit` | `qs-workbench`, `lsp` | kept public: used by `qs-workbench`, `lsp` |
| `struct qc_ide::rename::Rename` | `qc-preproc`, `qs-workbench`, `qc-diag`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `qs-workbench`, `qc-diag`, `desktop`, `lsp`, `xtask` |
| `struct qc_ide::symbols::Symbol` | `qs-workbench`, `desktop` | kept public: used by `qs-workbench`, `desktop` |
| `use qc_ide::AnalysisHost = host::AnalysisHost` | `qs-workbench`, `lsp` | kept public: used by `qs-workbench`, `lsp` |
| `use qc_ide::Builtin = qc_engines::Builtin` | `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qc-builtins`, `qc-engines`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qc-builtins`, `qc-engines`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask` |
| `use qc_ide::BuiltinInfo = hover::BuiltinInfo` | `lsp` | kept public: used by `lsp` |
| `use qc_ide::BuiltinRef = qc_engines::BuiltinRef` | `qc-engines`, `lsp` | kept public: used by `qc-engines`, `lsp` |
| `use qc_ide::Caution = rename::Caution` | `qs-workbench` | kept public: used by `qs-workbench` |
| `use qc_ide::Code = qc_diag::Code` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-layout`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-layout`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `use qc_ide::Completion = completion::Completion` | `qs-workbench`, `desktop`, `lsp`, `xtask` | kept public: used by `qs-workbench`, `desktop`, `lsp`, `xtask` |
| `use qc_ide::Config = host::Config` | `qs-workbench`, `desktop`, `lsp`, `xtask` | kept public: used by `qs-workbench`, `desktop`, `lsp`, `xtask` |
| `use qc_ide::DefKind = qc_sema::DefKind` | `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-engines`, `lsp`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-engines`, `lsp`, `xtask` |
| `use qc_ide::Diagnostic = qc_diag::Diagnostic` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-map`, `qs-tests`, `qc-project`, `lsp` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-map`, `qs-tests`, `qc-project`, `lsp` |
| `use qc_ide::Engine = qc_engines::Engine` | `qc-trace`, `qs-differential`, `qc-engines`, `qc-vm`, `xtask`, `fuzz` | kept public: used by `qc-trace`, `qs-differential`, `qc-engines`, `qc-vm`, `xtask`, `fuzz` |
| `use qc_ide::EngineBinding = rename::EngineBinding` | none | kept public: named in a public signature a dependent uses |
| `use qc_ide::FileEdit = rename::FileEdit` | `qs-workbench`, `lsp` | kept public: used by `qs-workbench`, `lsp` |
| `use qc_ide::Highlight = highlight::Highlight` | `qs-workbench`, `desktop` | kept public: used by `qs-workbench`, `desktop` |
| `use qc_ide::HighlightKind = highlight::HighlightKind` | `qs-workbench`, `desktop`, `lsp` | kept public: used by `qs-workbench`, `desktop`, `lsp` |
| `use qc_ide::Hover = hover::Hover` | `qs-workbench`, `desktop`, `lsp`, `xtask` | kept public: used by `qs-workbench`, `desktop`, `lsp`, `xtask` |
| `use qc_ide::Modifier = highlight::Modifier` | `qs-workbench`, `desktop`, `lsp` | kept public: used by `qs-workbench`, `desktop`, `lsp` |
| `use qc_ide::Modifiers = highlight::Modifiers` | `qs-workbench`, `qs-editor-view`, `desktop` | kept public: used by `qs-workbench`, `qs-editor-view`, `desktop` |
| `use qc_ide::ParameterHint = hints::ParameterHint` | none | kept public: named in a public signature a dependent uses |
| `use qc_ide::Rename = rename::Rename` | `qc-preproc`, `qs-workbench`, `qc-diag`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `qs-workbench`, `qc-diag`, `desktop`, `lsp`, `xtask` |
| `use qc_ide::RenameError = rename::RenameError` | `qs-workbench`, `lsp` | kept public: used by `qs-workbench`, `lsp` |
| `use qc_ide::Severity = qc_diag::Severity` | `qc-codegen`, `qs-workbench`, `qc-diag`, `qc-lexer`, `qc-engines`, `desktop`, `lsp` | kept public: used by `qc-codegen`, `qs-workbench`, `qc-diag`, `qc-lexer`, `qc-engines`, `desktop`, `lsp` |
| `use qc_ide::Span = qc_diag::Span` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qc-engines`, `lsp` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qc-engines`, `lsp` |
| `use qc_ide::Symbol = symbols::Symbol` | `qs-workbench`, `desktop` | kept public: used by `qs-workbench`, `desktop` |
| `use qc_ide::SymbolKind = symbols::SymbolKind` | `qs-workbench`, `desktop` | kept public: used by `qs-workbench`, `desktop` |
| `use qc_ide::SyntaxClass = highlight::SyntaxClass` | `qs-workbench`, `desktop` | kept public: used by `qs-workbench`, `desktop` |

## `qc-lexer` (L2)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qc_lexer::FTEQCC_MAX_WORD` | `qc-preproc` | kept public: used by `qc-preproc` |
| `const qc_lexer::Punct::ALL` | `qc-ide`, `qc-preproc`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `const qc_lexer::flags::LexFlags::NONE` | `qc-preproc`, `qc-sema`, `qc-syntax`, `qs-editor-view`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qc-syntax`, `qs-editor-view`, `desktop`, `xtask` |
| `enum qc_lexer::Dialect` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-differential`, `qc-engines`, `qs-tests`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-differential`, `qc-engines`, `qs-tests`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `enum qc_lexer::FrameCommand` | `qc-preproc`, `qc-codegen`, `qc-syntax`, `xtask` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-syntax`, `xtask` |
| `enum qc_lexer::Punct` | `qc-preproc`, `qc-codegen`, `qc-syntax`, `xtask` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-syntax`, `xtask` |
| `enum qc_lexer::TokenKind` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-project`, `xtask` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-project`, `xtask` |
| `enum qc_lexer::directive::Directive` | `qc-preproc`, `qc-syntax`, `qc-builtins`, `qc-project`, `xtask` | kept public: used by `qc-preproc`, `qc-syntax`, `qc-builtins`, `qc-project`, `xtask` |
| `enum qc_lexer::directive::Pragma` | `qc-preproc`, `qc-project`, `xtask` | kept public: used by `qc-preproc`, `qc-project`, `xtask` |
| `enum qc_lexer::flags::LexFlag` | `qc-preproc`, `qc-sema`, `qc-syntax` | kept public: used by `qc-preproc`, `qc-sema`, `qc-syntax` |
| `fn qc_lexer::Dialect::has_fteqcc_extensions` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-project`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-project`, `xtask` |
| `fn qc_lexer::Dialect::is_whitespace` | `qc-preproc`, `qc-sema`, `qs-install`, `qs-commands`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qs-install`, `qs-commands`, `xtask` |
| `fn qc_lexer::Punct::as_str` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qc-syntax`, `qc-diag`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qc-syntax`, `qc-diag`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask` |
| `fn qc_lexer::atof::atof` | `qc-sema`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-sim` | kept public: used by `qc-sema`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-sim` |
| `fn qc_lexer::directive::Directive::from_word` | `qc-preproc`, `qc-project` | kept public: used by `qc-preproc`, `qc-project` |
| `fn qc_lexer::directive::Directive::fteqcc_prefix` | none | kept public: named in a public signature a dependent uses |
| `fn qc_lexer::directive::Directive::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_lexer::directive::Pragma::from_word` | `qc-project` | kept public: used by `qc-project` |
| `fn qc_lexer::directive::Pragma::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_lexer::flags::LexFlags::at` | `qc-codegen`, `qc-trace`, `qc-sema`, `qs-mdl`, `qc-syntax`, `qs-sim` | kept public: used by `qc-codegen`, `qc-trace`, `qc-sema`, `qs-mdl`, `qc-syntax`, `qs-sim` |
| `fn qc_lexer::flags::LexFlags::has` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `desktop` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `desktop` |
| `fn qc_lexer::flags::LexFlags::with` | `qc-ide`, `qc-preproc`, `qc-sema`, `qc-syntax`, `qs-log`, `qs-install`, `qs-sim`, `qc-vm` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qc-syntax`, `qs-log`, `qs-install`, `qs-sim`, `qc-vm` |
| `fn qc_lexer::fte_string::fteqcc_string_bytes` | `qc-preproc`, `qc-syntax` | kept public: used by `qc-preproc`, `qc-syntax` |
| `fn qc_lexer::int_to_f32` | `qc-preproc`, `qc-sema`, `qs-differential`, `qs-map` | kept public: used by `qc-preproc`, `qc-sema`, `qs-differential`, `qs-map` |
| `fn qc_lexer::is_fteqcc_whitespace` | `qc-preproc` | kept public: used by `qc-preproc` |
| `fn qc_lexer::is_name_char` | `qc-preproc`, `qc-project` | kept public: used by `qc-preproc`, `qc-project` |
| `fn qc_lexer::is_name_start` | `qc-preproc`, `qc-project` | kept public: used by `qc-preproc`, `qc-project` |
| `fn qc_lexer::is_whitespace` | `qc-preproc`, `qc-sema`, `qc-syntax`, `qs-install`, `qs-commands`, `qc-project`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qc-syntax`, `qs-install`, `qs-commands`, `qc-project`, `xtask` |
| `fn qc_lexer::latin1` | `qc-codegen`, `qc-syntax` | kept public: used by `qc-codegen`, `qc-syntax` |
| `fn qc_lexer::lex` | `qc-preproc`, `qc-sema`, `qc-syntax`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qc-syntax`, `xtask` |
| `fn qc_lexer::lex_flagged` | `qc-preproc`, `qc-sema` | kept public: used by `qc-preproc`, `qc-sema` |
| `fn qc_lexer::lex_marked` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_lexer::lex_with` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `fuzz` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `fuzz` |
| `fn qc_lexer::lines::LineIndex::line` | `qc-preproc`, `qc-sema`, `qs-workbench`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qs-workbench`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask` |
| `fn qc_lexer::lines::LineIndex::new` | `qc-preproc`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `lsp`, `xtask` |
| `struct qc_lexer::FrameName` | none | kept public: named in a public signature a dependent uses |
| `struct qc_lexer::Lexed` | `qc-preproc`, `qc-syntax`, `qs-map` | kept public: used by `qc-preproc`, `qc-syntax`, `qs-map` |
| `struct qc_lexer::Token` | `qc-preproc`, `qc-sema`, `qc-syntax`, `qs-install`, `qs-map`, `qs-sim`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qc-syntax`, `qs-install`, `qs-map`, `qs-sim`, `desktop`, `xtask` |
| `struct qc_lexer::flags::LexFlags` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax` |
| `struct qc_lexer::lines::LineIndex` | `qc-preproc`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `lsp`, `xtask` |
| `use qc_lexer::Code = qc_diag::Code` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qs-layout`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qs-layout`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `use qc_lexer::Diagnostic = qc_diag::Diagnostic` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qs-map`, `qs-tests`, `qc-project`, `lsp` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qs-map`, `qs-tests`, `qc-project`, `lsp` |
| `use qc_lexer::Directive = directive::Directive` | `qc-preproc`, `qc-syntax`, `qc-builtins`, `qc-project`, `xtask` | kept public: used by `qc-preproc`, `qc-syntax`, `qc-builtins`, `qc-project`, `xtask` |
| `use qc_lexer::LexFlag = flags::LexFlag` | `qc-preproc`, `qc-sema`, `qc-syntax` | kept public: used by `qc-preproc`, `qc-sema`, `qc-syntax` |
| `use qc_lexer::LexFlags = flags::LexFlags` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax` |
| `use qc_lexer::LineIndex = lines::LineIndex` | `qc-preproc`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `lsp`, `xtask` |
| `use qc_lexer::Pragma = directive::Pragma` | `qc-preproc`, `qc-project`, `xtask` | kept public: used by `qc-preproc`, `qc-project`, `xtask` |
| `use qc_lexer::Severity = qc_diag::Severity` | `qc-ide`, `qc-codegen`, `qs-workbench`, `qc-diag`, `qc-engines`, `desktop`, `lsp` | kept public: used by `qc-ide`, `qc-codegen`, `qs-workbench`, `qc-diag`, `qc-engines`, `desktop`, `lsp` |
| `use qc_lexer::Span = qc_diag::Span` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-engines`, `lsp` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-engines`, `lsp` |
| `use qc_lexer::atof = atof::atof` | `qc-sema`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-sim` | kept public: used by `qc-sema`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-sim` |
| `use qc_lexer::fteqcc_string_bytes = fte_string::fteqcc_string_bytes` | `qc-preproc`, `qc-syntax` | kept public: used by `qc-preproc`, `qc-syntax` |

Items that left the root (8): `const qc_lexer::FTEQCC_MAX_TOKEN`, `const qc_lexer::Punct::FTEQCC_TABLE`, `const qc_lexer::QC_MAX_FRAME_NAME`, `const qc_lexer::QC_MAX_TOKEN`, `const qc_lexer::fte_string::FTEQCC_MAX_STRING`, `const qc_lexer::fte_string::STRING_TOO_LONG`, `use qc_lexer::FTEQCC_MAX_STRING = fte_string::FTEQCC_MAX_STRING`, `use qc_lexer::STRING_TOO_LONG = fte_string::STRING_TOO_LONG`. Each is `pub(crate)` now, under `#[cfg(test)]` where only the crate's tests read it; `qc_sema::nav::ModuleIndex::references_at` is removed.

## `qc-lno` (L0)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qc_lno::LNO_HEADER_SIZE` | `xtask` | kept public: used by `xtask` |
| `const qc_lno::LNO_MAGIC` | `xtask` | kept public: used by `xtask` |
| `const qc_lno::LNO_MAX_STATEMENTS` | none | kept public: the .lno format's model (LNO-*) |
| `const qc_lno::LNO_VERSION` | `xtask` | kept public: used by `xtask` |
| `enum qc_lno::CountField` | none | kept public: the .lno format's model (LNO-*) |
| `enum qc_lno::read::ReadError` | `qs-differential`, `qc-progs` | kept public: used by `qs-differential`, `qc-progs` |
| `enum qc_lno::write::WriteError` | `qc-codegen`, `qc-progs`, `fuzz` | kept public: used by `qc-codegen`, `qc-progs`, `fuzz` |
| `fn qc_lno::LnoFile::matches` | `qc-sema`, `qs-differential`, `qs-editor-view`, `qc-vm`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-sema`, `qs-differential`, `qs-editor-view`, `qc-vm`, `desktop`, `lsp`, `xtask` |
| `fn qc_lno::LnoFile::matches_header` | `qc-codegen`, `xtask` | kept public: used by `qc-codegen`, `xtask` |
| `fn qc_lno::LnoFile::numstatements` | `qc-vm` | kept public: used by `qc-vm` |
| `fn qc_lno::read::LnoFile::read` | `qc-sema`, `qs-vfs`, `qs-install`, `xtask` | kept public: used by `qc-sema`, `qs-vfs`, `qs-install`, `xtask` |
| `fn qc_lno::write::LnoFile::write` | `qc-preproc`, `qc-types`, `qs-log`, `qs-render`, `qc-builtins`, `qs-layout`, `qs-map`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qc-types`, `qs-log`, `qs-render`, `qc-builtins`, `qs-layout`, `qs-map`, `desktop`, `xtask` |
| `struct qc_lno::LnoFile` | `qc-codegen`, `xtask` | kept public: used by `qc-codegen`, `xtask` |
| `struct qc_lno::MatchError` | none | kept public: the .lno format's model (LNO-*) |
| `use qc_lno::ReadError = read::ReadError` | `qs-differential`, `qc-progs` | kept public: used by `qs-differential`, `qc-progs` |
| `use qc_lno::WriteError = write::WriteError` | `qc-codegen`, `qc-progs`, `fuzz` | kept public: used by `qc-codegen`, `qc-progs`, `fuzz` |

## `qc-preproc` (L2)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qc_preproc::LIMIT_QCC_MAX_FRAMES` | `qc-progs` | kept public: used by `qc-progs` |
| `const qc_preproc::fte_keywords::Keyword::ALL` | `qc-ide`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `const qc_preproc::fte_keywords::Keywords::NONE` | `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-editor-view`, `desktop`, `xtask` | kept public: used by `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-editor-view`, `desktop`, `xtask` |
| `const qc_preproc::fte_ops::Op::ALL` | `qc-ide`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `enum qc_preproc::compiler_flags::PragmaDefault` | `qc-sema` | kept public: used by `qc-sema` |
| `enum qc_preproc::compiler_flags::Priorities` | `qc-syntax` | kept public: used by `qc-syntax` |
| `enum qc_preproc::cond::Block` | `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-install`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-install`, `xtask` |
| `enum qc_preproc::fte_directive::graph::FileStart` | `qc-project` | kept public: used by `qc-project` |
| `enum qc_preproc::fte_directive::graph::FteEvent` | `qc-project` | kept public: used by `qc-project` |
| `enum qc_preproc::fte_keywords::Enabled` | `qc-sema`, `qc-syntax`, `qs-editor-view` | kept public: used by `qc-sema`, `qc-syntax`, `qs-editor-view` |
| `enum qc_preproc::fte_keywords::Keyword` | `qc-ide`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qc-project`, `desktop` | kept public: used by `qc-ide`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qc-project`, `desktop` |
| `enum qc_preproc::fte_ops::Op` | `qc-codegen`, `qc-sema`, `qs-differential`, `qs-editor-view`, `qs-commands` | kept public: used by `qc-codegen`, `qc-sema`, `qs-differential`, `qs-editor-view`, `qs-commands` |
| `enum qc_preproc::fte_tables::FteFlag` | `qc-codegen`, `qc-sema`, `qc-syntax` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax` |
| `enum qc_preproc::fte_tables::TargetFormat` | `qc-sema` | kept public: used by `qc-sema` |
| `enum qc_preproc::fte_warning::FteWarning` | `qc-sema` | kept public: used by `qc-sema` |
| `enum qc_preproc::fte_warning::WarningAction` | none | kept public: named in a public signature a dependent uses |
| `enum qc_preproc::source::TextOrigin` | none | kept public: named in a public signature a dependent uses |
| `fn qc_preproc::PpToken::touches` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_preproc::Preprocessor::is_defined` | `qc-project` | kept public: used by `qc-project` |
| `fn qc_preproc::Preprocessor::new` | `qc-sema`, `qc-syntax`, `qc-project`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `qc-syntax`, `qc-project`, `xtask`, `fuzz` |
| `fn qc_preproc::Preprocessor::origin` | `qc-sema`, `qs-map`, `fuzz` | kept public: used by `qc-sema`, `qs-map`, `fuzz` |
| `fn qc_preproc::Preprocessor::preprocess_file` | `qc-sema`, `qc-syntax`, `qc-project`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `qc-syntax`, `qc-project`, `xtask`, `fuzz` |
| `fn qc_preproc::Preprocessor::preprocess_file_named` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_preproc::Preprocessor::preprocess_source` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_preproc::Preprocessor::preprocess_source_named` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_preproc::Preprocessor::restart` | `qc-sema`, `fuzz` | kept public: used by `qc-sema`, `fuzz` |
| `fn qc_preproc::Preprocessor::sources` | `qc-sema`, `qc-syntax`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `qc-syntax`, `xtask`, `fuzz` |
| `fn qc_preproc::Preprocessor::target` | `qc-sema`, `qc-syntax`, `qs-log`, `xtask` | kept public: used by `qc-sema`, `qc-syntax`, `qs-log`, `xtask` |
| `fn qc_preproc::Preprocessor::with_command_line` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_preproc::Preprocessor::with_defines` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_preproc::Preprocessor::with_dialect` | `qc-sema`, `qc-syntax`, `qc-project`, `fuzz` | kept public: used by `qc-sema`, `qc-syntax`, `qc-project`, `fuzz` |
| `fn qc_preproc::Preprocessor::with_include_dirs` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_preproc::Preprocessor::with_working_dir` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_preproc::compiler_flags::CompilerFlags::at` | `qc-codegen`, `qc-trace`, `qc-sema`, `qs-mdl`, `qc-syntax`, `qs-sim` | kept public: used by `qc-codegen`, `qc-trace`, `qc-sema`, `qs-mdl`, `qc-syntax`, `qs-sim` |
| `fn qc_preproc::compiler_flags::CompilerFlags::default_on` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_preproc::compiler_flags::CompilerFlags::has` | `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-lexer`, `desktop` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-lexer`, `desktop` |
| `fn qc_preproc::compiler_flags::CompilerFlags::lex` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_preproc::compiler_flags::CompilerFlags::priorities` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_preproc::compiler_flags::CompilerFlags::with` | `qc-ide`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-install`, `qs-sim`, `qc-vm` | kept public: used by `qc-ide`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-install`, `qs-sim`, `qc-vm` |
| `fn qc_preproc::compiler_flags::CompilerFlags::with_default` | none | kept public: named in a public signature a dependent uses |
| `fn qc_preproc::compiler_flags::CompilerFlags::with_priorities` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_preproc::compiler_flags::FlagEdit::apply` | `qc-syntax`, `qs-differential`, `desktop` | kept public: used by `qc-syntax`, `qs-differential`, `desktop` |
| `fn qc_preproc::compiler_flags::FlagEdit::is_empty` | `qc-ide`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_preproc::compiler_flags::FlagEdit::of_default` | none | kept public: named in a public signature a dependent uses |
| `fn qc_preproc::compiler_flags::FlagEdit::of_flags` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_preproc::compiler_flags::FlagEdit::whole` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_preproc::compiler_flags::PragmaDefault::of_pragma` | none | kept public: named in a public signature a dependent uses |
| `fn qc_preproc::cond::CondStack::close` | `qc-project`, `desktop` | kept public: used by `qc-project`, `desktop` |
| `fn qc_preproc::cond::CondStack::elif` | `qc-project` | kept public: used by `qc-project` |
| `fn qc_preproc::cond::CondStack::open` | `qc-ide`, `qs-vfs`, `qs-workbench`, `qs-log`, `qs-differential`, `qs-install`, `qc-project`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qs-vfs`, `qs-workbench`, `qs-log`, `qs-differential`, `qs-install`, `qc-project`, `desktop`, `lsp`, `xtask` |
| `fn qc_preproc::cond::CondStack::otherwise` | `qc-project` | kept public: used by `qc-project` |
| `fn qc_preproc::cond::CondStack::taking` | `qc-project` | kept public: used by `qc-project` |
| `fn qc_preproc::cond::atoi` | `qc-lexer`, `qs-differential`, `qs-map`, `qs-sim`, `qc-project`, `xtask` | kept public: used by `qc-lexer`, `qs-differential`, `qs-map`, `qs-sim`, `qc-project`, `xtask` |
| `fn qc_preproc::cond::eval` | `qc-codegen`, `qc-sema`, `qc-project`, `xtask`, `fuzz` | kept public: used by `qc-codegen`, `qc-sema`, `qc-project`, `xtask`, `fuzz` |
| `fn qc_preproc::cond::fte_atoi` | `qc-project`, `xtask` | kept public: used by `qc-project`, `xtask` |
| `fn qc_preproc::fte_directive::com_parse` | `qc-project` | kept public: used by `qc-project` |
| `fn qc_preproc::fte_directive::graph::FteFileReader::new` | `qc-project` | kept public: used by `qc-project` |
| `fn qc_preproc::fte_directive::graph::FteFileReader::next` | `qc-ide`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-mdl`, `qc-syntax`, `qs-bsp`, `qc-diag`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-porting-example`, `qs-map`, `qc-progs`, `qs-editor-view`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-mdl`, `qc-syntax`, `qs-bsp`, `qc-diag`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-porting-example`, `qs-map`, `qc-progs`, `qs-editor-view`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_preproc::fte_directive::graph::FteFileReader::progs` | `qc-builtins`, `qs-sim`, `qc-project` | kept public: used by `qc-builtins`, `qs-sim`, `qc-project` |
| `fn qc_preproc::fte_directive::graph::ModuleState::define` | `qc-syntax`, `qc-project` | kept public: used by `qc-syntax`, `qc-project` |
| `fn qc_preproc::fte_directive::graph::ModuleState::fteqcc` | `qc-sema`, `qc-syntax`, `qc-project` | kept public: used by `qc-sema`, `qc-syntax`, `qc-project` |
| `fn qc_preproc::fte_directive::graph::ModuleState::is_defined` | `qc-project` | kept public: used by `qc-project` |
| `fn qc_preproc::fte_directive::graph::ModuleState::new` | `qc-project` | kept public: used by `qc-project` |
| `fn qc_preproc::fte_directive::graph::ModuleState::undefine` | `qc-project` | kept public: used by `qc-project` |
| `fn qc_preproc::fte_directive::graph::ModuleState::value` | `qc-codegen`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-differential`, `qs-render`, `qs-map`, `qs-editor-view`, `qc-project` | kept public: used by `qc-codegen`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-differential`, `qs-render`, `qs-map`, `qs-editor-view`, `qc-project` |
| `fn qc_preproc::fte_directive::include_dir_name` | `qc-project` | kept public: used by `qc-project` |
| `fn qc_preproc::fte_directive::join_paths` | `qc-project` | kept public: used by `qc-project` |
| `fn qc_preproc::fte_directive::load::qcc_load_as` | `qc-project` | kept public: used by `qc-project` |
| `fn qc_preproc::fte_directive::load::qcc_load_in_place` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_preproc::fte_directive::load::skip_script_line` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_preproc::fte_directive::pragma_include_dir` | `qc-project` | kept public: used by `qc-project` |
| `fn qc_preproc::fte_keywords::Keyword::flag` | none | kept public: named in a public signature a dependent uses |
| `fn qc_preproc::fte_keywords::Keyword::word` | `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qc-builtins`, `qs-map`, `xtask` | kept public: used by `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qc-builtins`, `qs-map`, `xtask` |
| `fn qc_preproc::fte_keywords::KeywordEdit::apply` | `qc-syntax`, `qs-differential`, `desktop` | kept public: used by `qc-syntax`, `qs-differential`, `desktop` |
| `fn qc_preproc::fte_keywords::KeywordEdit::is_empty` | `qc-ide`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_preproc::fte_keywords::KeywordEdit::whole` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_preproc::fte_keywords::Keywords::after_definition` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_preproc::fte_keywords::Keywords::at` | `qc-codegen`, `qc-trace`, `qc-sema`, `qs-mdl`, `qc-syntax`, `qs-sim` | kept public: used by `qc-codegen`, `qc-trace`, `qc-sema`, `qs-mdl`, `qc-syntax`, `qs-sim` |
| `fn qc_preproc::fte_keywords::Keywords::case_insensitive` | `qc-sema`, `qc-syntax` | kept public: used by `qc-sema`, `qc-syntax` |
| `fn qc_preproc::fte_keywords::Keywords::checks` | `qc-sema`, `qc-syntax` | kept public: used by `qc-sema`, `qc-syntax` |
| `fn qc_preproc::fte_keywords::Keywords::coexist` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_preproc::fte_keywords::Keywords::has` | `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-lexer`, `desktop` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-lexer`, `desktop` |
| `fn qc_preproc::fte_keywords::Keywords::hexen2_target` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_preproc::fte_keywords::Keywords::named_definition` | `qc-sema`, `qc-syntax` | kept public: used by `qc-sema`, `qc-syntax` |
| `fn qc_preproc::fte_keywords::Keywords::names` | `qc-sema`, `qc-syntax` | kept public: used by `qc-sema`, `qc-syntax` |
| `fn qc_preproc::fte_keywords::Keywords::with` | `qc-ide`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-install`, `qs-sim`, `qc-vm` | kept public: used by `qc-ide`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-install`, `qs-sim`, `qc-vm` |
| `fn qc_preproc::fte_keywords::Keywords::with_case_insensitive` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_preproc::fte_keywords::Keywords::with_coexist` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_preproc::fte_keywords::Keywords::with_hexen2_target` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_preproc::fte_line::fte_directive_end` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_preproc::fte_line::relex_from` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_preproc::fte_opcodes::OpcodeTarget::format` | `qc-sema`, `qc-progs` | kept public: used by `qc-sema`, `qc-progs` |
| `fn qc_preproc::fte_opcodes::OpcodeTarget::new` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_preproc::fte_opcodes::OpcodeTarget::revision` | `qs-workbench`, `qs-editor-view`, `desktop` | kept public: used by `qs-workbench`, `qs-editor-view`, `desktop` |
| `fn qc_preproc::fte_opcodes::OpcodeTarget::stores_at_offset` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_preproc::fte_opcodes::OpcodeTarget::valid` | `qc-sema`, `lsp` | kept public: used by `qc-sema`, `lsp` |
| `fn qc_preproc::fte_ops::Op::is_internal` | none | kept public: named in a public signature a dependent uses |
| `fn qc_preproc::fte_ops::Op::name` | `qc-ide`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_preproc::fte_ops::Op::named` | `qc-sema`, `qc-syntax`, `xtask` | kept public: used by `qc-sema`, `qc-syntax`, `xtask` |
| `fn qc_preproc::fte_ops::Op::number` | `qc-codegen`, `qc-types`, `qc-sema`, `qc-syntax`, `qc-edict`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-map`, `qc-progs`, `qc-engines`, `qs-tests`, `qc-vm`, `xtask` | kept public: used by `qc-codegen`, `qc-types`, `qc-sema`, `qc-syntax`, `qc-edict`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-map`, `qc-progs`, `qc-engines`, `qs-tests`, `qc-vm`, `xtask` |
| `fn qc_preproc::fte_options::CommandLine::flags` | `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-sim` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-sim` |
| `fn qc_preproc::fte_options::CommandLine::new` | `qc-sema`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `xtask`, `fuzz` |
| `fn qc_preproc::fte_options::CommandLine::warnings` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_preproc::fte_pragma::FlagSettings::is_on` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_preproc::fte_pragma::FlagSettings::keywords` | `qc-sema`, `qc-syntax` | kept public: used by `qc-sema`, `qc-syntax` |
| `fn qc_preproc::fte_pragma::FlagSettings::lex_flags` | `qc-codegen`, `qc-sema`, `qc-syntax` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax` |
| `fn qc_preproc::fte_pragma::FlagSettings::priorities` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_preproc::fte_tables::TargetFormat::supports_extern` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_preproc::fte_tables::is_opcode_name` | `qc-syntax` | kept public: used by `qc-syntax` |
| `fn qc_preproc::fte_warning::FteWarning::default_action` | none | kept public: named in a public signature a dependent uses |
| `fn qc_preproc::fte_warning::FteWarning::of_code` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_preproc::fte_warning::WarningSettings::reported_code` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_preproc::normalize` | `qc-ide`, `qc-sema`, `qs-workbench`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qs-sim`, `qc-project`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-sema`, `qs-workbench`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qs-sim`, `qc-project`, `lsp`, `xtask` |
| `fn qc_preproc::source::FileId::index` | `qc-ide`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qs-differential`, `qs-bench`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qs-differential`, `qs-bench`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask`, `fuzz` |
| `fn qc_preproc::source::Loader::load` | `qc-ide`, `qs-vfs`, `qs-render`, `qc-project`, `xtask` | kept public: used by `qc-ide`, `qs-vfs`, `qs-render`, `qc-project`, `xtask` |
| `fn qc_preproc::source::Loader::stored_name` | `qc-ide`, `qc-project` | kept public: used by `qc-ide`, `qc-project` |
| `fn qc_preproc::source::MemoryLoader::case_insensitive` | `qc-sema`, `qc-syntax`, `qc-project` | kept public: used by `qc-sema`, `qc-syntax`, `qc-project` |
| `fn qc_preproc::source::MemoryLoader::insert` | `qc-ide`, `qc-codegen`, `qs-wav`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qs-wav`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_preproc::source::MemoryLoader::new` | `qc-ide`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-engines`, `qc-project`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-engines`, `qc-project`, `lsp`, `xtask`, `fuzz` |
| `fn qc_preproc::source::SourceFile::file_span` | `qc-sema`, `qc-syntax` | kept public: used by `qc-sema`, `qc-syntax` |
| `fn qc_preproc::source::SourceMap::add` | `qc-syntax`, `qs-render`, `desktop`, `xtask` | kept public: used by `qc-syntax`, `qs-render`, `desktop`, `xtask` |
| `fn qc_preproc::source::SourceMap::file` | `qc-sema`, `qs-settings`, `qs-vfs`, `qc-syntax`, `qs-install`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `qs-settings`, `qs-vfs`, `qc-syntax`, `qs-install`, `xtask`, `fuzz` |
| `fn qc_preproc::source::SourceMap::files` | `qc-ide`, `qs-vfs`, `qs-workbench` | kept public: used by `qc-ide`, `qs-vfs`, `qs-workbench` |
| `fn qc_preproc::source::SourceMap::line` | `qc-sema`, `qs-workbench`, `qc-lexer`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-sema`, `qs-workbench`, `qc-lexer`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask` |
| `fn qc_preproc::source::SourceMap::text` | `qc-ide`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `mod qc_preproc::cond` | `qc-sema`, `qc-syntax`, `qc-project`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `qc-syntax`, `qc-project`, `xtask`, `fuzz` |
| `struct qc_preproc::Diagnostic` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-map`, `qs-tests`, `qc-project`, `lsp` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-map`, `qs-tests`, `qc-project`, `lsp` |
| `struct qc_preproc::ExpansionId` | none | kept public: named in a public signature a dependent uses |
| `struct qc_preproc::FileOutput` | `qc-syntax`, `fuzz` | kept public: used by `qc-syntax`, `fuzz` |
| `struct qc_preproc::PpToken` | `qc-sema`, `qc-syntax` | kept public: used by `qc-sema`, `qc-syntax` |
| `struct qc_preproc::Preprocessor` | `qc-sema`, `qc-syntax`, `qc-project`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `qc-syntax`, `qc-project`, `xtask`, `fuzz` |
| `struct qc_preproc::compiler_flags::CompilerFlags` | `qc-codegen`, `qc-sema`, `qc-syntax` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax` |
| `struct qc_preproc::compiler_flags::FlagEdit` | `qc-sema`, `qc-syntax` | kept public: used by `qc-sema`, `qc-syntax` |
| `struct qc_preproc::cond::CondStack` | `qc-project` | kept public: used by `qc-project` |
| `struct qc_preproc::fte_directive::graph::FteFileReader` | `qc-project` | kept public: used by `qc-project` |
| `struct qc_preproc::fte_directive::graph::ModuleState` | `qc-ide`, `qc-project` | kept public: used by `qc-ide`, `qc-project` |
| `struct qc_preproc::fte_keywords::KeywordEdit` | `qc-sema`, `qc-syntax` | kept public: used by `qc-sema`, `qc-syntax` |
| `struct qc_preproc::fte_keywords::Keywords` | `qc-sema`, `qc-syntax`, `qc-lexer`, `desktop` | kept public: used by `qc-sema`, `qc-syntax`, `qc-lexer`, `desktop` |
| `struct qc_preproc::fte_opcodes::OpcodeTarget` | `qc-sema` | kept public: used by `qc-sema` |
| `struct qc_preproc::fte_options::CommandLine` | `qc-sema`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `xtask`, `fuzz` |
| `struct qc_preproc::fte_pragma::FlagSettings` | `qc-sema` | kept public: used by `qc-sema` |
| `struct qc_preproc::fte_warning::WarningSettings` | `qc-sema` | kept public: used by `qc-sema` |
| `struct qc_preproc::source::FileId` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-differential`, `qs-bench`, `qc-engines`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-differential`, `qs-bench`, `qc-engines`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` |
| `struct qc_preproc::source::FileSpan` | `qc-sema` | kept public: used by `qc-sema` |
| `struct qc_preproc::source::FsLoader` | `qs-workbench`, `qc-project`, `desktop`, `lsp`, `xtask` | kept public: used by `qs-workbench`, `qc-project`, `desktop`, `lsp`, `xtask` |
| `struct qc_preproc::source::MemoryLoader` | `qc-ide`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-engines`, `qc-project`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-engines`, `qc-project`, `lsp`, `xtask`, `fuzz` |
| `struct qc_preproc::source::SourceFile` | `qc-syntax`, `qc-lexer`, `qc-project` | kept public: used by `qc-syntax`, `qc-lexer`, `qc-project` |
| `struct qc_preproc::source::SourceMap` | `fuzz` | kept public: used by `fuzz` |
| `trait qc_preproc::source::Loader` | `qc-ide`, `qc-sema`, `qs-workbench`, `qc-project`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-sema`, `qs-workbench`, `qc-project`, `desktop`, `lsp`, `xtask` |
| `use qc_preproc::CommandLine = fte_options::CommandLine` | `qc-sema`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `xtask`, `fuzz` |
| `use qc_preproc::CompilerFlags = compiler_flags::CompilerFlags` | `qc-codegen`, `qc-sema`, `qc-syntax` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax` |
| `use qc_preproc::Enabled = fte_keywords::Enabled` | `qc-sema`, `qc-syntax`, `qs-editor-view` | kept public: used by `qc-sema`, `qc-syntax`, `qs-editor-view` |
| `use qc_preproc::FileId = source::FileId` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-differential`, `qs-bench`, `qc-engines`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-differential`, `qs-bench`, `qc-engines`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` |
| `use qc_preproc::FileSpan = source::FileSpan` | `qc-sema` | kept public: used by `qc-sema` |
| `use qc_preproc::FileStart = fte_directive::FileStart` | `qc-project` | kept public: used by `qc-project` |
| `use qc_preproc::FlagEdit = compiler_flags::FlagEdit` | `qc-sema`, `qc-syntax` | kept public: used by `qc-sema`, `qc-syntax` |
| `use qc_preproc::FlagSettings = fte_pragma::FlagSettings` | `qc-sema` | kept public: used by `qc-sema` |
| `use qc_preproc::FsLoader = source::FsLoader` | `qs-workbench`, `qc-project`, `desktop`, `lsp`, `xtask` | kept public: used by `qs-workbench`, `qc-project`, `desktop`, `lsp`, `xtask` |
| `use qc_preproc::FteEvent = fte_directive::FteEvent` | `qc-project` | kept public: used by `qc-project` |
| `use qc_preproc::FteFileReader = fte_directive::FteFileReader` | `qc-project` | kept public: used by `qc-project` |
| `use qc_preproc::FteFlag = fte_tables::FteFlag` | `qc-codegen`, `qc-sema`, `qc-syntax` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax` |
| `use qc_preproc::FteWarning = fte_warning::FteWarning` | `qc-sema` | kept public: used by `qc-sema` |
| `use qc_preproc::Keyword = fte_keywords::Keyword` | `qc-ide`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qc-project`, `desktop` | kept public: used by `qc-ide`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qc-project`, `desktop` |
| `use qc_preproc::KeywordEdit = fte_keywords::KeywordEdit` | `qc-sema`, `qc-syntax` | kept public: used by `qc-sema`, `qc-syntax` |
| `use qc_preproc::Keywords = fte_keywords::Keywords` | `qc-sema`, `qc-syntax`, `qc-lexer`, `desktop` | kept public: used by `qc-sema`, `qc-syntax`, `qc-lexer`, `desktop` |
| `use qc_preproc::Loader = source::Loader` | `qc-ide`, `qc-sema`, `qs-workbench`, `qc-project`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-sema`, `qs-workbench`, `qc-project`, `desktop`, `lsp`, `xtask` |
| `use qc_preproc::MemoryLoader = source::MemoryLoader` | `qc-ide`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-engines`, `qc-project`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-engines`, `qc-project`, `lsp`, `xtask`, `fuzz` |
| `use qc_preproc::ModuleState = fte_directive::ModuleState` | `qc-ide`, `qc-project` | kept public: used by `qc-ide`, `qc-project` |
| `use qc_preproc::Op = fte_ops::Op` | `qc-codegen`, `qc-sema`, `qs-differential`, `qs-editor-view`, `qs-commands` | kept public: used by `qc-codegen`, `qc-sema`, `qs-differential`, `qs-editor-view`, `qs-commands` |
| `use qc_preproc::OpcodeTarget = fte_opcodes::OpcodeTarget` | `qc-sema` | kept public: used by `qc-sema` |
| `use qc_preproc::PragmaDefault = compiler_flags::PragmaDefault` | `qc-sema` | kept public: used by `qc-sema` |
| `use qc_preproc::Priorities = compiler_flags::Priorities` | `qc-syntax` | kept public: used by `qc-syntax` |
| `use qc_preproc::SourceFile = source::SourceFile` | `qc-syntax`, `qc-lexer`, `qc-project` | kept public: used by `qc-syntax`, `qc-lexer`, `qc-project` |
| `use qc_preproc::SourceMap = source::SourceMap` | `fuzz` | kept public: used by `fuzz` |
| `use qc_preproc::TargetFormat = fte_tables::TargetFormat` | `qc-sema` | kept public: used by `qc-sema` |
| `use qc_preproc::TextOrigin = source::TextOrigin` | none | kept public: named in a public signature a dependent uses |
| `use qc_preproc::WarningAction = fte_warning::WarningAction` | none | kept public: named in a public signature a dependent uses |
| `use qc_preproc::WarningSettings = fte_warning::WarningSettings` | `qc-sema` | kept public: used by `qc-sema` |
| `use qc_preproc::com_parse = fte_directive::com_parse` | `qc-project` | kept public: used by `qc-project` |
| `use qc_preproc::fte_directive::FileStart = graph::FileStart` | `qc-project` | kept public: used by `qc-project` |
| `use qc_preproc::fte_directive::FteEvent = graph::FteEvent` | `qc-project` | kept public: used by `qc-project` |
| `use qc_preproc::fte_directive::FteFileReader = graph::FteFileReader` | `qc-project` | kept public: used by `qc-project` |
| `use qc_preproc::fte_directive::ModuleState = graph::ModuleState` | `qc-ide`, `qc-project` | kept public: used by `qc-ide`, `qc-project` |
| `use qc_preproc::fte_directive::qcc_load_as = load::qcc_load_as` | `qc-project` | kept public: used by `qc-project` |
| `use qc_preproc::fte_directive::qcc_load_in_place = load::qcc_load_in_place` | `qc-syntax` | kept public: used by `qc-syntax` |
| `use qc_preproc::fte_directive::skip_script_line = load::skip_script_line` | `qc-syntax` | kept public: used by `qc-syntax` |
| `use qc_preproc::fte_directive_end = fte_line::fte_directive_end` | `qc-syntax` | kept public: used by `qc-syntax` |
| `use qc_preproc::include_dir_name = fte_directive::include_dir_name` | `qc-project` | kept public: used by `qc-project` |
| `use qc_preproc::is_opcode_name = fte_tables::is_opcode_name` | `qc-syntax` | kept public: used by `qc-syntax` |
| `use qc_preproc::join_paths = fte_directive::join_paths` | `qc-project` | kept public: used by `qc-project` |
| `use qc_preproc::pragma_include_dir = fte_directive::pragma_include_dir` | `qc-project` | kept public: used by `qc-project` |
| `use qc_preproc::qcc_load_as = fte_directive::qcc_load_as` | `qc-project` | kept public: used by `qc-project` |
| `use qc_preproc::qcc_load_in_place = fte_directive::qcc_load_in_place` | `qc-syntax` | kept public: used by `qc-syntax` |
| `use qc_preproc::relex_from = fte_line::relex_from` | `qc-syntax` | kept public: used by `qc-syntax` |
| `use qc_preproc::skip_script_line = fte_directive::skip_script_line` | `qc-syntax` | kept public: used by `qc-syntax` |

Items that left the root (15): `const qc_preproc::FTE_MAX_FRAMES`, `fn qc_preproc::ExpansionId::index`, `fn qc_preproc::Preprocessor::expansions`, `fn qc_preproc::cond::CondStack::flip`, `fn qc_preproc::cond::CondStack::into_open`, `fn qc_preproc::fte_directive::load::qcc_load_file`, `fn qc_preproc::fte_keywords::Keyword::of_flag`, `fn qc_preproc::fte_ops::Op::named_in_any_case`, `fn qc_preproc::fte_pragma::FlagSettings::compiler_flags`, `fn qc_preproc::fte_warning::FteWarning::for_name`, `fn qc_preproc::fte_warning::WarningSettings::action`, `fn qc_preproc::fte_warning::WarningSettings::after_pragma`, `struct qc_preproc::Expansion`, `use qc_preproc::fte_directive::qcc_load_file = load::qcc_load_file`, `use qc_preproc::qcc_load_file = fte_directive::qcc_load_file`. Each is `pub(crate)` now, under `#[cfg(test)]` where only the crate's tests read it; `qc_sema::nav::ModuleIndex::references_at` is removed.

## `qc-progs` (L0)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qc_progs::crc::PROGS_CRC_INIT` | `qs-pak` | kept public: used by `qs-pak` |
| `const qc_progs::crc::PROGS_CRC_POLY` | `qs-pak` | kept public: used by `qs-pak` |
| `const qc_progs::crc::PROGS_CRC_XOR` | none | kept public: the progs.dat format's model |
| `const qc_progs::limits::LIMIT_LOCALSTACK_SIZE` | `qc-vm` | kept public: used by `qc-vm` |
| `const qc_progs::limits::LIMIT_MAX_DATAGRAM` | `qc-builtins`, `xtask` | kept public: used by `qc-builtins`, `xtask` |
| `const qc_progs::limits::LIMIT_MAX_DEF_SLOT` | `xtask` | kept public: used by `xtask` |
| `const qc_progs::limits::LIMIT_MAX_DEF_TYPE` | none | kept public: the progs.dat format's model |
| `const qc_progs::limits::LIMIT_MAX_EDICTS` | `qc-edict`, `qs-differential`, `qs-sim` | kept public: used by `qc-edict`, `qs-differential`, `qs-sim` |
| `const qc_progs::limits::LIMIT_MAX_ENT_LEAFS` | none | kept public: the progs.dat format's model |
| `const qc_progs::limits::LIMIT_MAX_LIGHTSTYLES` | `qc-builtins` | kept public: used by `qc-builtins` |
| `const qc_progs::limits::LIMIT_MAX_MODELS` | `qc-builtins` | kept public: used by `qc-builtins` |
| `const qc_progs::limits::LIMIT_MAX_MSGLEN` | `qc-builtins` | kept public: used by `qc-builtins` |
| `const qc_progs::limits::LIMIT_MAX_OPERAND_SLOT` | none | kept public: the progs.dat format's model |
| `const qc_progs::limits::LIMIT_MAX_QPATH` | none | kept public: the progs.dat format's model |
| `const qc_progs::limits::LIMIT_MAX_SOUNDS` | `qc-builtins` | kept public: used by `qc-builtins` |
| `const qc_progs::limits::LIMIT_MAX_STACK_DEPTH` | `qc-vm` | kept public: used by `qc-vm` |
| `const qc_progs::limits::LIMIT_NUM_BUILTINS` | `qc-builtins`, `qc-vm` | kept public: used by `qc-builtins`, `qc-vm` |
| `const qc_progs::limits::LIMIT_QCC_MAX_DATA_PATH` | none | kept public: the progs.dat format's model |
| `const qc_progs::limits::LIMIT_QCC_MAX_ERRORS` | none | kept public: the progs.dat format's model |
| `const qc_progs::limits::LIMIT_QCC_MAX_FIELDS` | none | kept public: the progs.dat format's model |
| `const qc_progs::limits::LIMIT_QCC_MAX_FRAMES` | `qc-preproc` | kept public: used by `qc-preproc` |
| `const qc_progs::limits::LIMIT_QCC_MAX_FUNCTIONS` | none | kept public: the progs.dat format's model |
| `const qc_progs::limits::LIMIT_QCC_MAX_GLOBALS` | none | kept public: the progs.dat format's model |
| `const qc_progs::limits::LIMIT_QCC_MAX_NAME` | `qc-syntax` | kept public: used by `qc-syntax` |
| `const qc_progs::limits::LIMIT_QCC_MAX_PRECACHE` | none | kept public: the progs.dat format's model |
| `const qc_progs::limits::LIMIT_QCC_MAX_REGS` | none | kept public: the progs.dat format's model |
| `const qc_progs::limits::LIMIT_QCC_MAX_STATEMENTS` | none | kept public: the progs.dat format's model |
| `const qc_progs::limits::LIMIT_QCC_MAX_STRINGS` | none | kept public: the progs.dat format's model |
| `const qc_progs::limits::LIMIT_RUNAWAY` | `qc-vm` | kept public: used by `qc-vm` |
| `const qc_progs::opcode::PROGS_OPCODE_COUNT` | none | kept public: the progs.dat format's model |
| `const qc_progs::records::PROGS_DEF_SAVEGLOBAL` | `qc-codegen` | kept public: used by `qc-codegen` |
| `const qc_progs::records::PROGS_DEF_SIZE` | none | kept public: the progs.dat format's model |
| `const qc_progs::records::PROGS_FTE32_DEF_SIZE` | none | kept public: the progs.dat format's model |
| `const qc_progs::records::PROGS_FTE32_STATEMENT_SIZE` | none | kept public: the progs.dat format's model |
| `const qc_progs::records::PROGS_FTE_HEADER_SIZE` | none | kept public: the progs.dat format's model |
| `const qc_progs::records::PROGS_FTE_LINE_SIZE` | none | kept public: the progs.dat format's model |
| `const qc_progs::records::PROGS_FTE_SECONDARY16` | none | kept public: the progs.dat format's model |
| `const qc_progs::records::PROGS_FTE_SECONDARY32` | none | kept public: the progs.dat format's model |
| `const qc_progs::records::PROGS_FTE_TYPE_SIZE` | none | kept public: the progs.dat format's model |
| `const qc_progs::records::PROGS_FUNCTION_SIZE` | none | kept public: the progs.dat format's model |
| `const qc_progs::records::PROGS_GLOBAL_SIZE` | none | kept public: the progs.dat format's model |
| `const qc_progs::records::PROGS_HEADER_CRC` | `qs-differential`, `xtask` | kept public: used by `qs-differential`, `xtask` |
| `const qc_progs::records::PROGS_HEADER_SIZE` | `xtask` | kept public: used by `xtask` |
| `const qc_progs::records::PROGS_STATEMENT_SIZE` | none | kept public: the progs.dat format's model |
| `const qc_progs::records::PROGS_STRING_ALIGN` | none | kept public: the progs.dat format's model |
| `const qc_progs::records::PROGS_VERSION` | `qs-differential`, `xtask` | kept public: used by `qs-differential`, `xtask` |
| `const qc_progs::records::PROGS_VERSION_FTE` | none | kept public: the progs.dat format's model |
| `enum qc_progs::TableId` | none | kept public: the progs.dat format's model |
| `enum qc_progs::read::ReadError` | `qc-lno`, `qs-differential` | kept public: used by `qc-lno`, `qs-differential` |
| `enum qc_progs::read::StringSite` | none | kept public: the progs.dat format's model |
| `enum qc_progs::records::Format` | `qc-codegen`, `qs-wav`, `qs-tests`, `xtask` | kept public: used by `qc-codegen`, `qs-wav`, `qs-tests`, `xtask` |
| `enum qc_progs::write::WriteError` | `qc-codegen`, `qc-lno`, `fuzz` | kept public: used by `qc-codegen`, `qc-lno`, `fuzz` |
| `fn qc_progs::crc::crc_block` | `qc-codegen`, `qs-vfs`, `qs-pak`, `qs-bsp`, `xtask` | kept public: used by `qc-codegen`, `qs-vfs`, `qs-pak`, `qs-bsp`, `xtask` |
| `fn qc_progs::crc::crc_init` | none | kept public: the progs.dat format's model |
| `fn qc_progs::crc::crc_process_byte` | `qs-vfs` | kept public: used by `qs-vfs` |
| `fn qc_progs::crc::crc_value` | none | kept public: the progs.dat format's model |
| `fn qc_progs::read::Progs::bodyless_functions` | none | kept public: the progs.dat format's model |
| `fn qc_progs::read::Progs::bytes` | `qc-ide`, `qc-preproc`, `qc-trace`, `qs-vfs`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-install`, `qc-engines`, `qs-tests`, `qc-project`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-trace`, `qs-vfs`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-install`, `qc-engines`, `qs-tests`, `qc-project`, `xtask` |
| `fn qc_progs::read::Progs::ed_field_at_ofs` | none | kept public: the progs.dat format's model |
| `fn qc_progs::read::Progs::ed_find_field` | `qs-differential`, `qs-sim` | kept public: used by `qs-differential`, `qs-sim` |
| `fn qc_progs::read::Progs::ed_find_function` | `qs-differential`, `qs-sim` | kept public: used by `qs-differential`, `qs-sim` |
| `fn qc_progs::read::Progs::ed_find_global` | none | kept public: the progs.dat format's model |
| `fn qc_progs::read::Progs::ed_global_at_ofs` | none | kept public: the progs.dat format's model |
| `fn qc_progs::read::Progs::field_defs` | `qs-differential`, `xtask` | kept public: used by `qs-differential`, `xtask` |
| `fn qc_progs::read::Progs::file_crc` | `qs-differential`, `qc-vm` | kept public: used by `qs-differential`, `qc-vm` |
| `fn qc_progs::read::Progs::format` | `qc-preproc`, `qc-sema` | kept public: used by `qc-preproc`, `qc-sema` |
| `fn qc_progs::read::Progs::fte_header` | none | kept public: the progs.dat format's model |
| `fn qc_progs::read::Progs::functions` | `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm`, `xtask` | kept public: used by `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm`, `xtask` |
| `fn qc_progs::read::Progs::global_defs` | `qs-differential`, `xtask` | kept public: used by `qs-differential`, `xtask` |
| `fn qc_progs::read::Progs::globals` | `qc-trace`, `qs-differential`, `qc-vm`, `xtask` | kept public: used by `qc-trace`, `qs-differential`, `qc-vm`, `xtask` |
| `fn qc_progs::read::Progs::header` | `qc-codegen`, `qc-trace`, `qs-pak`, `qc-edict`, `qs-differential`, `xtask` | kept public: used by `qc-codegen`, `qc-trace`, `qs-pak`, `qc-edict`, `qs-differential`, `xtask` |
| `fn qc_progs::read::Progs::layout` | `qc-sema`, `qc-edict`, `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm`, `desktop` | kept public: used by `qc-sema`, `qc-edict`, `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm`, `desktop` |
| `fn qc_progs::read::Progs::line_numbers` | none | kept public: the progs.dat format's model |
| `fn qc_progs::read::Progs::read` | `qc-sema`, `qs-vfs`, `qc-edict`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask` | kept public: used by `qc-sema`, `qs-vfs`, `qc-edict`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask` |
| `fn qc_progs::read::Progs::read_with` | `qc-codegen`, `qc-vm`, `xtask`, `fuzz` | kept public: used by `qc-codegen`, `qc-vm`, `xtask`, `fuzz` |
| `fn qc_progs::read::Progs::statements` | `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qc-vm`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qc-vm`, `xtask` |
| `fn qc_progs::read::Progs::string` | `qc-trace`, `qc-lexer`, `qs-differential`, `qs-install`, `qc-builtins`, `qc-vm`, `xtask` | kept public: used by `qc-trace`, `qc-lexer`, `qs-differential`, `qs-install`, `qc-builtins`, `qc-vm`, `xtask` |
| `fn qc_progs::read::Progs::strings` | `qs-differential`, `qc-vm` | kept public: used by `qs-differential`, `qc-vm` |
| `fn qc_progs::read::Table::as_bytes` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-map`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-map`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_progs::read::Table::get` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-map`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-map`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_progs::read::Table::is_empty` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_progs::read::Table::iter` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-porting-example`, `qs-map`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-porting-example`, `qs-map`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_progs::read::Table::len` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-porting-example`, `qs-map`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-porting-example`, `qs-map`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_progs::records::Def::def_type` | `qc-codegen`, `qs-differential`, `qs-sim`, `xtask` | kept public: used by `qc-codegen`, `qs-differential`, `qs-sim`, `xtask` |
| `fn qc_progs::records::Def::is_save_global` | none | kept public: the progs.dat format's model |
| `fn qc_progs::records::Format::def_size` | none | kept public: the progs.dat format's model |
| `fn qc_progs::records::Format::header_size` | none | kept public: the progs.dat format's model |
| `fn qc_progs::records::Format::secondary_version` | none | kept public: the progs.dat format's model |
| `fn qc_progs::records::Format::statement_size` | none | kept public: the progs.dat format's model |
| `fn qc_progs::records::Format::version` | `qs-workbench`, `desktop` | kept public: used by `qs-workbench`, `desktop` |
| `fn qc_progs::records::Function::builtin_number` | `qc-vm` | kept public: used by `qc-vm` |
| `fn qc_progs::records::Record::decode` | none | kept public: the progs.dat format's model |
| `fn qc_progs::records::Record::encode` | none | kept public: the progs.dat format's model |
| `fn qc_progs::records::Record::size` | `qs-mdl`, `qs-vfs`, `qs-render`, `qs-editor-view`, `qs-sim`, `desktop`, `fuzz` | kept public: used by `qs-mdl`, `qs-vfs`, `qs-render`, `qs-editor-view`, `qs-sim`, `desktop`, `fuzz` |
| `fn qc_progs::write::Progs::to_file` | `xtask`, `fuzz` | kept public: used by `xtask`, `fuzz` |
| `fn qc_progs::write::write` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qc-lno`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-map`, `qs-editor-view`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qc-lno`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-map`, `qs-editor-view`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `mod qc_progs::crc` | `qc-codegen`, `qc-trace`, `qs-vfs`, `qs-pak`, `qs-bsp`, `xtask` | kept public: used by `qc-codegen`, `qc-trace`, `qs-vfs`, `qs-pak`, `qs-bsp`, `xtask` |
| `mod qc_progs::limits` | `qc-preproc`, `qs-mdl`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-commands`, `qc-vm`, `xtask` | kept public: used by `qc-preproc`, `qs-mdl`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-commands`, `qc-vm`, `xtask` |
| `mod qc_progs::opcode` | `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-tests`, `qc-vm`, `xtask` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-tests`, `qc-vm`, `xtask` |
| `struct qc_progs::Layout` | `qc-sema`, `qc-edict`, `qs-layout`, `qc-vm`, `desktop`, `xtask` | kept public: used by `qc-sema`, `qc-edict`, `qs-layout`, `qc-vm`, `desktop`, `xtask` |
| `struct qc_progs::read::Progs` | `qc-preproc`, `qc-codegen`, `qs-vfs`, `qc-edict`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-codegen`, `qs-vfs`, `qc-edict`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm`, `xtask`, `fuzz` |
| `struct qc_progs::read::ReadOptions` | `qc-codegen`, `qc-vm`, `xtask`, `fuzz` | kept public: used by `qc-codegen`, `qc-vm`, `xtask`, `fuzz` |
| `struct qc_progs::read::Table` | `qc-trace`, `qs-settings`, `qs-differential`, `qc-vm`, `xtask` | kept public: used by `qc-trace`, `qs-settings`, `qs-differential`, `qc-vm`, `xtask` |
| `struct qc_progs::read::TableIter` | none | kept public: the progs.dat format's model |
| `struct qc_progs::records::Def` | `qc-codegen`, `qc-sema`, `qs-differential`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qs-differential`, `xtask` |
| `struct qc_progs::records::FteHeader` | none | kept public: the progs.dat format's model |
| `struct qc_progs::records::Function` | `qc-ide`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-edict`, `qc-diag`, `qs-differential`, `qc-builtins`, `qc-engines`, `qs-tests`, `qs-sim`, `qc-vm`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-edict`, `qc-diag`, `qs-differential`, `qc-builtins`, `qc-engines`, `qs-tests`, `qs-sim`, `qc-vm`, `desktop`, `lsp`, `xtask` |
| `struct qc_progs::records::Header` | `qc-trace`, `qs-pak`, `qc-lno`, `qs-differential`, `qc-vm`, `lsp`, `xtask` | kept public: used by `qc-trace`, `qs-pak`, `qc-lno`, `qs-differential`, `qc-vm`, `lsp`, `xtask` |
| `struct qc_progs::records::Statement` | `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-lno`, `qc-diag`, `qs-differential`, `qc-builtins`, `qs-tests`, `qc-vm`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-lno`, `qc-diag`, `qs-differential`, `qc-builtins`, `qs-tests`, `qc-vm`, `xtask` |
| `struct qc_progs::write::FteParts` | none | kept public: the progs.dat format's model |
| `struct qc_progs::write::ProgsFile` | `qc-codegen`, `qc-edict`, `qc-builtins`, `qs-tests`, `xtask` | kept public: used by `qc-codegen`, `qc-edict`, `qc-builtins`, `qs-tests`, `xtask` |
| `trait qc_progs::records::Record` | `qc-trace`, `qc-sema`, `qs-log`, `qs-differential`, `qc-vm`, `xtask` | kept public: used by `qc-trace`, `qc-sema`, `qs-log`, `qs-differential`, `qc-vm`, `xtask` |
| `trait qc_progs::records::sealed::Sealed` | `qs-map` | kept public: used by `qs-map` |
| `use qc_progs::Def = records::Def` | `qc-codegen`, `qc-sema`, `qs-differential`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qs-differential`, `xtask` |
| `use qc_progs::Format = records::Format` | `qc-codegen`, `qs-wav`, `qs-tests`, `xtask` | kept public: used by `qc-codegen`, `qs-wav`, `qs-tests`, `xtask` |
| `use qc_progs::FteHeader = records::FteHeader` | none | kept public: the progs.dat format's model |
| `use qc_progs::FteParts = write::FteParts` | none | kept public: the progs.dat format's model |
| `use qc_progs::Function = records::Function` | `qc-ide`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-edict`, `qc-diag`, `qs-differential`, `qc-builtins`, `qc-engines`, `qs-tests`, `qs-sim`, `qc-vm`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-edict`, `qc-diag`, `qs-differential`, `qc-builtins`, `qc-engines`, `qs-tests`, `qs-sim`, `qc-vm`, `desktop`, `lsp`, `xtask` |
| `use qc_progs::Header = records::Header` | `qc-trace`, `qs-pak`, `qc-lno`, `qs-differential`, `qc-vm`, `lsp`, `xtask` | kept public: used by `qc-trace`, `qs-pak`, `qc-lno`, `qs-differential`, `qc-vm`, `lsp`, `xtask` |
| `use qc_progs::PROGS_DEF_SAVEGLOBAL = records::PROGS_DEF_SAVEGLOBAL` | `qc-codegen` | kept public: used by `qc-codegen` |
| `use qc_progs::PROGS_DEF_SIZE = records::PROGS_DEF_SIZE` | none | kept public: the progs.dat format's model |
| `use qc_progs::PROGS_FTE32_DEF_SIZE = records::PROGS_FTE32_DEF_SIZE` | none | kept public: the progs.dat format's model |
| `use qc_progs::PROGS_FTE32_STATEMENT_SIZE = records::PROGS_FTE32_STATEMENT_SIZE` | none | kept public: the progs.dat format's model |
| `use qc_progs::PROGS_FTE_HEADER_SIZE = records::PROGS_FTE_HEADER_SIZE` | none | kept public: the progs.dat format's model |
| `use qc_progs::PROGS_FTE_LINE_SIZE = records::PROGS_FTE_LINE_SIZE` | none | kept public: the progs.dat format's model |
| `use qc_progs::PROGS_FTE_SECONDARY16 = records::PROGS_FTE_SECONDARY16` | none | kept public: the progs.dat format's model |
| `use qc_progs::PROGS_FTE_SECONDARY32 = records::PROGS_FTE_SECONDARY32` | none | kept public: the progs.dat format's model |
| `use qc_progs::PROGS_FTE_TYPE_SIZE = records::PROGS_FTE_TYPE_SIZE` | none | kept public: the progs.dat format's model |
| `use qc_progs::PROGS_FUNCTION_SIZE = records::PROGS_FUNCTION_SIZE` | none | kept public: the progs.dat format's model |
| `use qc_progs::PROGS_GLOBAL_SIZE = records::PROGS_GLOBAL_SIZE` | none | kept public: the progs.dat format's model |
| `use qc_progs::PROGS_HEADER_CRC = records::PROGS_HEADER_CRC` | `qs-differential`, `xtask` | kept public: used by `qs-differential`, `xtask` |
| `use qc_progs::PROGS_HEADER_SIZE = records::PROGS_HEADER_SIZE` | `xtask` | kept public: used by `xtask` |
| `use qc_progs::PROGS_STATEMENT_SIZE = records::PROGS_STATEMENT_SIZE` | none | kept public: the progs.dat format's model |
| `use qc_progs::PROGS_STRING_ALIGN = records::PROGS_STRING_ALIGN` | none | kept public: the progs.dat format's model |
| `use qc_progs::PROGS_VERSION = records::PROGS_VERSION` | `qs-differential`, `xtask` | kept public: used by `qs-differential`, `xtask` |
| `use qc_progs::PROGS_VERSION_FTE = records::PROGS_VERSION_FTE` | none | kept public: the progs.dat format's model |
| `use qc_progs::Progs = read::Progs` | `qc-preproc`, `qc-codegen`, `qs-vfs`, `qc-edict`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-codegen`, `qs-vfs`, `qc-edict`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm`, `xtask`, `fuzz` |
| `use qc_progs::ProgsFile = write::ProgsFile` | `qc-codegen`, `qc-edict`, `qc-builtins`, `qs-tests`, `xtask` | kept public: used by `qc-codegen`, `qc-edict`, `qc-builtins`, `qs-tests`, `xtask` |
| `use qc_progs::ReadError = read::ReadError` | `qc-lno`, `qs-differential` | kept public: used by `qc-lno`, `qs-differential` |
| `use qc_progs::ReadOptions = read::ReadOptions` | `qc-codegen`, `qc-vm`, `xtask`, `fuzz` | kept public: used by `qc-codegen`, `qc-vm`, `xtask`, `fuzz` |
| `use qc_progs::Record = records::Record` | `qc-trace`, `qc-sema`, `qs-log`, `qs-differential`, `qc-vm`, `xtask` | kept public: used by `qc-trace`, `qc-sema`, `qs-log`, `qs-differential`, `qc-vm`, `xtask` |
| `use qc_progs::Statement = records::Statement` | `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-lno`, `qc-diag`, `qs-differential`, `qc-builtins`, `qs-tests`, `qc-vm`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-lno`, `qc-diag`, `qs-differential`, `qc-builtins`, `qs-tests`, `qc-vm`, `xtask` |
| `use qc_progs::StringSite = read::StringSite` | none | kept public: the progs.dat format's model |
| `use qc_progs::Table = read::Table` | `qc-trace`, `qs-settings`, `qs-differential`, `qc-vm`, `xtask` | kept public: used by `qc-trace`, `qs-settings`, `qs-differential`, `qc-vm`, `xtask` |
| `use qc_progs::TableIter = read::TableIter` | none | kept public: the progs.dat format's model |
| `use qc_progs::WriteError = write::WriteError` | `qc-codegen`, `qc-lno`, `fuzz` | kept public: used by `qc-codegen`, `qc-lno`, `fuzz` |
| `use qc_progs::limits::LIMIT_MAX_PARMS = qc_types::LIMIT_MAX_PARMS` | `qc-types`, `qc-syntax` | kept public: used by `qc-types`, `qc-syntax` |
| `use qc_progs::write = write::write` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qc-lno`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-map`, `qs-editor-view`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qc-lno`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-map`, `qs-editor-view`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |

## `qc-project` (L2)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qc_project::PRJ_MANIFEST` | `xtask` | kept public: used by `xtask` |
| `const qc_project::Target::ALL` | `qc-ide`, `qc-preproc`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `const qc_project::graph::PRJ_MAX_FILES` | `fuzz` | kept public: used by `fuzz` |
| `enum qc_project::Dialect` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qs-differential`, `qc-engines`, `qs-tests`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qs-differential`, `qc-engines`, `qs-tests`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `enum qc_project::ModuleKind` | `qc-ide`, `qc-sema`, `qc-engines`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-sema`, `qc-engines`, `xtask`, `fuzz` |
| `enum qc_project::ProjectError` | `qc-ide`, `qs-workbench`, `lsp` | kept public: used by `qc-ide`, `qs-workbench`, `lsp` |
| `enum qc_project::Target` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-edict`, `qc-lexer`, `qc-engines`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-edict`, `qc-lexer`, `qc-engines`, `desktop`, `lsp`, `xtask` |
| `enum qc_project::graph::Form` | `qc-sema`, `qc-syntax`, `xtask` | kept public: used by `qc-sema`, `qc-syntax`, `xtask` |
| `enum qc_project::graph::Via` | `qc-sema`, `xtask` | kept public: used by `qc-sema`, `xtask` |
| `enum qc_project::manifest::ManifestError` | none | kept public: named in a public signature a dependent uses |
| `fn qc_project::ModuleKind::as_str` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `desktop`, `lsp`, `xtask` |
| `fn qc_project::ModuleKind::output_file` | none | kept public: named in a public signature a dependent uses |
| `fn qc_project::graph::Define::parse` | `qc-ide`, `qc-sema`, `qs-settings`, `qs-mdl`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-sema`, `qs-settings`, `qs-mdl`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask` |
| `fn qc_project::graph::GraphIndex::index` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qs-differential`, `qs-bench`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qs-differential`, `qs-bench`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask`, `fuzz` |
| `fn qc_project::graph::build_graph` | `qc-sema`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `xtask`, `fuzz` |
| `fn qc_project::open` | `qc-ide`, `qc-preproc`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-workbench`, `qc-syntax`, `qs-log`, `qs-differential`, `qs-bench`, `qs-install`, `qs-map`, `qs-tests`, `qs-sim`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-workbench`, `qc-syntax`, `qs-log`, `qs-differential`, `qs-bench`, `qs-install`, `qs-map`, `qs-tests`, `qs-sim`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_project::open_dir` | `qc-ide`, `qc-engines` | kept public: used by `qc-ide`, `qc-engines` |
| `fn qc_project::open_file` | `qs-workbench`, `desktop` | kept public: used by `qs-workbench`, `desktop` |
| `struct qc_project::Module` | `qc-ide`, `qc-sema`, `qs-tests`, `xtask` | kept public: used by `qc-ide`, `qc-sema`, `qs-tests`, `xtask` |
| `struct qc_project::Project` | `qc-ide`, `qs-settings`, `qs-workbench`, `qs-layout`, `qs-commands`, `desktop`, `xtask` | kept public: used by `qc-ide`, `qs-settings`, `qs-workbench`, `qs-layout`, `qs-commands`, `desktop`, `xtask` |
| `struct qc_project::graph::Define` | `qc-preproc`, `qc-sema`, `qc-lexer` | kept public: used by `qc-preproc`, `qc-sema`, `qc-lexer` |
| `struct qc_project::graph::Diagnostic` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-map`, `qs-tests`, `lsp` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-map`, `qs-tests`, `lsp` |
| `struct qc_project::graph::FileGraph` | `qc-sema` | kept public: used by `qc-sema` |
| `struct qc_project::graph::GraphFile` | `qc-sema`, `xtask` | kept public: used by `qc-sema`, `xtask` |
| `struct qc_project::graph::GraphIndex` | none | kept public: named in a public signature a dependent uses |
| `use qc_project::Code = qc_lexer::Code` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-layout`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-layout`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `use qc_project::Define = graph::Define` | `qc-preproc`, `qc-sema`, `qc-lexer` | kept public: used by `qc-preproc`, `qc-sema`, `qc-lexer` |
| `use qc_project::Diagnostic = graph::Diagnostic` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-map`, `qs-tests`, `lsp` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-map`, `qs-tests`, `lsp` |
| `use qc_project::FileGraph = graph::FileGraph` | `qc-sema` | kept public: used by `qc-sema` |
| `use qc_project::Form = graph::Form` | `qc-sema`, `qc-syntax`, `xtask` | kept public: used by `qc-sema`, `qc-syntax`, `xtask` |
| `use qc_project::FsLoader = qc_preproc::FsLoader` | `qc-preproc`, `qs-workbench`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `qs-workbench`, `desktop`, `lsp`, `xtask` |
| `use qc_project::GraphFile = graph::GraphFile` | `qc-sema`, `xtask` | kept public: used by `qc-sema`, `xtask` |
| `use qc_project::GraphIndex = graph::GraphIndex` | none | kept public: named in a public signature a dependent uses |
| `use qc_project::Loader = qc_preproc::Loader` | `qc-ide`, `qc-preproc`, `qc-sema`, `qs-workbench`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qs-workbench`, `desktop`, `lsp`, `xtask` |
| `use qc_project::ManifestError = manifest::ManifestError` | none | kept public: named in a public signature a dependent uses |
| `use qc_project::MemoryLoader = qc_preproc::MemoryLoader` | `qc-ide`, `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-engines`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-engines`, `lsp`, `xtask`, `fuzz` |
| `use qc_project::PRJ_MAX_FILES = graph::PRJ_MAX_FILES` | `fuzz` | kept public: used by `fuzz` |
| `use qc_project::Via = graph::Via` | `qc-sema`, `xtask` | kept public: used by `qc-sema`, `xtask` |
| `use qc_project::build_graph = graph::build_graph` | `qc-sema`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `xtask`, `fuzz` |
| `use qc_project::normalize = qc_preproc::normalize` | `qc-ide`, `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qs-sim`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qs-sim`, `lsp`, `xtask` |

Items that left the root (8): `const qc_project::PRJ_BUILD_DIR`, `const qc_project::PRJ_CSQC_OUTPUT`, `const qc_project::PRJ_MENUQC_OUTPUT`, `const qc_project::PRJ_SSQC_OUTPUT`, `const qc_project::graph::PRJ_MAX_DEPTH`, `fn qc_project::ModuleKind::from_declared_output`, `fn qc_project::graph::FileGraph::is_program_list`, `use qc_project::PRJ_MAX_DEPTH = graph::PRJ_MAX_DEPTH`. Each is `pub(crate)` now, under `#[cfg(test)]` where only the crate's tests read it; `qc_sema::nav::ModuleIndex::references_at` is removed.

## `qc-sema` (L3)

| Item | Used by | Disposition |
| --- | --- | --- |
| `enum qc_sema::cast::Cast` | `qc-codegen`, `qc-syntax`, `xtask` | kept public: used by `qc-codegen`, `qc-syntax`, `xtask` |
| `enum qc_sema::cast::Kind` | `qc-syntax`, `qc-builtins`, `qs-paths`, `xtask` | kept public: used by `qc-syntax`, `qc-builtins`, `qs-paths`, `xtask` |
| `enum qc_sema::cast::Source` | `qc-preproc`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qs-log`, `qs-install`, `qc-builtins`, `qc-engines`, `qc-project`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qs-log`, `qs-install`, `qc-builtins`, `qc-engines`, `qc-project`, `desktop`, `xtask` |
| `enum qc_sema::lower::decl::LiteralValue` | `xtask` | kept public: used by `xtask` |
| `enum qc_sema::lower::decl::Qualifier` | `qc-codegen`, `qc-syntax` | kept public: used by `qc-codegen`, `qc-syntax` |
| `enum qc_sema::lower::decl::ValueKind` | `qc-codegen` | kept public: used by `qc-codegen` |
| `enum qc_sema::symbol::DefKind` | `qc-ide`, `qc-codegen`, `qs-workbench`, `qc-engines`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qs-workbench`, `qc-engines`, `lsp`, `xtask` |
| `enum qc_sema::ty::SimpleType` | `qc-codegen`, `qc-syntax`, `xtask` | kept public: used by `qc-codegen`, `qc-syntax`, `xtask` |
| `enum qc_sema::ty::Type` | `qc-ide`, `qc-codegen`, `qs-mdl`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qs-editor-view`, `qs-sim`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qs-mdl`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qs-editor-view`, `qs-sim`, `desktop`, `lsp`, `xtask` |
| `fn qc_sema::cast::Kind::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_sema::cast::Source::cast` | `qc-codegen`, `xtask` | kept public: used by `qc-codegen`, `xtask` |
| `fn qc_sema::cast::Source::kind` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-settings`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-log`, `qs-install`, `qs-layout`, `qc-project`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-settings`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-log`, `qs-install`, `qs-layout`, `qc-project`, `lsp`, `xtask` |
| `fn qc_sema::cast::Source::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_sema::cast::Type::fteqcc_name` | `qc-ide` | kept public: used by `qc-ide` |
| `fn qc_sema::cast::Type::fteqcc_scalar` | `qc-codegen` | kept public: used by `qc-codegen` |
| `fn qc_sema::check::TypedBody::type_at` | `qc-codegen` | kept public: used by `qc-codegen` |
| `fn qc_sema::check::fold::fold_builtin_call` | `qc-codegen` | kept public: used by `qc-codegen` |
| `fn qc_sema::check::fold::fold_floats` | `qc-codegen` | kept public: used by `qc-codegen` |
| `fn qc_sema::db::Db::analyze_module` | `qc-ide`, `qc-codegen`, `qs-tests`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qs-tests`, `xtask`, `fuzz` |
| `fn qc_sema::db::Db::counts` | `qc-ide` | kept public: used by `qc-ide` |
| `fn qc_sema::db::Db::dialect` | `qc-codegen` | kept public: used by `qc-codegen` |
| `fn qc_sema::db::Db::expanded_decls` | `qc-codegen`, `xtask` | kept public: used by `qc-codegen`, `xtask` |
| `fn qc_sema::db::Db::expanded_origins` | `qc-codegen` | kept public: used by `qc-codegen` |
| `fn qc_sema::db::Db::expanded_parse` | `qc-codegen` | kept public: used by `qc-codegen` |
| `fn qc_sema::db::Db::file_path` | `xtask` | kept public: used by `xtask` |
| `fn qc_sema::db::Db::file_text` | `qc-ide`, `qc-codegen`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `xtask` |
| `fn qc_sema::db::Db::module_index` | `qs-tests`, `fuzz` | kept public: used by `qs-tests`, `fuzz` |
| `fn qc_sema::db::Db::new` | `qc-ide`, `qc-codegen`, `qs-differential`, `qs-bench`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qs-differential`, `qs-bench`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` |
| `fn qc_sema::db::Db::parse` | `qc-ide`, `qs-settings`, `qs-mdl`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qs-settings`, `qs-mdl`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask` |
| `fn qc_sema::db::Db::set_autoproto` | `qc-ide`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `xtask`, `fuzz` |
| `fn qc_sema::db::Db::set_dialect` | `qc-ide`, `qc-codegen`, `qs-differential`, `qs-tests`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qs-differential`, `qs-tests`, `xtask`, `fuzz` |
| `fn qc_sema::db::Db::set_file` | `qc-ide`, `qc-codegen`, `qs-differential`, `qs-bench`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qs-differential`, `qs-bench`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` |
| `fn qc_sema::db::Db::set_file_text` | `qs-bench`, `qs-tests` | kept public: used by `qs-bench`, `qs-tests` |
| `fn qc_sema::db::ModuleAnalysis::diagnostics` | `qc-ide`, `qc-codegen`, `qc-syntax`, `qs-tests`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-syntax`, `qs-tests`, `xtask`, `fuzz` |
| `fn qc_sema::expand::ModuleInput::from_graph` | `xtask` | kept public: used by `xtask` |
| `fn qc_sema::expand::ModuleInput::from_module` | `qc-ide`, `qs-bench`, `xtask` | kept public: used by `qc-ide`, `qs-bench`, `xtask` |
| `fn qc_sema::expand::OriginMap::locate` | `qc-codegen`, `lsp` | kept public: used by `qc-codegen`, `lsp` |
| `fn qc_sema::lower::decl::LiteralValue::builtin_function` | `xtask` | kept public: used by `xtask` |
| `fn qc_sema::lower::decl::RawDecl::literal` | `xtask` | kept public: used by `xtask` |
| `fn qc_sema::lower::decl::declares` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-workbench`, `qc-syntax`, `qc-edict`, `qs-differential`, `qc-builtins`, `qc-engines`, `qs-sim`, `qc-project`, `desktop`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-workbench`, `qc-syntax`, `qc-edict`, `qs-differential`, `qc-builtins`, `qc-engines`, `qs-sim`, `qc-project`, `desktop`, `xtask` |
| `fn qc_sema::lower::lower_file` | `qc-codegen`, `qc-engines`, `qs-tests`, `xtask`, `fuzz` | kept public: used by `qc-codegen`, `qc-engines`, `qs-tests`, `xtask`, `fuzz` |
| `fn qc_sema::nav::ModuleIndex::definition_at` | `qc-ide` | kept public: used by `qc-ide` |
| `fn qc_sema::nav::ModuleIndex::references` | `qc-ide`, `qs-workbench`, `lsp` | kept public: used by `qc-ide`, `qs-workbench`, `lsp` |
| `fn qc_sema::resolve::FileDiagnostic::new` | `qc-engines` | kept public: used by `qc-engines` |
| `fn qc_sema::resolve::InitialValue::words` | `qc-codegen`, `qc-syntax` | kept public: used by `qc-codegen`, `qc-syntax` |
| `fn qc_sema::resolve::ModuleIndex::alias` | `qc-codegen`, `qc-syntax`, `qs-install` | kept public: used by `qc-codegen`, `qc-syntax`, `qs-install` |
| `fn qc_sema::resolve::ModuleIndex::array_len` | `qc-codegen` | kept public: used by `qc-codegen` |
| `fn qc_sema::resolve::ModuleIndex::body` | `qc-codegen`, `qc-syntax`, `qs-sim`, `xtask` | kept public: used by `qc-codegen`, `qc-syntax`, `qs-sim`, `xtask` |
| `fn qc_sema::resolve::ModuleIndex::def` | `qc-ide`, `qc-codegen`, `qc-engines` | kept public: used by `qc-ide`, `qc-codegen`, `qc-engines` |
| `fn qc_sema::resolve::ModuleIndex::definitions` | `qc-ide`, `qc-codegen`, `desktop` | kept public: used by `qc-ide`, `qc-codegen`, `desktop` |
| `fn qc_sema::resolve::ModuleIndex::defs` | `qc-ide`, `qs-tests`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qs-tests`, `xtask`, `fuzz` |
| `fn qc_sema::resolve::ModuleIndex::diagnostics` | `qc-ide`, `qc-codegen`, `qc-syntax`, `qs-tests`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-syntax`, `qs-tests`, `xtask`, `fuzz` |
| `fn qc_sema::resolve::ModuleIndex::initial_value` | `qc-codegen` | kept public: used by `qc-codegen` |
| `fn qc_sema::resolve::ModuleIndex::is_constant` | `qc-codegen` | kept public: used by `qc-codegen` |
| `fn qc_sema::resolve::ModuleIndex::redeclarations` | `qc-ide` | kept public: used by `qc-ide` |
| `fn qc_sema::resolve::ModuleIndex::refs` | `qc-ide`, `qc-codegen`, `qc-engines`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-engines`, `xtask`, `fuzz` |
| `fn qc_sema::resolve::ModuleIndex::struct_shape` | `qc-codegen` | kept public: used by `qc-codegen` |
| `fn qc_sema::resolve::resolve_module` | `qc-codegen`, `qc-engines`, `qs-tests`, `xtask`, `fuzz` | kept public: used by `qc-codegen`, `qc-engines`, `qs-tests`, `xtask`, `fuzz` |
| `fn qc_sema::symbol::DefId::index` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qs-differential`, `qs-bench`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qs-differential`, `qs-bench`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask`, `fuzz` |
| `fn qc_sema::symbol::DefKind::is_global` | `qc-ide`, `qc-codegen` | kept public: used by `qc-ide`, `qc-codegen` |
| `fn qc_sema::symbol::FileId::index` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qs-differential`, `qs-bench`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qs-differential`, `qs-bench`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask`, `fuzz` |
| `fn qc_sema::symbol::FileId::new` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-differential`, `qs-bench`, `qc-engines`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-differential`, `qs-bench`, `qc-engines`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` |
| `fn qc_sema::ty::SimpleType::as_str` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qs-settings`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qs-settings`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask` |
| `fn qc_sema::ty::Type::function` | `qc-codegen`, `qs-differential`, `qs-sim`, `xtask` | kept public: used by `qc-codegen`, `qs-differential`, `qs-sim`, `xtask` |
| `fn qc_sema::ty::Type::named` | `qc-preproc`, `qc-syntax`, `xtask` | kept public: used by `qc-preproc`, `qc-syntax`, `xtask` |
| `mod qc_sema::cast` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-syntax`, `qc-diag`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-syntax`, `qc-diag`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `xtask` |
| `mod qc_sema::db` | `qc-ide`, `qc-codegen`, `qs-differential`, `qs-bench`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qs-differential`, `qs-bench`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` |
| `mod qc_sema::expand` | `qc-preproc`, `qc-lexer`, `xtask` | kept public: used by `qc-preproc`, `qc-lexer`, `xtask` |
| `mod qc_sema::lower` | `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-vfs`, `qs-bsp`, `qc-edict`, `qc-builtins`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-vfs`, `qs-bsp`, `qc-edict`, `qc-builtins`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `xtask`, `fuzz` |
| `mod qc_sema::nav` | none | kept public: named in a public signature a dependent uses |
| `mod qc_sema::resolve` | `qc-ide`, `qc-codegen`, `qs-vfs`, `qs-log`, `qs-differential`, `qs-install`, `qc-builtins`, `qc-progs`, `qs-paths`, `qs-tests`, `qs-commands`, `qc-project`, `qc-vm`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qs-vfs`, `qs-log`, `qs-differential`, `qs-install`, `qc-builtins`, `qc-progs`, `qs-paths`, `qs-tests`, `qs-commands`, `qc-project`, `qc-vm`, `lsp`, `xtask` |
| `mod qc_sema::symbol` | `qc-ide`, `qc-codegen`, `qs-workbench`, `qs-differential`, `desktop`, `lsp` | kept public: used by `qc-ide`, `qc-codegen`, `qs-workbench`, `qs-differential`, `desktop`, `lsp` |
| `mod qc_sema::ty` | `qc-ide`, `qc-codegen`, `qc-syntax`, `qs-differential`, `qs-render`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-syntax`, `qs-differential`, `qs-render`, `xtask` |
| `struct qc_sema::cast::Literal` | `qc-codegen`, `qc-syntax` | kept public: used by `qc-codegen`, `qc-syntax` |
| `struct qc_sema::check::TypedBody` | none | kept public: named in a public signature a dependent uses |
| `struct qc_sema::db::Db` | `qc-ide`, `qc-codegen`, `qs-differential`, `qs-bench`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qs-differential`, `qs-bench`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` |
| `struct qc_sema::db::ModuleAnalysis` | `qc-ide`, `xtask` | kept public: used by `qc-ide`, `xtask` |
| `struct qc_sema::db::QueryCounts` | none | kept public: named in a public signature a dependent uses |
| `struct qc_sema::expand::ModuleInput` | `qc-ide`, `qc-codegen`, `qs-differential`, `qs-bench`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qs-differential`, `qs-bench`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` |
| `struct qc_sema::expand::OriginMap` | none | kept public: named in a public signature a dependent uses |
| `struct qc_sema::lower::FileDecls` | `qc-codegen` | kept public: used by `qc-codegen` |
| `struct qc_sema::lower::decl::BuiltinFunction` | none | kept public: named in a public signature a dependent uses |
| `struct qc_sema::lower::decl::RawDecl` | none | kept public: named in a public signature a dependent uses |
| `struct qc_sema::resolve::FileDiagnostic` | `qc-ide`, `qc-codegen`, `qc-engines` | kept public: used by `qc-ide`, `qc-codegen`, `qc-engines` |
| `struct qc_sema::resolve::InitialValue` | none | kept public: named in a public signature a dependent uses |
| `struct qc_sema::resolve::MemberShape` | none | kept public: named in a public signature a dependent uses |
| `struct qc_sema::resolve::ModuleIndex` | `qc-ide`, `qc-codegen`, `qc-engines`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-engines`, `fuzz` |
| `struct qc_sema::resolve::ResolveOptions` | `qc-codegen`, `qc-engines`, `qs-tests`, `xtask`, `fuzz` | kept public: used by `qc-codegen`, `qc-engines`, `qs-tests`, `xtask`, `fuzz` |
| `struct qc_sema::resolve::StructShape` | `qc-codegen` | kept public: used by `qc-codegen` |
| `struct qc_sema::symbol::DefId` | `qc-ide`, `qc-codegen` | kept public: used by `qc-ide`, `qc-codegen` |
| `struct qc_sema::symbol::Definition` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-workbench`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-workbench`, `desktop`, `lsp`, `xtask` |
| `struct qc_sema::symbol::FileId` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-differential`, `qs-bench`, `qc-engines`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-differential`, `qs-bench`, `qc-engines`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` |
| `struct qc_sema::symbol::Location` | `qc-ide`, `qc-codegen`, `qs-vfs`, `qs-workbench`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qs-vfs`, `qs-workbench`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `struct qc_sema::symbol::Reference` | `xtask` | kept public: used by `xtask` |
| `struct qc_sema::ty::FunctionType` | `qc-codegen`, `qc-syntax` | kept public: used by `qc-codegen`, `qc-syntax` |
| `struct qc_sema::ty::ParamLabel` | none | kept public: named in a public signature a dependent uses |
| `struct qc_sema::ty::ParamLabels` | none | kept public: named in a public signature a dependent uses |
| `use qc_sema::Db = db::Db` | `qc-ide`, `qc-codegen`, `qs-differential`, `qs-bench`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qs-differential`, `qs-bench`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` |
| `use qc_sema::DefId = symbol::DefId` | `qc-ide`, `qc-codegen` | kept public: used by `qc-ide`, `qc-codegen` |
| `use qc_sema::DefKind = symbol::DefKind` | `qc-ide`, `qc-codegen`, `qs-workbench`, `qc-engines`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qs-workbench`, `qc-engines`, `lsp`, `xtask` |
| `use qc_sema::Definition = symbol::Definition` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-workbench`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-workbench`, `desktop`, `lsp`, `xtask` |
| `use qc_sema::FileDecls = lower::FileDecls` | `qc-codegen` | kept public: used by `qc-codegen` |
| `use qc_sema::FileDiagnostic = resolve::FileDiagnostic` | `qc-ide`, `qc-codegen`, `qc-engines` | kept public: used by `qc-ide`, `qc-codegen`, `qc-engines` |
| `use qc_sema::FileId = symbol::FileId` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-differential`, `qs-bench`, `qc-engines`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-differential`, `qs-bench`, `qc-engines`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` |
| `use qc_sema::FunctionType = ty::FunctionType` | `qc-codegen`, `qc-syntax` | kept public: used by `qc-codegen`, `qc-syntax` |
| `use qc_sema::InitialValue = resolve::InitialValue` | none | kept public: named in a public signature a dependent uses |
| `use qc_sema::LiteralValue = lower::LiteralValue` | `xtask` | kept public: used by `xtask` |
| `use qc_sema::Location = symbol::Location` | `qc-ide`, `qc-codegen`, `qs-vfs`, `qs-workbench`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qs-vfs`, `qs-workbench`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `use qc_sema::MemberShape = resolve::MemberShape` | none | kept public: named in a public signature a dependent uses |
| `use qc_sema::ModuleAnalysis = db::ModuleAnalysis` | `qc-ide`, `xtask` | kept public: used by `qc-ide`, `xtask` |
| `use qc_sema::ModuleIndex = resolve::ModuleIndex` | `qc-ide`, `qc-codegen`, `qc-engines`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-engines`, `fuzz` |
| `use qc_sema::ModuleInput = expand::ModuleInput` | `qc-ide`, `qc-codegen`, `qs-differential`, `qs-bench`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qs-differential`, `qs-bench`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` |
| `use qc_sema::OriginMap = expand::OriginMap` | none | kept public: named in a public signature a dependent uses |
| `use qc_sema::QueryCounts = db::QueryCounts` | none | kept public: named in a public signature a dependent uses |
| `use qc_sema::Reference = symbol::Reference` | `xtask` | kept public: used by `xtask` |
| `use qc_sema::ResolveOptions = resolve::ResolveOptions` | `qc-codegen`, `qc-engines`, `qs-tests`, `xtask`, `fuzz` | kept public: used by `qc-codegen`, `qc-engines`, `qs-tests`, `xtask`, `fuzz` |
| `use qc_sema::SimpleType = ty::SimpleType` | `qc-codegen`, `qc-syntax`, `xtask` | kept public: used by `qc-codegen`, `qc-syntax`, `xtask` |
| `use qc_sema::StructShape = resolve::StructShape` | `qc-codegen` | kept public: used by `qc-codegen` |
| `use qc_sema::Type = ty::Type` | `qc-ide`, `qc-codegen`, `qs-mdl`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qs-editor-view`, `qs-sim`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qs-mdl`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qs-editor-view`, `qs-sim`, `desktop`, `lsp`, `xtask` |
| `use qc_sema::TypedBody = check::TypedBody` | none | kept public: named in a public signature a dependent uses |
| `use qc_sema::fold_builtin_call = check::fold::fold_builtin_call` | `qc-codegen` | kept public: used by `qc-codegen` |
| `use qc_sema::fold_floats = check::fold::fold_floats` | `qc-codegen` | kept public: used by `qc-codegen` |
| `use qc_sema::lower::LiteralValue = decl::LiteralValue` | `xtask` | kept public: used by `xtask` |
| `use qc_sema::lower::Qualifier = decl::Qualifier` | `qc-codegen`, `qc-syntax` | kept public: used by `qc-codegen`, `qc-syntax` |
| `use qc_sema::lower::ValueKind = decl::ValueKind` | `qc-codegen` | kept public: used by `qc-codegen` |
| `use qc_sema::lower::declares = decl::declares` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-workbench`, `qc-syntax`, `qc-edict`, `qs-differential`, `qc-builtins`, `qc-engines`, `qs-sim`, `qc-project`, `desktop`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-workbench`, `qc-syntax`, `qc-edict`, `qs-differential`, `qc-builtins`, `qc-engines`, `qs-sim`, `qc-project`, `desktop`, `xtask` |
| `use qc_sema::resolve_module = resolve::resolve_module` | `qc-codegen`, `qc-engines`, `qs-tests`, `xtask`, `fuzz` | kept public: used by `qc-codegen`, `qc-engines`, `qs-tests`, `xtask`, `fuzz` |

## `qc-syntax` (L2)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qc_syntax::c_scalar::REGISTERED_TYPES` | `qc-sema` | kept public: used by `qc-sema` |
| `const qc_syntax::parser::LIMIT_MAX_PARMS` | `qc-types`, `qc-progs` | kept public: used by `qc-types`, `qc-progs` |
| `const qc_syntax::parser::LIMIT_QCC_MAX_NAME` | `qc-progs` | kept public: used by `qc-progs` |
| `const qc_syntax::parser::QC_MAX_NESTING` | `qc-sema` | kept public: used by `qc-sema` |
| `enum qc_syntax::ast::BinaryOp` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `enum qc_syntax::ast::Binding` | `qc-sema`, `qc-engines`, `desktop` | kept public: used by `qc-sema`, `qc-engines`, `desktop` |
| `enum qc_syntax::ast::LiteralKind` | `qc-sema` | kept public: used by `qc-sema` |
| `enum qc_syntax::ast::Negation` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `enum qc_syntax::ast::SimpleTypeKind` | `qc-sema` | kept public: used by `qc-sema` |
| `enum qc_syntax::ast::UnaryOp` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `enum qc_syntax::c_scalar::CScalar` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `enum qc_syntax::kind::SyntaxKind` | `qc-ide`, `qc-codegen`, `qc-sema` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema` |
| `enum qc_syntax::recovery::RecoveryFault` | none | kept public: named in a public signature a dependent uses |
| `enum qc_syntax::tree::SyntaxElement` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::Parse::diagnostics` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-tests`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-tests`, `xtask`, `fuzz` |
| `fn qc_syntax::Parse::errors` | `qc-sema`, `qs-tests`, `xtask` | kept public: used by `qc-sema`, `qs-tests`, `xtask` |
| `fn qc_syntax::Parse::flags` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-sim` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-sim` |
| `fn qc_syntax::Parse::flags_at` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::Parse::keywords` | `qc-preproc`, `qc-sema` | kept public: used by `qc-preproc`, `qc-sema` |
| `fn qc_syntax::Parse::keywords_at` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::Parse::lex_flags` | `qc-preproc`, `qc-codegen`, `qc-sema` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema` |
| `fn qc_syntax::Parse::lex_flags_at` | `qc-sema`, `qc-engines` | kept public: used by `qc-sema`, `qc-engines` |
| `fn qc_syntax::Parse::relexed` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::Parse::syntax` | `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask`, `fuzz` |
| `fn qc_syntax::Parse::tree` | `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-engines`, `desktop`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-engines`, `desktop`, `xtask` |
| `fn qc_syntax::ast::AccessorDecl::declarators` | `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` |
| `fn qc_syntax::ast::AccessorDecl::members` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::AccessorDecl::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::ast::AccessorMember::body` | `qc-codegen`, `qc-sema`, `qs-sim`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qs-sim`, `xtask` |
| `fn qc_syntax::ast::AccessorMember::index_name` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::AccessorMember::index_ty` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::AccessorMember::is_set` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::AccessorMember::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::ast::AccessorMember::ty` | `qc-ide`, `qc-codegen`, `qc-sema`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `xtask` |
| `fn qc_syntax::ast::AccessorMember::value` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-differential`, `qs-render`, `qs-map`, `qs-editor-view`, `qc-project` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-differential`, `qs-render`, `qs-map`, `qs-editor-view`, `qc-project` |
| `fn qc_syntax::ast::AccessorMember::word` | `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-lexer`, `qc-builtins`, `qs-map`, `xtask` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-lexer`, `qc-builtins`, `qs-map`, `xtask` |
| `fn qc_syntax::ast::AggregateInit::elements` | `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-bsp` | kept public: used by `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-bsp` |
| `fn qc_syntax::ast::ArgList::args` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-install`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-install`, `xtask` |
| `fn qc_syntax::ast::ArgList::named` | `qc-preproc`, `qc-sema`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `xtask` |
| `fn qc_syntax::ast::ArgList::slots` | `qc-codegen`, `qc-sema`, `qs-differential` | kept public: used by `qc-codegen`, `qc-sema`, `qs-differential` |
| `fn qc_syntax::ast::ArraySuffix::is_empty` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qs-bsp`, `qc-lno`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qs-bsp`, `qc-lno`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::ast::AsmStmt::locals` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::AstNode::can_cast` | none | kept public: named in a public signature a dependent uses |
| `fn qc_syntax::ast::AstNode::cast` | `qc-codegen`, `qc-sema`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `xtask` |
| `fn qc_syntax::ast::AstNode::syntax` | `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask`, `fuzz` |
| `fn qc_syntax::ast::Attribute::arguments` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::Attribute::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::ast::AttributeList::attributes` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::BinaryExpr::lhs` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::BinaryExpr::op` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::BinaryExpr::op_token` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::BinaryExpr::rhs` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::Block::statements` | `qc-codegen`, `qc-sema`, `qs-differential`, `qc-builtins`, `qc-progs`, `qc-vm`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qs-differential`, `qc-builtins`, `qc-progs`, `qc-vm`, `xtask` |
| `fn qc_syntax::ast::Builtin::binding` | `qc-sema`, `qc-engines` | kept public: used by `qc-sema`, `qc-engines` |
| `fn qc_syntax::ast::Builtin::engine_name` | `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` |
| `fn qc_syntax::ast::Builtin::number` | `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qc-edict`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-map`, `qc-progs`, `qc-engines`, `qs-tests`, `qc-vm`, `xtask` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qc-edict`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-map`, `qc-progs`, `qc-engines`, `qs-tests`, `qc-vm`, `xtask` |
| `fn qc_syntax::ast::CallExpr::args` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-install`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-install`, `xtask` |
| `fn qc_syntax::ast::CallExpr::callee` | `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines` |
| `fn qc_syntax::ast::CaseLabel::is_default` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::CaseLabel::range_end` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::CaseLabel::value` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-differential`, `qs-render`, `qs-map`, `qs-editor-view`, `qc-project` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-differential`, `qs-render`, `qs-map`, `qs-editor-view`, `qc-project` |
| `fn qc_syntax::ast::CastExpr::operand` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::CastExpr::ty` | `qc-ide`, `qc-codegen`, `qc-sema`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `xtask` |
| `fn qc_syntax::ast::ClassDecl::declarators` | `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` |
| `fn qc_syntax::ast::ClassDecl::members` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::ClassDecl::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::ast::ClassDecl::parent` | `qc-ide`, `qc-preproc`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-differential`, `qs-install`, `qs-layout`, `qs-sim`, `qc-project`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-differential`, `qs-install`, `qs-layout`, `qs-sim`, `qc-project`, `xtask` |
| `fn qc_syntax::ast::Declaration::declarators` | `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` |
| `fn qc_syntax::ast::Declaration::ty` | `qc-ide`, `qc-codegen`, `qc-sema`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `xtask` |
| `fn qc_syntax::ast::Declarator::array_suffixes` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::Declarator::initializer` | `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` |
| `fn qc_syntax::ast::Declarator::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::ast::Declarator::params` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-commands`, `desktop`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-commands`, `desktop`, `xtask` |
| `fn qc_syntax::ast::Declarator::pointer_depth` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::Declarator::pointer_form` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::Declarator::prefix_params` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::Declarator::scope` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::DoStmt::body` | `qc-codegen`, `qc-sema`, `qs-sim`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qs-sim`, `xtask` |
| `fn qc_syntax::ast::DoStmt::condition` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::DoStmt::until` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::EnumDecl::declarators` | `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` |
| `fn qc_syntax::ast::EnumDecl::enumerators` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::EnumDecl::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::ast::ExprStmt::expr` | `qc-codegen`, `qc-sema`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `xtask` |
| `fn qc_syntax::ast::ExternBlock::items` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::FieldExpr::base` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::FieldExpr::field` | `qc-codegen`, `qc-sema`, `qc-edict`, `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qc-edict`, `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm`, `xtask` |
| `fn qc_syntax::ast::FieldType::inner` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::ForStmt::body` | `qc-codegen`, `qc-sema`, `qs-sim`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qs-sim`, `xtask` |
| `fn qc_syntax::ast::ForStmt::clauses` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::ForStmt::condition` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::ForStmt::init` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::ForStmt::step` | `qc-codegen`, `qc-sema`, `qs-differential`, `qs-commands`, `qc-vm`, `desktop` | kept public: used by `qc-codegen`, `qc-sema`, `qs-differential`, `qs-commands`, `qc-vm`, `desktop` |
| `fn qc_syntax::ast::FunctionBody::block` | `qc-codegen`, `qc-sema`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `xtask` |
| `fn qc_syntax::ast::FunctionBody::state` | `qc-codegen`, `qc-sema`, `qs-editor-view`, `desktop` | kept public: used by `qc-codegen`, `qc-sema`, `qs-editor-view`, `desktop` |
| `fn qc_syntax::ast::FunctionType::params` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-commands`, `desktop`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-commands`, `desktop`, `xtask` |
| `fn qc_syntax::ast::FunctionType::return_type` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::GotoStmt::label` | `qc-sema`, `desktop`, `xtask` | kept public: used by `qc-sema`, `desktop`, `xtask` |
| `fn qc_syntax::ast::IfStmt::condition` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::IfStmt::else_branch` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::IfStmt::negation` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::IfStmt::then_branch` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::IndexExpr::base` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::IndexExpr::index` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-bsp`, `qc-lno`, `qc-edict`, `qs-differential`, `qs-bench`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-bsp`, `qc-lno`, `qc-edict`, `qs-differential`, `qs-bench`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask`, `fuzz` |
| `fn qc_syntax::ast::LabelStmt::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::ast::Literal::kind` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-pak`, `qs-log`, `qs-install`, `qs-layout`, `qc-project`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-pak`, `qs-log`, `qs-install`, `qs-layout`, `qc-project`, `lsp`, `xtask` |
| `fn qc_syntax::ast::Literal::token` | `qc-codegen`, `qc-sema`, `qs-map`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qs-map`, `xtask` |
| `fn qc_syntax::ast::LocalDecl::declarators` | `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` |
| `fn qc_syntax::ast::LocalDecl::local_token` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::LocalDecl::ty` | `qc-ide`, `qc-codegen`, `qc-sema`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `xtask` |
| `fn qc_syntax::ast::LoopStmt::body` | `qc-codegen`, `qc-sema`, `qs-sim`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qs-sim`, `xtask` |
| `fn qc_syntax::ast::NameRef::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::ast::NameRef::scope` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::NamedArg::equals` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::NamedArg::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::ast::NamedArg::value` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-differential`, `qs-render`, `qs-map`, `qs-editor-view`, `qc-project` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-differential`, `qs-render`, `qs-map`, `qs-editor-view`, `qc-project` |
| `fn qc_syntax::ast::NotExpr::operand` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::Param::array_suffixes` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::Param::default_value` | `qc-codegen`, `qc-sema`, `qs-settings` | kept public: used by `qc-codegen`, `qc-sema`, `qs-settings` |
| `fn qc_syntax::ast::Param::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::ast::Param::ty` | `qc-ide`, `qc-codegen`, `qc-sema`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `xtask` |
| `fn qc_syntax::ast::ParamList::is_variadic` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::ParamList::params` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-commands`, `desktop`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-commands`, `desktop`, `xtask` |
| `fn qc_syntax::ast::ParamList::qcc_parm_names` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::ParenExpr::expr` | `qc-codegen`, `qc-sema`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `xtask` |
| `fn qc_syntax::ast::PointerType::inner` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::PostfixExpr::operand` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::ReturnStmt::sets` | `qc-sema`, `qs-sim` | kept public: used by `qc-sema`, `qs-sim` |
| `fn qc_syntax::ast::ReturnStmt::value` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-differential`, `qs-render`, `qs-map`, `qs-editor-view`, `qc-project` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-differential`, `qs-render`, `qs-map`, `qs-editor-view`, `qc-project` |
| `fn qc_syntax::ast::SimpleType::kind` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-pak`, `qs-log`, `qs-install`, `qs-layout`, `qc-project`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-pak`, `qs-log`, `qs-install`, `qs-layout`, `qc-project`, `lsp`, `xtask` |
| `fn qc_syntax::ast::SimpleType::text` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-workbench`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-workbench`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::ast::SizeofExpr::operand` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::SizeofExpr::ty` | `qc-ide`, `qc-codegen`, `qc-sema`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `xtask` |
| `fn qc_syntax::ast::SourceFile::declarations` | `qc-sema`, `qc-engines`, `xtask` | kept public: used by `qc-sema`, `qc-engines`, `xtask` |
| `fn qc_syntax::ast::SourceFile::items` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::State::exprs` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::State::frame` | `qc-codegen`, `qs-mdl`, `qs-differential`, `qs-render`, `qs-editor-view`, `qs-sim`, `desktop`, `fuzz` | kept public: used by `qc-codegen`, `qs-mdl`, `qs-differential`, `qs-render`, `qs-editor-view`, `qs-sim`, `desktop`, `fuzz` |
| `fn qc_syntax::ast::State::is_cycle` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::State::next` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-mdl`, `qs-bsp`, `qc-diag`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-porting-example`, `qs-map`, `qc-progs`, `qs-editor-view`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-mdl`, `qs-bsp`, `qc-diag`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-porting-example`, `qs-map`, `qc-progs`, `qs-editor-view`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::ast::StateStmt::state` | `qc-codegen`, `qc-sema`, `qs-editor-view`, `desktop` | kept public: used by `qc-codegen`, `qc-sema`, `qs-editor-view`, `desktop` |
| `fn qc_syntax::ast::StructDecl::declarators` | `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` |
| `fn qc_syntax::ast::StructDecl::fields` | `qc-trace`, `qc-edict`, `qs-differential`, `qc-progs`, `qc-vm`, `xtask` | kept public: used by `qc-trace`, `qc-edict`, `qs-differential`, `qc-progs`, `qc-vm`, `xtask` |
| `fn qc_syntax::ast::StructDecl::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::ast::SwitchStmt::block` | `qc-codegen`, `qc-sema`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `xtask` |
| `fn qc_syntax::ast::SwitchStmt::scrutinee` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::SwitchStmt::statement` | `qc-codegen`, `qc-sema`, `qs-differential`, `qc-vm`, `xtask`, `fuzz` | kept public: used by `qc-codegen`, `qc-sema`, `qs-differential`, `qc-vm`, `xtask`, `fuzz` |
| `fn qc_syntax::ast::TernaryExpr::arms` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::TernaryExpr::condition` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::ThinktimeStmt::operands` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::Type::params` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-commands`, `desktop`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-commands`, `desktop`, `xtask` |
| `fn qc_syntax::ast::TypedefDecl::declarators` | `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` |
| `fn qc_syntax::ast::UnaryExpr::op` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::UnaryExpr::operand` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::UsingAlias::alias` | `qc-codegen`, `qc-sema`, `qs-install` | kept public: used by `qc-codegen`, `qc-sema`, `qs-install` |
| `fn qc_syntax::ast::UsingAlias::target` | `qc-preproc`, `qc-sema`, `qs-log`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qs-log`, `xtask` |
| `fn qc_syntax::ast::UsingStmt::aliases` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::ast::UsingStmt::body` | `qc-codegen`, `qc-sema`, `qs-sim`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qs-sim`, `xtask` |
| `fn qc_syntax::ast::VectorExpr::elements` | `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-bsp` | kept public: used by `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-bsp` |
| `fn qc_syntax::ast::WhileStmt::body` | `qc-codegen`, `qc-sema`, `qs-sim`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qs-sim`, `xtask` |
| `fn qc_syntax::ast::WhileStmt::condition` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::ast::WhileStmt::until` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::binum::builtin_binum` | `qc-codegen`, `qc-engines`, `xtask` | kept public: used by `qc-codegen`, `qc-engines`, `xtask` |
| `fn qc_syntax::c_scalar::CScalar::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::c_scalar::CScalar::named` | `qc-preproc`, `qc-sema`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `xtask` |
| `fn qc_syntax::c_scalar::CWords::read` | `qc-sema`, `qs-vfs`, `qs-install`, `xtask` | kept public: used by `qc-sema`, `qs-vfs`, `qs-install`, `xtask` |
| `fn qc_syntax::kind::SyntaxKind::from_punct` | none | kept public: named in a public signature a dependent uses |
| `fn qc_syntax::kind::SyntaxKind::from_token_kind` | none | kept public: named in a public signature a dependent uses |
| `fn qc_syntax::kind::SyntaxKind::is_keyword` | `qc-ide` | kept public: used by `qc-ide` |
| `fn qc_syntax::kind::SyntaxKind::is_trivia` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::parse` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-bsp`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::parse_module_source` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::parse_preprocessed` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::parse_preprocessed_flagged` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::parse_with` | `qc-codegen`, `qc-sema`, `qc-engines`, `xtask`, `fuzz` | kept public: used by `qc-codegen`, `qc-sema`, `qc-engines`, `xtask`, `fuzz` |
| `fn qc_syntax::recovery::check_deletions_with` | `xtask` | kept public: used by `xtask` |
| `fn qc_syntax::tree::SyntaxElement::kind` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-pak`, `qs-log`, `qs-install`, `qs-layout`, `qc-project`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-pak`, `qs-log`, `qs-install`, `qs-layout`, `qc-project`, `lsp`, `xtask` |
| `fn qc_syntax::tree::SyntaxElement::span` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-log`, `qc-engines`, `qc-project`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-log`, `qc-engines`, `qc-project`, `fuzz` |
| `fn qc_syntax::tree::SyntaxNode::ancestors` | `qc-codegen`, `qc-sema`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `xtask` |
| `fn qc_syntax::tree::SyntaxNode::child_tokens` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_syntax::tree::SyntaxNode::children` | `qc-codegen`, `qc-sema`, `qs-editor-view`, `fuzz` | kept public: used by `qc-codegen`, `qc-sema`, `qs-editor-view`, `fuzz` |
| `fn qc_syntax::tree::SyntaxNode::children_with_tokens` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::tree::SyntaxNode::descendants` | `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `fuzz` |
| `fn qc_syntax::tree::SyntaxNode::descendants_with_tokens` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::tree::SyntaxNode::kind` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-pak`, `qs-log`, `qs-install`, `qs-layout`, `qc-project`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-pak`, `qs-log`, `qs-install`, `qs-layout`, `qc-project`, `lsp`, `xtask` |
| `fn qc_syntax::tree::SyntaxNode::parent` | `qc-ide`, `qc-preproc`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-differential`, `qs-install`, `qs-layout`, `qs-sim`, `qc-project`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-differential`, `qs-install`, `qs-layout`, `qs-sim`, `qc-project`, `xtask` |
| `fn qc_syntax::tree::SyntaxNode::span` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-log`, `qc-engines`, `qc-project`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-log`, `qc-engines`, `qc-project`, `fuzz` |
| `fn qc_syntax::tree::SyntaxNode::text` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-workbench`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-workbench`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::tree::SyntaxNode::token_of` | `qc-sema` | kept public: used by `qc-sema` |
| `fn qc_syntax::tree::SyntaxNode::tokens` | `qc-ide`, `qc-preproc`, `qc-sema`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `fuzz` |
| `fn qc_syntax::tree::SyntaxToken::kind` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-pak`, `qs-log`, `qs-install`, `qs-layout`, `qc-project`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-pak`, `qs-log`, `qs-install`, `qs-layout`, `qc-project`, `lsp`, `xtask` |
| `fn qc_syntax::tree::SyntaxToken::parent` | `qc-ide`, `qc-preproc`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-differential`, `qs-install`, `qs-layout`, `qs-sim`, `qc-project`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-differential`, `qs-install`, `qs-layout`, `qs-sim`, `qc-project`, `xtask` |
| `fn qc_syntax::tree::SyntaxToken::span` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-log`, `qc-engines`, `qc-project`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-log`, `qc-engines`, `qc-project`, `fuzz` |
| `fn qc_syntax::tree::SyntaxToken::text` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-workbench`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-workbench`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_syntax::tree::SyntaxToken::text_lossless` | `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` |
| `mod qc_syntax::ast` | `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `xtask` |
| `mod qc_syntax::c_scalar` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `mod qc_syntax::recovery` | `qc-sema`, `xtask` | kept public: used by `qc-sema`, `xtask` |
| `struct qc_syntax::Parse` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-layout`, `qc-engines` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-layout`, `qc-engines` |
| `struct qc_syntax::c_scalar::CContext` | `qc-sema` | kept public: used by `qc-sema` |
| `struct qc_syntax::c_scalar::CType` | none | kept public: named in a public signature a dependent uses |
| `struct qc_syntax::c_scalar::CWords` | `qc-sema` | kept public: used by `qc-sema` |
| `struct qc_syntax::recovery::DeletionFailure` | none | kept public: named in a public signature a dependent uses |
| `struct qc_syntax::recovery::DeletionReport` | none | kept public: named in a public signature a dependent uses |
| `struct qc_syntax::tree::SyntaxNode` | `qc-ide`, `qc-codegen`, `qc-sema`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `fuzz` |
| `struct qc_syntax::tree::SyntaxToken` | `qc-ide`, `qc-codegen`, `qc-sema` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema` |
| `trait qc_syntax::ast::AstNode` | `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines` |
| `use qc_syntax::Code = qc_lexer::Code` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-diag`, `qc-lexer`, `qs-layout`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-diag`, `qc-lexer`, `qs-layout`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `use qc_syntax::CompilerFlags = qc_preproc::CompilerFlags` | `qc-preproc`, `qc-codegen`, `qc-sema` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema` |
| `use qc_syntax::Diagnostic = qc_lexer::Diagnostic` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-diag`, `qc-lexer`, `qs-map`, `qs-tests`, `qc-project`, `lsp` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-diag`, `qc-lexer`, `qs-map`, `qs-tests`, `qc-project`, `lsp` |
| `use qc_syntax::Dialect = qc_lexer::Dialect` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-lexer`, `qs-differential`, `qc-engines`, `qs-tests`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-lexer`, `qs-differential`, `qc-engines`, `qs-tests`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `use qc_syntax::FlagEdit = qc_preproc::FlagEdit` | `qc-preproc`, `qc-sema` | kept public: used by `qc-preproc`, `qc-sema` |
| `use qc_syntax::Keyword = qc_preproc::Keyword` | `qc-ide`, `qc-preproc`, `qc-sema`, `qc-lexer`, `qc-project`, `desktop` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qc-lexer`, `qc-project`, `desktop` |
| `use qc_syntax::KeywordEdit = qc_preproc::KeywordEdit` | `qc-preproc`, `qc-sema` | kept public: used by `qc-preproc`, `qc-sema` |
| `use qc_syntax::Keywords = qc_preproc::Keywords` | `qc-preproc`, `qc-sema`, `qc-lexer`, `desktop` | kept public: used by `qc-preproc`, `qc-sema`, `qc-lexer`, `desktop` |
| `use qc_syntax::LIMIT_MAX_PARMS = parser::LIMIT_MAX_PARMS` | `qc-types`, `qc-progs` | kept public: used by `qc-types`, `qc-progs` |
| `use qc_syntax::LIMIT_QCC_MAX_NAME = parser::LIMIT_QCC_MAX_NAME` | `qc-progs` | kept public: used by `qc-progs` |
| `use qc_syntax::Priorities = qc_preproc::Priorities` | `qc-preproc` | kept public: used by `qc-preproc` |
| `use qc_syntax::QC_MAX_NESTING = parser::QC_MAX_NESTING` | `qc-sema` | kept public: used by `qc-sema` |
| `use qc_syntax::Span = qc_lexer::Span` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-diag`, `qc-lexer`, `qc-engines`, `lsp` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-diag`, `qc-lexer`, `qc-engines`, `lsp` |
| `use qc_syntax::SyntaxElement = tree::SyntaxElement` | `qc-sema` | kept public: used by `qc-sema` |
| `use qc_syntax::SyntaxKind = kind::SyntaxKind` | `qc-ide`, `qc-codegen`, `qc-sema` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema` |
| `use qc_syntax::SyntaxNode = tree::SyntaxNode` | `qc-ide`, `qc-codegen`, `qc-sema`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `fuzz` |
| `use qc_syntax::SyntaxToken = tree::SyntaxToken` | `qc-ide`, `qc-codegen`, `qc-sema` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema` |
| `use qc_syntax::builtin_binum = binum::builtin_binum` | `qc-codegen`, `qc-engines`, `xtask` | kept public: used by `qc-codegen`, `qc-engines`, `xtask` |

Items that left the root (32): `const qc_syntax::parser::FTEQCC_MAX_NAME`, `enum qc_syntax::c_scalar::CWord`, `enum qc_syntax::tree::GreenElement`, `fn qc_syntax::Parse::green`, `fn qc_syntax::ast::ClassDecl::accessor_members`, `fn qc_syntax::ast::ClassDecl::has_body`, `fn qc_syntax::ast::SimpleType::keyword`, `fn qc_syntax::ast::SourceFile::directives`, `fn qc_syntax::c_scalar::CType::warnings`, `fn qc_syntax::c_scalar::CWord::of`, `fn qc_syntax::recovery::check_deletions`, `fn qc_syntax::tree::GreenElement::is_empty`, `fn qc_syntax::tree::GreenElement::kind`, `fn qc_syntax::tree::GreenElement::len`, `fn qc_syntax::tree::GreenNode::children`, `fn qc_syntax::tree::GreenNode::is_empty`, `fn qc_syntax::tree::GreenNode::kind`, `fn qc_syntax::tree::GreenNode::len`, `fn qc_syntax::tree::GreenNode::new`, `fn qc_syntax::tree::GreenNode::write_text`, `fn qc_syntax::tree::GreenToken::kind`, `fn qc_syntax::tree::GreenToken::new`, `fn qc_syntax::tree::GreenToken::text`, `fn qc_syntax::tree::SyntaxNode::debug_tree`, `fn qc_syntax::tree::SyntaxNode::green`, `fn qc_syntax::tree::SyntaxNode::new_root`, `struct qc_syntax::tree::GreenNode`, `struct qc_syntax::tree::GreenToken`, `use qc_syntax::FTEQCC_MAX_NAME = parser::FTEQCC_MAX_NAME`, `use qc_syntax::GreenElement = tree::GreenElement`, `use qc_syntax::GreenNode = tree::GreenNode`, `use qc_syntax::GreenToken = tree::GreenToken`. Each is `pub(crate)` now, under `#[cfg(test)]` where only the crate's tests read it; `qc_sema::nav::ModuleIndex::references_at` is removed.

## `qc-trace` (L1)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qc_trace::TRACE_HEADER_SIZE` | none | kept public: the trace format's codec (TRACE-*) the oracles and the differential exchange |
| `const qc_trace::TRACE_MAGIC` | none | kept public: the trace format's codec (TRACE-*) the oracles and the differential exchange |
| `const qc_trace::TRACE_TAG_ERROR` | none | kept public: the trace format's codec (TRACE-*) the oracles and the differential exchange |
| `const qc_trace::TRACE_TAG_EXIT` | none | kept public: the trace format's codec (TRACE-*) the oracles and the differential exchange |
| `const qc_trace::TRACE_TAG_RUN` | none | kept public: the trace format's codec (TRACE-*) the oracles and the differential exchange |
| `const qc_trace::TRACE_TAG_STEP` | none | kept public: the trace format's codec (TRACE-*) the oracles and the differential exchange |
| `const qc_trace::TRACE_TAG_STRING` | none | kept public: the trace format's codec (TRACE-*) the oracles and the differential exchange |
| `const qc_trace::TRACE_UVAR_MAX_BYTES` | none | kept public: the trace format's codec (TRACE-*) the oracles and the differential exchange |
| `const qc_trace::TRACE_VERSION` | `qs-differential` | kept public: used by `qs-differential` |
| `enum qc_trace::JsonFault` | none | kept public: the trace format's codec (TRACE-*) the oracles and the differential exchange |
| `enum qc_trace::Record` | `qc-sema`, `qs-log`, `qs-differential`, `qc-progs`, `qc-vm`, `xtask` | kept public: used by `qc-sema`, `qs-log`, `qs-differential`, `qc-progs`, `qc-vm`, `xtask` |
| `enum qc_trace::StringValue` | `qs-differential`, `xtask` | kept public: used by `qs-differential`, `xtask` |
| `enum qc_trace::TraceError` | `qs-differential`, `qc-vm` | kept public: used by `qs-differential`, `qc-vm` |
| `fn qc_trace::Changes::fields` | `qc-syntax`, `qc-edict`, `qs-differential`, `qc-progs`, `qc-vm`, `xtask` | kept public: used by `qc-syntax`, `qc-edict`, `qs-differential`, `qc-progs`, `qc-vm`, `xtask` |
| `fn qc_trace::Changes::globals` | `qs-differential`, `qc-progs`, `qc-vm`, `xtask` | kept public: used by `qs-differential`, `qc-progs`, `qc-vm`, `xtask` |
| `fn qc_trace::Changes::is_empty` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_trace::Changes::new` | `qs-differential`, `qc-vm`, `xtask` | kept public: used by `qs-differential`, `qc-vm`, `xtask` |
| `fn qc_trace::Header::edict_size` | `qc-edict`, `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm`, `xtask` | kept public: used by `qc-edict`, `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm`, `xtask` |
| `fn qc_trace::Header::entity_number` | `qs-differential`, `xtask` | kept public: used by `qs-differential`, `xtask` |
| `fn qc_trace::Header::entityfields` | `qc-edict`, `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm`, `xtask` | kept public: used by `qc-edict`, `qs-differential`, `qc-builtins`, `qs-sim`, `qc-vm`, `xtask` |
| `fn qc_trace::Header::field_offset` | `xtask` | kept public: used by `xtask` |
| `fn qc_trace::Header::new` | `qs-differential`, `qc-vm`, `xtask` | kept public: used by `qs-differential`, `qc-vm`, `xtask` |
| `fn qc_trace::Header::numstrings` | `qs-differential`, `xtask` | kept public: used by `qs-differential`, `xtask` |
| `fn qc_trace::Header::pointer_target` | `qs-differential`, `xtask` | kept public: used by `qs-differential`, `xtask` |
| `fn qc_trace::Header::progs_crc` | `qs-differential`, `xtask` | kept public: used by `qs-differential`, `xtask` |
| `fn qc_trace::Record::changes` | `qs-differential`, `qc-vm`, `xtask` | kept public: used by `qs-differential`, `qc-vm`, `xtask` |
| `fn qc_trace::Text::as_bytes` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_trace::Text::new` | `qs-differential`, `qc-vm`, `xtask` | kept public: used by `qs-differential`, `qc-vm`, `xtask` |
| `fn qc_trace::Trace::string_value` | `qs-differential`, `xtask` | kept public: used by `qs-differential`, `xtask` |
| `fn qc_trace::binary::decode` | `qc-preproc`, `qs-wad`, `qs-vfs`, `qc-lno`, `qs-differential`, `qc-progs`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qs-wad`, `qs-vfs`, `qc-lno`, `qs-differential`, `qc-progs`, `xtask`, `fuzz` |
| `fn qc_trace::binary::encode` | `qc-preproc`, `qs-differential`, `qc-progs`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `qs-differential`, `qc-progs`, `lsp`, `xtask` |
| `fn qc_trace::binary::encode_header` | none | kept public: the trace format's codec (TRACE-*) the oracles and the differential exchange |
| `fn qc_trace::binary::Record::encode` | none | kept public: the trace format's codec (TRACE-*) the oracles and the differential exchange |
| `fn qc_trace::json::from_json` | none | kept public: the trace format's codec (TRACE-*) the oracles and the differential exchange |
| `fn qc_trace::json::header_to_json` | none | kept public: the trace format's codec (TRACE-*) the oracles and the differential exchange |
| `fn qc_trace::json::Record::to_json` | `xtask` | kept public: used by `xtask` |
| `fn qc_trace::json::to_json` | `xtask` | kept public: used by `xtask` |
| `struct qc_trace::Changes` | `qs-workbench`, `qs-differential`, `qs-editor-view`, `qs-commands`, `qc-vm`, `xtask` | kept public: used by `qs-workbench`, `qs-differential`, `qs-editor-view`, `qs-commands`, `qc-vm`, `xtask` |
| `struct qc_trace::FieldChange` | `qs-differential`, `qc-vm`, `xtask` | kept public: used by `qs-differential`, `qc-vm`, `xtask` |
| `struct qc_trace::GlobalChange` | `qs-differential`, `qc-vm`, `xtask` | kept public: used by `qs-differential`, `qc-vm`, `xtask` |
| `struct qc_trace::Header` | `qs-pak`, `qc-lno`, `qs-differential`, `qc-progs`, `qc-vm`, `lsp`, `xtask` | kept public: used by `qs-pak`, `qc-lno`, `qs-differential`, `qc-progs`, `qc-vm`, `lsp`, `xtask` |
| `struct qc_trace::Step` | `qc-sema`, `qc-syntax`, `qs-differential`, `qs-editor-view`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `xtask` | kept public: used by `qc-sema`, `qc-syntax`, `qs-differential`, `qs-editor-view`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `xtask` |
| `struct qc_trace::Text` | `qc-preproc`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-differential`, `qs-install`, `qs-editor-view`, `qs-commands`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-differential`, `qs-install`, `qs-editor-view`, `qs-commands`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `struct qc_trace::Trace` | `qs-differential`, `qs-sim`, `qc-vm`, `xtask` | kept public: used by `qs-differential`, `qs-sim`, `qc-vm`, `xtask` |
| `use qc_trace::decode = binary::decode` | `qc-preproc`, `qs-wad`, `qs-vfs`, `qc-lno`, `qs-differential`, `qc-progs`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qs-wad`, `qs-vfs`, `qc-lno`, `qs-differential`, `qc-progs`, `xtask`, `fuzz` |
| `use qc_trace::encode = binary::encode` | `qc-preproc`, `qs-differential`, `qc-progs`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `qs-differential`, `qc-progs`, `lsp`, `xtask` |
| `use qc_trace::encode_header = binary::encode_header` | none | kept public: the trace format's codec (TRACE-*) the oracles and the differential exchange |
| `use qc_trace::from_json = json::from_json` | none | kept public: the trace format's codec (TRACE-*) the oracles and the differential exchange |
| `use qc_trace::header_to_json = json::header_to_json` | none | kept public: the trace format's codec (TRACE-*) the oracles and the differential exchange |
| `use qc_trace::to_json = json::to_json` | `xtask` | kept public: used by `xtask` |

## `qc-types` (L0)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qc_types::EntityRef::WORLD` | `qc-edict`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-map`, `qs-sim`, `qc-vm`, `desktop`, `xtask` | kept public: used by `qc-edict`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-map`, `qs-sim`, `qc-vm`, `desktop`, `xtask` |
| `const qc_types::Etype::ALL` | `qc-ide`, `qc-preproc`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `const qc_types::FunctionRef::NULL` | `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qs-differential`, `qs-sim`, `qc-vm`, `xtask` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qs-differential`, `qs-sim`, `qc-vm`, `xtask` |
| `const qc_types::LIMIT_MAX_PARMS` | `qc-syntax`, `qc-progs` | kept public: used by `qc-syntax`, `qc-progs` |
| `const qc_types::PROGS_OFS_NULL` | none | kept public: the progs value vocabulary (TYPE-*) every engine crate and the compiler share |
| `const qc_types::PROGS_OFS_PARM0` | `qc-builtins`, `qs-tests`, `qc-vm` | kept public: used by `qc-builtins`, `qs-tests`, `qc-vm` |
| `const qc_types::PROGS_OFS_RETURN` | `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm` | kept public: used by `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm` |
| `const qc_types::PROGS_PARM_SLOTS` | `qc-builtins`, `qc-vm` | kept public: used by `qc-builtins`, `qc-vm` |
| `const qc_types::PROGS_RESERVED_OFS` | `qc-edict` | kept public: used by `qc-edict` |
| `const qc_types::PROGS_TYPE_COUNT` | none | kept public: the progs value vocabulary (TYPE-*) every engine crate and the compiler share |
| `const qc_types::PROGS_VECTOR_SLOTS` | none | kept public: the progs value vocabulary (TYPE-*) every engine crate and the compiler share |
| `const qc_types::Slot::ZERO` | `qc-sema`, `qc-edict`, `qs-differential`, `qc-builtins`, `qc-progs`, `qs-editor-view`, `qs-tests`, `qs-sim`, `qc-vm`, `desktop`, `xtask` | kept public: used by `qc-sema`, `qc-edict`, `qs-differential`, `qc-builtins`, `qc-progs`, `qs-editor-view`, `qs-tests`, `qs-sim`, `qc-vm`, `desktop`, `xtask` |
| `const qc_types::StringRef::EMPTY` | `qc-preproc`, `qc-lexer` | kept public: used by `qc-preproc`, `qc-lexer` |
| `enum qc_types::DefType` | `qs-differential`, `qc-progs`, `qs-sim`, `xtask` | kept public: used by `qs-differential`, `qc-progs`, `qs-sim`, `xtask` |
| `enum qc_types::Etype` | `qc-codegen`, `qc-sema`, `qc-edict`, `qs-differential`, `qc-builtins`, `qc-progs`, `qs-sim`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qc-edict`, `qs-differential`, `qc-builtins`, `qc-progs`, `qs-sim`, `xtask` |
| `enum qc_types::Value` | `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qc-lno`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-install`, `qs-map`, `qc-progs`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qc-lno`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-install`, `qs-map`, `qc-progs`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qc_types::DefType::etype` | `qc-codegen`, `qc-sema`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `xtask` |
| `fn qc_types::DefType::from_number` | `qc-progs` | kept public: used by `qc-progs` |
| `fn qc_types::DefType::number` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-edict`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-map`, `qc-progs`, `qc-engines`, `qs-tests`, `qc-vm`, `xtask` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-edict`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-map`, `qc-progs`, `qc-engines`, `qs-tests`, `qc-vm`, `xtask` |
| `fn qc_types::EntityRef::from_number` | `qc-edict` | kept public: used by `qc-edict` |
| `fn qc_types::EntityRef::number` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-edict`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-map`, `qc-progs`, `qc-engines`, `qs-tests`, `qc-vm`, `xtask` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-edict`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-map`, `qc-progs`, `qc-engines`, `qs-tests`, `qc-vm`, `xtask` |
| `fn qc_types::Etype::from_number` | none | kept public: the progs value vocabulary (TYPE-*) every engine crate and the compiler share |
| `fn qc_types::Etype::is_declarable` | none | kept public: the progs value vocabulary (TYPE-*) every engine crate and the compiler share |
| `fn qc_types::Etype::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qc_types::Etype::number` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-edict`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-map`, `qc-progs`, `qc-engines`, `qs-tests`, `qc-vm`, `xtask` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-edict`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-map`, `qc-progs`, `qc-engines`, `qs-tests`, `qc-vm`, `xtask` |
| `fn qc_types::Etype::slot_count` | `qc-edict` | kept public: used by `qc-edict` |
| `fn qc_types::FieldRef::component` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `fn qc_types::Slot::as_f32` | `qs-differential`, `qc-builtins`, `qc-progs`, `qs-tests`, `qs-sim`, `qc-vm` | kept public: used by `qs-differential`, `qc-builtins`, `qc-progs`, `qs-tests`, `qs-sim`, `qc-vm` |
| `fn qc_types::Slot::as_i32` | `qs-differential`, `qc-builtins`, `qc-progs`, `qs-sim`, `qc-vm` | kept public: used by `qs-differential`, `qc-builtins`, `qc-progs`, `qs-sim`, `qc-vm` |
| `fn qc_types::Slot::bits` | `qc-codegen`, `qc-trace`, `qc-edict`, `qs-differential`, `qc-builtins`, `qc-progs`, `qs-sim`, `qc-vm` | kept public: used by `qc-codegen`, `qc-trace`, `qc-edict`, `qs-differential`, `qc-builtins`, `qc-progs`, `qs-sim`, `qc-vm` |
| `fn qc_types::Slot::from_bits` | `qc-codegen`, `qc-progs`, `qc-vm`, `xtask` | kept public: used by `qc-codegen`, `qc-progs`, `qc-vm`, `xtask` |
| `fn qc_types::Slot::from_f32` | `qc-edict`, `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm` | kept public: used by `qc-edict`, `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm` |
| `fn qc_types::Slot::from_i32` | `qc-edict`, `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm` | kept public: used by `qc-edict`, `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm` |
| `fn qc_types::Slot::from_le_bytes` | none | kept public: the progs value vocabulary (TYPE-*) every engine crate and the compiler share |
| `fn qc_types::Slot::to_le_bytes` | `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qc-progs`, `qs-sim`, `qc-vm`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qc-progs`, `qs-sim`, `qc-vm`, `xtask`, `fuzz` |
| `fn qc_types::Value::etype` | `qc-codegen`, `qc-sema`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `xtask` |
| `fn qc_types::Value::read` | `qc-sema`, `qs-vfs`, `qs-install`, `xtask` | kept public: used by `qc-sema`, `qs-vfs`, `qs-install`, `xtask` |
| `fn qc_types::Value::write` | `qc-preproc`, `qc-lno`, `qs-log`, `qs-render`, `qc-builtins`, `qs-layout`, `qs-map`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qc-lno`, `qs-log`, `qs-render`, `qc-builtins`, `qs-layout`, `qs-map`, `desktop`, `xtask` |
| `fn qc_types::ofs_parm` | `qc-builtins`, `qs-sim` | kept public: used by `qc-builtins`, `qs-sim` |
| `struct qc_types::EntityRef` | `qc-edict`, `qc-builtins`, `qs-sim`, `qc-vm` | kept public: used by `qc-edict`, `qc-builtins`, `qs-sim`, `qc-vm` |
| `struct qc_types::FieldRef` | `qc-sema` | kept public: used by `qc-sema` |
| `struct qc_types::FunctionRef` | none | kept public: the progs value vocabulary (TYPE-*) every engine crate and the compiler share |
| `struct qc_types::PointerRef` | `qc-sema`, `qc-edict`, `qc-vm` | kept public: used by `qc-sema`, `qc-edict`, `qc-vm` |
| `struct qc_types::Slot` | `qc-codegen`, `qc-edict`, `qs-differential`, `qc-builtins`, `qc-progs`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask` | kept public: used by `qc-codegen`, `qc-edict`, `qs-differential`, `qc-builtins`, `qc-progs`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask` |
| `struct qc_types::StringRef` | none | kept public: the progs value vocabulary (TYPE-*) every engine crate and the compiler share |
| `struct qc_types::TooFewSlots` | none | kept public: the progs value vocabulary (TYPE-*) every engine crate and the compiler share |

## `qc-vm` (L1)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qc_vm::convert::VM_FLOAT_TO_INT_INVALID` | none | kept public: the VM the preview host and the differentials run (VM-*) |
| `enum qc_vm::error::Divergence` | `qs-differential`, `xtask` | kept public: used by `qs-differential`, `xtask` |
| `enum qc_vm::error::ErrorKind` | `qc-preproc`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-pak`, `qs-log`, `qs-differential`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-sim`, `qc-project`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-pak`, `qs-log`, `qs-differential`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-sim`, `qc-project`, `lsp`, `xtask` |
| `enum qc_vm::error::FieldFault` | none | kept public: the VM the preview host and the differentials run (VM-*) |
| `enum qc_vm::exec::ServerState` | `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim` | kept public: used by `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim` |
| `enum qc_vm::machine::MachineError` | none | kept public: the VM the preview host and the differentials run (VM-*) |
| `fn qc_vm::convert::double_to_float` | `qc-builtins`, `qs-sim` | kept public: used by `qc-builtins`, `qs-sim` |
| `fn qc_vm::convert::double_to_int` | `qc-builtins` | kept public: used by `qc-builtins` |
| `fn qc_vm::convert::float_to_int` | `qc-codegen`, `qc-builtins`, `qs-sim` | kept public: used by `qc-codegen`, `qc-builtins`, `qs-sim` |
| `fn qc_vm::convert::int_to_float` | `qc-codegen`, `qc-builtins`, `qs-sim` | kept public: used by `qc-codegen`, `qc-builtins`, `qs-sim` |
| `fn qc_vm::error::Divergence::statement` | `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-differential`, `xtask`, `fuzz` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-differential`, `xtask`, `fuzz` |
| `fn qc_vm::error::ErrorKind::is_fatal` | `qs-differential`, `qc-builtins` | kept public: used by `qs-differential`, `qc-builtins` |
| `fn qc_vm::error::ErrorKind::resets_depth` | `qs-differential` | kept public: used by `qs-differential` |
| `fn qc_vm::error::RunError::message` | `qc-preproc`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-bsp`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-project` | kept public: used by `qc-preproc`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-bsp`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-project` |
| `fn qc_vm::exec::Builtins::call` | `qc-codegen`, `qc-sema`, `qs-differential`, `qs-sim`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qs-differential`, `qs-sim`, `xtask` |
| `fn qc_vm::exec::Builtins::engine_string` | `qs-differential`, `qc-builtins`, `qs-sim` | kept public: used by `qs-differential`, `qc-builtins`, `qs-sim` |
| `fn qc_vm::exec::Builtins::limit` | `qs-differential`, `qc-builtins`, `qs-sim` | kept public: used by `qs-differential`, `qc-builtins`, `qs-sim` |
| `fn qc_vm::exec::Context::execute_program` | `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim` | kept public: used by `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim` |
| `fn qc_vm::exec::Context::machine` | `qc-builtins`, `qs-sim` | kept public: used by `qc-builtins`, `qs-sim` |
| `fn qc_vm::exec::Context::machine_view` | `qc-builtins`, `qs-sim` | kept public: used by `qc-builtins`, `qs-sim` |
| `fn qc_vm::exec::Context::progs` | `qc-builtins`, `qs-sim` | kept public: used by `qc-builtins`, `qs-sim` |
| `fn qc_vm::exec::Context::run_error` | `qc-builtins`, `qs-sim` | kept public: used by `qc-builtins`, `qs-sim` |
| `fn qc_vm::exec::Context::server` | `qc-builtins` | kept public: used by `qc-builtins` |
| `fn qc_vm::exec::Machine::execute_program` | `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim` | kept public: used by `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim` |
| `fn qc_vm::exec::Observer::error` | `qc-syntax`, `qc-lexer`, `qs-differential`, `qs-sim`, `desktop` | kept public: used by `qc-syntax`, `qc-lexer`, `qs-differential`, `qs-sim`, `desktop` |
| `fn qc_vm::exec::Observer::exit` | `qs-differential` | kept public: used by `qs-differential` |
| `fn qc_vm::exec::Observer::run` | `qc-preproc`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-commands`, `qs-sim`, `qc-project`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-commands`, `qs-sim`, `qc-project`, `desktop`, `xtask` |
| `fn qc_vm::exec::Observer::step` | `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-differential`, `qs-commands`, `desktop` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-differential`, `qs-commands`, `desktop` |
| `fn qc_vm::machine::Machine::argc` | `qc-builtins` | kept public: used by `qc-builtins` |
| `fn qc_vm::machine::Machine::current_statement` | `qc-builtins` | kept public: used by `qc-builtins` |
| `fn qc_vm::machine::Machine::depth` | `qc-builtins` | kept public: used by `qc-builtins` |
| `fn qc_vm::machine::Machine::edicts` | `qs-differential`, `qc-builtins`, `qs-sim` | kept public: used by `qs-differential`, `qc-builtins`, `qs-sim` |
| `fn qc_vm::machine::Machine::edicts_mut` | `qs-differential`, `qc-builtins`, `qs-sim` | kept public: used by `qs-differential`, `qc-builtins`, `qs-sim` |
| `fn qc_vm::machine::Machine::global` | `qc-codegen`, `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim` | kept public: used by `qc-codegen`, `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim` |
| `fn qc_vm::machine::Machine::global3` | `qc-builtins` | kept public: used by `qc-builtins` |
| `fn qc_vm::machine::Machine::globals` | `qc-trace`, `qs-differential`, `qc-progs`, `xtask` | kept public: used by `qc-trace`, `qs-differential`, `qc-progs`, `xtask` |
| `fn qc_vm::machine::Machine::localstack_used` | none | kept public: the VM the preview host and the differentials run (VM-*) |
| `fn qc_vm::machine::Machine::new` | `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim` | kept public: used by `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim` |
| `fn qc_vm::machine::Machine::set_global` | `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim` | kept public: used by `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim` |
| `fn qc_vm::machine::Machine::set_global3` | `qc-builtins` | kept public: used by `qc-builtins` |
| `fn qc_vm::machine::Machine::set_tracing` | `qc-builtins` | kept public: used by `qc-builtins` |
| `fn qc_vm::machine::Machine::tracing` | `qc-builtins` | kept public: used by `qc-builtins` |
| `fn qc_vm::trace::Trace::entityfields` | `qc-trace`, `qc-edict`, `qs-differential`, `qc-builtins`, `qs-sim`, `xtask` | kept public: used by `qc-trace`, `qc-edict`, `qs-differential`, `qc-builtins`, `qs-sim`, `xtask` |
| `fn qc_vm::trace::Trace::finish` | `qc-trace`, `qc-sema`, `qc-syntax`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qs-editor-view`, `desktop`, `xtask` | kept public: used by `qc-trace`, `qc-sema`, `qc-syntax`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qs-editor-view`, `desktop`, `xtask` |
| `fn qc_vm::trace::Trace::new` | `qs-sim` | kept public: used by `qs-sim` |
| `fn qc_vm::trace::Trace::records` | `qs-differential` | kept public: used by `qs-differential` |
| `mod qc_vm::convert` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-layout`, `qs-commands`, `qs-sim` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-layout`, `qs-commands`, `qs-sim` |
| `mod qc_vm::trace` | `qc-trace`, `qs-settings`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-trace`, `qs-settings`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qs-sim`, `xtask`, `fuzz` |
| `struct qc_vm::error::RunError` | `qs-differential`, `qc-builtins`, `qs-sim` | kept public: used by `qs-differential`, `qc-builtins`, `qs-sim` |
| `struct qc_vm::error::StackEntry` | none | kept public: the VM the preview host and the differentials run (VM-*) |
| `struct qc_vm::exec::Context` | `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qs-sim`, `desktop`, `xtask` | kept public: used by `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qs-sim`, `desktop`, `xtask` |
| `struct qc_vm::exec::NoObserver` | `qc-builtins`, `qs-tests`, `qs-sim` | kept public: used by `qc-builtins`, `qs-tests`, `qs-sim` |
| `struct qc_vm::machine::GlobalAccessError` | none | kept public: the VM the preview host and the differentials run (VM-*) |
| `struct qc_vm::machine::Machine` | `qs-differential`, `qs-install`, `qc-builtins`, `qs-tests`, `qs-sim`, `xtask` | kept public: used by `qs-differential`, `qs-install`, `qc-builtins`, `qs-tests`, `qs-sim`, `xtask` |
| `struct qc_vm::trace::Trace` | `qc-trace`, `qs-differential`, `qs-sim`, `xtask` | kept public: used by `qc-trace`, `qs-differential`, `qs-sim`, `xtask` |
| `trait qc_vm::exec::Builtins` | `qs-differential`, `qc-builtins`, `qs-sim` | kept public: used by `qs-differential`, `qc-builtins`, `qs-sim` |
| `trait qc_vm::exec::Observer` | `qs-differential` | kept public: used by `qs-differential` |
| `use qc_vm::Builtins = exec::Builtins` | `qs-differential`, `qc-builtins`, `qs-sim` | kept public: used by `qs-differential`, `qc-builtins`, `qs-sim` |
| `use qc_vm::Context = exec::Context` | `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qs-sim`, `desktop`, `xtask` | kept public: used by `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qs-sim`, `desktop`, `xtask` |
| `use qc_vm::Divergence = error::Divergence` | `qs-differential`, `xtask` | kept public: used by `qs-differential`, `xtask` |
| `use qc_vm::ErrorKind = error::ErrorKind` | `qc-preproc`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-pak`, `qs-log`, `qs-differential`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-sim`, `qc-project`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qs-settings`, `qs-vfs`, `qs-pak`, `qs-log`, `qs-differential`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-sim`, `qc-project`, `lsp`, `xtask` |
| `use qc_vm::FieldFault = error::FieldFault` | none | kept public: the VM the preview host and the differentials run (VM-*) |
| `use qc_vm::GlobalAccessError = machine::GlobalAccessError` | none | kept public: the VM the preview host and the differentials run (VM-*) |
| `use qc_vm::LIMIT_LOCALSTACK_SIZE = qc_progs::limits::LIMIT_LOCALSTACK_SIZE` | `qc-progs` | kept public: used by `qc-progs` |
| `use qc_vm::LIMIT_MAX_STACK_DEPTH = qc_progs::limits::LIMIT_MAX_STACK_DEPTH` | `qc-progs` | kept public: used by `qc-progs` |
| `use qc_vm::LIMIT_NUM_BUILTINS = qc_progs::limits::LIMIT_NUM_BUILTINS` | `qc-builtins`, `qc-progs` | kept public: used by `qc-builtins`, `qc-progs` |
| `use qc_vm::LIMIT_RUNAWAY = qc_progs::limits::LIMIT_RUNAWAY` | `qc-progs` | kept public: used by `qc-progs` |
| `use qc_vm::Machine = machine::Machine` | `qs-differential`, `qs-install`, `qc-builtins`, `qs-tests`, `qs-sim`, `xtask` | kept public: used by `qs-differential`, `qs-install`, `qc-builtins`, `qs-tests`, `qs-sim`, `xtask` |
| `use qc_vm::MachineError = machine::MachineError` | none | kept public: the VM the preview host and the differentials run (VM-*) |
| `use qc_vm::NoObserver = exec::NoObserver` | `qc-builtins`, `qs-tests`, `qs-sim` | kept public: used by `qc-builtins`, `qs-tests`, `qs-sim` |
| `use qc_vm::Observer = exec::Observer` | `qs-differential` | kept public: used by `qs-differential` |
| `use qc_vm::RunError = error::RunError` | `qs-differential`, `qc-builtins`, `qs-sim` | kept public: used by `qs-differential`, `qc-builtins`, `qs-sim` |
| `use qc_vm::ServerState = exec::ServerState` | `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim` | kept public: used by `qs-differential`, `qc-builtins`, `qs-tests`, `qs-sim` |
| `use qc_vm::StackEntry = error::StackEntry` | none | kept public: the VM the preview host and the differentials run (VM-*) |

Items that left the root (3): `fn qc_vm::error::ErrorKind::is_executed_statement`, `struct qc_vm::exec::NoBuiltins`, `use qc_vm::NoBuiltins = exec::NoBuiltins`. Each is `pub(crate)` now, under `#[cfg(test)]` where only the crate's tests read it; `qc_sema::nav::ModuleIndex::references_at` is removed.

## `qs-bsp` (L0)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qs_bsp::BSP_2PSB_LEAF_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_2PSB_NODE_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_2PSB_VERSION` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_ANIM_CYCLE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_BSP2_CLIPNODE_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_BSP2_EDGE_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_BSP2_FACE_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_BSP2_LEAF_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_BSP2_MARKSURFACE_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_BSP2_NODE_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_BSP2_VERSION` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_CLIPNODE_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_CONTENTS_EMPTY` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_CONTENTS_LAVA` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_CONTENTS_SKY` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_CONTENTS_SLIME` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_CONTENTS_SOLID` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_CONTENTS_WATER` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_EDGE_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_FACE_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_FITZ_MAX_LEAFS` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_FITZ_MAX_SURFACE_EXTENT` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_GL_MAX_SURFACE_EXTENT` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_HEADER_LUMPS` | `qs-render`, `qs-sim` | kept public: used by `qs-render`, `qs-sim` |
| `const qs_bsp::BSP_HEADER_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_HULL1_MAXS` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_HULL1_MINS` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_HULL2_MAXS` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_HULL2_MINS` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_LEAF_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_LIGHTMAP_SCALE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_MAXLIGHTMAPS` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_MAX_MAP_HULLS` | `qs-render`, `qs-sim` | kept public: used by `qs-render`, `qs-sim` |
| `const qs_bsp::BSP_MAX_MAP_LEAFS` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_MAX_SURFACE_EXTENT` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_MAX_VIS_ROW` | `fuzz` | kept public: used by `fuzz` |
| `const qs_bsp::BSP_MIPLEVELS` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_MIPTEX_HEADER_SIZE` | `qs-render` | kept public: used by `qs-render` |
| `const qs_bsp::BSP_MODEL_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_NODE_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_NUM_AMBIENTS` | `qs-sim` | kept public: used by `qs-sim` |
| `const qs_bsp::BSP_PLANE_ANYX` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_PLANE_ANYY` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_PLANE_ANYZ` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_PLANE_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_PLANE_X` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_PLANE_Y` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_PLANE_Z` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_SURFEDGE_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_TEXINFO_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_TEX_MISSING` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_TEX_SPECIAL` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_VERSION` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::BSP_VERTEX_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::LumpId::ALL` | `qc-ide`, `qc-preproc`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `const qs_bsp::SURF_DRAWFENCE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::SURF_DRAWLAVA` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::SURF_DRAWSKY` | `qs-render` | kept public: used by `qs-render` |
| `const qs_bsp::SURF_DRAWSLIME` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::SURF_DRAWTELE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::SURF_DRAWTILED` | `qs-render` | kept public: used by `qs-render` |
| `const qs_bsp::SURF_DRAWTURB` | `qs-render` | kept public: used by `qs-render` |
| `const qs_bsp::SURF_DRAWWATER` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::SURF_NOTEXTURE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::SURF_PLANEBACK` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::SURF_UNDERWATER` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::ent::BSP_ENT_EXTENSION` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_KEY` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_MAP_BRUSHES` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_MAP_CLIPNODES` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_MAP_EDGES` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_MAP_ENTITIES` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_MAP_ENTSTRING` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_MAP_FACES` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_MAP_LIGHTING` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_MAP_MARKSURFACES` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_MAP_MIPTEX` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_MAP_MODELS` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_MAP_NODES` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_MAP_PLANES` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_MAP_SURFEDGES` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_MAP_TEXINFO` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_MAP_TEXTURES` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_MAP_VERTS` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_MAP_VISIBILITY` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::limits::BSP_MAX_VALUE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::lit::BSP_LIT_HEADER_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::lit::BSP_LIT_MAGIC` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::lit::BSP_LIT_VERSION` | none | kept public: the BSP format's model (BSP-*) |
| `const qs_bsp::lit::PathId::FIRST` | `qs-vfs` | kept public: used by `qs-vfs` |
| `enum qs_bsp::BspError` | none | kept public: the BSP format's model (BSP-*) |
| `enum qs_bsp::BspLoader` | `qs-vfs`, `qs-render`, `qs-tests`, `qs-sim`, `fuzz` | kept public: used by `qs-vfs`, `qs-render`, `qs-tests`, `qs-sim`, `fuzz` |
| `enum qs_bsp::BspVersion` | `qs-render`, `qs-sim` | kept public: used by `qs-render`, `qs-sim` |
| `enum qs_bsp::LumpId` | none | kept public: the BSP format's model (BSP-*) |
| `enum qs_bsp::lit::LitChoice` | `qs-vfs` | kept public: used by `qs-vfs` |
| `enum qs_bsp::lit::LitError` | none | kept public: the BSP format's model (BSP-*) |
| `fn qs_bsp::Bsp::entities` | `qs-vfs`, `qs-render`, `qs-tests`, `qs-sim`, `fuzz` | kept public: used by `qs-vfs`, `qs-render`, `qs-tests`, `qs-sim`, `fuzz` |
| `fn qs_bsp::Bsp::face_samples` | `qs-render`, `fuzz` | kept public: used by `qs-render`, `fuzz` |
| `fn qs_bsp::Bsp::face_vertices` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_bsp::Bsp::hull0` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_bsp::Bsp::leaf_pvs` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_bsp::Bsp::read` | `qc-sema`, `qs-vfs`, `qs-render`, `qs-install`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `qs-vfs`, `qs-render`, `qs-install`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` |
| `fn qs_bsp::Bsp::vis_row_size` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_bsp::BspError::message` | `qc-preproc`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm` | kept public: used by `qc-preproc`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm` |
| `fn qs_bsp::BspLoader::max_surface_extent` | none | kept public: the BSP format's model (BSP-*) |
| `fn qs_bsp::Face::style_count` | `qs-render` | kept public: used by `qs-render` |
| `fn qs_bsp::LumpId::index` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qc-lno`, `qc-edict`, `qs-differential`, `qs-bench`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qc-lno`, `qc-edict`, `qs-differential`, `qs-bench`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-vm`, `xtask`, `fuzz` |
| `fn qs_bsp::LumpId::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qs_bsp::Texture::mip` | `qs-render` | kept public: used by `qs-render` |
| `fn qs_bsp::ent::ent_file_names` | `qs-tests` | kept public: used by `qs-tests` |
| `fn qs_bsp::hull_for_size` | none | kept public: the BSP format's model (BSP-*) |
| `fn qs_bsp::lit::Lit::read` | `qc-sema`, `qs-vfs`, `qs-render`, `qs-install`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `qs-vfs`, `qs-render`, `qs-install`, `xtask`, `fuzz` |
| `fn qs_bsp::lit::Lit::sample` | `qs-wav`, `qs-render`, `fuzz` | kept public: used by `qs-wav`, `qs-render`, `fuzz` |
| `fn qs_bsp::lit::LitChoice::developer_message` | none | kept public: the BSP format's model (BSP-*) |
| `fn qs_bsp::lit::LitChoice::message` | `qc-preproc`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm` | kept public: used by `qc-preproc`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm` |
| `fn qs_bsp::lit::LitChoice::rgb` | none | kept public: the BSP format's model (BSP-*) |
| `fn qs_bsp::lit::LitError::message` | `qc-preproc`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm` | kept public: used by `qc-preproc`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm` |
| `fn qs_bsp::lit::PathId::admits_companion_from` | none | kept public: the BSP format's model (BSP-*) |
| `fn qs_bsp::lit::PathId::next` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qc-syntax`, `qc-diag`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-porting-example`, `qs-map`, `qc-progs`, `qs-editor-view`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qc-syntax`, `qc-diag`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-porting-example`, `qs-map`, `qc-progs`, `qs-editor-view`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qs_bsp::lit::companion_name` | `qs-vfs` | kept public: used by `qs-vfs` |
| `fn qs_bsp::lit::grey` | `qs-render`, `xtask` | kept public: used by `qs-render`, `xtask` |
| `fn qs_bsp::lit::mod_load_lighting` | `qs-vfs` | kept public: used by `qs-vfs` |
| `fn qs_bsp::model_name` | `qc-preproc` | kept public: used by `qc-preproc` |
| `fn qs_bsp::model_precache_index` | none | kept public: the BSP format's model (BSP-*) |
| `fn qs_bsp::replace_entities` | `qs-tests`, `fuzz` | kept public: used by `qs-tests`, `fuzz` |
| `mod qs_bsp::ent` | `qs-differential`, `qc-builtins`, `qs-map`, `qc-engines`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qs-differential`, `qc-builtins`, `qs-map`, `qc-engines`, `qs-tests`, `qs-sim`, `xtask`, `fuzz` |
| `mod qs_bsp::limits` | `qc-preproc`, `qs-mdl`, `qc-syntax`, `qc-edict`, `qs-log`, `qs-differential`, `qc-builtins`, `qc-progs`, `qs-commands`, `qc-vm`, `xtask` | kept public: used by `qc-preproc`, `qs-mdl`, `qc-syntax`, `qc-edict`, `qs-log`, `qs-differential`, `qc-builtins`, `qc-progs`, `qs-commands`, `qc-vm`, `xtask` |
| `mod qs_bsp::lit` | `qc-codegen`, `qs-vfs`, `qc-syntax`, `qs-render`, `fuzz` | kept public: used by `qc-codegen`, `qs-vfs`, `qc-syntax`, `qs-render`, `fuzz` |
| `struct qs_bsp::Animation` | `qs-render` | kept public: used by `qs-render` |
| `struct qs_bsp::Bsp` | `qs-vfs`, `qs-render`, `qs-tests`, `qs-sim`, `fuzz` | kept public: used by `qs-vfs`, `qs-render`, `qs-tests`, `qs-sim`, `fuzz` |
| `struct qs_bsp::ClipNode` | `qs-sim` | kept public: used by `qs-sim` |
| `struct qs_bsp::Face` | `qs-render`, `qs-map` | kept public: used by `qs-render`, `qs-map` |
| `struct qs_bsp::Leaf` | `qc-syntax`, `qs-sim` | kept public: used by `qc-syntax`, `qs-sim` |
| `struct qs_bsp::Lump` | `qs-wad`, `qs-render`, `qs-sim` | kept public: used by `qs-wad`, `qs-render`, `qs-sim` |
| `struct qs_bsp::Model` | `qc-sema`, `qc-edict`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-sim`, `xtask` | kept public: used by `qc-sema`, `qc-edict`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-sim`, `xtask` |
| `struct qs_bsp::Node` | `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-render`, `qs-editor-view`, `qs-sim` | kept public: used by `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-render`, `qs-editor-view`, `qs-sim` |
| `struct qs_bsp::Plane` | `qs-render`, `qs-map`, `qs-sim` | kept public: used by `qs-render`, `qs-map`, `qs-sim` |
| `struct qs_bsp::Texinfo` | `qs-render` | kept public: used by `qs-render` |
| `struct qs_bsp::Texture` | `qc-sema`, `qs-mdl`, `qs-render`, `desktop` | kept public: used by `qc-sema`, `qs-mdl`, `qs-render`, `desktop` |
| `struct qs_bsp::lit::FoundLit` | `qs-vfs` | kept public: used by `qs-vfs` |
| `struct qs_bsp::lit::Lit` | `qs-render`, `desktop`, `fuzz` | kept public: used by `qs-render`, `desktop`, `fuzz` |
| `struct qs_bsp::lit::PathId` | `qs-vfs` | kept public: used by `qs-vfs` |
| `use qs_bsp::BSP_ENT_EXTENSION = ent::BSP_ENT_EXTENSION` | none | kept public: the BSP format's model (BSP-*) |
| `use qs_bsp::BSP_LIT_HEADER_SIZE = lit::BSP_LIT_HEADER_SIZE` | none | kept public: the BSP format's model (BSP-*) |
| `use qs_bsp::BSP_LIT_MAGIC = lit::BSP_LIT_MAGIC` | none | kept public: the BSP format's model (BSP-*) |
| `use qs_bsp::BSP_LIT_VERSION = lit::BSP_LIT_VERSION` | none | kept public: the BSP format's model (BSP-*) |
| `use qs_bsp::FoundLit = lit::FoundLit` | `qs-vfs` | kept public: used by `qs-vfs` |
| `use qs_bsp::Lit = lit::Lit` | `qs-render`, `desktop`, `fuzz` | kept public: used by `qs-render`, `desktop`, `fuzz` |
| `use qs_bsp::LitChoice = lit::LitChoice` | `qs-vfs` | kept public: used by `qs-vfs` |
| `use qs_bsp::LitError = lit::LitError` | none | kept public: the BSP format's model (BSP-*) |
| `use qs_bsp::PathId = lit::PathId` | `qs-vfs` | kept public: used by `qs-vfs` |
| `use qs_bsp::ent_file_names = ent::ent_file_names` | `qs-tests` | kept public: used by `qs-tests` |
| `use qs_bsp::mod_load_lighting = lit::mod_load_lighting` | `qs-vfs` | kept public: used by `qs-vfs` |

## `qs-commands` (L5)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qs_commands::command::Command::ID` | `qc-ide`, `qc-preproc`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-diag`, `qs-layout`, `qs-tests`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-diag`, `qs-layout`, `qs-tests`, `xtask` |
| `const qs_commands::command::Command::SUMMARY` | `qs-settings`, `qs-workbench`, `qs-layout` | kept public: used by `qs-settings`, `qs-workbench`, `qs-layout` |
| `enum qs_commands::error::CommandError` | `qs-settings`, `qs-workbench`, `qs-layout`, `desktop` | kept public: used by `qs-settings`, `qs-workbench`, `qs-layout`, `desktop` |
| `enum qs_commands::error::RegisterError` | `qs-settings`, `qs-workbench`, `qs-layout`, `desktop` | kept public: used by `qs-settings`, `qs-workbench`, `qs-layout`, `desktop` |
| `enum qs_commands::id::IdError` | none | kept public: named in a public signature a dependent uses |
| `enum qs_commands::params::FieldKind` | `desktop` | kept public: used by `desktop` |
| `enum qs_commands::params::Input` | `qc-syntax`, `qc-lexer`, `qs-differential`, `qs-tests`, `desktop`, `xtask` | kept public: used by `qc-syntax`, `qc-lexer`, `qs-differential`, `qs-tests`, `desktop`, `xtask` |
| `enum qs_commands::params::ParamError` | `qc-preproc` | kept public: used by `qc-preproc` |
| `fn qs_commands::command::Command::run` | `qc-preproc`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `xtask` |
| `fn qs_commands::command::Outcome::outside_history` | `qs-workbench`, `qs-layout` | kept public: used by `qs-workbench`, `qs-layout` |
| `fn qs_commands::command::Outcome::unchanged` | `qs-settings`, `qs-workbench` | kept public: used by `qs-settings`, `qs-workbench` |
| `fn qs_commands::command::Outcome::undoable` | `qs-settings`, `qs-workbench` | kept public: used by `qs-settings`, `qs-workbench` |
| `fn qs_commands::command::UndoStep::redo` | `qs-settings`, `qs-workbench`, `qs-editor-view` | kept public: used by `qs-settings`, `qs-workbench`, `qs-editor-view` |
| `fn qs_commands::command::UndoStep::undo` | `qs-settings`, `qs-workbench`, `qs-layout`, `qs-editor-view`, `fuzz` | kept public: used by `qs-settings`, `qs-workbench`, `qs-layout`, `qs-editor-view`, `fuzz` |
| `fn qs_commands::id::CommandId::as_str` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qc-project`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-settings`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qc-project`, `desktop`, `lsp`, `xtask` |
| `fn qs_commands::id::CommandId::new` | none | kept public: named in a public signature a dependent uses |
| `fn qs_commands::palette::search` | `qc-preproc`, `qs-wav`, `qs-vfs`, `qc-syntax`, `qs-bsp`, `qc-lexer`, `qs-render`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qs-paths`, `desktop` | kept public: used by `qc-preproc`, `qs-wav`, `qs-vfs`, `qc-syntax`, `qs-bsp`, `qc-lexer`, `qs-render`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qs-paths`, `desktop` |
| `fn qs_commands::params::Input::empty` | `desktop` | kept public: used by `desktop` |
| `fn qs_commands::params::fields` | `qc-ide`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-diag`, `qs-log`, `qs-differential`, `qs-render`, `qc-builtins`, `qc-progs`, `qs-tests`, `qs-sim`, `qc-vm`, `desktop`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-diag`, `qs-log`, `qs-differential`, `qs-render`, `qc-builtins`, `qc-progs`, `qs-tests`, `qs-sim`, `qc-vm`, `desktop`, `xtask`, `fuzz` |
| `fn qs_commands::params::object` | `qc-preproc`, `qc-trace`, `qc-sema`, `qs-settings`, `qc-vm`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qc-trace`, `qc-sema`, `qs-settings`, `qc-vm`, `desktop`, `xtask` |
| `fn qs_commands::registry::CommandInfo::id` | `qs-editor-view`, `qc-engines`, `desktop` | kept public: used by `qs-editor-view`, `qc-engines`, `desktop` |
| `fn qs_commands::registry::CommandInfo::params` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-settings`, `qc-syntax`, `desktop`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-settings`, `qc-syntax`, `desktop`, `xtask` |
| `fn qs_commands::registry::CommandInfo::summary` | `qs-settings`, `desktop` | kept public: used by `qs-settings`, `desktop` |
| `fn qs_commands::registry::Registry::commands` | `desktop` | kept public: used by `desktop` |
| `fn qs_commands::registry::Registry::get` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qs_commands::registry::Registry::markdown` | `xtask` | kept public: used by `xtask` |
| `fn qs_commands::registry::Registry::new` | `qs-settings`, `qs-workbench`, `qs-layout`, `desktop`, `fuzz` | kept public: used by `qs-settings`, `qs-workbench`, `qs-layout`, `desktop`, `fuzz` |
| `fn qs_commands::registry::Registry::register` | `qs-settings`, `qs-workbench`, `qs-layout` | kept public: used by `qs-settings`, `qs-workbench`, `qs-layout` |
| `fn qs_commands::session::Session::context` | `qs-settings`, `qs-workbench`, `qs-layout`, `desktop`, `fuzz` | kept public: used by `qs-settings`, `qs-workbench`, `qs-layout`, `desktop`, `fuzz` |
| `fn qs_commands::session::Session::execute` | `qs-settings`, `qs-workbench`, `qs-layout`, `desktop`, `fuzz` | kept public: used by `qs-settings`, `qs-workbench`, `qs-layout`, `desktop`, `fuzz` |
| `fn qs_commands::session::Session::new` | `qs-settings`, `qs-workbench`, `qs-layout`, `desktop`, `fuzz` | kept public: used by `qs-settings`, `qs-workbench`, `qs-layout`, `desktop`, `fuzz` |
| `fn qs_commands::session::Session::redo` | `qs-settings`, `qs-workbench`, `qs-editor-view` | kept public: used by `qs-settings`, `qs-workbench`, `qs-editor-view` |
| `fn qs_commands::session::Session::redo_depth` | `qs-settings` | kept public: used by `qs-settings` |
| `fn qs_commands::session::Session::registry` | `qs-settings`, `desktop` | kept public: used by `qs-settings`, `desktop` |
| `fn qs_commands::session::Session::undo` | `qs-settings`, `qs-workbench`, `qs-layout`, `qs-editor-view`, `fuzz` | kept public: used by `qs-settings`, `qs-workbench`, `qs-layout`, `qs-editor-view`, `fuzz` |
| `fn qs_commands::session::Session::undo_depth` | `qs-settings`, `qs-workbench`, `desktop` | kept public: used by `qs-settings`, `qs-workbench`, `desktop` |
| `mod qs_commands::palette` | `qs-wad`, `qs-mdl`, `qs-workbench`, `qs-render`, `desktop`, `xtask` | kept public: used by `qs-wad`, `qs-mdl`, `qs-workbench`, `qs-render`, `desktop`, `xtask` |
| `mod qs_commands::params` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qs-layout`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qs-layout`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `struct qs_commands::command::NoParams` | `qs-settings` | kept public: used by `qs-settings` |
| `struct qs_commands::command::Outcome` | `qs-settings`, `qs-workbench`, `qs-differential`, `qs-layout`, `xtask` | kept public: used by `qs-settings`, `qs-workbench`, `qs-differential`, `qs-layout`, `xtask` |
| `struct qs_commands::id::CommandId` | `desktop` | kept public: used by `desktop` |
| `struct qs_commands::palette::PaletteMatch` | `desktop` | kept public: used by `desktop` |
| `struct qs_commands::params::Choice` | `qc-sema`, `desktop` | kept public: used by `qc-sema`, `desktop` |
| `struct qs_commands::params::Field` | `qc-ide`, `qc-codegen`, `qc-trace`, `qc-types`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-edict`, `qs-log`, `qs-differential`, `qc-progs`, `qs-sim`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-trace`, `qc-types`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-edict`, `qs-log`, `qs-differential`, `qc-progs`, `qs-sim`, `desktop`, `lsp`, `xtask` |
| `struct qs_commands::registry::CommandInfo` | `desktop` | kept public: used by `desktop` |
| `struct qs_commands::registry::Registry` | `qs-settings`, `qs-workbench`, `qs-log`, `qs-install`, `qs-layout`, `desktop`, `xtask`, `fuzz` | kept public: used by `qs-settings`, `qs-workbench`, `qs-log`, `qs-install`, `qs-layout`, `desktop`, `xtask`, `fuzz` |
| `struct qs_commands::session::Session` | `qs-settings`, `qs-workbench`, `qs-layout`, `desktop`, `lsp`, `fuzz` | kept public: used by `qs-settings`, `qs-workbench`, `qs-layout`, `desktop`, `lsp`, `fuzz` |
| `trait qs_commands::command::Command` | `qs-settings`, `qs-workbench`, `qs-differential`, `qs-install`, `qs-layout`, `desktop`, `xtask`, `fuzz` | kept public: used by `qs-settings`, `qs-workbench`, `qs-differential`, `qs-install`, `qs-layout`, `desktop`, `xtask`, `fuzz` |
| `trait qs_commands::command::UndoStep` | `qs-settings`, `qs-workbench` | kept public: used by `qs-settings`, `qs-workbench` |
| `type qs_commands::command::BoxError` | `qs-settings`, `qs-workbench` | kept public: used by `qs-settings`, `qs-workbench` |
| `type qs_commands::command::Command::Error` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-map`, `qc-progs`, `qs-paths`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-types`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-map`, `qc-progs`, `qs-paths`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `type qs_commands::command::Command::Output` | `qc-preproc`, `qs-settings`, `qs-workbench`, `qc-lexer`, `qc-builtins`, `qs-layout`, `qs-map`, `qc-project`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qs-settings`, `qs-workbench`, `qc-lexer`, `qc-builtins`, `qs-layout`, `qs-map`, `qc-project`, `xtask`, `fuzz` |
| `type qs_commands::command::Command::Params` | `qs-settings`, `qs-workbench`, `qs-layout`, `desktop` | kept public: used by `qs-settings`, `qs-workbench`, `qs-layout`, `desktop` |
| `use qs_commands::BoxError = command::BoxError` | `qs-settings`, `qs-workbench` | kept public: used by `qs-settings`, `qs-workbench` |
| `use qs_commands::Command = command::Command` | `qs-settings`, `qs-workbench`, `qs-differential`, `qs-install`, `qs-layout`, `desktop`, `xtask`, `fuzz` | kept public: used by `qs-settings`, `qs-workbench`, `qs-differential`, `qs-install`, `qs-layout`, `desktop`, `xtask`, `fuzz` |
| `use qs_commands::CommandError = error::CommandError` | `qs-settings`, `qs-workbench`, `qs-layout`, `desktop` | kept public: used by `qs-settings`, `qs-workbench`, `qs-layout`, `desktop` |
| `use qs_commands::CommandId = id::CommandId` | `desktop` | kept public: used by `desktop` |
| `use qs_commands::CommandInfo = registry::CommandInfo` | `desktop` | kept public: used by `desktop` |
| `use qs_commands::IdError = id::IdError` | none | kept public: named in a public signature a dependent uses |
| `use qs_commands::NoParams = command::NoParams` | `qs-settings` | kept public: used by `qs-settings` |
| `use qs_commands::Outcome = command::Outcome` | `qs-settings`, `qs-workbench`, `qs-differential`, `qs-layout`, `xtask` | kept public: used by `qs-settings`, `qs-workbench`, `qs-differential`, `qs-layout`, `xtask` |
| `use qs_commands::RegisterError = error::RegisterError` | `qs-settings`, `qs-workbench`, `qs-layout`, `desktop` | kept public: used by `qs-settings`, `qs-workbench`, `qs-layout`, `desktop` |
| `use qs_commands::Registry = registry::Registry` | `qs-settings`, `qs-workbench`, `qs-log`, `qs-install`, `qs-layout`, `desktop`, `xtask`, `fuzz` | kept public: used by `qs-settings`, `qs-workbench`, `qs-log`, `qs-install`, `qs-layout`, `desktop`, `xtask`, `fuzz` |
| `use qs_commands::Session = session::Session` | `qs-settings`, `qs-workbench`, `qs-layout`, `desktop`, `lsp`, `fuzz` | kept public: used by `qs-settings`, `qs-workbench`, `qs-layout`, `desktop`, `lsp`, `fuzz` |
| `use qs_commands::UndoStep = command::UndoStep` | `qs-settings`, `qs-workbench` | kept public: used by `qs-settings`, `qs-workbench` |

Items that left the root (4): `const qs_commands::builtin::BUILTIN_IDS`, `const qs_commands::id::MAX_ID_LEN`, `use qs_commands::BUILTIN_IDS = builtin::BUILTIN_IDS`, `use qs_commands::MAX_ID_LEN = id::MAX_ID_LEN`. Each is `pub(crate)` now, under `#[cfg(test)]` where only the crate's tests read it; `qc_sema::nav::ModuleIndex::references_at` is removed.

## `qs-install` (L0)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qs_install::GOG_QUAKE_ENHANCED_ID` | none | kept public: the install detection the app's first run drives (INST-*) |
| `const qs_install::GOG_QUAKE_ID` | none | kept public: the install detection the app's first run drives (INST-*) |
| `const qs_install::STEAM_APP_ID` | none | kept public: the install detection the app's first run drives (INST-*) |
| `enum qs_install::Edition` | none | kept public: the install detection the app's first run drives (INST-*) |
| `enum qs_install::MissingReason` | none | kept public: the install detection the app's first run drives (INST-*) |
| `enum qs_install::Source` | `qc-preproc`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qs-log`, `qc-builtins`, `qc-engines`, `qc-project`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qs-log`, `qc-builtins`, `qc-engines`, `qc-project`, `desktop`, `xtask` |
| `enum qs_install::known::FileStatus` | none | kept public: the install detection the app's first run drives (INST-*) |
| `enum qs_install::known::Release` | none | kept public: the install detection the app's first run drives (INST-*) |
| `enum qs_install::link::LinkError` | none | kept public: the install detection the app's first run drives (INST-*) |
| `enum qs_install::link::LinkOutcome` | none | kept public: the install detection the app's first run drives (INST-*) |
| `fn qs_install::Finder::examine` | none | kept public: the install detection the app's first run drives (INST-*) |
| `fn qs_install::Finder::find` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qs_install::Finder::new` | none | kept public: the install detection the app's first run drives (INST-*) |
| `fn qs_install::Install::validate` | `qs-render` | kept public: used by `qs-render` |
| `fn qs_install::InstallFs::canonical` | none | kept public: the install detection the app's first run drives (INST-*) |
| `fn qs_install::examine_folder` | none | kept public: the install detection the app's first run drives (INST-*) |
| `fn qs_install::find_installs` | none | kept public: the install detection the app's first run drives (INST-*) |
| `fn qs_install::known::KnownFile::is_original_pak0` | none | kept public: the install detection the app's first run drives (INST-*) |
| `fn qs_install::known::KnownFile::is_registered_pak1` | none | kept public: the install detection the app's first run drives (INST-*) |
| `fn qs_install::known::identify` | none | kept public: the install detection the app's first run drives (INST-*) |
| `fn qs_install::known::size_and_md5` | none | kept public: the install detection the app's first run drives (INST-*) |
| `fn qs_install::link::link_dir` | none | kept public: the install detection the app's first run drives (INST-*) |
| `fn qs_install::registry::Registry::string` | `qc-trace`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qc-progs`, `qc-vm`, `xtask` | kept public: used by `qc-trace`, `qc-lexer`, `qs-differential`, `qc-builtins`, `qc-progs`, `qc-vm`, `xtask` |
| `static qs_install::known::FS_KNOWN_FILES` | none | kept public: the install detection the app's first run drives (INST-*) |
| `struct qs_install::CheckedFile` | none | kept public: the install detection the app's first run drives (INST-*) |
| `struct qs_install::Finder` | none | kept public: the install detection the app's first run drives (INST-*) |
| `struct qs_install::GameBase` | none | kept public: the install detection the app's first run drives (INST-*) |
| `struct qs_install::GameDir` | none | kept public: the install detection the app's first run drives (INST-*) |
| `struct qs_install::Install` | none | kept public: the install detection the app's first run drives (INST-*) |
| `struct qs_install::Missing` | `qs-wav`, `qc-sema`, `qs-vfs`, `qc-syntax`, `qs-bsp`, `qc-diag`, `qs-differential`, `qs-commands`, `desktop`, `xtask` | kept public: used by `qs-wav`, `qc-sema`, `qs-vfs`, `qc-syntax`, `qs-bsp`, `qc-diag`, `qs-differential`, `qs-commands`, `desktop`, `xtask` |
| `struct qs_install::Validation` | none | kept public: the install detection the app's first run drives (INST-*) |
| `struct qs_install::known::KnownFile` | none | kept public: the install detection the app's first run drives (INST-*) |
| `struct qs_install::registry::SystemRegistry` | none | kept public: the install detection the app's first run drives (INST-*) |
| `trait qs_install::InstallFs` | none | kept public: the install detection the app's first run drives (INST-*) |
| `trait qs_install::registry::Registry` | `qs-settings`, `qs-workbench`, `qs-log`, `qs-layout`, `qs-commands`, `desktop`, `xtask`, `fuzz` | kept public: used by `qs-settings`, `qs-workbench`, `qs-log`, `qs-layout`, `qs-commands`, `desktop`, `xtask`, `fuzz` |
| `use qs_install::FS_KNOWN_FILES = known::FS_KNOWN_FILES` | none | kept public: the install detection the app's first run drives (INST-*) |
| `use qs_install::FileStatus = known::FileStatus` | none | kept public: the install detection the app's first run drives (INST-*) |
| `use qs_install::KnownFile = known::KnownFile` | none | kept public: the install detection the app's first run drives (INST-*) |
| `use qs_install::LinkError = link::LinkError` | none | kept public: the install detection the app's first run drives (INST-*) |
| `use qs_install::LinkOutcome = link::LinkOutcome` | none | kept public: the install detection the app's first run drives (INST-*) |
| `use qs_install::Registry = registry::Registry` | `qs-settings`, `qs-workbench`, `qs-log`, `qs-layout`, `qs-commands`, `desktop`, `xtask`, `fuzz` | kept public: used by `qs-settings`, `qs-workbench`, `qs-log`, `qs-layout`, `qs-commands`, `desktop`, `xtask`, `fuzz` |
| `use qs_install::Release = known::Release` | none | kept public: the install detection the app's first run drives (INST-*) |
| `use qs_install::SystemRegistry = registry::SystemRegistry` | none | kept public: the install detection the app's first run drives (INST-*) |
| `use qs_install::identify = known::identify` | none | kept public: the install detection the app's first run drives (INST-*) |
| `use qs_install::link_dir = link::link_dir` | none | kept public: the install detection the app's first run drives (INST-*) |
| `use qs_install::size_and_md5 = known::size_and_md5` | none | kept public: the install detection the app's first run drives (INST-*) |

## `qs-layout` (L5)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qs_layout::MIN_PANEL_SIZE` | `desktop` | kept public: used by `desktop` |
| `const qs_layout::Panel::ALL` | `qc-ide`, `qc-preproc`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-types`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `const qs_layout::RESIZE_PANEL` | `desktop` | kept public: used by `desktop` |
| `const qs_layout::TOGGLE_PANEL` | `desktop` | kept public: used by `desktop` |
| `enum qs_layout::LayoutError` | `qc-edict`, `qc-vm` | kept public: used by `qc-edict`, `qc-vm` |
| `enum qs_layout::Panel` | `qs-commands`, `desktop` | kept public: used by `qs-commands`, `desktop` |
| `fn qs_layout::HasLayout::layout_mut` | none | kept public: named in a public signature a dependent uses |
| `fn qs_layout::Layout::panel` | `desktop` | kept public: used by `desktop` |
| `fn qs_layout::Layout::panel_mut` | none | kept public: named in a public signature a dependent uses |
| `fn qs_layout::Layout::read` | `qc-sema`, `qs-vfs`, `qs-install`, `desktop`, `xtask` | kept public: used by `qc-sema`, `qs-vfs`, `qs-install`, `desktop`, `xtask` |
| `fn qs_layout::Layout::write` | `qc-preproc`, `qc-types`, `qc-lno`, `qs-log`, `qs-render`, `qc-builtins`, `qs-map`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qc-types`, `qc-lno`, `qs-log`, `qs-render`, `qc-builtins`, `qs-map`, `desktop`, `xtask` |
| `fn qs_layout::layout_path` | `desktop` | kept public: used by `desktop` |
| `fn qs_layout::register` | `qc-preproc`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qs-commands`, `qc-project`, `desktop`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-sema`, `qs-settings`, `qs-workbench`, `qc-syntax`, `qs-commands`, `qc-project`, `desktop`, `xtask`, `fuzz` |
| `struct qs_layout::Layout` | `qc-sema`, `qc-edict`, `qc-progs`, `qc-vm`, `desktop`, `xtask` | kept public: used by `qc-sema`, `qc-edict`, `qc-progs`, `qc-vm`, `desktop`, `xtask` |
| `struct qs_layout::PanelState` | `desktop` | kept public: used by `desktop` |
| `struct qs_layout::ResizePanelParams` | `desktop` | kept public: used by `desktop` |
| `struct qs_layout::TogglePanelParams` | `desktop` | kept public: used by `desktop` |
| `trait qs_layout::HasLayout` | `desktop`, `xtask` | kept public: used by `desktop`, `xtask` |

Items that left the root (3): `const qs_layout::LAYOUT_FILE`, `struct qs_layout::BadPanelSize`, `struct qs_layout::PanelShown`. Each is `pub(crate)` now, under `#[cfg(test)]` where only the crate's tests read it; `qc_sema::nav::ModuleIndex::references_at` is removed.

## `qs-log` (L0)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qs_log::DEFAULT_DIRECTIVES` | none | kept public: the logging policy the app and the LSP server install (LOG-*) |
| `const qs_log::LOG_FILE_STEM` | `lsp` | kept public: used by `lsp` |
| `const qs_log::redact::SOURCE_FIELD_NAMES` | none | kept public: the logging policy the app and the LSP server install (LOG-*) |
| `enum qs_log::LogError` | none | kept public: the logging policy the app and the LSP server install (LOG-*) |
| `fn qs_log::LogConfig::for_app` | `desktop`, `lsp` | kept public: used by `desktop`, `lsp` |
| `fn qs_log::LogConfig::new` | none | kept public: the logging policy the app and the LSP server install (LOG-*) |
| `fn qs_log::directives_from` | none | kept public: the logging policy the app and the LSP server install (LOG-*) |
| `fn qs_log::init` | `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qs_log::panic::install_panic_hook` | none | kept public: the logging policy the app and the LSP server install (LOG-*) |
| `fn qs_log::subscriber` | none | kept public: the logging policy the app and the LSP server install (LOG-*) |
| `struct qs_log::LogConfig` | `desktop`, `lsp` | kept public: used by `desktop`, `lsp` |
| `struct qs_log::redact::SourceText` | none | kept public: the logging policy the app and the LSP server install (LOG-*) |
| `use qs_log::SOURCE_FIELD_NAMES = crate::redact::SOURCE_FIELD_NAMES` | none | kept public: the logging policy the app and the LSP server install (LOG-*) |
| `use qs_log::SourceText = crate::redact::SourceText` | none | kept public: the logging policy the app and the LSP server install (LOG-*) |
| `use qs_log::install_panic_hook = crate::panic::install_panic_hook` | none | kept public: the logging policy the app and the LSP server install (LOG-*) |

## `qs-map` (L0)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qs_map::check::MAP_MAX_SOUNDS` | none | kept public: the .map format's model (MAP-*) |
| `const qs_map::geometry::MAP_BASE_AXES` | none | kept public: the .map format's model (MAP-*) |
| `const qs_map::geometry::MAP_ON_EPSILON` | none | kept public: the .map format's model (MAP-*) |
| `const qs_map::lexer::MAP_MAX_TOKEN` | none | kept public: the .map format's model (MAP-*) |
| `const qs_map::qbsp::MAP_LUMP_LINE` | none | kept public: the .map format's model (MAP-*) |
| `const qs_map::qbsp::MAP_MAX_BRUSHES` | none | kept public: the .map format's model (MAP-*) |
| `const qs_map::qbsp::MAP_MAX_ENTITIES` | none | kept public: the .map format's model (MAP-*) |
| `const qs_map::qbsp::MAP_MAX_ENTSTRING` | none | kept public: the .map format's model (MAP-*) |
| `const qs_map::qbsp::MAP_MAX_KEY` | none | kept public: the .map format's model (MAP-*) |
| `const qs_map::qbsp::MAP_MAX_TEXINFO` | none | kept public: the .map format's model (MAP-*) |
| `const qs_map::qbsp::MAP_MAX_TEXTURE_NAME` | none | kept public: the .map format's model (MAP-*) |
| `const qs_map::qbsp::MAP_MAX_VALUE` | none | kept public: the .map format's model (MAP-*) |
| `const qs_map::qbsp::MAP_TEX_SPECIAL` | none | kept public: the .map format's model (MAP-*) |
| `enum qs_map::check::DiagnosticKind` | none | kept public: the .map format's model (MAP-*) |
| `enum qs_map::document::Compiler` | `qc-preproc`, `qc-codegen`, `qc-sema`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `xtask`, `fuzz` |
| `enum qs_map::document::Item` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qc-syntax`, `qs-differential`, `qs-install`, `qc-progs`, `qc-engines`, `qs-commands`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qc-syntax`, `qs-differential`, `qs-install`, `qc-progs`, `qc-engines`, `qs-commands`, `xtask` |
| `enum qs_map::document::MapError` | none | kept public: the .map format's model (MAP-*) |
| `enum qs_map::document::TexDef` | `xtask` | kept public: used by `xtask` |
| `enum qs_map::lexer::Gap` | `qc-preproc` | kept public: used by `qc-preproc` |
| `enum qs_map::lexer::LexError` | none | kept public: the .map format's model (MAP-*) |
| `enum qs_map::lexer::Lexed` | `qc-preproc`, `qc-syntax`, `qc-lexer` | kept public: used by `qc-preproc`, `qc-syntax`, `qc-lexer` |
| `enum qs_map::lexer::TokenLimit` | none | kept public: the .map format's model (MAP-*) |
| `enum qs_map::qbsp::QbspError` | none | kept public: the .map format's model (MAP-*) |
| `enum qs_map::qbsp::QbspWarning` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::check::MapFile::check` | `qs-mdl`, `xtask`, `fuzz` | kept public: used by `qs-mdl`, `xtask`, `fuzz` |
| `fn qs_map::cnum::c_atof` | `qs-differential`, `qs-sim` | kept public: used by `qs-differential`, `qs-sim` |
| `fn qs_map::cnum::c_atoi` | `qc-lexer`, `qs-sim` | kept public: used by `qc-lexer`, `qs-sim` |
| `fn qs_map::cnum::c_strtod` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::cnum::scan_vector` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::document::Brush::line` | `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-lexer`, `qs-editor-view`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-lexer`, `qs-editor-view`, `desktop`, `lsp`, `xtask` |
| `fn qs_map::document::Brush::new` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::document::Entity::brushes` | `xtask`, `fuzz` | kept public: used by `xtask`, `fuzz` |
| `fn qs_map::document::Entity::classname` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::document::Entity::has_brushes` | `qs-differential` | kept public: used by `qs-differential` |
| `fn qs_map::document::Entity::line` | `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-lexer`, `qs-editor-view`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-lexer`, `qs-editor-view`, `desktop`, `lsp`, `xtask` |
| `fn qs_map::document::Entity::new` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::document::Entity::origin` | `qc-preproc`, `qc-sema`, `fuzz` | kept public: used by `qc-preproc`, `qc-sema`, `fuzz` |
| `fn qs_map::document::Entity::pairs` | `qs-differential`, `qs-install` | kept public: used by `qs-differential`, `qs-install` |
| `fn qs_map::document::Entity::remove` | `qc-ide`, `qc-preproc`, `qc-trace`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-differential`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-trace`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-differential`, `lsp`, `xtask` |
| `fn qs_map::document::Entity::set` | `qc-preproc`, `qc-sema`, `qs-vfs`, `qs-workbench`, `qs-differential`, `qc-builtins`, `qs-tests`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qs-vfs`, `qs-workbench`, `qs-differential`, `qc-builtins`, `qs-tests`, `xtask` |
| `fn qs_map::document::Entity::value` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-differential`, `qs-render`, `qs-editor-view`, `qc-project` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-differential`, `qs-render`, `qs-editor-view`, `qc-project` |
| `fn qs_map::document::Face::line` | `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-lexer`, `qs-editor-view`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-lexer`, `qs-editor-view`, `desktop`, `lsp`, `xtask` |
| `fn qs_map::document::Face::new` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::document::Face::plane` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_map::document::Face::points` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::document::Face::qbsp_differs` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::document::Face::set` | `qc-preproc`, `qc-sema`, `qs-vfs`, `qs-workbench`, `qs-differential`, `qc-builtins`, `qs-tests`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qs-vfs`, `qs-workbench`, `qs-differential`, `qc-builtins`, `qs-tests`, `xtask` |
| `fn qs_map::document::Face::texdef` | `xtask` | kept public: used by `xtask` |
| `fn qs_map::document::Face::texture` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::document::Face::texvecs` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_map::document::MapFile::brush_models` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_map::document::MapFile::new` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::document::MapFile::parse` | `qc-ide`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qs_map::document::MapFile::push_entity` | `qs-sim` | kept public: used by `qs-sim` |
| `fn qs_map::document::MapFile::to_bytes` | `qs-vfs`, `qs-tests`, `xtask`, `fuzz` | kept public: used by `qs-vfs`, `qs-tests`, `xtask`, `fuzz` |
| `fn qs_map::document::MapFile::wad_paths` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_map::document::MapFile::world` | `qs-sim` | kept public: used by `qs-sim` |
| `fn qs_map::document::Pair::ericw_escape` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::document::Pair::key` | `qc-preproc`, `qs-settings`, `qs-differential`, `qs-editor-view`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qs-settings`, `qs-differential`, `qs-editor-view`, `desktop`, `xtask` |
| `fn qs_map::document::Pair::key_raw` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::document::Pair::line` | `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-lexer`, `qs-editor-view`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-lexer`, `qs-editor-view`, `desktop`, `lsp`, `xtask` |
| `fn qs_map::document::Pair::new` | `qs-differential` | kept public: used by `qs-differential` |
| `fn qs_map::document::Pair::set_value` | `qs-editor-view` | kept public: used by `qs-editor-view` |
| `fn qs_map::document::Pair::value` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-differential`, `qs-render`, `qs-editor-view`, `qc-project` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-differential`, `qs-render`, `qs-editor-view`, `qc-project` |
| `fn qs_map::document::Pair::value_raw` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::document::is_tool_key` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::geometry::MapFloat::rotation` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::geometry::MapFloat::to_f64` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::geometry::on_plane` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::geometry::plane` | `qs-bsp`, `qs-render`, `qs-sim`, `fuzz` | kept public: used by `qs-bsp`, `qs-render`, `qs-sim`, `fuzz` |
| `fn qs_map::geometry::qbsp_plane` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::geometry::texvecs` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_map::geometry::valve_texvecs` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::lexer::Lexer::new` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::lexer::Lexer::next_token` | `qs-sim` | kept public: used by `qs-sim` |
| `fn qs_map::lexer::Lexer::offset` | `qc-ide`, `qs-workbench`, `qs-editor-view`, `desktop`, `lsp` | kept public: used by `qc-ide`, `qs-workbench`, `qs-editor-view`, `desktop`, `lsp` |
| `fn qs_map::lexer::Lexer::same_line_token` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::lexer::Lexer::skip` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qc-syntax`, `qs-bsp`, `qc-diag`, `qs-differential`, `qs-render`, `qs-install`, `qs-editor-view`, `qs-tests`, `qs-sim`, `qc-project`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qc-syntax`, `qs-bsp`, `qc-diag`, `qs-differential`, `qs-render`, `qs-install`, `qs-editor-view`, `qs-tests`, `qs-sim`, `qc-project`, `desktop`, `xtask` |
| `fn qs_map::lexer::Lexer::text` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qs_map::lexer::Lexer::token` | `qc-codegen`, `qc-sema`, `qc-syntax`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax`, `xtask` |
| `fn qs_map::lexer::Token::is` | `xtask` | kept public: used by `xtask` |
| `fn qs_map::lexer::is_map_space` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::lexer::map_text` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::qbsp::QbspEntity::set_key_value` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::qbsp::QbspEntity::value_for_key` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::qbsp::QbspMap::entity_lump` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_map::qbsp::QbspMap::number_models` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_map::qbsp::QbspMap::texture_slots` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_map::qbsp::QbspMap::wad_path` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::qbsp::is_special` | none | kept public: the .map format's model (MAP-*) |
| `fn qs_map::qbsp::load_map_file` | `xtask`, `fuzz` | kept public: used by `xtask`, `fuzz` |
| `mod qs_map::cnum` | `qc-engines` | kept public: used by `qc-engines` |
| `mod qs_map::geometry` | `qs-differential` | kept public: used by `qs-differential` |
| `mod qs_map::lexer` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qc-project`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qc-project`, `xtask`, `fuzz` |
| `mod qs_map::qbsp` | `qs-bsp`, `qs-differential`, `xtask`, `fuzz` | kept public: used by `qs-bsp`, `qs-differential`, `xtask`, `fuzz` |
| `struct qs_map::check::Diagnostic` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-tests`, `qc-project`, `lsp` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-tests`, `qc-project`, `lsp` |
| `struct qs_map::document::Brush` | `qs-differential`, `qs-sim` | kept public: used by `qs-differential`, `qs-sim` |
| `struct qs_map::document::BrushModel` | `qs-sim` | kept public: used by `qs-sim` |
| `struct qs_map::document::Entity` | `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qc-syntax`, `qc-edict`, `qs-differential`, `qc-builtins`, `qs-sim`, `xtask` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qc-syntax`, `qc-edict`, `qs-differential`, `qc-builtins`, `qs-sim`, `xtask` |
| `struct qs_map::document::Face` | `qs-bsp`, `qs-render` | kept public: used by `qs-bsp`, `qs-render` |
| `struct qs_map::document::MapFile` | `qs-vfs`, `qs-differential`, `qs-tests`, `xtask`, `fuzz` | kept public: used by `qs-vfs`, `qs-differential`, `qs-tests`, `xtask`, `fuzz` |
| `struct qs_map::document::Pair` | `qs-differential` | kept public: used by `qs-differential` |
| `struct qs_map::geometry::Plane` | `qs-bsp`, `qs-render`, `qs-sim` | kept public: used by `qs-bsp`, `qs-render`, `qs-sim` |
| `struct qs_map::lexer::Lexer` | `qc-lexer` | kept public: used by `qc-lexer` |
| `struct qs_map::lexer::LineBreak` | none | kept public: the .map format's model (MAP-*) |
| `struct qs_map::lexer::Token` | `qc-preproc`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-install`, `qs-sim`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-install`, `qs-sim`, `desktop`, `xtask` |
| `struct qs_map::qbsp::QbspEntity` | none | kept public: the .map format's model (MAP-*) |
| `struct qs_map::qbsp::QbspFace` | none | kept public: the .map format's model (MAP-*) |
| `struct qs_map::qbsp::QbspMap` | none | kept public: the .map format's model (MAP-*) |
| `struct qs_map::qbsp::QbspTexinfo` | none | kept public: the .map format's model (MAP-*) |
| `struct qs_map::qbsp::TextureSlot` | none | kept public: the .map format's model (MAP-*) |
| `trait qs_map::geometry::MapFloat` | none | kept public: the .map format's model (MAP-*) |
| `trait qs_map::geometry::sealed::Sealed` | `qc-progs` | kept public: used by `qc-progs` |
| `type qs_map::geometry::TexVecs` | none | kept public: the .map format's model (MAP-*) |
| `use qs_map::Brush = document::Brush` | `qs-differential`, `qs-sim` | kept public: used by `qs-differential`, `qs-sim` |
| `use qs_map::BrushModel = document::BrushModel` | `qs-sim` | kept public: used by `qs-sim` |
| `use qs_map::Compiler = document::Compiler` | `qc-preproc`, `qc-codegen`, `qc-sema`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `xtask`, `fuzz` |
| `use qs_map::Diagnostic = check::Diagnostic` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-tests`, `qc-project`, `lsp` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-tests`, `qc-project`, `lsp` |
| `use qs_map::DiagnosticKind = check::DiagnosticKind` | none | kept public: the .map format's model (MAP-*) |
| `use qs_map::Entity = document::Entity` | `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qc-syntax`, `qc-edict`, `qs-differential`, `qc-builtins`, `qs-sim`, `xtask` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qc-syntax`, `qc-edict`, `qs-differential`, `qc-builtins`, `qs-sim`, `xtask` |
| `use qs_map::Face = document::Face` | `qs-bsp`, `qs-render` | kept public: used by `qs-bsp`, `qs-render` |
| `use qs_map::Item = document::Item` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qc-syntax`, `qs-differential`, `qs-install`, `qc-progs`, `qc-engines`, `qs-commands`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qs-mdl`, `qs-vfs`, `qc-syntax`, `qs-differential`, `qs-install`, `qc-progs`, `qc-engines`, `qs-commands`, `xtask` |
| `use qs_map::MAP_MAX_SOUNDS = check::MAP_MAX_SOUNDS` | none | kept public: the .map format's model (MAP-*) |
| `use qs_map::MapError = document::MapError` | none | kept public: the .map format's model (MAP-*) |
| `use qs_map::MapFile = document::MapFile` | `qs-vfs`, `qs-differential`, `qs-tests`, `xtask`, `fuzz` | kept public: used by `qs-vfs`, `qs-differential`, `qs-tests`, `xtask`, `fuzz` |
| `use qs_map::Pair = document::Pair` | `qs-differential` | kept public: used by `qs-differential` |
| `use qs_map::TexDef = document::TexDef` | `xtask` | kept public: used by `xtask` |
| `use qs_map::is_tool_key = document::is_tool_key` | none | kept public: the .map format's model (MAP-*) |

## `qs-mdl` (L0)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qs_mdl::md5::MD5_ANIM_FLAGS` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::md5::MD5_FRAME_INTERVAL` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::md5::MD5_MAGIC` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::md5::MD5_MAX_SKIN_FRAMES` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::md5::MD5_NO_PARENT` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::md5::MD5_VERSION` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_ALIAS_ONSEAM` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_DT_FACES_FRONT` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_EF_GIB` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_EF_GRENADE` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_EF_ROCKET` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_EF_ROTATE` | `qs-render` | kept public: used by `qs-render` |
| `const qs_mdl::mdl::MDL_EF_TRACER` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_EF_TRACER2` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_EF_TRACER3` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_EF_ZOMGIB` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_FRAME_GROUP` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_FRAME_HEADER_SIZE` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_FRAME_SINGLE` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_GL_MAX_POSES` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_GL_MAX_SKINS` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_GL_MAX_TRIS` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_GL_MAX_VERTS` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_GROUP_HEADER_SIZE` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_HEADER_SIZE` | `qs-render` | kept public: used by `qs-render` |
| `const qs_mdl::mdl::MDL_MAGIC` | `qs-render` | kept public: used by `qs-render` |
| `const qs_mdl::mdl::MDL_MAX_SKIN_HEIGHT` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_MAX_VERTS` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_NUMVERTEXNORMALS` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_SIZE_RATIO` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_SKIN_GROUP` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_SKIN_SINGLE` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_STVERT_SIZE` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_ST_RAND` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_ST_SYNC` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_TRIANGLE_SIZE` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::mdl::MDL_VERSION` | `qs-render` | kept public: used by `qs-render` |
| `const qs_mdl::spr::SPR_FACING_UPRIGHT` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::spr::SPR_FRAME_HEADER_SIZE` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::spr::SPR_GROUP` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::spr::SPR_HEADER_SIZE` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::spr::SPR_MAGIC` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::spr::SPR_ORIENTED` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::spr::SPR_SINGLE` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::spr::SPR_ST_RAND` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::spr::SPR_ST_SYNC` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::spr::SPR_VERSION` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::spr::SPR_VP_PARALLEL` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::spr::SPR_VP_PARALLEL_ORIENTED` | none | kept public: the MDL format's model (MDL-*) |
| `const qs_mdl::spr::SPR_VP_PARALLEL_UPRIGHT` | none | kept public: the MDL format's model (MDL-*) |
| `enum qs_mdl::md5::Md5Error` | none | kept public: the MDL format's model (MDL-*) |
| `enum qs_mdl::mdl::GlLimit` | none | kept public: the MDL format's model (MDL-*) |
| `enum qs_mdl::mdl::MdlError` | none | kept public: the MDL format's model (MDL-*) |
| `enum qs_mdl::mdl::MdlFrameEntry` | `qs-render`, `fuzz` | kept public: used by `qs-render`, `fuzz` |
| `enum qs_mdl::mdl::MdlSkin` | none | kept public: the MDL format's model (MDL-*) |
| `enum qs_mdl::spr::SprError` | none | kept public: the MDL format's model (MDL-*) |
| `enum qs_mdl::spr::SpriteFrameEntry` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_mdl::md5::Md5Anim::check_mesh` | `qs-vfs`, `fuzz` | kept public: used by `qs-vfs`, `fuzz` |
| `fn qs_mdl::md5::Md5Anim::frame` | `qc-codegen`, `qc-syntax`, `qs-differential`, `qs-render`, `qs-editor-view`, `qs-sim`, `desktop`, `fuzz` | kept public: used by `qc-codegen`, `qc-syntax`, `qs-differential`, `qs-render`, `qs-editor-view`, `qs-sim`, `desktop`, `fuzz` |
| `fn qs_mdl::md5::Md5Anim::num_animated_components` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_mdl::md5::Md5Anim::pose` | `qs-render`, `fuzz` | kept public: used by `qs-render`, `fuzz` |
| `fn qs_mdl::md5::Md5Anim::read` | `qc-sema`, `qs-vfs`, `qs-install`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `qs-vfs`, `qs-install`, `xtask`, `fuzz` |
| `fn qs_mdl::md5::Md5Mesh::read` | `qc-sema`, `qs-vfs`, `qs-install`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `qs-vfs`, `qs-install`, `xtask`, `fuzz` |
| `fn qs_mdl::md5::Md5Surface::tri` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_mdl::md5::Md5Surface::vert` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_mdl::md5::Md5Surface::weight` | none | kept public: the MDL format's model (MDL-*) |
| `fn qs_mdl::md5::Tokens::new` | none | kept public: the MDL format's model (MDL-*) |
| `fn qs_mdl::md5::md5_anim_path` | `qs-vfs` | kept public: used by `qs-vfs` |
| `fn qs_mdl::md5::md5_quat_w` | none | kept public: the MDL format's model (MDL-*) |
| `fn qs_mdl::md5::md5_skin_path` | none | kept public: the MDL format's model (MDL-*) |
| `fn qs_mdl::mdl::Mdl::bad_normals` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_mdl::mdl::Mdl::bounds` | `qc-sema`, `qs-sim`, `fuzz` | kept public: used by `qc-sema`, `qs-sim`, `fuzz` |
| `fn qs_mdl::mdl::Mdl::gl_limits` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_mdl::mdl::Mdl::position` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-lexer`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qs-tests`, `qs-sim`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-lexer`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qs-tests`, `qs-sim`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qs_mdl::mdl::Mdl::read` | `qc-sema`, `qs-vfs`, `qs-render`, `qs-install`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `qs-vfs`, `qs-render`, `qs-install`, `xtask`, `fuzz` |
| `fn qs_mdl::mdl::Mdl::size` | `qs-vfs`, `qs-render`, `qs-editor-view`, `qs-sim`, `desktop`, `fuzz` | kept public: used by `qs-vfs`, `qs-render`, `qs-editor-view`, `qs-sim`, `desktop`, `fuzz` |
| `fn qs_mdl::mdl::Mdl::skin_s` | `qs-render`, `fuzz` | kept public: used by `qs-render`, `fuzz` |
| `fn qs_mdl::mdl::MdlError::message` | `qc-preproc`, `qs-settings`, `qs-vfs`, `qs-bsp`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm` | kept public: used by `qc-preproc`, `qs-settings`, `qs-vfs`, `qs-bsp`, `qs-differential`, `qs-render`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm` |
| `fn qs_mdl::mdl::MdlFrame::vertex` | `qs-render` | kept public: used by `qs-render` |
| `fn qs_mdl::mdl::MdlFrame::vertex_count` | `qs-render`, `fuzz` | kept public: used by `qs-render`, `fuzz` |
| `fn qs_mdl::mdl::MdlFrameEntry::bbox` | none | kept public: the MDL format's model (MDL-*) |
| `fn qs_mdl::mdl::MdlFrameEntry::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-vfs`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qs_mdl::spr::Sprite::bounds` | `qc-sema`, `qs-sim`, `fuzz` | kept public: used by `qc-sema`, `qs-sim`, `fuzz` |
| `fn qs_mdl::spr::Sprite::read` | `qc-sema`, `qs-vfs`, `qs-install`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `qs-vfs`, `qs-install`, `qs-sim`, `xtask`, `fuzz` |
| `fn qs_mdl::spr::SpriteFrame::down` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_mdl::spr::SpriteFrame::left` | `qs-editor-view`, `fuzz` | kept public: used by `qs-editor-view`, `fuzz` |
| `fn qs_mdl::spr::SpriteFrame::right` | `qs-editor-view`, `fuzz` | kept public: used by `qs-editor-view`, `fuzz` |
| `fn qs_mdl::spr::SpriteFrame::up` | `fuzz` | kept public: used by `fuzz` |
| `mod qs_mdl::md5` | `qs-vfs`, `qs-install`, `fuzz` | kept public: used by `qs-vfs`, `qs-install`, `fuzz` |
| `mod qs_mdl::mdl` | `qc-codegen`, `qc-sema`, `qs-vfs`, `qc-syntax`, `qs-render`, `qc-builtins`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qc-codegen`, `qc-sema`, `qs-vfs`, `qc-syntax`, `qs-render`, `qc-builtins`, `qs-sim`, `xtask`, `fuzz` |
| `mod qs_mdl::spr` | `qs-vfs`, `qs-sim`, `xtask`, `fuzz` | kept public: used by `qs-vfs`, `qs-sim`, `xtask`, `fuzz` |
| `struct qs_mdl::md5::Md5Anim` | `qs-vfs`, `fuzz` | kept public: used by `qs-vfs`, `fuzz` |
| `struct qs_mdl::md5::Md5HierarchyEntry` | none | kept public: the MDL format's model (MDL-*) |
| `struct qs_mdl::md5::Md5Joint` | none | kept public: the MDL format's model (MDL-*) |
| `struct qs_mdl::md5::Md5Mesh` | `qs-vfs`, `fuzz` | kept public: used by `qs-vfs`, `fuzz` |
| `struct qs_mdl::md5::Md5Pose` | none | kept public: the MDL format's model (MDL-*) |
| `struct qs_mdl::md5::Md5Surface` | none | kept public: the MDL format's model (MDL-*) |
| `struct qs_mdl::md5::Md5Vert` | none | kept public: the MDL format's model (MDL-*) |
| `struct qs_mdl::md5::Md5Weight` | none | kept public: the MDL format's model (MDL-*) |
| `struct qs_mdl::md5::Tokens` | `qc-preproc`, `qc-syntax`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-syntax`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `struct qs_mdl::mdl::Mdl` | `qs-vfs`, `qs-render`, `fuzz` | kept public: used by `qs-vfs`, `qs-render`, `fuzz` |
| `struct qs_mdl::mdl::MdlFrame` | none | kept public: the MDL format's model (MDL-*) |
| `struct qs_mdl::mdl::MdlHeader` | none | kept public: the MDL format's model (MDL-*) |
| `struct qs_mdl::mdl::StVert` | none | kept public: the MDL format's model (MDL-*) |
| `struct qs_mdl::mdl::TriVertX` | none | kept public: the MDL format's model (MDL-*) |
| `struct qs_mdl::mdl::Triangle` | none | kept public: the MDL format's model (MDL-*) |
| `struct qs_mdl::spr::Sprite` | `qs-vfs`, `qs-sim`, `fuzz` | kept public: used by `qs-vfs`, `qs-sim`, `fuzz` |
| `struct qs_mdl::spr::SpriteFrame` | `fuzz` | kept public: used by `fuzz` |
| `struct qs_mdl::spr::SpriteGroup` | none | kept public: the MDL format's model (MDL-*) |
| `struct qs_mdl::spr::SpriteHeader` | none | kept public: the MDL format's model (MDL-*) |
| `use qs_mdl::Md5Anim = md5::Md5Anim` | `qs-vfs`, `fuzz` | kept public: used by `qs-vfs`, `fuzz` |
| `use qs_mdl::Md5Error = md5::Md5Error` | none | kept public: the MDL format's model (MDL-*) |
| `use qs_mdl::Md5Mesh = md5::Md5Mesh` | `qs-vfs`, `fuzz` | kept public: used by `qs-vfs`, `fuzz` |
| `use qs_mdl::Mdl = mdl::Mdl` | `qs-vfs`, `qs-render`, `fuzz` | kept public: used by `qs-vfs`, `qs-render`, `fuzz` |
| `use qs_mdl::MdlError = mdl::MdlError` | none | kept public: the MDL format's model (MDL-*) |
| `use qs_mdl::SprError = spr::SprError` | none | kept public: the MDL format's model (MDL-*) |
| `use qs_mdl::Sprite = spr::Sprite` | `qs-vfs`, `qs-sim`, `fuzz` | kept public: used by `qs-vfs`, `qs-sim`, `fuzz` |

## `qs-pak` (L0)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qs_pak::PAK_CRC_INIT` | none | kept public: the PAK format's model (PAK-*) |
| `const qs_pak::PAK_CRC_POLY` | none | kept public: the PAK format's model (PAK-*) |
| `const qs_pak::PAK_ENTRY_SIZE` | none | kept public: the PAK format's model (PAK-*) |
| `const qs_pak::PAK_HEADER_SIZE` | none | kept public: the PAK format's model (PAK-*) |
| `const qs_pak::PAK_MAGIC` | none | kept public: the PAK format's model (PAK-*) |
| `const qs_pak::PAK_MAX_FILES` | none | kept public: the PAK format's model (PAK-*) |
| `const qs_pak::PAK_NAME_SIZE` | none | kept public: the PAK format's model (PAK-*) |
| `const qs_pak::PAK_PAK0_COUNT` | `qs-vfs` | kept public: used by `qs-vfs` |
| `const qs_pak::PAK_PAK0_CRC` | `qs-vfs` | kept public: used by `qs-vfs` |
| `enum qs_pak::PakError` | `qs-vfs`, `qs-install` | kept public: used by `qs-vfs`, `qs-install` |
| `fn qs_pak::Entry::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qs_pak::Entry::range` | `qc-ide`, `qc-preproc`, `qc-sema`, `qc-syntax`, `qc-diag`, `qc-lexer`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qc-syntax`, `qc-diag`, `qc-lexer`, `lsp`, `xtask`, `fuzz` |
| `fn qs_pak::Header::directory_range` | none | kept public: the PAK format's model (PAK-*) |
| `fn qs_pak::Header::entry_count` | none | kept public: the PAK format's model (PAK-*) |
| `fn qs_pak::Header::parse` | `qc-ide`, `qc-sema`, `qs-settings`, `qs-mdl`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-sema`, `qs-settings`, `qs-mdl`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask` |
| `fn qs_pak::Pak::directory_crc` | `qs-vfs` | kept public: used by `qs-vfs` |
| `fn qs_pak::Pak::entries` | `qs-vfs`, `qs-install`, `xtask`, `fuzz` | kept public: used by `qs-vfs`, `qs-install`, `xtask`, `fuzz` |
| `fn qs_pak::Pak::entry_data` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_pak::Pak::file_len` | `xtask` | kept public: used by `xtask` |
| `fn qs_pak::Pak::find` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qs_pak::Pak::header` | `qc-codegen`, `qc-trace`, `qc-edict`, `qs-differential`, `qc-progs`, `xtask` | kept public: used by `qc-codegen`, `qc-trace`, `qc-edict`, `qs-differential`, `qc-progs`, `xtask` |
| `fn qs_pak::Pak::is_modified` | `qs-vfs`, `qs-install` | kept public: used by `qs-vfs`, `qs-install` |
| `fn qs_pak::Pak::read` | `qc-sema`, `qs-vfs`, `qs-install`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `qs-vfs`, `qs-install`, `xtask`, `fuzz` |
| `fn qs_pak::Pak::read_entry` | `qs-vfs`, `xtask`, `fuzz` | kept public: used by `qs-vfs`, `xtask`, `fuzz` |
| `fn qs_pak::Pak::read_from` | `qs-vfs`, `qs-install`, `xtask`, `fuzz` | kept public: used by `qs-vfs`, `qs-install`, `xtask`, `fuzz` |
| `struct qs_pak::Entry` | `qs-settings`, `qs-vfs`, `qc-builtins`, `qc-progs`, `qs-commands`, `xtask` | kept public: used by `qs-settings`, `qs-vfs`, `qc-builtins`, `qc-progs`, `qs-commands`, `xtask` |
| `struct qs_pak::Header` | `qc-trace`, `qc-lno`, `qs-differential`, `qc-progs`, `qc-vm`, `lsp`, `xtask` | kept public: used by `qc-trace`, `qc-lno`, `qs-differential`, `qc-progs`, `qc-vm`, `lsp`, `xtask` |
| `struct qs_pak::Pak` | `qs-vfs`, `qs-install`, `xtask`, `fuzz` | kept public: used by `qs-vfs`, `qs-install`, `xtask`, `fuzz` |

## `qs-paths` (L0)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qs_paths::APP_DIR_NAME` | `qs-tests` | kept public: used by `qs-tests` |
| `const qs_paths::APP_DIR_NAME_UNIX` | `qs-tests` | kept public: used by `qs-tests` |
| `enum qs_paths::PathsError` | none | kept public: the app directories every binary resolves (PATHS-*) |
| `enum qs_paths::Platform` | `qs-log`, `qs-install`, `qs-tests`, `lsp` | kept public: used by `qs-log`, `qs-install`, `qs-tests`, `lsp` |
| `fn qs_paths::AppDirs::cache` | `qs-editor-view`, `qs-tests` | kept public: used by `qs-editor-view`, `qs-tests` |
| `fn qs_paths::AppDirs::config` | `qs-settings`, `qs-log`, `qs-tests` | kept public: used by `qs-settings`, `qs-log`, `qs-tests` |
| `fn qs_paths::AppDirs::data` | `qc-ide`, `qs-wad`, `qs-layout`, `qs-tests`, `desktop`, `fuzz` | kept public: used by `qc-ide`, `qs-wad`, `qs-layout`, `qs-tests`, `desktop`, `fuzz` |
| `fn qs_paths::AppDirs::logs` | `qs-log`, `qs-tests`, `lsp` | kept public: used by `qs-log`, `qs-tests`, `lsp` |
| `fn qs_paths::Platform::current` | `qc-codegen`, `qc-syntax`, `qs-install`, `qs-tests` | kept public: used by `qc-codegen`, `qc-syntax`, `qs-install`, `qs-tests` |
| `fn qs_paths::current` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-mdl`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-editor-view`, `qs-tests`, `qs-sim`, `qc-vm`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-mdl`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-diag`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-editor-view`, `qs-tests`, `qs-sim`, `qc-vm`, `desktop`, `lsp`, `xtask` |
| `fn qs_paths::fonts::current_font_dirs` | `desktop` | kept public: used by `desktop` |
| `fn qs_paths::resolve` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-log`, `qs-differential`, `qs-install`, `qc-builtins`, `qc-progs`, `qs-tests`, `qs-commands`, `qc-project`, `qc-vm`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-log`, `qs-differential`, `qs-install`, `qc-builtins`, `qc-progs`, `qs-tests`, `qs-commands`, `qc-project`, `qc-vm`, `lsp`, `xtask` |
| `fn qs_paths::stores::StoreDirs::gog_libraries` | `qs-install` | kept public: used by `qs-install` |
| `fn qs_paths::stores::StoreDirs::heroic` | `qs-install` | kept public: used by `qs-install` |
| `fn qs_paths::stores::StoreDirs::steam` | `qs-install` | kept public: used by `qs-install` |
| `fn qs_paths::stores::current_store_dirs` | `qs-install` | kept public: used by `qs-install` |
| `fn qs_paths::stores::store_dirs` | `qs-install` | kept public: used by `qs-install` |
| `struct qs_paths::AppDirs` | `qs-settings`, `qs-log`, `qs-layout`, `lsp` | kept public: used by `qs-settings`, `qs-log`, `qs-layout`, `lsp` |
| `struct qs_paths::stores::StoreDirs` | `qs-install` | kept public: used by `qs-install` |
| `use qs_paths::StoreDirs = stores::StoreDirs` | `qs-install` | kept public: used by `qs-install` |
| `use qs_paths::current_font_dirs = fonts::current_font_dirs` | `desktop` | kept public: used by `desktop` |
| `use qs_paths::current_store_dirs = stores::current_store_dirs` | `qs-install` | kept public: used by `qs-install` |
| `use qs_paths::store_dirs = stores::store_dirs` | `qs-install` | kept public: used by `qs-install` |

Items that left the root (2): `fn qs_paths::fonts::font_dirs`, `use qs_paths::font_dirs = fonts::font_dirs`. Each is `pub(crate)` now, under `#[cfg(test)]` where only the crate's tests read it; `qc_sema::nav::ModuleIndex::references_at` is removed.

## `qs-porting-example` (L1)

| Item | Used by | Disposition |
| --- | --- | --- |
| `fn qs_porting_example::ed_new_string` | `qs-sim` | kept public: used by `qs-sim` |

## `qs-settings` (L5)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qs_settings::Setting::ALL` | `qc-ide`, `qc-preproc`, `qc-types`, `qc-sema`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-types`, `qc-sema`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-layout`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `enum qs_settings::Scope` | `qc-sema`, `qc-syntax`, `fuzz` | kept public: used by `qc-sema`, `qc-syntax`, `fuzz` |
| `enum qs_settings::Setting` | `qc-sema` | kept public: used by `qc-sema` |
| `enum qs_settings::Source` | `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-log`, `qs-install`, `qc-builtins`, `qc-engines`, `qc-project`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-log`, `qs-install`, `qc-builtins`, `qc-engines`, `qc-project`, `desktop`, `xtask` |
| `enum qs_settings::Theme` | `desktop` | kept public: used by `desktop` |
| `enum qs_settings::error::SettingsError` | none | kept public: named in a public signature a dependent uses |
| `fn qs_settings::Setting::default_value` | `qc-codegen`, `qc-sema`, `qc-syntax` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax` |
| `fn qs_settings::Setting::key` | `qc-preproc`, `qs-differential`, `qs-map`, `qs-editor-view`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qs-differential`, `qs-map`, `qs-editor-view`, `desktop`, `xtask` |
| `fn qs_settings::Setting::summary` | `qs-commands`, `desktop` | kept public: used by `qs-commands`, `desktop` |
| `fn qs_settings::SettingsFile::get` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lno`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-layout`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qs_settings::SettingsFile::load` | `qc-ide`, `qc-preproc`, `qs-vfs`, `qs-render`, `qc-project`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-vfs`, `qs-render`, `qc-project`, `xtask`, `fuzz` |
| `fn qs_settings::SettingsFile::put` | none | kept public: named in a public signature a dependent uses |
| `fn qs_settings::SettingsStore::file` | `qc-preproc`, `qc-sema`, `qs-vfs`, `qc-syntax`, `qs-install`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-sema`, `qs-vfs`, `qc-syntax`, `qs-install`, `xtask`, `fuzz` |
| `fn qs_settings::SettingsStore::open` | `qc-ide`, `qc-preproc`, `qs-vfs`, `qs-workbench`, `qs-log`, `qs-differential`, `qs-install`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-vfs`, `qs-workbench`, `qs-log`, `qs-differential`, `qs-install`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qs_settings::SettingsStore::put` | none | kept public: named in a public signature a dependent uses |
| `fn qs_settings::SettingsStore::value` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-differential`, `qs-render`, `qs-map`, `qs-editor-view`, `qc-project` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-differential`, `qs-render`, `qs-map`, `qs-editor-view`, `qc-project` |
| `fn qs_settings::commands::HasSettings::settings` | none | kept public: named in a public signature a dependent uses |
| `fn qs_settings::commands::register` | `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-layout`, `qs-commands`, `qc-project`, `desktop`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-layout`, `qs-commands`, `qc-project`, `desktop`, `xtask`, `fuzz` |
| `fn qs_settings::reference` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-pak`, `qc-syntax`, `qc-lexer`, `qs-render`, `qc-builtins`, `qc-progs`, `qc-engines`, `qs-tests`, `qs-commands`, `desktop`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-pak`, `qc-syntax`, `qc-lexer`, `qs-render`, `qc-builtins`, `qc-progs`, `qc-engines`, `qs-tests`, `qs-commands`, `desktop`, `xtask`, `fuzz` |
| `struct qs_settings::Appearance` | none | kept public: named in a public signature a dependent uses |
| `struct qs_settings::Log` | `qs-paths` | kept public: used by `qs-paths` |
| `struct qs_settings::SettingsFile` | `fuzz` | kept public: used by `fuzz` |
| `struct qs_settings::SettingsStore` | `xtask`, `fuzz` | kept public: used by `xtask`, `fuzz` |
| `struct qs_settings::commands::Entry` | `qs-vfs`, `qs-pak`, `qc-builtins`, `qc-progs`, `qs-commands`, `xtask` | kept public: used by `qs-vfs`, `qs-pak`, `qc-builtins`, `qc-progs`, `qs-commands`, `xtask` |
| `trait qs_settings::commands::HasSettings` | `xtask` | kept public: used by `xtask` |
| `use qs_settings::Entry = commands::Entry` | `qs-vfs`, `qs-pak`, `qc-builtins`, `qc-progs`, `qs-commands`, `xtask` | kept public: used by `qs-vfs`, `qs-pak`, `qc-builtins`, `qc-progs`, `qs-commands`, `xtask` |
| `use qs_settings::HasSettings = commands::HasSettings` | `xtask` | kept public: used by `xtask` |
| `use qs_settings::SettingsError = error::SettingsError` | none | kept public: named in a public signature a dependent uses |
| `use qs_settings::register = commands::register` | `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-layout`, `qs-commands`, `qc-project`, `desktop`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qs-layout`, `qs-commands`, `qc-project`, `desktop`, `xtask`, `fuzz` |

Items that left the root (5): `const qs_settings::DEFAULT_LOG_FILTER`, `fn qs_settings::SettingsStore::log_filter`, `fn qs_settings::SettingsStore::theme`, `fn qs_settings::project_settings_path`, `fn qs_settings::user_settings_path`. Each is `pub(crate)` now, under `#[cfg(test)]` where only the crate's tests read it; `qc_sema::nav::ModuleIndex::references_at` is removed.

## `qs-sim` (L4)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qs_sim::collision::area::AREA_DEPTH` | none | kept public: the engine model the preview host runs (SIM-* |
| `const qs_sim::collision::area::FL_ITEM` | `qs-differential`, `qc-builtins`, `xtask` | kept public: used by `qs-differential`, `qc-builtins`, `xtask` |
| `const qs_sim::collision::area::FL_MONSTER` | none | kept public: the engine model the preview host runs (SIM-* |
| `const qs_sim::collision::area::MAX_ENT_LEAFS` | `qc-progs` | kept public: used by `qc-progs` |
| `const qs_sim::collision::hull::CONTENTS_CURRENT_0` | none | kept public: the engine model the preview host runs (SIM-* |
| `const qs_sim::collision::hull::CONTENTS_CURRENT_DOWN` | none | kept public: the engine model the preview host runs (SIM-* |
| `const qs_sim::collision::hull::CONTENTS_EMPTY` | `qs-bsp`, `qc-builtins`, `xtask` | kept public: used by `qs-bsp`, `qc-builtins`, `xtask` |
| `const qs_sim::collision::hull::CONTENTS_SOLID` | `qs-bsp` | kept public: used by `qs-bsp` |
| `const qs_sim::collision::hull::CONTENTS_WATER` | `qs-bsp` | kept public: used by `qs-bsp` |
| `const qs_sim::collision::hull::HULL1` | none | kept public: the engine model the preview host runs (SIM-* |
| `const qs_sim::collision::hull::HULL2` | none | kept public: the engine model the preview host runs (SIM-* |
| `const qs_sim::collision::trace::DIST_EPSILON` | `qs-render` | kept public: used by `qs-render` |
| `const qs_sim::spawn::EDICT_SPAWNFLAG_NOT_DEATHMATCH` | none | kept public: the engine model the preview host runs (SIM-* |
| `const qs_sim::spawn::EDICT_SPAWNFLAG_NOT_EASY` | none | kept public: the engine model the preview host runs (SIM-* |
| `const qs_sim::spawn::EDICT_SPAWNFLAG_NOT_HARD` | none | kept public: the engine model the preview host runs (SIM-* |
| `const qs_sim::spawn::EDICT_SPAWNFLAG_NOT_MEDIUM` | none | kept public: the engine model the preview host runs (SIM-* |
| `const qs_sim::spawn::SPAWNFLAGS_SLOT` | `qs-differential` | kept public: used by `qs-differential` |
| `enum qs_sim::collision::CollisionError` | none | kept public: the engine model the preview host runs (SIM-* |
| `enum qs_sim::collision::Solid` | `qc-edict`, `qs-differential`, `qc-builtins` | kept public: used by `qc-edict`, `qs-differential`, `qc-builtins` |
| `enum qs_sim::collision::area::MoveKind` | none | kept public: the engine model the preview host runs (SIM-* |
| `enum qs_sim::collision::hull::EntityHull` | none | kept public: the engine model the preview host runs (SIM-* |
| `enum qs_sim::spawn::LoadError` | `qs-differential` | kept public: used by `qs-differential` |
| `enum qs_sim::spawn::SpawnError` | `qs-differential` | kept public: used by `qs-differential` |
| `enum qs_sim::spawn::Token` | `qc-preproc`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-install`, `qs-map`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-install`, `qs-map`, `desktop`, `xtask` |
| `enum qs_sim::world::WorldError` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::Collision::add_brush` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::Collision::area` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::Collision::leafs_of` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::Collision::model` | `qs-differential` | kept public: used by `qs-differential` |
| `fn qs_sim::collision::Collision::new` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::Collision::point_contents` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::Collision::sets` | `qc-sema`, `qc-syntax` | kept public: used by `qc-sema`, `qc-syntax` |
| `fn qs_sim::collision::Collision::true_point_contents` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::Collision::unlink` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::Host::body` | `qc-codegen`, `qc-sema`, `qc-syntax`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax`, `xtask` |
| `fn qs_sim::collision::Host::collision` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::Host::collision_mut` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::Host::set_abs` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::Host::touch` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::Solid::from_float` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::area::Area::insert_at` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::area::Area::link_of` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::area::Area::listed` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::area::Area::new` | `desktop` | kept public: used by `desktop` |
| `fn qs_sim::collision::area::Area::split` | `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-vfs`, `qc-syntax`, `qc-diag`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-porting-example`, `qs-map`, `qc-progs`, `qs-editor-view`, `qs-paths`, `qs-tests`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qs-vfs`, `qc-syntax`, `qc-diag`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-porting-example`, `qs-map`, `qc-progs`, `qs-editor-view`, `qs-paths`, `qs-tests`, `qs-commands`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qs_sim::collision::area::Area::unlink` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::area::LeafTree::new` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::area::LeafTree::touched` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::area::MoveKind::from_int` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::area::box_on_plane_side` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::area::link_edict` | `qs-differential`, `qc-builtins` | kept public: used by `qs-differential`, `qc-builtins` |
| `fn qs_sim::collision::area::sv_move` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::area::test_entity_position` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::hull::BoxHull::hull` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::hull::BoxHull::new` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::hull::EntityHull::hull` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::hull::Hull::node` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::hull::Hull::over` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::hull::Hull::point_contents` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::hull::Hull::size` | `qs-mdl`, `qs-vfs`, `qs-render`, `qs-editor-view`, `desktop`, `fuzz` | kept public: used by `qs-mdl`, `qs-vfs`, `qs-render`, `qs-editor-view`, `desktop`, `fuzz` |
| `fn qs_sim::collision::hull::HullSet::hull` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::hull::HullSet::new` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::hull::Plane::distance` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::hull::hull_for_entity` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::trace::Trace::new` | `qc-vm` | kept public: used by `qc-vm` |
| `fn qs_sim::collision::trace::clip_move_to_entity` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::collision::trace::recursive_hull_check` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::spawn::Parser::new` | `qs-mdl`, `qc-syntax` | kept public: used by `qs-mdl`, `qc-syntax` |
| `fn qs_sim::spawn::Parser::next_token` | `qs-map` | kept public: used by `qs-map` |
| `fn qs_sim::spawn::SpawnHost::allocate` | `qc-codegen` | kept public: used by `qc-codegen` |
| `fn qs_sim::spawn::SpawnHost::edict_value` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::spawn::SpawnHost::find_field` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::spawn::SpawnHost::find_function` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::spawn::SpawnHost::free` | `qc-sema`, `qc-edict`, `qs-differential`, `qc-builtins` | kept public: used by `qc-sema`, `qc-edict`, `qs-differential`, `qc-builtins` |
| `fn qs_sim::spawn::SpawnHost::global` | `qc-codegen`, `qs-differential`, `qc-builtins`, `qs-tests`, `qc-vm` | kept public: used by `qc-codegen`, `qs-differential`, `qc-builtins`, `qs-tests`, `qc-vm` |
| `fn qs_sim::spawn::SpawnHost::make_string` | `qs-differential` | kept public: used by `qs-differential` |
| `fn qs_sim::spawn::SpawnHost::set_field` | `qc-edict`, `qs-differential`, `qc-builtins`, `qc-vm` | kept public: used by `qc-edict`, `qs-differential`, `qc-builtins`, `qc-vm` |
| `fn qs_sim::spawn::SpawnHost::spawn` | `qc-preproc`, `qc-sema`, `qc-syntax`, `qs-differential`, `qs-tests`, `qs-commands`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qc-syntax`, `qs-differential`, `qs-tests`, `qs-commands`, `xtask` |
| `fn qs_sim::spawn::SpawnHost::spawnflags` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::spawn::c_atof` | `qs-differential`, `qs-map` | kept public: used by `qs-differential`, `qs-map` |
| `fn qs_sim::spawn::c_atoi` | `qc-lexer`, `qs-map` | kept public: used by `qc-lexer`, `qs-map` |
| `fn qs_sim::spawn::inhibited` | `qs-differential`, `xtask` | kept public: used by `qs-differential`, `xtask` |
| `fn qs_sim::spawn::load_from_file` | `qs-differential` | kept public: used by `qs-differential` |
| `fn qs_sim::world::World::connect` | `qs-render` | kept public: used by `qs-render` |
| `fn qs_sim::world::World::entities` | `qs-vfs`, `qs-bsp`, `qs-render`, `qs-tests`, `fuzz` | kept public: used by `qs-vfs`, `qs-bsp`, `qs-render`, `qs-tests`, `fuzz` |
| `fn qs_sim::world::World::frame` | `qc-codegen`, `qs-mdl`, `qc-syntax`, `qs-differential`, `qs-render`, `qs-editor-view`, `desktop`, `fuzz` | kept public: used by `qc-codegen`, `qs-mdl`, `qc-syntax`, `qs-differential`, `qs-render`, `qs-editor-view`, `desktop`, `fuzz` |
| `fn qs_sim::world::World::light_style` | `qs-render` | kept public: used by `qs-render` |
| `fn qs_sim::world::World::load` | `qc-ide`, `qc-preproc`, `qs-vfs`, `qs-render`, `qc-project`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qs-vfs`, `qs-render`, `qc-project`, `xtask` |
| `fn qs_sim::world::World::player_view` | `qs-render` | kept public: used by `qs-render` |
| `fn qs_sim::world::World::set_command` | `qs-render` | kept public: used by `qs-render` |
| `fn qs_sim::world::World::take_set_angle` | `qs-render` | kept public: used by `qs-render` |
| `fn qs_sim::world::World::take_sounds` | none | kept public: the engine model the preview host runs (SIM-* |
| `fn qs_sim::world::World::time` | `qs-render` | kept public: used by `qs-render` |
| `mod qs_sim::collision` | `qc-builtins` | kept public: used by `qc-builtins` |
| `mod qs_sim::spawn` | `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-edict`, `qs-differential`, `qc-builtins`, `qc-progs`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-vm`, `xtask` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-edict`, `qs-differential`, `qc-builtins`, `qc-progs`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-vm`, `xtask` |
| `struct qs_sim::builtins::SimBuiltins` | none | kept public: the engine model the preview host runs (SIM-* |
| `struct qs_sim::client::UserCmd` | `qs-render` | kept public: used by `qs-render` |
| `struct qs_sim::collision::Body` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `desktop`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `desktop`, `xtask` |
| `struct qs_sim::collision::Collision` | none | kept public: the engine model the preview host runs (SIM-* |
| `struct qs_sim::collision::area::Area` | `desktop` | kept public: used by `desktop` |
| `struct qs_sim::collision::area::LeafTree` | none | kept public: the engine model the preview host runs (SIM-* |
| `struct qs_sim::collision::hull::BoxHull` | none | kept public: the engine model the preview host runs (SIM-* |
| `struct qs_sim::collision::hull::BrushModel` | `qs-map` | kept public: used by `qs-map` |
| `struct qs_sim::collision::hull::ClipNode` | `qs-bsp` | kept public: used by `qs-bsp` |
| `struct qs_sim::collision::hull::Hull` | `qs-bsp` | kept public: used by `qs-bsp` |
| `struct qs_sim::collision::hull::HullSet` | none | kept public: the engine model the preview host runs (SIM-* |
| `struct qs_sim::collision::hull::Plane` | `qs-bsp`, `qs-render`, `qs-map` | kept public: used by `qs-bsp`, `qs-render`, `qs-map` |
| `struct qs_sim::collision::trace::Trace` | `qc-trace`, `qs-differential`, `qc-vm`, `xtask` | kept public: used by `qc-trace`, `qs-differential`, `qc-vm`, `xtask` |
| `struct qs_sim::collision::trace::TracePlane` | none | kept public: the engine model the preview host runs (SIM-* |
| `struct qs_sim::physics::Sound` | `qs-wav`, `qc-sema`, `qc-builtins`, `xtask` | kept public: used by `qs-wav`, `qc-sema`, `qc-builtins`, `xtask` |
| `struct qs_sim::spawn::FieldDef` | `qs-differential` | kept public: used by `qs-differential` |
| `struct qs_sim::spawn::Parser` | `qc-preproc`, `qs-mdl`, `qc-syntax`, `qs-map` | kept public: used by `qc-preproc`, `qs-mdl`, `qc-syntax`, `qs-map` |
| `struct qs_sim::world::Entity` | `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qc-syntax`, `qc-edict`, `qs-differential`, `qc-builtins`, `qs-map`, `xtask` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qc-syntax`, `qc-edict`, `qs-differential`, `qc-builtins`, `qs-map`, `xtask` |
| `struct qs_sim::world::World` | `qc-edict`, `qs-differential`, `qs-render`, `qc-builtins` | kept public: used by `qc-edict`, `qs-differential`, `qs-render`, `qc-builtins` |
| `trait qs_sim::collision::Host` | `qs-differential`, `qc-builtins`, `qs-tests`, `qc-vm`, `xtask` | kept public: used by `qs-differential`, `qc-builtins`, `qs-tests`, `qc-vm`, `xtask` |
| `trait qs_sim::spawn::SpawnHost` | `qs-differential` | kept public: used by `qs-differential` |
| `type qs_sim::builtins::ModelFiles` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::Entity = world::Entity` | `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qc-syntax`, `qc-edict`, `qs-differential`, `qc-builtins`, `qs-map`, `xtask` | kept public: used by `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qc-syntax`, `qc-edict`, `qs-differential`, `qc-builtins`, `qs-map`, `xtask` |
| `use qs_sim::ModelFiles = builtins::ModelFiles` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::SimBuiltins = builtins::SimBuiltins` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::Sound = physics::Sound` | `qs-wav`, `qc-sema`, `qc-builtins`, `xtask` | kept public: used by `qs-wav`, `qc-sema`, `qc-builtins`, `xtask` |
| `use qs_sim::UserCmd = client::UserCmd` | `qs-render` | kept public: used by `qs-render` |
| `use qs_sim::World = world::World` | `qc-edict`, `qs-differential`, `qs-render`, `qc-builtins` | kept public: used by `qc-edict`, `qs-differential`, `qs-render`, `qc-builtins` |
| `use qs_sim::WorldError = world::WorldError` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::AREA_DEPTH = area::AREA_DEPTH` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::Area = area::Area` | `desktop` | kept public: used by `desktop` |
| `use qs_sim::collision::BoxHull = hull::BoxHull` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::BrushModel = hull::BrushModel` | `qs-map` | kept public: used by `qs-map` |
| `use qs_sim::collision::CONTENTS_CURRENT_0 = hull::CONTENTS_CURRENT_0` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::CONTENTS_CURRENT_DOWN = hull::CONTENTS_CURRENT_DOWN` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::CONTENTS_EMPTY = hull::CONTENTS_EMPTY` | `qs-bsp`, `qc-builtins`, `xtask` | kept public: used by `qs-bsp`, `qc-builtins`, `xtask` |
| `use qs_sim::collision::CONTENTS_SOLID = hull::CONTENTS_SOLID` | `qs-bsp` | kept public: used by `qs-bsp` |
| `use qs_sim::collision::CONTENTS_WATER = hull::CONTENTS_WATER` | `qs-bsp` | kept public: used by `qs-bsp` |
| `use qs_sim::collision::ClipNode = hull::ClipNode` | `qs-bsp` | kept public: used by `qs-bsp` |
| `use qs_sim::collision::DIST_EPSILON = trace::DIST_EPSILON` | `qs-render` | kept public: used by `qs-render` |
| `use qs_sim::collision::EntityHull = hull::EntityHull` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::FL_ITEM = area::FL_ITEM` | `qs-differential`, `qc-builtins`, `xtask` | kept public: used by `qs-differential`, `qc-builtins`, `xtask` |
| `use qs_sim::collision::FL_MONSTER = area::FL_MONSTER` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::HULL1 = hull::HULL1` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::HULL2 = hull::HULL2` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::Hull = hull::Hull` | `qs-bsp` | kept public: used by `qs-bsp` |
| `use qs_sim::collision::HullSet = hull::HullSet` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::LeafTree = area::LeafTree` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::MAX_ENT_LEAFS = area::MAX_ENT_LEAFS` | `qc-progs` | kept public: used by `qc-progs` |
| `use qs_sim::collision::MoveKind = area::MoveKind` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::Plane = hull::Plane` | `qs-bsp`, `qs-render`, `qs-map` | kept public: used by `qs-bsp`, `qs-render`, `qs-map` |
| `use qs_sim::collision::Trace = trace::Trace` | `qc-trace`, `qs-differential`, `qc-vm`, `xtask` | kept public: used by `qc-trace`, `qs-differential`, `qc-vm`, `xtask` |
| `use qs_sim::collision::TracePlane = trace::TracePlane` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::box_on_plane_side = area::box_on_plane_side` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::clip_move_to_entity = trace::clip_move_to_entity` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::hull_for_entity = hull::hull_for_entity` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::link_edict = area::link_edict` | `qs-differential`, `qc-builtins` | kept public: used by `qs-differential`, `qc-builtins` |
| `use qs_sim::collision::recursive_hull_check = trace::recursive_hull_check` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::sv_move = area::sv_move` | none | kept public: the engine model the preview host runs (SIM-* |
| `use qs_sim::collision::test_entity_position = area::test_entity_position` | none | kept public: the engine model the preview host runs (SIM-* |

## `qs-vfs` (L0)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qs_vfs::FS_GAMENAME` | `qs-install` | kept public: used by `qs-install` |
| `const qs_vfs::FS_POP_PATH` | none | kept public: the game filesystem the preview host and the app read (VFS-*) |
| `const qs_vfs::FS_POP_SIZE` | none | kept public: the game filesystem the preview host and the app read (VFS-*) |
| `const qs_vfs::FS_POP_TABLE` | none | kept public: the game filesystem the preview host and the app read (VFS-*) |
| `enum qs_vfs::Element` | `qc-codegen`, `qc-sema` | kept public: used by `qc-codegen`, `qc-sema` |
| `enum qs_vfs::Location` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-workbench`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-workbench`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `enum qs_vfs::VfsError` | none | kept public: the game filesystem the preview host and the app read (VFS-*) |
| `enum qs_vfs::host::EntryKind` | `qs-workbench`, `qs-install`, `desktop` | kept public: used by `qs-workbench`, `qs-install`, `desktop` |
| `fn qs_vfs::Archive::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qs_vfs::Archive::pak` | none | kept public: the game filesystem the preview host and the app read (VFS-*) |
| `fn qs_vfs::Archive::path` | `qc-ide`, `qs-settings`, `qs-workbench`, `qs-log`, `qs-differential`, `qs-install`, `qs-layout`, `qs-tests`, `desktop`, `xtask` | kept public: used by `qc-ide`, `qs-settings`, `qs-workbench`, `qs-log`, `qs-differential`, `qs-install`, `qs-layout`, `qs-tests`, `desktop`, `xtask` |
| `fn qs_vfs::Directory::host_path` | none | kept public: the game filesystem the preview host and the app read (VFS-*) |
| `fn qs_vfs::Directory::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qs_vfs::Element::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qs_vfs::FileRef::element` | `qc-codegen` | kept public: used by `qc-codegen` |
| `fn qs_vfs::FileRef::location` | `qc-ide`, `qs-workbench`, `qs-log`, `desktop`, `lsp` | kept public: used by `qc-ide`, `qs-workbench`, `qs-log`, `desktop`, `lsp` |
| `fn qs_vfs::FileRef::name` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-types`, `qc-sema`, `qs-mdl`, `qs-pak`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-lexer`, `qs-log`, `qs-differential`, `qc-builtins`, `qs-editor-view`, `qc-engines`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qs_vfs::LoadedFile::bytes` | `qc-ide`, `qc-preproc`, `qc-trace`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-install`, `qc-progs`, `qc-engines`, `qs-tests`, `qc-project`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-trace`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-install`, `qc-progs`, `qc-engines`, `qs-tests`, `qc-project`, `xtask` |
| `fn qs_vfs::LoadedFile::bytes_with_nul` | none | kept public: the game filesystem the preview host and the app read (VFS-*) |
| `fn qs_vfs::LoadedFile::into_bytes` | `qc-ide`, `qc-preproc`, `qc-sema`, `qs-mdl`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qs-mdl`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-tests`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask` |
| `fn qs_vfs::LoadedFile::size` | `qs-mdl`, `qs-render`, `qs-editor-view`, `qs-sim`, `desktop`, `fuzz` | kept public: used by `qs-mdl`, `qs-render`, `qs-editor-view`, `qs-sim`, `desktop`, `fuzz` |
| `fn qs_vfs::Vfs::elements` | `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-bsp` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax`, `qs-bsp` |
| `fn qs_vfs::Vfs::files` | `qc-ide`, `qc-preproc`, `qs-workbench` | kept public: used by `qc-ide`, `qc-preproc`, `qs-workbench` |
| `fn qs_vfs::Vfs::find` | `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-sema`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qs-wad`, `qc-codegen`, `qc-sema`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qs_vfs::Vfs::game_dir` | none | kept public: the game filesystem the preview host and the app read (VFS-*) |
| `fn qs_vfs::Vfs::host` | `xtask` | kept public: used by `xtask` |
| `fn qs_vfs::Vfs::is_modified` | `qs-pak`, `qs-install` | kept public: used by `qs-pak`, `qs-install` |
| `fn qs_vfs::Vfs::is_registered` | none | kept public: the game filesystem the preview host and the app read (VFS-*) |
| `fn qs_vfs::Vfs::load` | `qc-ide`, `qc-preproc`, `qs-render`, `qc-project`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qs-render`, `qc-project`, `xtask` |
| `fn qs_vfs::Vfs::load_wad` | none | kept public: the game filesystem the preview host and the app read (VFS-*) |
| `fn qs_vfs::Vfs::new` | `qs-render` | kept public: used by `qs-render` |
| `fn qs_vfs::Vfs::read` | `qc-sema`, `qs-install`, `xtask` | kept public: used by `qc-sema`, `qs-install`, `xtask` |
| `fn qs_vfs::host::HostFs::open` | `qc-ide`, `qc-preproc`, `qs-workbench`, `qs-log`, `qs-differential`, `qs-install`, `qc-project`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qs-workbench`, `qs-log`, `qs-differential`, `qs-install`, `qc-project`, `desktop`, `lsp`, `xtask` |
| `fn qs_vfs::host::HostFs::read_dir` | `qs-install` | kept public: used by `qs-install` |
| `fn qs_vfs::host::find_entry` | `qs-install` | kept public: used by `qs-install` |
| `struct qs_vfs::Archive` | `xtask` | kept public: used by `xtask` |
| `struct qs_vfs::Directory` | `qs-workbench`, `qs-install`, `qs-paths`, `desktop` | kept public: used by `qs-workbench`, `qs-install`, `qs-paths`, `desktop` |
| `struct qs_vfs::FileRef` | `desktop` | kept public: used by `desktop` |
| `struct qs_vfs::LoadedFile` | `qs-render` | kept public: used by `qs-render` |
| `struct qs_vfs::Vfs` | `qs-render` | kept public: used by `qs-render` |
| `struct qs_vfs::host::DirEntry` | `qs-install` | kept public: used by `qs-install` |
| `struct qs_vfs::host::StdFs` | `qs-render`, `qs-install` | kept public: used by `qs-render`, `qs-install` |
| `trait qs_vfs::host::HostFs` | `qs-install` | kept public: used by `qs-install` |
| `type qs_vfs::host::HostFs::File` | `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-install`, `qc-progs`, `qs-tests`, `qc-project`, `desktop`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qs-workbench`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-install`, `qc-progs`, `qs-tests`, `qc-project`, `desktop`, `xtask` |
| `use qs_vfs::DirEntry = host::DirEntry` | `qs-install` | kept public: used by `qs-install` |
| `use qs_vfs::EntryKind = host::EntryKind` | `qs-workbench`, `qs-install`, `desktop` | kept public: used by `qs-workbench`, `qs-install`, `desktop` |
| `use qs_vfs::HostFs = host::HostFs` | `qs-install` | kept public: used by `qs-install` |
| `use qs_vfs::StdFs = host::StdFs` | `qs-render`, `qs-install` | kept public: used by `qs-render`, `qs-install` |
| `use qs_vfs::find_entry = host::find_entry` | `qs-install` | kept public: used by `qs-install` |

## `qs-wad` (L0)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qs_wad::WAD_CMP_LZSS` | none | kept public: the WAD2 format's model (WAD-*) |
| `const qs_wad::WAD_CMP_NONE` | none | kept public: the WAD2 format's model (WAD-*) |
| `const qs_wad::WAD_CONCHARS_SIZE` | none | kept public: the WAD2 format's model (WAD-*) |
| `const qs_wad::WAD_CONCHARS_TRANSPARENT` | none | kept public: the WAD2 format's model (WAD-*) |
| `const qs_wad::WAD_ENTRY_SIZE` | none | kept public: the WAD2 format's model (WAD-*) |
| `const qs_wad::WAD_GFX_NAME` | `qs-vfs` | kept public: used by `qs-vfs` |
| `const qs_wad::WAD_HEADER_SIZE` | none | kept public: the WAD2 format's model (WAD-*) |
| `const qs_wad::WAD_MAGIC` | none | kept public: the WAD2 format's model (WAD-*) |
| `const qs_wad::WAD_MIPLEVELS` | `qs-bsp` | kept public: used by `qs-bsp` |
| `const qs_wad::WAD_MIPTEX_HEADER_SIZE` | `qs-bsp` | kept public: used by `qs-bsp` |
| `const qs_wad::WAD_NAME_SIZE` | none | kept public: the WAD2 format's model (WAD-*) |
| `const qs_wad::WAD_TYP_LABEL` | none | kept public: the WAD2 format's model (WAD-*) |
| `const qs_wad::WAD_TYP_MIPTEX` | none | kept public: the WAD2 format's model (WAD-*) |
| `const qs_wad::WAD_TYP_NONE` | none | kept public: the WAD2 format's model (WAD-*) |
| `const qs_wad::WAD_TYP_PALETTE` | none | kept public: the WAD2 format's model (WAD-*) |
| `const qs_wad::WAD_TYP_QPIC` | none | kept public: the WAD2 format's model (WAD-*) |
| `const qs_wad::WAD_TYP_QTEX` | none | kept public: the WAD2 format's model (WAD-*) |
| `const qs_wad::WAD_TYP_SOUND` | none | kept public: the WAD2 format's model (WAD-*) |
| `enum qs_wad::WadError` | `qs-vfs` | kept public: used by `qs-vfs` |
| `fn qs_wad::Conchars::glyph` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_wad::Conchars::parse` | `qc-ide`, `qc-sema`, `qs-settings`, `qs-mdl`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-sema`, `qs-settings`, `qs-mdl`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qs_wad::LumpName::as_bytes` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qs-wav`, `qc-trace`, `qc-sema`, `qs-mdl`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-bench`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qc-engines`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qs_wad::MipTexHeader::parse` | `qc-ide`, `qc-sema`, `qs-settings`, `qs-mdl`, `qc-syntax`, `qs-bsp`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-sema`, `qs-settings`, `qs-mdl`, `qc-syntax`, `qs-bsp`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qs_wad::QPic::parse` | `qc-ide`, `qc-sema`, `qs-settings`, `qs-mdl`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-sema`, `qs-settings`, `qs-mdl`, `qc-syntax`, `qc-lexer`, `qs-log`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-vm`, `lsp`, `xtask`, `fuzz` |
| `fn qs_wad::Wad::data` | `qc-ide`, `qs-paths`, `qs-tests`, `desktop`, `fuzz` | kept public: used by `qc-ide`, `qs-paths`, `qs-tests`, `desktop`, `fuzz` |
| `fn qs_wad::Wad::find` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-vfs`, `qs-pak`, `qs-workbench`, `qc-syntax`, `qs-bsp`, `qc-edict`, `qc-diag`, `qc-lexer`, `qs-differential`, `qs-render`, `qs-install`, `qc-builtins`, `qs-map`, `qc-progs`, `qs-editor-view`, `qc-engines`, `qs-paths`, `qs-tests`, `qs-commands`, `qs-sim`, `qc-project`, `qc-vm`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qs_wad::Wad::lump` | `qs-bsp`, `fuzz` | kept public: used by `qs-bsp`, `fuzz` |
| `fn qs_wad::Wad::lumps` | `qs-vfs`, `qs-bsp`, `fuzz` | kept public: used by `qs-vfs`, `qs-bsp`, `fuzz` |
| `fn qs_wad::Wad::read` | `qc-sema`, `qs-vfs`, `qs-install`, `xtask`, `fuzz` | kept public: used by `qc-sema`, `qs-vfs`, `qs-install`, `xtask`, `fuzz` |
| `fn qs_wad::w_cleanup_name` | none | kept public: the WAD2 format's model (WAD-*) |
| `struct qs_wad::Conchars` | `fuzz` | kept public: used by `fuzz` |
| `struct qs_wad::Lump` | `qs-bsp`, `qs-render`, `qs-sim` | kept public: used by `qs-bsp`, `qs-render`, `qs-sim` |
| `struct qs_wad::LumpName` | none | kept public: the WAD2 format's model (WAD-*) |
| `struct qs_wad::MipTexHeader` | `qs-bsp`, `fuzz` | kept public: used by `qs-bsp`, `fuzz` |
| `struct qs_wad::QPic` | `fuzz` | kept public: used by `fuzz` |
| `struct qs_wad::Wad` | `qs-vfs`, `fuzz` | kept public: used by `qs-vfs`, `fuzz` |

## `qs-wav` (L0)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qs_wav::WAV_CHUNK_HEADER_SIZE` | none | kept public: the WAV reader (WAV-*) |
| `const qs_wav::WAV_CUE_LOOPSTART_OFFSET` | none | kept public: the WAV reader (WAV-*) |
| `const qs_wav::WAV_FORMAT_PCM` | none | kept public: the WAV reader (WAV-*) |
| `const qs_wav::WAV_LIST_LENGTH_OFFSET` | none | kept public: the WAV reader (WAV-*) |
| `const qs_wav::WAV_LIST_MARK_OFFSET` | none | kept public: the WAV reader (WAV-*) |
| `const qs_wav::WAV_NO_LOOP` | none | kept public: the WAV reader (WAV-*) |
| `const qs_wav::WAV_SOUND_DIR` | none | kept public: the WAV reader (WAV-*) |
| `enum qs_wav::WavError` | none | kept public: the WAV reader (WAV-*) |
| `fn qs_wav::Sound::sample` | `qs-bsp`, `qs-render`, `fuzz` | kept public: used by `qs-bsp`, `qs-render`, `fuzz` |
| `fn qs_wav::get_wavinfo` | `fuzz` | kept public: used by `fuzz` |
| `fn qs_wav::s_load_sound` | `qs-vfs`, `fuzz` | kept public: used by `qs-vfs`, `fuzz` |
| `fn qs_wav::s_load_sound_path` | none | kept public: the WAV reader (WAV-*) |
| `fn qs_wav::sound_name` | `qs-vfs` | kept public: used by `qs-vfs` |
| `struct qs_wav::Sound` | `qc-sema`, `qc-builtins`, `qs-sim`, `xtask` | kept public: used by `qc-sema`, `qc-builtins`, `qs-sim`, `xtask` |
| `struct qs_wav::WavInfo` | none | kept public: the WAV reader (WAV-*) |

## `qs-workbench` (L5)

| Item | Used by | Disposition |
| --- | --- | --- |
| `const qs_workbench::commands::COMPLETE` | `desktop` | kept public: used by `desktop` |
| `const qs_workbench::commands::DEFINITION` | `desktop` | kept public: used by `desktop` |
| `const qs_workbench::commands::HOVER` | `desktop` | kept public: used by `desktop` |
| `const qs_workbench::commands::OPEN_FILE` | `desktop` | kept public: used by `desktop` |
| `const qs_workbench::commands::OPEN_PROJECT` | `desktop` | kept public: used by `desktop` |
| `const qs_workbench::commands::REFERENCES` | `qc-sema`, `desktop` | kept public: used by `qc-sema`, `desktop` |
| `const qs_workbench::commands::RENAME` | `desktop` | kept public: used by `desktop` |
| `const qs_workbench::commands::SAVE_FILE` | `desktop` | kept public: used by `desktop` |
| `const qs_workbench::commands::UPDATE_FILE` | `desktop` | kept public: used by `desktop` |
| `enum qs_workbench::EntryKind` | `qs-vfs`, `qs-install`, `desktop` | kept public: used by `qs-vfs`, `qs-install`, `desktop` |
| `enum qs_workbench::WorkbenchError` | none | kept public: named in a public signature a dependent uses |
| `enum qs_workbench::commands::CandidateKind` | none | kept public: named in a public signature a dependent uses |
| `fn qs_workbench::Location::column` | `desktop` | kept public: used by `desktop` |
| `fn qs_workbench::Location::line` | `qc-preproc`, `qc-sema`, `qc-lexer`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `qc-sema`, `qc-lexer`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask` |
| `fn qs_workbench::Location::of` | `qs-sim`, `xtask` | kept public: used by `qs-sim`, `xtask` |
| `fn qs_workbench::OpenFile::classes` | `desktop` | kept public: used by `desktop` |
| `fn qs_workbench::OpenFile::generation` | `desktop` | kept public: used by `desktop` |
| `fn qs_workbench::OpenFile::highlights` | `qc-ide`, `desktop`, `lsp` | kept public: used by `qc-ide`, `desktop`, `lsp` |
| `fn qs_workbench::OpenFile::location` | `qc-ide`, `qs-vfs`, `qs-log`, `desktop`, `lsp` | kept public: used by `qc-ide`, `qs-vfs`, `qs-log`, `desktop`, `lsp` |
| `fn qs_workbench::OpenFile::offset` | `qc-ide`, `qs-map`, `qs-editor-view`, `desktop`, `lsp` | kept public: used by `qc-ide`, `qs-map`, `qs-editor-view`, `desktop`, `lsp` |
| `fn qs_workbench::OpenFile::outline` | `desktop` | kept public: used by `desktop` |
| `fn qs_workbench::OpenFile::path` | `qc-ide`, `qs-settings`, `qs-vfs`, `qs-log`, `qs-differential`, `qs-install`, `qs-layout`, `qs-tests`, `desktop`, `xtask` | kept public: used by `qc-ide`, `qs-settings`, `qs-vfs`, `qs-log`, `qs-differential`, `qs-install`, `qs-layout`, `qs-tests`, `desktop`, `xtask` |
| `fn qs_workbench::OpenFile::revision` | `qc-preproc`, `qs-editor-view`, `desktop` | kept public: used by `qc-preproc`, `qs-editor-view`, `desktop` |
| `fn qs_workbench::OpenFile::text` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qs_workbench::OpenFile::version` | `qc-progs`, `desktop` | kept public: used by `qc-progs`, `desktop` |
| `fn qs_workbench::Workbench::new` | `desktop` | kept public: used by `desktop` |
| `fn qs_workbench::Workbench::open` | `qc-ide`, `qc-preproc`, `qs-vfs`, `qs-log`, `qs-differential`, `qs-install`, `qc-project`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qs-vfs`, `qs-log`, `qs-differential`, `qs-install`, `qc-project`, `desktop`, `lsp`, `xtask` |
| `fn qs_workbench::Workbench::open_file` | `desktop` | kept public: used by `desktop` |
| `fn qs_workbench::Workbench::open_project` | `desktop` | kept public: used by `desktop` |
| `fn qs_workbench::Workbench::problems` | `desktop` | kept public: used by `desktop` |
| `fn qs_workbench::Workbench::project_symbols` | `desktop` | kept public: used by `desktop` |
| `fn qs_workbench::Workbench::root` | `desktop` | kept public: used by `desktop` |
| `fn qs_workbench::Workbench::set_config` | `lsp` | kept public: used by `lsp` |
| `fn qs_workbench::Workbench::tree` | `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-engines`, `desktop`, `xtask` | kept public: used by `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-engines`, `desktop`, `xtask` |
| `fn qs_workbench::commands::HasWorkbench::workbench` | `desktop` | kept public: used by `desktop` |
| `fn qs_workbench::commands::register` | `qc-preproc`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-layout`, `qs-commands`, `qc-project`, `desktop`, `xtask`, `fuzz` | kept public: used by `qc-preproc`, `qc-sema`, `qs-settings`, `qc-syntax`, `qs-layout`, `qs-commands`, `qc-project`, `desktop`, `xtask`, `fuzz` |
| `fn qs_workbench::editing::Workbench::completions` | `qc-ide`, `lsp` | kept public: used by `qc-ide`, `lsp` |
| `fn qs_workbench::editing::Workbench::definition` | `qc-ide`, `qc-sema`, `lsp` | kept public: used by `qc-ide`, `qc-sema`, `lsp` |
| `fn qs_workbench::editing::Workbench::hover` | `qc-ide`, `lsp` | kept public: used by `qc-ide`, `lsp` |
| `fn qs_workbench::editing::Workbench::references` | `qc-ide`, `qc-sema`, `lsp` | kept public: used by `qc-ide`, `qc-sema`, `lsp` |
| `fn qs_workbench::editing::Workbench::rename` | `qc-ide`, `qc-preproc`, `lsp` | kept public: used by `qc-ide`, `qc-preproc`, `lsp` |
| `fn qs_workbench::editing::Workbench::save` | `desktop` | kept public: used by `desktop` |
| `fn qs_workbench::editing::Workbench::text` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-trace`, `qc-sema`, `qc-syntax`, `qs-differential`, `qc-builtins`, `qs-map`, `qs-editor-view`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `fn qs_workbench::editing::Workbench::update` | `qs-render`, `qs-install`, `desktop` | kept public: used by `qs-render`, `qs-install`, `desktop` |
| `struct qs_workbench::Location` | `qc-ide`, `qc-codegen`, `qc-sema`, `qs-vfs`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qs-vfs`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `struct qs_workbench::OpenFile` | `desktop` | kept public: used by `desktop` |
| `struct qs_workbench::Problem` | `desktop` | kept public: used by `desktop` |
| `struct qs_workbench::TreeEntry` | `desktop` | kept public: used by `desktop` |
| `struct qs_workbench::Workbench` | `desktop`, `xtask` | kept public: used by `desktop`, `xtask` |
| `struct qs_workbench::commands::AtParams` | `desktop` | kept public: used by `desktop` |
| `struct qs_workbench::commands::Candidate` | `desktop` | kept public: used by `desktop` |
| `struct qs_workbench::commands::HoverInfo` | `desktop` | kept public: used by `desktop` |
| `struct qs_workbench::commands::OpenFileParams` | `desktop` | kept public: used by `desktop` |
| `struct qs_workbench::commands::OpenProjectParams` | `desktop` | kept public: used by `desktop` |
| `struct qs_workbench::commands::Place` | `qc-preproc`, `qc-syntax`, `desktop` | kept public: used by `qc-preproc`, `qc-syntax`, `desktop` |
| `struct qs_workbench::commands::RenameParams` | `desktop`, `lsp` | kept public: used by `desktop`, `lsp` |
| `struct qs_workbench::commands::SaveFileParams` | `desktop` | kept public: used by `desktop` |
| `struct qs_workbench::commands::UpdateFileParams` | `desktop` | kept public: used by `desktop` |
| `struct qs_workbench::editing::Change` | `qs-settings`, `qs-commands` | kept public: used by `qs-settings`, `qs-commands` |
| `struct qs_workbench::editing::Renamed` | `xtask` | kept public: used by `xtask` |
| `trait qs_workbench::commands::HasWorkbench` | `desktop`, `xtask` | kept public: used by `desktop`, `xtask` |
| `type qs_workbench::commands::HasWorkbench::Loader` | `qc-ide`, `qc-preproc`, `qc-sema`, `qc-project`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-sema`, `qc-project`, `desktop`, `lsp`, `xtask` |
| `use qs_workbench::* = crate::commands::*` | none | kept public: named in a public signature a dependent uses |
| `use qs_workbench::Caution = qc_ide::Caution` | `qc-ide` | kept public: used by `qc-ide` |
| `use qs_workbench::Change = crate::editing::Change` | `qs-settings`, `qs-commands` | kept public: used by `qs-settings`, `qs-commands` |
| `use qs_workbench::Code = qc_ide::Code` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-layout`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qs-settings`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-layout`, `qs-editor-view`, `qc-engines`, `qs-tests`, `qs-commands`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `use qs_workbench::Completion = qc_ide::Completion` | `qc-ide`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `desktop`, `lsp`, `xtask` |
| `use qs_workbench::Config = qc_ide::Config` | `qc-ide`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `desktop`, `lsp`, `xtask` |
| `use qs_workbench::DefKind = qc_ide::DefKind` | `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-codegen`, `qc-sema`, `qc-engines`, `lsp`, `xtask` |
| `use qs_workbench::Diagnostic = qc_ide::Diagnostic` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-map`, `qs-tests`, `qc-project`, `lsp` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qs-map`, `qs-tests`, `qc-project`, `lsp` |
| `use qs_workbench::Dialect = qc_project::Dialect` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-differential`, `qc-engines`, `qs-tests`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-lexer`, `qs-differential`, `qc-engines`, `qs-tests`, `qc-project`, `desktop`, `lsp`, `xtask`, `fuzz` |
| `use qs_workbench::FileEdit = qc_ide::FileEdit` | `qc-ide`, `lsp` | kept public: used by `qc-ide`, `lsp` |
| `use qs_workbench::FileRange = qc_ide::FileRange` | `qc-ide`, `lsp` | kept public: used by `qc-ide`, `lsp` |
| `use qs_workbench::FsLoader = qc_project::FsLoader` | `qc-preproc`, `qc-project`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-preproc`, `qc-project`, `desktop`, `lsp`, `xtask` |
| `use qs_workbench::Highlight = qc_ide::Highlight` | `qc-ide`, `desktop` | kept public: used by `qc-ide`, `desktop` |
| `use qs_workbench::HighlightKind = qc_ide::HighlightKind` | `qc-ide`, `desktop`, `lsp` | kept public: used by `qc-ide`, `desktop`, `lsp` |
| `use qs_workbench::Hover = qc_ide::Hover` | `qc-ide`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `desktop`, `lsp`, `xtask` |
| `use qs_workbench::Modifier = qc_ide::Modifier` | `qc-ide`, `desktop`, `lsp` | kept public: used by `qc-ide`, `desktop`, `lsp` |
| `use qs_workbench::Modifiers = qc_ide::Modifiers` | `qc-ide`, `qs-editor-view`, `desktop` | kept public: used by `qc-ide`, `qs-editor-view`, `desktop` |
| `use qs_workbench::RenameError = qc_ide::RenameError` | `qc-ide`, `lsp` | kept public: used by `qc-ide`, `lsp` |
| `use qs_workbench::Renamed = crate::editing::Renamed` | `xtask` | kept public: used by `xtask` |
| `use qs_workbench::Severity = qc_ide::Severity` | `qc-ide`, `qc-codegen`, `qc-diag`, `qc-lexer`, `qc-engines`, `desktop`, `lsp` | kept public: used by `qc-ide`, `qc-codegen`, `qc-diag`, `qc-lexer`, `qc-engines`, `desktop`, `lsp` |
| `use qs_workbench::Span = qc_ide::Span` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qc-engines`, `lsp` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-syntax`, `qc-diag`, `qc-lexer`, `qc-engines`, `lsp` |
| `use qs_workbench::Symbol = qc_ide::Symbol` | `qc-ide`, `desktop` | kept public: used by `qc-ide`, `desktop` |
| `use qs_workbench::SymbolKind = qc_ide::SymbolKind` | `qc-ide`, `desktop` | kept public: used by `qc-ide`, `desktop` |
| `use qs_workbench::SyntaxClass = qc_ide::SyntaxClass` | `qc-ide`, `desktop` | kept public: used by `qc-ide`, `desktop` |
| `use qs_workbench::Target = qc_project::Target` | `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-edict`, `qc-lexer`, `qc-engines`, `qc-project`, `desktop`, `lsp`, `xtask` | kept public: used by `qc-ide`, `qc-preproc`, `qc-codegen`, `qc-sema`, `qc-edict`, `qc-lexer`, `qc-engines`, `qc-project`, `desktop`, `lsp`, `xtask` |

Items that left the root (3): `struct qs_workbench::commands::FileOpened`, `struct qs_workbench::commands::ProjectOpened`, `struct qs_workbench::commands::RenameDone`. Each is `pub(crate)` now, under `#[cfg(test)]` where only the crate's tests read it; `qc_sema::nav::ModuleIndex::references_at` is removed.

