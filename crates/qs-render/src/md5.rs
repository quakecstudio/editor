// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-2001 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/r_alias.c, GLAlias_CreateShaders, R_DrawAliasModel

//! The 2021 rerelease's MD5 models, drawn in place of the `.mdl` models they replace as
//! alias models are drawn (REND-12): each frame's vertices from the joints, lit by
//! QuakeSpasm's shader formula for a vertex normal.

use qs_client::PoseBlend;
use qs_mdl::md5::{Md5Anim, Md5Error, Md5Mesh, Md5Pose, Md5Surface, md5_anim_path, md5_skin_path};

use crate::alias::{
    AliasEntity, AliasError, AliasVertex, SKIN_RATE, SKIN_SLOTS, Shading, rotate_for_entity, whole,
};
use crate::anorm_dots::SHADEDOT_QUANT;
use crate::mesh::{Image, Palette};
use crate::picture::load_image;
use crate::{AXES, CHANNEL_MAX, RGBA_BYTES};

/// Degrees in a turn, as `quantise` divides a yaw.
const FULL_TURN: f64 = 360.0;
/// `MAX_SKINS` in QuakeSpasm-Spiked's `gl_model.h`: the most skins a surface looks for.
const MAX_SKINS: u32 = 32;
/// `r_avertexnormal_dot`'s slope for a normal facing away from the light, "reproduces
/// anorm_dots within as reasonable a degree of tolerance" (REND-12.5).
const SHADE_AWAY: f32 = 13.0 / 44.0;
/// `radiansangle = (quantizedangle / 16.0) * 2.0 * 3.14159` (REND-12.5).
#[expect(
    clippy::approx_constant,
    reason = "spec REND-12.5: QuakeSpasm's literal 3.14159, not pi"
)]
const SHADE_PI: f32 = 3.14159;
const SHADE_STEPS: f32 = 16.0;

/// A joint's place in one frame: its position and its orientation quaternion, x, y, z, w.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Joint {
    position: [f32; 3],
    orientation: [f32; 4],
}

impl Joint {
    /// `v` turned by the joint's orientation, then moved to its position.
    fn transform(&self, v: [f32; 3]) -> [f32; 3] {
        let [x, y, z, w] = self.orientation;
        let q = [x, y, z];
        let t = cross(q, v).map(|c| 2.0 * c);
        let turned = add(add(v, t.map(|c| w * c)), cross(q, t));
        add(turned, self.position)
    }

    /// `child`, a joint placed relative to this one, in this one's space (REND-12.2).
    fn then(&self, child: &Self) -> Self {
        let [ax, ay, az, aw] = self.orientation;
        let [bx, by, bz, bw] = child.orientation;
        Self {
            position: self.transform(child.position),
            orientation: [
                aw * bx + ax * bw + ay * bz - az * by,
                aw * by - ax * bz + ay * bw + az * bx,
                aw * bz + ax * by - ay * bx + az * bw,
                aw * bw - ax * bx - ay * by - az * bz,
            ],
        }
    }
}

/// A vertex in one frame, in the model's space.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
}

/// One mesh of the model: its triangles' corners and its skins.
#[derive(Clone, Debug, PartialEq)]
struct Part {
    /// Each corner, three a triangle: its vertex and its skin coordinates.
    corners: Vec<(usize, [f32; 2])>,
    /// Each skin's four slots, indices into the model's images (REND-12.4).
    skins: Vec<[usize; SKIN_SLOTS]>,
}

/// An `.md5mesh` and its `.md5anim`, ready to draw (REND-12).
#[derive(Clone, Debug, PartialEq)]
pub struct Md5Model {
    parts: Vec<Part>,
    /// Each frame's vertices of each part.
    frames: Vec<Vec<Vec<Vertex>>>,
    images: Vec<Image>,
    shading: Shading,
}

