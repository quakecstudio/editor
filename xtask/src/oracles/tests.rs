// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or sum is a failed test"
)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::testing::{fresh_dir, root, sample_suite};
use super::*;
use crate::exec::{Fake, Reply};
use crate::parse_cli;

/// What a fake source file of id's holds.
const PR_EDICT: &str = "/* a comment */\nedict_t *ED_Alloc (void)\n{\n\treturn 0;\n}\n";
const PR_EXEC: &str = "void PR_ExecuteProgram (func_t fnum)\n{\n}\n";

/// A [`Net`] that serves fixed files and records what was asked for.
#[derive(Default)]
struct Files {
    by_url: BTreeMap<String, Vec<u8>>,
    asked: RefCell<Vec<String>>,
}

impl Net for Files {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        self.asked.borrow_mut().push(url.to_owned());
        self.by_url
            .get(url)
            .cloned()
            .ok_or_else(|| format!("{url}: 404"))
    }

    fn download(&self, url: &str, out: &Path) -> Result<(), String> {
        let bytes = self.get(url)?;
        fs::write(out, bytes).map_err(|e| e.to_string())
    }
}

const COMMIT: &str = "bf4ac424ce754894ac8f1dae6a3981954bc9852d";

fn url(file: &str) -> String {
    format!("https://raw.githubusercontent.com/id-Software/Quake/{COMMIT}/{file}")
}

/// A repository holding a `sources.toml` of two of id's files, one function extracted from
/// one, and the vector files `cargo xtask oracles` reads; and the files served for it.
fn fake_root(name: &str) -> (PathBuf, Files) {
    let dir = fresh_dir(name);
    fs::create_dir_all(dir.join("oracles/vm")).unwrap();
    let sources = format!(
        "[[upstream]]\nid = \"quake\"\nrepo = \"id-Software/Quake\"\ncommit = \"{COMMIT}\"\n\n\
         [upstream.files]\n\"WinQuake/pr_exec.c\" = \"{}\"\n\"WinQuake/pr_edict.c\" = \"{}\"\n\n\
         [[extract]]\nfile = \"WinQuake/pr_edict.c\"\nfunctions = [\"ED_Alloc\"]\n",
        sha256::hex_digest(PR_EXEC.as_bytes()),
        sha256::hex_digest(PR_EDICT.as_bytes()),
    );
    fs::write(dir.join(SOURCES), sources).unwrap();
    // DIFF-14.12: the fixed FTEQCC is built from a copy of the fetched tree, so the fake tree
    // holds a `qcc_pr_comp.c` with the text of every fix.
    let fteqcc = dir.join(OUT).join("src/fteqcc").join(super::defects::FILE);
    fs::create_dir_all(fteqcc.parent().unwrap()).unwrap();
    let pinned: String = super::defects::FIXES
        .iter()
        .map(|fix| fix.find.repeat(fix.times))
        .collect();
    fs::write(&fteqcc, pinned).unwrap();
    fs::write(
        dir.join("oracles/vm/host.c"),
        "int main(void) { return 0; }\n",
    )
    .unwrap();
    for spec in ["vm", "scenario"] {
        let from = root().join("docs/spec/vectors").join(spec);
        let to = dir.join("docs/spec/vectors").join(spec);
        fs::create_dir_all(&to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let path = entry.unwrap().path();
            fs::copy(&path, to.join(path.file_name().unwrap())).unwrap();
        }
    }
    let mut files = Files::default();
    files
        .by_url
        .insert(url("WinQuake/pr_exec.c"), PR_EXEC.as_bytes().to_vec());
    files
        .by_url
        .insert(url("WinQuake/pr_edict.c"), PR_EDICT.as_bytes().to_vec());
    (dir, files)
}

