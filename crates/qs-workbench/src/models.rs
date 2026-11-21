// SPDX-License-Identifier: GPL-3.0-or-later

//! The `model.*` commands (L-47): a project's `.mdl` alias models read, changed and written
//! back, each change one step with undo. A model is what a monster, a weapon or an item looks
//! like: its skins, its triangles and where each vertex is in each frame (MDL-1 to MDL-5).
//! `qs_mdl::mdl::write` holds one whole and writes it as id's modelgen does (MDL-6).

use std::fmt;
use std::path::{Path, PathBuf};

use qs_commands::{Access, BoxError, Command, Outcome, RegisterError, Registry, UndoStep};
use qs_map_edit::HasMap;
use qs_map_edit::textures::light_palette;
use qs_mdl::mdl::write::{MdlWriteError, Model, ModelFrame, ModelFrameEntry, ModelSkin};
use qs_mdl::mdl::{Mdl, MdlError, TriVertX};
use qs_vfs::{StdFs, Vfs};
use qs_wad::texture::{Palette, PaletteIndex};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::HasWorkbench;

/// The id of `model.inspect`.
pub const MODEL_INSPECT: &str = "model.inspect";
/// The id of `model.copy_from_game`.
pub const MODEL_COPY: &str = "model.copy_from_game";
/// The id of `model.export_skin`.
pub const MODEL_EXPORT_SKIN: &str = "model.export_skin";
/// The id of `model.set_skin`.
pub const MODEL_SET_SKIN: &str = "model.set_skin";
/// The id of `model.recolor`.
pub const MODEL_RECOLOR: &str = "model.recolor";
/// The id of `model.scale`.
pub const MODEL_SCALE: &str = "model.scale";
/// The id of `model.move`.
pub const MODEL_MOVE: &str = "model.move";
/// The id of `model.rename_frames`.
pub const MODEL_RENAME_FRAMES: &str = "model.rename_frames";
/// The id of `model.remove_frames`.
pub const MODEL_REMOVE_FRAMES: &str = "model.remove_frames";
/// The id of `model.add_frames`.
pub const MODEL_ADD_FRAMES: &str = "model.add_frames";

/// The colours of a row of the palette, and its rows: a skin's colours come in rows of 16
/// shades (REND-4.13).
const ROW: u8 = 16;
/// A colour's row is its high four bits, its shade its low four.
const ROW_SHIFT: u32 = 4;
const SHADE_MASK: u8 = 0x0F;
/// The first colour of the rows whose shades run from bright to dark, "backwards ranges"
/// (REND-4.13).
const BACKWARDS_FROM: u8 = 128;
/// The bytes of a pixel of a PNG read or written here: red, green, blue and alpha.
const RGBA: usize = 4;
/// A pixel that shows all of its colour.
const OPAQUE: u8 = 255;

