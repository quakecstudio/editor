// SPDX-License-Identifier: GPL-3.0-or-later

//! Inter-Quake Export files (`.iqe`), read as a mesh (MDL-9.2), from the format's own
//! description, `specs/iqe/iqe.txt`: the positions (`vp`), texture coordinates (`vt`) and
//! blends (`vb`) of its vertices, its faces (`fa`, `fm`), its skeleton (`joint`, `pq`, `pm`)
//! and its animations (`animation`, `frame`). An alias model has no skeleton: every frame of
//! every animation becomes a pose of the mesh, each vertex moved by the joints it is
//! blended to.

use core::fmt;

use super::Mesh;
use super::obj::Up;
use super::rig::{Affine, Boned, RigError, Vec3, scaled, single, turn};

/// What a file's first line starts with: "The first line of an IQE file must be: # Inter-Quake
/// Export".
pub const IQE_HEADER: &str = "# Inter-Quake Export";
/// The name of the one frame of a file without animations.
pub const IQE_FRAME: &str = "base";
/// The name of an animation the file gives none.
pub const IQE_ANIMATION: &str = "anim";
/// The corners of a triangle.
const CORNERS: usize = 3;

/// What is wrong with a line of an `.iqe` file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IqeFault {
    /// The file does not start with [`IQE_HEADER`].
    Header,
    /// An argument is no number.
    Number,
    /// A face has fewer than three corners, or one that is no vertex of the file.
    Face,
    /// The file has more animations of one name than can be told apart in a frame's name.
    Names,
    /// A joint's parent is not a joint above it.
    Joint,
    /// A `pa` pose: the description does not say in what order its three rotations apply.
    Angles,
    /// A joint's base pose flattens space, so nothing can be taken back out of it.
    Flat,
}

/// Why an `.iqe` file was not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IqeError {
    /// The line, from 1; 0 for the file as a whole.
    pub line: usize,
    /// What is wrong.
    pub fault: IqeFault,
}

impl fmt::Display for IqeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.fault {
            IqeFault::Header => "the file does not start with \"# Inter-Quake Export\"",
            IqeFault::Number => "an argument is no number",
            IqeFault::Face => "a face needs three corners, each a vertex of the file",
            IqeFault::Names => "too many animations have one name for their frames to be named",
            IqeFault::Joint => "a joint's parent must be a joint above it",
            IqeFault::Angles => {
                "a pa pose is not read: the format does not say in what order its rotations \
                 apply; write poses as pq"
            }
            IqeFault::Flat => "a joint's base pose has no size on an axis",
        };
        write!(f, "line {}: {what}", self.line)
    }
}

impl core::error::Error for IqeError {}

/// Where the parts of a `pq` pose's arguments start: its quaternion's x, y and z, its w, and
/// its scale, after the translation's three.
const PQ_ROTATION: usize = 3;
const PQ_W: usize = 6;
const PQ_SCALE: usize = 7;
/// Where the rows of a `pm` pose's rotation matrix start, and its scale.
const PM_ROWS: [usize; 3] = [3, 6, 9];
const PM_SCALE: usize = 12;

/// The three numbers of `values` from `from`, `or` for one not given.
fn three(values: &[f64], from: usize, or: f64) -> Vec3 {
    let at = |place: usize| values.get(place).copied().unwrap_or(or);
    [
        at(from),
        at(from.saturating_add(1)),
        at(from.saturating_add(2)),
    ]
}

/// A `pq` pose: "output = (input*scale)*rotation + translation". A quaternion is made a unit
/// one first; "If Qw is unspecified, it is generated such that it is a negative value".
fn pose_quaternion(values: &[f64]) -> Affine {
    let [x, y, z] = three(values, PQ_ROTATION, 0.0);
    let w = values
        .get(PQ_W)
        .copied()
        .unwrap_or_else(|| -(1.0 - x * x - y * y - z * z).max(0.0).sqrt());
    let rotation = turn([x, y, z, w]);
    scaled(
        rotation,
        three(values, PQ_SCALE, 1.0),
        three(values, 0, 0.0),
    )
}

/// A `pm` pose: its translation, the rows of its rotation matrix and its scale.
fn pose_matrix(values: &[f64]) -> Affine {
    scaled(
        PM_ROWS.map(|row| three(values, row, 0.0)),
        three(values, PM_SCALE, 1.0),
        three(values, 0, 0.0),
    )
}

/// A face as its line gives it, kept until the whole file is read: a face may come before
/// the vertices it names.
#[derive(Clone, Debug)]
struct Face {
    line: usize,
    /// Its corners' indexes as written.
    indexes: Vec<i64>,
    /// The vertex an index of 0 or more counts from: the file's first for `fa`, the mesh's
    /// first for `fm`.
    from: usize,
    /// How many vertices were defined above the face: an index below 0 counts back from
    /// there.
    above: usize,
}

