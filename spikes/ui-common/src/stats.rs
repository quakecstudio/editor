// SPDX-License-Identifier: GPL-3.0-or-later
//! Frame samples, percentiles and the CSV both prototypes write.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;

use crate::script::Phase;

/// The pass threshold: one frame at 120 Hz, in nanoseconds.
pub const BUDGET_NS: u64 = 8_333_333;

/// One measured frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sample {
    pub frame: usize,
    /// CPU time to produce the frame, from applying the scripted action to handing the frame
    /// to the GPU, excluding any wait for vsync.
    pub frame_ns: u64,
    /// Time spent applying the scripted action to the model (the same code in both).
    pub action_ns: u64,
    /// Wall time since the previous frame started; bounded below by the display's refresh.
    pub interval_ns: u64,
    /// The framework's own layout-and-paint time, where it reports it separately (gpui).
    pub draw_ns: u64,
    /// The framework's own GPU submission time, where it reports it separately (gpui).
    pub present_ns: u64,
}

/// Percentiles of one phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    pub count: usize,
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    pub max: u64,
    /// Frames over [`BUDGET_NS`].
    pub over_budget: usize,
}

/// Nearest-rank percentile of sorted values; `None` when empty.
#[must_use]
pub fn percentile(sorted: &[u64], pct: u64) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let n = u64::try_from(sorted.len()).ok()?;
    let rank = pct.saturating_mul(n).div_ceil(100).max(1);
    let index = usize::try_from(rank.saturating_sub(1)).ok()?;
    sorted.get(index).copied()
}

/// Summarises a set of frame times.
#[must_use]
pub fn summarise(values: &[u64]) -> Option<Summary> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    Some(Summary {
        count: sorted.len(),
        p50: percentile(&sorted, 50)?,
        p95: percentile(&sorted, 95)?,
        p99: percentile(&sorted, 99)?,
        max: *sorted.last()?,
        over_budget: sorted.iter().filter(|v| **v > BUDGET_NS).count(),
    })
}

/// Everything a run recorded.
#[derive(Debug, Default)]
pub struct Recorder {
    samples: Vec<(Phase, Sample)>,
}

impl Recorder {
    pub fn push(&mut self, phase: Phase, sample: Sample) {
        self.samples.push((phase, sample));
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Per-phase summaries of `frame_ns`, plus `all` over every measured phase.
    #[must_use]
    pub fn summaries(&self) -> BTreeMap<&'static str, Summary> {
        let mut by_phase: BTreeMap<&'static str, Vec<u64>> = BTreeMap::new();
        for (phase, sample) in &self.samples {
            by_phase
                .entry(phase.name())
                .or_default()
                .push(sample.frame_ns);
            if phase.measured() {
                by_phase.entry("all").or_default().push(sample.frame_ns);
            }
        }
        by_phase
            .into_iter()
            .filter_map(|(name, values)| Some((name, summarise(&values)?)))
            .collect()
    }

    /// Writes the raw samples as CSV and returns a plain-text summary table.
    ///
    /// # Errors
    /// Fails when the file cannot be written.
    pub fn write(&self, path: &Path, framework: &str, host: &str) -> io::Result<String> {
        let mut csv = String::from(
            "framework,host,phase,frame,frame_ns,action_ns,interval_ns,draw_ns,present_ns\n",
        );
        for (phase, s) in &self.samples {
            let _ = writeln!(
                csv,
                "{framework},{host},{},{},{},{},{},{},{}",
                phase.name(),
                s.frame,
                s.frame_ns,
                s.action_ns,
                s.interval_ns,
                s.draw_ns,
                s.present_ns
            );
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, csv)?;
        let mut table = format!(
            "{framework} on {host}: frame CPU time in ms (budget {:.2} ms at p99)\n",
            ms(BUDGET_NS)
        );
        table.push_str("phase        frames    p50    p95    p99    max  over\n");
        for (name, s) in self.summaries() {
            let _ = writeln!(
                table,
                "{name:<12} {:>6} {:>6.2} {:>6.2} {:>6.2} {:>6.2} {:>5}",
                s.count,
                ms(s.p50),
                ms(s.p95),
                ms(s.p99),
                ms(s.max),
                s.over_budget
            );
        }
        Ok(table)
    }
}

#[expect(clippy::cast_precision_loss, reason = "display only")]
fn ms(ns: u64) -> f64 {
    ns as f64 / 1_000_000.0
}

/// The host key: `QS_BENCH_HOST` if set (as in `docs/perf-budgets.md`), else OS and arch.
#[must_use]
pub fn host() -> String {
    std::env::var("QS_BENCH_HOST")
        .unwrap_or_else(|_| format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_percentiles() {
        let values: Vec<u64> = (1..=100).collect();
        assert_eq!(percentile(&values, 50), Some(50));
        assert_eq!(percentile(&values, 99), Some(99));
        assert_eq!(percentile(&values, 100), Some(100));
        assert_eq!(percentile(&[7], 99), Some(7));
        assert_eq!(percentile(&[], 99), None);
    }

    #[test]
    fn summaries_count_frames_over_budget_and_skip_warmup_in_all() {
        let mut rec = Recorder::default();
        rec.push(
            Phase::Warmup,
            Sample {
                frame_ns: 50_000_000,
                ..Sample::default()
            },
        );
        rec.push(
            Phase::Type,
            Sample {
                frame_ns: 1_000_000,
                ..Sample::default()
            },
        );
        rec.push(
            Phase::Scroll,
            Sample {
                frame_ns: 9_000_000,
                ..Sample::default()
            },
        );
        let all = rec.summaries().get("all").copied();
        assert_eq!(
            all.map(|s| (s.count, s.max, s.over_budget)),
            Some((2, 9_000_000, 1))
        );
    }
}