/// Why a model command failed.
#[derive(Debug)]
pub enum ModelError {
    /// No project is open.
    NoProject,
    /// The file could not be read.
    Read {
        /// The file.
        path: PathBuf,
        /// Why.
        error: std::io::Error,
    },
    /// The file could not be written.
    Write {
        /// The file.
        path: PathBuf,
        /// Why.
        error: std::io::Error,
    },
    /// The file is no alias model.
    Model {
        /// The file.
        path: PathBuf,
        /// Why.
        error: MdlError,
    },
    /// The model as changed cannot be written.
    Unwritable(MdlWriteError),
    /// The model has no skin of the number asked for, or the skin is a group.
    NoSkin {
        /// The number.
        skin: usize,
        /// How many skins the model has.
        skins: usize,
    },
    /// The model has no frame of the name asked for.
    NoFrame {
        /// The name.
        name: String,
    },
    /// The image is no PNG.
    Image(image::ImageError),
    /// The image is not the size of the model's skins.
    SkinSize {
        /// The image's width and height.
        image: [u32; 2],
        /// The skins' width and height.
        skin: [i32; 2],
    },
    /// The game's palette, `gfx/palette.lmp`, was not found in the project or the game.
    NoPalette,
    /// A row of the palette is from 0 to 15.
    Row {
        /// The row asked for.
        row: u8,
    },
    /// No game was found to read the model from, or the game has no such file.
    NotInGame {
        /// The file's name in the game.
        name: String,
    },
    /// The file to make is there already.
    Exists {
        /// The file.
        path: PathBuf,
    },
    /// A scale that is 0 or less, or no number.
    Scale,
    /// The two models do not have the same vertices, so a frame of one is none of the other.
    Vertices {
        /// How many the model has.
        model: usize,
        /// How many the model the frames come from has.
        other: usize,
    },
    /// A frame to add reaches outside the box the model's frames fill, which its vertices
    /// are packed in.
    OutsideBox {
        /// The frame.
        name: String,
    },
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoProject => f.write_str("no project is open"),
            Self::Read { path, error } => {
                write!(f, "{}: cannot be read: {error}", path.display())
            }
            Self::Write { path, error } => {
                write!(f, "{}: cannot be written: {error}", path.display())
            }
            Self::Model { path, error } => {
                write!(f, "{}", error.message(&path.display().to_string()))
            }
            Self::Unwritable(error) => write!(f, "the model cannot be written: {error}"),
            Self::NoSkin { skin, skins } => write!(
                f,
                "the model has no single skin {skin}: it has {skins}, numbered from 0"
            ),
            Self::NoFrame { name } => write!(f, "the model has no frame named {name}"),
            Self::Image(error) => write!(f, "the image is no PNG: {error}"),
            Self::SkinSize {
                image: [iw, ih],
                skin: [sw, sh],
            } => write!(
                f,
                "the image is {iw} by {ih}: a skin of this model is {sw} by {sh}"
            ),
            Self::NoPalette => f.write_str(
                "the game's palette, gfx/palette.lmp, was not found in the project or the game",
            ),
            Self::Row { row } => write!(f, "{row} is no row of the palette: a row is 0 to 15"),
            Self::NotInGame { name } => write!(f, "the game has no file {name}"),
            Self::Exists { path } => write!(f, "{}: is there already", path.display()),
            Self::Scale => f.write_str("a scale is a number above 0"),
            Self::Vertices { model, other } => write!(
                f,
                "the model has {model} vertices and the one the frames come from {other}: \
                 frames are added between models of the same vertices"
            ),
            Self::OutsideBox { name } => write!(
                f,
                "the frame {name} reaches outside the box the model's frames fill"
            ),
        }
    }
}

impl core::error::Error for ModelError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Read { error, .. } | Self::Write { error, .. } => Some(error),
            Self::Model { error, .. } => Some(error),
            Self::Unwritable(error) => Some(error),
            Self::Image(error) => Some(error),
            Self::NoProject
            | Self::NoSkin { .. }
            | Self::NoFrame { .. }
            | Self::SkinSize { .. }
            | Self::NoPalette
            | Self::Row { .. }
            | Self::NotInGame { .. }
            | Self::Exists { .. }
            | Self::Scale
            | Self::Vertices { .. }
            | Self::OutsideBox { .. } => None,
        }
    }
}

/// A model's file as it was and as a command left it: undo writes the first back, redo the
/// second.
struct ModelStep {
    path: PathBuf,
    before: Vec<u8>,
    after: Vec<u8>,
}

impl<C: 'static> UndoStep<C> for ModelStep {
    fn undo(&mut self, _: &mut C) -> Result<(), BoxError> {
        Ok(std::fs::write(&self.path, &self.before)?)
    }

    fn redo(&mut self, _: &mut C) -> Result<(), BoxError> {
        Ok(std::fs::write(&self.path, &self.after)?)
    }
}

/// A model read from a file of the project.
struct Loaded {
    path: PathBuf,
    bytes: Vec<u8>,
    model: Model,
}

impl Loaded {
    /// The model at `path`.
    fn read(path: &Path) -> Result<Self, ModelError> {
        let bytes = std::fs::read(path).map_err(|error| ModelError::Read {
            path: path.to_path_buf(),
            error,
        })?;
        let model = Model::read(&bytes).map_err(|error| ModelError::Model {
            path: path.to_path_buf(),
            error,
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            bytes,
            model,
        })
    }