/// The output path a compile or link names: after `-o`, or in `/Fo` or `/Fe`.
fn output_of(command: &Command) -> Option<PathBuf> {
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    for (index, arg) in args.iter().enumerate() {
        if arg == "-o" {
            return args.get(index + 1).map(PathBuf::from);
        }
        if let Some(path) = arg.strip_prefix("/Fe").or_else(|| arg.strip_prefix("/Fo")) {
            return Some(PathBuf::from(path));
        }
    }
    None
}

/// A fake compiler that writes each output it is asked for, and a fake of every other
/// program that exits with `code`.
fn tools(code: i32) -> impl Fn(&Command) -> Reply {
    move |command: &Command| match output_of(command) {
        Some(path) => Reply {
            files: vec![(path, Vec::new())],
            ..Reply::default()
        },
        None => Reply::exit(code),
    }
}

#[test]
fn the_oracles_are_built_once_from_the_files_fetched() {
    let (dir, files) = fake_root("oracles-build");
    let fake = Fake::new(tools(0));
    let oracles = build_with(&dir, &fake, &files).unwrap();
    assert!(oracles.vm.ends_with(Compiler::executable("vm-oracle")));
    assert_eq!(files.asked.borrow().len(), 2);
    let generated = fs::read_to_string(dir.join(OUT).join("build/id_pr_edict.c")).unwrap();
    assert!(
        generated.contains("#define QS_EXTRACTED_PR_EDICT"),
        "{generated}"
    );
    assert!(
        generated.contains("edict_t *ED_Alloc (void)\n{\n\treturn 0;\n}\n"),
        "{generated}"
    );
    assert!(dir.join(OUT).join("bin/stamp").is_file());
    let compiles = fake.seen.lock().unwrap().len();
    // qcc, FTEQCC, the VM oracle, the engine oracle, the view oracle and the fixed FTEQCC
    // (DIFF-14.12): each source, then each link.
    let sources = QCC_FILES.len()
        + 2 * FTEQCC_FILES.len()
        + 2
        + VM_HARNESS.len()
        + ENGINE_FILES.len()
        + ENGINE_HARNESS.len()
        + if cfg!(windows) {
            0
        } else {
            VIEW_FILES.len() + VIEW_HARNESS.len() + 1
        };
    assert_eq!(compiles, sources + 5);
    assert!(
        oracles
            .engine
            .ends_with(Compiler::executable("engine-oracle"))
    );
    let fixed =
        fs::read_to_string(dir.join(OUT).join("src/fteqcc-fixed").join(defects::FILE)).unwrap();
    assert!(defects::FIXES.iter().all(|fix| fixed.contains(fix.replace)));
    // Built again with nothing changed: the stamp says the binaries are current.
    build_with(&dir, &fake, &files).unwrap();
    assert_eq!(fake.seen.lock().unwrap().len(), compiles);
    assert_eq!(files.asked.borrow().len(), 2, "the fetched files are kept");
    // A change to one of the oracles' own files rebuilds them.
    fs::write(
        dir.join("oracles/vm/host.c"),
        "int main(void) { return 1; }\n",
    )
    .unwrap();
    build_with(&dir, &fake, &files).unwrap();
    assert_eq!(fake.seen.lock().unwrap().len(), compiles * 2);
}

#[test]
fn a_file_whose_digest_differs_is_refused() {
    let (dir, mut files) = fake_root("oracles-digest");
    files
        .by_url
        .insert(url("WinQuake/pr_exec.c"), b"changed".to_vec());
    let fake = Fake::new(tools(0));
    let error = build_with(&dir, &fake, &files).err().unwrap();
    assert!(error.contains("sha256"), "{error}");
    assert!(!dir.join(OUT).join("src/quake/WinQuake/pr_exec.c").exists());
    files.by_url.clear();
    assert!(
        build_with(&dir, &fake, &files)
            .err()
            .unwrap()
            .contains("404")
    );
}

#[test]
fn a_build_that_fails_to_compile_is_an_error() {
    let (dir, files) = fake_root("oracles-compile-fails");
    let fake = Fake::new(|_: &Command| Reply::exit(1));
    let error = build_with(&dir, &fake, &files).err().unwrap();
    assert!(error.contains("failed"), "{error}");
}

