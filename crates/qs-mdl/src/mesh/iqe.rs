// SPDX-License-Identifier: GPL-3.0-or-later

//! Inter-Quake Export files (`.iqe`), read as a mesh (MDL-9.2), from the format's own
//! description, `specs/iqe/iqe.txt`: the positions (`vp`), texture coordinates (`vt`) and
//! blends (`vb`) of its vertices, its faces (`fa`, `fm`), its skeleton (`joint`, `pq`, `pm`)
//! and its animations (`animation`, `frame`). An alias model has no skeleton: every frame of
//! every animation becomes a pose of the mesh, each vertex moved by the joints it is
//! blended to.

use core::fmt;

use super::obj::Up;
use super::{Corner, Mesh, MeshFrame};

/// What a file's first line starts with: "The first line of an IQE file must be: # Inter-Quake
/// Export".
pub const IQE_HEADER: &str = "# Inter-Quake Export";
/// The name of the one frame of a file without animations.
pub const IQE_FRAME: &str = "base";
/// The name of an animation the file gives none.
pub const IQE_ANIMATION: &str = "anim";
/// The most bytes of a frame's name (MDL-6.3).
const NAME_MAX: usize = 15;
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

/// Three numbers.
type Vec3 = [f64; 3];
/// A matrix of three rows.
type Mat3 = [Vec3; 3];

/// A rotation and scale followed by a move: what a pose does to a point.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Affine {
    matrix: Mat3,
    moved: Vec3,
}

/// The dot product.
fn dot([ax, ay, az]: Vec3, [bx, by, bz]: Vec3) -> f64 {
    ax * bx + ay * by + az * bz
}

/// A matrix's columns as rows.
const fn transposed([[a, b, c], [d, e, f], [g, h, i]]: Mat3) -> Mat3 {
    [[a, d, g], [b, e, h], [c, f, i]]
}

/// The cross product.
fn cross([ax, ay, az]: Vec3, [bx, by, bz]: Vec3) -> Vec3 {
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

impl Affine {
    const IDENTITY: Self = Self {
        matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        moved: [0.0; 3],
    };

    /// Where `point` goes.
    fn apply(&self, point: Vec3) -> Vec3 {
        let [x, y, z] = self.matrix.map(|row| dot(row, point));
        let [mx, my, mz] = self.moved;
        [x + mx, y + my, z + mz]
    }

    /// `self` after `first`: what a joint's pose is in its parent's space.
    fn after(&self, first: &Self) -> Self {
        let columns = transposed(first.matrix);
        Self {
            matrix: self
                .matrix
                .map(|row| columns.map(|column| dot(row, column))),
            moved: self.apply(first.moved),
        }
    }

    /// What undoes `self`; `None` when it flattens space.
    fn inverse(&self) -> Option<Self> {
        let [a, b, c] = self.matrix;
        let determinant = dot(a, cross(b, c));
        if determinant.abs() < f64::MIN_POSITIVE {
            return None;
        }
        // The inverse's columns are the cross products of the rows, over the determinant.
        let matrix = transposed([cross(b, c), cross(c, a), cross(a, b)])
            .map(|row| row.map(|part| part / determinant));
        let [x, y, z] = matrix.map(|row| dot(row, self.moved));
        Some(Self {
            matrix,
            moved: [-x, -y, -z],
        })
    }
}

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
    let length = (x * x + y * y + z * z + w * w).sqrt();
    let (x, y, z, w) = if length > 0.0 {
        (x / length, y / length, z / length, w / length)
    } else {
        (0.0, 0.0, 0.0, 1.0)
    };
    let rotation: Mat3 = [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ];
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

/// The pose that scales a point, then turns it by `rotation`, then moves it by `moved`.
fn scaled(rotation: Mat3, [sx, sy, sz]: Vec3, moved: Vec3) -> Affine {
    Affine {
        matrix: rotation.map(|[a, b, c]| [a * sx, b * sy, c * sz]),
        moved,
    }
}

/// A vertex of the file.
#[derive(Clone, Debug, Default)]
struct Vertex {
    position: Vec3,
    /// The joints it is blended to, each with its weight.
    blends: Vec<(usize, f64)>,
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

/// A single-precision position.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec MDL-9.2: a posed position is stored in single precision, the nearest"
)]
fn single(point: Vec3) -> [f32; 3] {
    point.map(|part| part as f32)
}

/// What is read of a file.
#[derive(Default)]
struct File {
    vertices: Vec<Vertex>,
    texcoords: Vec<[f32; 2]>,
    /// The faces, each three vertices.
    faces: Vec<[usize; 3]>,
    /// Where each mesh's vertices start.
    meshes: Vec<usize>,
    /// Each joint's parent.
    parents: Vec<Option<usize>>,
    /// The base poses, one for each joint in turn.
    base: Vec<Affine>,
    animations: Vec<Animation>,
}