/// An animation: its name and its frames, each a pose for each joint in turn.
#[derive(Clone, Debug, Default)]
struct Animation {
    name: String,
    frames: Vec<Vec<Affine>>,
}

/// The words of a line: split at spaces, a word in double quotes kept whole without them, as
/// the format's own exporter writes a name.
fn words(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = line.trim_start();
    while !rest.is_empty() {
        let (word, after) = match rest.strip_prefix('"') {
            Some(quoted) => quoted.split_once('"').unwrap_or((quoted, "")),
            None => rest.split_once(char::is_whitespace).unwrap_or((rest, "")),
        };
        out.push(word);
        rest = after.trim_start();
    }
    out
}

/// What is read of a file. Its vertex attributes are arrays of their own, which "may be
/// interleaved in any order, or not interleaved at all": the Nth of each is the Nth vertex's.
#[derive(Default)]
struct File {
    positions: Vec<Vec3>,
    texcoords: Vec<[f32; 2]>,
    /// For each `vb` in turn, the joints a vertex is blended to, each with its weight.
    blends: Vec<Vec<(usize, f64)>>,
    faces: Vec<Face>,
    /// Where each mesh's vertices start.
    meshes: Vec<usize>,
    /// Each joint's parent.
    parents: Vec<Option<usize>>,
    /// The base poses, one for each joint in turn.
    base: Vec<Affine>,
    animations: Vec<Animation>,
}

impl File {
    /// Keeps the face of `words`, whose indexes of 0 or more count from `from`: three
    /// corners or more, each a whole number.
    fn face(&mut self, line: usize, words: &[&str], from: usize) -> Option<()> {
        let indexes: Vec<i64> = words
            .iter()
            .map(|word| word.parse().ok())
            .collect::<Option<_>>()?;
        if indexes.len() < CORNERS {
            return None;
        }
        self.faces.push(Face {
            line,
            indexes,
            from,
            above: self.positions.len(),
        });
        Some(())
    }

    /// The triangles of the faces, now that every vertex is read: an index of 0 or more
    /// counts from the face's start, one below 0 back from the last vertex above the face,
    /// "-1" that one; more than three are "a convex, planar polygon", a fan.
    fn triangles(&self) -> Result<Vec<[usize; 3]>, IqeError> {
        let mut triangles = Vec::new();
        for face in &self.faces {
            let place = |index: i64| -> Option<usize> {
                let start = if index < 0 { face.above } else { face.from };
                let place = i64::try_from(start).ok()?.checked_add(index)?;
                usize::try_from(place)
                    .ok()
                    .filter(|at| *at < self.positions.len())
            };
            let corners: Vec<usize> = face
                .indexes
                .iter()
                .map(|index| place(*index))
                .collect::<Option<_>>()
                .ok_or(IqeError {
                    line: face.line,
                    fault: IqeFault::Face,
                })?;
            if let Some((&first, rest)) = corners.split_first() {
                for pair in rest.windows(2) {
                    if let [second, third] = *pair {
                        triangles.push([first, second, third]);
                    }
                }
            }
        }
        Ok(triangles)
    }

    /// The mesh.
    fn mesh(self, up: Up) -> Result<Mesh, IqeError> {
        let whole = |fault| IqeError { line: 0, fault };
        // "If there is no triangle data, then all vertex data ... is interpreted as mesh
        // local triangle data, where each group of 3 vertexes defines a triangle."
        let faces = if self.faces.is_empty() {
            let mut starts = self.meshes.clone();
            if starts.first() != Some(&0) {
                starts.insert(0, 0);
            }
            starts.push(self.positions.len());
            let mut faces = Vec::new();
            for pair in starts.windows(2) {
                if let [from, to] = *pair {
                    let mut at = from;
                    while at.saturating_add(CORNERS) <= to {
                        faces.push([at, at.saturating_add(1), at.saturating_add(2)]);
                        at = at.saturating_add(CORNERS);
                    }
                }
            }
            faces
        } else {
            self.triangles()?
        };
        // "Each vertex array must either be absent, or the same length as all other vertex
        // arrays": a vertex has the texture coordinate of its own number.
        let textured = self.texcoords.len() == self.positions.len();
        let triangles = faces;
        let blended = self.blends.len() == self.positions.len();
        Boned {
            positions: self.positions,
            texcoords: if textured { self.texcoords } else { Vec::new() },
            blends: if blended { self.blends } else { Vec::new() },
            triangles,
            parents: self.parents,
            base: self.base,
            animations: self
                .animations
                .into_iter()
                .map(|animation| (animation.name, animation.frames))
                .collect(),
            bind: Vec::new(),
            rest: IQE_FRAME,
            unnamed: IQE_ANIMATION,
        }
        .mesh(up)
        .map_err(|error| {
            whole(match error {
                RigError::Flat => IqeFault::Flat,
                RigError::Names => IqeFault::Names,
            })
        })
    }
}

