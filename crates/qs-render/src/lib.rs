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
mod md5;
mod mesh;
mod offscreen;
mod particles;
mod pick;
mod picture;
mod play;
mod skybox;
mod sprite;
mod styles;

pub use alias::{
    AliasEntity, AliasError, AliasKind, AliasModel, AliasVertex, bob_rotate, entity_light,
    shade_light,
};
pub use camera::Camera;
pub use gpu::{
    AliasDraw, Assets, FlatDraw, FlatFrame, FlatVertex, Fog, MapCallback, MapRenderer, ModelFrame,
    PlayerSkin, SkinKey, SpriteDraw, SpriteFrame, View,
};
pub use light::light_point;
pub use md5::Md5Model;
pub use mesh::{
    Animation, Batch, Extras, GAMMA, Image, Lightmaps, MapMesh, MeshBuilder, MeshError, Palette,
    Placement, REND_MAX_DLIGHTS, Surface, Vertex, frame, mip_levels,
};
pub use offscreen::{Offscreen, OffscreenError, Pixels};
pub use particles::{ParticleVertex, particle_texture, particle_triangles};
pub use pick::Hit;
pub use play::{FogState, Pics, Play, PlayError, fog_parse_worldspawn};
pub use skybox::{SKY_SIDES, SkyKeys, Skybox, sky_parse_worldspawn};
pub use sprite::{Eye, SpriteEntity, SpriteError, SpriteModel, SpriteVertex};
pub use styles::{LightStyles, MAX_LIGHTSTYLES};
