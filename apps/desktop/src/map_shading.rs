// SPDX-License-Identifier: GPL-3.0-or-later

//! How the map editor's 3D view draws the map (P7-13, MAP-17): as its brushes' edges, as their
//! faces with their textures, shaded by the way each faces as TrenchBroom draws them, or lit by
//! the last compile's lightmaps and coloured light. Textured and Lit are drawn by `qs-render`,
//! the renderer the game and the BSP view use, under the editor's own lines.
//!
//! Every string comes from [`crate::strings`]; a test fails on a string literal here.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui::{Rect, Ui};
use eframe::egui_wgpu::{self, RenderState};
use qs_bsp::{Bsp, BspLoader, Lit};
use qs_layout::MapShading;
use qs_map::MapFile;
use qs_map_edit::textures::TextureSources;
use qs_map_edit::{EntityIndex, MapDocument, MapRevision};
use qs_render::{
    Camera, EditorCallback, Extras, FlatFace, Image, Look, MapMesh, MeshBuilder, Placement, Shade,
    View,
};
use qs_wad::texture::{Palette, PaletteIndex};

use crate::map_scene::{FaceShape, Scene};
use crate::map_view;
use crate::strings::Text;

/// The toolbar's label of `shading`.
const fn text(shading: MapShading) -> Text {
    match shading {
        MapShading::Lines => Text::MapShadeLines,
        MapShading::Textured => Text::MapShadeTextured,
        MapShading::Lit => Text::MapShadeLit,
    }
}

/// What `shading` draws, the toolbar's tooltip.
const fn hint(shading: MapShading) -> Text {
    match shading {
        MapShading::Lines => Text::MapShadeLinesHint,
        MapShading::Textured => Text::MapShadeTexturedHint,
        MapShading::Lit => Text::MapShadeLitHint,
    }
}

/// What a textured mesh is made from, each part a count or hash that changes when it does: the
/// scene's builds, the entities hidden, and the texture sources' loads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TexturedFrom {
    /// How many times the scene was built.
    pub(crate) scene: u64,
    /// A hash of the entities hidden.
    pub(crate) hidden: u64,
    /// How many times the texture sources were loaded.
    pub(crate) sources: u64,
}

/// Whether a compiled BSP was drawn or would not load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Loaded {
    /// It is drawn.
    Drawn,
    /// It would not load; it is not read again until it is written again.
    Failed,
}

/// What the renderer was last given, so it is made again only when that changes.
#[derive(Clone, Debug, PartialEq)]
enum Made {
    /// The textured faces of the scene.
    Textured(TexturedFrom),
    /// The compiled BSP at its time on disk.
    Lit(PathBuf, SystemTime, Loaded),
}

/// The state of the Lit view's compile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LitState {
    /// The map has no BSP beside it yet.
    NoCompile,
    /// The BSP is the map as it is now.
    Current,
    /// The map was edited, or saved, after the BSP was written.
    Stale,
}

/// The 3D view's shading, as the layout holds it, and the mesh it draws.
#[derive(Debug, Default)]
pub(crate) struct ShadedView {
    shading: MapShading,
    made: Option<Made>,
    lit: Option<LitState>,
    /// The map's edits the BSP drawn was compiled from, which a selection leaves alone: those
    /// when the compile was first seen, none when the saved map was already newer than it.
    compiled_at: Option<MapRevision>,
    /// Each texture decoded, by name, and the sources' load they were decoded from.
    images: (HashMap<String, Image>, u64),
}

impl ShadedView {
    /// The toolbar's choice of shading, `current` the layout's this frame: the one clicked,
    /// which `view.map_shading` sets.
    pub(crate) fn toolbar(ui: &mut Ui, current: MapShading) -> Option<MapShading> {
        let mut chosen = None;
        for shading in MapShading::ALL {
            if ui
                .selectable_label(current == shading, text(shading).get())
                .on_hover_text(hint(shading).get())
                .clicked()
            {
                chosen = Some(shading);
            }
        }
        chosen
    }

    /// What the view says of the Lit view's compile, when it shows Lit and has something to say.
    pub(crate) const fn note(&self) -> Option<Text> {
        match (self.shading, self.lit) {
            (MapShading::Lit, Some(LitState::NoCompile)) => Some(Text::MapLitNone),
            (MapShading::Lit, Some(LitState::Stale)) => Some(Text::MapLitStale),
            _ => None,
        }
    }

