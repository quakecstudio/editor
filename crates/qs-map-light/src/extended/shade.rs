// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/ltface.cc, Lightmap_ForStyle, Lightmap_Save, GetLightValue, GetLightValueWithAngle, GetLightContrib, Light_ClampMin, Dirt_GetScaleFactor, CullLight, LightFace_Entity, LightPoint_Entity, LightFace_Sky, LightPoint_Sky, LightFace_Min, LightFace_LocalMin, LightFace_AutoMin, DirectLightFace, PostProcessLightFace, CalcLightgridAtPoint, LightPoint_ScaleAndClamp, add, brightness
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/lightgrid.cc, FixPointAndCalcLightgrid

//! Lighting a face (LIGHT-10, LIGHT-14, LIGHT-17): what each light, each sun and the
//! minimum light give each of its samples, into one lightmap for each style.

use super::math::brightness;
use super::mottle::mottle;
use crate::extended::bounce::{BOUNCE_GATE, Emitter};
use crate::extended::dirt::Dirt;
use crate::extended::keys::{CHANNEL_DEFAULT, Dirt as DirtKey, FULL, Formula, World};
use crate::extended::lights::{Lamp, Lights, Spot};
use crate::extended::math::{
    add, distance, dot, length, normalize, normalize_with_length, scale, sub,
};
use crate::extended::points::Patch;
use crate::extended::rays::{Asker, ModelId, Passed, Rays, Stack, Surface};
use crate::extended::scene::Map;
use crate::extended::sun::Sun;
use crate::extended::vis::is_unseen;
use crate::{AXES, Vec3, to_f32};

/// `LF_SCALE`: the distance an inverse light's brightness is given at (LIGHT-10.3).
const LF_SCALE: f32 = 128.0;
/// `gate`: a contribution at most this bright is not traced (LIGHT-10.9).
const GATE: f32 = 0.001;
/// How near a light a sample may be: nearer, it is taken as this far, straight below
/// (LIGHT-10.9).
const NEAREST: f32 = 0.1;
/// `LIGHT_ANGLE_EPSILON`: how far a face may be turned from a sun and still be lit.
const ANGLE_EPSILON: f32 = 0.01;
/// `MAX_SKY_DIST`: how far a ray toward a sun goes.
const SKY_DIST: f32 = 1_000_000.0;
/// What a box of what a light sees may miss a face's box by (LIGHT-10.8).
const BOUNDS_EPSILON: f32 = 0.001;
/// The styles a point's light is kept for (`lightgrid_samples_t`).
const POINT_STYLES: usize = 4;
/// Half, of a box's two corners.
const HALF: f32 = 0.5;
/// The channels of the six sides of the cube a point's light falls on.
const SIDE_CHANNELS: f32 = 18.0;
/// The sides of the cube a point's light falls on.
const CUBE_SIDES: usize = 6;

/// The lightmap of one style (`lightmap_t`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Lightmap {
    /// Its style, none until a light has reached it.
    pub(crate) style: Option<i32>,
    pub(crate) samples: Vec<Vec3>,
    /// The light its samples took since the last bounce counted it (`bounce_color`).
    pub(crate) bounce: Vec3,
}

/// A face's lightmaps (`lightmapdict_t`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Lightmaps {
    pub(crate) all: Vec<Lightmap>,
    /// The samples of each.
    size: usize,
}

impl Lightmaps {
    /// No lightmaps, each to hold `size` samples.
    pub(crate) const fn new(size: usize) -> Self {
        Self {
            all: Vec::new(),
            size,
        }
    }

    /// `Lightmap_ForStyle`: where the lightmap of `style` is, or one no light has reached
    /// yet, or a new one.
    fn for_style(&mut self, style: i32) -> usize {
        if let Some(at) = self.all.iter().position(|lm| lm.style == Some(style)) {
            return at;
        }
        if let Some(at) = self.all.iter().position(|lm| lm.style.is_none()) {
            return at;
        }
        self.all.push(Lightmap {
            style: None,
            samples: vec![[0.0; AXES]; self.size],
            bounce: [0.0; AXES],
        });
        self.all.len().saturating_sub(1)
    }

