// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary bytes to the semantic model (P2-54) as a module of one to three files, split
//! at the first two NULs after a leading byte that picks the profile: bit 0 the FTEQCC
//! profile, bit 1 autoprototyping, bit 2 `-Fcaseinsens` (FTEF-19.29) and bit 3 `-Fno-qcfuncs`
//! (FTEF-19.28), the last two as the command line and as the resolver's options. The module is resolved as written (`Db::module_index`) and
//! through the preprocessor (`Db::analyze_module`), and checked against every engine for
//! missing builtins (`qc_engines::missing_builtins`). Fails on a panic, on a definition,
//! reference or diagnostic outside the bytes of its file, and when editing each file in turn
//! and re-resolving gives another index than resolving the edited files from scratch.

#![no_main]

use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;
use qc_engines::{Engine, missing_builtins};
use qc_preproc::CommandLine;
use qc_project::ModuleKind;
use qc_sema::lower::lower_file;
use qc_sema::{Db, FileId, Location, ModuleIndex, ModuleInput, ResolveOptions, resolve_module};
use qc_syntax::Dialect;

/// Whether `at` is a span inside the bytes of its file.
fn inside(files: &[&[u8]], at: Location) -> bool {
    files
        .get(at.file.0)
        .is_some_and(|text| at.span.start <= at.span.end && at.span.end <= text.len())
}

fn check_index(files: &[&[u8]], index: &ModuleIndex) {
    for def in &index.defs {
        assert!(inside(files, def.location), "{def:?}");
    }
    for reference in &index.refs {
        assert!(inside(files, reference.location), "{reference:?}");
    }
    for d in &index.diagnostics {
        let at = Location { file: d.file, span: d.diagnostic.span };
        assert!(inside(files, at), "{d:?}");
    }
}

fn db_of(files: &[&[u8]], dialect: Dialect, autoproto: bool) -> Db {
    let mut db = Db::new();
    db.set_dialect(dialect);
    db.set_autoproto(autoproto);
    for (index, text) in files.iter().enumerate() {
        db.set_file(FileId(index), PathBuf::from(format!("f{index}.qc")), text.to_vec());
    }
    db
}

fuzz_target!(|data: &[u8]| {
    let Some((&flags, rest)) = data.split_first() else {
        return;
    };
    let dialect = if flags & 1 == 1 { Dialect::Fteqcc } else { Dialect::Vanilla };
    let autoproto = flags & 2 == 2;
    let case_insensitive = flags & 4 == 4;
    let qcfuncs_off = flags & 8 == 8;
    let args: Vec<&str> = [(case_insensitive, "-Fcaseinsens"), (qcfuncs_off, "-Fno-qcfuncs")]
        .into_iter()
        .filter_map(|(on, arg)| on.then_some(arg))
        .collect();
    let files: Vec<&[u8]> = rest.splitn(3, |b| *b == 0).collect();
    let ids: Vec<FileId> = (0..files.len()).map(FileId).collect();
    let input = ModuleInput {
        compiled: ids.clone(),
        command_line: CommandLine::new(&args),
        ..ModuleInput::default()
    };

    let mut db = db_of(&files, dialect, autoproto);
    let index = db.module_index(&ids);
    check_index(&files, &index);
    let analysis = db.analyze_module(&input);
    check_index(&files, &analysis.index);
    for d in &analysis.syntax {
        let at = Location { file: d.file, span: d.diagnostic.span };
        assert!(inside(&files, at), "{d:?}");
    }

    // The missing-builtin check reads the parses and the index resolved from them.
    let parses: Vec<_> = files.iter().map(|text| qc_syntax::parse_with(text, dialect)).collect();
    let lowered: Vec<_> = parses.iter().map(|parse| lower_file(parse, dialect)).collect();
    // The struct update keeps the build when `ResolveOptions` gains a field.
    let options = ResolveOptions {
        dialect,
        autoproto,
        case_insensitive,
        qcfuncs_off,
        ..ResolveOptions::default()
    };
    let resolved = resolve_module(&lowered, options);
    for engine in Engine::ALL {
        for kind in [ModuleKind::Ssqc, ModuleKind::Csqc, ModuleKind::Menuqc] {
            for d in missing_builtins(engine, kind, &parses, &resolved, dialect) {
                let at = Location { file: d.file, span: d.diagnostic.span };
                assert!(inside(&files, at), "{d:?}");
            }
        }
    }

    // Editing each file in turn, to the next file's text with a declaration of its own added
    // so that every edit changes the file, and re-resolving equals resolving the edited files
    // from scratch: no file's cached queries outlive an edit of it or of another file.
    let mut edited: Vec<Vec<u8>> = files.iter().map(|text| text.to_vec()).collect();
    for index in 0..files.len() {
        let mut text = files[(index + 1) % files.len()].to_vec();
        text.extend_from_slice(format!("\nfloat fuzz_edit_{index};\n").as_bytes());
        edited[index] = text.clone();
        db.set_file(FileId(index), PathBuf::from(format!("f{index}.qc")), text);
        let views: Vec<&[u8]> = edited.iter().map(Vec::as_slice).collect();
        let mut fresh = db_of(&views, dialect, autoproto);
        assert_eq!(db.module_index(&ids), fresh.module_index(&ids));
        assert_eq!(db.analyze_module(&input), fresh.analyze_module(&input));
    }
});
