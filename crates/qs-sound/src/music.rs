// SPDX-License-Identifier: GPL-3.0-or-later

//! Music tracks as files (`docs/spec/engine/rerelease.md` RRE-3): the files a CD track number
//! names, in the order the host tries them, as RRE-3.1 states them.

/// The directory the tracks are in (RRE-3).
pub const RRE_MUSIC_DIR: &str = "music";

/// The extensions QuakeSpasm's codecs read, in its handler order (RRE-3.1).
pub const RRE_MUSIC_EXTENSIONS: [&str; 8] =
    ["ogg", "opus", "mp3", "flac", "wav", "it", "s3m", "xm"];

/// A CD track and whether it loops, as `svc_cdtrack` names them (CLIENT-1.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Track {
    /// The track number.
    pub number: u8,
    /// Whether it loops; the server's `looptrack` is not read by QuakeSpasm, which always
    /// loops.
    pub looping: bool,
}

/// The files that may hold `track`, `music/track<NN>.<ext>` for each extension the host
/// decodes, in the order given (RRE-3.1); the host takes the one from the search path entry
/// of highest priority.
#[must_use]
pub fn track_files(track: u8, extensions: &[&str]) -> Vec<String> {
    extensions
        .iter()
        .map(|ext| format!("{RRE_MUSIC_DIR}/track{track:02}.{ext}"))
        .collect()
}
