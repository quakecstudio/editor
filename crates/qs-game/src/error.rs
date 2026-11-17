// SPDX-License-Identifier: GPL-3.0-or-later

//! Why the game could not load, draw or run.

use std::fmt;

/// Why the game could not load, draw or run: the stage that failed and what it reported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The command line asks for what the host cannot do.
    Usage(String),
    /// The install, a map or one of its files did not load.
    Load(String),
    /// A frame did not draw, or its picture did not encode.
    Draw(String),
    /// The sound device did not open.
    Sound(String),
    /// A view script did not read, or its output did not write.
    Script(String),
    /// The bridge's socket did not open.
    Bridge(String),
    /// The window did not open or stopped.
    Window(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(reason)
            | Self::Load(reason)
            | Self::Draw(reason)
            | Self::Sound(reason)
            | Self::Script(reason)
            | Self::Bridge(reason)
            | Self::Window(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for Error {}
