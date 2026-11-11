// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's own diagnostics and the quick fixes for the common ones (P4-34, UI-18).
//!
//! The diagnostics are the editor's, not the compiler's (QCD-7.115, QCD-7.116): a local no
//! expression names (`W0321`), and a model or sound a call uses that no precache call names
//! (`W0322`), which id's engine refuses at run time. The quick fixes are rust-analyzer's and VS
//! Code's: each a titled edit for one diagnostic at the caret, as `textDocument/codeAction`
//! offers them:
//!
//! - an unknown name that is a builtin of the module's engine is declared with its number and
//!   signature from the engine-target database, at the top of the file (`E0301`);
//! - an unknown field is replaced by the field of the module nearest it in spelling, within two
//!   edits (`E0301`, `E0318`, `E0319`);
//! - an unused local's declaration is removed (`W0321`);
//! - a missing precache is added at the start of `worldspawn`'s body (`W0322`).

use std::collections::BTreeSet;
use std::path::Path;

use qc_diag::{Code, Diagnostic, Span};
use qc_engines::BuiltinKey;
use qc_project::Loader;
use qc_sema::{DefKind, FileDiagnostic, FileId, ModuleIndex};
use qc_syntax::Parse;
use qc_syntax::ast::{self, AstNode as _};

use crate::host::AnalysisHost;
use crate::rename::FileEdit;

/// The calls that precache a model, and those that precache a sound.
const PRECACHE_MODEL: [&str; 2] = ["precache_model", "precache_model2"];
/// As [`PRECACHE_MODEL`], for sounds.
const PRECACHE_SOUND: [&str; 2] = ["precache_sound", "precache_sound2"];

/// What a call uses a precached name for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Precached {
    /// A model, which `precache_model` names.
    Model,
    /// A sound, which `precache_sound` names.
    Sound,
}

impl Precached {
    /// The call that uses the name, and which argument, from 0, it is (QCD-7.116).
    fn of_call(callee: &str) -> Option<(Self, usize)> {
        match callee {
            "setmodel" => Some((Self::Model, 1)),
            "sound" => Some((Self::Sound, 2)),
            "ambientsound" => Some((Self::Sound, 1)),
            _ => None,
        }
    }

    /// The call that precaches it.
    const fn precache(self) -> &'static str {
        match self {
            Self::Model => "precache_model",
            Self::Sound => "precache_sound",
        }
    }
}

/// A quick fix: what it does and the edits that do it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeAction {
    /// What it does, as the editor's picker lists it.
    pub title: String,
    /// The diagnostic it fixes.
    pub code: Code,
    /// The edit, in one file.
    pub edit: FileEdit,
}

/// Each call of `parse` whose callee is a plain name: the name and the arguments.
fn calls(parse: &Parse) -> Vec<(String, Vec<ast::Expr>)> {
    parse
        .syntax()
        .descendants()
        .filter_map(|node| {
            let call = ast::CallExpr::cast(node)?;
            let Some(ast::Expr::Name(callee)) = call.callee() else {
                return None;
            };
            let name = String::from_utf8_lossy(callee.name()?.text()).into_owned();
            Some((name, call.args()?.args().collect()))
        })
        .collect()
}

/// The string literal `expr` is, as written with its quotes, and its span.
fn string_literal(expr: &ast::Expr) -> Option<(String, Span)> {
    let ast::Expr::Literal(literal) = expr else {
        return None;
    };
    if literal.kind()? != ast::LiteralKind::String {
        return None;
    }
    let token = literal.token()?;
    Some((
        String::from_utf8_lossy(token.text()).into_owned(),
        token.span(),
    ))
}

