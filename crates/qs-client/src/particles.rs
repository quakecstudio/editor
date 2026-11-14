// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/r_part.c, R_InitParticles, R_ClearParticles, R_EntityParticles, R_ParseParticleEffect, R_ParticleExplosion, R_ParticleExplosion2, R_BlobExplosion, R_RunParticleEffect, R_LavaSplash, R_TeleportSplash, R_RocketTrail, R_DrawParticles

//! The particle system (`docs/spec/engine/particles.md`, PART-1 to PART-3): the pool, each
//! effect's particles and how they move and change colour each frame; drawing one is the
//! renderer's (PART-3.2).
//!
//! Implements PART-1 to PART-3.

use qc_builtins::math::vector_normalize;
use qc_vm::convert::{double_to_float, float_to_int, int_to_float};
use qs_mdl::mdl::MDL_VERTEX_NORMALS;

use crate::models::Random;

/// `MAX_PARTICLES` (PART-1.1).
pub const PART_MAX_PARTICLES: usize = 2048;
/// `ABSOLUTE_MIN_PARTICLES` (PART-1.1).
pub const PART_ABSOLUTE_MIN_PARTICLES: usize = 512;
/// `ramp1`, the explosion's colours (PART-3.1).
pub const PART_RAMP1: [i32; 8] = [0x6f, 0x6d, 0x6b, 0x69, 0x67, 0x65, 0x63, 0x61];
/// `ramp2`, the second explosion colours.
pub const PART_RAMP2: [i32; 8] = [0x6f, 0x6e, 0x6d, 0x6c, 0x6b, 0x6a, 0x68, 0x66];
/// `ramp3`, the fire colours; six are given and the last two are 0.
pub const PART_RAMP3: [i32; 8] = [0x6d, 0x6b, 6, 5, 4, 3, 0, 0];
/// `PART_GRAVITY_SCALE`: how much of `sv_gravity` a frame's gravity is (PART-3.1).
pub const PART_GRAVITY_SCALE: f32 = 0.05;
/// `sv_gravity`'s default, which the client reads (PART-3.1).
pub const PART_GRAVITY: f32 = 800.0;
/// `NUMVERTEXNORMALS`: the entity field's particles (PART-2.8).
pub const PART_ENTITY_PARTICLES: usize = 162;
/// `beamlength` of the entity field (PART-2.8).
const BEAM_LENGTH: f32 = 16.0;
/// `dist` of the entity field (PART-2.8).
const FIELD_DIST: f32 = 64.0;
/// The entity field's colour, its life in seconds, and its angular speeds' range: `(rand() &
/// 255) * 0.01` (PART-2.8).
const FIELD_COLOR: i32 = 0x6f;
const FIELD_LIFE: f32 = 0.01;
const FIELD_SPIN_MASK: i32 = 255;
const FIELD_SPIN_SCALE: f32 = 0.01;
/// No position or velocity yet.
const NOWHERE: [f32; 3] = [0.0; 3];

