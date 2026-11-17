// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/light.cc, light_main, LightWorld

//! The extended dialect's lighting (LIGHT-8 on): a map lit as ericw-tools' `light` lights it.
//!
//! [`run`] reads the entities and the world's settings (`keys`, `lights`), gathers the map's
//! models and the faces that cast shadow (`scene`, `rays`), finds each face's smoothed
//! normals and sample points (`phong`, `points`), lights them (`shade`, `sun`) and writes
//! the lightmaps (`output`).

mod bounce;
mod brush_faces;
mod dirt;
mod keys;
mod lights;
mod math;
mod mottle;
mod output;
mod phong;
mod points;
mod project;
mod rays;
mod scene;
mod shade;
mod sidecar;
mod sun;
mod surflight;
#[cfg(test)]
mod tests;
mod vis;

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use qs_bsp::{Bsp, Bspx};
use qs_map_bsp::{Dialect, LeafKind, SurfaceFlags};

use self::bounce::{Bouncer, Stage};
use self::dirt::Dirt;
use self::lights::Lights;
use self::phong::Smoothing;
use self::points::Patch;
use self::rays::{Rays, Stack};
use self::scene::{Given, Map, MapFault};
use self::shade::{Lightmaps, Shader};
pub use self::sidecar::{CONTENT_EXTENSION, SidecarFile, Sidecars, TEXINFO_EXTENSION};
use crate::face::{FaceId, Lit};
use crate::lump::{self, Colour};
use crate::{Finish, Light, LightError, LightWarning, Lighted};

/// The BSPX lumps of a lighting, which `light` takes out before it writes its own
/// (`bspdata->bspx.entries.erase`).
const STALE_LUMPS: [&[u8]; 8] = [
    b"LMSTYLE16",
    b"LMSTYLE",
    b"LMOFFSET",
    b"DECOUPLED_LM",
    b"RGBLIGHTING",
    b"LIGHTINGDIR",
    b"LIGHTING_E5BGR9",
    b"FACENORMALS",
];

/// How many faces a thread lights before taking more.
const FACE_BATCH: usize = 16;

/// What the BSP compiler and the game give the extended dialect's lighting besides the BSP.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Extras<'a> {
    /// The flags of each texinfo, in order (LIGHT-9.8); a texinfo past the end has none.
    pub surfaces: &'a [SurfaceFlags],
    /// What each leaf is to a sample, in order (LIGHT-9.9); a leaf past the end is as the
    /// BSP says.
    pub leaves: &'a [LeafKind],
    /// The game's palette, 256 colours of three bytes, for the colour of glass
    /// (LIGHT-11.6); without it glass does not colour light.
    pub palette: Option<&'a [u8]>,
}

/// One face lit: its lightmaps, and whether any of its samples has a colour.
type FaceResult = (Option<Lit>, bool, Vec<LightWarning>);

impl Extras<'_> {
    /// `LoadExtendedTexinfoFlags`, `LoadExtendedContentFlags`: these with what is not
    /// `read`'s taken out, and a warning for each: flags for more texinfos than the BSP has,
    /// or leaves in another number than its own, are another BSP's (LIGHT-9.11).
    fn matched_to(self, read: &Bsp<'_>) -> (Self, Vec<LightWarning>) {
        let mut extras = self;
        let mut mismatched = Vec::new();
        if extras.surfaces.len() > read.texinfo.len() {
            mismatched.push(LightWarning::SidecarMismatch(SidecarFile::Texinfo));
            extras.surfaces = &[];
        }
        if !extras.leaves.is_empty() && extras.leaves.len() != read.leafs.len() {
            mismatched.push(LightWarning::SidecarMismatch(SidecarFile::Content));
            extras.leaves = &[];
        }
        (extras, mismatched)
    }
}

