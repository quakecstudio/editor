// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs the number vectors of `docs/spec/vectors/scenario/numbers.txt` (format in its README)
//! through the VM oracle's scenario mode: the acceptance criterion of P1-37 that the oracle
//! refuses exactly the texts DIFF-2.7 to DIFF-2.9 refuse.
//!
//! Each vector becomes a scenario that puts its text where its directive takes an integer or
//! a number. The oracle reads only whether the scenario is refused (exit status 2, with the
//! directive's line in the message) or runs (exit status 0); the value read is checked on the
//! Rust side by `scenario_numbers` in `crates/qs-tests`. The progs is the smallest one that
//! DIFF-3.1 accepts, so a scenario that is not refused runs until its first QuakeC call
//! fails, which ends it with an error record and exit status 0 (DIFF-8).

use std::fs;
use std::path::Path;
use std::process::Command;

use qc_progs::{Function, ProgsFile, Statement, write};
use qc_types::Slot;

use super::Oracles;

const VECTORS: &str = "docs/spec/vectors/scenario/numbers.txt";

/// The global slots and entity fields DIFF-3.1 requires of a progs.
const MIN_GLOBALS: usize = 92;
const MIN_FIELDS: i32 = 105;

/// The exit statuses of `oracles/README.md`.
const EXIT_TRACED: i32 = 0;
const EXIT_REFUSED: i32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Directive {
    Frames,
    Frametime,
    Model,
    VAngle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Vector {
    line: usize,
    directive: Directive,
    text: String,
    refused: bool,
}

/// Checks every vector against the oracle; `Ok(false)` when one of them disagrees.
pub(crate) fn check(root: &Path, oracles: &Oracles) -> Result<bool, String> {
    let text = fs::read_to_string(root.join(VECTORS)).map_err(|e| format!("{VECTORS}: {e}"))?;
    let vectors = parse(&text)?;
    if vectors.is_empty() {
        return Err(format!("no vectors found in {VECTORS}"));
    }
    let work = oracles.work.join("numbers");
    fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    fs::write(work.join("progs.dat"), progs()?).map_err(|e| e.to_string())?;
    let (mut passed, mut failed) = (0_u32, 0_u32);
    for vector in &vectors {
        match run(oracles, &work, vector) {
            Ok(()) => passed = passed.saturating_add(1),
            Err(problem) => {
                failed = failed.saturating_add(1);
                println!(
                    "FAIL  {VECTORS}:{}: {} {}: {problem}",
                    vector.line,
                    name(vector.directive),
                    vector.text
                );
            }
        }
    }
    println!("oracles: {passed} scenario number vectors passed, {failed} failed");
    Ok(failed == 0)
}

fn name(directive: Directive) -> &'static str {
    match directive {
        Directive::Frames => "frames",
        Directive::Frametime => "frametime",
        Directive::Model => "model",
        Directive::VAngle => "v_angle",
    }
}

/// Reads the vector file: `<directive> <text> <result>` per line, `#` comments.
fn parse(text: &str) -> Result<Vec<Vector>, String> {
    let mut vectors = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index.saturating_add(1);
        let body = raw.split_once('#').map_or(raw, |(before, _)| before);
        let words: Vec<&str> = body.split_whitespace().collect();
        let (directive, text, result) = match words.as_slice() {
            [] => continue,
            [directive, text, result] => (*directive, *text, *result),
            _ => {
                return Err(format!(
                    "{VECTORS}:{line}: expected <directive> <text> <result>"
                ));
            }
        };
        let directive = match directive {
            "frames" => Directive::Frames,
            "frametime" => Directive::Frametime,
            "model" => Directive::Model,
            "v_angle" => Directive::VAngle,
            other => return Err(format!("{VECTORS}:{line}: unknown directive `{other}`")),
        };
        let digits = match directive {
            Directive::Frames => 0,
            Directive::Frametime => 16,
            Directive::Model | Directive::VAngle => 8,
        };
        let value_ok = if digits == 0 {
            !result.is_empty() && result.bytes().all(|b| b.is_ascii_digit())
        } else {
            result.len() == digits && result.bytes().all(|b| b.is_ascii_hexdigit())
        };
        if result != "refused" && !value_ok {
            return Err(format!("{VECTORS}:{line}: bad result `{result}`"));
        }
        vectors.push(Vector {
            line,
            directive,
            text: text.to_owned(),
            refused: result == "refused",
        });
    }
    Ok(vectors)
}

/// The scenario for one vector, and the line of it that holds the vector's text.
fn scenario(vector: &Vector) -> (String, usize) {
    let text = &vector.text;
    match vector.directive {
        Directive::Frames => (format!("progs progs.dat\nframes {text}\n"), 2),
        Directive::Frametime => (format!("progs progs.dat\nframes 0\nframetime {text}\n"), 3),
        Directive::Model => (
            format!("progs progs.dat\nframes 0\nmodel m {text} 0 0 0 0 0\n"),
            3,
        ),
        Directive::VAngle => (
            format!("progs progs.dat\nclients 1\nframes 1\ninput 1 1 v_angle {text} 0 0\n"),
            4,
        ),
    }
}

/// The smallest progs DIFF-3.1 accepts: id's header CRC, 92 globals and 105 fields.
fn progs() -> Result<Vec<u8>, String> {
    let file = ProgsFile {
        entityfields: MIN_FIELDS,
        statements: vec![Statement::default()],
        functions: vec![Function::default()],
        strings: vec![0],
        globals: vec![Slot(0); MIN_GLOBALS],
        ..ProgsFile::default()
    };
    write(&file).map_err(|e| format!("cannot write the progs: {e:?}"))
}

