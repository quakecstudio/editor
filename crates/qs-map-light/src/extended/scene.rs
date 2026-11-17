// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2016 Eric Wasylishen
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: common/bsputils.cc, Face_VertexAtIndex, Face_Plane, Face_TextureName, TextureName_Contents, ContentsOrSurfaceFlags_IsTranslucent, Light_PointInSolid_r, Light_PointInSolid
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/light.cc, FindModelInfo, ModelInfoForFace, CacheTextures
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/ltface.cc, Light_PointInAnySolid, Face_IsLightmapped
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/trace.cc, Light_PointInLeaf, SampleTexture, clamp_texcoord
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/trace_embree.cc, Embree_TraceInit, CreateGeometry, Face_Alpha
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: common/imglib.cc, calculate_average

//! The map as the extended dialect's lighting sees it (LIGHT-8, LIGHT-9): the BSP's faces with
//! their points, planes and textures, the brush models with their settings, and the flags
//! each texinfo carries from the BSP compiler.

use qs_bsp::{BSP_TEX_SPECIAL, Bsp, Face};
use qs_map_bsp::{LeafKind, SurfaceFlags};

use crate::extended::keys::CHANNEL_DEFAULT;
use crate::extended::keys::{Dict, ModelKeys, Source, World};
use crate::extended::math::{Plane, add, sub};
use crate::extended::rays::{Caster, Corners, ModelId, Surface, Texel, Texels};
use crate::extended::vis::Visibility;
use crate::face::FaceId;
use crate::{AXES, Vec3};

/// The leaf contents light cares for (BSP-12).
const CONTENTS_SOLID: i32 = -2;
const CONTENTS_SKY: i32 = -6;
/// How near a node's plane a point is tried on both sides of it (LIGHT-12.7).
const ON_PLANE: f64 = 0.1;
/// How far a point in solid is moved along each axis to find open space (LIGHT-10.7).
const OUT_OF_SOLID: f32 = 2.0;
/// `LMSCALE_DEFAULT`: texels to a luxel.
pub(crate) const LUXEL: f32 = 16.0;
/// A texel index that is see-through in a texture whose name starts with `{`.
const FENCE_HOLE: u8 = 255;
/// A texel that is not see-through.
const OPAQUE: u8 = 255;
/// The widest phong angle a model gives its faces: ericw-tools keeps it in a byte.
const PHONG_ANGLE_MAX: f32 = 255.0;
/// The bytes of a palette's colour.
const PALETTE_ENTRY: usize = 3;

/// What a face's texture name makes of it (`TextureName_Contents`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Skin {
    /// A name starting with `sky`.
    Sky,
    /// A name starting with `*`: water, slime or lava.
    Liquid,
    /// Any other.
    Solid,
}

/// A brush model with its settings (`modelinfo_t`, LIGHT-9.7).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Model {
    pub(crate) keys: ModelKeys,
    /// Its entity's `origin`, which its faces are moved by.
    pub(crate) offset: Vec3,
    /// The entity's `targetname`.
    pub(crate) targetname: Vec<u8>,
}

/// The map: the BSP, its models and its faces' flags.
pub(crate) struct Map<'a> {
    pub(crate) bsp: &'a Bsp<'a>,
    pub(crate) world: World,
    pub(crate) models: Vec<Model>,
    /// The flags of each texinfo, a model's `_phong` among them (LIGHT-13.1).
    pub(crate) flags: Vec<SurfaceFlags>,
    /// The model of each face; none for a face of no model.
    face_model: Vec<Option<usize>>,
    /// What each leaf is to a sample (LIGHT-9.9); a leaf past the end is as the BSP says.
    leaves: &'a [LeafKind],
    /// The game's palette, for the colour of glass (LIGHT-11.6).
    palette: Option<&'a [u8]>,
    /// The BSP's visibility, when it has been through vis (LIGHT-10.10).
    pub(crate) visibility: Option<Visibility>,
}

/// What a map is given besides its BSP, entities and flags.
pub(crate) struct Given<'a> {
    /// The names of the models whose shadows a light switches, each with the light's style
    /// (LIGHT-11.5).
    pub(crate) switches: Vec<(Vec<u8>, i32)>,
    /// What each leaf is to a sample (LIGHT-9.9).
    pub(crate) leaves: &'a [LeafKind],
    /// The game's palette (LIGHT-11.6).
    pub(crate) palette: Option<&'a [u8]>,
}