impl Light {
    /// `light_main` and `LightWorld`: the BSP `bytes`, read as `read`, lit in the extended
    /// dialect (LIGHT-8).
    pub(crate) fn run_extended(
        &self,
        bytes: &[u8],
        read: &Bsp<'_>,
        extras: Extras<'_>,
    ) -> Result<Lighted, LightError> {
        let light = self;
        let mut lights = Lights::read(read.entity_lump)?;
        let (extras, mismatched) = extras.matched_to(read);
        let map = Map::new(
            read,
            lights.world.clone(),
            &lights.dicts,
            extras.surfaces.to_vec(),
            &Given {
                switches: lights.shadow_switches(),
                leaves: extras.leaves,
                palette: extras.palette,
            },
        )
        .map_err(|MapFault::NoEntityForModel(model)| LightError::NoEntityForModel { model })?;
        let rays = Rays::new(map.casters());
        let unprojected = lights.project(&map)?;
        lights.set_up(&map, &rays, light.jitter);
        // The radiosity surface lights, before any face is lit (LIGHT-16.4).
        let (mut emitters, surface_lit) = lights.radiosity(&map, &rays)?;
        let smoothing = Smoothing::new(&map);
        // The quick light does neither dirt nor bounce (LIGHT-8.8).
        let quick = light.finish == Finish::Quick;
        let dirt = Dirt::new(&map.world, &lights.lamps, light.jitter).filter(|_| !quick);
        let shader = Shader {
            map: &map,
            rays: &rays,
            lights: &lights,
            dirt: dirt.as_ref(),
            surface_lit: &surface_lit,
        };
        let count = read.faces.len();
        // Each face's samples and lightmaps, between the passes: every face is lit directly
        // before any light bounces, and every bounce is over before the minimum light.
        let direct_emitters = &emitters;
        let slots: Vec<Mutex<Option<(Patch, Lightmaps)>>> = on_threads(count, |face, stack| {
            Mutex::new(
                Patch::new(&map, &smoothing, FaceId::new(face), light.sampling).map(|mut patch| {
                    let lightmaps = shader.direct(&mut patch, direct_emitters, stack);
                    (patch, lightmaps)
                }),
            )
        })?;
        let bounces = if quick {
            0
        } else {
            usize::try_from(map.world.bounce.get()).unwrap_or(0)
        };
        if bounces > 0 {
            // `Face_ShouldBounce`: a face that is a surface light gives none back.
            let mut bouncing = map.bouncing_faces();
            for (bounces, lit) in bouncing.iter_mut().zip(&surface_lit) {
                *bounces &= lit.is_none();
            }
            let corners = map.moved_corners();
            let texture = map.bounce_colours();
            let bouncer = Bouncer {
                world: &map.world,
                bounces: &bouncing,
                corners: &corners,
                texture: &texture,
            };
            for depth in 0..bounces {
                let mut any = false;
                let mut stack = Vec::new();
                for (face, (slot, emitter)) in slots.iter().zip(&mut emitters).enumerate() {
                    let Ok(mut slot) = slot.lock() else {
                        continue;
                    };
                    let Some((patch, lightmaps)) = slot.as_mut() else {
                        continue;
                    };
                    let had = emitter.is_some();
                    any |= bouncer.emit(face, lightmaps, emitter, depth);
                    // A new emitter is culled as a light is: by the leaves its face is in
                    // when the BSP has visibility (LIGHT-10.10), else by what it can see
                    // (LIGHT-10.8).
                    if !had && let Some(emitter) = emitter {
                        if map.visibility.is_some() {
                            emitter.leaves.clone_from(&patch.leaves);
                        } else {
                            emitter.bounds =
                                Some(rays.visible_bounds(&map, emitter.pos, &mut stack));
                        }
                    }
                }
                if !any {
                    break;
                }
                let emitters = &emitters;
                on_threads(count, |face, stack| {
                    if let Some(mut slot) = slots.get(face).and_then(|slot| slot.lock().ok())
                        && let Some((patch, lightmaps)) = slot.as_mut()
                    {
                        shader.emitted(patch, lightmaps, emitters, Stage::Bounce(depth), stack);
                    }
                })?;
            }
        }
        let emitters = &emitters;
        let results: Vec<FaceResult> = on_threads(count, |face, stack| {
            let taken = slots
                .get(face)
                .and_then(|slot| slot.lock().ok())
                .and_then(|mut slot| slot.take());
            taken.map_or((None, false, Vec::new()), |(mut patch, mut lightmaps)| {
                shader.post(&mut patch, &mut lightmaps, emitters, stack);
                let done = lightmaps.finish(&map.world, &patch, light.sampling);
                (done.lit, done.coloured, done.warnings)
            })
        })?;
        let mut faces = Vec::with_capacity(count);
        let mut colour = Colour::White;
        // ericw-tools warns of each kind once, however many faces it is true of.
        let mut written: Vec<LightWarning> = Vec::new();
        for (lit, coloured, warned) in results {
            if coloured {
                colour = Colour::Coloured;
            }
            written.extend(warned);
            faces.push(lit);
        }
        // The grey byte is always the brightest channel (LIGHT-18.5), which is what a coloured
        // lump's is; the `.lit` is written only when a sample has a colour (LIGHT-18.6).
        let mut out = lump::written(
            bytes,
            read,
            &faces,
            &lights.written(),
            Colour::Coloured,
            Dialect::Extended,
        )?;
        // LIGHT-18.7: the BSP's BSPX lumps are kept, but those that held a lighting this one
        // takes the place of.
        let mut bspx = Bspx::read(bytes);
        for stale in STALE_LUMPS {
            bspx.remove(stale);
        }
        bspx.append_to(&mut out.bsp)
            .map_err(|_| LightError::Overrun)?;
        Ok(Lighted {
            bsp: out.bsp,
            lit: out.lit.filter(|_| colour == Colour::Coloured),
            warnings: lights.warned(&unprojected, mismatched, &written),
            switchable: lights.switchable(),
        })
    }
}

/// `work` done for each of `count` things on every thread the machine has, with a stack for
/// its rays; the results in order.
fn on_threads<T: Send>(
    count: usize,
    work: impl Fn(usize, &mut Stack) -> T + Sync,
) -> Result<Vec<T>, LightError> {
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let batches = count.div_ceil(FACE_BATCH);
    let next = AtomicUsize::new(0);
    let mut results: Vec<Option<T>> = std::iter::repeat_with(|| None).take(count).collect();
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads.clamp(1, batches.max(1)))
            .map(|_| {
                s.spawn(|| {
                    let mut mine = Vec::new();
                    let mut stack = Vec::new();
                    loop {
                        let start = next
                            .fetch_add(1, Ordering::Relaxed)
                            .saturating_mul(FACE_BATCH);
                        if start >= count {
                            break;
                        }
                        for index in start..start.saturating_add(FACE_BATCH).min(count) {
                            mine.push((index, work(index, &mut stack)));
                        }
                    }
                    mine
                })
            })
            .collect();
        for handle in handles {
            if let Ok(mine) = handle.join() {
                for (index, result) in mine {
                    if let Some(slot) = results.get_mut(index) {
                        *slot = Some(result);
                    }
                }
            }
        }
    });
    results
        .into_iter()
        .map(|result| result.ok_or(LightError::Thread))
        .collect()
}
