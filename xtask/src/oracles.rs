// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask oracles`: fetches id's sources and FTEQCC's by commit and checksum, builds the
//! oracles with the system C compiler, and checks them (P1-11, P1-26, `oracles/README.md`).
//!
//! - `cargo xtask oracles` builds every oracle and checks the VM oracle against every vector in
//!   `docs/spec/vectors/vm/` that id's C defines, and its scenario reader against the number
//!   vectors of `docs/spec/vectors/scenario/numbers.txt`, and runs the input file probes of
//!   `oracles/probes.rs` and the scenario tests of `oracles/scenarios.rs`.
//!   It then compiles every builtin initialiser of `oracles/builtins.rs` with FTEQCC and id's
//!   qcc and checks the dialect profiles against them.
//! - `cargo xtask oracles builtins [--record]` builds them and runs only that check; with
//!   `--record` it writes the oracles' outcomes to `xtask/fixtures/oracles/builtin-outcomes.json`.
//! - `cargo xtask oracles build` only builds them, and prints where the binaries are.
//! - `cargo xtask oracles id101 <dir>` builds them, compiles id's QuakeC 1.01 in `<dir>` (as
//!   `cargo xtask corpus fetch id-1.01` writes it) with id's qcc, and runs the VM oracle on the
//!   result.
//! - `cargo xtask oracles rerelease <dir>` builds them and compiles the six rerelease codebases
//!   under `<dir>` with FTEQCC into `target/oracles/rerelease/`, checking each progs.dat and
//!   .lno.
//! - `cargo xtask oracles scenarios [<name>...]` builds them and runs only the scenario tests of
//!   `oracles/scenarios.rs`, or the ones named.
//! - `cargo xtask oracles fuzz <scenario | init | progs | all> [--time <seconds>]` builds the
//!   libFuzzer targets of `oracles/vm/fuzz.c` and runs them (`oracles/fuzz.rs`).
//! - `cargo xtask oracles json <trace>` prints a binary trace in the JSON form (TRACE-6).
//!
//! Everything lives under `target/oracles/`: `src/` holds the fetched files, `build/` the
//! objects and the generated files that hold id's extracted functions, `bin/` the binaries
//! (`qcc`, `vm-oracle` and `fteqcc`), `rerelease/` the progs FTEQCC built. A stamp of every input
//! lets an unchanged build, such as one restored by CI's cache, skip the compile. Nothing here
//! is part of the shipped app.

mod builtins;
mod codegen;
mod compile;
mod extract;
mod fuzz;
pub(crate) mod id101;
mod numbers;
mod probes;
pub(crate) mod rerelease;
mod scenarios;
pub(crate) mod sha256;
#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
mod vectors;

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::download::{CURL, Net};
use crate::exec::{Exec, System};
use compile::{Code, Compiler, Instrument, Unit};

const SOURCES: &str = "oracles/sources.toml";
const OUT: &str = "target/oracles";

/// When this environment variable is set and not empty, our harness files are built with
/// clang's source-based coverage (`compile::COVERAGE_FLAGS`), and every run of the VM oracle
/// writes a profile where `LLVM_PROFILE_FILE` says. `cargo xtask readiness` sets both to
/// measure the coverage of our C (`docs/testing.md`, TEST-1).
pub(crate) const COVERAGE_ENV: &str = "QS_ORACLE_COVERAGE";

fn coverage_requested() -> bool {
    std::env::var_os(COVERAGE_ENV).is_some_and(|value| !value.is_empty())
}

/// Our harness files, compiled with every warning an error.
const VM_HARNESS: [&str; 5] = [
    "oracles/vm/host.c",
    "oracles/vm/trace.c",
    "oracles/vm/engine.c",
    "oracles/vm/builtins.c",
    "oracles/vm/scenario.c",
];

/// The files of id's qcc, compiled unchanged.
const QCC_FILES: [&str; 4] = ["qcc/qcc.c", "qcc/pr_lex.c", "qcc/pr_comp.c", "qcc/cmdlib.c"];

