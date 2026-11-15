// SPDX-License-Identifier: GPL-3.0-or-later

//! Importing PNG images as textures (P6-16, MAP-23): an image quantized to the game's palette,
//! with its mip levels and checked against Quake's texture names (WAD-6, WAD-7), written into
//! the project's own WAD, `textures/<project>.wad`, in place of a texture of the same name or
//! after the rest; and that WAD added to the open map's world `wad` key, with undo, so the map
//! compiler, the texture browser and the engines find it (MAP-6.3). [`quantized`] gives the
//! picture an import would save, for a preview before it does.

use std::path::{Path, PathBuf};

use qs_commands::{Command, Outcome};
use qs_wad::Wad;
use qs_wad::texture::{DecodedTexture, Dither, Palette, RgbaImage, TextureName};
use qs_wad::write::WadBuilder;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::textures::{Game, game_palette};
use crate::{EntityIndex, MapEdited, MapSource, edit_map};
use crate::{HasMap, MapEditError, MapProject};

/// The id of `texture.import`.
pub const TEXTURE_IMPORT: &str = "texture.import";

/// The project's folder its own WAD is kept in.
pub const TEXTURE_FOLDER: &str = "textures";
/// The WAD's name when the project's folder has none to give it.
const DEFAULT_WAD: &str = "project";
/// The world's keys that list the map's WADs, in the order the compilers read them: `_wad`,
/// else `wad` (MAP-6.3).
const WAD_KEYS: [&[u8]; 2] = [b"_wad", b"wad"];
/// The separator between the WADs the key lists, as ericw-tools reads it (MAP-6.3).
const WAD_SEPARATOR: char = ';';
/// The name the project's WAD is written under while it is written, then renamed from.
const WRITING: &str = "wad.writing";

/// How an imported image is matched to the palette (WAD-7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DitherChoice {
    /// Each pixel to its nearest colour.
    #[default]
    None,
    /// Floyd and Steinberg's error diffusion.
    FloydSteinberg,
}

impl From<DitherChoice> for Dither {
    fn from(choice: DitherChoice) -> Self {
        match choice {
            DitherChoice::None => Self::None,
            DitherChoice::FloydSteinberg => Self::FloydSteinberg,
        }
    }
}

/// The parameters of `texture.import`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextureImportParams {
    /// The PNG image.
    pub path: PathBuf,
    /// The texture's name; the image's file name, without its extension, when not given.
    #[serde(default)]
    pub name: Option<String>,
    /// How the image is matched to the palette.
    #[serde(default)]
    pub dither: DitherChoice,
}

/// What `texture.import` did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextureImported {
    /// The texture's name.
    pub name: String,
    /// The project's WAD it was written into.
    pub wad: PathBuf,
    /// Whether it took the place of a texture of the same name.
    pub replaced: bool,
    /// Whether the open map's world was given the WAD.
    pub map_names_wad: bool,
}

