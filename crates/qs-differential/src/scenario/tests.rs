// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the scenario reader (`differential.md` DIFF-2).
//!
//! The number vectors run every text of `docs/spec/vectors/scenario/numbers.txt` through the
//! reader as the oracle (`xtask/src/oracles/numbers.rs`) runs it through the id VM: each text
//! is put in a scenario where its directive takes an integer or a number, and the reader must
//! accept exactly the texts the vector file does not mark `refused`, and read each accepted
//! one as the value the file gives. This is the acceptance criterion of P1-13 that the
//! runner's scenario reader gives every vector its result (DIFF-2.7 to DIFF-2.9).

#![expect(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed expect, index or panic is a failed test, and line numbers are \
              small counts"
)]

use std::fs;
use std::path::{Path, PathBuf};

use super::*;

/// The repository root, from this crate's directory.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root is reachable")
}

/// Reads a scenario from a string with `.` as its directory.
fn read_str(text: &str) -> Result<Scenario, Refused> {
    read(text.as_bytes(), Path::new("."))
}

// --- The number vectors (DIFF-2.7 to DIFF-2.9) ---

const VECTORS: &str = "docs/spec/vectors/scenario/numbers.txt";

/// One vector: its line, directive, text and result (`refused` or the value).
struct Vector {
    line: usize,
    directive: String,
    text: String,
    result: String,
}

/// Reads the vector file, `<directive> <text> <result>` per line, `#` comments.
fn vectors() -> Vec<Vector> {
    let path = root().join(VECTORS);
    let text = fs::read_to_string(&path).expect("the number vectors are readable");
    let mut vectors = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let body = raw.split_once('#').map_or(raw, |(before, _)| before);
        let words: Vec<&str> = body.split_whitespace().collect();
        match words.as_slice() {
            [] => {}
            [directive, text, result] => vectors.push(Vector {
                line: index + 1,
                directive: (*directive).to_owned(),
                text: (*text).to_owned(),
                result: (*result).to_owned(),
            }),
            _ => panic!(
                "{VECTORS}:{}: expected <directive> <text> <result>",
                index + 1
            ),
        }
    }
    vectors
}

/// Builds the scenario that puts `text` where `directive` takes a number, as
/// `xtask/src/oracles/numbers.rs` does, so the reader reads the text in context.
fn scenario_for(directive: &str, text: &str) -> String {
    match directive {
        "frames" => format!("progs p.dat\nframes {text}\n"),
        "frametime" => format!("progs p.dat\nframes 0\nframetime {text}\n"),
        "model" => format!("progs p.dat\nframes 0\nmodel m {text} 0 0 0 0 0\n"),
        "v_angle" => format!("progs p.dat\nclients 1\nframes 1\ninput 1 1 v_angle {text} 0 0\n"),
        other => panic!("{VECTORS}: unknown directive `{other}`"),
    }
}

/// The value the reader read for the vector, in the vector file's form, or `None` if refused.
fn read_value(directive: &str, scenario: &Scenario) -> Option<String> {
    match directive {
        "frames" => Some(scenario.frames.to_string()),
        "frametime" => Some(format!("{:016x}", scenario.frametime.to_bits())),
        "model" => Some(format!("{:08x}", scenario.models[0].mins[0].to_bits())),
        "v_angle" => match scenario.inputs[0] {
            Input::VAngle { values, .. } => Some(format!("{:08x}", values[0].to_bits())),
            _ => panic!("the v_angle scenario did not read a v_angle input"),
        },
        other => panic!("{VECTORS}: unknown directive `{other}`"),
    }
}

