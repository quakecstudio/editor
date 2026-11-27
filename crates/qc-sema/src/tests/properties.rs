// SPDX-License-Identifier: GPL-3.0-or-later

//! Property tests (TEST-4) for the invariants the crate README states: every definition,
//! reference, redeclaration and diagnostic is at a span of its source file, under both
//! profiles and through the preprocessor or not; and re-resolving after an edit gives what
//! resolving the edited files from scratch gives.

use std::path::PathBuf;

use proptest::prelude::*;
use qc_syntax::Dialect;

use crate::db::{Db, ModuleAnalysis};
use crate::expand::ModuleInput;
use crate::resolve::ModuleIndex;
use crate::symbol::{FileId, Location};

/// A fragment of QuakeC source, so token soup has the shape of declarations, bodies, macros
/// and the full dialect's forms.
fn fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("float".to_owned()),
        Just("void".to_owned()),
        Just("vector".to_owned()),
        Just("entity".to_owned()),
        Just("string".to_owned()),
        Just("local".to_owned()),
        Just("if".to_owned()),
        Just("else".to_owned()),
        Just("while".to_owned()),
        Just("return".to_owned()),
        Just("class".to_owned()),
        Just("accessor".to_owned()),
        Just("get".to_owned()),
        Just("virtual:".to_owned()),
        Just("enum".to_owned()),
        Just("typedef".to_owned()),
        Just("super::".to_owned()),
        Just("this".to_owned()),
        Just("::".to_owned()),
        Just("->".to_owned()),
        "[a-c]{1,2}",
        "-?[0-9]{1,2}(\\.[0-9])?",
        Just("#0:a".to_owned()),
        Just("\"s\"".to_owned()),
        Just("$frame a b\n".to_owned()),
        Just("$a".to_owned()),
        Just("\n#define a b\n".to_owned()),
        Just("\n#define m(x) x x\n".to_owned()),
        Just("\n#ifdef a\n".to_owned()),
        Just("\n#endif\n".to_owned()),
        Just("\n#include \"f1.qc\"\n".to_owned()),
        Just("\n#pragma noref 1\n".to_owned()),
        "[-+*/&|!=<>;,.(){}\\[\\]#:?]{1,2}",
        Just("\n".to_owned()),
    ]
}

/// One to three files of token soup.
fn files() -> impl Strategy<Value = Vec<String>> {
    proptest::collection::vec(
        proptest::collection::vec(fragment(), 0..40).prop_map(|parts| parts.join(" ")),
        1..=3,
    )
}

fn dialect() -> impl Strategy<Value = Dialect> {
    prop_oneof![Just(Dialect::Vanilla), Just(Dialect::Fteqcc)]
}

/// A database holding `files` as `f0.qc`, `f1.qc`, ... in `dialect`.
fn db_of(files: &[String], dialect: Dialect) -> Db {
    let mut db = Db::new();
    db.set_dialect(dialect);
    for (index, text) in files.iter().enumerate() {
        db.set_file(
            FileId::new(index),
            PathBuf::from(format!("f{index}.qc")),
            text.as_bytes().to_vec(),
        );
    }
    db
}

fn ids(files: &[String]) -> Vec<FileId> {
    (0..files.len()).map(FileId::new).collect()
}

fn input(files: &[String]) -> ModuleInput {
    ModuleInput {
        compiled: ids(files),
        ..ModuleInput::default()
    }
}

/// Whether `at` is a span inside the bytes of its file.
fn inside(files: &[String], at: Location) -> bool {
    files
        .get(at.file.index())
        .is_some_and(|text| at.span.start <= at.span.end && at.span.end <= text.len())
}

/// Every location of an index, and of the diagnostics of an analysis, is inside its file.
fn index_inside(files: &[String], index: &ModuleIndex) -> Result<(), TestCaseError> {
    for def in &index.defs {
        prop_assert!(inside(files, def.location), "{def:?}");
    }
    for reference in index.refs.iter().chain(&index.redeclarations) {
        prop_assert!(inside(files, reference.location), "{reference:?}");
    }
    for d in &index.diagnostics {
        let at = Location {
            file: d.file,
            span: d.diagnostic.span,
        };
        prop_assert!(inside(files, at), "{d:?}");
    }
    Ok(())
}