    /// Writes the model as changed and answers `answer` with the step that undoes it.
    fn written<T, C: 'static>(self, answer: T) -> Result<Outcome<T, C>, ModelError> {
        let after = self.model.to_bytes().map_err(ModelError::Unwritable)?;
        std::fs::write(&self.path, &after).map_err(|error| ModelError::Write {
            path: self.path.clone(),
            error,
        })?;
        Ok(Outcome::undoable(
            answer,
            ModelStep {
                path: self.path,
                before: self.bytes,
                after,
            },
        ))
    }

    /// The pixels of the single skin `skin`.
    fn skin_mut(&mut self, skin: usize) -> Result<&mut Vec<u8>, ModelError> {
        let skins = self.model.skins.len();
        match self.model.skins.get_mut(skin) {
            Some(ModelSkin::Single(pixels)) => Ok(pixels),
            Some(ModelSkin::Group { .. }) | None => Err(ModelError::NoSkin { skin, skins }),
        }
    }
}

/// Every frame of `model`, in file order, a group's frames in turn.
fn frames(model: &Model) -> impl Iterator<Item = &ModelFrame> {
    model.frames.iter().flat_map(|entry| match entry {
        ModelFrameEntry::Single(frame) => core::slice::from_ref(frame).iter(),
        ModelFrameEntry::Group { frames, .. } => frames.iter(),
    })
}

/// A frame's name as text.
fn name_of(frame: &ModelFrame) -> String {
    String::from_utf8_lossy(frame.name()).into_owned()
}

/// The game's palette for the project of `context`: the project's own, else the game's.
fn palette<C: HasWorkbench + HasMap>(context: &mut C) -> Result<Palette, ModelError> {
    let root = context.workbench().root().map(Path::to_path_buf);
    let game = context.game_folders().base;
    let bytes = light_palette(root.as_deref(), game.as_deref()).ok_or(ModelError::NoPalette)?;
    Palette::from_lmp(&bytes).map_err(|_| ModelError::NoPalette)
}

/// The parameters of a command that only names a model.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelParams {
    /// The `.mdl` file, in the project.
    pub path: PathBuf,
}

/// One frame entry of a model, as `model.inspect` answers it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FrameInfo {
    /// The frame's name, or for a group its frames' names in turn.
    pub names: Vec<String>,
}

/// What `model.inspect` answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelInspected {
    /// How many skins it has.
    pub skins: usize,
    /// A skin's width and height in pixels.
    pub skin_size: [i32; 2],
    /// How many vertices it has.
    pub vertices: usize,
    /// How many triangles.
    pub triangles: usize,
    /// Its frame entries, in the order QuakeC numbers them from 0: a `$frame` line lists a
    /// name for each, in this order.
    pub frames: Vec<FrameInfo>,
    /// The box its frames fill, lowest corner then highest, in the model's own units.
    pub bounds: [[f32; 3]; 2],
    /// Its `flags`: the trail or spin the client gives it (MDL-1.9).
    pub flags: i32,
    /// The names its frames have, for a `$frame` line: each entry's first name.
    pub frame_line: String,
}

/// `model.inspect`.
struct ModelInspect;

impl<C: 'static> Command<C> for ModelInspect {
    const ID: &'static str = MODEL_INSPECT;
    const ACCESS: Access = Access::Read;
    const SUMMARY: &'static str = "Reads a .mdl model: its skins and their size, its vertices \
        and triangles, the box it fills, and its frames by name in the order QuakeC numbers \
        them, as a $frame line lists them.";
    type Params = ModelParams;
    type Output = ModelInspected;
    type Error = ModelError;

    fn run(
        &self,
        _: &mut C,
        params: ModelParams,
    ) -> Result<Outcome<ModelInspected, C>, ModelError> {
        let loaded = Loaded::read(&params.path)?;
        let model = &loaded.model;
        let header = &model.header;
        let entries: Vec<FrameInfo> = model
            .frames
            .iter()
            .map(|entry| FrameInfo {
                names: match entry {
                    ModelFrameEntry::Single(frame) => vec![name_of(frame)],
                    ModelFrameEntry::Group { frames, .. } => frames.iter().map(name_of).collect(),
                },
            })
            .collect();
        let mut low = [u8::MAX; 3];
        let mut high = [0_u8; 3];
        for vert in frames(model).flat_map(|frame| &frame.verts) {
            for ((least, most), at) in low.iter_mut().zip(&mut high).zip(vert.v) {
                *least = (*least).min(at);
                *most = (*most).max(at);
            }
        }
        let place = |packed: [u8; 3]| {
            let mut out = [0.0_f32; 3];
            for (((at, byte), step), origin) in out
                .iter_mut()
                .zip(packed)
                .zip(header.scale)
                .zip(header.scale_origin)
            {
                *at = f32::from(byte) * step + origin;
            }
            out
        };
        let frame_line = entries
            .iter()
            .filter_map(|entry| entry.names.first().cloned())
            .collect::<Vec<_>>()
            .join(" ");
        Ok(Outcome::outside_history(ModelInspected {
            skins: model.skins.len(),
            skin_size: [header.skinwidth, header.skinheight],
            vertices: model.stverts.len(),
            triangles: model.triangles.len(),
            frames: entries,
            bounds: [place(low), place(high)],
            flags: header.flags,
            frame_line,
        }))
    }
}

