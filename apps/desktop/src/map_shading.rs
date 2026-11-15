// SPDX-License-Identifier: GPL-3.0-or-later

//! How the map editor's 3D view draws the map (P7-13, MAP-17): as its brushes' edges, as their
//! faces with their textures, shaded by the way each faces as TrenchBroom draws them, or lit by
//! the last compile's lightmaps and coloured light. Textured and Lit are drawn by `qs-render`,
//! the renderer the game and the BSP view use, under the editor's own lines.
//!
//! Every string comes from [`crate::strings`]; a test fails on a string literal here.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui::{Rect, Ui};
use eframe::egui_wgpu::{self, RenderState};
use qs_bsp::{Bsp, BspLoader, Lit};
use qs_layout::MapShading;
use qs_map_edit::textures::TextureSources;
use qs_map_edit::{MapDocument, MapRevision};
use qs_render::{
    Camera, EditorCallback, EditorRenderer, Extras, FlatFace, Image, Look, MapMesh, MapRenderer,
    MeshBuilder, Placement, Shade, View,
};
use qs_wad::texture::{Palette, PaletteIndex};

use crate::map_scene::{FaceShape, Scene};
use crate::map_view::DEPTH_FORMAT;
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

/// What the renderer was last given, so it is made again only when that changes.
#[derive(Clone, Debug, PartialEq)]
enum Made {
    /// The textured faces of the map at a revision.
    Textured(PathBuf, MapRevision),
    /// The compiled BSP at its time on disk.
    Lit(PathBuf, Option<SystemTime>),
}

/// The state of the Lit view's compile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LitState {
    /// The map has no BSP beside it yet.
    NoCompile,
    /// The BSP is at least as new as the map.
    Current,
    /// The map was saved after the BSP was written.
    Stale,
}

/// The 3D view's shading, as the layout holds it, and the mesh it draws.
#[derive(Debug, Default)]
pub(crate) struct ShadedView {
    shading: MapShading,
    made: Option<Made>,
    lit: Option<LitState>,
}

impl ShadedView {
    /// The toolbar's choice of shading: the one clicked, which `view.map_shading` sets.
    pub(crate) fn toolbar(&self, ui: &mut Ui) -> Option<MapShading> {
        let mut chosen = None;
        for shading in MapShading::ALL {
            if ui
                .selectable_label(self.shading == shading, text(shading).get())
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

    /// Whether the renderer draws the faces this frame, so the view leaves out the world's
    /// edges.
    pub(crate) const fn draws_faces(&self) -> bool {
        match self.shading {
            MapShading::Lines => false,
            MapShading::Textured => matches!(self.made, Some(Made::Textured(..))),
            MapShading::Lit => matches!(self.made, Some(Made::Lit(..))),
        }
    }

    /// Makes the renderer's mesh again when the shading, the map or its compile changed:
    /// the textured faces of `scene` with `sources`' textures, or the BSP beside the map.
    pub(crate) fn refresh(
        &mut self,
        shading: MapShading,
        state: Option<&RenderState>,
        document: &MapDocument,
        scene: &Scene,
        sources: Option<&TextureSources>,
    ) {
        self.shading = shading;
        let Some(state) = state else {
            return;
        };
        let Some(palette) = sources.and_then(TextureSources::palette) else {
            return;
        };
        let path = document.path().to_path_buf();
        match self.shading {
            MapShading::Lines => {}
            MapShading::Textured => {
                let want = Made::Textured(path, document.revision());
                if self.made.as_ref() != Some(&want) {
                    let mesh = scene.textured_mesh(sources, Look::QuakeSpasm);
                    install(state, &mesh);
                    self.made = Some(want);
                }
            }
            MapShading::Lit => {
                let bsp = path.with_extension("bsp");
                let written = modified(&bsp);
                self.lit = Some(match (written, modified(&path)) {
                    (None, _) => LitState::NoCompile,
                    (Some(bsp), Some(map)) if map > bsp => LitState::Stale,
                    (Some(_), _) => LitState::Current,
                });
                let want = Made::Lit(bsp.clone(), written);
                if written.is_some() && self.made.as_ref() != Some(&want) {
                    match lit(&bsp, palette) {
                        Ok(mesh) => {
                            install(state, &mesh);
                            self.made = Some(want);
                        }
                        Err(error) => tracing::warn!(%error, path = %bsp.display(), "lit view"),
                    }
                }
            }
        }
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

/// Puts a renderer of `mesh` in `state`'s callback resources as the editor's, replacing the
/// one there.
fn install(state: &RenderState, mesh: &MapMesh) {
    let renderer = MapRenderer::new(
        &state.device,
        &state.queue,
        (state.target_format, DEPTH_FORMAT),
        mesh,
        &qs_render::Assets::default(),
    );
    state
        .renderer
        .write()
        .callback_resources
        .insert(EditorRenderer(renderer));
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
    /// The textured mesh of the scene's brush faces: each texture decoded from `sources`, one
    /// the sources lack drawn as Quake's black and pink checker, and each face shaded by its
    /// normal.
    pub(crate) fn textured_mesh(&self, sources: Option<&TextureSources>, look: Look) -> MapMesh {
        let mut names: Vec<&str> = Vec::new();
        let mut images: Vec<Image> = Vec::new();
        let mut faces = Vec::new();
        for brush in &self.brushes {
            for face in &brush.faces {
                let slot = match names
                    .iter()
                    .position(|n| n.eq_ignore_ascii_case(&face.texture))
                {
                    Some(slot) => slot,
                    None => {
                        names.push(&face.texture);
                        images.push(decoded(sources, &face.texture));
                        names.len().saturating_sub(1)
                    }
                };
                let Some(image) = images.get(slot) else {
                    continue;
                };
                if let Some(flat) = face.flat(slot, image) {
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

/// The texture `name` from `sources` as an image; Quake's 2 by 2 black and pink checker when
/// it is not found (REND-13, `notexture_mip`).
fn decoded(sources: Option<&TextureSources>, name: &str) -> Image {
    match sources.and_then(|sources| sources.decode(name)) {
        Some(texture) => Image {
            width: texture.width,
            height: texture.height,
            rgba: texture.rgba,
        },
        None => Image {
            width: 2,
            height: 2,
            rgba: MISSING.to_vec(),
        },
    }
}

/// Quake's missing-texture checker, `notexture_data`: pink, black, black, pink.
const MISSING: [u8; 16] = [
    159, 91, 83, 255, 0, 0, 0, 255, 0, 0, 0, 255, 159, 91, 83, 255,
];

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
    let placements: Vec<Placement> = (0..bsp.models.len())
        .map(|model| Placement {
            model,
            ..Placement::WORLD
        })
        .collect();
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