/// The files of FTEQCC's command-line compiler, compiled unchanged: the `qcc` target of
/// `engine/qclib/Makefile` (`QCC_OBJS`, `COMMON_OBJS` and `TUI_OBJS`).
const FTEQCC_FILES: [&str; 10] = [
    "engine/qclib/qccmain.c",
    "engine/qclib/qcc_pr_comp.c",
    "engine/qclib/qcc_pr_lex.c",
    "engine/qclib/packager.c",
    "engine/qclib/decomp.c",
    "engine/qclib/comprout.c",
    "engine/qclib/hash.c",
    "engine/qclib/qcc_cmdlib.c",
    "engine/qclib/qcd_main.c",
    "engine/qclib/qcctui.c",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Sources {
    upstream: Vec<Upstream>,
    extract: Vec<Extract>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Upstream {
    id: String,
    repo: String,
    commit: String,
    files: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Extract {
    file: String,
    functions: Vec<String>,
}

/// Where the built oracles are.
pub(crate) struct Oracles<'a> {
    pub(crate) qcc: PathBuf,
    pub(crate) vm: PathBuf,
    pub(crate) fteqcc: PathBuf,
    pub(crate) work: PathBuf,
    pub(crate) rerelease: PathBuf,
    /// What runs them: the system, or a test's fake.
    pub(crate) exec: &'a dyn Exec,
}

/// Runs `cargo xtask oracles [build | id101 <dir> | rerelease <dir> | json <trace>]`.
pub(crate) fn main(root: &Path, args: &[String]) -> Result<bool, String> {
    run(root, args, &System, &CURL)
}

/// [`main`] with the programs `exec` runs and the files `net` fetches.
fn run(root: &Path, args: &[String], exec: &dyn Exec, net: &dyn Net) -> Result<bool, String> {
    let build = || build_with(root, exec, net);
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] => {
            let oracles = build()?;
            let vectors = vectors::check(root, &oracles)?;
            let numbers = numbers::check(root, &oracles)?;
            let probes = probes::check(&oracles)?;
            let scenarios = scenarios::check(&oracles, None)?;
            let builtins = builtins::check(root, &oracles, false)?;
            let codegen = codegen::check(root, &oracles, false)?;
            Ok([vectors, numbers, probes, scenarios, builtins, codegen]
                .iter()
                .all(|passed| *passed))
        }
        ["builtins"] => builtins::check(root, &build()?, false),
        ["builtins", "--record"] => builtins::check(root, &build()?, true),
        ["codegen"] => codegen::check(root, &build()?, false),
        ["codegen", "--record"] => codegen::check(root, &build()?, true),
        ["fuzz", rest @ ..] => fuzz::run(root, rest, exec, net),
        ["scenarios", names @ ..] => {
            let oracles = build()?;
            scenarios::check(&oracles, Some(names))
        }
        ["build"] => {
            let oracles = build()?;
            println!("oracles: qcc at {}", oracles.qcc.display());
            println!("oracles: vm-oracle at {}", oracles.vm.display());
            println!("oracles: fteqcc at {}", oracles.fteqcc.display());
            Ok(true)
        }
        ["id101", dir] => {
            let oracles = build()?;
            id101::check(&oracles, Path::new(dir))
        }
        ["rerelease", dir] => {
            let oracles = build()?;
            rerelease::check(&oracles, Path::new(dir))
        }
        ["json", trace] => {
            let bytes = fs::read(trace).map_err(|e| format!("{trace}: {e}"))?;
            let trace = qc_trace::decode(&bytes).map_err(|e| format!("{trace}: {e:?}"))?;
            print!("{}", qc_trace::to_json(&trace));
            Ok(true)
        }
        _ => Err(
            "usage: cargo xtask oracles [build | scenarios [<name>...] | fuzz <target> [--time <s>] \
             | builtins [--record] | codegen [--record] | id101 <codebase directory> \
             | rerelease <directory of codebases> | json <trace>]"
                .to_owned(),
        ),
    }
}

pub(crate) fn read_sources(root: &Path) -> Result<Sources, String> {
    let path = root.join(SOURCES);
    let text = fs::read_to_string(&path).map_err(|e| format!("{SOURCES}: {e}"))?;
    let sources: Sources = toml::from_str(&text).map_err(|e| format!("{SOURCES}: {e}"))?;
    for upstream in &sources.upstream {
        let commit_ok =
            upstream.commit.len() == 40 && upstream.commit.bytes().all(|b| b.is_ascii_hexdigit());
        if !commit_ok {
            return Err(format!(
                "{SOURCES}: {} is not pinned to a full commit",
                upstream.id
            ));
        }
        if upstream.files.keys().any(|f| f.ends_with("quakedef.h")) {
            return Err(format!(
                "{SOURCES}: id's quakedef.h must not be fetched; oracles/vm/quakedef.h stands in"
            ));
        }
    }
    Ok(sources)
}

