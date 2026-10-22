// SPDX-License-Identifier: GPL-3.0-or-later

//! Rename: every declaration and use of a definition, in every module that compiles its file,
//! given a new name, and checked to mean what it meant before.
//!
//! A rename edits each place the definition's name is written: its declarations (QC-11.5) and
//! uses (QC-12.5), in every module that compiles its file. A vector's `_x`, `_y` and `_z`
//! components are names of their own (QC-11.7), so renaming `v` to `w` renames uses of `v_x`
//! to `w_x` too. A place whose text is not the name, the use of a macro that expanded to it
//! (QC-9.7), cannot be edited without changing the macro, so the rename is refused there.
//!
//! A name the engine binds is refused: a system global or field, whose place before
//! `end_sys_globals` or `end_sys_fields` the header CRC checks (EDICT-2.2, EDICT-3.2), and a
//! field the engine reads by name (EDICT-3.4). A function or field that map data may name, as an
//! entity's `classname` (EDICT-8.6) or a key (EDICT-8.2), is renamed with a [`Caution`], since
//! no source in the project says which maps name it.
//!
//! The new name must be a QuakeC name (QC-3.1). Whether it changes what the program means
//! (a local that would then hide a global, a name already taken, a word the dialect reads as a
//! keyword) is not guessed from rules: as IntelliJ's rename checks for conflicts, every module
//! the edits touch is resolved again with them applied, and the rename is refused unless every
//! definition, later declaration, use and diagnostic is the same, at the same places once the
//! edits have moved them.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use qc_diag::Span;
use qc_project::Loader;
use qc_sema::{Db, DefId, DefKind, FileId, Location, ModuleAnalysis, ModuleIndex};

use crate::FileRange;
use crate::host::AnalysisHost;
use crate::navigation::ModuleDef;
use crate::syntax::{name_at, name_token_near};

/// The `_x`, `_y` and `_z` suffixes of a vector's component names (QC-11.7).
const COMPONENTS: [&str; 3] = ["_x", "_y", "_z"];

/// The fields id's engine reads by name (EDICT-3.4): `gravity` and `items2`, and the
/// mission-pack ammo fields `Host_Give_f` sets (`GetEdictFieldValue(sv_player, "ammo_shells1")`
/// and so on in WinQuake/host_cmd.c, id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d).
const ENGINE_FIELDS: [&str; 9] = [
    "gravity",
    "items2",
    "ammo_shells1",
    "ammo_nails1",
    "ammo_lava_nails",
    "ammo_rockets1",
    "ammo_multi_rockets",
    "ammo_cells1",
    "ammo_plasma",
];

/// How the engine binds a name, so that renaming it breaks the progs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineBinding {
    /// A global declared before `end_sys_globals` (EDICT-2.2).
    SystemGlobal,
    /// A field declared before `end_sys_fields` (EDICT-3.2).
    SystemField,
    /// A field the engine reads by name (EDICT-3.4).
    FieldByName,
}

/// What a rename that is made cannot check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Caution {
    /// A function, which a map may name as an entity's `classname` or a key's value
    /// (EDICT-8.6, EDICT-8.2).
    MapFunction(String),
    /// A field, which a map may name as a key (EDICT-8.2).
    MapField(String),
}

impl Caution {
    /// The renamed name.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::MapFunction(name) | Self::MapField(name) => name,
        }
    }
}

impl fmt::Display for Caution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MapFunction(name) => write!(
                f,
                "a map that names the function {name} as a classname or a key's value still \
                 names the old name"
            ),
            Self::MapField(name) => write!(
                f,
                "a map whose entities set the field {name} as a key still names the old name"
            ),
        }
    }
}

/// A rename: its edits, and what it cannot check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rename {
    /// The edits of each file, by path.
    pub edits: Vec<FileEdit>,
    /// What the edits cannot have changed, if anything.
    pub caution: Option<Caution>,
}

/// Why a rename cannot be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenameError {
    /// No resolved name is at the position.
    NoName,
    /// The new name is not a QuakeC name (QC-3.1).
    InvalidName(String),
    /// The compiler made the definition, so no source names it (FTEF-9.5).
    Generated(String),
    /// The name is a vector's component (QC-11.7), renamed with the vector.
    Component {
        /// The component's name.
        component: String,
        /// The vector's.
        vector: String,
    },
    /// The engine binds the name (EDICT-2.2, EDICT-3.2, EDICT-3.4).
    EngineName(String, EngineBinding),
    /// The name is written at this place through a macro (QC-9.7).
    InMacro(FileRange),
    /// With the new name, the place in the edited text means something else.
    Conflict(FileRange),
}

