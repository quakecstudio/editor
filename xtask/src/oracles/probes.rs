// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs malformed and edge-case input files through the VM oracle and checks its exit status
//! (`oracles/README.md`): the regression tests of P1-36. Scenario files are read as
//! `docs/spec/differential.md` DIFF-2.1, DIFF-2.7 and DIFF-2.9 say, so a NUL byte is a byte of
//! its line; the init file of vector mode takes the numbers `oracles/README.md` names.
//!
//! A refused file must exit with status 2 and name the line the probe gives; any other must
//! exit with status 0.

use std::fs;
use std::path::Path;
use std::process::Command;

use qc_progs::{Function, ProgsFile, Statement, write};
use qc_types::Slot;

use super::Oracles;

const EXIT_TRACED: i32 = 0;
const EXIT_REFUSED: i32 = 2;

/// Which of the oracle's modes reads the probe's file.
#[derive(Clone, Copy, Debug)]
enum Mode {
    Scenario,
    Vector,
}

#[derive(Debug)]
struct Probe {
    mode: Mode,
    text: &'static [u8],
    /// The line a refusal names, or `None` for a file the oracle must run.
    refused_at: Option<u32>,
}

const fn scenario(text: &'static [u8], refused_at: Option<u32>) -> Probe {
    Probe {
        mode: Mode::Scenario,
        text,
        refused_at,
    }
}

const fn vector(text: &'static [u8], refused_at: Option<u32>) -> Probe {
    Probe {
        mode: Mode::Vector,
        text,
        refused_at,
    }
}