/// `R_ParticleExplosion`'s and `R_BlobExplosion`'s particles, and the count
/// `R_RunParticleEffect` takes as an explosion (PART-2.1, PART-2.3, PART-2.4).
pub(crate) const EXPLOSION_PARTICLES: i32 = 1024;
/// An explosion particle's life in seconds, and its starting ramp's mask (PART-2.1).
const EXPLOSION_LIFE: f32 = 5.0;
const EXPLOSION_RAMP_MASK: i32 = 3;
/// `R_ParticleExplosion2`'s particles and their life in seconds (PART-2.2).
const EXPLOSION2_PARTICLES: i32 = 512;
const EXPLOSION2_LIFE: f32 = 0.3;
/// A blob particle's extra life: `(rand() & 8) * 0.05` seconds (PART-2.3).
const BLOB_LIFE_MASK: i32 = 8;
const BLOB_LIFE_STEP: f32 = 0.05;
/// The blob explosion's two base colours and the spread above each (PART-2.3).
const BLOB_COLOR: i32 = 66;
const BLOB2_COLOR: i32 = 150;
const BLOB_COLORS: i32 = 6;
/// `R_RunParticleEffect`: life in tenths of a second below 5, the colour's low bits, the
/// jitter `(rand() & 15) - 8`, and the speed along `dir` (PART-2.4).
const EFFECT_LIFE_STEPS: i32 = 5;
const EFFECT_LIFE_STEP: f32 = 0.1;
const COLOR_SPREAD: i32 = 7;
/// `rand() & 7`: the lava splash's jitter within a cell (PART-2.5).
const CELL_JITTER_MASK: i32 = 7;
/// `(rand() & 7) * 0.02`: the teleport splash's extra life (PART-2.6).
const TELEPORT_LIFE_MASK: i32 = 7;
const JITTER_MASK: i32 = 15;
const JITTER_CENTRE: i32 = 8;
const EFFECT_SPEED: f32 = 15.0;
/// `R_LavaSplash` and `R_TeleportSplash`'s grid: -16 to 16 across, with the teleport's
/// height -24 to 32, a cell every 4 for the teleport, and each cell 8 units of direction
/// (PART-2.5, PART-2.6).
const SPLASH_HALF: i32 = 16;
const TELEPORT_LOW: i32 = -24;
const TELEPORT_HIGH: i32 = 32;
const TELEPORT_STEP: usize = 4;
const SPLASH_CELL: i32 = 8;
/// The lava splash's life, `2 + (rand() & 31) * 0.02`, its colour, its upward direction,
/// and its height's spread (PART-2.5).
const LAVA_LIFE: f32 = 2.0;
const LAVA_LIFE_MASK: i32 = 31;
const SPLASH_LIFE_STEP: f32 = 0.02;
const LAVA_COLOR: i32 = 224;
const LAVA_UP: f32 = 256.0;
const LAVA_HEIGHT_MASK: i32 = 63;
/// A splash particle's speed, `50 + (rand() & 63)` (PART-2.5, PART-2.6).
const SPLASH_SPEED: i32 = 50;
const SPLASH_SPEED_MASK: i32 = 63;
/// The teleport splash's life, `0.2 + (rand() & 7) * 0.02`, its colour and its position's
/// spread (PART-2.6).
const TELEPORT_LIFE: f32 = 0.2;
const TELEPORT_COLOR: i32 = 7;
const TELEPORT_SPREAD_MASK: i32 = 3;
/// An explosion particle's origin `(rand() % 32) - 16` and velocity `(rand() % 512) - 256`
/// from the explosion (PART-2.1).
const SCATTER_ORIGIN: i32 = 32;
const SCATTER_ORIGIN_CENTRE: i32 = 16;
const SCATTER_SPEED: i32 = 512;
const SCATTER_SPEED_CENTRE: i32 = 256;
/// `R_DrawParticles`' rates per second: the fire ramp, the two explosion ramps and the
/// explosion's growth (PART-3.1).
const FIRE_RAMP_RATE: f32 = 5.0;
const EXPLODE_RAMP_RATE: f32 = 10.0;
const EXPLODE2_RAMP_RATE: f32 = 15.0;
const EXPLODE_GROWTH: f32 = 4.0;
/// Where a particle dies off the end of the fire ramp and the explosion ramps (PART-3.1).
const FIRE_RAMP_END: f32 = 6.0;
const EXPLODE_RAMP_END: f32 = 8.0;
/// A time before any clock, which frees the particle on the next frame.
const DEAD: f32 = -1.0;

/// `ptype_t`: how a particle moves and recolours each frame (PART-3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticleType {
    /// `pt_static`.
    Static,
    /// `pt_grav`.
    Grav,
    /// `pt_slowgrav`.
    SlowGrav,
    /// `pt_fire`.
    Fire,
    /// `pt_explode`.
    Explode,
    /// `pt_explode2`.
    Explode2,
    /// `pt_blob`.
    Blob,
    /// `pt_blob2`.
    Blob2,
}

/// One particle, `particle_t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    /// Its position.
    pub origin: [f32; 3],
    /// Its velocity.
    pub velocity: [f32; 3],
    /// Its palette colour.
    pub color: i32,
    /// Its place in a colour ramp.
    pub ramp: f32,
    /// When it dies, on the client clock.
    pub die: f32,
    /// How it moves.
    pub kind: ParticleType,
}