impl fmt::Display for RenameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoName => f.write_str("there is no name to rename here"),
            Self::InvalidName(name) => write!(
                f,
                "{name:?} is not a QuakeC name: it must start with a letter or _ and hold only \
                 letters, digits and _"
            ),
            Self::Generated(name) => write!(f, "{name} is made by the compiler"),
            Self::Component { component, vector } => write!(
                f,
                "{component} is a component of the vector {vector}: rename {vector}"
            ),
            Self::EngineName(name, EngineBinding::SystemGlobal) => write!(
                f,
                "{name} is a system global, which the engine finds by its place before \
                 end_sys_globals"
            ),
            Self::EngineName(name, EngineBinding::SystemField) => write!(
                f,
                "{name} is a system field, which the engine finds by its place before \
                 end_sys_fields"
            ),
            Self::EngineName(name, EngineBinding::FieldByName) => {
                write!(f, "the engine reads the field {name} by its name")
            }
            Self::InMacro(at) => write!(
                f,
                "the name is used through a macro in {} at byte {}",
                at.path.display(),
                at.span.start
            ),
            Self::Conflict(at) => write!(
                f,
                "the new name changes what the code means in {} at byte {}",
                at.path.display(),
                at.span.start
            ),
        }
    }
}

impl std::error::Error for RenameError {}

/// The edits a rename makes to one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEdit {
    /// The file.
    pub path: PathBuf,
    /// The spans to replace and their new text, in source order, none overlapping.
    pub edits: Vec<(Span, String)>,
}

impl<L: Loader> AnalysisHost<L> {
    /// The name at byte `offset` of `path` that a rename would edit: the span of the name
    /// under the cursor.
    ///
    /// # Errors
    ///
    /// [`RenameError::NoName`] when no resolved name is there, [`RenameError::Generated`]
    /// for a definition the compiler made, and [`RenameError::InMacro`] when the name there
    /// is a macro's use.
    pub fn prepare_rename(&mut self, path: &Path, offset: usize) -> Result<FileRange, RenameError> {
        let target = self.def_at(path, offset).ok_or(RenameError::NoName)?;
        let (state, data) = self.module(target.module).ok_or(RenameError::NoName)?;
        let def = data
            .analysis
            .index
            .def(target.def)
            .ok_or(RenameError::NoName)?;
        if def.generated {
            return Err(RenameError::Generated(def.name.clone()));
        }
        let index = &data.analysis.index;
        if let Some(binding) = engine_binding(index, target.def) {
            return Err(RenameError::EngineName(def.name.clone(), binding));
        }
        // A component is declared at its vector's name (QC-11.7).
        let written = state.db.file_text(def.location.file);
        if let Some(vector) = COMPONENTS
            .iter()
            .find_map(|suffix| def.name.strip_suffix(suffix))
            .filter(|vector| name_at(written, def.location.span, vector))
        {
            return Err(RenameError::Component {
                component: def.name.clone(),
                vector: vector.to_owned(),
            });
        }
        let path = qc_project::normalize(path);
        let file = state.file_id(&path).ok_or(RenameError::NoName)?;
        let token = data
            .parses
            .get(file.0)
            .and_then(|parse| name_token_near(parse, offset))
            .ok_or(RenameError::NoName)?;
        let span = token.span();
        if !name_at(state.db.file_text(file), span, &def.name) {
            return Err(RenameError::InMacro(FileRange { path, span }));
        }
        Ok(FileRange { path, span })
    }

    /// The edits that rename the definition of the name at byte `offset` of `path` to
    /// `new_name`, by file in path order, none when the name is already `new_name`, with what
    /// they cannot check.
    ///
    /// # Errors
    ///
    /// [`RenameError`] when there is no name there, it is the engine's or a component's,
    /// `new_name` is no QuakeC name, a place the name is written is a macro's use, or the edited
    /// program would mean something else.
    pub fn rename(
        &mut self,
        path: &Path,
        offset: usize,
        new_name: &str,
    ) -> Result<Rename, RenameError> {
        self.prepare_rename(path, offset)?;
        if !is_name(new_name) {
            return Err(RenameError::InvalidName(new_name.to_owned()));
        }
        let target = self.def_at(path, offset).ok_or(RenameError::NoName)?;
        let (old_name, kind, builtin) = self
            .module(target.module)
            .and_then(|(_, data)| {
                let def = data.analysis.index.def(target.def)?;
                Some((
                    def.name.clone(),
                    def.kind,
                    data.builtins.contains_key(&def.name),
                ))
            })
            .ok_or(RenameError::NoName)?;
        // A builtin is bound by its number or engine name (QC-13.3), and no map spawns one.
        let caution = match kind {
            DefKind::Function if !builtin => Some(Caution::MapFunction(old_name.clone())),
            DefKind::Field => Some(Caution::MapField(old_name.clone())),
            DefKind::Function | DefKind::Global | DefKind::Param | DefKind::Local => None,
        };
        if old_name == new_name {
            return Ok(Rename {
                edits: Vec::new(),
                caution: None,
            });
        }
        let same = self.same_defs(target);
        let mut edits: BTreeMap<PathBuf, Vec<(Span, String)>> = BTreeMap::new();
        let mut renamed: Vec<HashSet<DefId>> = vec![HashSet::new(); self.modules.len()];
        for at in &same {
            for (def, suffix) in self.renamed_defs(*at, &old_name) {
                if let Some(set) = renamed.get_mut(at.module) {
                    set.insert(def);
                }
                self.edits_of(at.module, def, &old_name, suffix, new_name, &mut edits)?;
            }
        }
        let mut out = Vec::new();
        for (path, mut list) in edits {
            list.sort();
            list.dedup();
            out.push(FileEdit { path, edits: list });
        }
        self.check_meaning(&out, &renamed, &old_name, new_name)?;
        Ok(Rename {
            edits: out,
            caution,
        })
    }

