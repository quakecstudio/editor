// SPDX-License-Identifier: GPL-3.0-or-later

//! The wgpu renderer (P5-05, MAP-17): a BSP's world surfaces with their textures and
//! lightmaps, as GLQuake draws them (`docs/spec/render.md`), drawn inside egui's render pass
//! through a paint callback, so the map view and the preview host share one renderer
//! (ADR-0003).
//!
//! [`MapMesh::build`] turns a [`qs_bsp::Bsp`] into vertices, indices, textures and a lightmap
//! atlas on the CPU; [`MapRenderer`] uploads it and draws it; [`MapCallback`] draws it from
//! egui; [`Camera`] gives the matrix it is drawn with.
//!
//! Implements `docs/spec/render.md` REND-1 to REND-6.

mod alias;
mod anorm_dots;
mod camera;
mod gpu;
mod light;
mod mesh;
mod pick;
mod styles;

pub use alias::{AliasEntity, AliasError, AliasModel, AliasVertex, bob_rotate, shade_light};
pub use camera::Camera;
pub use gpu::{AliasDraw, MapCallback, MapRenderer, ModelFrame, View};
pub use light::light_point;
pub use mesh::{
    Animation, Batch, GAMMA, Image, Lightmaps, MapMesh, MeshBuilder, MeshError, Palette, Placement,
    Surface, Vertex, frame, mip_levels,
};
pub use pick::Hit;
pub use styles::{LightStyles, MAX_LIGHTSTYLES};
