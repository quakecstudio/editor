// SPDX-License-Identifier: GPL-3.0-or-later

//! Line numbers for byte offsets.

/// The start offset of every line of a source, for turning an offset into a line number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    /// Indexes `text`. Only LF starts a line; CR is whitespace (QC-1.2).
    #[must_use]
    pub fn new(text: &[u8]) -> Self {
        let mut starts = vec![0];
        for (offset, &b) in text.iter().enumerate() {
            if b == b'\n' {
                starts.push(offset.saturating_add(1));
            }
        }
        Self { starts }
    }

    /// The 1-based line of the byte at `offset`: one more than the number of LF bytes before
    /// it (QC-1.2).
    #[must_use]
    pub fn line(&self, offset: usize) -> u32 {
        let line = self.starts.partition_point(|&start| start <= offset);
        u32::try_from(line).unwrap_or(u32::MAX)
    }
}