const PROBES: &[Probe] = &[
    // DIFF-2.1: a NUL byte ends neither the line nor the file.
    scenario(b"progs progs.dat\nframes 1\nmap te\0st\nbogus 1\n", Some(3)),
    scenario(
        b"progs progs.dat\nframes 0\n# a \0 in a comment\nbogus 1\n",
        Some(4),
    ),
    scenario(b"progs progs.dat\nframes 0\n# a \0 in a comment\n", None),
    scenario(b"progs progs.dat\nframes 0\nmap test\0\n", Some(3)),
    scenario(b"progs progs.dat\nframes\0 0\n", Some(2)),
    scenario(b"progs progs.dat\nframes 0\0\n", Some(2)),
    scenario(b"progs progs.dat\nframes 1\0 0\n", Some(2)),
    scenario(b"progs progs.dat\nframes 0\nframetime 0.1\0x\n", Some(3)),
    scenario(b"progs progs.dat\nframes 0\nseed 1\0\n", Some(3)),
    scenario(b"progs progs.dat\0x\nframes 0\n", Some(1)),
    scenario(
        b"progs progs.dat\nframes 0\nmodel m 0 0 0 0 0 0\0\n",
        Some(3),
    ),
    // A model or setting name holding a NUL byte is a name no QuakeC string gives.
    scenario(b"progs progs.dat\nframes 0\nmodel a\0b 0 0 0 0 0 0\n", None),
    scenario(
        b"progs progs.dat\nframes 0\nmodel a\0b 0 0 0 0 0 0\nmodel a\0c 0 0 0 0 0 0\n",
        None,
    ),
    scenario(
        b"progs progs.dat\nframes 0\nmodel a\0b 0 0 0 0 0 0\nmodel a\0b 0 0 0 0 0 0\n",
        Some(4),
    ),
    scenario(b"progs progs.dat\nframes 0\ncvar sk\0ill 3\n", None),
    scenario(b"progs progs.dat\nframes 0\ncvar skill 3\0x\n", None),
    // DIFF-2.7: leading zeros, however many.
    scenario(b"progs progs.dat\nframes 0000000000000000001\n", None),
    scenario(
        b"progs progs.dat\nframes 00000000000000000000000000000000001000000\n",
        None,
    ),
    scenario(b"progs progs.dat\nframes -0000000000000000000\n", None),
    scenario(
        b"progs progs.dat\nframes 0000000000000000000001000001\n",
        Some(2),
    ),
    scenario(b"progs progs.dat\nframes 9999999999999999999\n", Some(2)),
    scenario(b"progs progs.dat\nframes -9999999999999999999\n", Some(2)),
    scenario(
        b"progs progs.dat\nframes 0\nseed 00000000000000000004294967295\n",
        None,
    ),
    scenario(b"progs progs.dat\nframes 0\nseed 4294967296\n", Some(3)),
    // The init file of vector mode: each number is the one its line takes.
    vector(b"edicts 3\nentry 1\n", None),
    vector(
        b"edicts 3\nmaxclients 1\nseed 4294967295\nentry 1\nglobal 100 bits ffffffff\n\
          global 101 entity 2\nglobal 102 pointer 1 105\nfield 2 105 bits 0\n",
        None,
    ),
    vector(
        b"edicts 3\nglobal 100x bits zz\nseed abc\nentry 1\n",
        Some(2),
    ),
    vector(b"edicts 3\nglobal 100x bits 0\n", Some(2)),
    vector(b"edicts 3\nglobal 100 bits zz\n", Some(2)),
    vector(b"edicts 3\nglobal 100 bits 1ffffffff\n", Some(2)),
    vector(b"edicts 3\nglobal 100 bits 0x1\n", Some(2)),
    vector(b"edicts 3\nglobal 100 bits -1\n", Some(2)),
    vector(b"edicts 3\nglobal 128 bits 0\n", Some(2)),
    vector(b"edicts 3\nglobal 100 entity 1x\n", Some(2)),
    vector(b"edicts 3\nglobal 100 entity -1\n", Some(2)),
    vector(b"edicts 3\nglobal 100 entity 99999999999\n", Some(2)),
    vector(b"edicts 3\nglobal 100 pointer 1 105x\n", Some(2)),
    vector(b"edicts 3\nglobal 100 bits 0 0\n", Some(2)),
    vector(b"edicts 3\nfield 1x 105 bits 0\n", Some(2)),
    vector(b"edicts 3\nfield 3 105 bits 0\n", Some(2)),
    vector(b"edicts 3\nfield 1 110 bits 0\n", Some(2)),
    vector(b"edicts 3\nseed abc\n", Some(2)),
    vector(b"edicts 3\nseed -1\n", Some(2)),
    vector(b"edicts 3\nseed 4294967296\n", Some(2)),
    vector(b"edicts 3\nentry 1x\n", Some(2)),
    vector(b"edicts 3x\n", Some(1)),
    vector(b"edicts 3 4\n", Some(1)),
    vector(b"edicts 0\n", Some(1)),
    vector(b"edicts 3\nmaxclients x\n", Some(2)),
    vector(b"edicts 3\nmaxclients 17\n", Some(2)),
    vector(b"edicts 3\nhost run 1x\n", Some(2)),
    vector(b"edicts 3\nhost think 1x 0.1\n", Some(2)),
    vector(b"edicts 3\nhost think 3 0.1\n", Some(2)),
    vector(b"edicts 3\nhost clock inf\n", Some(2)),
    vector(b"edicts 3\nhost clock 0x1p-4\n", Some(2)),
    vector(b"edicts 3\nhost clock +1\n", Some(2)),
    vector(b"edicts 3\nhost physics 0.1 0\n", Some(2)),
    vector(b"edicts 3\nhost frame 0.1 1x\n", Some(2)),
    vector(b"edicts 3\nhost edict 1 128\n", Some(2)),
    vector(b"edicts 3\nhost edict 1x 100\n", Some(2)),
    vector(b"edicts 3\nhost clock 1 2 3 4 5 6 7\n", Some(2)),
    vector(b"edicts 3\nglobal 100 bits 0\0junk\n", Some(2)),
    vector(b"edicts 3\n\0\n", Some(2)),
    // P1-RG readiness review (TEST-9): directive-level refusals DIFF-2.9 names, which the
    // number vectors (frametime, DIFF-2.8) and the earlier NUL and leading-zero probes do not
    // reach. Each is refused before any run, naming the line.
    // DIFF-2.2: a map name is 1 to 63 bytes from a-z, 0-9 and _, so an uppercase letter or a
    // 64th byte is refused.
    scenario(b"progs progs.dat\nmap Test\nframes 0\n", Some(2)),
    scenario(
        b"progs progs.dat\n\
          map aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nframes 0\n",
        Some(2),
    ),
    // DIFF-2.2: submodels is an integer from 0 to 254, so 255 is refused.
    scenario(b"progs progs.dat\nsubmodels 255\nframes 0\n", Some(2)),
    // DIFF-2.2 and DIFF-2.9: frames may appear at most once, so a second frames is refused.
    scenario(b"progs progs.dat\nframes 0\nframes 1\n", Some(3)),
    // DIFF-2.6: an input field is button0, button2, impulse or v_angle, so button1 is refused.
    scenario(
        b"progs progs.dat\nframes 10\nclients 1\ninput 1 1 button1 1\n",
        Some(4),
    ),
    // DIFF-2.6: impulse takes an integer from 1 to 255, so 256 is refused.
    scenario(
        b"progs progs.dat\nframes 10\nclients 1\ninput 1 1 impulse 256\n",
        Some(4),
    ),
];

/// Runs every probe; `Ok(false)` when the oracle disagrees with one.
pub(crate) fn check(oracles: &Oracles) -> Result<bool, String> {
    let work = oracles.work.join("probes");
    fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    fs::write(work.join("progs.dat"), progs()?).map_err(|e| e.to_string())?;
    let (mut passed, mut failed) = (0_u32, 0_u32);
    for (index, probe) in PROBES.iter().enumerate() {
        match run(oracles, &work, probe) {
            Ok(()) => passed = passed.saturating_add(1),
            Err(problem) => {
                failed = failed.saturating_add(1);
                println!(
                    "FAIL  probe {}: {:?} {:?}: {problem}",
                    index.saturating_add(1),
                    probe.mode,
                    String::from_utf8_lossy(probe.text)
                );
            }
        }
    }
    println!("oracles: {passed} input file probes passed, {failed} failed");
    Ok(failed == 0)
}