/// What stops the run when the map cannot be lit as ericw-tools reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MapFault {
    /// `Couldn't find entity for model *<n>`.
    NoEntityForModel(usize),
}

/// Whether two names are the same but for case (`Q_strcasecmp`).
fn same_name(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The vertex a face's surfedge names: an edge's first vertex, or its second when the edge is
/// walked backward.
fn vertex_of(bsp: &Bsp<'_>, surfedge: i32) -> Option<usize> {
    let (edge, end) = if surfedge < 0 {
        (surfedge.checked_neg()?, 1)
    } else {
        (surfedge, 0)
    };
    let [a, b] = *bsp.edges.get(usize::try_from(edge).ok()?)?;
    usize::try_from(if end == 0 { a } else { b }).ok()
}

impl<'a> Map<'a> {
    /// `FindModelInfo`: the models of `bsp` with the settings of their entities among
    /// `entities`, the first of which is the world's (LIGHT-9.7).
    pub(crate) fn new(
        bsp: &'a Bsp<'a>,
        world: World,
        entities: &[Dict],
        mut flags: Vec<SurfaceFlags>,
        given: &Given<'a>,
    ) -> Result<Self, MapFault> {
        flags.resize(bsp.texinfo.len(), SurfaceFlags::default());
        let mut models = Vec::with_capacity(bsp.models.len());
        let mut world_keys = ModelKeys::default();
        // The world always casts shadows, and its phong angle is the worldspawn's.
        world_keys.shadow.set(1.0, Source::Map);
        if world.phong_angle.is_changed() {
            world_keys
                .phong_angle
                .set(world.phong_angle.get(), Source::Map);
        }
        models.push(Model {
            keys: world_keys,
            offset: [0.0; AXES],
            targetname: Vec::new(),
        });
        for index in 1..bsp.models.len() {
            let name = format!("*{index}");
            let entity = entities
                .iter()
                .find(|entity| entity.get(b"model") == name.as_bytes())
                .ok_or(MapFault::NoEntityForModel(index))?;
            let mut keys = ModelKeys::read(entity);
            let targetname = entity.get(b"targetname").to_vec();
            // A light with `_switchableshadow_target` switches the shadow of the model of
            // that name, by the light's style (LIGHT-11.5).
            if let Some((_, style)) = given.switches.iter().find(|(name, _)| *name == targetname) {
                keys.switchableshadow.set(1.0, Source::Default);
                keys.switchshadstyle.set(*style, Source::Default);
            }
            models.push(Model {
                keys,
                offset: entity.vector(b"origin"),
                targetname,
            });
        }
        let mut face_model = vec![None; bsp.faces.len()];
        for (index, model) in bsp.models.iter().enumerate() {
            let faces = face_model
                .get_mut(model.firstface..model.firstface.saturating_add(model.numfaces))
                .unwrap_or_default();
            for slot in faces {
                // The first model that holds a face is its model.
                slot.get_or_insert(index);
            }
        }
        // `CalculateVertexNormals`: a model's phong angle is every one of its faces'
        // texinfos' (LIGHT-13.1).
        for (model, about) in bsp.models.iter().zip(&models) {
            let angle = about
                .keys
                .resolved_phong_angle()
                .round_ties_even()
                .clamp(0.0, PHONG_ANGLE_MAX);
            if angle < 1.0 {
                continue;
            }
            let faces = bsp
                .faces
                .get(model.firstface..model.firstface.saturating_add(model.numfaces))
                .unwrap_or_default();
            for face in faces {
                if let Some(slot) = flags.get_mut(face.texinfo) {
                    slot.phong_angle = angle;
                }
            }
        }
        Ok(Self {
            bsp,
            world,
            models,
            flags,
            face_model,
            leaves: given.leaves,
            palette: given.palette,
            visibility: Visibility::new(bsp),
        })
    }

    /// The face numbered `index`.
    pub(crate) fn face(&self, face: FaceId) -> Option<&Face> {
        self.bsp.faces.get(face.index())
    }

    /// `ModelInfoForFace`: the number of the model the face is in.
    pub(crate) fn model_of(&self, face: usize) -> Option<usize> {
        self.face_model.get(face).copied().flatten()
    }

    /// The flags of a face's texinfo.
    pub(crate) fn flags_of(&self, face: &Face) -> SurfaceFlags {
        self.flags.get(face.texinfo).copied().unwrap_or_default()
    }

    /// `Face_VertexAtIndex`: the number of the face's vertex `at`.
    pub(crate) fn vertex_at(&self, face: &Face, at: usize) -> Option<usize> {
        let surfedge = *self.bsp.surfedges.get(face.firstedge.checked_add(at)?)?;
        vertex_of(self.bsp, surfedge)
    }

    /// The numbers of the face's vertices, in order.
    pub(crate) fn vertices(&self, face: &Face) -> Vec<usize> {
        (0..face.numedges)
            .filter_map(|at| self.vertex_at(face, at))
            .collect()
    }

    /// `Face_Points`: the face's corners, in order.
    pub(crate) fn points(&self, face: &Face) -> Vec<Vec3> {
        self.vertices(face)
            .into_iter()
            .filter_map(|vertex| self.bsp.vertexes.get(vertex).copied())
            .collect()
    }

    /// `Face_Plane`: the face's plane, facing the way the face does.
    pub(crate) fn plane(&self, face: &Face) -> Plane {
        let plane = self
            .bsp
            .planes
            .get(face.planenum)
            .map_or([0.0; AXES + 1], |p| {
                [p.normal[0], p.normal[1], p.normal[2], p.dist]
            });
        let [x, y, z, dist] = plane;
        let plane = Plane {
            normal: [x, y, z],
            dist,
        };
        if face.side == 0 {
            plane
        } else {
            plane.flipped()
        }
    }

    /// `Face_TextureName`: the name of the face's texture, empty when it has none.
    pub(crate) fn texture_name(&self, face: &Face) -> &[u8] {
        self.texture(face).map_or(&[], |t| t.name.as_slice())
    }

    fn texture(&self, face: &Face) -> Option<&qs_bsp::Texture<'a>> {
        let slot = self.bsp.texinfo.get(face.texinfo)?.texture?;
        self.bsp.textures.get(slot)?.as_ref()
    }

    /// What the face's texture name makes of it.
    pub(crate) fn skin(&self, face: &Face) -> Skin {
        let name = self.texture_name(face);
        if name
            .get(..AXES)
            .is_some_and(|start| start.eq_ignore_ascii_case(b"sky"))
        {
            Skin::Sky
        } else if name.first() == Some(&b'*') {
            Skin::Liquid
        } else {
            Skin::Solid
        }
    }

    /// `Face_IsLightmapped`: whether the face has a lightmap: it has a texinfo without
    /// `TEX_SPECIAL`, and its texture is neither `trigger` nor `skip` (LIGHT-8.3).
    pub(crate) fn is_lightmapped(&self, face: &Face) -> bool {
        let Some(texinfo) = self.bsp.texinfo.get(face.texinfo) else {
            return false;
        };
        let name = self.texture_name(face);
        !same_name(name, b"trigger")
            && !same_name(name, b"skip")
            && texinfo.flags & BSP_TEX_SPECIAL == 0
    }

    /// Whether `point` is in a solid or sky leaf of the model's hull 0, trying both sides of
    /// a node whose plane the point is within 0.1 of (`Light_PointInSolid`, LIGHT-12.7).
    pub(crate) fn in_solid(&self, model: usize, point: Vec3) -> bool {
        let Some(head) = self
            .bsp
            .models
            .get(model)
            .and_then(|m| m.headnode.first().copied())
        else {
            return false;
        };
        let mut stack = vec![head];
        while let Some(at) = stack.pop() {
            let Ok(node) = usize::try_from(at) else {
                // A leaf is -1 less its number.
                let leaf = usize::try_from(at.saturating_add(1).saturating_neg()).ok();
                let kind = leaf
                    .and_then(|leaf| self.leaves.get(leaf))
                    .copied()
                    .unwrap_or_default();
                let contents = leaf
                    .and_then(|leaf| self.bsp.leafs.get(leaf))
                    .map_or(0, |leaf| leaf.contents);
                let closed = match kind {
                    LeafKind::Closed => true,
                    LeafKind::Open => false,
                    LeafKind::Native => contents == CONTENTS_SOLID || contents == CONTENTS_SKY,
                };
                if closed {
                    return true;
                }
                continue;
            };
            let Some(node) = self.bsp.nodes.get(node) else {
                continue;
            };
            let [front, back] = node.children;
            let dist = self.bsp.planes.get(node.planenum).map_or(0.0, |plane| {
                let n = plane.normal;
                f64::from(n[0]) * f64::from(point[0])
                    + f64::from(n[1]) * f64::from(point[1])
                    + f64::from(n[2]) * f64::from(point[2])
                    - f64::from(plane.dist)
            });
            if dist > ON_PLANE {
                stack.push(front);
            } else if dist < -ON_PLANE {
                stack.push(back);
            } else {
                stack.push(back);
                stack.push(front);
            }
        }
        false
    }

    /// `FixLightOnFace`: `point`, or when it is in the world's solid the first of the six
    /// points 2 units from it along an axis, back before forward, that is not; none when
    /// none is (LIGHT-10.7).
    pub(crate) fn out_of_solid(&self, point: Vec3) -> Option<Vec3> {
        if !self.in_solid(0, point) {
            return Some(point);
        }
        (0..AXES)
            .flat_map(|axis| [(axis, -OUT_OF_SOLID), (axis, OUT_OF_SOLID)])
            .map(|(axis, by)| {
                let mut moved = point;
                if let Some(coordinate) = moved.get_mut(axis) {
                    *coordinate += by;
                }
                moved
            })
            .find(|moved| !self.in_solid(0, *moved))
    }

    /// The models that always cast shadow: the world, and each with `_shadow` that is not a
    /// switchable shadow (`tracelist`).
    pub(crate) fn shadow_models(&self) -> impl Iterator<Item = (usize, &Model)> {
        self.models.iter().enumerate().filter(|(index, model)| {
            *index == 0 || (!model.keys.switchableshadow.is_on() && model.keys.shadow.is_on())
        })
    }

    /// `Light_PointInAnySolid`: whether a sample of `model` at `point` is in solid: in its own
    /// model, in the world when the model is on the default channel, or in a model of its
    /// channel that always casts shadow and is opaque (LIGHT-12.7).
    #[expect(
        clippy::float_cmp,
        reason = "spec LIGHT-12.7: only a model whose alpha is exactly 1 hides a sample"
    )]
    pub(crate) fn in_any_solid(&self, model: usize, point: Vec3) -> bool {
        if self.in_solid(model, point) {
            return true;
        }
        let channels = self
            .models
            .get(model)
            .map_or(1, |m| m.keys.object_channel_mask.get());
        if channels == 1 && self.in_solid(0, point) {
            return true;
        }
        self.shadow_models().any(|(index, caster)| {
            caster.keys.object_channel_mask.get() == channels
                && caster.keys.alpha.get() == 1.0
                && self.in_solid(index, sub(point, caster.offset))
        })
    }
}