impl Md5Model {
    /// The model `name` from its mesh file `bytes`, with its animation and skins read
    /// through `read`; `shading` is the `.mdl`'s it replaces, full light for a torch (REND-4.7,
    /// REND-12.1 to REND-12.4).
    ///
    /// # Errors
    ///
    /// [`AliasError::Md5`] for a mesh or an animation that does not read, or an animation
    /// that does not go with the mesh (MD5-2, MD5-3).
    pub fn read(
        name: &str,
        bytes: &[u8],
        read: &dyn Fn(&str) -> Option<Vec<u8>>,
        palette: &Palette,
        shading: Shading,
    ) -> Result<Self, AliasError> {
        let error = |e: Md5Error| AliasError::Md5(format!("{name}: {e}"));
        let mesh = Md5Mesh::read(bytes).map_err(error)?;
        let anim_path = md5_anim_path(name);
        let anim = match read(&anim_path) {
            Some(bytes) => {
                let anim = Md5Anim::read(&bytes).map_err(error)?;
                anim.check_mesh(&mesh).map_err(error)?;
                // REND-12.2: every frame is made here, so a count larger than the file is
                // refused, as MD5-3.11 bounds the components.
                if anim.num_frames > u64::try_from(bytes.len()).unwrap_or(u64::MAX) {
                    return Err(AliasError::Md5(format!("{anim_path} has too many frames")));
                }
                Some(anim)
            }
            None => None,
        };
        let poses = match &anim {
            Some(anim) => (0..anim.num_frames)
                .map(|frame| frame_joints(&mesh, anim, frame))
                .collect(),
            None => vec![bind_joints(&mesh)],
        };
        let mut images = Vec::new();
        let mut parts = Vec::new();
        for surface in &mesh.meshes {
            let skins = surface_skins(&surface.shader, read, palette, &mut images);
            let corners = (0..surface.num_tris)
                .filter_map(|t| surface.tri(t))
                .flatten()
                .filter_map(|v| {
                    let index = usize::try_from(v).ok()?;
                    Some((index, surface.vert(v)?.st))
                })
                .collect();
            parts.push(Part { corners, skins });
        }
        let frames = poses
            .iter()
            .map(|joints| {
                mesh.meshes
                    .iter()
                    .map(|surface| surface_vertices(surface, joints))
                    .collect()
            })
            .collect();
        Ok(Self {
            parts,
            frames,
            images,
            shading,
        })
    }

    /// Every skin image, each a texture to upload as an alias skin is.
    #[must_use]
    pub fn images(&self) -> &[Image] {
        &self.images
    }

    /// The model's frames: the animation's, or one, the mesh's own pose (REND-12.2).
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Appends `entity`'s triangles at `time` seconds to `out`, part by part, and returns
    /// each part's vertices and the index into [`Self::images`] it is drawn with (REND-12.3
    /// to REND-12.5).
    pub fn draw(
        &self,
        entity: &AliasEntity,
        time: f64,
        out: &mut Vec<AliasVertex>,
    ) -> Vec<(usize, usize)> {
        let shadelight = self.shading.light(entity.light);
        let light = shade_vector(entity.angles[1]);
        let (from, to, blend) = match entity.blend {
            Some(PoseBlend {
                previous,
                current,
                blend,
            }) => (previous, current, blend),
            None => (entity.frame, entity.frame, 0.0),
        };
        let frame = |index: usize| self.frames.get(index).or_else(|| self.frames.first());
        let (Some(from), to) = (frame(from), frame(to)) else {
            return Vec::new();
        };
        let to = to.unwrap_or(from);
        let [pitch, yaw, roll] = entity.angles;
        let turn = [-pitch, yaw, roll];
        let iblend = 1.0 - blend;
        let mut runs = Vec::new();
        for (index, part) in self.parts.iter().enumerate() {
            let start = out.len();
            let first = from.get(index).map_or(&[][..], Vec::as_slice);
            let second = to.get(index).map_or(first, Vec::as_slice);
            out.extend(part.corners.iter().filter_map(|&(vertex, texture)| {
                let one = *first.get(vertex)?;
                let two = second.get(vertex).copied().unwrap_or(one);
                let position = add(
                    one.position.map(|c| c * iblend),
                    two.position.map(|c| c * blend),
                );
                let shade =
                    normal_dot(one.normal, light) * iblend + normal_dot(two.normal, light) * blend;
                Some(AliasVertex {
                    position: rotate_for_entity(entity.origin, turn, position),
                    texture,
                    shade: (shade * shadelight).clamp(0.0, 1.0),
                })
            }));
            let slots = part.skins.get(entity.skin).or_else(|| part.skins.first());
            let slot = whole(time * SKIN_RATE) & (SKIN_SLOTS - 1);
            let image = slots.and_then(|s| s.get(slot)).copied().unwrap_or(0);
            runs.push((out.len().saturating_sub(start), image));
        }
        runs
    }
}

