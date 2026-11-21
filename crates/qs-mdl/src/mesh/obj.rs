// SPDX-License-Identifier: GPL-3.0-or-later

//! Wavefront `.obj` files, read as a mesh of one pose (MDL-9.1): the geometric vertices
//! (`v`), the texture vertices (`vt`) and the faces (`f`) of the format's polygonal geometry.
//! Every other statement is passed over.

use core::fmt;

use super::{Corner, Mesh, MeshFrame};

/// The name of the one frame an `.obj` file gives.
pub const OBJ_FRAME: &str = "base";
/// The corners a face has at least: "A minimum of three vertices are required."
const FACE_MIN: usize = 3;

/// Which of a file's axes points up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Up {
    /// `y` is up and `-z` forward, as most programs write an `.obj`: a position `(x, y, z)`
    /// is Quake's `(-z, -x, y)`.
    #[default]
    Y,
    /// `z` is up and `x` forward, Quake's own axes.
    Z,
}

impl Up {
    /// A position of the file in Quake's axes.
    #[must_use]
    pub const fn turned(self, [x, y, z]: [f32; 3]) -> [f32; 3] {
        match self {
            Self::Y => [-z, -x, y],
            Self::Z => [x, y, z],
        }
    }
}

/// What is wrong with a line of an `.obj` file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjFault {
    /// A coordinate is no number, or a vertex has fewer than three.
    Number,
    /// A face has fewer than three corners.
    Face,
    /// A face's corner is no reference number, or refers to a vertex the file does not have
    /// above it.
    Reference,
}

/// Why an `.obj` file was not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjError {
    /// The line, from 1.
    pub line: usize,
    /// What is wrong with it.
    pub fault: ObjFault,
}

impl fmt::Display for ObjError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.fault {
            ObjFault::Number => "a vertex needs numbers, three for a position",
            ObjFault::Face => "a face needs three corners or more",
            ObjFault::Reference => "a face refers to a vertex the file does not have above it",
        };
        write!(f, "line {}: {what}", self.line)
    }
}

impl core::error::Error for ObjError {}

/// The numbers of a statement's arguments.
fn numbers<'a>(words: impl Iterator<Item = &'a str>) -> Option<Vec<f32>> {
    words
        .map(|word| word.parse::<f32>().ok().filter(|value| value.is_finite()))
        .collect()
}

/// The place in a list of `count` a reference number names: from 1 down the file, or below 0
/// back up it, "-1 indicates the vertex immediately above the element".
fn referred(word: &str, count: usize) -> Option<usize> {
    let number: i64 = word.parse().ok()?;
    let place = if number < 0 {
        i64::try_from(count).ok()?.checked_add(number)?
    } else {
        number.checked_sub(1)?
    };
    usize::try_from(place).ok().filter(|place| *place < count)
}

/// Reads an `.obj` file as a mesh of one pose, its axes turned as `up` says.
///
/// Implements MDL-9.1.
///
/// # Errors
///
/// [`ObjError`] with the line: a vertex without numbers, a face of fewer than three corners,
/// and a reference to a vertex the file does not have above the face.
pub fn read(text: &str, up: Up) -> Result<Mesh, ObjError> {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut mesh = Mesh::default();
    for (line, text) in (1_usize..).zip(text.lines()) {
        let fault = |fault| ObjError { line, fault };
        // "#" starts a comment.
        let statement = text.split('#').next().unwrap_or("");
        let mut words = statement.split_whitespace();
        match words.next() {
            Some("v") => {
                let values = numbers(words).ok_or_else(|| fault(ObjFault::Number))?;
                let (Some(&x), Some(&y), Some(&z)) = (values.first(), values.get(1), values.get(2))
                else {
                    return Err(fault(ObjFault::Number));
                };
                positions.push(up.turned([x, y, z]));
            }
            Some("vt") => {
                let values = numbers(words).ok_or_else(|| fault(ObjFault::Number))?;
                let u = values.first().copied().unwrap_or(0.0);
                // "v is the value for the vertical direction of the texture. The default is
                // 0." Ours: it is measured up from the picture's foot, so down from its top
                // it is 1 - v.
                let v = values.get(1).copied().unwrap_or(0.0);
                mesh.texcoords.push([u, 1.0 - v]);
            }
            Some("f") => {
                let mut corners = Vec::new();
                for word in words {
                    let mut parts = word.split('/');
                    let position = parts
                        .next()
                        .and_then(|part| referred(part, positions.len()))
                        .ok_or_else(|| fault(ObjFault::Reference))?;
                    let texcoord = match parts.next().filter(|part| !part.is_empty()) {
                        Some(part) => Some(
                            referred(part, mesh.texcoords.len())
                                .ok_or_else(|| fault(ObjFault::Reference))?,
                        ),
                        None => None,
                    };
                    corners.push(Corner { position, texcoord });
                }
                if corners.len() < FACE_MIN {
                    return Err(fault(ObjFault::Face));
                }
                // A face's corners go counterclockwise as seen from outside, and a model's
                // clockwise: a fan from the first corner, each triangle turned around.
                let Some((&first, rest)) = corners.split_first() else {
                    continue;
                };
                for pair in rest.windows(2) {
                    if let [second, third] = *pair {
                        mesh.triangles.push([first, third, second]);
                    }
                }
            }
            Some(_) | None => {}
        }
    }
    mesh.frames.push(MeshFrame {
        name: OBJ_FRAME.to_owned(),
        positions,
    });
    Ok(mesh)
}

#[cfg(test)]
mod tests;