#[test]
fn sources_must_be_pinned_and_never_fetch_quakedef() {
    let (dir, _) = fake_root("oracles-sources");
    let path = dir.join(SOURCES);
    let text = fs::read_to_string(&path).unwrap();
    assert!(read_sources(&dir).is_ok());
    fs::write(&path, text.replace(COMMIT, "main")).unwrap();
    assert!(read_sources(&dir).err().unwrap().contains("full commit"));
    fs::write(
        &path,
        text.replace("WinQuake/pr_exec.c", "WinQuake/quakedef.h"),
    )
    .unwrap();
    assert!(read_sources(&dir).err().unwrap().contains("quakedef.h"));
    fs::write(&path, "[[upstream]]\n").unwrap();
    assert!(read_sources(&dir).is_err());
    fs::remove_file(&path).unwrap();
    assert!(read_sources(&dir).is_err());
}

#[test]
fn an_extract_from_a_file_not_fetched_is_an_error() {
    let (dir, files) = fake_root("oracles-extract");
    let path = dir.join(SOURCES);
    let text = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        text.replace(
            "file = \"WinQuake/pr_edict.c\"",
            "file = \"WinQuake/sv_phys.c\"",
        ),
    )
    .unwrap();
    let fake = Fake::new(tools(0));
    let error = build_with(&dir, &fake, &files).err().unwrap();
    assert!(error.contains("is not a fetched file"), "{error}");
    fs::write(
        &path,
        text.replace("functions = [\"ED_Alloc\"]", "functions = [\"ED_Free\"]"),
    )
    .unwrap();
    assert!(build_with(&dir, &fake, &files).is_err());
}

/// `cargo xtask oracles` with `text`.
fn cli(text: &str) -> Result<Args, clap::Error> {
    parse_cli(&text.split_whitespace().collect::<Vec<_>>())
}

#[test]
fn every_command_of_cargo_xtask_oracles_runs() {
    let (dir, files) = fake_root("oracles-commands");
    let fake = Fake::new(tools(0));
    // A dozen cases stand for the cross product: the commands are what is tested here.
    let suite = || sample_suite().0;
    assert!(run(&dir, &cli("build").unwrap(), &fake, &files, &suite).unwrap());
    // The fake VM oracle writes no trace, so every check that reads one fails.
    assert!(!run(&dir, &cli("").unwrap(), &fake, &files, &suite).unwrap());
    // The recorded mode runs the VM checks, then reads the fixture, which this root has none of.
    let recorded = run(&dir, &cli("--recorded").unwrap(), &fake, &files, &suite).unwrap_err();
    assert!(recorded.contains("builtin-outcomes.json"), "{recorded}");
    assert!(
        !run(
            &dir,
            &cli("scenarios defaults").unwrap(),
            &fake,
            &files,
            &suite
        )
        .unwrap()
    );
    // The fake compilers write no progs.dat, so the builtin differential fails too.
    assert!(!run(&dir, &cli("builtins").unwrap(), &fake, &files, &suite).unwrap());
    // spec FTEF-14.50: where `unsigned long` is 32 bits the fixture is not recorded at all.
    let recording = run(
        &dir,
        &cli("builtins --record").unwrap(),
        &fake,
        &files,
        &suite,
    );
    if super::builtins::ULong::HOST == super::builtins::ULong::Bits64 {
        assert!(!recording.unwrap());
    } else {
        assert!(recording.is_err());
    }
    assert!(cli("builtins again").is_err());
    // The fake qcc writes no progs.dat and the fake root records no codegen outcomes.
    assert!(!run(&dir, &cli("codegen").unwrap(), &fake, &files, &suite).unwrap());
    assert!(
        !run(
            &dir,
            &cli("codegen --record").unwrap(),
            &fake,
            &files,
            &suite
        )
        .unwrap()
    );
    assert!(cli("codegen again").is_err());
    // spec DIFF-13.4: the grammar differential reads its fixture, which this root has none of.
    let recorded = run(
        &dir,
        &cli("grammar --recorded").unwrap(),
        &fake,
        &files,
        &suite,
    )
    .unwrap_err();
    assert!(recorded.contains("grammar-outcomes.json"), "{recorded}");
    assert!(!run(&dir, &cli("grammar").unwrap(), &fake, &files, &suite).unwrap());
    assert!(cli("grammar again").is_err());
    assert!(run(&dir, &cli("id101 nowhere").unwrap(), &fake, &files, &suite).is_err());
    let codebases = fresh_dir("oracles-commands-codebases");
    assert!(cli("rerelease").is_err());
    assert!(
        run(
            &dir,
            &cli(&format!("rerelease {}", codebases.display())).unwrap(),
            &fake,
            &files,
            &suite
        )
        .is_err()
    );
    assert!(cli("unknown").is_err());
    assert!(
        cli("--recorded build").is_err(),
        "--recorded is the whole run's"
    );
    assert!(cli("fuzz").is_err());
}

