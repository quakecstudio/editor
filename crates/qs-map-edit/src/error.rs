// SPDX-License-Identifier: GPL-3.0-or-later

//! Why a map command could not open, change or save the open map.

use core::fmt;
use std::error::Error;
use std::path::PathBuf;

use qs_map::shapes::ShapeError;
use qs_wad::texture::TextureError;
use qs_wad::write::WadWriteError;

use crate::uv::FaceIndex;
use crate::{BrushIndex, EntityIndex, MapHunkNumber};

/// Why a map command could not open, change or save the open map.
#[derive(Debug)]
pub enum MapEditError {
    /// The file could not be written.
    Save {
        /// The file.
        path: PathBuf,
        /// Why.
        error: std::io::Error,
    },
    /// The file could not be read.
    NoFile {
        /// The file.
        path: PathBuf,
    },
    /// A map to make whose file is there already (`map.new`).
    Exists {
        /// The file.
        path: PathBuf,
    },
    /// No project is open.
    NoProject,
    /// A prefab's name is not one its file can have: letters, digits, `-` and `_` (P6-23).
    PrefabName {
        /// The name.
        name: String,
    },
    /// Nothing is selected to make a prefab of (P6-23).
    NoSelection,
    /// No map is open in the map view.
    NoMap,
    /// The open map has no entity at that place.
    NoEntity {
        /// The entity asked for.
        entity: EntityIndex,
    },
    /// The world, or a brush entity, where only a point entity can be rotated or deleted.
    NotPointEntity {
        /// The entity.
        entity: EntityIndex,
    },
    /// A BSP that does not load or cannot take a new entity lump.
    Bsp {
        /// The file.
        path: PathBuf,
        /// Why.
        error: qs_bsp::BspError,
    },
    /// An entity that is no group's own entity, or a group not linked, where one is needed
    /// (P7-08).
    NotGroup {
        /// The entity.
        entity: EntityIndex,
    },
    /// An entity that is not a brush entity, where one is needed (P7-08).
    NotBrushEntity {
        /// The entity.
        entity: EntityIndex,
    },
    /// No face of the brushes has the texture to replace (P7-08).
    NoTexture {
        /// The texture.
        texture: String,
    },
    /// Text pasted into the map that is not a map's entities (P7-08).
    NotMapText {
        /// Why.
        error: qs_map::MapError,
    },
    /// A `.map` that does not parse, or a key it cannot hold.
    Map {
        /// The file.
        path: PathBuf,
        /// Why.
        error: qs_map::MapError,
    },
    /// Another program changed the map's file since the editor read or saved it (P5-17):
    /// saving would overwrite that, so the map is reloaded or kept first (`map.resolve`).
    ChangedOnDisk {
        /// The file.
        path: PathBuf,
    },
    /// An image could not be read (P6-16).
    Image {
        /// The image.
        path: PathBuf,
        /// Why.
        error: image::ImageError,
    },
    /// A texture cannot be made: its name or size breaks Quake's rules (P6-16, WAD-6, WAD-7).
    Texture {
        /// Why.
        error: TextureError,
    },
    /// A WAD could not be read (P6-16, WAD-2).
    Wad {
        /// The WAD.
        path: PathBuf,
        /// Why.
        error: qs_wad::WadError,
    },
    /// A WAD could not be written (P6-16, WAD-5).
    WadWrite {
        /// The WAD.
        path: PathBuf,
        /// Why.
        error: WadWriteError,
    },
    /// No palette was found in the project or the game to make a texture in (P6-16).
    NoPalette,
    /// A quick fix would remove a brush that has volume: the map changed since its issue was
    /// found (P6-19).
    NotEmpty {
        /// The entity.
        entity: EntityIndex,
        /// The brush.
        brush: BrushIndex,
    },
    /// The entity has no such key (P6-19).
    NoKey {
        /// The entity.
        entity: EntityIndex,
        /// The key.
        key: String,
    },
    /// The brush has no face at this place (P6-22).
    NoFace {
        /// The entity.
        entity: EntityIndex,
        /// The brush.
        brush: BrushIndex,
        /// The face.
        face: FaceIndex,
    },
    /// No WAD or BSP of the map holds the texture, so its size, which the edit needs, is not
    /// known (P6-22, MAP-13).
    NoTextureSize {
        /// The texture's name.
        texture: String,
    },
    /// A scale factor of 0, infinite or not a number, which would leave no texture (P6-22,
    /// MAP-13.11).
    BadScale {
        /// The factors given.
        scale: [f64; 2],
    },
    /// The open map is a BSP with no `.map` source, so there is nothing to compile (P6-13).
    NoMapSource {
        /// The BSP.
        path: PathBuf,
    },
    /// The open map's review has no hunk of this number (P6-21).
    NoMapHunk {
        /// The number asked for.
        hunk: MapHunkNumber,
    },
    /// The open map has no room of this name (MAP-9).
    NoRoom {
        /// The name asked for.
        name: String,
    },
    /// A `.ent` beside the BSP, which the engine reads in place of the entity lump
    /// (BSP-19.1), so a rewritten lump would not be seen.
    EntOverride {
        /// The `.ent`.
        path: PathBuf,
    },
    /// The entity is locked, by `map.lock` or in a locked layer (P5-14).
    Locked {
        /// The entity.
        entity: EntityIndex,
    },
    /// The entity cannot be moved into a layer or group: it is the world or a layer (P5-16).
    CannotMove {
        /// The entity.
        entity: EntityIndex,
    },
    /// The entity is no layer or group to move into, or is the one being moved or one inside
    /// it (P5-16).
    NotContainer {
        /// The entity.
        entity: EntityIndex,
    },
    /// The entity has no brush at this place (P6-08).
    NoBrush {
        /// The entity.
        entity: EntityIndex,
        /// The brush.
        brush: BrushIndex,
    },
    /// A brush edit would leave no valid brush (P6-08, MAP-3.4).
    Brush {
        /// Why.
        error: qs_map::BrushError,
    },
    /// A shape's settings are out of range, or a brush of it would not be valid (P6-18,
    /// MAP-10).
    Shape {
        /// Why.
        error: ShapeError,
    },
    /// The entity is no TrenchBroom layer (MAP-9.1).
    NotLayer {
        /// The entity.
        entity: EntityIndex,
    },
    /// A linked group's copies could not be updated (MAP-9.7).
    Link {
        /// Why.
        error: qs_map::LinkError,
    },
}