    /// Whether the renderer draws the faces this frame, so the view leaves out the edges of
    /// the brushes it draws.
    pub(crate) const fn draws_faces(&self) -> bool {
        match self.shading {
            MapShading::Lines => false,
            MapShading::Textured => matches!(self.made, Some(Made::Textured(..))),
            MapShading::Lit => matches!(self.made, Some(Made::Lit(_, _, Loaded::Drawn))),
        }
    }

    /// Makes the renderer's mesh again when the shading, the scene or the compile changed:
    /// the textured faces of `scene` not `hidden`, made from `from`, with `sources`' textures,
    /// or the BSP beside the map.
    pub(crate) fn refresh(
        &mut self,
        (shading, state): (MapShading, Option<&RenderState>),
        document: &MapDocument,
        (scene, from, hidden): (&Scene, TexturedFrom, &BTreeSet<EntityIndex>),
        sources: Option<&TextureSources>,
    ) {
        self.shading = shading;
        let Some(state) = state else {
            return;
        };
        match self.shading {
            MapShading::Lines => {}
            MapShading::Textured => {
                if self.made != Some(Made::Textured(from)) {
                    let (images, loaded) = &mut self.images;
                    if *loaded != from.sources {
                        images.clear();
                        *loaded = from.sources;
                    }
                    let mut image = |name: &str| {
                        images
                            .entry(name.to_ascii_lowercase())
                            .or_insert_with(|| decoded(sources, name))
                            .clone()
                    };
                    let mesh = scene.textured_mesh(&mut image, hidden, Look::QuakeSpasm);
                    map_view::install_editor(state, &mesh);
                    self.made = Some(Made::Textured(from));
                }
            }
            MapShading::Lit => self.refresh_lit(state, document, sources),
        }
    }

    /// Follows the BSP beside the map: drawn again when it is written again, none drawn when it
    /// is gone, and the map's revision it was compiled from noted when it is first seen.
    fn refresh_lit(
        &mut self,
        state: &RenderState,
        document: &MapDocument,
        sources: Option<&TextureSources>,
    ) {
        let path = document.path();
        let bsp = path.with_extension("bsp");
        let Some(written) = modified(&bsp) else {
            self.lit = Some(LitState::NoCompile);
            if matches!(self.made, Some(Made::Lit(..))) {
                self.made = None;
            }
            return;
        };
        let seen =
            matches!(&self.made, Some(Made::Lit(drawn, at, _)) if *drawn == bsp && *at == written);
        if !seen {
            let saved_after = modified(path).is_some_and(|map| map > written);
            self.compiled_at = (!saved_after).then(|| document.edits());
            let loaded = match sources
                .and_then(TextureSources::palette)
                .ok_or_else(|| "no palette".to_owned())
                .and_then(|palette| lit(&bsp, palette))
            {
                Ok(mesh) => {
                    map_view::install_editor(state, &mesh);
                    Loaded::Drawn
                }
                Err(error) => {
                    tracing::warn!(%error, path = %bsp.display(), "the lit view's BSP");
                    Loaded::Failed
                }
            };
            self.made = Some(Made::Lit(bsp, written, loaded));
        }
        // A BSP that would not load asks for a compile, as a missing one does.
        self.lit = Some(
            if matches!(self.made, Some(Made::Lit(_, _, Loaded::Failed))) {
                LitState::NoCompile
            } else if self.compiled_at == Some(document.edits()) {
                LitState::Current
            } else {
                LitState::Stale
            },
        );
    }

    /// Paints the faces in the 3D pane `area`, seen by `camera`, under what the view draws
    /// after; nothing in Lines, or before a mesh is made.
    pub(crate) fn paint(&self, ui: &Ui, area: Rect, camera: &Camera) {
        if !self.draws_faces() {
            return;
        }
        let aspect = area.width() / area.height().max(1.0);
        let view = View::new(camera.clip_from_world(aspect), camera.origin, 0.0);
        ui.painter_at(area)
            .add(egui_wgpu::Callback::new_paint_callback(
                area,
                EditorCallback { view },
            ));
    }
}