// spec DIFF-2.7: an integer is an optional `-` and digits, read in base 10, in range or refused
// spec DIFF-2.8: a number has the decimal syntax, is rounded once to the nearest double or f32,
// is finite, and frametime is above 0 and at most 1
// spec DIFF-2.9: a value that is not an integer or a number, or is out of range, is refused
#[test]
fn every_number_vector_reads_as_its_result_says() {
    let vectors = vectors();
    assert_eq!(
        vectors.len(),
        107,
        "{VECTORS} should hold 107 vectors, found {}",
        vectors.len()
    );
    for vector in &vectors {
        let scenario = scenario_for(&vector.directive, &vector.text);
        let read = read_str(&scenario);
        if vector.result == "refused" {
            assert!(
                read.is_err(),
                "{VECTORS}:{}: {} {} should be refused, read {read:?}",
                vector.line,
                vector.directive,
                vector.text
            );
        } else {
            let scenario = read.unwrap_or_else(|e| {
                panic!(
                    "{VECTORS}:{}: {} {} should be accepted, refused: {e}",
                    vector.line, vector.directive, vector.text
                )
            });
            let got = read_value(&vector.directive, &scenario);
            assert_eq!(
                got.as_deref(),
                Some(vector.result.as_str()),
                "{VECTORS}:{}: {} {} read as the wrong value",
                vector.line,
                vector.directive,
                vector.text
            );
        }
    }
}

// spec DIFF-2.7: every directive and both verdicts are exercised by the vector set
// spec DIFF-2.8: every number directive and both verdicts are exercised by the vector set
#[test]
fn the_vector_set_covers_each_directive_and_verdict() {
    let vectors = vectors();
    for directive in ["frames", "frametime", "model", "v_angle"] {
        for refused in [false, true] {
            assert!(
                vectors
                    .iter()
                    .any(|v| v.directive == directive && (v.result == "refused") == refused),
                "{VECTORS} has no {} vector for {directive}",
                if refused { "refused" } else { "accepted" }
            );
        }
    }
}

// --- The scenario file (DIFF-2.1 to DIFF-2.6, DIFF-2.9) ---

// spec DIFF-2.2: a scenario with every directive reads into the fields it names
#[test]
fn a_full_scenario_reads_every_directive() {
    let text = "\
# a full scenario
progs     progs.dat
entities  start.ent
map       start
maxclients 4
submodels 12
cvar      skill 2
clients   1
frames    300
frametime 0.05
seed      7
model     *1 -16 -16 -16 16 16 16
input     10 1 impulse 9
input     20 1 button0 1
input     50 1 v_angle 0 90 0
";
    let s = read_str(text).expect("the scenario is accepted");
    assert_eq!(s.progs, Path::new("./progs.dat"));
    assert_eq!(s.entities, Some(PathBuf::from("./start.ent")));
    assert_eq!(s.map, "start");
    assert_eq!(s.maxclients, 4);
    assert_eq!(s.clients, 1);
    assert_eq!(s.submodels, 12);
    assert_eq!(s.frames, 300);
    assert_eq!(s.frametime.to_bits(), 0.05_f64.to_bits());
    assert_eq!(s.seed, 7);
    assert_eq!(s.models.len(), 1);
    assert_eq!(s.models[0].name, b"*1");
    assert_eq!(
        s.cvars,
        vec![Cvar {
            name: b"skill".to_vec(),
            text: b"2".to_vec()
        }]
    );
    assert_eq!(s.inputs.len(), 3);
}

// spec DIFF-2.2: the defaults of DIFF-2.2 fill in a scenario that gives only progs
#[test]
fn the_defaults_fill_in() {
    let s = read_str("progs progs.dat\n").expect("accepted");
    assert_eq!(s.map, DIFF_DEFAULT_MAP);
    assert_eq!(s.maxclients, 1);
    assert_eq!(s.clients, 0);
    assert_eq!(s.submodels, 0);
    assert_eq!(s.frames, DIFF_DEFAULT_FRAMES);
    assert_eq!(s.frametime.to_bits(), DIFF_DEFAULT_FRAMETIME.to_bits());
    assert_eq!(s.seed, DIFF_DEFAULT_SEED);
    assert_eq!(s.entities, None);
    assert!(s.models.is_empty() && s.cvars.is_empty() && s.inputs.is_empty());
}

// spec DIFF-2.9: a scenario with no progs is refused
#[test]
fn no_progs_is_refused() {
    let err = read_str("frames 10\n").expect_err("refused");
    assert_eq!(err.line, 0);
}

