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

use crate::exec::Exec;
use crate::host::host_triple;

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

/// The flags of a libFuzzer build (`oracles/vm/fuzz.c`): every file with the address and
/// undefined-behaviour sanitizers and libFuzzer's coverage, a failed undefined-behaviour check
/// a crash; the link brings libFuzzer. The alignment check is left out: id's edict layout puts
/// edict `n` at `n` times the edict size (EDICT-1.5), which on a 64-bit host is not a multiple
/// of the 8 bytes `edict_t` asks for, and x86-64 and arm64 read such addresses, as id's own
/// 64-bit builds do.
pub(crate) const FUZZ_FLAGS: [&str; 4] = [
    "-fsanitize=fuzzer-no-link,address,undefined",
    "-fno-sanitize=alignment",
    "-fno-sanitize-recover=undefined",
    "-g",
];
pub(crate) const FUZZ_LINK_FLAGS: [&str; 1] = ["-fsanitize=fuzzer,address,undefined"];

/// What a build adds to the reference build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Instrument {
    /// Nothing: the oracles as CI runs them.
    None,
    /// Source-based coverage of our files ([`COVERAGE_FLAGS`]).
    Coverage,
    /// A fuzz target ([`FUZZ_FLAGS`]).
    Fuzz,
}

/// The compiler, and the environment it needs (MSVC's `INCLUDE`, `LIB` and `PATH`).
pub(crate) struct Compiler<'a> {
    path: PathBuf,
    env: Vec<(OsString, OsString)>,
    msvc: bool,
    instrument: Instrument,
    exec: &'a dyn Exec,
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

/// Refuses `instrument` unless the compiler at `path` is clang (`is_clang`): coverage and
/// fuzzing use clang's instrumentation.
fn needs_clang(instrument: Instrument, is_clang: bool, path: &Path) -> Result<(), String> {
    if instrument != Instrument::None && !is_clang {
        return Err(format!(
            "C coverage and fuzzing need clang, and the C compiler is {} (set CC=clang)",
            path.display()
        ));
    }
    Ok(())
}

impl<'a> Compiler<'a> {
    /// Finds the system C compiler (`CC` overrides it, as the `cc` crate documents). Coverage
    /// and fuzzing need clang (for fuzzing, one with libFuzzer).
    pub(crate) fn find(
        scratch: &Path,
        instrument: Instrument,
        exec: &'a dyn Exec,
    ) -> Result<Self, String> {
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
        needs_clang(instrument, tool.is_like_clang(), tool.path())?;
        Ok(Self {
            path: tool.path().to_path_buf(),
            env: tool
                .get_envs()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
            msvc: tool.is_like_msvc(),
            instrument,
            exec,
        })
    }

    /// The compiler's path and flags, for the build stamp.
    pub(crate) fn describe(&self) -> String {
        let mut flags = if self.msvc {
            MSVC_FLAGS.join(" ")
        } else {
            GNU_FLAGS.join(" ")
        };
        match self.instrument {
            Instrument::None => {}
            Instrument::Coverage => {
                flags.push_str(" coverage ");
                flags.push_str(&COVERAGE_FLAGS.join(" "));
            }
            Instrument::Fuzz => {
                flags.push_str(" fuzz ");
                flags.push_str(&FUZZ_FLAGS.join(" "));
            }
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
                if self.instrument == Instrument::Coverage {
                    command.args(COVERAGE_FLAGS).arg("-DQS_COVERAGE");
                }
            } else {
                command.arg("-w");
            }
            if self.instrument == Instrument::Fuzz {
                command.args(FUZZ_FLAGS).arg("-DQS_FUZZ");
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
        run(
            self.exec,
            command,
            &format!("compile {}", unit.source.display()),
        )
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
            match self.instrument {
                Instrument::None => {}
                Instrument::Coverage => {
                    command.args(COVERAGE_FLAGS);
                }
                Instrument::Fuzz => {
                    command.args(FUZZ_LINK_FLAGS);
                }
            }
            command.arg("-o").arg(out).arg("-lm");
        }
        run(self.exec, command, &format!("link {}", out.display()))
    }
}