/// The time `path` was last written, none when it cannot be read.
fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl FaceShape {
    /// The face as a flat face drawn with texture `slot`, whose image is `image`; none for a
    /// face whose texture has no axes.
    fn flat(&self, slot: usize, image: &Image) -> Option<FlatFace> {
        let size = [image.width, image.height].map(|n| f64::from(n.max(1)));
        let mut corners = Vec::with_capacity(self.polygon.len());
        for &point in &self.polygon {
            let [s, t] = self.texels(point)?;
            corners.push((
                point.map(to_f32),
                [to_f32(s / size[0]), to_f32(t / size[1])],
            ));
        }
        // The renderer draws a face wound clockwise seen from its front, as Quake winds them
        // (REND-1.10): seen from where the face's normal points, its corners turn against it.
        if dot(winding_normal(&corners), self.plane.0) > 0.0 {
            corners.reverse();
        }
        Some(FlatFace {
            corners,
            texture: slot,
            shade: shade_of(self.plane.0),
        })
    }
}

impl Scene {
    /// The textured mesh of the scene's brush faces but those of the entities `hidden`: each
    /// texture's image from `image`, and each face shaded by its normal.
    pub(crate) fn textured_mesh(
        &self,
        image: &mut dyn FnMut(&str) -> Image,
        hidden: &BTreeSet<EntityIndex>,
        look: Look,
    ) -> MapMesh {
        let mut names: Vec<&str> = Vec::new();
        let mut images: Vec<Image> = Vec::new();
        let mut faces = Vec::new();
        for brush in self.brushes.iter().filter(|b| !hidden.contains(&b.entity)) {
            for face in brush.faces.iter().filter(|f| !is_unseen(&f.texture)) {
                let slot = match names
                    .iter()
                    .position(|n| n.eq_ignore_ascii_case(&face.texture))
                {
                    Some(slot) => slot,
                    None => {
                        names.push(&face.texture);
                        images.push(image(&face.texture));
                        names.len().saturating_sub(1)
                    }
                };
                let Some(drawn) = images.get(slot) else {
                    continue;
                };
                if let Some(flat) = face.flat(slot, drawn) {
                    faces.push(flat);
                }
            }
        }
        MapMesh::flat(&faces, images, look)
    }
}

/// The normal the corners' turn makes by the right hand: the sum of the cross products of
/// each pair of corners about the first, sound for any convex polygon.
fn winding_normal(corners: &[([f32; 3], [f32; 2])]) -> [f32; 3] {
    let Some(&([fx, fy, fz], _)) = corners.first() else {
        return [0.0, 0.0, 0.0];
    };
    let mut normal = [0.0_f32, 0.0, 0.0];
    for pair in corners.windows(2).skip(1) {
        if let [([ax, ay, az], _), ([bx, by, bz], _)] = pair {
            let [ux, uy, uz] = [ax - fx, ay - fy, az - fz];
            let [vx, vy, vz] = [bx - fx, by - fy, bz - fz];
            let [nx, ny, nz] = normal;
            normal = [
                nx + (uy * vz - uz * vy),
                ny + (uz * vx - ux * vz),
                nz + (ux * vy - uy * vx),
            ];
        }
    }
    normal
}

/// The dot product of `a` and `b`.
fn dot([ax, ay, az]: [f32; 3], [bx, by, bz]: [f32; 3]) -> f32 {
    ax * bx + ay * by + az * bz
}

/// How brightly a face pointing along `normal` is drawn: floors and ceilings brightest, walls
/// facing x a little darker and walls facing y darker again, as TrenchBroom shades its 3D view,
/// so neighbouring faces read apart without a light.
pub(crate) fn shade_of([x, y, z]: [f32; 3]) -> Shade {
    let level = SHADE_FLOOR + (1.0 - SHADE_FLOOR) * (z * z + x * x * SHADE_X + y * y * SHADE_Y);
    Shade::new(level_of(level * f32::from(Shade::FULL.get())))
}

/// The darkest a face is shaded, as a share of full light.
const SHADE_FLOOR: f32 = 0.55;
/// How much light a wall facing x keeps, past the floor.
const SHADE_X: f32 = 0.8;
/// How much light a wall facing y keeps, past the floor.
const SHADE_Y: f32 = 0.6;

/// The texture `name` from `sources` as an image; when it is not found, Quake's black and pink
/// checker (`notexture_mip`, REND-13) at a missing texture's size, two blocks of 32 texels a
/// side, so it repeats every 64 units as QuakeSpasm stretches it.
pub(crate) fn decoded(sources: Option<&TextureSources>, name: &str) -> Image {
    match sources.and_then(|sources| sources.decode(name)) {
        Some(texture) => Image {
            width: texture.width,
            height: texture.height,
            rgba: texture.rgba,
        },
        None => missing(),
    }
}