impl TextureImportParams {
    /// The texture's name: the one given, else the image's file name without its extension.
    fn texture_name(&self) -> Result<TextureName, MapEditError> {
        let name = self.name.clone().unwrap_or_else(|| {
            self.path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        TextureName::parse(name.as_bytes()).map_err(|error| MapEditError::Texture { error })
    }
}

/// The project's own WAD in the project at `root`: `textures/<folder>.wad`, the folder's name
/// kept to letters, digits, `-` and `_`, so the world's key can list it.
#[must_use]
pub fn project_wad(root: &Path) -> PathBuf {
    let folder: String = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let folder = if folder.is_empty() {
        DEFAULT_WAD.to_owned()
    } else {
        folder
    };
    root.join(TEXTURE_FOLDER).join(format!("{folder}.wad"))
}

/// The texture lump the PNG `png` makes, named `name`, in `palette`.
fn encode(
    png: &[u8],
    path: &Path,
    name: &TextureName,
    palette: &Palette,
    dither: DitherChoice,
) -> Result<Vec<u8>, MapEditError> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .map_err(|error| MapEditError::Image {
            path: path.to_path_buf(),
            error,
        })?
        .to_rgba8();
    let texture = |error| MapEditError::Texture { error };
    let rgba = RgbaImage::new(image.width(), image.height(), image.as_raw()).map_err(texture)?;
    rgba.encode(name, palette, dither.into(), None)
        .map_err(texture)
}

/// The picture `texture.import` would save for the PNG `png` named `name`, in `palette`: the
/// image quantized, as the texture browser would show it, for a preview before importing.
///
/// # Errors
///
/// [`MapEditError::Image`] when `png` is not a PNG, and [`MapEditError::Texture`] for a
/// name or size Quake's textures cannot take (WAD-6, WAD-7).
pub fn quantized(
    png: &[u8],
    name: &str,
    palette: &Palette,
    dither: DitherChoice,
) -> Result<DecodedTexture, MapEditError> {
    let name =
        TextureName::parse(name.as_bytes()).map_err(|error| MapEditError::Texture { error })?;
    let lump = encode(png, Path::new(""), &name, palette, dither)?;
    palette
        .decode_miptex(&lump)
        .map_err(|error| MapEditError::Texture { error })
}

/// The WAD at `path` with the texture `lump` named `name` in it: in the place of one of the
/// same name, else after the rest; and whether it replaced one.
fn with_texture(
    path: &Path,
    name: &TextureName,
    lump: Vec<u8>,
) -> Result<(Vec<u8>, bool), MapEditError> {
    let old = match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(MapEditError::Save {
                path: path.to_path_buf(),
                error,
            });
        }
    };
    let unreadable = |error| MapEditError::Wad {
        path: path.to_path_buf(),
        error,
    };
    let unwritable = |error| MapEditError::WadWrite {
        path: path.to_path_buf(),
        error,
    };
    let mut builder = WadBuilder::new();
    let mut new = Some(lump);
    let mut replaced = false;
    if let Some(old) = &old {
        let wad = Wad::read(old).map_err(unreadable)?;
        for entry in wad.lumps() {
            let data = wad.data(entry).map_err(unreadable)?;
            if entry.name.as_bytes().eq_ignore_ascii_case(name.as_bytes()) {
                if let Some(lump) = new.take() {
                    builder.add_texture(name, lump).map_err(unwritable)?;
                    replaced = true;
                }
            } else {
                builder
                    .add(entry.name.as_bytes(), entry.lump_type, data.to_vec())
                    .map_err(unwritable)?;
            }
        }
    }
    if let Some(lump) = new {
        builder.add_texture(name, lump).map_err(unwritable)?;
    }
    let bytes = builder.to_bytes().map_err(unwritable)?;
    Ok((bytes, replaced))
}

/// `texture.import`.
pub(crate) struct TextureImport;