    /// `colour` added to the sample `sample` of the lightmap at `at`, and to what it has to
    /// bounce; `style` has now reached it (`Lightmap_Save`).
    fn reach(&mut self, at: usize, style: i32, sample: usize, colour: Vec3) {
        let Some(lightmap) = self.all.get_mut(at) else {
            return;
        };
        lightmap.style.get_or_insert(style);
        lightmap.bounce = add(lightmap.bounce, colour);
        if let Some(total) = lightmap.samples.get_mut(sample) {
            *total = add(*total, colour);
        }
    }
}

/// `Light_ClampMin`: each channel of `sample` raised to `colour`'s at `light`; whether any
/// was.
fn clamp_min(sample: &mut Vec3, light: f32, colour: Vec3) -> bool {
    let mut changed = false;
    for (channel, of) in sample.iter_mut().zip(colour) {
        let least = of * (light / FULL);
        if least > *channel {
            *channel = least;
            changed = true;
        }
    }
    changed
}

impl Lamp {
    /// `GetLightValue`: a light of `light` at `dist`, by its formula (LIGHT-10.3).
    fn value_at(&self, world: &World, dist: f32) -> f32 {
        let lamp = self;
        let keys = &lamp.keys;
        let (light, falloff) = (keys.light.get(), keys.falloff.get());
        let formula = keys.formula.get();
        if matches!(formula, Formula::Infinite | Formula::LocalMin) {
            return light;
        }
        if falloff > 0.0 && formula == Formula::Linear {
            return if falloff > dist {
                light * (1.0 - dist / falloff)
            } else {
                0.0
            };
        }
        let value = world.scaledist.get() * keys.atten.get() * dist;
        match formula {
            Formula::Inverse => light / (value / LF_SCALE),
            Formula::Inverse2 => light / ((value * value) / (LF_SCALE * LF_SCALE)),
            Formula::Inverse2A => {
                let value = value + LF_SCALE;
                light / ((value * value) / (LF_SCALE * LF_SCALE))
            }
            Formula::Linear | Formula::Unknown | Formula::Infinite | Formula::LocalMin => {
                if light > 0.0 {
                    (light - value).max(0.0)
                } else {
                    (light + value).min(0.0)
                }
            }
        }
    }
}

/// How a sample faces a light.
#[derive(Clone, Copy)]
struct Facing {
    /// The sample's normal; none for a point on no face, which every light reaches whole.
    normal: Option<Vec3>,
    /// Whether its face is lit from both sides.
    twosided: bool,
}

/// What a light gives a sample, before anything is in the way (`GetLightContrib`).
struct Contribution {
    colour: Vec3,
    /// The unit direction from the sample to the light, and how far it is.
    dir: Vec3,
    dist: f32,
}

impl Lamp {
    /// `GetLightContrib` and `GetLightValueWithAngle`: what `lamp` gives a sample at `point`
    /// (LIGHT-10.3, LIGHT-10.5, LIGHT-10.9).
    fn contribution(&self, map: &Map<'_>, point: Vec3, facing: Facing) -> Contribution {
        let world = &map.world;
        let lamp = self;
        let (mut dir, mut dist) = normalize_with_length(sub(lamp.origin(), point));
        if dist < NEAREST {
            dist = NEAREST;
            dir = [0.0, 0.0, 1.0];
        }
        let mut angle = facing.normal.map_or(1.0, |normal| dot(dir, normal));
        if (lamp.keys.bleed.get() || facing.twosided) && angle < 0.0 {
            angle = -angle;
        }
        let mut value = 0.0;
        if angle >= 0.0 {
            let anglescale = f64::from(lamp.keys.anglescale.get());
            let angle = to_f32((1.0 - anglescale) + anglescale * f64::from(angle));
            value = lamp.value_at(world, dist) * angle * spot_scale(lamp.spot, dir);
        }
        // A projector's colour is the texture's where the sample falls in its picture, by
        // the light's own over 255; nothing outside the picture (LIGHT-10.11).
        let colour = match &lamp.projection {
            Some(projection) => {
                let own = lamp.keys.color.get();
                let mut colour = map.projected(projection, point).unwrap_or([0.0; AXES]);
                for (channel, own) in colour.iter_mut().zip(own) {
                    *channel *= own * (1.0 / FULL);
                }
                colour
            }
            None => lamp.keys.color.get(),
        };
        Contribution {
            colour: scale(scale(colour, value), 1.0 / FULL),
            dir,
            dist,
        }
    }
}

