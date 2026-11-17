// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/entities.cc, LoadEntities, LightStyleForTargetname, ParseEscapeSequences, CheckEntityFields, MatchTargets, SetupSpotlights, SetupSuns, SetupSkyDomes, JitterEntity, FixLightOnFace, FixLightsOnFaces, UniformPointOnSphere, EstimateVisibleBoundsAtPoint, SetupLights, MakeSurfaceLights, FaceMatchesSurfaceLightTemplate, GL_SubdivideSurface, SubdividePolygon, CreateSurfaceLightOnFaceSubdivision, CreateSurfaceLight, WriteEntitiesToString

//! The lights of a map in the extended dialect (LIGHT-10, LIGHT-14 to LIGHT-16): the entity
//! lump read as ericw-tools reads it, each light entity's settings checked, the lights that
//! surfaces and scatter add, the suns, and where each light can be seen from.

use crate::entities::{EntityId, token};
use crate::extended::keys::{
    CHANNEL_DEFAULT, DEFAULT_LIGHT, Dict, Dirt, Formula, LightKeys, Source, World,
};
use crate::extended::math::{
    Aabb, add, length, normalize_with_length, polygon_centroid, scale, sub, vec_from_mangle,
};
use crate::extended::rays::{Asker, Rays, Texels};
use crate::extended::scene::{Map, Skin};
use crate::extended::sun::{Random, Sun, SunLight, Suns};
use crate::extended::vis::leaf_at;
use crate::face::FaceId;
use crate::{AXES, Jitter, LightError, Vec3, to_f32};

/// `light`: the start of a light entity's classname (LIGHT-10.1).
const LIGHT_PREFIX: &[u8] = b"light";
/// The bit a `\b` in a value turns on in the bytes after it (LIGHT-8.6).
const BOLD: u8 = 0x80;
/// `surflight_subdivide`: the size of the squares a surface light's faces are cut to
/// (LIGHT-16.2).
const SUBDIVIDE: f32 = 128.0;
/// Half: the middle of an extent, and what rounds a number to the nearest whole.
const HALF: f64 = 0.5;
/// The least a cut may leave on either side.
const LEAST_CUT: f32 = 8.0;
/// How far above its face a surface light is put with no `_surface_offset` (LIGHT-16.3).
const SURFACE_OFFSET: f32 = 2.0;
/// The rays along each of the two angles that find what a light can see, and how far each
/// goes (LIGHT-10.8).
const SIGHT_RAYS: u16 = 32;
const SIGHT_REACH: f32 = 65536.0;
/// How much the box of what a light sees is grown on every side.
const SIGHT_GROWTH: f32 = 0.25;
/// Degrees to a half turn.
const HALF_TURN: f64 = 180.0;
/// A leaf with nothing in it (BSP-12).
const CONTENTS_EMPTY: i32 = -1;

/// A spotlight's aim and cone (LIGHT-10.5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spot {
    /// The unit direction it shines along.
    pub(crate) vec: Vec3,
    /// Minus the cosine of half its cone: past it a sample gets nothing.
    pub(crate) falloff: f32,
    /// Minus the cosine of half its inner cone: within it a sample gets everything.
    pub(crate) falloff2: f32,
}

/// A light (`light_t`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Lamp {
    pub(crate) keys: LightKeys,
    /// Its entity.
    pub(crate) entity: EntityId,
    /// The entity it targets.
    target: Option<usize>,
    pub(crate) spot: Option<Spot>,
    /// What it can see, when lights are culled by it (LIGHT-10.8).
    pub(crate) bounds: Option<Aabb>,
    /// The leaf it is in, when lights are culled by the BSP's visibility (LIGHT-10.10).
    pub(crate) leaf: Option<usize>,
}

impl Lamp {
    /// Where it is.
    pub(crate) fn origin(&self) -> Vec3 {
        self.keys.origin.get()
    }
}

/// The entities and lights of a map.
pub(crate) struct Lights {
    pub(crate) dicts: Vec<Dict>,
    pub(crate) world: World,
    pub(crate) lamps: Vec<Lamp>,
    pub(crate) suns: Vec<Sun>,
    /// The names given styles, each with its style (`lightstyleForTargetname`).
    styles: Vec<(Vec<u8>, i32)>,
    /// The worldspawn keys whose values did not read (LIGHT-9.4).
    pub(crate) invalid: Vec<String>,
}

/// `ParseEscapeSequences`: `value` with each `\b` taken out, the bytes between one and the
/// next with their high bit set.
fn escaped(value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len());
    let mut bold = false;
    let mut bytes = value.iter().copied().peekable();
    while let Some(byte) = bytes.next() {
        if byte == b'\\' && bytes.peek() == Some(&b'b') {
            bytes.next();
            bold = !bold;
        } else {
            out.push(if bold { byte | BOLD } else { byte });
        }
    }
    out
}

/// A key with the white space about it taken off, as `entdict_t::parse` trims it.
fn trimmed(key: &[u8]) -> &[u8] {
    key.trim_ascii()
}