impl<C: HasMap + 'static> Command<C> for TextureImport {
    const ID: &'static str = TEXTURE_IMPORT;
    const SUMMARY: &'static str = "Imports a PNG image as a texture: quantized to the game's \
        palette with its mip levels, written into the project's own WAD in textures/, in place \
        of a texture of the same name, and that WAD named in the open map's world.";
    type Params = TextureImportParams;
    type Output = TextureImported;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: TextureImportParams,
    ) -> Result<Outcome<TextureImported, C>, MapEditError> {
        let name = params.texture_name()?;
        let root = context
            .map_project()
            .project_root()
            .ok_or(MapEditError::NoProject)?;
        let game = context.game_folders().base;
        let palette = game_palette(Some(&root), game.as_deref().map(Game::at).as_ref())
            .ok_or(MapEditError::NoPalette)?;
        let png = std::fs::read(&params.path).map_err(|_| MapEditError::NoFile {
            path: params.path.clone(),
        })?;
        let lump = encode(&png, &params.path, &name, &palette, params.dither)?;
        let wad = project_wad(&root);
        let (bytes, replaced) = with_texture(&wad, &name, lump)?;
        if let Some(folder) = wad.parent() {
            std::fs::create_dir_all(folder).map_err(|error| MapEditError::Save {
                path: folder.to_path_buf(),
                error,
            })?;
        }
        // Written beside it, then renamed over it, so a failed write leaves the WAD as it was.
        let writing = wad.with_extension(WRITING);
        std::fs::write(&writing, bytes)
            .and_then(|()| std::fs::rename(&writing, &wad))
            .map_err(|error| MapEditError::Save {
                path: wad.clone(),
                error,
            })?;
        let file = wad.file_name().map(|n| n.to_string_lossy().into_owned());
        let named = format!("{TEXTURE_FOLDER}/{}", file.unwrap_or_default());
        let mut imported = TextureImported {
            name: String::from_utf8_lossy(name.as_bytes()).into_owned(),
            wad,
            replaced,
            map_names_wad: false,
        };
        // The open map's world, when it is a `.map`: the key it lists its WADs in, the one the
        // compilers read first, its value, and whether the WAD is among them.
        let listed = context.map_store().map().and_then(|document| {
            let MapSource::Map = document.source() else {
                return None;
            };
            let world = document.file().entities.first()?;
            let key = WAD_KEYS
                .into_iter()
                .find(|key| world.value(key).is_some_and(|value| !value.is_empty()))
                .unwrap_or(b"wad");
            let value = world.value(key).unwrap_or_default();
            let named_already = document
                .file()
                .wad_paths(qs_map::Compiler::EricwTools)
                .iter()
                .any(|wad| wad.as_slice() == named.as_bytes());
            Some((
                key,
                String::from_utf8_lossy(value).into_owned(),
                named_already,
                document.path().to_path_buf(),
            ))
        });
        let Some((key, listed, named_already, map)) = listed else {
            return Ok(Outcome::outside_history(imported));
        };
        imported.map_names_wad = true;
        if named_already {
            return Ok(Outcome::outside_history(imported));
        }
        let value = if listed.trim().is_empty() {
            named
        } else {
            format!("{listed}{WAD_SEPARATOR}{named}")
        };
        let ((), step) = edit_map(context, |document| {
            document
                .entity_mut(EntityIndex::WORLD)?
                .set(key, value.as_bytes())
                .map_err(|error| MapEditError::Map { path: map, error })
        })?;
        Ok(Outcome::undoable(imported, step))
    }
}

/// The id of the command that adds a WAD file to the map's list (P7-08).
pub const MAP_ADD_WAD: &str = "map.add_wad";

/// The parameters of `map.add_wad`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddWadParams {
    /// The WAD file.
    pub path: PathBuf,
}

/// `map.add_wad`.
pub(super) struct AddWad;

impl<C: HasMap + 'static> Command<C> for AddWad {
    const ID: &'static str = MAP_ADD_WAD;
    const SUMMARY: &'static str = "Adds a WAD file to the open map's list in its world's `wad` \
        or `_wad` key, by its path from the map's folder when it is under it, as TrenchBroom \
        adds a WAD dropped on its window; one listed already is not listed again.";
    type Params = AddWadParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: AddWadParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
        let document = context.map_store().map().ok_or(MapEditError::NoMap)?;
        let map = document.path().to_path_buf();
        let folder = map.parent().map(Path::to_path_buf).unwrap_or_default();
        let named = params.path.strip_prefix(&folder).unwrap_or(&params.path);
        let named = named.to_string_lossy().replace('\\', "/");
        let world = document
            .file()
            .entities
            .first()
            .ok_or(MapEditError::NoMap)?;
        let key = WAD_KEYS
            .into_iter()
            .find(|key| world.value(key).is_some_and(|value| !value.is_empty()))
            .unwrap_or(b"wad");
        let listed = String::from_utf8_lossy(world.value(key).unwrap_or_default()).into_owned();
        let added = MapEdited {
            entity: Some(EntityIndex::WORLD),
        };
        if listed.split(WAD_SEPARATOR).any(|wad| wad == named) {
            return Ok(Outcome::outside_history(added));
        }
        let value = if listed.trim().is_empty() {
            named
        } else {
            format!("{listed}{WAD_SEPARATOR}{named}")
        };
        let ((), step) = edit_map(context, |document| {
            document
                .entity_mut(EntityIndex::WORLD)?
                .set(key, value.as_bytes())
                .map_err(|error| MapEditError::Map { path: map, error })
        })?;
        Ok(Outcome::undoable(added, step))
    }
}

#[cfg(test)]
mod tests;