/// The parameters of `model.copy_from_game`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CopyParams {
    /// The model's name in the game, such as `progs/soldier.mdl`.
    pub name: String,
    /// The `.mdl` file to make of it, in the project; one that is there already is refused.
    pub path: PathBuf,
}

/// What a command that writes a model answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ModelWritten {
    /// The model's file.
    pub path: PathBuf,
    /// How many frame entries it has now.
    pub frames: usize,
}

/// `model.copy_from_game`.
struct ModelCopy;

impl<C: HasWorkbench + HasMap + 'static> Command<C> for ModelCopy {
    const ID: &'static str = MODEL_COPY;
    const SUMMARY: &'static str = "Copies a model of the game, by its name there such as \
        progs/soldier.mdl, into the project as a file of its own to change: a mod's file of \
        the same name takes the game's place, and one of a new name is a new model.";
    type Params = CopyParams;
    type Output = ModelWritten;
    type Error = ModelError;

    fn run(
        &self,
        context: &mut C,
        params: CopyParams,
    ) -> Result<Outcome<ModelWritten, C>, ModelError> {
        if context.workbench().root().is_none() {
            return Err(ModelError::NoProject);
        }
        let missing = || ModelError::NotInGame {
            name: params.name.clone(),
        };
        let base = context.game_folders().base.ok_or_else(missing)?;
        let base = base.to_string_lossy().into_owned();
        let vfs = Vfs::new(StdFs, &["-basedir", base.as_str()], ".").map_err(|_| missing())?;
        let file = vfs.find(params.name.as_bytes()).ok_or_else(missing)?;
        let bytes = vfs.read(&file).map_err(|_| missing())?;
        let model = Mdl::read(&bytes).map_err(|error| ModelError::Model {
            path: PathBuf::from(&params.name),
            error,
        })?;
        let frames = model.frames.len();
        let wrote = |error| ModelError::Write {
            path: params.path.clone(),
            error,
        };
        if let Some(folder) = params.path.parent() {
            std::fs::create_dir_all(folder).map_err(wrote)?;
        }
        let mut made = match std::fs::File::create_new(&params.path) {
            Ok(made) => made,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(ModelError::Exists { path: params.path });
            }
            Err(error) => return Err(wrote(error)),
        };
        std::io::Write::write_all(&mut made, &bytes).map_err(wrote)?;
        Ok(Outcome::outside_history(ModelWritten {
            path: params.path,
            frames,
        }))
    }
}

/// The parameters of `model.export_skin`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportSkinParams {
    /// The `.mdl` file.
    pub path: PathBuf,
    /// The skin, from 0; 0 when not given.
    #[serde(default)]
    pub skin: usize,
    /// The PNG file to write it to.
    pub png_path: PathBuf,
}

/// What `model.export_skin` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SkinExported {
    /// The PNG written.
    pub png_path: PathBuf,
    /// Its width and height.
    pub size: [i32; 2],
}

/// `model.export_skin`.
struct ModelExportSkin;