impl Lights {
    /// `LoadEntities`: the entities of `lump`, the world's settings, and a light for each
    /// entity whose classname starts with `light` (LIGHT-8.5, LIGHT-10.1, LIGHT-10.2).
    pub(crate) fn read(lump: &[u8]) -> Result<Self, LightError> {
        let text = lump.split(|&b| b == 0).next().unwrap_or_default();
        let mut at = 0;
        let mut dicts: Vec<Dict> = Vec::new();
        while let Some(open) = token(text, &mut at) {
            if open != b"{" {
                return Err(LightError::ExpectedBrace { found: open });
            }
            let mut dict = Dict::default();
            loop {
                let key = token(text, &mut at).ok_or(LightError::Unclosed)?;
                if key == b"}" {
                    break;
                }
                let value = token(text, &mut at).ok_or(LightError::Unclosed)?;
                if value == b"}" {
                    return Err(LightError::NoValue);
                }
                dict.set(trimmed(&key), value);
            }
            dicts.push(dict);
        }
        let world_dict = dicts
            .first()
            .filter(|dict| dict.get(b"classname") == b"worldspawn")
            .ok_or(LightError::NoWorldspawn)?;
        let (world, invalid) = World::read(world_dict);
        let mut styles: Vec<(Vec<u8>, i32)> = Vec::new();
        for dict in &mut dicts {
            let scale = dict.get(b"lightmap_scale").to_vec();
            if !scale.is_empty() {
                dict.remove(b"lightmap_scale");
                dict.set(b"_lightmap_scale", scale);
            }
            let targetname = dict.get(b"targetname").to_vec();
            if dict.get(b"classname").starts_with(LIGHT_PREFIX) && !targetname.is_empty() {
                let style = style_for(&mut styles, &world, &targetname)?;
                dict.set(b"style", style.to_string().into_bytes());
            }
            if dict.int(b"_switchableshadow") == 1 {
                let style = style_for(&mut styles, &world, &targetname)?;
                dict.set(b"switchshadstyle", style.to_string().into_bytes());
            }
            let pairs: Vec<(Vec<u8>, Vec<u8>)> = dict.pairs().to_vec();
            for (key, value) in pairs {
                dict.set(&key, escaped(&value));
            }
        }
        let mut lamps = Vec::new();
        for (entity, dict) in dicts.iter().enumerate() {
            if !dict.get(b"classname").starts_with(LIGHT_PREFIX) {
                continue;
            }
            let mut keys = LightKeys::read(dict);
            let spot = keys.mangle.is_changed().then(|| Spot {
                vec: vec_from_mangle(keys.mangle.get()),
                falloff: 0.0,
                falloff2: 0.0,
            });
            if !keys.switchableshadow_target.get().is_empty() {
                keys.nostaticlight.set(true, Source::Default);
            }
            keys.check(&world);
            lamps.push(Lamp {
                keys,
                entity: EntityId(entity),
                target: None,
                spot,
                bounds: None,
                leaf: None,
            });
        }
        Ok(Self {
            dicts,
            world,
            lamps,
            suns: Vec::new(),
            styles,
            invalid,
        })
    }

    /// How many names were given styles.
    pub(crate) const fn switchable(&self) -> usize {
        self.styles.len()
    }

    /// The names of the models whose shadows a light switches, each with the light's style
    /// (`LightWithSwitchableShadowTargetValue`, LIGHT-11.5).
    pub(crate) fn shadow_switches(&self) -> Vec<(Vec<u8>, i32)> {
        self.lamps
            .iter()
            .filter(|lamp| !lamp.keys.switchableshadow_target.get().is_empty())
            .map(|lamp| {
                (
                    lamp.keys.switchableshadow_target.get(),
                    lamp.keys.style.get(),
                )
            })
            .collect()
    }

    /// `WriteEntitiesToString`: the entity lump, each entity's keys in order, and the NUL
    /// that ends it (LIGHT-8.6).
    pub(crate) fn written(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for dict in &self.dicts {
            dict.write(&mut out);
        }
        out.push(0);
        out
    }

    /// `SetupLights`: the surface lights and the scattered copies added, targets matched,
    /// spotlights aimed, the suns and domes made, and lights moved out of solid; with no
    /// visibility in the BSP, what each light can see (LIGHT-10.4 to LIGHT-10.8).
    pub(crate) fn set_up(&mut self, map: &Map<'_>, rays: &Rays, jitter: Jitter) {
        let mut random = Random::new(jitter);
        self.make_surface_lights(map);
        self.scatter(&mut random);
        self.match_targets();
        self.aim_spotlights();
        self.make_suns(&mut random);
        for lamp in &mut self.lamps {
            if lamp.keys.light.get() != 0.0
                && !lamp.keys.nonudge.get()
                && lamp.keys.light_channel_mask.get() == CHANNEL_DEFAULT
                && lamp.keys.shadow_channel_mask.get() == CHANNEL_DEFAULT
            {
                let moved = map
                    .out_of_solid(lamp.origin())
                    .unwrap_or_else(|| lamp.origin());
                lamp.keys.origin.set(moved, Source::Map);
            }
        }
        if map.visibility.is_some() {
            for lamp in &mut self.lamps {
                lamp.leaf = leaf_at(map.bsp, lamp.origin());
            }
        } else {
            let mut stack = Vec::new();
            for lamp in &mut self.lamps {
                lamp.bounds = Some(rays.visible_bounds(map, lamp.origin(), &mut stack));
            }
        }
    }