/// Downloads every pinned file that is missing from the cache, and checks every file's SHA-256.
pub(crate) fn fetch(sources: &Sources, cache: &Path, net: &dyn Net) -> Result<(), String> {
    for upstream in &sources.upstream {
        for (file, want) in &upstream.files {
            let path = cache.join(&upstream.id).join(file);
            if fs::read(&path).is_ok_and(|bytes| sha256::hex_digest(&bytes) == *want) {
                continue;
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            let url = format!(
                "https://raw.githubusercontent.com/{}/{}/{file}",
                upstream.repo, upstream.commit
            );
            let partial = path.with_extension("partial");
            net.download(&url, &partial)?;
            let bytes = fs::read(&partial).map_err(|e| format!("{}: {e}", partial.display()))?;
            let got = sha256::hex_digest(&bytes);
            if got != *want {
                return Err(format!(
                    "{}:{file}: sha256 {got}, {SOURCES} says {want}",
                    upstream.repo
                ));
            }
            fs::rename(&partial, &path).map_err(|e| format!("{}: {e}", path.display()))?;
            println!(
                "oracles: fetched {}@{}:{file}",
                upstream.repo, upstream.commit
            );
        }
    }
    Ok(())
}

/// Writes one generated file per `[[extract]]` entry: the named functions, copied unchanged.
fn generate(sources: &Sources, cache: &Path, out: &Path) -> Result<Vec<PathBuf>, String> {
    let mut generated = Vec::new();
    for entry in &sources.extract {
        let upstream = sources
            .upstream
            .iter()
            .find(|u| u.files.contains_key(&entry.file))
            .ok_or_else(|| format!("{SOURCES}: {} is not a fetched file", entry.file))?;
        let path = cache.join(&upstream.id).join(&entry.file);
        let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let text = String::from_utf8_lossy(&bytes);
        let stem = Path::new(&entry.file)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("extract");
        // `QS_EXTRACTED_<FILE>` lets quakedef.h redirect an identifier in one file's functions
        // only (`PF_makestatic`'s call of `SV_ModelIndex`, oracles/README.md).
        let mut c = format!(
            "/* Generated by cargo xtask oracles: functions copied unchanged from\n   \
             {}@{}: {}.\n   Not part of the repository. */\n#define QS_EXTRACTED_{}\n\
             #include \"quakedef.h\"\n",
            upstream.repo,
            upstream.commit,
            entry.file,
            stem.to_ascii_uppercase()
        );
        for name in &entry.functions {
            let function =
                extract::function(&text, name).map_err(|e| format!("{}: {e}", entry.file))?;
            c.push_str(&format!(
                "#line {} \"{}\"\n{}",
                function.line, entry.file, function.text
            ));
        }
        let file = out.join(format!("id_{stem}.c"));
        fs::write(&file, c).map_err(|e| format!("{}: {e}", file.display()))?;
        generated.push(file);
    }
    Ok(generated)
}

/// The stamp of every input to the build: the pinned sources, our C files and the compiler.
fn stamp(root: &Path, compiler: &Compiler) -> Result<String, String> {
    // The xtask code that extracts id's functions and drives the compiler is an input too.
    let mut inputs = vec![
        compiler.describe(),
        sha256::hex_digest(include_str!("oracles.rs").as_bytes()),
        sha256::hex_digest(include_str!("oracles/extract.rs").as_bytes()),
        sha256::hex_digest(include_str!("oracles/compile.rs").as_bytes()),
    ];
    let mut files = Vec::new();
    collect_files(&root.join("oracles"), &mut files)?;
    files.sort();
    for file in files {
        let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let name = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .display()
            .to_string();
        inputs.push(format!("{name} {}", sha256::hex_digest(&bytes)));
    }
    Ok(sha256::hex_digest(inputs.join("\n").as_bytes()))
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

/// Where the oracles are built.
fn locate<'a>(root: &Path, exec: &'a dyn Exec) -> Oracles<'a> {
    let out = root.join(OUT);
    let bin = out.join("bin");
    Oracles {
        qcc: bin.join(Compiler::executable("qcc")),
        vm: bin.join(Compiler::executable("vm-oracle")),
        fteqcc: bin.join(Compiler::executable("fteqcc")),
        work: out.join("work"),
        rerelease: out.join("rerelease"),
        exec,
    }
}

/// The VM oracle's executable, once built.
pub(crate) fn vm_oracle(root: &Path) -> PathBuf {
    locate(root, &System).vm
}

/// Fetches, generates and compiles every oracle, unless the stamp shows they are current.
pub(crate) fn build(root: &Path) -> Result<Oracles<'static>, String> {
    build_with(root, &System, &CURL)
}