fn run(exec: &dyn Exec, mut command: Command, what: &str) -> Result<(), String> {
    let output = exec
        .output(&mut command)
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

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::super::testing::fresh_dir;
    use super::*;
    use crate::exec::{Fake, Reply, System};

    fn compiler(exec: &dyn Exec, msvc: bool, instrument: Instrument) -> Compiler<'_> {
        Compiler {
            path: PathBuf::from(if msvc { "cl.exe" } else { "cc" }),
            env: vec![("INCLUDE".into(), "inc".into())],
            msvc,
            instrument,
            exec,
        }
    }

    fn unit<'a>(code: Code, includes: &'a [PathBuf], forced: Option<&'a Path>) -> Unit<'a> {
        Unit {
            source: Path::new("a.c"),
            code,
            includes,
            defines: &["QS_ENGINE"],
            forced_include: forced,
        }
    }

    #[test]
    fn only_an_instrumented_build_needs_clang() {
        let path = Path::new("/usr/bin/gcc");
        assert!(needs_clang(Instrument::None, false, path).is_ok());
        assert!(needs_clang(Instrument::None, true, path).is_ok());
        assert!(needs_clang(Instrument::Coverage, true, path).is_ok());
        assert!(needs_clang(Instrument::Fuzz, true, path).is_ok());
        for instrument in [Instrument::Coverage, Instrument::Fuzz] {
            let error = needs_clang(instrument, false, path).unwrap_err();
            assert!(error.contains("/usr/bin/gcc (set CC=clang)"), "{error}");
        }
    }

    #[test]
    fn our_files_are_strict_and_upstream_files_quiet() {
        let fake = Fake::new(|_: &Command| Reply::exit(0));
        let includes = [PathBuf::from("inc")];
        let gnu = compiler(&fake, false, Instrument::None);
        gnu.object(&unit(Code::Ours, &includes, None), Path::new("a.o"))
            .unwrap();
        gnu.object(
            &unit(Code::Upstream, &[], Some(Path::new("compat.h"))),
            Path::new("b.o"),
        )
        .unwrap();
        gnu.link(&[PathBuf::from("a.o")], Path::new("bin")).unwrap();
        let msvc = compiler(&fake, true, Instrument::None);
        msvc.object(&unit(Code::Ours, &includes, None), Path::new("a.obj"))
            .unwrap();
        msvc.object(
            &unit(Code::Upstream, &[], Some(Path::new("compat.h"))),
            Path::new("b.obj"),
        )
        .unwrap();
        msvc.link(&[PathBuf::from("a.obj")], Path::new("bin.exe"))
            .unwrap();
        let seen = fake.seen.borrow();
        let gnu_flags = GNU_FLAGS.join(" ");
        assert_eq!(
            seen[0],
            format!("cc {gnu_flags} -Wall -Wextra -Werror -I inc -DQS_ENGINE -c a.c -o a.o")
        );
        assert_eq!(
            seen[1],
            format!("cc {gnu_flags} -w -DQS_ENGINE -include compat.h -c a.c -o b.o")
        );
        assert_eq!(seen[2], "cc a.o -o bin -lm");
        let msvc_flags = MSVC_FLAGS.join(" ");
        assert_eq!(
            seen[3],
            format!("cl.exe {msvc_flags} /W3 /WX /Iinc /DQS_ENGINE /c a.c /Foa.obj")
        );
        assert_eq!(
            seen[4],
            format!("cl.exe {msvc_flags} /w /DQS_ENGINE /FIcompat.h /c a.c /Fob.obj")
        );
        assert_eq!(seen[5], "cl.exe /nologo a.obj /Febin.exe");
    }

    #[test]
    fn coverage_and_fuzz_builds_add_their_flags() {
        let fake = Fake::new(|_: &Command| Reply::exit(0));
        let coverage = compiler(&fake, false, Instrument::Coverage);
        coverage
            .object(&unit(Code::Ours, &[], None), Path::new("a.o"))
            .unwrap();
        coverage
            .object(&unit(Code::Upstream, &[], None), Path::new("b.o"))
            .unwrap();
        coverage.link(&[], Path::new("bin")).unwrap();
        let fuzz = compiler(&fake, false, Instrument::Fuzz);
        fuzz.object(&unit(Code::Upstream, &[], None), Path::new("c.o"))
            .unwrap();
        fuzz.link(&[], Path::new("fuzz")).unwrap();
        let seen = fake.seen.borrow();
        assert!(seen[0].contains("-fprofile-instr-generate -fcoverage-mapping -DQS_COVERAGE"));
        assert!(
            !seen[1].contains("-fprofile-instr-generate"),
            "id's files are not counted"
        );
        assert!(seen[2].contains("-fprofile-instr-generate"));
        assert!(seen[3].contains("-fsanitize=fuzzer-no-link,address,undefined"));
        assert!(seen[3].contains("-DQS_FUZZ"));
        assert!(seen[4].contains("-fsanitize=fuzzer,address,undefined"));
        assert!(
            compiler(&fake, false, Instrument::Coverage)
                .describe()
                .ends_with("coverage -fprofile-instr-generate -fcoverage-mapping")
        );
        assert!(
            compiler(&fake, false, Instrument::Fuzz)
                .describe()
                .contains(" fuzz ")
        );
        assert!(
            compiler(&fake, true, Instrument::None)
                .describe()
                .contains("/fp:precise")
        );
    }

    #[test]
    fn a_failed_compile_says_what_and_why() {
        let fails = Fake::new(|_: &Command| Reply {
            code: 1,
            stderr: "a.c:1: error".to_owned(),
            ..Reply::default()
        });
        let error = compiler(&fails, false, Instrument::None)
            .object(&unit(Code::Ours, &[], None), Path::new("a.o"))
            .unwrap_err();
        assert!(
            error.contains("compile a.c failed") && error.contains("a.c:1: error"),
            "{error}"
        );
        let cannot = Fake::new(|_: &Command| Reply {
            cannot_run: true,
            ..Reply::default()
        });
        let error = compiler(&cannot, false, Instrument::None)
            .link(&[], Path::new("bin"))
            .unwrap_err();
        assert!(error.contains("cannot run the compiler"), "{error}");
    }

    #[test]
    fn the_system_compiler_is_found_and_builds_a_program() {
        let dir = fresh_dir("compile-system");
        let compiler = Compiler::find(&dir, Instrument::None, &System).unwrap();
        assert!(
            compiler
                .describe()
                .contains(if cfg!(windows) { "/O2" } else { "-O2" })
        );
        let source = dir.join("main.c");
        std::fs::write(&source, "int main(void) { return 0; }\n").unwrap();
        let object = dir.join(if cfg!(windows) { "main.obj" } else { "main.o" });
        compiler
            .object(
                &Unit {
                    source: &source,
                    code: Code::Ours,
                    includes: &[],
                    defines: &[],
                    forced_include: None,
                },
                &object,
            )
            .unwrap();
        let program = dir.join(Compiler::executable("main"));
        compiler.link(&[object], &program).unwrap();
        assert!(Command::new(&program).status().unwrap().success());
        assert!(Compiler::executable("x").starts_with('x'));
    }
}