/// Reads an `.iqe` file as a mesh, its axes turned as `up` says: a pose for each frame of
/// each of its animations, named by the animation and the frame's number from 1, or one pose
/// of the mesh as it is where the file has no animation.
///
/// Implements MDL-9.2.
///
/// # Errors
///
/// [`IqeError`] with the line: a file without the header, an argument that is no number, a
/// face without three vertices of the file, a joint whose parent is not above it, a `pa`
/// pose, and a base pose that flattens space.
pub fn read(text: &str, up: Up) -> Result<Mesh, IqeError> {
    if !text.starts_with(IQE_HEADER) {
        return Err(IqeError {
            line: 1,
            fault: IqeFault::Header,
        });
    }
    let mut file = File::default();
    for (line, text) in (1_usize..).zip(text.lines()) {
        let fault = |fault| IqeError { line, fault };
        // "Any line beginning with # is a comment".
        if text.starts_with('#') {
            continue;
        }
        let words = words(text);
        let Some((&command, arguments)) = words.split_first() else {
            continue;
        };
        let numbers = || -> Result<Vec<f64>, IqeError> {
            arguments
                .iter()
                .map(|word| word.parse::<f64>().ok().filter(|value| value.is_finite()))
                .collect::<Option<_>>()
                .ok_or_else(|| fault(IqeFault::Number))
        };
        let at = |values: &[f64], place: usize| values.get(place).copied().unwrap_or(0.0);
        match command {
            "vp" => {
                let values = numbers()?;
                file.positions
                    .push([at(&values, 0), at(&values, 1), at(&values, 2)]);
            }
            "vt" => {
                let values = numbers()?;
                let [u, v, _] = single([at(&values, 0), at(&values, 1), 0.0]);
                file.texcoords.push([u, v]);
            }
            "vb" => {
                // "Blend indexes ... and blend weights", in pairs; one of no weight is unused.
                let mut blends = Vec::new();
                for [joint, weight] in arguments.as_chunks::<2>().0 {
                    let joint: usize = joint.parse().map_err(|_| fault(IqeFault::Number))?;
                    let weight: f64 = weight.parse().map_err(|_| fault(IqeFault::Number))?;
                    if weight > 0.0 && weight.is_finite() {
                        blends.push((joint, weight));
                    }
                }
                file.blends.push(blends);
            }
            "mesh" => file.meshes.push(file.positions.len()),
            "fa" => file
                .face(line, arguments, 0)
                .ok_or_else(|| fault(IqeFault::Face))?,
            "fm" => {
                let from = file.meshes.last().copied().unwrap_or(0);
                file.face(line, arguments, from)
                    .ok_or_else(|| fault(IqeFault::Face))?;
            }
            "joint" => {
                // "If PARENT is less than 0 or unspecified, then the joint has no parent."
                let parent = match arguments.get(1) {
                    Some(word) => word.parse::<i64>().map_err(|_| fault(IqeFault::Number))?,
                    None => -1,
                };
                let parent = match usize::try_from(parent) {
                    Ok(parent) if parent < file.parents.len() => Some(parent),
                    Ok(_) => return Err(fault(IqeFault::Joint)),
                    Err(_) => None,
                };
                file.parents.push(parent);
            }
            "pq" | "pm" => {
                let values = numbers()?;
                let pose = if command == "pq" {
                    pose_quaternion(&values)
                } else {
                    pose_matrix(&values)
                };
                // Poses before any animation are the base poses; after one they are its
                // current frame's.
                match file.animations.last_mut() {
                    Some(animation) => {
                        if animation.frames.is_empty() {
                            animation.frames.push(Vec::new());
                        }
                        if let Some(frame) = animation.frames.last_mut() {
                            frame.push(pose);
                        }
                    }
                    None => file.base.push(pose),
                }
            }
            "pa" => return Err(fault(IqeFault::Angles)),
            "animation" => file.animations.push(Animation {
                name: arguments.first().copied().unwrap_or("").to_owned(),
                frames: Vec::new(),
            }),
            "frame" => {
                if let Some(animation) = file.animations.last_mut() {
                    animation.frames.push(Vec::new());
                }
            }
            // "All text starting on the line after the "comment" command, up to the end of
            // the file, is treated verbatim as the comment section."
            "comment" => break,
            _ => {}
        }
    }
    file.mesh(up)
}

#[cfg(test)]
mod tests;