impl<C: HasWorkbench + HasMap + 'static> Command<C> for ModelExportSkin {
    const ID: &'static str = MODEL_EXPORT_SKIN;
    const SUMMARY: &'static str = "Writes a skin of a model as a PNG, in the game's palette, \
        to look at or to paint on and give back with model.set_skin.";
    type Params = ExportSkinParams;
    type Output = SkinExported;
    type Error = ModelError;

    fn run(
        &self,
        context: &mut C,
        params: ExportSkinParams,
    ) -> Result<Outcome<SkinExported, C>, ModelError> {
        let palette = palette(context)?;
        let mut loaded = Loaded::read(&params.path)?;
        let header = loaded.model.header;
        let pixels = loaded.skin_mut(params.skin)?;
        let mut rgba = Vec::with_capacity(pixels.len().saturating_mul(RGBA));
        for &pixel in pixels.iter() {
            rgba.extend_from_slice(&palette.colour(PaletteIndex::new(pixel)));
            rgba.push(OPAQUE);
        }
        let size = |side: i32| u32::try_from(side).unwrap_or(0);
        let wrote = |error: std::io::Error| ModelError::Write {
            path: params.png_path.clone(),
            error,
        };
        if let Some(folder) = params.png_path.parent() {
            std::fs::create_dir_all(folder).map_err(wrote)?;
        }
        image::save_buffer_with_format(
            &params.png_path,
            &rgba,
            size(header.skinwidth),
            size(header.skinheight),
            image::ExtendedColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .map_err(ModelError::Image)?;
        Ok(Outcome::outside_history(SkinExported {
            png_path: params.png_path,
            size: [header.skinwidth, header.skinheight],
        }))
    }
}

/// The parameters of `model.set_skin`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSkinParams {
    /// The `.mdl` file.
    pub path: PathBuf,
    /// The skin to replace, from 0; 0 when not given.
    #[serde(default)]
    pub skin: usize,
    /// The PNG to take it from, of the skin's own width and height: each pixel becomes the
    /// nearest colour of the game's palette.
    pub png_path: PathBuf,
}

/// `model.set_skin`.
struct ModelSetSkin;

impl<C: HasWorkbench + HasMap + 'static> Command<C> for ModelSetSkin {
    const ID: &'static str = MODEL_SET_SKIN;
    const SUMMARY: &'static str = "Replaces a skin of a model with a PNG of the skin's size, \
        each pixel matched to the nearest colour of the game's palette.";
    type Params = SetSkinParams;
    type Output = ModelWritten;
    type Error = ModelError;

    fn run(
        &self,
        context: &mut C,
        params: SetSkinParams,
    ) -> Result<Outcome<ModelWritten, C>, ModelError> {
        let palette = palette(context)?;
        let png = std::fs::read(&params.png_path).map_err(|error| ModelError::Read {
            path: params.png_path.clone(),
            error,
        })?;
        let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .map_err(ModelError::Image)?
            .into_rgba8();
        let mut loaded = Loaded::read(&params.path)?;
        let header = loaded.model.header;
        let fits = i32::try_from(image.width()).ok() == Some(header.skinwidth)
            && i32::try_from(image.height()).ok() == Some(header.skinheight);
        if !fits {
            return Err(ModelError::SkinSize {
                image: [image.width(), image.height()],
                skin: [header.skinwidth, header.skinheight],
            });
        }
        let pixels = loaded.skin_mut(params.skin)?;
        let (colours, _) = image.as_raw().as_chunks::<RGBA>();
        for (pixel, [red, green, blue, _]) in pixels.iter_mut().zip(colours) {
            if let Some(nearest) = palette.nearest([*red, *green, *blue], 0..=u8::MAX) {
                *pixel = nearest.get();
            }
        }
        let frames = loaded.model.frames.len();
        let path = loaded.path.clone();
        loaded.written(ModelWritten { path, frames })
    }
}

/// The parameters of `model.recolor`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecolorParams {
    /// The `.mdl` file.
    pub path: PathBuf,
    /// The skin, from 0; 0 when not given.
    #[serde(default)]
    pub skin: usize,
    /// Rows of the palette to change, each a row to take and the row to give: the palette is
    /// 16 rows of 16 shades, and a pixel keeps its shade. `[[1, 4]]` turns the browns of row
    /// 1 into the reds of row 4.
    pub rows: Vec<[u8; 2]>,
}

/// What `model.recolor` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Recolored {
    /// The model's file.
    pub path: PathBuf,
    /// How many pixels changed colour.
    pub pixels: usize,
}

