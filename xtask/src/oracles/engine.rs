// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask oracles engine <script> <trace> [--expect <jsonl>] [-- <engine word>...]`: id's
//! server on a script (P3-23, `oracles/README.md` "The engine oracle"). The oracle runs the
//! script twice; the two traces must be the same bytes, and `qc-trace`'s strict decoder must
//! read them, every record after the header a frame or a string (TRACE-4.11).
//!
//! With `--expect`, the trace's canonical form must equal the one recorded there, which proves
//! the run the same on another system: the canonical form numbers each engine string value by
//! when its first string record comes, from `0xffff_0000` up, wherever it appears, since those
//! values are offsets into the oracle's memory and differ from one build to the next
//! (TRACE-7.3). Every other value is as the trace has it.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use qc_trace::{Changes, FieldChange, GlobalChange, Record, Trace};

use super::Oracles;

/// Runs `script` through the engine oracle twice, writing the trace to `trace`, and checks the
/// runs agree and the trace reads; returns whether it passed.
/// `engine` is the engine's command line, `-basedir <dir>` and the like.
pub(super) fn check(
    oracles: &Oracles<'_>,
    script: &Path,
    trace: &Path,
    expect: Option<&Path>,
    engine: &[String],
) -> Result<bool, String> {
    let again = trace.with_extension("again");
    for out in [trace, again.as_path()] {
        let output = oracles
            .exec
            .output(
                Command::new(&oracles.engine)
                    .arg(script)
                    .arg(out)
                    .args(engine),
            )
            .map_err(|e| format!("{}: {e}", oracles.engine.display()))?;
        if !output.status.success() {
            return Err(format!(
                "engine-oracle {} stopped with {}: {}",
                script.display(),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
    }
    let first = fs::read(trace).map_err(|e| format!("{}: {e}", trace.display()))?;
    let second = fs::read(&again).map_err(|e| format!("{}: {e}", again.display()))?;
    let decoded = qc_trace::decode(&first).map_err(|e| format!("{}: {e:?}", trace.display()))?;
    let frames = summary(&decoded.records)?;
    if first != second {
        println!(
            "FAIL  engine {}: two runs gave different traces ({} and {} bytes)",
            script.display(),
            first.len(),
            second.len()
        );
        return Ok(false);
    }
    if let Some(expected) = expect {
        let text =
            fs::read_to_string(expected).map_err(|e| format!("{}: {e}", expected.display()))?;
        let recorded =
            qc_trace::from_json(&text).map_err(|e| format!("{}: {e:?}", expected.display()))?;
        let ours = canonical(&decoded)?;
        if ours != recorded {
            let at = first_difference(&ours, &recorded);
            println!(
                "FAIL  engine {}: the trace differs from {} at record {at}",
                script.display(),
                expected.display()
            );
            return Ok(false);
        }
    }
    println!(
        "ok    engine {}: {frames} frames, {} bytes, the same on two runs{}",
        script.display(),
        first.len(),
        if expect.is_some() {
            " and as recorded"
        } else {
            ""
        }
    );
    Ok(true)
}

/// Writes the canonical JSON form of the trace at `trace` to `out`, for `--expect`.
pub(super) fn record(trace: &Path, out: &Path) -> Result<(), String> {
    let bytes = fs::read(trace).map_err(|e| format!("{}: {e}", trace.display()))?;
    let decoded = qc_trace::decode(&bytes).map_err(|e| format!("{}: {e:?}", trace.display()))?;
    fs::write(out, qc_trace::to_json(&canonical(&decoded)?))
        .map_err(|e| format!("{}: {e}", out.display()))
}

/// The first value [`canonical`] gives an engine string value.
const FIRST_CANONICAL_STRING: u32 = 0xffff_0000;

/// `trace` with every engine string value numbered by when its first string record comes,
/// from [`FIRST_CANONICAL_STRING`], in its string records and wherever a change set holds it.
pub(super) fn canonical(trace: &Trace) -> Result<Trace, String> {
    let mut numbers: BTreeMap<u32, u32> = BTreeMap::new();
    for record in &trace.records {
        if let Record::String { value, .. } = record
            && !numbers.contains_key(value)
        {
            let next = u32::try_from(numbers.len())
                .ok()
                .and_then(|n| FIRST_CANONICAL_STRING.checked_add(n))
                .ok_or("more engine strings than the canonical form numbers")?;
            numbers.insert(*value, next);
        }
    }
    let renumber = |bits: u32| numbers.get(&bits).copied().unwrap_or(bits);
    let changes = |changes: &Changes| -> Result<Changes, String> {
        let globals = changes
            .globals()
            .iter()
            .map(|g| GlobalChange {
                slot: g.slot,
                bits: renumber(g.bits),
            })
            .collect();
        let fields = changes
            .fields()
            .iter()
            .map(|f| FieldChange {
                edict: f.edict,
                field: f.field,
                bits: renumber(f.bits),
            })
            .collect();
        Changes::new(globals, fields).map_err(|e| format!("{e:?}"))
    };
    let mut records = Vec::with_capacity(trace.records.len());
    for record in &trace.records {
        records.push(match record {
            Record::String { value, text } => Record::String {
                value: renumber(*value),
                text: text.clone(),
            },
            Record::Frame { frame, changes: c } => Record::Frame {
                frame: *frame,
                changes: changes(c)?,
            },
            Record::Error {
                function,
                statement,
                message,
                changes: c,
            } => Record::Error {
                function: *function,
                statement: *statement,
                message: message.clone(),
                changes: changes(c)?,
            },
            other @ (Record::Run { .. } | Record::Step(_) | Record::Exit { .. }) => other.clone(),
        });
    }
    Ok(Trace {
        header: trace.header,
        records,
    })
}

/// The index of the first record where `a` and `b` differ, or the shorter's length.
fn first_difference(a: &Trace, b: &Trace) -> usize {
    a.records
        .iter()
        .zip(&b.records)
        .position(|(x, y)| x != y)
        .unwrap_or_else(|| a.records.len().min(b.records.len()))
}

/// The number of frame records, or why the records are not an engine trace's: frames and
/// strings only, the frames numbered from 1 in order (TRACE-4.11).
fn summary(records: &[Record]) -> Result<u32, String> {
    let mut frames = 0_u32;
    for record in records {
        match record {
            Record::Frame { frame, .. } => {
                let next = frames.saturating_add(1);
                if *frame != next {
                    return Err(format!("frame {frame} where frame {next} was due"));
                }
                frames = next;
            }
            Record::String { .. } => {}
            Record::Run { .. } | Record::Step(_) | Record::Exit { .. } | Record::Error { .. } => {
                return Err("an engine trace holds frame and string records only".to_owned());
            }
        }
    }
    Ok(frames)
}

#[cfg(test)]
mod tests;