impl Map<'_> {
    /// `Embree_TraceInit`: the triangles of every face that casts shadow, each with what it
    /// is to light (LIGHT-11.1, LIGHT-11.2).
    pub(crate) fn casters(&self) -> Vec<Corners> {
        let mut out = Vec::new();
        for (index, (model, about)) in self.bsp.models.iter().zip(&self.models).enumerate() {
            let keys = &about.keys;
            let world = index == 0;
            let switchable = keys.switchableshadow.is_on();
            let own_channels = keys.object_channel_mask.get();
            let casts = world
                || keys.shadow.is_on()
                || keys.shadowself.is_on()
                || keys.shadowworldonly.is_on()
                || switchable
                || own_channels != CHANNEL_DEFAULT;
            if !casts {
                continue;
            }
            for number in model.firstface..model.firstface.saturating_add(model.numfaces) {
                let Some(face) = self.face(FaceId::new(number)) else {
                    continue;
                };
                let flags = self.flags_of(face);
                if flags.no_shadow || self.model_of(number).is_none() {
                    continue;
                }
                let alpha = flags.light_alpha.unwrap_or_else(|| {
                    if keys.alpha.is_changed() {
                        keys.alpha.get()
                    } else {
                        1.0
                    }
                });
                let fence = self.texture_name(face).first() == Some(&b'{');
                let channels = flags.object_channel_mask.unwrap_or(own_channels);
                let skin = self.skin(face);
                let surface = if switchable
                    || own_channels != CHANNEL_DEFAULT
                    || channels != CHANNEL_DEFAULT
                    || alpha < 1.0
                    || fence
                {
                    Surface::Filtered
                } else if skin == Skin::Sky {
                    Surface::Sky
                } else if skin == Skin::Liquid {
                    // The world's liquids cast no shadow; a shadow-casting model's do.
                    if world {
                        continue;
                    }
                    Surface::Solid
                } else if world || keys.shadow.is_on() {
                    Surface::Solid
                } else {
                    Surface::Filtered
                };
                let caster = Caster {
                    model: ModelId(index),
                    face: number,
                    surface,
                    world_only: keys.shadowworldonly.is_on(),
                    self_only: keys.shadowself.is_on(),
                    switch_style: switchable.then(|| keys.switchshadstyle.get()),
                    channels,
                    alpha,
                    fence,
                    glass: alpha < 1.0,
                };
                let points: Vec<Vec3> = self
                    .points(face)
                    .into_iter()
                    .map(|point| add(point, about.offset))
                    .collect();
                let Some(&first) = points.first() else {
                    continue;
                };
                for pair in points.windows(2).skip(1) {
                    if let [before, here] = *pair {
                        out.push(Corners {
                            points: [before, here, first],
                            caster,
                        });
                    }
                }
            }
        }
        out
    }
}

