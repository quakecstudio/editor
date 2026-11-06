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
//! - `cargo xtask oracles --recorded` does the same, but checks the profiles and the project
//!   graph against the builtin outcomes the fixture records instead of running FTEQCC and qcc on
//!   them: seconds instead of half an hour. Pull requests run it unless they change the oracles
//!   or the differential (docs/ci.md); the nightly Differential workflow runs the live check.
//! - `cargo xtask oracles builtins --recorded` checks the profile against the recorded outcomes
//!   alone, with no oracle built or run.
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
pub(crate) mod codegen;
mod compile;
mod defects;
mod engine;
mod extract;
mod fuzz;
mod grammar;
pub(crate) mod id101;
mod numbers;
mod probes;
pub(crate) mod rerelease;
pub(crate) mod rng;
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
/// The upstream of id's files the VM oracle compiles (`oracles/sources.toml`).
const VM_UPSTREAM: &str = "quake";
/// The upstream of id's files the engine oracle compiles, quakedef.h among them.
const ENGINE_UPSTREAM: &str = "quake-engine";
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
    "oracles/trace/trace.c",
    "oracles/vm/engine.c",
    "oracles/vm/builtins.c",
    "oracles/vm/scenario.c",
];

/// The engine oracle's own files (`oracles/engine/`), compiled with every warning an error;
/// it shares the trace writer.
const ENGINE_HARNESS: [&str; 3] = [
    "oracles/engine/engine.c",
    "oracles/engine/rand.c",
    "oracles/trace/trace.c",
];

/// The view oracle's own files (`oracles/engine/`), compiled with every warning an error: the
/// system, video and sound card that id's whole engine runs on headless (P3-28).
const VIEW_HARNESS: [&str; 2] = ["oracles/engine/view_host.c", "oracles/engine/rand.c"];

/// The view oracle's files of id's client that call `rand`, given a generator of their own so
/// that the server's draws are the server oracle's (VIEWDIFF-2.3).
const VIEW_CLIENT_FILES: [&str; 6] = [
    "cl_main.c",
    "cl_parse.c",
    "cl_tent.c",
    "menu.c",
    "r_part.c",
    "snd_dma.c",
];

/// id's files the view oracle compiles whole, with id's own headers: the client, the software
/// renderer with the C stand-ins for its assembly (`nonintel.c`), the mixer, the server, and
/// the null CD and input drivers and loopback network a headless engine needs.
const VIEW_FILES: [&str; 67] = [
    "cd_null.c",
    "chase.c",
    "cl_demo.c",
    "cl_input.c",
    "cl_main.c",
    "cl_parse.c",
    "cl_tent.c",
    "cmd.c",
    "common.c",
    "console.c",
    "crc.c",
    "cvar.c",
    "d_edge.c",
    "d_fill.c",
    "d_init.c",
    "d_modech.c",
    "d_part.c",
    "d_polyse.c",
    "d_scan.c",
    "d_sky.c",
    "d_sprite.c",
    "d_surf.c",
    "d_vars.c",
    "d_zpoint.c",
    "draw.c",
    "host.c",
    "host_cmd.c",
    "in_null.c",
    "keys.c",
    "mathlib.c",
    "menu.c",
    "model.c",
    "net_loop.c",
    "net_main.c",
    "net_none.c",
    "net_vcr.c",
    "nonintel.c",
    "pr_cmds.c",
    "pr_edict.c",
    "pr_exec.c",
    "r_aclip.c",
    "r_alias.c",
    "r_bsp.c",
    "r_draw.c",
    "r_edge.c",
    "r_efrag.c",
    "r_light.c",
    "r_main.c",
    "r_misc.c",
    "r_part.c",
    "r_sky.c",
    "r_sprite.c",
    "r_surf.c",
    "r_vars.c",
    "sbar.c",
    "screen.c",
    "snd_dma.c",
    "snd_mem.c",
    "snd_mix.c",
    "sv_main.c",
    "sv_move.c",
    "sv_phys.c",
    "sv_user.c",
    "view.c",
    "wad.c",
    "world.c",
    "zone.c",
];