/// How much of a spotlight's light a sample in the direction `dir` from it gets: none past
/// its cone, all within its inner cone, and between the two in proportion (LIGHT-10.5).
fn spot_scale(spot: Option<Spot>, dir: Vec3) -> f32 {
    let Some(spot) = spot else {
        return 1.0;
    };
    let falloff = dot(spot.vec, dir);
    if falloff > spot.falloff {
        0.0
    } else if falloff > spot.falloff2 {
        to_f32(1.0 - f64::from((falloff - spot.falloff2) / (spot.falloff - spot.falloff2)))
    } else {
        1.0
    }
}

/// Which lights a pass over them takes: those that add light go before the minimum light,
/// those that take it away after (LIGHT-8.4).
#[derive(Clone, Copy)]
enum Pass {
    Adding,
    Taking,
}

impl Pass {
    /// Whether a light of brightness `light` is this pass's.
    fn takes(self, light: f32) -> bool {
        match self {
            Self::Adding => light > 0.0,
            Self::Taking => light < 0.0,
        }
    }
}

/// Who asks about dirt: a light, or the sky and the minimum light, which always take it.
#[derive(Clone, Copy)]
enum DirtFor<'a> {
    Lamp(&'a Lamp, f32),
    Ambient,
}

/// What lights a map's faces.
pub(crate) struct Shader<'a> {
    pub(crate) map: &'a Map<'a>,
    pub(crate) rays: &'a Rays,
    pub(crate) lights: &'a Lights,
    /// The dome dirt is measured over, when any is (`dirt_in_use`, LIGHT-19).
    pub(crate) dirt: Option<&'a Dirt>,
}