impl fmt::Display for MapEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Save { path, error } => {
                write!(f, "{}: cannot be written: {error}", path.display())
            }
            Self::NoFile { path } => write!(f, "{}: cannot be read", path.display()),
            Self::Exists { path } => write!(f, "{}: is there already", path.display()),
            Self::NoProject => f.write_str("no project is open"),
            Self::PrefabName { name } => write!(
                f,
                "{name:?} is no prefab name: use letters, digits, - and _"
            ),
            Self::NoSelection => f.write_str("nothing is selected to make a prefab of"),
            Self::NoMap => f.write_str("no map is open"),
            Self::NoEntity { entity } => write!(f, "the map has no entity {entity}"),
            Self::NotPointEntity { entity } => {
                write!(f, "entity {entity} is the world or a brush entity")
            }
            Self::Bsp { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Map { path, error } => write!(f, "{}: {error}", path.display()),
            Self::NotMapText { error } => write!(f, "the text pasted is not a map's: {error}"),
            Self::NotGroup { entity } => write!(f, "entity {entity} is no group, or not linked"),
            Self::NoTexture { texture } => write!(f, "no face has the texture {texture}"),
            Self::NotBrushEntity { entity } => write!(f, "entity {entity} is no brush entity"),
            Self::ChangedOnDisk { path } => write!(
                f,
                "{}: another program changed it since it was opened; reload it or keep yours \
                 first",
                path.display()
            ),
            Self::Image { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Texture { error } => write!(f, "{error}"),
            Self::Wad { path, error } => write!(f, "{}: {error}", path.display()),
            Self::WadWrite { path, error } => write!(f, "{}: {error}", path.display()),
            Self::NoPalette => {
                f.write_str("no gfx/palette.lmp in the project or the game to make a texture in")
            }
            Self::NotEmpty { entity, brush } => write!(
                f,
                "brush {} of entity {entity} has volume, so it is not removed",
                brush.get()
            ),
            Self::NoKey { entity, key } => write!(f, "entity {entity} has no key {key}"),
            Self::NoFace {
                entity,
                brush,
                face,
            } => write!(
                f,
                "brush {} of entity {entity} has no face {}",
                brush.get(),
                face.get()
            ),
            Self::NoTextureSize { texture } => {
                write!(
                    f,
                    "no WAD of the map holds {texture}, so its size is not known"
                )
            }
            Self::BadScale { scale } => {
                write!(
                    f,
                    "a scale factor must be a number other than 0, not {scale:?}"
                )
            }
            Self::NoMapSource { path } => {
                write!(
                    f,
                    "{}: a BSP with no .map source to compile",
                    path.display()
                )
            }
            Self::NoMapHunk { hunk } => write!(f, "the map's review has no hunk {hunk}"),
            Self::NoRoom { name } => write!(f, "the map has no room named {name}"),
            Self::EntOverride { path } => write!(
                f,
                "{}: the engine reads this in place of the BSP's entities; save to it, or move \
                 it away before saving into the BSP",
                path.display()
            ),
            Self::Locked { entity } => write!(f, "entity {entity} is locked"),
            Self::CannotMove { entity } => {
                write!(
                    f,
                    "entity {entity} is the world or a layer, which go in nothing"
                )
            }
            Self::NotContainer { entity } => {
                write!(f, "entity {entity} is no layer or group it can go into")
            }
            Self::NoBrush { entity, brush } => {
                write!(f, "entity {entity} has no brush {}", brush.get())
            }
            Self::Brush { error } => write!(f, "{error}"),
            Self::Shape { error } => write!(f, "{error}"),
            Self::NotLayer { entity } => write!(f, "entity {entity} is no layer"),
            Self::Link { error } => write!(f, "{error}"),
        }
    }
}