/// id's server files the engine oracle compiles whole, with id's own headers.
const ENGINE_FILES: [&str; 16] = [
    "sv_main.c",
    "sv_phys.c",
    "sv_move.c",
    "sv_user.c",
    "world.c",
    "model.c",
    "pr_edict.c",
    "pr_exec.c",
    "pr_cmds.c",
    "common.c",
    "cvar.c",
    "cmd.c",
    "zone.c",
    "mathlib.c",
    "crc.c",
    "host_cmd.c",
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
    /// The oracle the functions are built into.
    #[serde(default)]
    oracle: Target,
    file: String,
    functions: Vec<String>,
}

/// Which oracle an `[[extract]]` entry feeds.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Target {
    /// The VM oracle, over `oracles/vm/quakedef.h`.
    #[default]
    Vm,
    /// The engine oracle, over id's `quakedef.h` and `oracles/engine/extracted.h`.
    Engine,
}

/// Where the built oracles are.
pub(crate) struct Oracles<'a> {
    pub(crate) qcc: PathBuf,
    pub(crate) vm: PathBuf,
    /// id's server, headless, driven by a script (`oracles/engine/`, TRACE-4.11).
    pub(crate) engine: PathBuf,
    /// id's whole engine, headless, writing screenshots and its mixed sound (P3-28).
    pub(crate) view: PathBuf,
    pub(crate) fteqcc: PathBuf,
    /// FTEQCC with the defects of [`defects::FIXES`] fixed, which proves a divergence an
    /// FTEQCC defect (DIFF-14.12).
    pub(crate) fteqcc_fixed: PathBuf,
    pub(crate) work: PathBuf,
    pub(crate) rerelease: PathBuf,
    /// What runs them: the system, or a test's fake.
    pub(crate) exec: &'a dyn Exec,
}

/// `cargo xtask oracles [--recorded | build | scenarios [<name>...] | fuzz <target>...
/// [--time <s>] | builtins [--record | --recorded] | codegen [--record] | grammar [--record |
/// --recorded] | id101 <codebase directory> | rerelease <directory of codebases> | engine
/// <script> <trace> | json <trace>]`.
#[derive(clap::Parser, Debug)]
#[command(args_conflicts_with_subcommands = true)]
pub(crate) struct Args {
    /// The builtin and grammar differentials against their recorded outcomes, and the
    /// compiler differential left to `cargo test`: what pull requests run (docs/ci.md).
    #[arg(long)]
    recorded: bool,
    #[command(subcommand)]
    command: Option<OracleCommand>,
}

#[derive(clap::Subcommand, Debug)]
enum OracleCommand {
    /// The builtin differential.
    Builtins(Recording),
    /// The compiler differential.
    Codegen {
        /// Record the oracles' outcomes.
        #[arg(long)]
        record: bool,
    },
    /// The grammar differential.
    Grammar(Recording),
    /// Fuzz the VM oracle's targets.
    Fuzz(fuzz::Args),
    /// The VM vectors' scenarios, every one when none is named.
    Scenarios {
        /// The scenarios.
        names: Vec<String>,
    },
    /// Build the oracles and print where they are.
    Build,
    /// id 1.01's codebase through qcc and our compiler.
    Id101 {
        /// The codebase directory.
        dir: PathBuf,
    },
    /// The rerelease's codebases through FTEQCC and our compiler.
    Rerelease {
        /// The directory of codebases.
        dir: PathBuf,
    },
    /// id's server on a script, twice: the traces must agree and decode (P3-23).
    Engine {
        /// The script (`oracles/README.md`, "The engine oracle").
        script: PathBuf,
        /// Where the trace goes.
        trace: PathBuf,
        /// A recorded trace in canonical JSON form that this one must equal.
        #[arg(long)]
        expect: Option<PathBuf>,
        /// Write the trace's canonical JSON form here, to record it.
        #[arg(long)]
        record: Option<PathBuf>,
        /// The engine's command line, after `--`: `-basedir <dir>` and the like.
        #[arg(last = true)]
        engine: Vec<String>,
    },
    /// A trace file as JSON.
    Json {
        /// The trace.
        trace: PathBuf,
    },
}

/// Whether a differential records the oracles' outcomes or checks the ones recorded.
#[derive(clap::Args, Debug)]
struct Recording {
    /// Record the oracles' outcomes.
    #[arg(long, conflicts_with = "recorded")]
    record: bool,
    /// Check against the recorded outcomes, without the oracles.
    #[arg(long)]
    recorded: bool,
}