/// `model.recolor`.
struct ModelRecolor;

impl<C: 'static> Command<C> for ModelRecolor {
    const ID: &'static str = MODEL_RECOLOR;
    const SUMMARY: &'static str = "Recolours a skin of a model by rows of the palette: every \
        pixel of a row becomes the same shade of another row, as the game recolours a player's \
        shirt and trousers.";
    type Params = RecolorParams;
    type Output = Recolored;
    type Error = ModelError;

    fn run(&self, _: &mut C, params: RecolorParams) -> Result<Outcome<Recolored, C>, ModelError> {
        if let Some(&row) = params.rows.iter().flatten().find(|&&row| row >= ROW) {
            return Err(ModelError::Row { row });
        }
        let mut loaded = Loaded::read(&params.path)?;
        let pixels = loaded.skin_mut(params.skin)?;
        let mut changed = 0_usize;
        for pixel in pixels.iter_mut() {
            // A colour is its row in its high four bits and its shade in its low four.
            let (row, shade) = (*pixel >> ROW_SHIFT, *pixel & SHADE_MASK);
            let Some(&[_, to]) = params.rows.iter().find(|[from, _]| *from == row) else {
                continue;
            };
            // A row from 128 on runs from bright to dark: between the two kinds a shade is
            // counted from the other end (REND-4.13).
            let backwards = |row: u8| row.saturating_mul(ROW) >= BACKWARDS_FROM;
            let shade = if backwards(row) == backwards(to) {
                shade
            } else {
                ROW.saturating_sub(1).saturating_sub(shade)
            };
            let colour = to.saturating_mul(ROW).saturating_add(shade);
            if colour != *pixel {
                *pixel = colour;
                changed = changed.saturating_add(1);
            }
        }
        let path = loaded.path.clone();
        loaded.written(Recolored {
            path,
            pixels: changed,
        })
    }
}

/// The parameters of `model.scale` and `model.move`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransformParams {
    /// The `.mdl` file.
    pub path: PathBuf,
    /// For `model.scale`, how many times larger on x, y and z, each above 0; for
    /// `model.move`, how far along each, in the model's units.
    pub by: [f32; 3],
}

/// What `model.scale` and `model.move` answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Transformed {
    /// The model's file.
    pub path: PathBuf,
    /// The box its frames fill now, lowest corner then highest.
    pub bounds: [[f32; 3]; 2],
}

/// The box `model`'s packed vertices can reach: its origin, and 255 steps from it.
fn reach(model: &Model) -> [[f32; 3]; 2] {
    let header = &model.header;
    let mut far = header.scale_origin;
    for (at, step) in far.iter_mut().zip(header.scale) {
        *at += step * f32::from(u8::MAX);
    }
    [header.scale_origin, far]
}

/// `model.scale`.
struct ModelScale;

impl<C: 'static> Command<C> for ModelScale {
    const ID: &'static str = MODEL_SCALE;
    const SUMMARY: &'static str = "Makes a model larger or smaller about its origin, on each \
        axis: its vertices keep their packed places and the header's step and origin are \
        scaled, so nothing is rounded again. The box QuakeC gives the entity (setsize) is its \
        own to change.";
    type Params = TransformParams;
    type Output = Transformed;
    type Error = ModelError;

    fn run(
        &self,
        _: &mut C,
        params: TransformParams,
    ) -> Result<Outcome<Transformed, C>, ModelError> {
        if !params.by.iter().all(|by| by.is_finite() && *by > 0.0) {
            return Err(ModelError::Scale);
        }
        let mut loaded = Loaded::read(&params.path)?;
        let header = &mut loaded.model.header;
        let mut largest = 0.0_f32;
        for ((((step, origin), eye), by), _) in header
            .scale
            .iter_mut()
            .zip(&mut header.scale_origin)
            .zip(&mut header.eye_position)
            .zip(params.by)
            .zip(0..)
        {
            *step *= by;
            *origin *= by;
            *eye *= by;
            largest = largest.max(by);
        }
        header.bounding_radius *= largest;
        let bounds = reach(&loaded.model);
        let path = loaded.path.clone();
        loaded.written(Transformed { path, bounds })
    }
}

/// `model.move`.
struct ModelMove;