impl Error for MapEditError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Save { error, .. } => Some(error),
            Self::Bsp { error, .. } => Some(error),
            Self::Map { error, .. } => Some(error),
            Self::NotMapText { error } => Some(error),
            Self::Image { error, .. } => Some(error),
            Self::Texture { error } => Some(error),
            Self::Wad { error, .. } => Some(error),
            Self::WadWrite { error, .. } => Some(error),
            Self::Brush { error } => Some(error),
            Self::Shape { error } => Some(error),
            Self::Link { error } => Some(error),
            Self::NoFile { .. }
            | Self::Exists { .. }
            | Self::NoProject
            | Self::PrefabName { .. }
            | Self::NoSelection
            | Self::NotGroup { .. }
            | Self::NoTexture { .. }
            | Self::NotBrushEntity { .. }
            | Self::NoMap
            | Self::NoEntity { .. }
            | Self::NotPointEntity { .. }
            | Self::ChangedOnDisk { .. }
            | Self::NoPalette
            | Self::NotEmpty { .. }
            | Self::NoKey { .. }
            | Self::NoFace { .. }
            | Self::NoTextureSize { .. }
            | Self::BadScale { .. }
            | Self::NoMapSource { .. }
            | Self::NoMapHunk { .. }
            | Self::NoRoom { .. }
            | Self::EntOverride { .. }
            | Self::Locked { .. }
            | Self::CannotMove { .. }
            | Self::NotContainer { .. }
            | Self::NoBrush { .. }
            | Self::NotLayer { .. } => None,
        }
    }
}