/// [`build`] with the programs `exec` runs and the files `net` fetches.
fn build_with<'a>(root: &Path, exec: &'a dyn Exec, net: &dyn Net) -> Result<Oracles<'a>, String> {
    let sources = read_sources(root)?;
    let out = root.join(OUT);
    let cache = out.join("src");
    let objects = out.join("build");
    let bin = out.join("bin");
    let oracles = locate(root, exec);
    for dir in [&cache, &objects, &bin, &oracles.work] {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let instrument = if coverage_requested() {
        Instrument::Coverage
    } else {
        Instrument::None
    };
    let compiler = Compiler::find(&objects, instrument, exec)?;
    let stamp_now = stamp(root, &compiler)?;
    let stamp_file = bin.join("stamp");
    if oracles.qcc.is_file()
        && oracles.vm.is_file()
        && oracles.fteqcc.is_file()
        && fs::read_to_string(&stamp_file).is_ok_and(|s| s.trim() == stamp_now)
    {
        println!("oracles: up to date in {}", bin.display());
        return Ok(oracles);
    }
    fetch(&sources, &cache, net)?;
    let tools = cache.join("quake-tools");

    // id's qcc, unchanged.
    let qcc_compat = root.join("oracles/qcc/compat.h");
    let mut qcc_includes = Vec::new();
    if cfg!(windows) {
        qcc_includes.push(root.join("oracles/qcc/win32"));
    }
    let mut qcc_objects = Vec::new();
    for file in QCC_FILES {
        let source = tools.join(file);
        let object = objects.join(format!("qcc_{}.o", stem(&source)));
        compiler.object(
            &Unit {
                source: &source,
                code: Code::Upstream,
                includes: &qcc_includes,
                defines: &[],
                forced_include: Some(&qcc_compat),
            },
            &object,
        )?;
        qcc_objects.push(object);
    }
    compiler.link(&qcc_objects, &oracles.qcc)?;

    // FTEQCC, unchanged, without zlib: it then cannot read or write compressed archives, which
    // compiling a progs.src never needs. `qcc_pr_comp.c` calls the POSIX `strcasecmp`, which
    // the Windows C runtime names `_stricmp`.
    let fteqcc_dir = cache.join("fteqcc");
    let fteqcc_defines: &[&str] = if cfg!(windows) {
        &["NO_ZLIB", "strcasecmp=_stricmp"]
    } else {
        &["NO_ZLIB"]
    };
    let mut fteqcc_objects = Vec::new();
    for file in FTEQCC_FILES {
        let source = fteqcc_dir.join(file);
        let object = objects.join(format!("fteqcc_{}.o", stem(&source)));
        compiler.object(
            &Unit {
                source: &source,
                code: Code::Upstream,
                includes: &[],
                defines: fteqcc_defines,
                forced_include: None,
            },
            &object,
        )?;
        fteqcc_objects.push(object);
    }
    compiler.link(&fteqcc_objects, &oracles.fteqcc)?;

    // The VM oracle: pr_exec.c whole, id's extracted functions, and our harness.
    let vm_objects = compile_vm(root, &sources, &cache, &objects, &compiler, &[])?;
    compiler.link(&vm_objects, &oracles.vm)?;
    fs::write(&stamp_file, format!("{stamp_now}\n"))
        .map_err(|e| format!("{}: {e}", stamp_file.display()))?;
    println!(
        "oracles: built qcc, vm-oracle and fteqcc in {}",
        bin.display()
    );
    Ok(oracles)
}

/// Compiles the VM oracle's objects into `objects`: `pr_exec.c` whole, id's extracted
/// functions, our harness, and `extra`, each of our files with the defines given; returns the
/// objects to link. `cargo xtask oracles fuzz` adds a fuzz target as `extra`.
pub(crate) fn compile_vm(
    root: &Path,
    sources: &Sources,
    cache: &Path,
    objects: &Path,
    compiler: &Compiler,
    extra: &[(PathBuf, &[&str])],
) -> Result<Vec<PathBuf>, String> {
    let quake = cache.join("quake").join("WinQuake");
    let includes = vec![root.join("oracles/vm"), quake.clone()];
    let mut vm_objects = Vec::new();
    let pr_exec = quake.join("pr_exec.c");
    let object = objects.join("vm_pr_exec.o");
    compiler.object(
        &Unit {
            source: &pr_exec,
            code: Code::Upstream,
            includes: &includes,
            defines: &["QS_PR_EXEC"],
            forced_include: None,
        },
        &object,
    )?;
    vm_objects.push(object);
    for source in generate(sources, cache, objects)? {
        let object = objects.join(format!("vm_{}.o", stem(&source)));
        compiler.object(
            &Unit {
                source: &source,
                code: Code::Upstream,
                includes: &includes,
                defines: &["QS_ENGINE"],
                forced_include: None,
            },
            &object,
        )?;
        vm_objects.push(object);
    }
    let harness = VM_HARNESS
        .iter()
        .map(|file| (root.join(file), &["QS_ENGINE"][..]));
    let extra = extra.iter().map(|(file, defines)| (file.clone(), *defines));
    for (source, defines) in harness.chain(extra) {
        let object = objects.join(format!("vm_{}.o", stem(&source)));
        compiler.object(
            &Unit {
                source: &source,
                code: Code::Ours,
                includes: &includes,
                defines,
                forced_include: None,
            },
            &object,
        )?;
        vm_objects.push(object);
    }
    Ok(vm_objects)
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_owned()
}