fn run(oracles: &Oracles, work: &Path, vector: &Vector) -> Result<(), String> {
    let (text, line) = scenario(vector);
    let path = work.join("scenario.scn");
    fs::write(&path, text).map_err(|e| e.to_string())?;
    let output = oracles
        .exec
        .output(
            Command::new(&oracles.vm)
                .arg("scenario")
                .arg(&path)
                .arg(work.join("trace.bin")),
        )
        .map_err(|e| format!("cannot run the VM oracle: {e}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    let want = if vector.refused {
        EXIT_REFUSED
    } else {
        EXIT_TRACED
    };
    if output.status.code() != Some(want) {
        return Err(format!(
            "the VM oracle exited with {}, the vector says {want}: {stderr}",
            output.status
        ));
    }
    if vector.refused && !stderr.contains(&format!("scenario.scn:{line}:")) {
        return Err(format!(
            "the VM oracle refused the scenario, but not at line {line}: {stderr}"
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

    use super::super::testing::{args, fake_oracles, fresh_dir, root};
    use super::{Directive, VECTORS, Vector, check, parse, progs, scenario};
    use crate::exec::{Fake, Reply};

    /// A fake VM oracle that refuses exactly the vectors marked `refused`, at their line.
    fn agreeing(command: &Command) -> Reply {
        let written = std::fs::read_to_string(&args(command)[1]).unwrap();
        let vectors = parse(&std::fs::read_to_string(root().join(VECTORS)).unwrap()).unwrap();
        let vector = vectors
            .iter()
            .find(|vector| scenario(vector).0 == written)
            .unwrap();
        if vector.refused {
            Reply {
                code: 2,
                stderr: format!("vm-oracle: scenario.scn:{}: refused", scenario(vector).1),
                ..Reply::default()
            }
        } else {
            Reply::exit(0)
        }
    }

    // spec DIFF-3.1: the number vectors' progs is the smallest the oracle accepts, with 92
    // globals and 105 fields.
    #[test]
    fn the_progs_has_the_fewest_globals_and_fields_accepted() {
        let bytes = progs().unwrap();
        let read = qc_progs::Progs::read(&bytes).unwrap();
        assert_eq!(read.header().entityfields, super::MIN_FIELDS);
        assert_eq!(read.globals().len(), super::MIN_GLOBALS);
        assert_eq!(read.statements().len(), 1);
        assert_eq!(read.functions().len(), 1);
    }

    #[test]
    fn every_number_vector_passes_an_oracle_that_agrees() {
        let work = fresh_dir("numbers-agree");
        let fake = Fake::new(agreeing);
        assert!(check(&root(), &fake_oracles(&work, &fake)).unwrap());
        assert!(fake.seen.lock().unwrap().len() > 100);
    }

    #[test]
    fn a_number_vector_fails_on_the_wrong_status_or_line() {
        let work = fresh_dir("numbers-disagree");
        let runs = Fake::new(|_: &Command| Reply::exit(0));
        assert!(!check(&root(), &fake_oracles(&work, &runs)).unwrap());
        let wrong_line = Fake::new(|_: &Command| Reply {
            code: 2,
            stderr: "vm-oracle: scenario.scn:99: refused".to_owned(),
            ..Reply::default()
        });
        assert!(!check(&root(), &fake_oracles(&work, &wrong_line)).unwrap());
        let cannot_run = Fake::new(|_: &Command| Reply {
            cannot_run: true,
            ..Reply::default()
        });
        assert!(!check(&root(), &fake_oracles(&work, &cannot_run)).unwrap());
    }

    #[test]
    fn parses_vectors_and_comments() {
        let vectors =
            parse("# c\n\nframes 01 1 # ok\nframetime .1 3fb999999999999a\nmodel x refused\n")
                .unwrap();
        assert_eq!(vectors.len(), 3);
        assert_eq!(
            vectors[0],
            Vector {
                line: 3,
                directive: Directive::Frames,
                text: "01".to_owned(),
                refused: false,
            }
        );
        assert_eq!(vectors[1].directive, Directive::Frametime);
        assert!(vectors[2].refused);
    }

    #[test]
    fn refuses_malformed_vector_lines() {
        assert!(parse("frames 1\n").is_err());
        assert!(parse("frames 1 1 1\n").is_err());
        assert!(parse("seed 1 1\n").is_err());
        assert!(parse("frames 1 x\n").is_err());
        assert!(parse("frametime 1 3ff0\n").is_err());
        assert!(parse("model 1 3f80000g\n").is_err());
        assert!(parse("v_angle 1 3ff0000000000000\n").is_err());
    }

    #[test]
    fn puts_the_text_on_the_line_it_names() {
        for directive in [
            Directive::Frames,
            Directive::Frametime,
            Directive::Model,
            Directive::VAngle,
        ] {
            let vector = Vector {
                line: 1,
                directive,
                text: "TEXT".to_owned(),
                refused: true,
            };
            let (text, line) = scenario(&vector);
            let held = text.lines().nth(line - 1).unwrap();
            assert!(held.contains(" TEXT"), "{held}");
        }
    }

    #[test]
    fn writes_a_progs_qc_progs_reads() {
        let bytes = progs().unwrap();
        assert!(qc_progs::Progs::read(&bytes).is_ok());
    }
}