impl Particle {
    /// A particle at `origin`, still, of `color`, dying at `die`, moving as `kind`.
    const fn at(origin: [f32; 3], color: i32, die: f32, kind: ParticleType) -> Self {
        Self {
            origin,
            velocity: NOWHERE,
            color,
            ramp: 0.0,
            die,
            kind,
        }
    }
}

/// A rocket trail's kind, `R_RocketTrail`'s `type` below 128 (PART-2.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trail {
    /// 0: a rocket's fire.
    Rocket,
    /// 1: a grenade's smoke, two steps up the fire ramp.
    Smoke,
    /// 2: a gib's blood.
    Blood,
    /// 3: a wizard's green tracer.
    Tracer,
    /// 4: a zombie gib's slight blood, every other step.
    SlightBlood,
    /// 5: a knight's orange tracer.
    Tracer2,
    /// 6: a vore ball's purple trail.
    Voor,
}

/// How far apart a trail's particles are: `type` below 128, or 128 and up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrailSpacing {
    /// Three units apart.
    Normal,
    /// One unit apart.
    Dense,
}

/// The units between a trail's particles (PART-2.7).
const TRAIL_STEP: f32 = 3.0;
/// The units between a dense trail's particles.
const TRAIL_DENSE_STEP: f32 = 1.0;
/// Where smoke starts on the fire ramp.
const SMOKE_RAMP_START: f32 = 2.0;
/// Blood's base colour.
const BLOOD_COLOR: i32 = 67;
/// A tracer's life in seconds.
const TRACER_LIFE: f32 = 0.5;
/// The wizard tracer's base colour.
const TRACER_COLOR: i32 = 52;
/// The knight tracer's base colour.
const TRACER2_COLOR: i32 = 230;
/// A tracer's sideways speed.
const TRACER_SPEED: f32 = 30.0;
/// The vore trail's base colour.
const VOOR_COLOR: i32 = 152;
/// A vore trail particle's life in seconds.
const VOOR_LIFE: f32 = 0.3;
/// A trail particle's life in seconds, but a tracer's or a vore trail's.
const TRAIL_LIFE: f32 = 2.0;
/// A trail's jitter, `(rand() % 6) - 3`.
const TRAIL_JITTER: i32 = 6;
const TRAIL_JITTER_CENTRE: i32 = 3;
/// The ramp and blood colour spread of a trail, `rand() & 3`.
const TRAIL_SPREAD_MASK: i32 = 3;
/// The bit of `tracercount` that picks a tracer's brighter colour.
const TRACER_BRIGHT: i32 = 4;

/// The particle pool: the active particles, newest first as id's list keeps them, and how
/// many may be active (PART-1.1).
#[derive(Clone, Debug, PartialEq)]
pub struct Particles {
    active: Vec<Particle>,
    capacity: usize,
    /// `tracercount`, kept across trails (PART-2.7).
    tracercount: i32,
    /// `avelocities`, drawn once (PART-2.8).
    avelocities: Option<Vec<[f32; 3]>>,
}

impl Default for Particles {
    fn default() -> Self {
        Self::new(PART_MAX_PARTICLES)
    }
}