/// The mesh's joints as its file places them, absolute (MD5-2.4).
fn bind_joints(mesh: &Md5Mesh) -> Vec<Joint> {
    mesh.joints
        .iter()
        .map(|joint| Joint {
            position: joint.position,
            orientation: joint.orientation,
        })
        .collect()
}

/// Frame `frame`'s joints, each pose placed on its parent's, in order, as QuakeSpasm-Spiked
/// concatenates them "relative->abs"; a joint whose parent is not before it is placed as
/// if it had none (REND-12.2).
fn frame_joints(mesh: &Md5Mesh, anim: &Md5Anim, frame: u64) -> Vec<Joint> {
    let mut joints: Vec<Joint> = Vec::with_capacity(mesh.joints.len());
    for (index, joint) in mesh.joints.iter().enumerate() {
        let pose = anim.pose(frame, index).unwrap_or(Md5Pose {
            position: [0.0; AXES],
            orientation: [0.0, 0.0, 0.0, -1.0],
        });
        let local = Joint {
            position: pose.position,
            orientation: pose.orientation,
        };
        let parent = usize::try_from(joint.parent)
            .ok()
            .filter(|&p| p < index)
            .and_then(|p| joints.get(p));
        let placed = parent.map_or(local, |parent| parent.then(&local));
        joints.push(placed);
    }
    joints
}

/// Each vertex of `surface` with `joints`: the sum over its weights of `bias` times the
/// weight's position through its joint, and its normal, the normalised sum of the normals
/// of its triangles as `MD5_ComputeNormals` sums them, turned to point out of a triangle
/// wound as Quake winds its own (REND-12.2).
fn surface_vertices(surface: &Md5Surface, joints: &[Joint]) -> Vec<Vertex> {
    let positions: Vec<[f32; 3]> = (0..surface.num_verts)
        .map(|v| {
            let Some(vert) = surface.vert(v) else {
                return [0.0; AXES];
            };
            (0..vert.weight_count)
                .filter_map(|i| surface.weight(vert.first_weight.checked_add(i)?))
                .filter_map(|weight| {
                    let joint = joints.get(usize::try_from(weight.joint).ok()?)?;
                    Some(joint.transform(weight.position).map(|c| c * weight.bias))
                })
                .fold([0.0; AXES], add)
        })
        .collect();
    let mut normals = vec![[0.0_f32; AXES]; positions.len()];
    for triangle in (0..surface.num_tris).filter_map(|t| surface.tri(t)) {
        let [Ok(a), Ok(b), Ok(c)] = triangle.map(usize::try_from) else {
            continue;
        };
        let (Some(&p0), Some(&p1), Some(&p2)) =
            (positions.get(a), positions.get(b), positions.get(c))
        else {
            continue;
        };
        let normal = normalize(cross(sub(p2, p0), sub(p1, p0)));
        for vertex in [a, b, c] {
            if let Some(sum) = normals.get_mut(vertex) {
                *sum = add(*sum, normal);
            }
        }
    }
    positions
        .into_iter()
        .zip(normals)
        .map(|(position, normal)| Vertex {
            position,
            normal: normalize(normal),
        })
        .collect()
}

