// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask oracles engine <script> <trace>`: id's server on a script (P3-23,
//! `oracles/README.md` "The engine oracle"). The oracle runs the script twice; the two traces
//! must be the same bytes, and `qc-trace`'s strict decoder must read them, every record after
//! the header a frame or a string (TRACE-4.11).

use std::fs;
use std::path::Path;
use std::process::Command;

use qc_trace::Record;

use super::Oracles;

/// Runs `script` through the engine oracle twice, writing the trace to `trace`, and checks the
/// runs agree and the trace reads; returns whether it passed.
pub(super) fn check(oracles: &Oracles<'_>, script: &Path, trace: &Path) -> Result<bool, String> {
    let again = trace.with_extension("again");
    for out in [trace, again.as_path()] {
        let output = oracles
            .exec
            .output(Command::new(&oracles.engine).arg(script).arg(out))
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
    println!(
        "ok    engine {}: {frames} frames, {} bytes, the same on two runs",
        script.display(),
        first.len()
    );
    Ok(true)
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
