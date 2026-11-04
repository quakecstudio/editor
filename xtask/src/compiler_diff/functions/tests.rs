// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap, index or match is a failed test"
)]

use qc_progs::{Function, ProgsFile, Statement, write as write_progs_file};
use qc_types::Slot;

use super::*;
use crate::exec::{Fake, Reply};
use crate::oracles::testing::fake_oracles;
use crate::test_dirs::fresh;

fn names(list: &[&str]) -> BTreeSet<Vec<u8>> {
    list.iter().map(|n| n.as_bytes().to_vec()).collect()
}

/// A progs whose `f` calls `g` by name and calls through a variable, and whose `g` is done:
/// statements 1 and 2 are `f`'s, 3 is `g`'s.
fn calling_progs() -> Vec<u8> {
    let mut globals = vec![Slot::ZERO; 32];
    globals[10] = Slot::from_i32(2);
    let op = |op: Opcode| u32::from(op.number());
    let statements = vec![
        Statement::default(),
        Statement {
            op: op(Opcode::Call0),
            a: 10,
            b: 0,
            c: 0,
        },
        Statement {
            op: op(Opcode::Call1),
            a: 11,
            b: 0,
            c: 0,
        },
        Statement {
            op: op(Opcode::Done),
            a: 0,
            b: 0,
            c: 0,
        },
    ];
    let functions = vec![
        Function::default(),
        Function {
            first_statement: 1,
            s_name: 1,
            ..Function::default()
        },
        Function {
            first_statement: 3,
            s_name: 3,
            ..Function::default()
        },
    ];
    write_progs_file(&ProgsFile {
        statements,
        functions,
        globals,
        strings: b"\0f\0g\0\0".to_vec(),
        ..ProgsFile::default()
    })
    .unwrap()
}

// spec DIFF-15.1: a block is emptied only when it runs from `{` to `}` with every conditional
// directive in it closed in it
#[test]
fn only_a_whole_block_is_emptied() {
    assert!(is_whole_block(b"{ x = 1; }"));
    assert!(is_whole_block(b"{\n#ifdef A\n x = 1;\n #endif\n}"));
    assert!(is_whole_block(b"{\n#if A\n#ifndef B\n#endif\n#endif\n}"));
    assert!(!is_whole_block(b"{\n#ifdef A\n x = 1;\n}"));
    assert!(!is_whole_block(b"{\n#endif\n#ifdef A\n}"));
    assert!(!is_whole_block(b"x = 1; }"));
    assert!(!is_whole_block(b"{ x = 1;"));
    assert!(!is_whole_block(b""));
}

// spec DIFF-15.1: the cut program keeps the bodies of the functions kept and empties every
// other one, leaving the text around them and the files with none as they are
#[test]
fn a_cut_empties_every_body_not_kept() {
    let a = PathBuf::from("a.qc");
    let b = PathBuf::from("b.qc");
    let text = b"void() f = { f1(); };\nvoid() g = { g1(); };\nfloat h() { return 2; }\n";
    let sources: BTreeMap<PathBuf, Vec<u8>> = [
        (a.clone(), text.to_vec()),
        (b.clone(), b"float x;\n".to_vec()),
    ]
    .into();
    let body = |name: &str, block: &str| {
        let start = text
            .windows(block.len())
            .position(|w| w == block.as_bytes())
            .unwrap();
        Body {
            name: name.as_bytes().to_vec(),
            path: a.clone(),
            start,
            end: start + block.len(),
        }
    };
    let bodies = [
        body("f", "{ f1(); }"),
        body("g", "{ g1(); }"),
        body("h", "{ return 2; }"),
    ];
    let files = cut(&bodies, &sources, &names(&["g"]));
    assert_eq!(
        files.len(),
        1,
        "a file with no body emptied is not rewritten"
    );
    assert_eq!(
        String::from_utf8(files[&a].clone()).unwrap(),
        "void() f = {};\nvoid() g = { g1(); };\nfloat h() {}\n"
    );
    assert!(cut(&bodies, &sources, &names(&["f", "g", "h"])).is_empty());
}

// spec DIFF-15.2: a function's calls by name are the functions the globals its CALLs read
// hold; a call through a variable, whose global holds no function, adds none, and the kept
// functions are the closure of the calls, through any depth and any cycle
#[test]
fn the_kept_functions_are_the_closure_of_the_calls() {
    let bytes = calling_progs();
    let progs = Progs::read(&bytes).unwrap();
    let graph = calls(&progs);
    assert_eq!(graph[b"f".as_slice()], names(&["g"]));
    assert!(graph[b"g".as_slice()].is_empty());
    let cycle: BTreeMap<Vec<u8>, BTreeSet<Vec<u8>>> = [
        (b"a".to_vec(), names(&["b"])),
        (b"b".to_vec(), names(&["c", "a"])),
        (b"c".to_vec(), names(&[])),
        (b"d".to_vec(), names(&["a"])),
    ]
    .into();
    assert_eq!(closure(&cycle, b"a"), names(&["a", "b", "c"]));
    assert_eq!(closure(&cycle, b"c"), names(&["c"]));
    assert_eq!(closure(&cycle, b"unknown"), names(&["unknown"]));
}

// spec DIFF-15.5: a statement of the whole progs is covered when the runs list its function's
// name and its offset in that body; an offset past the body or a name the progs has not adds
// nothing, and the total leaves out statement 0
#[test]
fn coverage_counts_the_listed_statements_of_each_body() {
    let bytes = calling_progs();
    let progs = Progs::read(&bytes).unwrap();
    assert_eq!(coverage(&progs, ""), (0, 3));
    assert_eq!(
        coverage(&progs, "f\t0\nf\t1\nf\t1\ng\t1\nh\t0\nbad line\n"),
        (2, 3)
    );
    assert_eq!(coverage(&progs, "f\t0\nf\t1\ng\t0\n"), (3, 3));
}