/// Grey, the colour a face with no texture name bounces, and the middle a texture's colour
/// is mixed from (`CacheTextures`).
const GREY: f32 = 127.0;
const HALF_GREY: f32 = 0.5;
/// The least alpha of a texel that counts toward a texture's colour.
const SEEN_ALPHA: u8 = 127;

impl Map<'_> {
    /// `Face_ShouldBounce`: whether each face may bounce light: it always casts shadow, has
    /// a lightmap and has not `_bounce -1` (LIGHT-20.1).
    pub(crate) fn bouncing_faces(&self) -> Vec<bool> {
        let mut out = vec![false; self.bsp.faces.len()];
        for corner in self.casters() {
            if corner.caster.surface != Surface::Solid {
                continue;
            }
            let Some(face) = self.face(FaceId::new(corner.caster.face)) else {
                continue;
            };
            if self.is_lightmapped(face)
                && !self.flags_of(face).no_bounce
                && let Some(slot) = out.get_mut(corner.caster.face)
            {
                *slot = true;
            }
        }
        out
    }

    /// Each face's corners, moved with its model.
    pub(crate) fn moved_corners(&self) -> Vec<Vec<Vec3>> {
        self.bsp
            .faces
            .iter()
            .enumerate()
            .map(|(index, face)| {
                let offset = self
                    .model_of(index)
                    .and_then(|model| self.models.get(model))
                    .map_or([0.0; AXES], |model| model.offset);
                self.points(face)
                    .into_iter()
                    .map(|point| add(point, offset))
                    .collect()
            })
            .collect()
    }

    /// `Face_LookupTextureBounceColor`: the colour each face's texture gives the light it
    /// bounces: grey, turned toward the texture's own colour by `_bouncecolorscale`
    /// (LIGHT-20.6).
    pub(crate) fn bounce_colours(&self) -> Vec<Vec3> {
        let by = self.world.bouncecolorscale.get();
        let mut known: Vec<Option<Vec3>> = vec![None; self.bsp.textures.len()];
        self.bsp
            .faces
            .iter()
            .map(|face| {
                if self.texture_name(face).is_empty() {
                    return [HALF_GREY; AXES];
                }
                let slot = self
                    .bsp
                    .texinfo
                    .get(face.texinfo)
                    .and_then(|texinfo| texinfo.texture);
                let average = match slot.and_then(|slot| known.get_mut(slot)) {
                    Some(cached) => *cached.get_or_insert_with(|| self.average_colour(face)),
                    None => self.average_colour(face),
                };
                average.map(|c| (GREY + (c - GREY) * by) / f32::from(OPAQUE))
            })
            .collect()
    }

    /// `img::calculate_average`: the mean colour of the texels of `face`'s texture that are
    /// not see-through, each channel cut to a whole number; black for a texture with no
    /// texels, and grey with no palette to colour them (LIGHT-20.6).
    fn average_colour(&self, face: &Face) -> Vec3 {
        let Some(palette) = self.palette else {
            return [GREY; AXES];
        };
        let Some(texture) = self.texture(face) else {
            return [0.0; AXES];
        };
        let fence = texture.name.first() == Some(&b'{');
        let texels = usize::try_from(texture.width.saturating_mul(texture.height)).unwrap_or(0);
        let mut sum = [0.0_f64; AXES];
        let mut count = 0_u32;
        for &index in texture.pixels.iter().take(texels) {
            let alpha = if fence && index == FENCE_HOLE {
                0
            } else {
                OPAQUE
            };
            if alpha < SEEN_ALPHA {
                continue;
            }
            let start = usize::from(index).saturating_mul(PALETTE_ENTRY);
            if let Some(&[r, g, b]) = palette.get(start..start.saturating_add(PALETTE_ENTRY)) {
                sum = [
                    sum[0] + f64::from(r),
                    sum[1] + f64::from(g),
                    sum[2] + f64::from(b),
                ];
                count = count.saturating_add(1);
            }
        }
        if count == 0 {
            return [0.0; AXES];
        }
        sum.map(|channel| crate::to_f32((channel / f64::from(count)).floor()))
    }
}