    /// The definitions a rename of `at` renames, each with the suffix its name has after the
    /// renamed name: the definition, and a vector's components declared with it (QC-11.7).
    fn renamed_defs(&self, at: ModuleDef, name: &str) -> Vec<(DefId, &'static str)> {
        let mut out = vec![(at.def, "")];
        let Some((_, data)) = self.module(at.module) else {
            return out;
        };
        let index = &data.analysis.index;
        let Some(def) = index.def(at.def) else {
            return out;
        };
        for suffix in COMPONENTS {
            let component = format!("{name}{suffix}");
            let found = index
                .defs
                .iter()
                .position(|d| d.location == def.location && d.name == component);
            if let Some(found) = found {
                out.push((DefId(found), suffix));
            }
        }
        out
    }

    /// Adds the edits renaming every place definition `def` of module `module` is written,
    /// its name being `old` and `suffix`, to `new` and `suffix`.
    fn edits_of(
        &self,
        module: usize,
        def: DefId,
        old: &str,
        suffix: &str,
        new: &str,
        edits: &mut BTreeMap<PathBuf, Vec<(Span, String)>>,
    ) -> Result<(), RenameError> {
        let Some((state, data)) = self.module(module) else {
            return Ok(());
        };
        let own = format!("{old}{suffix}");
        let replacement = format!("{new}{suffix}");
        for location in data.analysis.index.references(def, true) {
            let Some(path) = state.path(location.file) else {
                continue;
            };
            let text = state.db.file_text(location.file);
            if name_at(text, location.span, &own) {
                edits
                    .entry(path.to_path_buf())
                    .or_default()
                    .push((location.span, replacement.clone()));
            } else if suffix.is_empty() || !name_at(text, location.span, old) {
                // A component declared at its vector's name is renamed with the vector; any
                // other place that does not hold the name is a macro's use.
                return Err(RenameError::InMacro(FileRange {
                    path: path.to_path_buf(),
                    span: location.span,
                }));
            }
        }
        Ok(())
    }

    /// Resolves every module an edit touches again with the edits applied, and fails unless
    /// each means what it meant (see the module documentation).
    fn check_meaning(
        &mut self,
        edits: &[FileEdit],
        renamed: &[HashSet<DefId>],
        old: &str,
        new: &str,
    ) -> Result<(), RenameError> {
        let by_path: BTreeMap<&Path, &FileEdit> =
            edits.iter().map(|e| (e.path.as_path(), e)).collect();
        // A module that compiles an edited file but not the definition's has not been read.
        self.ensure_all();
        for (index, state) in self.modules.iter().enumerate() {
            if !state
                .files
                .iter()
                .any(|p| by_path.contains_key(p.as_path()))
            {
                continue;
            }
            let Some(data) = state.data() else {
                continue;
            };
            let mut db = Db::new();
            db.set_dialect(state.dialect);
            db.set_autoproto(state.autoproto());
            for (id, path) in state.files.iter().enumerate() {
                let text = state.db.file_text(FileId(id));
                let text = by_path
                    .get(path.as_path())
                    .map_or_else(|| text.to_vec(), |edit| apply(text, &edit.edits));
                db.set_file(FileId(id), path.clone(), text);
            }
            let after = db.analyze_module(state.input());
            let moved = Moved {
                files: &state.files,
                edits: &by_path,
            };
            let empty = HashSet::new();
            let renamed = renamed.get(index).unwrap_or(&empty);
            if let Some(location) =
                first_difference(&data.analysis, &after, &moved, renamed, old, new)
            {
                return Err(RenameError::Conflict(FileRange {
                    path: state
                        .path(location.file)
                        .map(Path::to_path_buf)
                        .unwrap_or_default(),
                    span: location.span,
                }));
            }
        }
        Ok(())
    }
}