    /// `MatchTargets`: each light's target is the first entity whose `targetname` is its
    /// `target` but for case.
    fn match_targets(&mut self) {
        for lamp in &mut self.lamps {
            let Some(dict) = self.dicts.get(lamp.entity.index()) else {
                continue;
            };
            let target = dict.get(b"target");
            if target.is_empty() {
                continue;
            }
            lamp.target = self
                .dicts
                .iter()
                .position(|other| other.get(b"targetname").eq_ignore_ascii_case(target));
        }
    }

    /// `SetupSpotlights`: a light with a target shines at it; every spotlight's cone is its
    /// `angle`, 40 with none, and its inner cone its `_softangle` (LIGHT-10.5).
    fn aim_spotlights(&mut self) {
        for lamp in &mut self.lamps {
            let mut target_dist = 0.0;
            if let Some(target) = lamp.target.and_then(|at| self.dicts.get(at)) {
                let (vec, dist) =
                    normalize_with_length(sub(target.vector(b"origin"), lamp.origin()));
                target_dist = dist;
                lamp.spot = Some(Spot {
                    vec,
                    falloff: 0.0,
                    falloff2: 0.0,
                });
            }
            let Some(spot) = &mut lamp.spot else {
                continue;
            };
            let mut cone = if lamp.keys.spotangle.is_changed() {
                lamp.keys.spotangle.get()
            } else {
                0.0
            };
            if cone == 0.0 {
                cone = LightKeys::default().spotangle.get();
            }
            let half_cos = |degrees: f32| {
                to_f32(-(f64::from(degrees / 2.0) * core::f64::consts::PI / HALF_TURN).cos())
            };
            spot.falloff = half_cos(cone);
            let mut inner = lamp.keys.spotangle2.get();
            if inner <= 0.0 || inner > cone {
                inner = cone;
            }
            spot.falloff2 = half_cos(inner);
            if target_dist > 0.0
                && lamp.keys.falloff.get() == 0.0
                && self.world.spotlight_auto_falloff.get()
            {
                let radius = f64::from(target_dist)
                    * (f64::from(cone / 2.0) * core::f64::consts::PI / HALF_TURN).tan();
                lamp.keys
                    .falloff
                    .set(to_f32(f64::from(target_dist) + radius), Source::Map);
            }
        }
    }

    /// `JitterEntities`: for each light with `_deviance`, `_samples` less one copies, each
    /// moved by up to the deviance on each axis (LIGHT-10.6).
    fn scatter(&mut self, random: &mut Random) {
        let mut copies = Vec::new();
        for lamp in &self.lamps {
            if lamp.keys.sun.get() {
                continue;
            }
            let deviance = f64::from(lamp.keys.deviance.get());
            for _ in 1..lamp.keys.samples.get() {
                let mut copy = lamp.clone();
                let origin = lamp
                    .origin()
                    .map(|c| to_f32(f64::from(c) + random.signed_unit() * deviance));
                copy.keys.origin.set(origin, Source::Map);
                copies.push(copy);
            }
        }
        self.lamps.extend(copies);
    }