/// The editor's diagnostics of a module (QCD-7.115, QCD-7.116): unused locals, and models and
/// sounds used and never precached.
pub(crate) fn editor_diagnostics(index: &ModuleIndex, parses: &[Parse]) -> Vec<FileDiagnostic> {
    let mut out = Vec::new();
    let named: BTreeSet<_> = index.refs().iter().map(|r| r.def).collect();
    // A vector's components are locals of their own, written where the vector is (QC-11.7):
    // the places a named local was declared at, so a vector any component of which is named
    // is used, and a component is never reported on its own.
    let used_at: BTreeSet<_> = index
        .definitions()
        .filter(|(id, def)| def.kind == DefKind::Local && named.contains(id))
        .map(|(_, def)| (def.location.file, def.location.span.start))
        .collect();
    let mut reported = BTreeSet::new();
    for (_, def) in index.definitions() {
        let at = (def.location.file, def.location.span.start);
        if def.kind == DefKind::Local
            && !def.generated
            && def.member_of.is_none()
            && !used_at.contains(&at)
            && reported.insert(at)
        {
            out.push(FileDiagnostic {
                file: def.location.file,
                diagnostic: Diagnostic {
                    code: Code::W0321,
                    message: format!("Local {} is never used", def.name),
                    span: def.location.span,
                },
            });
        }
    }
    let mut precached: BTreeSet<(bool, String)> = BTreeSet::new();
    for parse in parses {
        for (callee, args) in calls(parse) {
            let model = PRECACHE_MODEL.contains(&callee.as_str());
            if (model || PRECACHE_SOUND.contains(&callee.as_str()))
                && let Some((text, _)) = args.first().and_then(string_literal)
            {
                precached.insert((model, text));
            }
        }
    }
    for (index, parse) in parses.iter().enumerate() {
        for (callee, args) in calls(parse) {
            let Some((kind, at)) = Precached::of_call(&callee) else {
                continue;
            };
            let Some((text, span)) = args.get(at).and_then(string_literal) else {
                continue;
            };
            // The empty name, and a brush model `*N`, are precached by the engine itself.
            let name = text.trim_matches('"');
            if name.is_empty() || name.starts_with('*') {
                continue;
            }
            if !precached.contains(&(kind == Precached::Model, text.clone())) {
                out.push(FileDiagnostic {
                    file: FileId::new(index),
                    diagnostic: Diagnostic {
                        code: Code::W0322,
                        message: format!("{text} is never precached"),
                        span,
                    },
                });
            }
        }
    }
    out
}

/// How many single-character edits turn `a` into `b`, stopping past `limit`.
fn distance(a: &str, b: &str, limit: usize) -> Option<usize> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut row = vec![i.saturating_add(1)];
        for (j, cb) in b.iter().enumerate() {
            let replace = previous
                .get(j)
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(usize::from(ca != cb));
            let insert = row.get(j).copied().unwrap_or(usize::MAX).saturating_add(1);
            let delete = previous
                .get(j.saturating_add(1))
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(1);
            row.push(replace.min(insert).min(delete));
        }
        previous = row;
    }
    previous.last().copied().filter(|d| *d <= limit)
}

/// The quotes around `name` in a message, as the diagnostics write `Unknown value "NAME"`.
fn quoted(message: &str) -> Option<&str> {
    let start = message.find('"')?.saturating_add(1);
    let rest = message.get(start..)?;
    rest.get(..rest.find('"')?)
}

impl<L: Loader> AnalysisHost<L> {
    /// The quick fixes for the diagnostics of `path` that meet `range`, in the order of the
    /// diagnostics.
    pub fn code_actions(&mut self, path: &Path, range: Span) -> Vec<CodeAction> {
        let diagnostics = self.diagnostics(path);
        let Some(text) = self.text(path) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for diagnostic in diagnostics {
            let span = diagnostic.span;
            if span.end < range.start || span.start > range.end {
                continue;
            }
            let action = match diagnostic.code {
                Code::E0301 | Code::E0318 | Code::E0319 => self
                    .declare_builtin(path, &diagnostic)
                    .or_else(|| self.nearest_field(path, &text, &diagnostic)),
                Code::W0321 => remove_local(path, &text, &diagnostic),
                Code::W0322 => self.add_precache(path, &diagnostic),
                _ => None,
            };
            out.extend(action);
        }
        out
    }

    /// `E0301` on a name the module's engine has as a builtin: its declaration at the top of
    /// the file.
    fn declare_builtin(&mut self, path: &Path, diagnostic: &Diagnostic) -> Option<CodeAction> {
        if diagnostic.code != Code::E0301 {
            return None;
        }
        let name = quoted(&diagnostic.message)?.to_owned();
        let &(module, _) = self.modules_of(path).first()?;
        let state = self.modules.get(module)?;
        let builtin = state
            .engine
            .builtins(state.kind)
            .into_iter()
            .find(|b| b.name == name && !b.signature.is_empty())?;
        let key = match builtin.key {
            BuiltinKey::Number(number) => format!("#{number}"),
            BuiltinKey::Name(engine) => format!("#0:{engine}"),
        };
        let line = format!("{} {name} = {key};\n", builtin.signature);
        Some(CodeAction {
            title: format!("Declare the builtin {name} = {key}"),
            code: diagnostic.code,
            edit: FileEdit {
                path: path.to_path_buf(),
                edits: vec![(Span::new(0, 0), line)],
            },
        })
    }

