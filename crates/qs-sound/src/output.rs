// SPDX-License-Identifier: GPL-3.0-or-later

//! Where the mixed frames go: a device the preview host app provides, or nothing for a
//! headless run.

/// Takes the mixer's frames, left, right pairs of 16-bit samples at the mixer's rate.
pub trait Output {
    /// Plays `frames`.
    fn write(&mut self, frames: &[i16]);
}

/// Counts the frames it is given and plays nothing: the headless mode's output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NullOutput {
    frames: u64,
}

impl NullOutput {
    /// How many frames have been written.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }
}

impl Output for NullOutput {
    fn write(&mut self, frames: &[i16]) {
        let pairs = u64::try_from(frames.len().checked_div(2).unwrap_or(0)).unwrap_or(u64::MAX);
        self.frames = self.frames.saturating_add(pairs);
    }
}
