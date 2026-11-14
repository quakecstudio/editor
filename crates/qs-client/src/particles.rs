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
    const fn at(origin: [f32; 3], color: i32, die: f32, kind: ParticleType) -> Self {
        Self {
            origin,
            velocity: [0.0; 3],
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
        for i in 0..1024 {
            let mut p = Particle::at([0.0; 3], PART_RAMP1[0], time + 5.0, ParticleType::Explode);
            p.ramp = int_to_float(rand.rand() & 3);
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
        for i in 0..512_i32 {
            let color = color_start.wrapping_add(i.checked_rem(color_length).unwrap_or(0));
            let mut p = Particle::at([0.0; 3], color, time + 0.3, ParticleType::Blob);
            scatter(rand, &mut p, org);
            if !self.alloc(p) {
                return;
            }
        }
    }

    /// `R_BlobExplosion (org)` (PART-2.3).
    pub fn blob_explosion(&mut self, rand: &mut dyn Random, time: f32, org: [f32; 3]) {
        for i in 0..1024 {
            let die = time + 1.0 + int_to_float(rand.rand() & 8) * 0.05;
            let (kind, base): (ParticleType, i32) = if i & 1 == 1 {
                (ParticleType::Blob, 66)
            } else {
                (ParticleType::Blob2, 150)
            };
            let color = base.wrapping_add(rand.rand().checked_rem(6).unwrap_or(0));
            let mut p = Particle::at([0.0; 3], color, die, kind);
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
        if count == 1024 {
            self.explosion(rand, time, org);
            return;
        }
        for _ in 0..count.max(0) {
            let die = time + 0.1 * int_to_float(rand.rand().checked_rem(5).unwrap_or(0));
            let c = (color & !7).wrapping_add(rand.rand() & 7);
            let mut p = Particle::at([0.0; 3], c, die, ParticleType::SlowGrav);
            let [ox, oy, oz] = org;
            let jitter = |rand: &mut dyn Random| int_to_float((rand.rand() & 15).wrapping_sub(8));
            let jx = jitter(rand);
            let jy = jitter(rand);
            let jz = jitter(rand);
            p.origin = [ox + jx, oy + jy, oz + jz];
            p.velocity = dir.map(|d| d * 15.0);
            if !self.alloc(p) {
                return;
            }
        }
    }

    /// `R_LavaSplash (org)` (PART-2.5).
    pub fn lava_splash(&mut self, rand: &mut dyn Random, time: f32, org: [f32; 3]) {
        for i in -16_i32..16 {
            for j in -16_i32..16 {
                let die = time + 2.0 + int_to_float(rand.rand() & 31) * 0.02;
                let color = 224_i32.wrapping_add(rand.rand() & 7);
                let dx = int_to_float(j.wrapping_mul(8).wrapping_add(rand.rand() & 7));
                let dy = int_to_float(i.wrapping_mul(8).wrapping_add(rand.rand() & 7));
                let dir = [dx, dy, 256.0];
                let [ox, oy, oz] = org;
                let origin = [ox + dx, oy + dy, oz + int_to_float(rand.rand() & 63)];
                let (unit, _) = vector_normalize(dir);
                let vel = int_to_float(50_i32.wrapping_add(rand.rand() & 63));
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
        for i in (-16_i32..16).step_by(4) {
            for j in (-16_i32..16).step_by(4) {
                for k in (-24_i32..32).step_by(4) {
                    let die = time + 0.2 + int_to_float(rand.rand() & 7) * 0.02;
                    let color = 7_i32.wrapping_add(rand.rand() & 7);
                    let dir = [
                        int_to_float(j.wrapping_mul(8)),
                        int_to_float(i.wrapping_mul(8)),
                        int_to_float(k.wrapping_mul(8)),
                    ];
                    let [ox, oy, oz] = org;
                    let origin = [
                        ox + int_to_float(i.wrapping_add(rand.rand() & 3)),
                        oy + int_to_float(j.wrapping_add(rand.rand() & 3)),
                        oz + int_to_float(k.wrapping_add(rand.rand() & 3)),
                    ];
                    let (unit, _) = vector_normalize(dir);
                    let vel = int_to_float(50_i32.wrapping_add(rand.rand() & 63));
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
            let mut p = Particle::at(at, 0, time + 2.0, ParticleType::Static);
            let [ax, ay, az] = at;
            let near = |rand: &mut dyn Random| {
                int_to_float(rand.rand().checked_rem(6).unwrap_or(0).wrapping_sub(3))
            };
            match trail {
                Trail::Rocket | Trail::Smoke => {
                    let start = if trail == Trail::Smoke {
                        SMOKE_RAMP_START
                    } else {
                        0.0
                    };
                    p.ramp = int_to_float(rand.rand() & 3) + start;
                    p.color = ramp3(p.ramp);
                    p.kind = ParticleType::Fire;
                    let (jx, jy, jz) = (near(rand), near(rand), near(rand));
                    p.origin = [ax + jx, ay + jy, az + jz];
                }
                Trail::Blood | Trail::SlightBlood => {
                    p.kind = ParticleType::Grav;
                    p.color = BLOOD_COLOR.wrapping_add(rand.rand() & 3);
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
                    p.color = base.wrapping_add((self.tracercount & 4) << 1);
                    self.tracercount = self.tracercount.wrapping_add(1);
                    let [vx, vy, _] = vec;
                    p.velocity = if self.tracercount & 1 != 0 {
                        [TRACER_SPEED * vy, TRACER_SPEED * -vx, 0.0]
                    } else {
                        [TRACER_SPEED * -vy, TRACER_SPEED * vx, 0.0]
                    };
                }
                Trail::Voor => {
                    p.color = VOOR_COLOR.wrapping_add(rand.rand() & 3);
                    p.kind = ParticleType::Static;
                    p.die = time + VOOR_LIFE;
                    let wide =
                        |rand: &mut dyn Random| int_to_float((rand.rand() & 15).wrapping_sub(8));
                    let (jx, jy, jz) = (wide(rand), wide(rand), wide(rand));
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
                    let a = int_to_float(rand.rand() & 255) * 0.01;
                    let b = int_to_float(rand.rand() & 255) * 0.01;
                    let c = int_to_float(rand.rand() & 255) * 0.01;
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
                0x6f,
                time + 0.01,
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
        let time1 = frametime * 5.0;
        let time2 = frametime * 10.0;
        let time3 = frametime * 15.0;
        let grav = frametime * gravity * PART_GRAVITY_SCALE;
        let dvel = 4.0 * frametime;
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
                    if p.ramp >= 6.0 {
                        p.die = -1.0;
                    } else {
                        p.color = ramp3(p.ramp);
                    }
                    p.velocity[2] += grav;
                }
                ParticleType::Explode => {
                    p.ramp += time2;
                    if p.ramp >= 8.0 {
                        p.die = -1.0;
                    } else {
                        p.color = ramp(&PART_RAMP1, p.ramp);
                    }
                    p.velocity = p.velocity.map(|v| v + v * dvel);
                    p.velocity[2] -= grav;
                }
                ParticleType::Explode2 => {
                    p.ramp += time3;
                    if p.ramp >= 8.0 {
                        p.die = -1.0;
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
    let mut origin = [0.0; 3];
    let mut velocity = [0.0; 3];
    for (o, (v, base)) in origin.iter_mut().zip(velocity.iter_mut().zip(org)) {
        *o = base + int_to_float(rand.rand().checked_rem(32).unwrap_or(0).wrapping_sub(16));
        *v = int_to_float(rand.rand().checked_rem(512).unwrap_or(0).wrapping_sub(256));
    }
    p.origin = origin;
    p.velocity = velocity;
}