impl Particles {
    /// A pool of `capacity` particles, at least `PART_ABSOLUTE_MIN_PARTICLES` (PART-1.1).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            active: Vec::new(),
            capacity: capacity.max(PART_ABSOLUTE_MIN_PARTICLES),
            tracercount: 0,
            avelocities: None,
        }
    }

    /// `R_ClearParticles`: every particle freed (PART-1.1).
    pub fn clear(&mut self) {
        self.active.clear();
    }

    /// The live particles, newest first.
    #[must_use]
    pub fn active(&self) -> &[Particle] {
        &self.active
    }

    /// How many particles the pool holds.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Takes one particle from the free list (PART-1.2): `None` when none is free.
    fn alloc(&mut self, particle: Particle) -> bool {
        if self.active.len() >= self.capacity {
            return false;
        }
        self.active.insert(0, particle);
        true
    }

    /// `R_ParticleExplosion (org)` (PART-2.1).
    pub fn explosion(&mut self, rand: &mut dyn Random, time: f32, org: [f32; 3]) {
        for i in 0..EXPLOSION_PARTICLES {
            let mut p = Particle::at(
                NOWHERE,
                PART_RAMP1[0],
                time + EXPLOSION_LIFE,
                ParticleType::Explode,
            );
            p.ramp = int_to_float(rand.rand() & EXPLOSION_RAMP_MASK);
            if i & 1 == 0 {
                p.kind = ParticleType::Explode2;
            }
            scatter(rand, &mut p, org);
            if !self.alloc(p) {
                return;
            }
        }
    }

    /// `R_ParticleExplosion2 (org, colorStart, colorLength)` (PART-2.2).
    pub fn explosion2(
        &mut self,
        rand: &mut dyn Random,
        time: f32,
        org: [f32; 3],
        color_start: i32,
        color_length: i32,
    ) {
        for i in 0..EXPLOSION2_PARTICLES {
            let color = color_start.wrapping_add(i.checked_rem(color_length).unwrap_or(0));
            let mut p = Particle::at(NOWHERE, color, time + EXPLOSION2_LIFE, ParticleType::Blob);
            scatter(rand, &mut p, org);
            if !self.alloc(p) {
                return;
            }
        }
    }

    /// `R_BlobExplosion (org)` (PART-2.3).
    pub fn blob_explosion(&mut self, rand: &mut dyn Random, time: f32, org: [f32; 3]) {
        for i in 0..EXPLOSION_PARTICLES {
            let die = time + 1.0 + int_to_float(rand.rand() & BLOB_LIFE_MASK) * BLOB_LIFE_STEP;
            let (kind, base): (ParticleType, i32) = if i & 1 == 1 {
                (ParticleType::Blob, BLOB_COLOR)
            } else {
                (ParticleType::Blob2, BLOB2_COLOR)
            };
            let color = base.wrapping_add(rand.rand().checked_rem(BLOB_COLORS).unwrap_or(0));
            let mut p = Particle::at(NOWHERE, color, die, kind);
            scatter(rand, &mut p, org);
            if !self.alloc(p) {
                return;
            }
        }
    }

    /// `R_RunParticleEffect (org, dir, color, count)` (PART-2.4): 1024 is an explosion.
    pub fn run_effect(
        &mut self,
        rand: &mut dyn Random,
        time: f32,
        org: [f32; 3],
        dir: [f32; 3],
        color: i32,
        count: i32,
    ) {
        if count == EXPLOSION_PARTICLES {
            self.explosion(rand, time, org);
            return;
        }
        for _ in 0..count.max(0) {
            let steps = rand.rand().checked_rem(EFFECT_LIFE_STEPS).unwrap_or(0);
            let die = time + EFFECT_LIFE_STEP * int_to_float(steps);
            let c = (color & !COLOR_SPREAD).wrapping_add(rand.rand() & COLOR_SPREAD);
            let mut p = Particle::at(NOWHERE, c, die, ParticleType::SlowGrav);
            let [ox, oy, oz] = org;
            let jx = jitter(rand);
            let jy = jitter(rand);
            let jz = jitter(rand);
            p.origin = [ox + jx, oy + jy, oz + jz];
            p.velocity = dir.map(|d| d * EFFECT_SPEED);
            if !self.alloc(p) {
                return;
            }
        }
    }

    /// `R_LavaSplash (org)` (PART-2.5).
    pub fn lava_splash(&mut self, rand: &mut dyn Random, time: f32, org: [f32; 3]) {
        for i in -SPLASH_HALF..SPLASH_HALF {
            for j in -SPLASH_HALF..SPLASH_HALF {
                let life = int_to_float(rand.rand() & LAVA_LIFE_MASK) * SPLASH_LIFE_STEP;
                let die = time + LAVA_LIFE + life;
                let color = LAVA_COLOR.wrapping_add(rand.rand() & COLOR_SPREAD);
                let cell = |n: i32, r: i32| n.wrapping_mul(SPLASH_CELL).wrapping_add(r);
                let dx = int_to_float(cell(j, rand.rand() & CELL_JITTER_MASK));
                let dy = int_to_float(cell(i, rand.rand() & CELL_JITTER_MASK));
                let dir = [dx, dy, LAVA_UP];
                let [ox, oy, oz] = org;
                let origin = [
                    ox + dx,
                    oy + dy,
                    oz + int_to_float(rand.rand() & LAVA_HEIGHT_MASK),
                ];
                let (unit, _) = vector_normalize(dir);
                let vel = splash_speed(rand);
                let mut p = Particle::at(origin, color, die, ParticleType::SlowGrav);
                p.velocity = unit.map(|d| d * vel);
                if !self.alloc(p) {
                    return;
                }
            }
        }
    }

    /// `R_TeleportSplash (org)` (PART-2.6).
    pub fn teleport_splash(&mut self, rand: &mut dyn Random, time: f32, org: [f32; 3]) {
        for i in (-SPLASH_HALF..SPLASH_HALF).step_by(TELEPORT_STEP) {
            for j in (-SPLASH_HALF..SPLASH_HALF).step_by(TELEPORT_STEP) {
                for k in (TELEPORT_LOW..TELEPORT_HIGH).step_by(TELEPORT_STEP) {
                    let life = int_to_float(rand.rand() & TELEPORT_LIFE_MASK) * SPLASH_LIFE_STEP;
                    let die = time + TELEPORT_LIFE + life;
                    let color = TELEPORT_COLOR.wrapping_add(rand.rand() & COLOR_SPREAD);
                    let dir = [
                        int_to_float(j.wrapping_mul(SPLASH_CELL)),
                        int_to_float(i.wrapping_mul(SPLASH_CELL)),
                        int_to_float(k.wrapping_mul(SPLASH_CELL)),
                    ];
                    let [ox, oy, oz] = org;
                    let spread = |n: i32, r: i32| int_to_float(n.wrapping_add(r));
                    let origin = [
                        ox + spread(i, rand.rand() & TELEPORT_SPREAD_MASK),
                        oy + spread(j, rand.rand() & TELEPORT_SPREAD_MASK),
                        oz + spread(k, rand.rand() & TELEPORT_SPREAD_MASK),
                    ];
                    let (unit, _) = vector_normalize(dir);
                    let vel = splash_speed(rand);
                    let mut p = Particle::at(origin, color, die, ParticleType::SlowGrav);
                    p.velocity = unit.map(|d| d * vel);
                    if !self.alloc(p) {
                        return;
                    }
                }
            }
        }
    }

    /// `R_RocketTrail (start, end, type)` (PART-2.7): `type` is `trail`, plus 128 for
    /// [`TrailSpacing::Dense`].
    pub fn rocket_trail(
        &mut self,
        rand: &mut dyn Random,
        time: f32,
        (start, end): ([f32; 3], [f32; 3]),
        trail: Trail,
        spacing: TrailSpacing,
    ) {
        let [sx, sy, sz] = start;
        let [ex, ey, ez] = end;
        let (vec, mut len) = vector_normalize([ex - sx, ey - sy, ez - sz]);
        let dec = match spacing {
            TrailSpacing::Normal => TRAIL_STEP,
            TrailSpacing::Dense => TRAIL_DENSE_STEP,
        };
        let mut at = start;
        while len > 0.0 {
            len -= dec;
            let mut p = Particle::at(at, 0, time + TRAIL_LIFE, ParticleType::Static);
            let [ax, ay, az] = at;
            let near = |rand: &mut dyn Random| {
                let r = rand.rand().checked_rem(TRAIL_JITTER).unwrap_or(0);
                int_to_float(r.wrapping_sub(TRAIL_JITTER_CENTRE))
            };
            match trail {
                Trail::Rocket | Trail::Smoke => {
                    let start = if trail == Trail::Smoke {
                        SMOKE_RAMP_START
                    } else {
                        0.0
                    };
                    p.ramp = int_to_float(rand.rand() & TRAIL_SPREAD_MASK) + start;
                    p.color = ramp3(p.ramp);
                    p.kind = ParticleType::Fire;
                    let (jx, jy, jz) = (near(rand), near(rand), near(rand));
                    p.origin = [ax + jx, ay + jy, az + jz];
                }
                Trail::Blood | Trail::SlightBlood => {
                    p.kind = ParticleType::Grav;
                    p.color = BLOOD_COLOR.wrapping_add(rand.rand() & TRAIL_SPREAD_MASK);
                    let (jx, jy, jz) = (near(rand), near(rand), near(rand));
                    p.origin = [ax + jx, ay + jy, az + jz];
                    if trail == Trail::SlightBlood {
                        len -= TRAIL_STEP;
                    }
                }
                Trail::Tracer | Trail::Tracer2 => {
                    p.die = time + TRACER_LIFE;
                    p.kind = ParticleType::Static;
                    let base: i32 = if trail == Trail::Tracer {
                        TRACER_COLOR
                    } else {
                        TRACER2_COLOR
                    };
                    p.color = base.wrapping_add((self.tracercount & TRACER_BRIGHT) << 1);
                    self.tracercount = self.tracercount.wrapping_add(1);
                    let [vx, vy, _] = vec;
                    p.velocity = if self.tracercount & 1 != 0 {
                        [TRACER_SPEED * vy, TRACER_SPEED * -vx, 0.0]
                    } else {
                        [TRACER_SPEED * -vy, TRACER_SPEED * vx, 0.0]
                    };
                }
                Trail::Voor => {
                    p.color = VOOR_COLOR.wrapping_add(rand.rand() & TRAIL_SPREAD_MASK);
                    p.kind = ParticleType::Static;
                    p.die = time + VOOR_LIFE;
                    let (jx, jy, jz) = (jitter(rand), jitter(rand), jitter(rand));
                    p.origin = [ax + jx, ay + jy, az + jz];
                }
            }
            if !self.alloc(p) {
                return;
            }
            let [vx, vy, vz] = vec;
            at = [ax + vx, ay + vy, az + vz];
        }
    }

    /// `R_EntityParticles (ent)`, the `EF_BRIGHTFIELD` field (PART-2.8): with velocity 0, a
    /// deliberate divergence from reading a dead particle's.
    pub fn entity_field(&mut self, rand: &mut dyn Random, time: f32, origin: [f32; 3]) {
        if self.avelocities.is_none() {
            let drawn = (0..PART_ENTITY_PARTICLES)
                .map(|_| {
                    let mut spin =
                        || int_to_float(rand.rand() & FIELD_SPIN_MASK) * FIELD_SPIN_SCALE;
                    let a = spin();
                    let b = spin();
                    let c = spin();
                    [a, b, c]
                })
                .collect();
            self.avelocities = Some(drawn);
        }
        let avelocities = self.avelocities.clone().unwrap_or_default();
        for (normal, avelocity) in MDL_VERTEX_NORMALS.iter().zip(avelocities.iter()) {
            let [ay, ap, ar] = *avelocity;
            let angle = f64::from(time * ay);
            let (sy, cy) = (double_to_float(angle.sin()), double_to_float(angle.cos()));
            let angle = f64::from(time * ap);
            let (sp, cp) = (double_to_float(angle.sin()), double_to_float(angle.cos()));
            let _roll = time * ar;
            let forward = [cp * cy, cp * sy, -sp];
            let [ox, oy, oz] = origin;
            let [nx, ny, nz] = *normal;
            let p = Particle::at(
                [
                    ox + nx * FIELD_DIST + forward[0] * BEAM_LENGTH,
                    oy + ny * FIELD_DIST + forward[1] * BEAM_LENGTH,
                    oz + nz * FIELD_DIST + forward[2] * BEAM_LENGTH,
                ],
                FIELD_COLOR,
                time + FIELD_LIFE,
                ParticleType::Explode,
            );
            if !self.alloc(p) {
                return;
            }
        }
    }

    /// `R_DrawParticles`' movement (PART-3.1): the dead are freed, then every particle is
    /// moved by its velocity and its type for a frame of `frametime` seconds, with
    /// `gravity` the server's.
    pub fn update(&mut self, time: f32, frametime: f32, gravity: f32) {
        self.active.retain(|p| p.die >= time);
        let time1 = frametime * FIRE_RAMP_RATE;
        let time2 = frametime * EXPLODE_RAMP_RATE;
        let time3 = frametime * EXPLODE2_RAMP_RATE;
        let grav = frametime * gravity * PART_GRAVITY_SCALE;
        let dvel = EXPLODE_GROWTH * frametime;
        for p in &mut self.active {
            let [vx, vy, vz] = p.velocity;
            let [ox, oy, oz] = p.origin;
            p.origin = [
                ox + vx * frametime,
                oy + vy * frametime,
                oz + vz * frametime,
            ];
            match p.kind {
                ParticleType::Static => {}
                ParticleType::Fire => {
                    p.ramp += time1;
                    if p.ramp >= FIRE_RAMP_END {
                        p.die = DEAD;
                    } else {
                        p.color = ramp3(p.ramp);
                    }
                    p.velocity[2] += grav;
                }
                ParticleType::Explode => {
                    p.ramp += time2;
                    if p.ramp >= EXPLODE_RAMP_END {
                        p.die = DEAD;
                    } else {
                        p.color = ramp(&PART_RAMP1, p.ramp);
                    }
                    p.velocity = p.velocity.map(|v| v + v * dvel);
                    p.velocity[2] -= grav;
                }
                ParticleType::Explode2 => {
                    p.ramp += time3;
                    if p.ramp >= EXPLODE_RAMP_END {
                        p.die = DEAD;
                    } else {
                        p.color = ramp(&PART_RAMP2, p.ramp);
                    }
                    p.velocity = p.velocity.map(|v| v - v * frametime);
                    p.velocity[2] -= grav;
                }
                ParticleType::Blob => {
                    p.velocity = p.velocity.map(|v| v + v * dvel);
                    p.velocity[2] -= grav;
                }
                ParticleType::Blob2 => {
                    let [vx, vy, vz] = p.velocity;
                    p.velocity = [vx - vx * dvel, vy - vy * dvel, vz - grav];
                }
                ParticleType::Grav | ParticleType::SlowGrav => {
                    p.velocity[2] -= grav;
                }
            }
        }
    }
}