impl<C: 'static> Command<C> for ModelMove {
    const ID: &'static str = MODEL_MOVE;
    const SUMMARY: &'static str = "Moves a model against its origin, the point the entity's \
        origin is at: its vertices keep their packed places and the header's origin moves.";
    type Params = TransformParams;
    type Output = Transformed;
    type Error = ModelError;

    fn run(
        &self,
        _: &mut C,
        params: TransformParams,
    ) -> Result<Outcome<Transformed, C>, ModelError> {
        if !params.by.iter().all(|by| by.is_finite()) {
            return Err(ModelError::Scale);
        }
        let mut loaded = Loaded::read(&params.path)?;
        for (origin, by) in loaded.model.header.scale_origin.iter_mut().zip(params.by) {
            *origin += by;
        }
        let bounds = reach(&loaded.model);
        let path = loaded.path.clone();
        loaded.written(Transformed { path, bounds })
    }
}

/// The parameters of `model.rename_frames`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenameFramesParams {
    /// The `.mdl` file.
    pub path: PathBuf,
    /// Each a frame's name and the name to give it, at most 15 bytes.
    pub names: Vec<[String; 2]>,
}

/// `model.rename_frames`.
struct ModelRenameFrames;

impl<C: 'static> Command<C> for ModelRenameFrames {
    const ID: &'static str = MODEL_RENAME_FRAMES;
    const SUMMARY: &'static str = "Renames frames of a model, each by its name. The game finds \
        a frame by its number, so QuakeC's $frame names are its own: keep them in step.";
    type Params = RenameFramesParams;
    type Output = ModelWritten;
    type Error = ModelError;

    fn run(
        &self,
        _: &mut C,
        params: RenameFramesParams,
    ) -> Result<Outcome<ModelWritten, C>, ModelError> {
        let mut loaded = Loaded::read(&params.path)?;
        for [from, to] in &params.names {
            let mut found = false;
            for entry in &mut loaded.model.frames {
                let each: &mut [ModelFrame] = match entry {
                    ModelFrameEntry::Single(frame) => core::slice::from_mut(frame),
                    ModelFrameEntry::Group { frames, .. } => frames,
                };
                for frame in each
                    .iter_mut()
                    .filter(|frame| frame.name() == from.as_bytes())
                {
                    frame
                        .set_name(to.as_bytes())
                        .map_err(ModelError::Unwritable)?;
                    found = true;
                }
            }
            if !found {
                return Err(ModelError::NoFrame { name: from.clone() });
            }
        }
        let frames = loaded.model.frames.len();
        let path = loaded.path.clone();
        loaded.written(ModelWritten { path, frames })
    }
}

/// The parameters of `model.remove_frames`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveFramesParams {
    /// The `.mdl` file.
    pub path: PathBuf,
    /// The frames to take out, each by its name; a group goes with its first frame's name.
    pub frames: Vec<String>,
}

/// `model.remove_frames`.
struct ModelRemoveFrames;

impl<C: 'static> Command<C> for ModelRemoveFrames {
    const ID: &'static str = MODEL_REMOVE_FRAMES;
    const SUMMARY: &'static str = "Takes frames out of a model, each by its name. The frames \
        after one taken out are numbered one less, so QuakeC's $frame list loses the same \
        names.";
    type Params = RemoveFramesParams;
    type Output = ModelWritten;
    type Error = ModelError;

    fn run(
        &self,
        _: &mut C,
        params: RemoveFramesParams,
    ) -> Result<Outcome<ModelWritten, C>, ModelError> {
        let mut loaded = Loaded::read(&params.path)?;
        let first = |entry: &ModelFrameEntry| match entry {
            ModelFrameEntry::Single(frame) => Some(name_of(frame)),
            ModelFrameEntry::Group { frames, .. } => frames.first().map(name_of),
        };
        for name in &params.frames {
            let place = loaded
                .model
                .frames
                .iter()
                .position(|entry| first(entry).as_ref() == Some(name))
                .ok_or_else(|| ModelError::NoFrame { name: name.clone() })?;
            loaded.model.frames.remove(place);
        }
        let frames = loaded.model.frames.len();
        let path = loaded.path.clone();
        loaded.written(ModelWritten { path, frames })
    }
}