/// `clamp_texcoord`: the texel a texture coordinate falls on, the texture repeating.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "spec LIGHT-11.7: ericw-tools casts the coordinate to an unsigned index"
)]
fn texel(coordinate: f32, size: u32) -> u32 {
    if size == 0 {
        return 0;
    }
    if coordinate >= 0.0 {
        (coordinate as u32).checked_rem(size).unwrap_or(0)
    } else {
        let back = (coordinate.abs().ceil() as u32)
            .checked_rem(size)
            .unwrap_or(0);
        size.saturating_sub(back).checked_rem(size).unwrap_or(0)
    }
}

impl Map<'_> {
    /// `img::find`: the number of the texture named `name`, in any case; none for no name
    /// and for a name no texture has.
    pub(crate) fn texture_named(&self, name: &[u8]) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        self.bsp.textures.iter().position(|texture| {
            texture
                .as_ref()
                .is_some_and(|texture| texture.name.eq_ignore_ascii_case(name))
        })
    }

    /// The number of the texture of face `face`.
    pub(crate) fn texture_of(&self, face: usize) -> Option<usize> {
        let face = self.face(FaceId::new(face))?;
        let texinfo = self.bsp.texinfo.get(face.texinfo)?;
        usize::try_from(texinfo.miptex).ok()
    }
}