#[test]
fn a_trace_prints_as_json() {
    let suite = || sample_suite().0;
    let dir = fresh_dir("oracles-json");
    let trace = dir.join("t.trace");
    let bytes = qc_trace::encode(&qc_trace::Trace {
        header: qc_trace::Header::new(1, 548, 108, 110, 1).unwrap(),
        records: Vec::new(),
    });
    fs::write(&trace, bytes).unwrap();
    let fake = Fake::new(tools(0));
    let files = Files::default();
    let args = parse_cli::<Args, _>(&["json".to_owned(), trace.display().to_string()]).unwrap();
    assert!(run(&dir, &args, &fake, &files, &suite).unwrap());
    fs::write(&trace, b"not a trace").unwrap();
    assert!(run(&dir, &args, &fake, &files, &suite).is_err());
    fs::remove_file(&trace).unwrap();
    assert!(run(&dir, &args, &fake, &files, &suite).is_err());
}

#[test]
fn the_fuzz_targets_build_seed_and_run() {
    let suite = || sample_suite().0;
    let (dir, files) = fake_root("oracles-fuzz");
    let fake = Fake::new(tools(0));
    // Where the C compiler is not clang (gcc on Linux, MSVC on Windows, unless CC says
    // otherwise), fuzzing stops before building anything.
    if Compiler::find(&dir, Instrument::Fuzz, &fake).is_err() {
        let error = run(
            &dir,
            &cli("fuzz scenario --time 5").unwrap(),
            &fake,
            &files,
            &suite,
        )
        .unwrap_err();
        assert!(error.contains("need clang"), "{error}");
        return;
    }
    assert!(
        run(
            &dir,
            &cli("fuzz scenario --time 5").unwrap(),
            &fake,
            &files,
            &suite
        )
        .unwrap()
    );
    let corpus = dir.join(OUT).join("fuzz/scenario/corpus");
    assert!(fs::read_dir(&corpus).unwrap().count() > 50);
    let seen = fake.seen.lock().unwrap().clone();
    let fuzzing = seen.last().unwrap();
    assert!(fuzzing.contains("-max_total_time=5"), "{fuzzing}");
    assert!(seen.iter().any(|line| line.contains("-DQS_FUZZ_SCENARIO")));
    let crashes = Fake::new(|command: &Command| match output_of(command) {
        Some(path) => Reply {
            files: vec![(path, Vec::new())],
            ..Reply::default()
        },
        None => Reply::exit(77),
    });
    assert!(
        !run(
            &dir,
            &cli("fuzz init --time 5").unwrap(),
            &crashes,
            &files,
            &suite
        )
        .unwrap()
    );
    assert!(dir.join(OUT).join("fuzz/init/fuzz.log").is_file());
}