/// The parameters of `model.add_frames`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddFramesParams {
    /// The `.mdl` file to add frames to.
    pub path: PathBuf,
    /// The `.mdl` file the frames come from, a model of the same vertices: another copy of
    /// the model with other poses.
    pub from_path: PathBuf,
    /// The frames to add, each by its name there, in this order, after the model's own.
    pub frames: Vec<String>,
}

/// `model.add_frames`.
struct ModelAddFrames;

impl<C: 'static> Command<C> for ModelAddFrames {
    const ID: &'static str = MODEL_ADD_FRAMES;
    const SUMMARY: &'static str = "Adds frames of another model of the same vertices to the \
        end of a model's own, each by its name: QuakeC's $frame list gains the same names at \
        its end.";
    type Params = AddFramesParams;
    type Output = ModelWritten;
    type Error = ModelError;

    fn run(
        &self,
        _: &mut C,
        params: AddFramesParams,
    ) -> Result<Outcome<ModelWritten, C>, ModelError> {
        let mut loaded = Loaded::read(&params.path)?;
        let other = Loaded::read(&params.from_path)?;
        let (ours, theirs) = (loaded.model.stverts.len(), other.model.stverts.len());
        if ours != theirs {
            return Err(ModelError::Vertices {
                model: ours,
                other: theirs,
            });
        }
        let (to, from) = (loaded.model.header, other.model.header);
        for name in &params.frames {
            let frame = frames(&other.model)
                .find(|frame| frame.name() == name.as_bytes())
                .ok_or_else(|| ModelError::NoFrame { name: name.clone() })?;
            // Each vertex goes from the other model's box into this one's: where it is, then
            // the byte of this model's step nearest it (MDL-5.3).
            let mut verts = Vec::with_capacity(frame.verts.len());
            for vert in &frame.verts {
                let mut v = [0_u8; 3];
                for ((((byte, packed), (step, origin)), to_step), to_origin) in v
                    .iter_mut()
                    .zip(vert.v)
                    .zip(from.scale.into_iter().zip(from.scale_origin))
                    .zip(to.scale)
                    .zip(to.scale_origin)
                {
                    let at = f32::from(packed) * step + origin;
                    let steps = ((at - to_origin) / to_step).round();
                    if !(0.0..=f32::from(u8::MAX)).contains(&steps) {
                        return Err(ModelError::OutsideBox { name: name.clone() });
                    }
                    *byte = whole_byte(steps);
                }
                verts.push(TriVertX {
                    v,
                    lightnormalindex: vert.lightnormalindex,
                });
            }
            let (mut low, mut high) = ([u8::MAX; 3], [0_u8; 3]);
            for vert in &verts {
                for ((least, most), at) in low.iter_mut().zip(&mut high).zip(vert.v) {
                    *least = (*least).min(at);
                    *most = (*most).max(at);
                }
            }
            loaded
                .model
                .frames
                .push(ModelFrameEntry::Single(ModelFrame {
                    bboxmin: TriVertX {
                        v: low,
                        lightnormalindex: 0,
                    },
                    bboxmax: TriVertX {
                        v: high,
                        lightnormalindex: 0,
                    },
                    name: frame.name,
                    verts,
                }));
        }
        let frames = loaded.model.frames.len();
        let path = loaded.path.clone();
        loaded.written(ModelWritten { path, frames })
    }
}

/// A whole number from 0 to 255, held in a float, as a byte.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value was checked to be a whole number from 0 to 255"
)]
const fn whole_byte(steps: f32) -> u8 {
    steps as u8
}

/// Registers the `model.*` commands.
///
/// # Errors
///
/// [`RegisterError`] when one of their ids is taken.
pub(crate) fn register_models<C: HasWorkbench + HasMap + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(ModelInspect)?;
    registry.register(ModelCopy)?;
    registry.register(ModelExportSkin)?;
    registry.register(ModelSetSkin)?;
    registry.register(ModelRecolor)?;
    registry.register(ModelScale)?;
    registry.register(ModelMove)?;
    registry.register(ModelRenameFrames)?;
    registry.register(ModelRemoveFrames)?;
    registry.register(ModelAddFrames)
}

#[cfg(test)]
mod tests;