pub(crate) fn main(root: &Path, args: &Args) -> Result<bool, String> {
    run(root, args, &System, &CURL, &builtins::Suite::all)
}

/// [`main`] with the programs `exec` runs, the files `net` fetches and the builtin suite.
fn run(
    root: &Path,
    args: &Args,
    exec: &dyn Exec,
    net: &dyn Net,
    suite: &dyn Fn() -> builtins::Suite,
) -> Result<bool, String> {
    let build = || build_with(root, exec, net);
    let Some(command) = &args.command else {
        let oracles = build()?;
        let vectors = vectors::check(root, &oracles)?;
        let numbers = numbers::check(root, &oracles)?;
        let probes = probes::check(&oracles)?;
        let scenarios = scenarios::check(&oracles, None)?;
        // The same, the builtin differential against the outcomes its fixture records rather
        // than the oracles, about a minute where the live run takes half an hour, and the
        // compiler differential left to `cargo test`, which checks it against its recorded
        // outcomes: what pull requests run unless they change the oracles or the
        // differentials' cases (docs/ci.md).
        if args.recorded {
            let builtins = builtins::check_recorded(root, &suite())?;
            let grammar = grammar::check_recorded(root)?;
            return Ok([vectors, numbers, probes, scenarios, builtins, grammar]
                .iter()
                .all(|passed| *passed));
        }
        let builtins = builtins::check(root, &oracles, false, &suite())?;
        let codegen = codegen::check(root, &oracles, false)?;
        let grammar = grammar::check(root, &oracles, false)?;
        return Ok([
            vectors, numbers, probes, scenarios, builtins, codegen, grammar,
        ]
        .iter()
        .all(|passed| *passed));
    };
    match command {
        OracleCommand::Builtins(Recording { recorded: true, .. }) => {
            builtins::check_recorded(root, &suite())
        }
        OracleCommand::Builtins(Recording { record, .. }) => {
            builtins::check(root, &build()?, *record, &suite())
        }
        OracleCommand::Codegen { record } => codegen::check(root, &build()?, *record),
        OracleCommand::Grammar(Recording { recorded: true, .. }) => grammar::check_recorded(root),
        OracleCommand::Grammar(Recording { record, .. }) => {
            grammar::check(root, &build()?, *record)
        }
        OracleCommand::Fuzz(options) => fuzz::run(root, options, exec, net),
        OracleCommand::Scenarios { names } => {
            let oracles = build()?;
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            scenarios::check(&oracles, Some(&names))
        }
        OracleCommand::Build => {
            let oracles = build()?;
            println!("oracles: qcc at {}", oracles.qcc.display());
            println!("oracles: vm-oracle at {}", oracles.vm.display());
            println!("oracles: engine-oracle at {}", oracles.engine.display());
            println!("oracles: fteqcc at {}", oracles.fteqcc.display());
            println!(
                "oracles: fteqcc-fixed at {}",
                oracles.fteqcc_fixed.display()
            );
            Ok(true)
        }
        OracleCommand::Id101 { dir } => {
            let oracles = build()?;
            id101::check(&oracles, dir)
        }
        OracleCommand::Rerelease { dir } => {
            let oracles = build()?;
            rerelease::check(&oracles, dir)
        }
        OracleCommand::Engine {
            script,
            trace,
            expect,
            record,
            engine,
        } => {
            let oracles = build()?;
            let passed = engine::check(&oracles, script, trace, expect.as_deref(), engine)?;
            if passed && let Some(out) = record {
                engine::record(trace, out)?;
            }
            Ok(passed)
        }
        OracleCommand::Json { trace } => {
            let name = trace.display();
            let bytes = fs::read(trace).map_err(|e| format!("{name}: {e}"))?;
            let decoded = qc_trace::decode(&bytes).map_err(|e| format!("{name}: {e:?}"))?;
            print!("{}", qc_trace::to_json(&decoded));
            Ok(true)
        }
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
        // The VM oracle's sources keep id's quakedef.h out: `pr_exec.c` includes it from its
        // own directory first, where it would stand in for ours. The engine oracle's own
        // upstream, kept in a directory of its own, has it.
        if upstream.id == VM_UPSTREAM && upstream.files.keys().any(|f| f.ends_with("quakedef.h")) {
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
fn generate(
    sources: &Sources,
    cache: &Path,
    out: &Path,
    target: Target,
) -> Result<Vec<PathBuf>, String> {
    let mut generated = Vec::new();
    for entry in sources.extract.iter().filter(|e| e.oracle == target) {
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
             #include \"quakedef.h\"\n{}",
            upstream.repo,
            upstream.commit,
            entry.file,
            stem.to_ascii_uppercase(),
            if target == Target::Engine {
                "#include \"extracted.h\"\n"
            } else {
                ""
            }
        );
        for name in &entry.functions {
            let function =
                extract::function(&text, name).map_err(|e| format!("{}: {e}", entry.file))?;
            c.push_str(&format!(
                "#line {} \"{}\"\n{}",
                function.line, entry.file, function.text
            ));
        }
        let prefix = if target == Target::Engine {
            "engine_id"
        } else {
            "id"
        };
        let file = out.join(format!("{prefix}_{stem}.c"));
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
        sha256::hex_digest(include_str!("oracles/defects.rs").as_bytes()),
    ];
    let mut files = Vec::new();
    collect_files(&root.join("oracles"), &mut files)?;
    files.sort();
    // Only what the build reads: the C, its headers and `sources.toml`. The engine oracle's
    // routes and their recordings are data a run reads, and editing them rebuilds nothing.
    files.retain(|file| {
        file.extension()
            .is_some_and(|extension| extension == "c" || extension == "h" || extension == "toml")
    });
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
        engine: bin.join(Compiler::executable("engine-oracle")),
        view: bin.join(Compiler::executable("view-oracle")),
        fteqcc: bin.join(Compiler::executable("fteqcc")),
        fteqcc_fixed: bin.join(Compiler::executable("fteqcc-fixed")),
        work: out.join("work"),
        rerelease: out.join("rerelease"),
        exec,
    }
}

/// The VM oracle's executable, once built.
pub(crate) fn vm_oracle(root: &Path) -> PathBuf {
    locate(root, &System).vm
}

/// Runs `script` once through the engine oracle, writing its trace to `trace` (DIFF-16.1);
/// `engine` is the engine's command line.
pub(crate) fn engine_trace(
    oracles: &Oracles<'_>,
    script: &Path,
    trace: &Path,
    engine: &[String],
) -> Result<(), String> {
    engine::run_once(oracles, script, trace, engine)
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
        && oracles.engine.is_file()
        && (cfg!(windows) || oracles.view.is_file())
        && oracles.fteqcc.is_file()
        && oracles.fteqcc_fixed.is_file()
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
    build_fteqcc(
        &compiler,
        &fteqcc_dir,
        &objects,
        "fteqcc",
        fteqcc_defines,
        &oracles.fteqcc,
    )?;

    // The VM oracle: pr_exec.c whole, id's extracted functions, and our harness.
    let vm_objects = compile_vm(root, &sources, &cache, &objects, &compiler, &[])?;
    compiler.link(&vm_objects, &oracles.vm)?;
    let engine_objects = compile_engine(root, &sources, &cache, &objects, &compiler)?;
    compiler.link(&engine_objects, &oracles.engine)?;
    // VIEWDIFF-2.6: id's Windows client needs its DirectX and MGL drivers.
    if !cfg!(windows) {
        let view_objects = compile_view(root, &cache, &objects, &compiler)?;
        compiler.link(&view_objects, &oracles.view)?;
    }
    // DIFF-14.12: the same FTEQCC with its defects fixed, from a copy of the tree whose
    // `qcc_pr_comp.c` has every fix of `defects::FIXES` applied.
    let fixed_dir = cache.join("fteqcc-fixed");
    copy_tree(&fteqcc_dir, &fixed_dir)?;
    let file = fixed_dir.join(defects::FILE);
    let pinned = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let fixed = defects::apply(&pinned)?;
    fs::write(&file, fixed).map_err(|e| format!("{}: {e}", file.display()))?;
    build_fteqcc(
        &compiler,
        &fixed_dir,
        &objects,
        "fteqcc_fixed",
        fteqcc_defines,
        &oracles.fteqcc_fixed,
    )?;
    fs::write(&stamp_file, format!("{stamp_now}\n"))
        .map_err(|e| format!("{}: {e}", stamp_file.display()))?;
    println!(
        "oracles: built qcc, vm-oracle, engine-oracle, view-oracle, fteqcc and fteqcc-fixed in {}",
        bin.display()
    );
    Ok(oracles)
}

/// Compiles FTEQCC's files from the tree `dir` into objects named after `prefix` and links
/// them to `out`.
fn build_fteqcc(
    compiler: &Compiler<'_>,
    dir: &Path,
    objects: &Path,
    prefix: &str,
    defines: &[&str],
    out: &Path,
) -> Result<(), String> {
    let mut built = Vec::new();
    for file in FTEQCC_FILES {
        let source = dir.join(file);
        let object = objects.join(format!("{prefix}_{}.o", stem(&source)));
        compiler.object(
            &Unit {
                source: &source,
                code: Code::Upstream,
                includes: &[],
                defines,
                forced_include: None,
            },
            &object,
        )?;
        built.push(object);
    }
    compiler.link(&built, out)
}

/// Copies the tree `from` to `to`, replacing what `to` held.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    if to.exists() {
        fs::remove_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    let mut files = Vec::new();
    collect_files(from, &mut files)?;
    for file in files {
        let relative = file.strip_prefix(from).map_err(|e| e.to_string())?;
        let target = to.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::copy(&file, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    Ok(())
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
    for source in generate(sources, cache, objects, Target::Vm)? {
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

/// Compiles the engine oracle's objects into `objects`: id's server files whole and the
/// functions extracted for it, over id's own headers, and our harness; returns the objects to
/// link.
fn compile_engine(
    root: &Path,
    sources: &Sources,
    cache: &Path,
    objects: &Path,
    compiler: &Compiler,
) -> Result<Vec<PathBuf>, String> {
    let quake = cache.join(ENGINE_UPSTREAM).join("WinQuake");
    let includes = vec![root.join("oracles/engine"), quake.clone()];
    let upstream = ENGINE_FILES
        .iter()
        .map(|file| quake.join(file))
        .chain(generate(sources, cache, objects, Target::Engine)?)
        .map(|source| (source, Code::Upstream));
    let ours = ENGINE_HARNESS
        .iter()
        .map(|file| (root.join(file), Code::Ours));
    let rand = root.join("oracles/engine/rand.h");
    let mut engine_objects = Vec::new();
    for (source, code) in upstream.chain(ours) {
        let object = objects.join(format!("engine_{}.o", stem(&source)));
        let forced_include = match code {
            Code::Upstream => Some(rand.as_path()),
            Code::Ours => None,
        };
        compiler.object(
            &Unit {
                source: &source,
                code,
                includes: &includes,
                defines: &[],
                forced_include,
            },
            &object,
        )?;
        engine_objects.push(object);
    }
    Ok(engine_objects)
}

/// The view oracle's objects: [`VIEW_FILES`] from id's tree, unchanged but for `rand`
/// (`oracles/engine/rand.h`), and [`VIEW_HARNESS`].
fn compile_view(
    root: &Path,
    cache: &Path,
    objects: &Path,
    compiler: &Compiler,
) -> Result<Vec<PathBuf>, String> {
    let quake = cache.join(ENGINE_UPSTREAM).join("WinQuake");
    let includes = vec![root.join("oracles/engine"), quake.clone()];
    let rand = root.join("oracles/engine/rand.h");
    let upstream = VIEW_FILES.iter().map(|file| {
        (
            quake.join(file),
            Code::Upstream,
            VIEW_CLIENT_FILES.contains(file),
        )
    });
    let ours = VIEW_HARNESS
        .iter()
        .map(|file| (root.join(file), Code::Ours, false));
    let mut view_objects = Vec::new();
    for (source, code, client) in upstream.chain(ours) {
        let object = objects.join(format!("view_{}.o", stem(&source)));
        let forced_include = match code {
            Code::Upstream => Some(rand.as_path()),
            Code::Ours => None,
        };
        compiler.object(
            &Unit {
                source: &source,
                code,
                includes: &includes,
                defines: if client { &["QS_CLIENT_RAND"] } else { &[] },
                forced_include,
            },
            &object,
        )?;
        view_objects.push(object);
    }
    Ok(view_objects)
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_owned()
}
