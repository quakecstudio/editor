// SPDX-License-Identifier: GPL-3.0-or-later

//! The view script `--view-script` plays (P3-28): the map, then frames run, the player
//! placed and the view shot, as `oracles/engine/view_host.c` reads the same file for id's
//! engine, so the two frames and sounds can be compared (VIEWDIFF-1).

use std::path::PathBuf;

/// One step of a view script after its map (VIEWDIFF-1.2).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Step {
    /// `frames <n>`: n frames of 1/72 of a second.
    Frames(u32),
    /// `place <x> <y> <z> <pitch> <yaw>`: the player moved there, looking that way, at rest.
    Place {
        /// Where.
        origin: [f32; 3],
        /// Pitch and yaw, in degrees.
        angles: [f32; 2],
    },
    /// `shot <file>`: the view written to the file as a binary PPM.
    Shot(PathBuf),
    /// `toggleconsole`: the console opened or closed (CON-4.8).
    ToggleConsole,
    /// `togglemenu`: the main menu opened or closed (MENU-1.1).
    ToggleMenu,
}

/// A view script: its map and its steps (VIEWDIFF-1).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ViewScript {
    /// `map <name>`, the first line.
    pub(crate) map: String,
    /// The lines after it, in order.
    pub(crate) steps: Vec<Step>,
}

impl ViewScript {
    /// The script `text` says, its blank lines and `#` comments skipped (VIEWDIFF-1.1,
    /// VIEWDIFF-1.2).
    ///
    /// # Errors
    ///
    /// The line that is not one of VIEWDIFF-1.2's, or a first line that is not `map`.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'));
        let map = lines
            .next()
            .and_then(|line| line.strip_prefix("map "))
            .map(str::trim)
            .filter(|map| !map.is_empty() && !map.contains(char::is_whitespace))
            .ok_or("a view script starts with `map <name>`")?
            .to_owned();
        let mut steps = Vec::new();
        for line in lines {
            steps.push(step(line).ok_or_else(|| format!("not a view script line: {line}"))?);
        }
        Ok(Self { map, steps })
    }
}

/// The step `line` is, if it is one.
fn step(line: &str) -> Option<Step> {
    match line {
        "toggleconsole" => return Some(Step::ToggleConsole),
        "togglemenu" => return Some(Step::ToggleMenu),
        _ => {}
    }
    let (word, rest) = line.split_once(' ')?;
    match word {
        "frames" => rest.trim().parse().ok().map(Step::Frames),
        "place" => {
            let numbers: Vec<f32> = rest
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()
                .ok()?;
            let [x, y, z, pitch, yaw] = <[f32; 5]>::try_from(numbers).ok()?;
            [x, y, z, pitch, yaw]
                .iter()
                .all(|n| n.is_finite())
                .then_some(Step::Place {
                    origin: [x, y, z],
                    angles: [pitch, yaw],
                })
        }
        "shot" => {
            let file = rest.trim();
            (!file.is_empty()).then(|| Step::Shot(PathBuf::from(file)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