// spec DIFF-2.1: a `#` and the rest of the line is a comment, and a blank line is skipped
#[test]
fn comments_and_blank_lines_are_skipped() {
    let s = read_str("# head\n\nprogs progs.dat # trailing\n\t\n").expect("accepted");
    assert_eq!(s.progs, Path::new("./progs.dat"));
}

// spec DIFF-2.1: a line is ended by a line feed, or a carriage return and a line feed
#[test]
fn crlf_endings_are_read() {
    let s = read_str("progs progs.dat\r\nframes 5\r\n").expect("accepted");
    assert_eq!(s.frames, 5);
}

// spec DIFF-2.1: a path cannot contain a space, so words split on spaces and tabs
#[test]
fn words_split_on_spaces_and_tabs() {
    let s = read_str("progs\t\tprogs.dat\n  frames   5  \n").expect("accepted");
    assert_eq!(s.frames, 5);
}

// spec DIFF-2.9: an unknown key is refused
#[test]
fn an_unknown_key_is_refused() {
    let err = read_str("progs progs.dat\nwibble 1\n").expect_err("refused");
    assert_eq!(err.line, 2);
}

// spec DIFF-2.9: a key that may appear once is refused when repeated
#[test]
fn a_repeated_single_key_is_refused() {
    let err = read_str("progs a.dat\nframes 5\nframes 6\n").expect_err("refused");
    assert_eq!(err.line, 3);
    // A key that may repeat (model, cvar, input) is not refused.
    read_str("progs a.dat\ncvar skill 1\ncvar coop 1\n").expect("cvar may repeat");
}

// spec DIFF-2.9: a missing or extra argument is refused
#[test]
fn a_wrong_argument_count_is_refused() {
    assert_eq!(read_str("progs\n").expect_err("refused").line, 1);
    assert_eq!(read_str("progs a b\n").expect_err("refused").line, 1);
    assert_eq!(
        read_str("progs a.dat\nmodel m 0 0 0 0 0\n")
            .expect_err("refused")
            .line,
        2
    );
}

// spec DIFF-2.2: a map name is 1 to 63 bytes from a-z, 0-9 and _
#[test]
fn a_map_name_is_checked() {
    read_str("progs a.dat\nmap e1m1\n").expect("a valid name");
    assert!(read_str("progs a.dat\nmap E1M1\n").is_err(), "uppercase");
    assert!(read_str("progs a.dat\nmap a-b\n").is_err(), "a hyphen");
    assert!(read_str("progs a.dat\nmap \n").is_err(), "empty");
    let long = "a".repeat(64);
    assert!(
        read_str(&format!("progs a.dat\nmap {long}\n")).is_err(),
        "too long"
    );
    let max = "a".repeat(63);
    read_str(&format!("progs a.dat\nmap {max}\n")).expect("63 bytes");
}

// spec DIFF-2.2: maxclients is 1 to 16, clients 0 to maxclients, submodels 0 to 254
#[test]
fn the_integer_ranges_are_enforced() {
    assert!(read_str("progs a.dat\nmaxclients 0\n").is_err());
    assert!(read_str("progs a.dat\nmaxclients 17\n").is_err());
    read_str("progs a.dat\nmaxclients 16\n").expect("16 is in range");
    assert!(read_str("progs a.dat\nsubmodels 255\n").is_err());
    read_str("progs a.dat\nsubmodels 254\n").expect("254 is in range");
    assert!(read_str("progs a.dat\nseed 4294967296\n").is_err());
    read_str("progs a.dat\nseed 4294967295\n").expect("u32::MAX seed");
}

// spec DIFF-2.9: clients is refused when it is more than maxclients (a whole-file check)
#[test]
fn clients_over_maxclients_is_refused() {
    let err = read_str("progs a.dat\nmaxclients 2\nclients 3\n").expect_err("refused");
    assert_eq!(err.line, 0);
}