    /// An unknown name after a `.`: the field of the module nearest it in spelling.
    fn nearest_field(
        &mut self,
        path: &Path,
        text: &[u8],
        diagnostic: &Diagnostic,
    ) -> Option<CodeAction> {
        let span = diagnostic.span;
        let written = String::from_utf8_lossy(text.get(span.start..span.end)?).into_owned();
        // Only a name after `.`, a field's place.
        let before = text.get(..span.start)?;
        if before.iter().rev().find(|b| !b.is_ascii_whitespace()) != Some(&b'.') {
            return None;
        }
        let &(module, _) = self.modules_of(path).first()?;
        let (_, data) = self.module(module)?;
        let best = data
            .analysis
            .index
            .defs()
            .iter()
            .filter(|def| def.kind == DefKind::Field && def.name != written)
            .filter_map(|def| Some((distance(&written, &def.name, 2)?, def.name.clone())))
            .min()?;
        Some(CodeAction {
            title: format!("Change {written} to {}", best.1),
            code: diagnostic.code,
            edit: FileEdit {
                path: path.to_path_buf(),
                edits: vec![(span, best.1)],
            },
        })
    }

    /// `W0322`: the precache call at the start of `worldspawn`'s body.
    fn add_precache(&mut self, path: &Path, diagnostic: &Diagnostic) -> Option<CodeAction> {
        let &(module, file) = self.modules_of(path).first()?;
        let (state, data) = self.module(module)?;
        let parse = data.parses.get(file.index())?;
        let kind = calls(parse).into_iter().find_map(|(callee, args)| {
            let (kind, at) = Precached::of_call(&callee)?;
            let (_, span) = args.get(at).and_then(string_literal)?;
            (span == diagnostic.span).then_some(kind)
        })?;
        let literal = String::from_utf8_lossy(
            state
                .db
                .file_text(file)
                .get(diagnostic.span.start..diagnostic.span.end)?,
        )
        .into_owned();
        let worldspawn = data.analysis.index.defs().iter().find(|def| {
            def.kind == DefKind::Function && def.name == "worldspawn" && def.member_of.is_none()
        })?;
        let at = worldspawn.location;
        let target = state.path(at.file)?.to_path_buf();
        let body = state.db.file_text(at.file);
        let open = body
            .get(at.span.end..)?
            .iter()
            .position(|b| *b == b'{')?
            .saturating_add(at.span.end)
            .saturating_add(1);
        let call = kind.precache();
        Some(CodeAction {
            title: format!("Add {call} ({literal}) to worldspawn"),
            code: diagnostic.code,
            edit: FileEdit {
                path: target,
                edits: vec![(Span::new(open, open), format!("\n\t{call} ({literal});"))],
            },
        })
    }
}

/// `W0321`: the unused local's declaration removed: its line when it declares that local alone,
/// else the name and the comma beside it.
fn remove_local(path: &Path, text: &[u8], diagnostic: &Diagnostic) -> Option<CodeAction> {
    let span = diagnostic.span;
    let name = String::from_utf8_lossy(text.get(span.start..span.end)?).into_owned();
    let line_start = text
        .get(..span.start)?
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(0, |i| i.saturating_add(1));
    let line_end = text
        .get(span.end..)?
        .iter()
        .position(|b| *b == b'\n')
        .map_or(text.len(), |i| span.end.saturating_add(i).saturating_add(1));
    let line = String::from_utf8_lossy(text.get(line_start..line_end)?).into_owned();
    let words: Vec<&str> = line
        .split(|c: char| c.is_whitespace() || c == ';')
        .filter(|w| !w.is_empty())
        .collect();
    let alone = words.first() == Some(&"local") && !line.contains(',') && words.len() == 3;
    let cut = if alone {
        Span::new(line_start, line_end)
    } else {
        let after = text.get(span.end..line_end)?;
        let skip = after
            .iter()
            .take_while(|b| **b == b' ' || **b == b'\t')
            .count();
        if after.get(skip) == Some(&b',') {
            let rest = after.get(skip.saturating_add(1)..)?;
            let spaces = rest
                .iter()
                .take_while(|b| **b == b' ' || **b == b'\t')
                .count();
            Span::new(
                span.start,
                span.end
                    .saturating_add(skip)
                    .saturating_add(1)
                    .saturating_add(spaces),
            )
        } else {
            let before = text.get(line_start..span.start)?;
            let comma = before.iter().rposition(|b| *b == b',')?;
            if before
                .get(comma.saturating_add(1)..)?
                .iter()
                .any(|b| !b.is_ascii_whitespace())
            {
                return None;
            }
            Span::new(line_start.saturating_add(comma), span.end)
        }
    };
    Some(CodeAction {
        title: format!("Remove the unused local {name}"),
        code: diagnostic.code,
        edit: FileEdit {
            path: path.to_path_buf(),
            edits: vec![(cut, String::new())],
        },
    })
}
