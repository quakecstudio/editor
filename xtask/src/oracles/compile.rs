// SPDX-License-Identifier: GPL-3.0-or-later

//! The system C compiler: found by the `cc` crate (so MSVC is found without a developer
//! prompt), and driven directly to compile objects and link executables.
//!
//! Every file is compiled as `docs/spec/vm.md` "Reference build" requires: strict aliasing off
//! (VM-5.1, VM-10.2) and floating-point contraction off (VM-5.2), at `-O2`. `char` is signed, as
//! on x86, which `builtins.md` BUILTIN-10.2 and id's `COM_Parse` assume; MSVC's `char` is
//! signed, does not optimise on strict aliasing and, under `/fp:precise`, does not contract.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use cc::Build;

/// The flags every file is compiled with, recorded in the build stamp.
pub(crate) const GNU_FLAGS: [&str; 5] = [
    "-std=gnu99",
    "-O2",
    "-fno-strict-aliasing",
    "-ffp-contract=off",
    "-fsigned-char",
];
pub(crate) const MSVC_FLAGS: [&str; 6] = [
    "/nologo",
    "/std:c11",
    "/O2",
    "/fp:precise",
    "/D_CRT_SECURE_NO_WARNINGS",
    "/D_CRT_NONSTDC_NO_DEPRECATE",
];

/// The flags that add clang's source-based coverage to our files and to the link, for the C
/// coverage the Readiness workflow measures (`docs/testing.md`, TEST-1). Our files are also
/// compiled with `QS_COVERAGE` defined, so that a crash writes the profile before it exits.
pub(crate) const COVERAGE_FLAGS: [&str; 2] = ["-fprofile-instr-generate", "-fcoverage-mapping"];

/// The compiler, and the environment it needs (MSVC's `INCLUDE`, `LIB` and `PATH`).
pub(crate) struct Compiler {
    path: PathBuf,
    env: Vec<(OsString, OsString)>,
    msvc: bool,
    coverage: bool,
}

/// Whose code a file is: upstream code (id's, FTEQCC's) is compiled without warnings, ours with
/// every warning an error.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Code {
    Upstream,
    Ours,
}

/// One compile.
pub(crate) struct Unit<'a> {
    pub(crate) source: &'a Path,
    pub(crate) code: Code,
    pub(crate) includes: &'a [PathBuf],
    pub(crate) defines: &'a [&'a str],
    pub(crate) forced_include: Option<&'a Path>,
}

/// The target triple of the host xtask runs on, for the `cc` crate.
fn host_triple() -> Result<&'static str, String> {
    let triple = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("windows", "aarch64") => "aarch64-pc-windows-msvc",
        (os, arch) => return Err(format!("no C compiler setup for {arch} {os}")),
    };
    Ok(triple)
}

impl Compiler {
    /// Finds the system C compiler (`CC` overrides it, as the `cc` crate documents). With
    /// `coverage`, our files and the link take [`COVERAGE_FLAGS`], which need clang.
    pub(crate) fn find(scratch: &Path, coverage: bool) -> Result<Self, String> {
        let triple = host_triple()?;
        let tool = Build::new()
            .target(triple)
            .host(triple)
            .opt_level(2)
            .debug(false)
            .out_dir(scratch)
            .cargo_metadata(false)
            .cargo_warnings(false)
            .emit_rerun_if_env_changed(false)
            .try_get_compiler()
            .map_err(|e| format!("no C compiler found: {e}"))?;
        if coverage && !tool.is_like_clang() {
            return Err(format!(
                "C coverage needs clang, and the C compiler is {} (set CC=clang)",
                tool.path().display()
            ));
        }
        Ok(Self {
            path: tool.path().to_path_buf(),
            env: tool
                .get_envs()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
            msvc: tool.is_like_msvc(),
            coverage,
        })
    }

    /// The compiler's path and flags, for the build stamp.
    pub(crate) fn describe(&self) -> String {
        let mut flags = if self.msvc {
            MSVC_FLAGS.join(" ")
        } else {
            GNU_FLAGS.join(" ")
        };
        if self.coverage {
            flags.push_str(" coverage ");
            flags.push_str(&COVERAGE_FLAGS.join(" "));
        }
        format!("{} {flags}", self.path.display())
    }

    /// The file name of an executable on this host.
    pub(crate) fn executable(name: &str) -> String {
        if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_owned()
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.path);
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command
    }

    /// Compiles `unit` to the object file `out`.
    pub(crate) fn object(&self, unit: &Unit<'_>, out: &Path) -> Result<(), String> {
        let mut command = self.command();
        if self.msvc {
            command.args(MSVC_FLAGS);
            command.arg(if unit.code == Code::Ours { "/W3" } else { "/w" });
            if unit.code == Code::Ours {
                command.arg("/WX");
            }
            for dir in unit.includes {
                command.arg(format!("/I{}", dir.display()));
            }
            for define in unit.defines {
                command.arg(format!("/D{define}"));
            }
            if let Some(forced) = unit.forced_include {
                command.arg(format!("/FI{}", forced.display()));
            }
            command
                .arg("/c")
                .arg(unit.source)
                .arg(format!("/Fo{}", out.display()));
        } else {
            command.args(GNU_FLAGS);
            if unit.code == Code::Ours {
                command.args(["-Wall", "-Wextra", "-Werror"]);
                if self.coverage {
                    command.args(COVERAGE_FLAGS).arg("-DQS_COVERAGE");
                }
            } else {
                command.arg("-w");
            }
            for dir in unit.includes {
                command.arg("-I").arg(dir);
            }
            for define in unit.defines {
                command.arg(format!("-D{define}"));
            }
            if let Some(forced) = unit.forced_include {
                command.arg("-include").arg(forced);
            }
            command.arg("-c").arg(unit.source).arg("-o").arg(out);
        }
        run(command, &format!("compile {}", unit.source.display()))
    }

    /// Links `objects` into the executable `out`.
    pub(crate) fn link(&self, objects: &[PathBuf], out: &Path) -> Result<(), String> {
        let mut command = self.command();
        if self.msvc {
            command.arg("/nologo");
            command.args(objects);
            command.arg(format!("/Fe{}", out.display()));
        } else {
            command.args(objects);
            if self.coverage {
                command.args(COVERAGE_FLAGS);
            }
            command.arg("-o").arg(out).arg("-lm");
        }
        run(command, &format!("link {}", out.display()))
    }
}

fn run(mut command: Command, what: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|e| format!("{what}: cannot run the compiler: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{what} failed ({}):\n{}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}