    /// `SetupSuns` and `SetupSkyDomes`: a sun for each light with `_sun`, the world's sun and
    /// second sun, the world's domes, and a dome for each light with `_sunlight2` or
    /// `_sunlight3`; such a light then gives no light of its own (LIGHT-14, LIGHT-15).
    fn make_suns(&mut self, random: &mut Random) {
        let world = &self.world;
        let mut suns = Suns {
            all: Vec::new(),
            world_dirt: world.dirt.get(),
            random,
        };
        let of = |lamp: &Lamp| SunLight {
            light: lamp.keys.light.get(),
            color: lamp.keys.color.get(),
            dirt: lamp.keys.dirt(),
            anglescale: lamp.keys.anglescale.get(),
            style: lamp.keys.style.get(),
        };
        for lamp in &mut self.lamps {
            if !(lamp.keys.sun.get() && lamp.keys.light.get() > 0.0) {
                continue;
            }
            let mangle = lamp.keys.mangle.get();
            let direction = if let Some(target) = lamp.target.and_then(|at| self.dicts.get(at)) {
                sub(target.vector(b"origin"), lamp.origin())
            } else if length(mangle) > 0.0 {
                vec_from_mangle(mangle)
            } else {
                scale(lamp.origin(), -1.0)
            };
            suns.sun(direction, lamp.keys.deviance.get(), of(lamp));
            lamp.keys.light.set(0.0, Source::Map);
        }
        let world_sun = |light: f32, color: Vec3, dirt: f32| SunLight {
            light,
            color,
            dirt: dirt_of_int(dirt),
            anglescale: world.anglescale.get(),
            style: 0,
        };
        let deviance = world.sun_deviance.get();
        let sun_dirt = world.sunlight_dirt.get();
        suns.sun(
            world.sunvec.get(),
            deviance,
            world_sun(world.sunlight.get(), world.sunlight_color.get(), sun_dirt),
        );
        if world.sun2.get() != 0.0 {
            suns.sun(
                world.sun2vec.get(),
                deviance,
                world_sun(world.sun2.get(), world.sun2_color.get(), sun_dirt),
            );
        }
        let dome_dirt = world.sunlight2_dirt.get();
        suns.dome(
            Some(world_sun(
                world.sunlight2.get(),
                world.sunlight2_color.get(),
                dome_dirt,
            )),
            Some(world_sun(
                world.sunlight3.get(),
                world.sunlight3_color.get(),
                dome_dirt,
            )),
        );
        for lamp in &mut self.lamps {
            let (upper, lower) = (lamp.keys.sunlight2.get(), lamp.keys.sunlight3.get());
            if !((upper || lower) && lamp.keys.light.get() > 0.0) {
                continue;
            }
            if upper {
                suns.dome(Some(of(lamp)), None);
            } else {
                suns.dome(None, Some(of(lamp)));
            }
            lamp.keys.light.set(0.0, Source::Map);
        }
        self.suns = suns.all;
    }

    /// `MakeSurfaceLights`: each light with `_surface` becomes a template, and gives no
    /// light itself; every face of that texture, taken leaf by leaf, is cut into squares of
    /// 128 and a copy of the template put over the middle of each (LIGHT-16).
    fn make_surface_lights(&mut self, map: &Map<'_>) {
        let mut templates: Vec<(Lamp, Vec<u8>)> = Vec::new();
        for lamp in &mut self.lamps {
            let texture = self
                .dicts
                .get(lamp.entity.index())
                .map_or(&[][..], |dict| dict.get(b"_surface"));
            if texture.is_empty() {
                continue;
            }
            templates.push((lamp.clone(), texture.to_vec()));
            lamp.keys.light.set(0.0, Source::Map);
        }
        if templates.is_empty() {
            return;
        }
        let mut visited = vec![false; map.bsp.faces.len()];
        for leaf in &map.bsp.leafs {
            let underwater = leaf.contents != CONTENTS_EMPTY;
            let marks = map
                .bsp
                .marksurfaces
                .get(
                    leaf.firstmarksurface
                        ..leaf.firstmarksurface.saturating_add(leaf.nummarksurfaces),
                )
                .unwrap_or_default();
            for &face_index in marks {
                let (Some(face), Some(model)) =
                    (map.face(FaceId::new(face_index)), map.model_of(face_index))
                else {
                    continue;
                };
                // The underwater side of a liquid's surface gives no lights.
                if underwater && map.skin(face) == Skin::Liquid {
                    continue;
                }
                let Some(seen) = visited.get_mut(face_index) else {
                    continue;
                };
                if core::mem::replace(seen, true) {
                    continue;
                }
                let name = map.texture_name(face);
                let group = map.flags_of(face).surflight_group;
                let matching: Vec<&Lamp> = templates
                    .iter()
                    .filter(|(lamp, texture)| {
                        let radiosity = self
                            .dicts
                            .get(lamp.entity.index())
                            .map_or(0, |dict| dict.int(b"_surface_radiosity"));
                        radiosity == 0
                            && lamp.keys.surflight_group.get() == group
                            && name.eq_ignore_ascii_case(texture)
                    })
                    .map(|(lamp, _)| lamp)
                    .collect();
                if matching.is_empty() {
                    continue;
                }
                let offset = map.models.get(model).map_or([0.0; AXES], |m| m.offset);
                let normal = map.plane(face).normal;
                let mut pieces = Vec::new();
                subdivide(map.points(face), &mut pieces);
                for piece in pieces {
                    let middle = polygon_centroid(&piece);
                    if middle.iter().any(|c| c.is_nan()) {
                        continue;
                    }
                    for template in &matching {
                        let dict = self.dicts.get(template.entity.index());
                        let mut above = dict.map_or(0.0, |dict| dict.float(b"_surface_offset"));
                        if above == 0.0 {
                            above = SURFACE_OFFSET;
                        }
                        let mut copy = (*template).clone();
                        copy.keys
                            .origin
                            .set(add(add(middle, scale(normal, above)), offset), Source::Map);
                        if dict.is_some_and(|dict| dict.int(b"_surface_spotlight") != 0) {
                            copy.spot = Some(Spot {
                                vec: normal,
                                falloff: 0.0,
                                falloff2: 0.0,
                            });
                        }
                        self.lamps.push(copy);
                    }
                }
            }
        }
    }
}

