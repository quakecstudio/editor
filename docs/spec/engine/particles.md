# Particles

Prefix: `PART` (registered in [`../README.md`](../README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the particle system: the pool, each effect's particles (explosions, impacts, lava and
teleport splashes, trails, the entity field), their movement and colour ramps each frame, and
how one is drawn, as implemented by `WinQuake/r_part.c` in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`. The GLQuake drawing is the one described. To be
implemented by `qs-render` for the preview host (P3-26, #240).

A particle has an origin, a velocity, a colour (a palette index), a `ramp` (a float index
into a colour ramp), a type and a `die` time on the client clock. `rand ()` is the C library's;
the preview host draws from its own seeded source (DIFF-1.4), and the engine differential
compares particle counts and types, not positions drawn from the random source.

## PART-1 The pool

**Source:** `WinQuake/r_part.c`, `R_InitParticles`, `R_ClearParticles`.

### Behaviour

- **PART-1.1** The pool holds `PART_MAX_PARTICLES` particles, or the `-particles` argument's
  count raised to `PART_ABSOLUTE_MIN_PARTICLES`. `R_ClearParticles`, at each map load, puts
  every particle on the free list in index order and empties the active list.
- **PART-1.2** An effect takes particles from the head of the free list and pushes each onto
  the head of the active list; when the free list is empty the effect stops early with the
  particles it has.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `PART_MAX_PARTICLES` | 2048 | `r_part.c`, `MAX_PARTICLES` |
| `PART_ABSOLUTE_MIN_PARTICLES` | 512 | `r_part.c`, `ABSOLUTE_MIN_PARTICLES` |
| `PART_RAMP1` | `[0x6f, 0x6d, 0x6b, 0x69, 0x67, 0x65, 0x63, 0x61]` | `r_part.c`, `ramp1` |
| `PART_RAMP2` | `[0x6f, 0x6e, 0x6d, 0x6c, 0x6b, 0x6a, 0x68, 0x66]` | `r_part.c`, `ramp2` |
| `PART_RAMP3` | `[0x6d, 0x6b, 6, 5, 4, 3, 0, 0]` | `r_part.c`, `ramp3` (six values given; the last two of the eight are 0) |

## PART-2 Effects

**Source:** `WinQuake/r_part.c`, `R_ParticleExplosion`, `R_ParticleExplosion2`,
`R_BlobExplosion`, `R_RunParticleEffect`, `R_ParseParticleEffect`, `R_LavaSplash`,
`R_TeleportSplash`, `R_RocketTrail`, `R_EntityParticles`.

In each statement `r` is a fresh `rand ()` and `t` is `cl.time`.

### Behaviour

- **PART-2.1** `R_ParticleExplosion (org)`: 1024 particles, `die = t + 5`, colour
  `PART_RAMP1[0]`, `ramp = r & 3`, each component of the origin `org + (r % 32) - 16` and of
  the velocity `(r % 512) - 256`; odd-numbered particles are `pt_explode`, even ones
  `pt_explode2`.
- **PART-2.2** `R_ParticleExplosion2 (org, colorStart, colorLength)`: 512 particles,
  `die = t + 0.3`, colour `colorStart + (i % colorLength)` for particle `i`, type `pt_blob`,
  origin and velocity as PART-2.1.
- **PART-2.3** `R_BlobExplosion (org)`: 1024 particles, `die = t + 1 + (r & 8) * 0.05`;
  odd ones `pt_blob` with colour `66 + r % 6`, even ones `pt_blob2` with colour `150 + r % 6`;
  origin and velocity as PART-2.1.
- **PART-2.4** `R_RunParticleEffect (org, dir, color, count)` with `count` 1024 is PART-2.1
  ("rocket explosion"). Otherwise `count` particles: `die = t + 0.1 * (r % 5)`, colour
  `(color & ~7) + (r & 7)`, type `pt_slowgrav`, each origin component `org + (r & 15) - 8`
  and velocity `dir * 15`. `svc_particle` (CLIENT-1.4) reads the origin, a direction of three
  signed bytes over 16, a count byte and a colour byte, and a count of 255 means 1024.
- **PART-2.5** `R_LavaSplash (org)`: for `i` and `j` from -16 to 15, one particle with
  `die = t + 2 + (r & 31) * 0.02`, colour `224 + (r & 7)`, type `pt_slowgrav`,
  `dir = (j * 8 + (r & 7), i * 8 + (r & 7), 256)`, origin `org + (dir.x, dir.y, r & 63)`, and
  velocity `dir` normalised times `50 + (r & 63)`.
- **PART-2.6** `R_TeleportSplash (org)`: for `i` and `j` from -16 to 12 by 4 and `k` from -24
  to 28 by 4, one particle with `die = t + 0.2 + (r & 7) * 0.02`, colour `7 + (r & 7)`, type
  `pt_slowgrav`, `dir = (j * 8, i * 8, k * 8)`, origin `org + (i + (r & 3), j + (r & 3), k +
  (r & 3))`, and velocity `dir` normalised times `50 + (r & 63)`.
- **PART-2.7** `R_RocketTrail (start, end, type)`: `vec` is `end - start` normalised and `len`
  its length; a `type` of 128 or more is `type - 128` with a step `dec` of 1, else the step
  is 3. While `len > 0`: `len -= dec`, one particle with zero velocity and `die = t + 2`, set
  by `type`, then `start += vec`. The types: 0 (rocket) `ramp = r & 3`, colour
  `PART_RAMP3[ramp]`, `pt_fire`, origin `start + (r % 6) - 3` per component; 1 (smoke) the
  same with `ramp = (r & 3) + 2`; 2 (blood) `pt_grav`, colour `67 + (r & 3)`, the same
  origin; 3 and 5 (tracers) `die = t + 0.5`, `pt_static`, colour `52 + ((tracercount & 4)
  << 1)` for 3 and `230 + ((tracercount & 4) << 1)` for 5, origin `start`, and velocity
  `(30 * vec.y, -30 * vec.x)` when `tracercount` is odd after it is incremented, else
  `(-30 * vec.y, 30 * vec.x)`, with `tracercount` a counter kept across calls; 4 (slight
  blood) as 2 and `len -= 3` more; 6 (voor trail) colour `152 + (r & 3)`, `pt_static`,
  `die = t + 0.3`, origin `start + (r & 15) - 8` per component.
- **PART-2.8** `R_EntityParticles (ent)` (`EF_BRIGHTFIELD`): 162 particles, one per vertex
  normal of the MDL table (MDL-5), `die = t + 0.01`, colour 0x6f, `pt_explode`, at `origin +
  normal * 64 + forward * 16` where `forward` is the direction of the angles `t *
  avelocities[i]`, and `avelocities` are 162 triples drawn once as `(r & 255) * 0.01`. id
  sets no velocity, so each particle keeps whatever its pool slot last held and PART-3.1
  moves it by that for its one frame of life; the preview host gives it velocity zero, a
  deliberate divergence from reading a dead particle's state.

## PART-3 Movement and drawing

**Source:** `WinQuake/r_part.c`, `R_DrawParticles`.

### Behaviour

- **PART-3.1** Each frame, with `frametime = cl.time - cl.oldtime`: particles whose `die` is
  before `cl.time` are freed, then every active one is drawn and then moved: `origin +=
  velocity * frametime`, then by type with `grav = frametime * sv_gravity * 0.05`, `dvel = 4
  * frametime`, `time1 = frametime * 5`, `time2 = frametime * 10` and `time3 = frametime * 15`:
  - `pt_static`: nothing;
  - `pt_fire`: `ramp += time1`; at 6 or more the particle dies next frame (`die = -1`), else
    its colour is `PART_RAMP3[(int) ramp]`; `velocity.z += grav`;
  - `pt_explode`: `ramp += time2`; at 8 or more it dies, else colour `PART_RAMP1[(int) ramp]`;
    `velocity += velocity * dvel`; `velocity.z -= grav`;
  - `pt_explode2`: `ramp += time3`; at 8 or more it dies, else colour `PART_RAMP2[(int)
    ramp]`; `velocity -= velocity * frametime`; `velocity.z -= grav`;
  - `pt_blob`: `velocity += velocity * dvel`; `velocity.z -= grav`;
  - `pt_blob2`: `velocity.x` and `.y` lose `velocity * dvel`; `velocity.z -= grav`;
  - `pt_grav` and `pt_slowgrav`: `velocity.z -= grav`.
- **PART-3.2** GLQuake draws a particle as one triangle with the particle texture: the
  corners `origin`, `origin + up * 1.5 * scale` and `origin + right * 1.5 * scale`, in the
  particle's palette colour, where `scale` is 1 when the particle is less than 20 units in
  front of the eye (`(origin - eye) . forward`) and `1 + 0.004 * that distance` otherwise,
  blended over the scene. The preview host draws the same triangle; the fidelity check
  (P3-28) compares the frame as an image.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `PART_GRAVITY_SCALE` | 0.05 | `r_part.c`, `R_DrawParticles` |
| `PART_DRAG` | 4 | `r_part.c`, `R_DrawParticles` (`dvel = 4 * frametime`) |
| `PART_FIRE_RAMP_RATE` | 5 | `r_part.c`, `R_DrawParticles` (`time1`) |
| `PART_EXPLODE_RAMP_RATE` | 10 | `r_part.c`, `R_DrawParticles` (`time2`) |
| `PART_EXPLODE2_RAMP_RATE` | 15 | `r_part.c`, `R_DrawParticles` (`time3`) |
| `PART_SCALE_NEAR` | 20 | `r_part.c`, `R_DrawParticles` |
| `PART_SCALE_RATE` | 0.004 | `r_part.c`, `R_DrawParticles` |
| `PART_SIZE` | 1.5 | `r_part.c`, `R_DrawParticles` (`VectorScale (vup, 1.5, up)`) |