impl File {
    /// A face's corners as vertices: counted from `from`, or below 0 back from the last
    /// vertex, "-1" the last; more than three are "a convex, planar polygon", a fan.
    fn face(&mut self, words: &[&str], from: usize) -> Option<()> {
        let count = i64::try_from(self.vertices.len()).ok()?;
        let from = i64::try_from(from).ok()?;
        let mut corners = Vec::with_capacity(words.len());
        for word in words {
            let index = word.parse::<i64>().ok()?;
            let place = if index < 0 {
                count.checked_add(index)?
            } else {
                from.checked_add(index)?
            };
            corners.push(
                usize::try_from(place)
                    .ok()
                    .filter(|at| *at < self.vertices.len())?,
            );
        }
        let (&first, rest) = corners.split_first()?;
        if rest.len() < CORNERS.saturating_sub(1) {
            return None;
        }
        for pair in rest.windows(2) {
            if let [second, third] = *pair {
                self.faces.push([first, second, third]);
            }
        }
        Some(())
    }

    /// Each joint's pose in the model's space, of `poses`, each in its parent's: a joint the
    /// frame gives no pose keeps its base pose.
    fn world(&self, poses: &[Affine]) -> Vec<Affine> {
        let mut world: Vec<Affine> = Vec::with_capacity(self.parents.len());
        for (joint, parent) in self.parents.iter().enumerate() {
            let own = poses
                .get(joint)
                .or_else(|| self.base.get(joint))
                .copied()
                .unwrap_or(Affine::IDENTITY);
            let placed = parent
                .and_then(|parent| world.get(parent))
                .map_or(own, |parent| parent.after(&own));
            world.push(placed);
        }
        world
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
            starts.push(self.vertices.len());
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
            self.faces.clone()
        };
        // "Each vertex array must either be absent, or the same length as all other vertex
        // arrays": a vertex has the texture coordinate of its own number.
        let textured = self.texcoords.len() == self.vertices.len();
        let triangles = faces
            .into_iter()
            .map(|face| {
                face.map(|vertex| Corner {
                    position: vertex,
                    texcoord: textured.then_some(vertex),
                })
            })
            .collect();
        let bind = self.world(&self.base);
        let unbind: Vec<Affine> = bind
            .iter()
            .map(|joint| joint.inverse().ok_or_else(|| whole(IqeFault::Flat)))
            .collect::<Result<_, _>>()?;
        let posed = |poses: &[Affine]| -> Vec<[f32; 3]> {
            let world = self.world(poses);
            // What each joint does to a point of the mesh as it was bound.
            let moves: Vec<Affine> = world
                .iter()
                .zip(&unbind)
                .map(|(now, unbind)| now.after(unbind))
                .collect();
            self.vertices
                .iter()
                .map(|vertex| {
                    let total: f64 = vertex.blends.iter().map(|(_, weight)| weight).sum();
                    let mut at = [0.0, 0.0, 0.0];
                    for (joint, weight) in &vertex.blends {
                        let Some(moved) = moves.get(*joint) else {
                            continue;
                        };
                        for (sum, part) in at.iter_mut().zip(moved.apply(vertex.position)) {
                            *sum += part * weight / total;
                        }
                    }
                    let blended =
                        total > 0.0 && vertex.blends.iter().all(|(joint, _)| *joint < moves.len());
                    up.turned(single(if blended { at } else { vertex.position }))
                })
                .collect()
        };
        let mut frames = Vec::new();
        for animation in &self.animations {
            for (number, poses) in (1_usize..).zip(&animation.frames) {
                frames.push(MeshFrame {
                    name: frame_name(&animation.name, number),
                    positions: posed(poses),
                });
            }
        }
        if frames.is_empty() {
            frames.push(MeshFrame {
                name: IQE_FRAME.to_owned(),
                positions: posed(&self.base),
            });
        }
        Ok(Mesh {
            triangles,
            texcoords: if textured { self.texcoords } else { Vec::new() },
            frames,
        })
    }
}

/// The name of frame `number` of the animation `animation`: its name and the number, as
/// QuakeC names frames (`run1`, `run2`), the name cut to what a frame's name holds.
fn frame_name(animation: &str, number: usize) -> String {
    let digits = number.to_string();
    let room = NAME_MAX.saturating_sub(digits.len());
    let mut name: String = animation
        .chars()
        .filter(|letter| letter.is_ascii_alphanumeric() || *letter == '_')
        .collect();
    name.truncate(room);
    if name.is_empty() {
        name.push_str(IQE_ANIMATION);
    }
    name.push_str(&digits);
    name
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
                file.vertices.push(Vertex {
                    position: [at(&values, 0), at(&values, 1), at(&values, 2)],
                    blends: Vec::new(),
                });
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
                if let Some(vertex) = file.vertices.last_mut() {
                    vertex.blends = blends;
                }
            }
            "mesh" => file.meshes.push(file.vertices.len()),
            "fa" => file
                .face(arguments, 0)
                .ok_or_else(|| fault(IqeFault::Face))?,
            "fm" => {
                let from = file.meshes.last().copied().unwrap_or(0);
                file.face(arguments, from)
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