// spec DIFF-15.1: every function body at file level is found in its source file, whatever
// form defines it, and a body a macro writes and a `__wrap` body are left out
#[test]
fn the_bodies_are_found_in_their_files() {
    let dir = fresh("compiler-diff/functions-bodies");
    let source = "void() a = { local float x; x = 1; };\n\
                  float b() { return 2; }\n\
                  #define BODY { a(); }\n\
                  void() c = BODY;\n\
                  void() d = [1, d] { b(); };\n\
                  __wrap void() a = { prior(); };\n";
    fs::write(dir.join("progs.src"), "progs.dat\nt.qc\n").unwrap();
    fs::write(dir.join("t.qc"), source).unwrap();
    let (_, db) = codegen::compile_analysed(&dir, Profile::Fteqcc { autoproto: false }).unwrap();
    let (bodies, sources) = bodies(&db);
    let found: Vec<(&[u8], &str)> = bodies
        .iter()
        .map(|b| {
            let text = std::str::from_utf8(&sources[&b.path][b.start..b.end]).unwrap();
            (b.name.as_slice(), text)
        })
        .collect();
    assert_eq!(
        found,
        [
            (b"a".as_slice(), "{ local float x; x = 1; }"),
            (b"b".as_slice(), "{ return 2; }"),
            (b"d".as_slice(), "{ b(); }"),
        ]
    );
}

/// A fake pair of FTEQCCs: the fixed one always writes a progs, the pinned one only when
/// `pinned_accepts`, otherwise printing an error.
fn fteqccs(pinned_accepts: bool) -> impl Fn(&Command) -> Reply + Sync {
    move |command: &Command| {
        let dir = command.get_current_dir().unwrap().parent().unwrap();
        let fixed = command.get_program() == "fteqcc-fixed";
        if fixed || pinned_accepts {
            Reply {
                files: vec![(dir.join("progs.dat"), b"progs".to_vec())],
                ..Reply::default()
            }
        } else {
            Reply {
                code: 1,
                stdout: "t.qc:1: error: boom\ncompilation errors\n".to_owned(),
                ..Reply::default()
            }
        }
    }
}

// spec DIFF-15.3: a cut is compiled in a copy of its codebase by the fixed FTEQCC, the pinned
// FTEQCC and qc-codegen, the three progs written beside it; a cut FTEQCC refuses, or one
// qc-codegen refuses when FTEQCC compiled it, fails with what the compiler printed kept
#[test]
fn a_cut_is_compiled_by_the_three_compilers() {
    let base = fresh("compiler-diff/functions-compile");
    let source = base.join("codebase");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("progs.src"), "progs.dat\nt.qc\n").unwrap();
    fs::write(
        source.join("t.qc"),
        "void() f = { local float x; x = 1; };\n",
    )
    .unwrap();
    let cut_text: BTreeMap<PathBuf, Vec<u8>> =
        [(source.join("t.qc"), b"void() f = {};\n".to_vec())].into();
    let accepts = Fake::new(fteqccs(true));
    let oracles = fake_oracles(&base, &accepts);
    let dir = base.join("ok");
    let log = base.join("ok.log");
    assert!(matches!(
        compile_cut(&oracles, "copper", &source, &dir, &cut_text, &log).unwrap(),
        Cut::Compiled
    ));
    for name in ["fixed.dat", "theirs.dat", "ours.dat"] {
        assert!(dir.join(name).is_file(), "{name}");
    }
    assert!(
        !dir.join("src").exists(),
        "the copy is removed once compiled"
    );
    assert!(!log.exists());
    let seen = accepts.seen.lock().unwrap().join("\n");
    assert!(
        seen.starts_with("fteqcc-fixed -Tid -flno -o ../progs.dat"),
        "{seen}"
    );

    let refuses = Fake::new(fteqccs(false));
    let oracles = fake_oracles(&base, &refuses);
    let log = base.join("refused.log");
    let Cut::Failed(why) = compile_cut(
        &oracles,
        "copper",
        &source,
        &base.join("refused"),
        &cut_text,
        &log,
    )
    .unwrap() else {
        panic!("the pinned FTEQCC refused the cut");
    };
    assert!(why.contains("theirs.dat: FTEQCC rejected the cut program: t.qc:1: error: boom"));
    assert!(
        fs::read_to_string(&log)
            .unwrap()
            .contains("compilation errors")
    );

    let bad: BTreeMap<PathBuf, Vec<u8>> =
        [(source.join("t.qc"), b"void() f = { x = 1; };\n".to_vec())].into();
    let oracles = fake_oracles(&base, &accepts);
    let log = base.join("ours.log");
    let Cut::Failed(why) =
        compile_cut(&oracles, "copper", &source, &base.join("ours"), &bad, &log).unwrap()
    else {
        panic!("qc-codegen refused the cut");
    };
    assert!(
        why.starts_with("qc-codegen refused what FTEQCC compiled"),
        "{why}"
    );
    assert!(log.is_file());
}

// spec DIFF-15.6: a codebase's per-function runs are held to DIFF-12.5's 90%, crmod7 to its
// accepted 87.7%
#[test]
fn the_bar_is_ninety_percent_but_for_crmod7() {
    assert!((bar("copper") - 0.9).abs() < f64::EPSILON);
    assert!((bar("rerelease") - 0.9).abs() < f64::EPSILON);
    assert!((bar("crmod7") - 0.877).abs() < f64::EPSILON);
}