// spec DIFF-2.6: an input's field, range and value count are checked
#[test]
fn input_lines_are_checked() {
    let base = "progs a.dat\nclients 1\nframes 10\n";
    read_str(&format!("{base}input 1 1 button0 1\n")).expect("button0");
    read_str(&format!("{base}input 1 1 impulse 255\n")).expect("impulse 255");
    read_str(&format!("{base}input 1 1 v_angle 0 90 0\n")).expect("v_angle");
    assert!(
        read_str(&format!("{base}input 1 1 button0 2\n")).is_err(),
        "button over 1"
    );
    assert!(
        read_str(&format!("{base}input 1 1 impulse 0\n")).is_err(),
        "impulse under 1"
    );
    assert!(
        read_str(&format!("{base}input 1 1 impulse 256\n")).is_err(),
        "impulse over 255"
    );
    assert!(
        read_str(&format!("{base}input 1 1 jump 1\n")).is_err(),
        "unknown field"
    );
    assert!(
        read_str(&format!("{base}input 1 1 v_angle 0 90\n")).is_err(),
        "two angles"
    );
}

// spec DIFF-2.9: an input's frame after the last frame, or edict not a client, is refused
#[test]
fn an_out_of_range_input_is_refused() {
    let err =
        read_str("progs a.dat\nclients 1\nframes 5\ninput 6 1 button0 1\n").expect_err("refused");
    assert_eq!(err.line, 0);
    let err =
        read_str("progs a.dat\nclients 1\nframes 5\ninput 1 2 button0 1\n").expect_err("refused");
    assert_eq!(err.line, 0);
}

// spec DIFF-2.9: a repeated model name is refused
#[test]
fn a_repeated_model_name_is_refused() {
    let err =
        read_str("progs a.dat\nmodel m 0 0 0 0 0 0\nmodel m 1 1 1 1 1 1\n").expect_err("refused");
    assert_eq!(err.line, 3);
}

// spec DIFF-2.1: a NUL byte in a key or a plain argument is refused; a model or cvar name, or a
// cvar text, may hold one
#[test]
fn nul_bytes_follow_their_rule() {
    // A NUL in the key: an unknown key.
    assert!(read(b"pro\0gs a.dat\n", Path::new(".")).is_err());
    // A NUL in a plain argument (frames value): refused.
    assert!(read(b"progs a.dat\nframes 1\x002\n", Path::new(".")).is_err());
    // A NUL in a model name: accepted, the name holds the NUL.
    let s = read(b"progs a.dat\nmodel m\0x 0 0 0 0 0 0\n", Path::new(".")).expect("model name NUL");
    assert_eq!(s.models[0].name, b"m\0x");
    // A NUL in a cvar name and text: accepted.
    let s = read(b"progs a.dat\ncvar a\0b c\0d\n", Path::new(".")).expect("cvar NUL");
    assert_eq!(s.cvars[0].name, b"a\0b");
    assert_eq!(s.cvars[0].text, b"c\0d");
}

// spec DIFF-2.1: a 17th word on a line is too many arguments
#[test]
fn too_many_words_is_refused() {
    let many = (0..17).map(|_| "x").collect::<Vec<_>>().join(" ");
    let err = read_str(&format!("progs a.dat\ninput {many}\n")).expect_err("refused");
    assert_eq!(err.line, 2);
}

// spec DIFF-2.4: a cvar line keeps every setting in file order, a later one for a name after
#[test]
fn cvars_keep_file_order() {
    let s = read_str("progs a.dat\ncvar skill 1\ncvar skill 3\ncvar coop 1\n").expect("accepted");
    assert_eq!(s.cvars.len(), 3);
    assert_eq!(s.cvars[0].text, b"1");
    assert_eq!(s.cvars[1].text, b"3");
}

// spec DIFF-2.1: relative paths are joined to the scenario's directory
#[test]
fn paths_resolve_against_the_directory() {
    let s = read(b"progs sub/progs.dat\n", Path::new("/maps/start")).expect("accepted");
    assert_eq!(s.progs, Path::new("/maps/start/sub/progs.dat"));
}