/// The skins of a part whose shader is `shader`: skin `n` frame `f` the image
/// `progs/<shader>_<nn>_<ff>`, frames from 0 to 3 and skins from 0 until one has no frame
/// 0, the four slots filled as QuakeSpasm-Spiked fills them; a part with none is white
/// (REND-12.4).
fn surface_skins(
    shader: &[u8],
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
    palette: &Palette,
    images: &mut Vec<Image>,
) -> Vec<[usize; SKIN_SLOTS]> {
    let mut skins = Vec::new();
    for skin in 0..MAX_SKINS {
        let mut found = Vec::new();
        for frame in 0..u32::try_from(SKIN_SLOTS).unwrap_or(0) {
            let path = String::from_utf8_lossy(&md5_skin_path(shader, skin, frame)).into_owned();
            let Some(image) = load_image(&path, read, palette) else {
                break;
            };
            found.push(images.len());
            images.push(image);
        }
        let slot = |i: usize| found.get(i).copied();
        let Some(first) = slot(0) else {
            break;
        };
        // "this stuff is hideous": fewer than four frames repeat the first two.
        let second = if found.len() < 2 {
            first
        } else {
            slot(1).unwrap_or(first)
        };
        skins.push(if found.len() < SKIN_SLOTS {
            [first, second, first, second]
        } else {
            [
                first,
                second,
                slot(2).unwrap_or(first),
                slot(SKIN_SLOTS - 1).unwrap_or(second),
            ]
        });
    }
    if skins.is_empty() {
        skins.push([images.len(); SKIN_SLOTS]);
        images.push(Image {
            width: 1,
            height: 1,
            rgba: vec![CHANNEL_MAX; RGBA_BYTES],
        });
    }
    skins
}

/// QuakeSpasm's `shadevector`: the yaw quantised to 16 steps as `shadedots` are, then
/// `(cos(-a), sin(-a), 1)` normalised (REND-12.5).
fn shade_vector(yaw: f32) -> [f32; 3] {
    let radians = (quantise(yaw) / SHADE_STEPS) * 2.0 * SHADE_PI;
    normalize([(-radians).cos(), (-radians).sin(), 1.0])
}

/// `((int)(yaw * (SHADEDOT_QUANT / 360.0))) & (SHADEDOT_QUANT - 1)`, in double as C
/// promotes it, then as the float `radiansangle` is made from.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "spec REND-12.5: C's (int) cast of the yaw's steps, then 0 to 15 as a float"
)]
fn quantise(yaw: f32) -> f32 {
    let steps = f64::from(yaw) * (f64::from(SHADE_STEPS) / FULL_TURN);
    let mask = i64::try_from(SHADEDOT_QUANT.saturating_sub(1)).unwrap_or(0);
    ((steps as i64) & mask) as f32
}

/// `r_avertexnormal_dot`: `1 + dot * 13/44` for a normal facing away from the light, else
/// `1 + dot` (REND-12.5).
fn normal_dot(normal: [f32; 3], light: [f32; 3]) -> f32 {
    let dot = normal.iter().zip(light).map(|(a, b)| a * b).sum::<f32>();
    if dot < 0.0 {
        1.0 + dot * SHADE_AWAY
    } else {
        1.0 + dot
    }
}

fn add([ax, ay, az]: [f32; 3], [bx, by, bz]: [f32; 3]) -> [f32; 3] {
    [ax + bx, ay + by, az + bz]
}

fn sub([ax, ay, az]: [f32; 3], [bx, by, bz]: [f32; 3]) -> [f32; 3] {
    [ax - bx, ay - by, az - bz]
}

fn cross([ax, ay, az]: [f32; 3], [bx, by, bz]: [f32; 3]) -> [f32; 3] {
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

/// `VectorNormalize`: `v` over its length, a zero vector left zero.
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let length = v.iter().map(|c| c * c).sum::<f32>().sqrt();
    if length > 0.0 {
        v.map(|c| c / length)
    } else {
        v
    }
}

#[cfg(test)]
pub(crate) mod tests;