/// A dirt setting kept as a float, as `AddSun` takes it: an `int`, 1 or -1 or neither.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec LIGHT-14.6: ericw-tools passes the dirt setting to AddSun as an int"
)]
fn dirt_of_int(value: f32) -> Dirt {
    match value as i32 {
        1 => Dirt::On,
        -1 => Dirt::Off,
        _ => Dirt::World,
    }
}

/// `LightStyleForTargetname`: the style of `name`, the next from the world's first for a new
/// one or for no name; past the world's last the run stops (LIGHT-10.2).
fn style_for(
    styles: &mut Vec<(Vec<u8>, i32)>,
    world: &World,
    name: &[u8],
) -> Result<i32, LightError> {
    if !name.is_empty()
        && let Some((_, style)) = styles.iter().find(|(known, _)| known == name)
    {
        return Ok(*style);
    }
    let max = world.style_max.get();
    let style = i32::try_from(styles.len())
        .ok()
        .and_then(|count| world.style_start.get().checked_add(count))
        .filter(|style| *style < max)
        .ok_or(LightError::TooManyStyles { max })?;
    styles.push((name.to_vec(), style));
    Ok(style)
}

impl LightKeys {
    /// `CheckEntityFields`: a light's settings put right (LIGHT-10.3).
    fn check(&mut self, world: &World) {
        let keys = self;
        if !keys.light.is_changed() {
            keys.light.set(DEFAULT_LIGHT, Source::Map);
        }
        if keys.atten.get() <= 0.0 {
            keys.atten.set(1.0, Source::Map);
        }
        if !(0.0..=1.0).contains(&keys.anglescale.get()) {
            keys.anglescale.set(world.anglescale.get(), Source::Map);
        }
        if keys.falloff.get() > 0.0 && keys.formula.get() != Formula::Linear {
            keys.falloff.set(0.0, Source::Map);
        }
        if keys.formula.get() == Formula::Unknown {
            keys.formula.set(Formula::Linear, Source::Map);
            keys.light.set(0.0, Source::Map);
        }
        if keys.deviance.get() > 0.0 && keys.samples.get() == 0 {
            keys.samples
                .set(LightKeys::default().samples.get(), Source::Map);
        }
        if keys.deviance.get() <= 0.0 || keys.samples.get() <= 1 {
            keys.deviance.set(0.0, Source::Map);
            keys.samples.set(1, Source::Map);
        }
        // A light of several samples shares its brightness among them, but a linear one.
        let shared = match keys.formula.get() {
            Formula::Inverse | Formula::Inverse2 | Formula::Infinite | Formula::Inverse2A => true,
            Formula::LocalMin => world.addminlight.get(),
            Formula::Linear | Formula::Unknown => false,
        };
        if shared {
            let samples = u16::try_from(keys.samples.get()).map_or(1.0, f32::from);
            keys.light.set(keys.light.get() / samples, Source::Map);
        }
        if !keys.shadow_channel_mask.is_changed() {
            keys.shadow_channel_mask
                .set(keys.light_channel_mask.get(), Source::Default);
        }
    }
}

impl Rays {
    /// `EstimateVisibleBoundsAtPoint`: the box of where 32 by 32 rays spread over the sphere
    /// about `point` end, each at what it hits or 65536 units out, grown by a quarter of its size
    /// on every side (LIGHT-10.8).
    pub(crate) fn visible_bounds(
        &self,
        texels: &dyn Texels,
        point: Vec3,
        stack: &mut Vec<usize>,
    ) -> Aabb {
        let rays = self;
        let mut bounds = Aabb::at(point);
        let last = f32::from(SIGHT_RAYS.saturating_sub(1));
        for x in 0..SIGHT_RAYS {
            for y in 0..SIGHT_RAYS {
                // `UniformPointOnSphere`.
                let (u1, u2) = (f32::from(x) / last, f32::from(y) / last);
                let theta = to_f32(f64::from(u1) * 2.0 * core::f64::consts::PI);
                let u = to_f32(2.0 * f64::from(u2) - 1.0);
                let s = to_f32((1.0 - f64::from(u * u)).sqrt());
                let dir = [s * theta.cos(), s * theta.sin(), u];
                let struck = rays.struck((point, dir, SIGHT_REACH), &Asker::NOBODY, texels, stack);
                let reach = struck.hit.map_or(SIGHT_REACH, |(_, dist)| dist);
                bounds = bounds.with(add(point, scale(dir, reach)));
            }
        }
        bounds.grown(SIGHT_GROWTH)
    }
}