/// How the engine binds definition `def`, if it does: a global or field declared before
/// `end_sys_globals` or `end_sys_fields` (EDICT-2.2, EDICT-3.2), or a field it reads by name
/// (EDICT-3.4). Definitions are in declaration order (QC-12.2), so "before" is a smaller id.
fn engine_binding(index: &ModuleIndex, def: DefId) -> Option<EngineBinding> {
    let definition = index.def(def)?;
    let before = |marker: &str, kinds: &[DefKind]| {
        index
            .defs
            .iter()
            .position(|d| d.name == marker && d.owner.is_none())
            .is_some_and(|end| def.0 < end && kinds.contains(&definition.kind))
    };
    if before("end_sys_globals", &[DefKind::Global, DefKind::Function]) {
        return Some(EngineBinding::SystemGlobal);
    }
    if before("end_sys_fields", &[DefKind::Field]) {
        return Some(EngineBinding::SystemField);
    }
    (definition.kind == DefKind::Field && ENGINE_FIELDS.contains(&definition.name.as_str()))
        .then_some(EngineBinding::FieldByName)
}

/// Whether `name` is a QuakeC name (QC-3.1): an ASCII letter or `_`, then ASCII letters,
/// digits and `_`.
fn is_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// `text` with `edits`, sorted and not overlapping, applied.
fn apply(text: &[u8], edits: &[(Span, String)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0_usize;
    for (span, replacement) in edits {
        out.extend_from_slice(text.get(at..span.start).unwrap_or_default());
        out.extend_from_slice(replacement.as_bytes());
        at = span.end;
    }
    out.extend_from_slice(text.get(at..).unwrap_or_default());
    out
}

/// Where a place of the text before the edits is after them.
struct Moved<'a> {
    files: &'a [PathBuf],
    edits: &'a BTreeMap<&'a Path, &'a FileEdit>,
}

impl Moved<'_> {
    /// Offset `offset` of `file` after the edits before it; an edit ending at `offset` is
    /// before it.
    fn offset(&self, file: FileId, offset: usize) -> usize {
        let Some(edit) = self
            .files
            .get(file.0)
            .and_then(|path| self.edits.get(path.as_path()))
        else {
            return offset;
        };
        let mut out = offset;
        for (span, replacement) in &edit.edits {
            if span.end > offset {
                break;
            }
            out = out
                .saturating_sub(span.end.saturating_sub(span.start))
                .saturating_add(replacement.len());
        }
        out
    }

    /// Location `location` after the edits.
    fn location(&self, location: Location) -> Location {
        Location {
            file: location.file,
            span: Span::new(
                self.offset(location.file, location.span.start),
                self.offset(location.file, location.span.end),
            ),
        }
    }
}

/// The first place where the analysis after the edits differs from the one before, the
/// renamed definitions' names aside; `None` when they agree.
fn first_difference(
    before: &ModuleAnalysis,
    after: &ModuleAnalysis,
    moved: &Moved<'_>,
    renamed: &HashSet<DefId>,
    old: &str,
    new: &str,
) -> Option<Location> {
    let (b, a) = (&before.index, &after.index);
    for (id, def) in b.defs.iter().enumerate() {
        let location = moved.location(def.location);
        let name = if renamed.contains(&DefId(id)) {
            def.name.replacen(old, new, 1)
        } else {
            def.name.clone()
        };
        let Some(other) = a.defs.get(id) else {
            return Some(location);
        };
        if other.name != name || other.kind != def.kind || other.location != location {
            return Some(location);
        }
    }
    if let Some(extra) = a.defs.get(b.defs.len()) {
        return Some(extra.location);
    }
    let pairs = [(&b.refs, &a.refs), (&b.redeclarations, &a.redeclarations)];
    for (was, now) in pairs {
        for (index, reference) in was.iter().enumerate() {
            let location = moved.location(reference.location);
            match now.get(index) {
                Some(other) if other.def == reference.def && other.location == location => {}
                Some(_) | None => return Some(location),
            }
        }
        if let Some(extra) = now.get(was.len()) {
            return Some(extra.location);
        }
    }
    let codes = |analysis: &ModuleAnalysis, move_it: bool| -> Vec<(FileId, Span, &'static str)> {
        let mut out: Vec<(FileId, Span, &'static str)> = analysis
            .diagnostics()
            .map(|d| {
                let at = Location {
                    file: d.file,
                    span: d.diagnostic.span,
                };
                let at = if move_it { moved.location(at) } else { at };
                (at.file, at.span, d.diagnostic.code.as_str())
            })
            .collect();
        out.sort_unstable();
        out
    };
    let (was, now) = (codes(before, true), codes(after, false));
    if was != now {
        let differs = was
            .iter()
            .zip(&now)
            .find(|(x, y)| x != y)
            .map(|(x, _)| *x)
            .or_else(|| was.get(now.len()).copied())
            .or_else(|| now.get(was.len()).copied());
        return differs.map(|(file, span, _)| Location { file, span });
    }
    None
}
