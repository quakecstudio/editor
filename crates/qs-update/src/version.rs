// SPDX-License-Identifier: GPL-3.0-or-later

//! A release's version, `YYYY.MM.N` (`docs/conventions.md`, `docs/releasing.md`).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The parts of a version: the year, the month and the month's release.
const PARTS: usize = 3;

/// A release's version, `YYYY.MM.N`: the year, the month and the month's release counted from
/// 1, as `2026.11.1`. Versions order as their releases were made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Version {
    year: u32,
    month: u32,
    release: u32,
}

/// Why a text is not a version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotAVersion(String);

impl fmt::Display for NotAVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} is not a version, three numbers as 2026.11.1",
            self.0
        )
    }
}

impl std::error::Error for NotAVersion {}

impl FromStr for Version {
    type Err = NotAVersion;

    /// Three decimal numbers with a `.` between them and nothing else: no sign, no space and
    /// no pre-release suffix, since a pre-release reaches no feed.
    fn from_str(text: &str) -> Result<Self, NotAVersion> {
        let mut numbers = [0_u32; PARTS];
        let mut parts = text.split('.');
        for number in &mut numbers {
            *number = parts
                .next()
                .filter(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|part| part.parse().ok())
                .ok_or_else(|| NotAVersion(text.to_owned()))?;
        }
        if parts.next().is_some() {
            return Err(NotAVersion(text.to_owned()));
        }
        let [year, month, release] = numbers;
        Ok(Self {
            year,
            month,
            release,
        })
    }
}

impl TryFrom<String> for Version {
    type Error = NotAVersion;

    fn try_from(text: String) -> Result<Self, NotAVersion> {
        text.parse()
    }
}

impl From<Version> for String {
    fn from(version: Version) -> Self {
        version.to_string()
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.year, self.month, self.release)
    }
}