impl Shader<'_> {
    fn world(&self) -> &World {
        &self.map.world
    }

    /// `Dirt_GetScaleFactor`: how much of a contribution is left after dirt, for a sample
    /// whose sky is `occlusion` closed off (LIGHT-19).
    fn dirt_scale(&self, occlusion: f32, who: DirtFor<'_>, patch: Option<&Patch>) -> f32 {
        let world = self.world();
        if self.dirt.is_none() || patch.is_some_and(|patch| patch.nodirt) {
            return 1.0;
        }
        let (mut gain, mut dirtscale) = (world.dirtgain.get(), world.dirtscale.get());
        if let DirtFor::Lamp(lamp, _) = who {
            let dirty = match lamp.keys.dirt() {
                DirtKey::On => true,
                DirtKey::Off => false,
                DirtKey::World => world.dirt.get(),
            };
            if !dirty {
                return 1.0;
            }
            if lamp.keys.dirtgain.get() != 0.0 {
                gain = lamp.keys.dirtgain.get();
            }
            if lamp.keys.dirtscale.get() != 0.0 {
                dirtscale = lamp.keys.dirtscale.get();
            }
        }
        if occlusion <= 0.0 {
            return 1.0;
        }
        let mut dirt = (occlusion.powf(gain).min(1.0) * dirtscale).min(1.0);
        if let DirtFor::Lamp(lamp, dist) = who
            && lamp.keys.dirt_on_radius.is_changed()
            && lamp.keys.dirt_off_radius.is_changed()
        {
            let (on, off) = (
                lamp.keys.dirt_on_radius.get(),
                lamp.keys.dirt_off_radius.get(),
            );
            if dist < off {
                dirt = 0.0;
            } else if dist < on {
                dirt *= (dist - off) / (on - off);
            }
        }
        1.0 - dirt
    }

    /// `CullLight`: whether `lamp` cannot reach `patch`: what it sees misses the face's box,
    /// or it is too dim at the nearest point of the sphere around the face (LIGHT-10.8).
    fn culled(&self, lamp: &Lamp, patch: &Patch) -> bool {
        if let Some(bounds) = lamp.bounds
            && bounds.is_apart_from(patch.extents.bounds, BOUNDS_EPSILON)
            && lamp.keys.light_channel_mask.get() == CHANNEL_DEFAULT
            && lamp.keys.shadow_channel_mask.get() == CHANNEL_DEFAULT
        {
            return true;
        }
        let dist = distance(lamp.origin(), patch.extents.origin) - patch.extents.radius;
        dist >= 0.0 && lamp.value_at(self.world(), dist).abs() <= GATE
    }

    fn asker(&self, patch: &Patch, shadow_mask: i32) -> Asker {
        Asker {
            model: Some(ModelId(patch.model)),
            switch_style: self
                .map
                .models
                .get(patch.model)
                .map_or(0, |model| model.keys.switchshadstyle.get()),
            shadow_mask,
        }
    }

    /// `LightFace_Entity`: what `lamp` gives each sample of `patch` it reaches
    /// (LIGHT-10.9).
    fn lamp_on_face(
        &self,
        lamp: &Lamp,
        patch: &Patch,
        lightmaps: &mut Lightmaps,
        stack: &mut Stack,
    ) {
        let keys = &lamp.keys;
        if patch.plane.distance_to(lamp.origin()) < 0.0
            && !keys.bleed.get()
            && !patch.curved
            && !patch.twosided
        {
            return;
        }
        // With visibility in the BSP, a light whose leaf the face's leaves cannot see.
        if keys.light_channel_mask.get() == CHANNEL_DEFAULT
            && keys.shadow_channel_mask.get() == CHANNEL_DEFAULT
            && is_unseen(self.map.bsp, &patch.seen, lamp.leaf)
        {
            return;
        }
        if self.culled(lamp, patch) || keys.light_channel_mask.get() & patch.channels == 0 {
            return;
        }
        let asker = self.asker(patch, keys.shadow_channel_mask.get());
        let own_style = keys.style.get();
        let mut slot: Option<(i32, usize)> = None;
        for (index, sample) in patch.samples.iter().enumerate() {
            if sample.occluded {
                continue;
            }
            let facing = Facing {
                normal: Some(sample.normal),
                twosided: patch.twosided,
            };
            let given = lamp.contribution(self.map, sample.point, facing);
            let dirt = self.dirt_scale(
                sample.occlusion,
                DirtFor::Lamp(lamp, given.dist),
                Some(patch),
            );
            let colour = scale(given.colour, dirt);
            if brightness(colour).abs() <= GATE {
                continue;
            }
            let Some(passed) = self.rays.occluded(
                (sample.point, given.dir, given.dist),
                &asker,
                self.map,
                stack,
            ) else {
                continue;
            };
            self.add(lightmaps, &mut slot, (own_style, passed), index, colour);
        }
    }

    /// A ray's light added to the lightmap of its style: the light's own, or for a light of
    /// style 0 the style of a switchable shadow the ray crossed (LIGHT-11.5).
    fn add(
        &self,
        lightmaps: &mut Lightmaps,
        slot: &mut Option<(i32, usize)>,
        (own_style, passed): (i32, Passed),
        sample: usize,
        colour: Vec3,
    ) {
        let style = if own_style == 0 {
            passed.dynamic_style
        } else {
            own_style
        };
        let at = match *slot {
            Some((cached, at)) if cached == style => at,
            _ => {
                let at = lightmaps.for_style(style);
                *slot = Some((style, at));
                at
            }
        };
        lightmaps.reach(at, style, sample, passed.tinted(colour));
    }

    /// `LightFace_Sky`: what `sun` gives each sample of `patch` that sees the sky along it
    /// (LIGHT-14.7).
    fn sun_on_face(&self, sun: &Sun, patch: &Patch, lightmaps: &mut Lightmaps, stack: &mut Stack) {
        let incoming = normalize(sun.vec);
        if dot(incoming, patch.plane.normal) < -ANGLE_EPSILON && !patch.curved && !patch.twosided {
            return;
        }
        if !patch.is_on_default_channel() {
            return;
        }
        let asker = self.asker(patch, CHANNEL_DEFAULT);
        let mut slot: Option<(i32, usize)> = None;
        for (index, sample) in patch.samples.iter().enumerate() {
            if sample.occluded {
                continue;
            }
            let mut angle = dot(incoming, sample.normal);
            if patch.twosided && angle < 0.0 {
                angle = -angle;
            }
            let angle = (1.0 - sun.anglescale) + sun.anglescale * angle.max(0.0);
            let mut value = angle * sun.light;
            if sun.dirt {
                value *= self.dirt_scale(sample.occlusion, DirtFor::Ambient, Some(patch));
            }
            let colour = scale(sun.color, value / FULL);
            if brightness(colour).abs() <= GATE {
                continue;
            }
            let Some((passed, sky)) =
                self.rays
                    .sees_sky((sample.point, incoming, SKY_DIST), &asker, self.map, stack)
            else {
                continue;
            };
            // LIGHT-14.8: a sun with a texture comes only through sky of that texture.
            if sun.sky_texture.is_some() && self.map.texture_of(sky) != sun.sky_texture {
                continue;
            }
            self.add(lightmaps, &mut slot, (sun.style, passed), index, colour);
        }
    }

    /// `LightFace_Min`: every sample of `patch` raised to `light` of `colour` in the lightmap
    /// of `style`, or with `_addmin` given that much more (LIGHT-17.1, LIGHT-17.2).
    fn min_on_face(
        &self,
        (colour, light): (Vec3, f32),
        patch: &Patch,
        lightmaps: &mut Lightmaps,
        style: i32,
    ) {
        let world = self.world();
        let Some(face) = self.map.face(patch.face) else {
            return;
        };
        if self.map.flags_of(face).no_minlight {
            return;
        }
        let at = lightmaps.for_style(style);
        let mut hit = false;
        if let Some(lightmap) = lightmaps.all.get_mut(at) {
            for (sample, total) in patch.samples.iter().zip(&mut lightmap.samples) {
                let mut value = light;
                if world.minlight_dirt.get() {
                    value *= self.dirt_scale(sample.occlusion, DirtFor::Ambient, Some(patch));
                }
                if world.addminlight.get() {
                    *total = add(*total, scale(colour, value / FULL));
                    hit = true;
                } else {
                    if patch.mottle {
                        value += mottle(sample.point);
                    }
                    hit |= clamp_min(total, value, colour);
                }
            }
            if hit {
                lightmap.style.get_or_insert(style);
            }
        }
    }

    /// `LightFace_LocalMin`: each light whose `delay` is 4 raises the samples of `patch` it
    /// can see to its light (LIGHT-17.3).
    fn local_min_on_face(&self, patch: &Patch, lightmaps: &mut Lightmaps, stack: &mut Stack) {
        let world = self.world();
        let Some(face) = self.map.face(patch.face) else {
            return;
        };
        let flags = self.map.flags_of(face);
        if flags.no_minlight || flags.light_ignore || self.ignores_light(patch) {
            return;
        }
        for lamp in &self.lights.lamps {
            let keys = &lamp.keys;
            if keys.formula.get() != Formula::LocalMin || keys.nostaticlight.get() {
                continue;
            }
            if self.culled(lamp, patch) {
                continue;
            }
            if keys.light_channel_mask.get() & patch.channels == 0 {
                return;
            }
            let asker = self.asker(patch, keys.shadow_channel_mask.get());
            let (style, light, colour) = (keys.style.get(), keys.light.get(), keys.color.get());
            let at = lightmaps.for_style(style);
            let mut hit = false;
            for (index, sample) in patch.samples.iter().enumerate() {
                if sample.occluded {
                    continue;
                }
                let current = lightmaps
                    .all
                    .get(at)
                    .and_then(|lm| lm.samples.get(index))
                    .copied()
                    .unwrap_or_default();
                if !world.addminlight.get() && brightness(current) >= light {
                    continue;
                }
                let (dir, dist) = normalize_with_length(sub(lamp.origin(), sample.point));
                if self
                    .rays
                    .occluded((sample.point, dir, dist), &asker, self.map, stack)
                    .is_none()
                {
                    continue;
                }
                let value = light
                    * self.dirt_scale(sample.occlusion, DirtFor::Lamp(lamp, 0.0), Some(patch));
                let Some(total) = lightmaps
                    .all
                    .get_mut(at)
                    .and_then(|lm| lm.samples.get_mut(index))
                else {
                    continue;
                };
                if world.addminlight.get() {
                    *total = add(*total, scale(colour, value / FULL));
                    hit = true;
                } else {
                    hit |= clamp_min(total, value, colour);
                }
            }
            if hit && let Some(lightmap) = lightmaps.all.get_mut(at) {
                lightmap.style.get_or_insert(style);
            }
        }
    }

    /// Whether no light but the minimum reaches `patch`: `_lightignore` on its model or its
    /// face.
    fn ignores_light(&self, patch: &Patch) -> bool {
        let model = self
            .map
            .models
            .get(patch.model)
            .is_some_and(|model| model.keys.lightignore.get());
        let face = self
            .map
            .face(patch.face)
            .is_some_and(|face| self.map.flags_of(face).light_ignore);
        model || face
    }

    /// The lights and suns whose light is over 0, or under it, on `patch`.
    fn lights_on_face(
        &self,
        patch: &Patch,
        lightmaps: &mut Lightmaps,
        stack: &mut Stack,
        pass: Pass,
    ) {
        let wanted = |light: f32| pass.takes(light);
        for lamp in &self.lights.lamps {
            if lamp.keys.formula.get() == Formula::LocalMin || lamp.keys.nostaticlight.get() {
                continue;
            }
            if wanted(lamp.keys.light.get()) {
                self.lamp_on_face(lamp, patch, lightmaps, stack);
            }
        }
        for sun in &self.lights.suns {
            if wanted(sun.light) {
                self.sun_on_face(sun, patch, lightmaps, stack);
            }
        }
    }

    /// `DirectLightFace`: the dirt of `patch`, then the lights and suns that add light and
    /// the local minimum lights (LIGHT-8.4).
    pub(crate) fn direct(&self, patch: &mut Patch, stack: &mut Stack) -> Lightmaps {
        let mut lightmaps = Lightmaps::new(patch.samples.len());
        if let Some(dirt) = self.dirt {
            // A model's dirt rays are stopped only by what is on its own channels.
            let asker = self.asker(patch, patch.channels);
            dirt.occlude(patch, self.rays, &asker, self.map, stack);
        }
        if !self.ignores_light(patch) {
            self.lights_on_face(patch, &mut lightmaps, stack, Pass::Adding);
        }
        self.local_min_on_face(patch, &mut lightmaps, stack);
        lightmaps
    }

    /// `IndirectLightFace`: the light the faces of `emitters` give back on bounce `depth`
    /// added to `patch` (LIGHT-20.5).
    pub(crate) fn indirect(
        &self,
        patch: &Patch,
        lightmaps: &mut Lightmaps,
        emitters: &[Option<Emitter>],
        depth: usize,
        stack: &mut Stack,
    ) {
        if self.ignores_light(patch) || !patch.is_on_default_channel() {
            return;
        }
        let world = self.world();
        let asker = self.asker(patch, CHANNEL_DEFAULT);
        for emitter in emitters.iter().flatten() {
            for emission in &emitter.emissions {
                if emission.depth != depth
                    || emitter.is_culled(world, emission, patch)
                    || emitter
                        .leaves
                        .iter()
                        .any(|leaf| is_unseen(self.map.bsp, &patch.seen, Some(*leaf)))
                {
                    continue;
                }
                let mut at: Option<usize> = None;
                for (index, sample) in patch.samples.iter().enumerate() {
                    if sample.occluded {
                        continue;
                    }
                    let (dir, dist) = emitter.toward(sample.point);
                    let normal = (!patch.twosided).then_some(sample.normal);
                    let given = emitter.lighting(world, emission, (dir, dist), normal);
                    if given.iter().all(|c| *c <= BOUNCE_GATE) {
                        continue;
                    }
                    let Some(passed) =
                        self.rays
                            .occluded((emitter.pos, dir, dist), &asker, self.map, stack)
                    else {
                        continue;
                    };
                    let dirt = self.dirt_scale(sample.occlusion, DirtFor::Ambient, Some(patch));
                    let slot = *at.get_or_insert_with(|| lightmaps.for_style(emission.style));
                    lightmaps.reach(
                        slot,
                        emission.style,
                        index,
                        scale(passed.tinted(given), dirt),
                    );
                }
            }
        }
    }

    /// `PostProcessLightFace`: the minimum light of `patch`, for a brush model the light at
    /// its middle on its hidden samples, and last the lights that take light away
    /// (LIGHT-8.4).
    pub(crate) fn post(
        &self,
        patch: &mut Patch,
        lightmaps: &mut Lightmaps,
        emitters: &[Option<Emitter>],
        stack: &mut Stack,
    ) {
        let world = self.world();
        let (minlight, colour) = if patch.minlight > world.minlight.get() {
            (patch.minlight, patch.minlight_color)
        } else {
            (world.minlight.get(), world.minlight_color.get())
        };
        if minlight != 0.0 {
            self.min_on_face((colour, minlight), patch, lightmaps, 0);
        }
        if patch.model != 0 {
            self.auto_min_on_face(patch, lightmaps, emitters, stack);
        }
        if !self.ignores_light(patch) {
            self.lights_on_face(patch, lightmaps, stack, Pass::Taking);
        }
    }

    /// `LightFace_AutoMin`: the hidden samples of a brush model's face raised to the light
    /// at the model's middle, or every sample with `_autominlight`; the samples then count as
    /// seen (LIGHT-17.4).
    fn auto_min_on_face(
        &self,
        patch: &mut Patch,
        lightmaps: &mut Lightmaps,
        emitters: &[Option<Emitter>],
        stack: &mut Stack,
    ) {
        let Some(model) = self.map.models.get(patch.model) else {
            return;
        };
        let every = if model.keys.autominlight.is_changed() {
            if !model.keys.autominlight.get() {
                return;
            }
            true
        } else {
            if !patch.samples.iter().any(|sample| sample.occluded) {
                return;
            }
            false
        };
        let mut middle = self
            .map
            .bsp
            .models
            .get(patch.model)
            .map_or([0.0; AXES], |m| scale(add(m.mins, m.maxs), HALF));
        let target = model.keys.autominlight_target.get();
        if !target.is_empty()
            && let Some(entity) = self
                .lights
                .dicts
                .iter()
                .find(|dict| dict.get(b"targetname") == target.as_slice())
        {
            middle = entity.vector(b"origin");
        }
        let Some(at_middle) = self.light_at(middle, emitters, stack) else {
            return;
        };
        for (style, colour) in at_middle {
            let at = lightmaps.for_style(style);
            let Some(lightmap) = lightmaps.all.get_mut(at) else {
                continue;
            };
            for (sample, total) in patch.samples.iter().zip(&mut lightmap.samples) {
                if every || sample.occluded {
                    *total = [
                        total[0].max(colour[0]),
                        total[1].max(colour[1]),
                        total[2].max(colour[2]),
                    ];
                }
            }
            lightmap.style.get_or_insert(style);
        }
        for sample in &mut patch.samples {
            sample.occluded = false;
        }
    }

    /// `FixPointAndCalcLightgrid`: the light at `point` for each style, scaled as a
    /// lightmap's is; none for a point in the world's solid with no open space 2 units from
    /// it (LIGHT-17.5).
    fn light_at(
        &self,
        point: Vec3,
        emitters: &[Option<Emitter>],
        stack: &mut Stack,
    ) -> Option<Vec<(i32, Vec3)>> {
        let point = self.map.out_of_solid(point)?;
        let mut cube = PointLight::default();
        for pass in [Pass::Adding, Pass::Taking] {
            let wanted = |light: f32| pass.takes(light);
            for lamp in &self.lights.lamps {
                let keys = &lamp.keys;
                if keys.formula.get() == Formula::LocalMin
                    || keys.nostaticlight.get()
                    || !wanted(keys.light.get())
                    || keys.light_channel_mask.get() != CHANNEL_DEFAULT
                {
                    continue;
                }
                let facing = Facing {
                    normal: None,
                    twosided: false,
                };
                let given = lamp.contribution(self.map, point, facing);
                if brightness(given.colour).abs() <= GATE {
                    continue;
                }
                if let Some(passed) = self.rays.occluded(
                    (point, given.dir, given.dist),
                    &Asker::NOBODY,
                    self.map,
                    stack,
                ) {
                    cube.add(
                        passed.tinted(given.colour),
                        keys.style.get(),
                        given.dir,
                        keys.anglescale.get(),
                    );
                }
            }
            for sun in &self.lights.suns {
                if !wanted(sun.light) {
                    continue;
                }
                let incoming = normalize(sun.vec);
                let colour = scale(sun.color, sun.light / FULL);
                if brightness(colour).abs() <= GATE {
                    continue;
                }
                let struck =
                    self.rays
                        .struck((point, incoming, SKY_DIST), &Asker::NOBODY, self.map, stack);
                if matches!(struck.hit, Some((Surface::Sky, _))) {
                    cube.add(
                        struck.passed.tinted(colour),
                        sun.style,
                        incoming,
                        sun.anglescale,
                    );
                }
            }
        }
        let world = self.world();
        // `LightPoint_SurfaceLight`: every bounce's light, falling on the point whole.
        for emitter in emitters.iter().flatten() {
            for emission in &emitter.emissions {
                let away = sub(point, emitter.pos);
                let dist = length(away);
                let dir = if dist == 0.0 {
                    [0.0, 0.0, 1.0]
                } else {
                    away.map(|c| c / dist)
                };
                let given = emitter.lighting(world, emission, (dir, dist), None);
                if given.iter().all(|c| *c <= BOUNCE_GATE) {
                    continue;
                }
                if let Some(passed) =
                    self.rays
                        .occluded((emitter.pos, dir, dist), &Asker::NOBODY, self.map, stack)
                {
                    cube.add(passed.tinted(given), emission.style, scale(dir, -1.0), 1.0);
                }
            }
        }
        Some(
            cube.styles
                .iter()
                .map(|side| (side.style, world.scaled_and_clamped(side.flat, 0.0, 1.0)))
                .collect(),
        )
    }
}