/// `(rand() & 15) - 8`: a particle's jitter about its point (PART-2.4, PART-2.7).
fn jitter(rand: &mut dyn Random) -> f32 {
    int_to_float((rand.rand() & JITTER_MASK).wrapping_sub(JITTER_CENTRE))
}

/// `50 + (rand() & 63)`: a splash particle's speed (PART-2.5, PART-2.6).
fn splash_speed(rand: &mut dyn Random) -> f32 {
    int_to_float(SPLASH_SPEED.wrapping_add(rand.rand() & SPLASH_SPEED_MASK))
}

/// `(int) ramp` into a table, as C indexes it.
fn ramp(table: &[i32; 8], ramp: f32) -> i32 {
    usize::try_from(float_to_int(ramp))
        .ok()
        .and_then(|i| table.get(i))
        .copied()
        .unwrap_or(0)
}

fn ramp3(value: f32) -> i32 {
    ramp(&PART_RAMP3, value)
}

/// The origin and velocity of an explosion's particle (PART-2.1): each component of the
/// origin `org + (r % 32) - 16` and of the velocity `(r % 512) - 256`.
fn scatter(rand: &mut dyn Random, p: &mut Particle, org: [f32; 3]) {
    let mut origin = NOWHERE;
    let mut velocity = NOWHERE;
    let mut spread = |range: i32, centre: i32| {
        int_to_float(
            rand.rand()
                .checked_rem(range)
                .unwrap_or(0)
                .wrapping_sub(centre),
        )
    };
    for (o, (v, base)) in origin.iter_mut().zip(velocity.iter_mut().zip(org)) {
        *o = base + spread(SCATTER_ORIGIN, SCATTER_ORIGIN_CENTRE);
        *v = spread(SCATTER_SPEED, SCATTER_SPEED_CENTRE);
    }
    p.origin = origin;
    p.velocity = velocity;
}