impl Texels for Map<'_> {
    /// `SampleTexture`: the texel of the face's texture at `point`, nearest and repeating.
    /// A missing texture is all 0. A texel is opaque, but index 255 of a `{` texture; its
    /// colour is the palette's, white without one (LIGHT-11.6, LIGHT-11.7).
    fn sample(&self, face: usize, point: Vec3) -> Texel {
        let Some(face) = self.face(FaceId::new(face)) else {
            return Texel::default();
        };
        let (Some(texture), Some(texinfo)) =
            (self.texture(face), self.bsp.texinfo.get(face.texinfo))
        else {
            return Texel::default();
        };
        if texture.width == 0 || texture.pixels.is_empty() {
            return Texel::default();
        }
        let [s, t] = texinfo.vecs.map(|axis| {
            let [x, y, z, offset] = axis.map(f64::from);
            crate::to_f32(
                x * f64::from(point[0])
                    + y * f64::from(point[1])
                    + z * f64::from(point[2])
                    + offset,
            )
        });
        let (x, y) = (texel(s, texture.width), texel(t, texture.height));
        let at = usize::try_from(texture.width.saturating_mul(y).saturating_add(x)).ok();
        let Some(index) = at.and_then(|at| texture.pixels.get(at)).copied() else {
            return Texel::default();
        };
        let alpha = if texture.name.first() == Some(&b'{') && index == FENCE_HOLE {
            0
        } else {
            OPAQUE
        };
        let start = usize::from(index).saturating_mul(PALETTE_ENTRY);
        let colour = self
            .palette
            .and_then(|palette| palette.get(start..start.saturating_add(PALETTE_ENTRY)));
        match colour {
            Some(&[r, g, b]) => [r, g, b, alpha],
            // With no palette a texel is white, which tints no light (LIGHT-11.6).
            _ => [OPAQUE, OPAQUE, OPAQUE, alpha],
        }
    }
}