fn analysis_inside(files: &[String], analysis: &ModuleAnalysis) -> Result<(), TestCaseError> {
    index_inside(files, &analysis.index)?;
    for d in &analysis.syntax {
        let at = Location {
            file: d.file,
            span: d.diagnostic.span,
        };
        prop_assert!(inside(files, at), "{d:?}");
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    // spec QC-9.7: every definition, reference and diagnostic is at a span of a source file,
    // whether the files are resolved as written or through the preprocessor, in either profile.
    #[test]
    fn every_location_is_inside_its_file(files in files(), dialect in dialect()) {
        let mut db = db_of(&files, dialect);
        index_inside(&files, &db.module_index(&ids(&files)))?;
        analysis_inside(&files, &db.analyze_module(&input(&files)))?;
    }

    // spec QC-9.7: any bytes, not only QuakeC's shape, keep every location inside its file.
    #[test]
    fn any_bytes_keep_locations_inside(
        bytes in proptest::collection::vec(any::<u8>(), 0..256),
        dialect in dialect(),
    ) {
        let files = [String::from_utf8_lossy(&bytes).into_owned()];
        let mut db = db_of(&files, dialect);
        index_inside(&files, &db.module_index(&ids(&files)))?;
        analysis_inside(&files, &db.analyze_module(&input(&files)))?;
    }

    // spec QC-12.5: re-resolving after an edit to one file gives the index and analysis that
    // resolving the edited files from scratch gives.
    #[test]
    fn an_edit_then_re_resolution_equals_a_fresh_one(
        files in files(),
        edit in proptest::collection::vec(fragment(), 0..40),
        which in 0usize..3,
        dialect in dialect(),
    ) {
        let which = which.checked_rem(files.len()).unwrap();
        let mut db = db_of(&files, dialect);
        let _ = db.module_index(&ids(&files));
        let _ = db.analyze_module(&input(&files));
        let mut edited = files;
        edited[which] = edit.join(" ");
        db.set_file(
            FileId::new(which),
            PathBuf::from(format!("f{which}.qc")),
            edited[which].as_bytes().to_vec(),
        );
        let mut fresh = db_of(&edited, dialect);
        prop_assert_eq!(db.module_index(&ids(&edited)), fresh.module_index(&ids(&edited)));
        prop_assert_eq!(
            db.analyze_module(&input(&edited)),
            fresh.analyze_module(&input(&edited))
        );
    }
}

// spec FTEF-19.85: the token soup that overflowed the stack on Linux (L-81, found from proptest
// seed 2136 of a run of `every_location_is_inside_its_file`): an enum named `enum`'s base reads
// back to itself through the includes; every location stays inside its file, on a stack the
// size the property test's cases fit in
#[test]
fn an_enum_based_on_itself_keeps_locations_inside() {
    let files = vec![
        "\n#endif\n enum enum \n#endif\n virtual: virtual: entity \n#include \"f1.qc\"\n \n#endif\n \
         typedef return \n#ifdef a\n typedef \n#include \"f1.qc\"\n accessor \n#endif\n $a \n#endif\n \
         return \"s\" this return ba if"
            .to_owned(),
        "enum \n#endif\n \n#define a b\n if \"s\" this vector \"s\"".to_owned(),
    ];
    let checked = std::thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(move || {
            let mut db = db_of(&files, Dialect::Fteqcc);
            index_inside(&files, &db.module_index(&ids(&files)))?;
            analysis_inside(&files, &db.analyze_module(&input(&files)))
        })
        .unwrap()
        .join()
        .unwrap();
    checked.unwrap();
}

/// A stack the property test's ordinary cases fit in, so a recursion with no bound fails at
/// once rather than after the default stack (L-81).
const SMALL_STACK: usize = 96 * 1024;