/// The light of one style at a point: on each side of a cube, and taking each light at the
/// side it lights most (`lightgrid_sample_t`).
#[derive(Clone, Copy, Default)]
struct StyleAtPoint {
    style: i32,
    sides: [Vec3; CUBE_SIDES],
    flat: Vec3,
}

impl StyleAtPoint {
    /// `lightgrid_sample_t::add`.
    fn add(&mut self, colour: Vec3, to_light: Vec3, anglescale: f32) {
        let mut most = 0.0_f32;
        // The sides face along x, y and z, each way.
        let facings = to_light.into_iter().flat_map(|along| [along, -along]);
        for (total, facing) in self.sides.iter_mut().zip(facings) {
            if facing < 0.0 {
                continue;
            }
            let coefficient = 1.0 + (facing - 1.0) * anglescale;
            *total = add(*total, scale(colour, coefficient));
            most = most.max(coefficient);
        }
        self.flat = add(self.flat, scale(colour, most));
    }

    /// `lightgrid_sample_t::brightness`.
    fn brightness(&self) -> f32 {
        let total: f32 = self.sides.iter().flatten().sum();
        total / SIDE_CHANNELS
    }
}

/// The light at a point, for up to four styles (`lightgrid_samples_t`).
#[derive(Default)]
struct PointLight {
    styles: Vec<StyleAtPoint>,
}