/// `SubdividePolygon`: `points` cut along each axis at the multiple of 128 nearest its
/// middle, each side cut again, until no cut leaves 8 units on both sides; the pieces go to
/// `out` (LIGHT-16.2).
fn subdivide(points: Vec<Vec3>, out: &mut Vec<Vec<Vec3>>) {
    let Some(&first) = points.first() else {
        return;
    };
    let bounds = points.iter().fold(Aabb::at(first), |b, &p| b.with(p));
    for axis in 0..AXES {
        let (Some(&low), Some(&high)) = (bounds.mins.get(axis), bounds.maxs.get(axis)) else {
            continue;
        };
        let middle = to_f32((f64::from(low) + f64::from(high)) * HALF);
        let cut = SUBDIVIDE * to_f32((f64::from(middle / SUBDIVIDE) + HALF).floor());
        if high - cut < LEAST_CUT || cut - low < LEAST_CUT {
            continue;
        }
        let dist = |point: &Vec3| point.get(axis).copied().unwrap_or_default() - cut;
        let (mut front, mut back) = (Vec::new(), Vec::new());
        for (here, next) in points.iter().zip(points.iter().cycle().skip(1)) {
            let (d0, d1) = (dist(here), dist(next));
            if d0 >= 0.0 {
                front.push(*here);
            }
            if d0 <= 0.0 {
                back.push(*here);
            }
            if d0 == 0.0 || d1 == 0.0 {
                continue;
            }
            if (d0 > 0.0) != (d1 > 0.0) {
                let fraction = d0 / (d0 - d1);
                let crossing = add(*here, scale(sub(*next, *here), fraction));
                front.push(crossing);
                back.push(crossing);
            }
        }
        subdivide(front, out);
        subdivide(back, out);
        return;
    }
    out.push(points);
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: the values are exact in single precision, and a failed unwrap \
                  is a failed test"
    )]

    use super::*;

    fn lights(text: &str) -> Lights {
        Lights::read(text.as_bytes()).unwrap()
    }

    const WORLD: &str = "{\n\"classname\" \"worldspawn\"\n}\n";

    // spec LIGHT-8.5: the first entity must be the worldspawn.
    #[test]
    fn the_first_entity_must_be_the_worldspawn() {
        let no_world = Lights::read(b"{\n\"classname\" \"light\"\n}\n");
        assert_eq!(no_world.err(), Some(LightError::NoWorldspawn));
        assert_eq!(Lights::read(b"").err(), Some(LightError::NoWorldspawn));
        assert_eq!(
            Lights::read(b"}").err(),
            Some(LightError::ExpectedBrace {
                found: b"}".to_vec()
            })
        );
        assert_eq!(
            Lights::read(b"{\n\"classname\"").err(),
            Some(LightError::Unclosed)
        );
        assert_eq!(
            Lights::read(b"{\n\"classname\" }").err(),
            Some(LightError::NoValue)
        );
    }

    // spec LIGHT-10.2: every entity whose classname starts with light and that has a
    // targetname gets the style of that name, from 32; the name past the last stops the run.
    #[test]
    fn named_lights_get_styles_from_thirty_two() {
        let text = format!(
            "{WORLD}{{\n\"classname\" \"light_torch\"\n\"targetname\" \"a\"\n\"style\" \"5\"\n}}\n\
             {{\n\"classname\" \"light\"\n\"targetname\" \"b\"\n}}\n\
             {{\n\"classname\" \"light\"\n\"targetname\" \"a\"\n}}\n\
             {{\n\"classname\" \"func_wall\"\n\"targetname\" \"c\"\n}}\n"
        );
        let lights = lights(&text);
        let style = |at: usize| lights.dicts.get(at).map(|d| d.get(b"style").to_vec());
        assert_eq!(style(1), Some(b"32".to_vec()), "its own style is replaced");
        assert_eq!(style(2), Some(b"33".to_vec()));
        assert_eq!(style(3), Some(b"32".to_vec()));
        assert_eq!(style(4), Some(Vec::new()));
        assert_eq!(lights.switchable(), 2);
        assert_eq!(lights.lamps.len(), 3);
        assert_eq!(lights.lamps.first().map(|l| l.keys.style.get()), Some(32));

        let mut many = String::from(WORLD);
        for name in 0..33 {
            many.push_str(&format!(
                "{{\n\"classname\" \"light\"\n\"targetname\" \"t{name}\"\n}}\n"
            ));
        }
        assert_eq!(
            Lights::read(many.as_bytes()).err(),
            Some(LightError::TooManyStyles { max: 64 })
        );
    }

    // spec LIGHT-11.5: an entity with _switchableshadow 1 gets a style of its own in
    // switchshadstyle, a new one each time it has no name.
    #[test]
    fn switchable_shadows_get_styles() {
        let text = format!(
            "{WORLD}{{\n\"classname\" \"func_wall\"\n\"_switchableshadow\" \"1\"\n}}\n\
             {{\n\"classname\" \"func_wall\"\n\"_switchableshadow\" \"1\"\n}}\n\
             {{\n\"classname\" \"light\"\n\"_switchableshadow_target\" \"door\"\n\"style\" \"7\"\n}}\n"
        );
        let lights = lights(&text);
        let style = |at: usize| {
            lights
                .dicts
                .get(at)
                .map(|d| d.get(b"switchshadstyle").to_vec())
        };
        assert_eq!(style(1), Some(b"32".to_vec()));
        assert_eq!(style(2), Some(b"33".to_vec()));
        assert_eq!(lights.shadow_switches(), [(b"door".to_vec(), 7)]);
        assert!(
            lights
                .lamps
                .first()
                .is_some_and(|lamp| lamp.keys.nostaticlight.get())
        );
    }

    // spec LIGHT-8.6: the entity lump is written back with each entity's keys in order; a \b
    // in a value is taken out and sets the high bit of the bytes up to the next.
    #[test]
    fn the_lump_is_written_in_order_with_bold_bytes() {
        let lights = lights(
            "{\n\"classname\" \"worldspawn\"\n\"message\" \"a\\bbc\\bd\"\n\"lightmap_scale\" \"8\"\n}\n",
        );
        let mut expected = b"{\n\"classname\" \"worldspawn\"\n\"message\" \"a".to_vec();
        expected.extend_from_slice(&[b'b' | 0x80, b'c' | 0x80]);
        expected.extend_from_slice(b"d\"\n\"_lightmap_scale\" \"8\"\n}\n\0");
        assert_eq!(lights.written(), expected);
    }

    // spec LIGHT-10.3: a light's settings are put right: 300 with no light, wait of 0 is 1,
    // an anglescale outside 0 to 1 is the world's, falloff only on a linear light, an unknown
    // delay puts the light out, and samples share an inverse light's brightness.
    #[test]
    fn a_lights_fields_are_checked() {
        let lamp = |keys: &str| {
            let text = format!("{WORLD}{{\n\"classname\" \"light\"\n{keys}}}\n");
            lights(&text).lamps.first().cloned().unwrap().keys
        };
        let plain = lamp("");
        assert_eq!(plain.light.get(), 300.0);
        assert_eq!(plain.anglescale.get(), 0.5);
        assert_eq!(plain.samples.get(), 1);
        assert_eq!(lamp("\"light\" \"0\"\n").light.get(), 0.0);
        assert_eq!(lamp("\"wait\" \"0\"\n").atten.get(), 1.0);
        assert_eq!(lamp("\"_anglescale\" \"0.25\"\n").anglescale.get(), 0.25);
        assert_eq!(lamp("\"_anglescale\" \"2\"\n").anglescale.get(), 0.5);
        assert_eq!(lamp("\"_falloff\" \"64\"\n").falloff.get(), 64.0);
        assert_eq!(
            lamp("\"_falloff\" \"64\"\n\"delay\" \"2\"\n").falloff.get(),
            0.0
        );
        let unknown = lamp("\"delay\" \"9\"\n");
        assert_eq!(unknown.formula.get(), Formula::Linear);
        assert_eq!(unknown.light.get(), 0.0);
        let scattered = lamp("\"_deviance\" \"8\"\n\"delay\" \"1\"\n\"light\" \"160\"\n");
        assert_eq!(scattered.samples.get(), 16);
        assert_eq!(scattered.light.get(), 10.0);
        let linear = lamp("\"_deviance\" \"8\"\n\"_samples\" \"4\"\n\"light\" \"160\"\n");
        assert_eq!(linear.light.get(), 160.0, "a linear light is not shared");
        let one = lamp("\"_deviance\" \"8\"\n\"_samples\" \"1\"\n");
        assert_eq!(one.deviance.get(), 0.0);
        assert_eq!(
            lamp("\"_light_channel_mask\" \"2\"\n")
                .shadow_channel_mask
                .get(),
            2
        );
    }

    // spec LIGHT-10.5: a light with a target is a spotlight shining at it, and one with
    // mangle shines along it; the cone is angle, 40 with none, and the inner cone _softangle.
    #[test]
    fn spotlights_are_aimed_and_their_cones_set() {
        let text = format!(
            "{WORLD}{{\n\"classname\" \"light\"\n\"origin\" \"0 0 64\"\n\"target\" \"T\"\n}}\n\
             {{\n\"classname\" \"info_null\"\n\"targetname\" \"t\"\n\"origin\" \"0 0 0\"\n}}\n\
             {{\n\"classname\" \"light\"\n\"mangle\" \"90 0 0\"\n\"angle\" \"90\"\n\"_softangle\" \"60\"\n}}\n\
             {{\n\"classname\" \"light\"\n\"mangle\" \"0 0 0\"\n\"_softangle\" \"80\"\n}}\n\
             {{\n\"classname\" \"light\"\n}}\n"
        );
        let mut lights = lights(&text);
        lights.match_targets();
        lights.aim_spotlights();
        let spot = |at: usize| lights.lamps.get(at).and_then(|lamp| lamp.spot);
        let aimed = spot(0).unwrap();
        assert_eq!(aimed.vec, [0.0, 0.0, -1.0]);
        let default_cone = to_f32(-(20.0_f64.to_radians().cos()));
        assert_eq!(aimed.falloff, default_cone);
        assert_eq!(aimed.falloff2, default_cone);
        let mangled = spot(1).unwrap();
        assert!((mangled.vec[1] - 1.0).abs() < 1e-6);
        assert_eq!(mangled.falloff, to_f32(-(45.0_f64.to_radians().cos())));
        assert_eq!(mangled.falloff2, to_f32(-(30.0_f64.to_radians().cos())));
        let wide = spot(2).unwrap();
        assert_eq!(
            wide.falloff2, wide.falloff,
            "an inner cone wider than the cone"
        );
        assert_eq!(spot(3), None);
    }

    // spec LIGHT-10.6: a light with _deviance gets _samples less one copies, each within the
    // deviance of it on every axis.
    #[test]
    fn a_light_with_deviance_is_scattered() {
        let text = format!(
            "{WORLD}{{\n\"classname\" \"light\"\n\"origin\" \"100 100 100\"\n\"_deviance\" \"4\"\n\"_samples\" \"5\"\n}}\n"
        );
        let mut lights = lights(&text);
        lights.scatter(&mut Random::new(Jitter::Bsd));
        assert_eq!(lights.lamps.len(), 5);
        for lamp in lights.lamps.iter().skip(1) {
            let away = sub(lamp.origin(), [100.0; 3]);
            assert!(away.iter().all(|c| c.abs() <= 4.0));
            assert_ne!(away, [0.0; 3]);
        }
    }

    // spec LIGHT-14.2: the world's sun shines along _sun_mangle, straight down with none; a
    // light with _sun shines at its target, or along its mangle, and gives no light itself.
    // spec LIGHT-14.6: sunlight's dirt is _sunlight_dirt.
    #[test]
    fn suns_come_from_the_world_and_from_sun_lights() {
        let text = "{\n\"classname\" \"worldspawn\"\n\"_sunlight\" \"200\"\n\"_sun_mangle\" \"0 -90 0\"\n\
                    \"_sunlight2\" \"65\"\n\"_sunlight_dirt\" \"1\"\n\"_sunlight3\" \"130\"\n\"_sun2\" \"50\"\n}\n\
                    {\n\"classname\" \"light\"\n\"_sun\" \"1\"\n\"mangle\" \"0 -90 0\"\n\"light\" \"80\"\n\"style\" \"3\"\n}\n\
                    {\n\"classname\" \"light\"\n\"_sunlight2\" \"1\"\n\"light\" \"65\"\n}\n\
                    {\n\"classname\" \"light\"\n\"_sunlight3\" \"1\"\n\"light\" \"65\"\n}\n";
        let mut lights = lights(text);
        lights.match_targets();
        lights.make_suns(&mut Random::new(Jitter::Bsd));
        // The light's sun, the world's two suns, its two domes and the lights' two domes.
        assert_eq!(lights.suns.len(), 1 + 2 + 130 + 65 + 65);
        let first = lights.suns.first().copied().unwrap();
        assert!((first.vec[2] - 16384.0).abs() < 0.01);
        assert_eq!((first.light, first.style), (80.0, 3));
        let world = lights.suns.get(1).copied().unwrap();
        assert_eq!(world.light, 200.0);
        assert!(world.dirt);
        assert!((world.vec[2] - 16384.0).abs() < 0.01);
        let second = lights.suns.get(2).copied().unwrap();
        assert_eq!(second.light, 50.0);
        assert_eq!(
            second.vec,
            [0.0, 0.0, 16384.0],
            "straight down with no mangle"
        );
        let dome = lights.suns.get(3).copied().unwrap();
        assert_eq!(dome.light, 1.0);
        assert!(!dome.dirt, "the dome's dirt is _sunlight2_dirt");
        assert!(lights.lamps.iter().all(|lamp| lamp.keys.light.get() == 0.0));
    }

    // spec LIGHT-16.2: a face is cut at the multiples of 128 nearest its middle until no cut
    // leaves 8 units on both sides.
    #[test]
    fn a_face_is_cut_into_squares_of_128() {
        let square = |size: f32| {
            vec![
                [0.0, 0.0, 0.0],
                [0.0, size, 0.0],
                [size, size, 0.0],
                [size, 0.0, 0.0],
            ]
        };
        let mut pieces = Vec::new();
        subdivide(square(256.0), &mut pieces);
        assert_eq!(pieces.len(), 4);
        let middles: Vec<Vec3> = pieces.iter().map(|p| polygon_centroid(p)).collect();
        assert!(middles.contains(&[64.0, 64.0, 0.0]));
        assert!(middles.contains(&[192.0, 192.0, 0.0]));
        let mut small = Vec::new();
        subdivide(square(100.0), &mut small);
        assert_eq!(small.len(), 1, "the nearest multiple is its edge");
        let mut sliver = Vec::new();
        subdivide(square(134.0), &mut sliver);
        assert_eq!(sliver.len(), 1, "a cut would leave under 8 units");
        let mut cut = Vec::new();
        subdivide(square(137.0), &mut cut);
        assert_eq!(cut.len(), 4);
        subdivide(Vec::new(), &mut cut);
        assert_eq!(cut.len(), 4, "no points, no piece");
    }
}