/// A progs DIFF-3.1 accepts and the vector environment can run: 128 globals, 110 fields, and
/// function 1 that returns at once.
fn progs() -> Result<Vec<u8>, String> {
    let entry = Function {
        first_statement: 1,
        ..Function::default()
    };
    let file = ProgsFile {
        entityfields: 110,
        statements: vec![Statement::default(), Statement::default()],
        functions: vec![Function::default(), entry],
        strings: vec![0],
        globals: vec![Slot::ZERO; 128],
        ..ProgsFile::default()
    };
    write(&file).map_err(|e| format!("cannot write the progs: {e:?}"))
}

fn run(oracles: &Oracles, work: &Path, probe: &Probe) -> Result<(), String> {
    let name = match probe.mode {
        Mode::Scenario => "probe.scn",
        Mode::Vector => "probe.init",
    };
    let input = work.join(name);
    fs::write(&input, probe.text).map_err(|e| e.to_string())?;
    let mut command = Command::new(&oracles.vm);
    match probe.mode {
        Mode::Scenario => command.arg("scenario").arg(&input),
        Mode::Vector => command
            .arg("vector")
            .arg(work.join("progs.dat"))
            .arg(&input),
    };
    let output = oracles
        .exec
        .output(command.arg(work.join("trace.bin")))
        .map_err(|e| format!("cannot run the VM oracle: {e}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    let want = if probe.refused_at.is_some() {
        EXIT_REFUSED
    } else {
        EXIT_TRACED
    };
    if output.status.code() != Some(want) {
        return Err(format!(
            "the VM oracle exited with {}, the probe says {want}: {stderr}",
            output.status
        ));
    }
    if let Some(line) = probe.refused_at
        && !stderr.contains(&format!("{name}:{line}:"))
    {
        return Err(format!(
            "the VM oracle refused the file, but not at line {line}: {stderr}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use std::process::Command;

    use super::super::testing::{args, fake_oracles, fresh_dir};
    use super::{Mode, PROBES, check, progs};
    use crate::exec::{Fake, Reply};

    /// A fake VM oracle that does what each probe says: refuses at its line, or runs.
    fn agreeing(command: &Command) -> Reply {
        let args = args(command);
        let vector = args[0].as_os_str() == "vector";
        let input = std::fs::read(&args[if vector { 2 } else { 1 }]).unwrap();
        let probe = PROBES.iter().find(|probe| probe.text == input).unwrap();
        let name = match probe.mode {
            Mode::Scenario => "probe.scn",
            Mode::Vector => "probe.init",
        };
        match probe.refused_at {
            Some(line) => Reply {
                code: 2,
                stderr: format!("vm-oracle: {name}:{line}: refused"),
                ..Reply::default()
            },
            None => Reply::exit(0),
        }
    }

    #[test]
    fn the_progs_has_a_function_1_that_returns_at_once() {
        let bytes = progs().unwrap();
        let read = qc_progs::Progs::read(&bytes).unwrap();
        assert_eq!(read.header().entityfields, 110);
        assert_eq!(read.functions().get(1).unwrap().first_statement, 1);
        assert_eq!(read.statements().len(), 2);
        assert_eq!(read.globals().len(), 128);
    }

    #[test]
    fn every_probe_passes_an_oracle_that_does_what_it_says() {
        let work = fresh_dir("probes-agree");
        let fake = Fake::new(agreeing);
        assert!(check(&fake_oracles(&work, &fake)).unwrap());
        assert_eq!(fake.seen.lock().unwrap().len(), PROBES.len());
        assert!(work.join("probes/progs.dat").is_file());
    }

    #[test]
    fn a_probe_fails_on_the_wrong_status_or_line() {
        let work = fresh_dir("probes-disagree");
        let runs = Fake::new(|_: &Command| Reply::exit(0));
        assert!(!check(&fake_oracles(&work, &runs)).unwrap());
        let wrong_line = Fake::new(|_: &Command| Reply {
            code: 2,
            stderr: "vm-oracle: probe.scn:99: refused".to_owned(),
            ..Reply::default()
        });
        assert!(!check(&fake_oracles(&work, &wrong_line)).unwrap());
        let cannot_run = Fake::new(|_: &Command| Reply {
            cannot_run: true,
            ..Reply::default()
        });
        assert!(!check(&fake_oracles(&work, &cannot_run)).unwrap());
    }

    #[test]
    fn writes_a_progs_qc_progs_reads() {
        let bytes = progs().unwrap();
        assert!(qc_progs::Progs::read(&bytes).is_ok());
    }

    #[test]
    fn every_probe_ends_its_last_line() {
        for probe in PROBES {
            assert!(probe.text.ends_with(b"\n"), "{probe:?}");
        }
    }
}