impl PointLight {
    /// `lightgrid_samples_t::add`: `colour` added to its style's light; with four styles
    /// kept, a fifth takes the place of the dimmest.
    fn add(&mut self, colour: Vec3, style: i32, to_light: Vec3, anglescale: f32) {
        if let Some(known) = self.styles.iter_mut().find(|side| side.style == style) {
            known.add(colour, to_light, anglescale);
            return;
        }
        let mut new = StyleAtPoint {
            style,
            ..StyleAtPoint::default()
        };
        new.add(colour, to_light, anglescale);
        if self.styles.len() < POINT_STYLES {
            self.styles.push(new);
            return;
        }
        let mut dimmest: Option<(usize, f32)> = None;
        for (at, side) in self.styles.iter().enumerate() {
            let bright = side.brightness();
            if dimmest.is_none_or(|(_, least)| bright < least) {
                dimmest = Some((at, bright));
            }
        }
        if let Some(slot) = dimmest.and_then(|(at, _)| self.styles.get_mut(at)) {
            *slot = new;
        }
    }
}

impl World {
    /// `LightFace_ScaleAndClamp` for one sample: `colour` with nothing under 0, held to
    /// `maxlight` of the face or of the world, its colour faded toward grey by
    /// `lightcolorscale`, scaled by `_range` and bent by `_gamma` (LIGHT-18.1).
    #[expect(
        clippy::float_cmp,
        reason = "spec LIGHT-18.1: ericw-tools tests lightcolorscale and gamma against exactly 1"
    )]
    pub(crate) fn scaled_and_clamped(
        &self,
        colour: Vec3,
        maxlight: f32,
        lightcolorscale: f32,
    ) -> Vec3 {
        let world = self;
        let mut colour = colour.map(|c| c.max(0.0));
        let most = |c: Vec3| c[0].max(c[1]).max(c[2]);
        let limit = if maxlight != 0.0 {
            maxlight
        } else {
            world.maxlight.get()
        };
        if limit != 0.0 {
            let (brightest, limit) = (most(colour), limit * 2.0);
            if brightest > limit {
                colour = scale(colour, limit / brightest);
            }
        }
        if lightcolorscale != 1.0 {
            let grey = most(colour);
            colour = colour.map(|c| grey + (c - grey) * lightcolorscale);
        }
        colour = scale(colour, world.rangescale.get());
        let gamma = world.gamma.get();
        if gamma != 1.0 {
            colour = colour.map(|c| (c / FULL).powf(1.0 / gamma) * FULL);
        }
        colour
    }
}