/// Quake's missing-texture checker, [`MISSING_SIDE`] texels a side in blocks of half that.
pub(crate) fn missing() -> Image {
    let half = MISSING_BLOCK;
    let mut rgba = Vec::new();
    for row in 0..MISSING_SIDE {
        for column in 0..MISSING_SIDE {
            let pink = (row < half) == (column < half);
            rgba.extend(if pink { PINK } else { BLACK });
        }
    }
    Image {
        width: MISSING_SIDE,
        height: MISSING_SIDE,
        rgba,
    }
}

/// A missing texture's side in texels.
const MISSING_SIDE: u32 = 64;
/// A missing texture's checker block's side in texels: half its side.
const MISSING_BLOCK: u32 = 32;
/// `notexture_data`'s pink.
const PINK: [u8; 4] = [159, 91, 83, 255];
/// `notexture_data`'s black.
const BLACK: [u8; 4] = [0, 0, 0, 255];

/// The mesh of the BSP at `path`, every model at its compiled place, lit by its lightmaps and
/// the `.lit` beside it, through `palette`.
fn lit(path: &Path, palette: &Palette) -> Result<MapMesh, String> {
    // `gfx/palette.lmp`'s 768 bytes, as the renderer reads a palette.
    let bytes: Vec<u8> = (0..=u8::MAX)
        .flat_map(|index| palette.colour(PaletteIndex::new(index)))
        .collect();
    let palette = qs_render::Palette::read(&bytes).map_err(|e| e.to_string())?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let bsp = Bsp::read(&bytes, BspLoader::FitzQuake).map_err(|e| e.to_string())?;
    let lit_bytes = std::fs::read(path.with_extension("lit")).ok();
    let lit = lit_bytes
        .as_deref()
        .and_then(|bytes| Lit::read(bytes, bsp.lighting.len()).ok());
    let placements = placements(&bsp);
    let mut builder = MeshBuilder::new(&palette, Look::QuakeSpasm);
    let extras = Extras {
        lit: lit.as_ref(),
        ..Extras::default()
    };
    builder
        .add_with(&bsp, &extras, &placements)
        .map_err(|e| e.to_string())?;
    Ok(builder.finish())
}

/// Whether a face of texture `name` is one the game never draws, so the textured view leaves
/// it out as the lit one does: the skip, hint, clip, origin and trigger textures, in any case
/// (QBSP-15.16).
fn is_unseen(name: &str) -> bool {
    UNSEEN
        .iter()
        .any(|unseen| name.eq_ignore_ascii_case(unseen))
}

/// The textures of [`is_unseen`].
const UNSEEN: [&str; 11] = [
    "skip",
    "*waterskip",
    "*slimeskip",
    "*lavaskip",
    "bevel",
    "null",
    "hint",
    "hintskip",
    "clip",
    "origin",
    "trigger",
];

/// The world and each brush entity's model at its origin, as the engine places them; triggers
/// are left out, as the engine does not draw them.
fn placements(bsp: &Bsp<'_>) -> Vec<Placement> {
    let mut placements = vec![Placement::WORLD];
    let Ok(file) = MapFile::parse(bsp.entities()) else {
        return placements;
    };
    for entity in &file.entities {
        let Some(model) = entity
            .value(b"model")
            .and_then(|m| m.strip_prefix(b"*"))
            .and_then(|m| std::str::from_utf8(m).ok())
            .and_then(|m| m.parse::<usize>().ok())
        else {
            continue;
        };
        if entity
            .classname()
            .is_some_and(|name| name.starts_with(TRIGGER))
            || model == 0
            || model >= bsp.models.len()
        {
            continue;
        }
        placements.push(Placement {
            model,
            origin: entity.origin().map(to_f32),
            ..Placement::WORLD
        });
    }
    placements
}

/// The start of a trigger's classname: `trigger_multiple`, `trigger_teleport` and the rest.
pub(crate) const TRIGGER: &[u8] = b"trigger_";

/// A map coordinate as the renderer takes it.
#[expect(
    clippy::cast_possible_truncation,
    reason = "map coordinates and texture coordinates are far inside f32's range"
)]
const fn to_f32(value: f64) -> f32 {
    value as f32
}

/// A light level as a shade, held to 0 to 255.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the level is clamped to 0..=255 and rounded first"
)]
fn level_of(value: f32) -> u8 {
    value.round().clamp(0.0, f32::from(Shade::FULL.get())) as u8
}

#[cfg(test)]
mod tests;
