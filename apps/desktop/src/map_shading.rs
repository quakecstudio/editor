// SPDX-License-Identifier: GPL-3.0-or-later

//! How the map editor's 3D view draws the map (P7-13, MAP-17): as its brushes' edges, as their
//! faces with their textures, shaded by the way each faces as TrenchBroom draws them, or lit by
//! the last compile's lightmaps and coloured light. Textured and Lit are drawn by `qs-render`,
//! the renderer the game and the BSP view use, under the editor's own lines.
//!
//! Every string comes from [`crate::strings`]; a test fails on a string literal here.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use eframe::egui::{Rect, Ui};
use eframe::egui_wgpu::{self, RenderState};
use qs_bsp::{Bsp, BspLoader, Lit};
use qs_layout::MapShading;
use qs_map::MapFile;
use qs_map_edit::textures::TextureSources;
use qs_map_edit::{EntityIndex, MapDocument, MapRevision};
use qs_render::{
    AliasDraw, AliasEntity, AliasModel, Camera, EditorCallback, EntityPass, Extras, FlatFace,
    Image, Look, MapMesh, MeshBuilder, ModelFrame, Placement, Shade, View,
};
use qs_wad::texture::Palette;

use crate::icons::{Icon, Toggled};
use crate::map_scene::{FaceShape, Line, Scene};
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
            let icon = match shading {
                MapShading::Lines => Icon::Lines,
                MapShading::Textured => Icon::Textured,
                MapShading::Lit => Icon::Lit,
            };
            let state = if current == shading {
                Toggled::On
            } else {
                Toggled::Off
            };
            if icon
                .toggle(ui, state, text(shading), hint(shading))
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
        (sources, models): (Option<&TextureSources>, &HashMap<String, Arc<[u8]>>),
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
                    let palette = sources.and_then(TextureSources::palette);
                    let posed = scene.posed_models(models, palette, hidden);
                    map_view::install_editor(state, &mesh, &posed);
                    self.made = Some(Made::Textured(from));
                }
            }
            MapShading::Lit => {
                let palette = sources.and_then(TextureSources::palette);
                let pose = || scene.posed_models(models, palette, hidden);
                self.refresh_lit(state, document, sources, &pose);
            }
        }
    }

    /// Follows the BSP beside the map: drawn again when it is written again, none drawn when it
    /// is gone, and the map's revision it was compiled from noted when it is first seen.
    fn refresh_lit(
        &mut self,
        state: &RenderState,
        document: &MapDocument,
        sources: Option<&TextureSources>,
        pose: &dyn Fn() -> PosedModels,
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
                    map_view::install_editor(state, &mesh, &pose());
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
    /// The brushes, by their place, the textured view draws nothing of: those of `triggers`,
    /// and those whose every face is unseen ([`FaceShape::unseen`]), such as a clip or hint
    /// brush. The map editor draws their edges over the faces, so they stay visible and can be
    /// picked; a brush with a face drawn is seen by it, as TrenchBroom outlines none.
    pub(crate) fn outlined(&self, triggers: &BTreeSet<EntityIndex>) -> Vec<usize> {
        self.brushes
            .iter()
            .enumerate()
            .filter(|(_, b)| triggers.contains(&b.entity) || b.faces.iter().all(FaceShape::unseen))
            .map(|(index, _)| index)
            .collect()
    }

    /// The edges of the `outlined` brushes ([`Scene::outlined`]) not of a `hidden` entity.
    pub(crate) fn outlined_lines<'a>(
        &'a self,
        outlined: &'a [usize],
        hidden: &'a BTreeSet<EntityIndex>,
    ) -> impl Iterator<Item = &'a Line> + 'a {
        outlined
            .iter()
            .filter_map(|&index| self.brushes.get(index))
            .filter(|brush| !hidden.contains(&brush.entity))
            .flat_map(|brush| &brush.lines)
    }

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
            for face in brush.faces.iter().filter(|f| !f.unseen()) {
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

/// The light the editor draws a model in: the most GLQuake's caps let an entity's
/// `ambientlight` and `shadelight` reach together (REND-4.7), a `shadelight` of 64 over 200.
const MODEL_LIGHT: f32 = 192.0;

/// What the editor raises a model's shade by: 200 over that `shadelight` of 64, so each vertex
/// is its normal's shade dot itself (REND-4.8), the skin as it is where the vertex faces the
/// light and darker where it faces away, as the textured faces are shaded by the way they
/// face; the game's caps would draw it at a third of that.
const MODEL_SHADE_GAIN: f32 = 200.0 / 64.0;

/// The class models of a scene, posed for the renderer: every skin image, and each entity's
/// first frame at its origin, turned to its angle.
#[derive(Clone, Debug, Default)]
pub(crate) struct PosedModels {
    /// Every model's skin images, one model's after another's.
    pub(crate) skins: Vec<Image>,
    /// The entities' triangles and the skin each run is drawn with.
    pub(crate) frame: ModelFrame,
}

impl Scene {
    /// The class models of the point entities not `hidden`, read from `files` through
    /// `palette`, posed at their first frame and first skin, as TrenchBroom shows an entity's
    /// model; none without a palette.
    pub(crate) fn posed_models(
        &self,
        files: &HashMap<String, Arc<[u8]>>,
        palette: Option<&Palette>,
        hidden: &BTreeSet<EntityIndex>,
    ) -> PosedModels {
        let mut posed = PosedModels::default();
        let Some(palette) = palette.map(qs_render::Palette::from) else {
            return posed;
        };
        let mut read: HashMap<&str, Option<(AliasModel, usize)>> = HashMap::new();
        for shape in &self.entities {
            if shape.brush || hidden.contains(&shape.index) {
                continue;
            }
            let Some(name) = shape.model.as_deref() else {
                continue;
            };
            let model = read.entry(name).or_insert_with(|| {
                let model = AliasModel::read(name, files.get(name)?, &palette).ok()?;
                let first = posed.skins.len();
                posed.skins.extend(model.images().iter().cloned());
                Some((model, first))
            });
            let Some((model, first)) = model else {
                continue;
            };
            let start = posed.frame.vertices.len();
            let entity =
                AliasEntity::new(shape.origin, [0.0, shape.yaw.get(), 0.0], 0, 0, MODEL_LIGHT);
            let image = model.draw(&entity, 0.0, &mut posed.frame.vertices);
            for vertex in posed.frame.vertices.iter_mut().skip(start) {
                vertex.shade = (vertex.shade * MODEL_SHADE_GAIN).min(1.0);
            }
            let (Ok(first_vertex), Ok(count)) = (
                u32::try_from(start),
                u32::try_from(posed.frame.vertices.len().saturating_sub(start)),
            ) else {
                continue;
            };
            posed.frame.draws.push(AliasDraw {
                skin: first.saturating_add(image),
                first: first_vertex,
                count,
                pass: EntityPass::Opaque,
            });
        }
        posed
    }
}

/// The mesh of the BSP at `path`, every model at its compiled place, lit by its lightmaps and
/// the `.lit` beside it, through `palette`.
fn lit(path: &Path, palette: &Palette) -> Result<MapMesh, String> {
    let palette = qs_render::Palette::from(palette);
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

impl FaceShape {
    /// Whether the game never draws this face, so the textured view leaves it out as the lit
    /// one does: its texture, the name after its last `/`, is a nodraw or hint one
    /// (QBSP-15.16), `clip` (QBSP-4.1) or `origin` (QBSP-15.13), in any case.
    pub(crate) fn unseen(&self) -> bool {
        let name = self.texture.rsplit('/').next().unwrap_or_default();
        UNSEEN
            .iter()
            .any(|unseen| name.eq_ignore_ascii_case(unseen))
    }
}

/// The textures of [`FaceShape::unseen`].
const UNSEEN: [&str; 10] = [
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
