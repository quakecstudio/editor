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
use qs_mdl::md5::{Md5Anim, Md5Error, Md5Mesh};
use qs_mdl::mdl::write::{MdlWriteError, Model, ModelFrame, ModelFrameEntry, ModelSkin};
use qs_mdl::mdl::{Mdl, MdlError, TriVertX};
use qs_mdl::mesh::fbx::FbxError;
use qs_mdl::mesh::iqe::IqeError;
use qs_mdl::mesh::md5::Md5MeshError;
use qs_mdl::mesh::obj::{ObjError, Up};
use qs_mdl::mesh::smd::{Smd, SmdError};
use qs_mdl::mesh::{Mesh, MeshError, MeshSkin, fbx, iqe, obj};
use qs_vfs::{StdFs, Vfs};
use qs_wad::texture::{Palette, PaletteIndex, WAD_FULLBRIGHT_FIRST};
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
/// The id of `model.import`.
pub const MODEL_IMPORT: &str = "model.import";

/// The colours of a row of the palette, and its rows: a skin's colours come in rows of 16
/// shades (REND-4.13).
const ROW: u8 = 16;
/// A colour's row is its high four bits, its shade its low four.
const ROW_SHIFT: u32 = 4;
const SHADE_MASK: u8 = 0x0F;
/// The first colour of the rows whose shades run from bright to dark, "backwards ranges"
/// (REND-4.13).
const BACKWARDS_FROM: u8 = 128;
/// The last of the palette's ordinary colours; those after it show at full brightness in
/// the dark (WAD-7.4).
const ORDINARY_LAST: u8 = WAD_FULLBRIGHT_FIRST - 1;
/// The axes a vertex has.
const AXES: usize = 3;
/// The width and height of the skin an imported mesh with texture coordinates is given
/// when the caller gives neither a picture nor a size.
const IMPORT_SKIN_SIZE: [i32; 2] = [128, 128];
/// The colour of the palette an imported model's skin is filled with when the caller gives
/// neither a picture nor a colour: one of the palette's first row.
const IMPORT_SKIN_COLOUR: u8 = 8;
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
    /// A move that is no number.
    Move,
    /// The file to import is no kind of mesh that is read.
    Format {
        /// The file.
        path: PathBuf,
    },
    /// The `.obj` file to import could not be read as one.
    Obj {
        /// The file.
        path: PathBuf,
        /// Why.
        error: ObjError,
    },
    /// The `.iqe` file to import could not be read as one.
    Iqe {
        /// The file.
        path: PathBuf,
        /// Why.
        error: IqeError,
    },
    /// The `.md5mesh` or `.md5anim` file to import could not be read as one.
    Md5 {
        /// The file.
        path: PathBuf,
        /// Why.
        error: Md5Error,
    },
    /// The MD5 model's animations do not pose its mesh.
    Md5Mesh(Md5MeshError),
    /// The `.smd` file to import could not be read as one, or is no model.
    Smd {
        /// The file.
        path: PathBuf,
        /// Why.
        error: SmdError,
    },
    /// The `.fbx` file to import could not be read as one.
    Fbx {
        /// The file.
        path: PathBuf,
        /// Why.
        error: FbxError,
    },
    /// Animation files were given for a mesh that takes none: one whose file holds its own
    /// animations, or has none.
    Animations {
        /// The mesh's file.
        path: PathBuf,
    },
    /// The mesh read is no model.
    Mesh(MeshError),
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
            Self::Move => f.write_str("a move is a number on each axis"),
            Self::Format { path } => write!(
                f,
                "{}: is no .obj, .iqe, .md5mesh, .smd or .fbx file, the meshes that are read",
                path.display()
            ),
            Self::Obj { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Iqe { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Md5 { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Md5Mesh(error) => write!(f, "{error}"),
            Self::Smd { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Fbx { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Animations { path } => write!(
                f,
                "{}: takes no animation files: they go with an .md5mesh or an .smd model",
                path.display()
            ),
            Self::Mesh(error) => write!(f, "{error}"),
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
            Self::Obj { error, .. } => Some(error),
            Self::Iqe { error, .. } => Some(error),
            Self::Md5 { error, .. } => Some(error),
            Self::Md5Mesh(error) => Some(error),
            Self::Smd { error, .. } => Some(error),
            Self::Fbx { error, .. } => Some(error),
            Self::Mesh(error) => Some(error),
            Self::NoProject
            | Self::NoSkin { .. }
            | Self::NoFrame { .. }
            | Self::SkinSize { .. }
            | Self::NoPalette
            | Self::Row { .. }
            | Self::NotInGame { .. }
            | Self::Exists { .. }
            | Self::Scale
            | Self::Move
            | Self::Format { .. }
            | Self::Animations { .. }
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
        let bytes = qs_paths::read_regular_file(path).map_err(|error| ModelError::Read {
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

/// How much of a skin is of one row of the palette, as `model.inspect` answers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RowUse {
    /// The row of the palette, 0 to 15.
    pub row: u8,
    /// How many of the skin's pixels are of it.
    pub pixels: usize,
}

/// The rows of the palette the pixels of `skin` are of, most pixels first.
fn rows_used(skin: &ModelSkin) -> Vec<RowUse> {
    let mut counts = [0_usize; ROW as usize];
    let pixels: Vec<&u8> = match skin {
        ModelSkin::Single(pixels) => pixels.iter().collect(),
        ModelSkin::Group { skins, .. } => skins.iter().flatten().collect(),
    };
    for pixel in pixels {
        if let Some(count) = counts.get_mut(usize::from(pixel >> ROW_SHIFT)) {
            *count = count.saturating_add(1);
        }
    }
    let mut used: Vec<RowUse> = (0..ROW)
        .zip(counts)
        .filter(|&(_, pixels)| pixels > 0)
        .map(|(row, pixels)| RowUse { row, pixels })
        .collect();
    used.sort_by_key(|row| core::cmp::Reverse(row.pixels));
    used
}

/// What `model.inspect` answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelInspected {
    /// How many skins it has.
    pub skins: usize,
    /// For each skin, the rows of the palette its pixels are of, most pixels first: what
    /// `model.recolor` takes as the rows to change.
    pub skin_rows: Vec<Vec<RowUse>>,
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
    const SUMMARY: &'static str = "Reads a .mdl model: its skins, their size and the rows \
        of the palette each is of, its vertices and triangles, the box it fills, and its frames by name in the order QuakeC numbers \
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
        let mut low = [u8::MAX; AXES];
        let mut high = [0_u8; AXES];
        for vert in frames(model).flat_map(|frame| &frame.verts) {
            for ((least, most), at) in low.iter_mut().zip(&mut high).zip(vert.v) {
                *least = (*least).min(at);
                *most = (*most).max(at);
            }
        }
        let place = |packed: [u8; 3]| {
            let mut out = [0.0_f32; AXES];
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
            skin_rows: model.skins.iter().map(rows_used).collect(),
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
        .map_err(|error| match error {
            image::ImageError::IoError(error) => wrote(error),
            other => ModelError::Image(other),
        })?;
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
    /// The PNG to take it from, of the skin's own width and height: each pixel that changed
    /// becomes the nearest of the palette's colours 0 to 223, or a fullbright colour (224 to
    /// 255, which glow in the dark) when it is exactly that colour.
    pub png_path: PathBuf,
}

/// The palette index a pixel of a skin takes from `colour`, a pixel of a PNG. The index it
/// has when that is its colour already, so a skin given back as it went out is unchanged; else
/// the nearest of the ordinary colours (WAD-7.4), so a painted colour does not glow in the
/// dark; but a colour that is exactly a fullbright one and no ordinary one is that one, which
/// is how a part is painted to glow.
fn skin_index(palette: &Palette, now: u8, colour: [u8; 3]) -> u8 {
    if palette.colour(PaletteIndex::new(now)) == colour {
        return now;
    }
    let exact =
        |found: Option<PaletteIndex>| found.filter(|index| palette.colour(*index) == colour);
    let ordinary = palette.nearest(colour, 0..=ORDINARY_LAST);
    exact(ordinary)
        .or_else(|| exact(palette.nearest(colour, WAD_FULLBRIGHT_FIRST..=u8::MAX)))
        .or(ordinary)
        .map_or(now, PaletteIndex::get)
}

/// `model.set_skin`.
struct ModelSetSkin;

impl<C: HasWorkbench + HasMap + 'static> Command<C> for ModelSetSkin {
    const ID: &'static str = MODEL_SET_SKIN;
    const SUMMARY: &'static str = "Replaces a skin of a model with a PNG of the skin's size, \
        each changed pixel matched to the nearest of the palette's colours that do not glow \
        in the dark, or to a fullbright colour when it is exactly that colour.";
    type Params = SetSkinParams;
    type Output = ModelWritten;
    type Error = ModelError;

    fn run(
        &self,
        context: &mut C,
        params: SetSkinParams,
    ) -> Result<Outcome<ModelWritten, C>, ModelError> {
        let palette = palette(context)?;
        let png =
            qs_paths::read_regular_file(&params.png_path).map_err(|error| ModelError::Read {
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
            *pixel = skin_index(&palette, *pixel, [*red, *green, *blue]);
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
            return Err(ModelError::Move);
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
    /// Each a frame's name, as the file has it, and the name to give it, at most 15 bytes.
    /// All are renamed at once, so two frames can take each other's names.
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
        // Each pair names a frame as the file has it, so two frames can change names with
        // each other: no pair renames what another pair named.
        if let Some([from, _]) = params
            .names
            .iter()
            .find(|[from, _]| !frames(&loaded.model).any(|frame| frame.name() == from.as_bytes()))
        {
            return Err(ModelError::NoFrame { name: from.clone() });
        }
        for entry in &mut loaded.model.frames {
            let each: &mut [ModelFrame] = match entry {
                ModelFrameEntry::Single(frame) => core::slice::from_mut(frame),
                ModelFrameEntry::Group { frames, .. } => frames,
            };
            for frame in each {
                let renamed = params
                    .names
                    .iter()
                    .find(|[from, _]| frame.name() == from.as_bytes());
                if let Some([_, to]) = renamed {
                    frame
                        .set_name(to.as_bytes())
                        .map_err(ModelError::Unwritable)?;
                }
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
                let mut v = [0_u8; AXES];
                for ((((byte, packed), (step, origin)), to_step), to_origin) in v
                    .iter_mut()
                    .zip(vert.v)
                    .zip(from.scale.into_iter().zip(from.scale_origin))
                    .zip(to.scale)
                    .zip(to.scale_origin)
                {
                    let at = f32::from(packed) * step + origin;
                    // A model flat on an axis has no step there (MDL-7.2): only a vertex in
                    // its plane fits, at byte 0.
                    let steps = if to_step.abs() > 0.0 {
                        ((at - to_origin) / to_step).round()
                    } else if (at - to_origin).abs() > 0.0 {
                        f32::NAN
                    } else {
                        0.0
                    };
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
            let (mut low, mut high) = ([u8::MAX; AXES], [0_u8; AXES]);
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

/// Which axis of a mesh's file points up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum UpAxis {
    /// y is up and -z forward, as most programs write an `.obj`.
    Y,
    /// z is up and x forward, as Quake has them.
    Z,
}

/// How an imported model's vertices lie on its skin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SkinLayout {
    /// By the mesh's own texture coordinates.
    Own,
    /// Laid out here: the model's front on the skin's left half and its back on the right.
    FrontAndBack,
}

/// The parameters of `model.import`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportParams {
    /// The mesh to read: an `.obj` file, one pose; an `.iqe` or an `.fbx` file, a pose
    /// for each frame of each of its animations; or an `.md5mesh` or an `.smd` model, posed
    /// by the files of `animation_paths`.
    pub source_path: PathBuf,
    /// The animations of an `.md5mesh`, its `.md5anim` files, or of an `.smd` model, its
    /// animation `.smd` files: a pose for each frame of each, named by the file and the
    /// frame's number. An `.md5mesh` given none takes the `.md5anim` of its own name beside
    /// it, when there is one.
    #[serde(default)]
    pub animation_paths: Vec<PathBuf>,
    /// The `.mdl` file to make of it, in the project; one that is there already is refused.
    pub path: PathBuf,
    /// Which axis of the file is up: y for an `.obj` and an `.fbx`, and z for the others,
    /// when not given. A y-up file is turned so that its -z is forward, Quake's +x, which
    /// is how programs write a model that faces its viewer's way.
    #[serde(default)]
    pub up: Option<UpAxis>,
    /// What every position is multiplied by, to make the model the size Quake's are (a
    /// player is 56 units high); 1 when not given.
    #[serde(default)]
    pub scale: Option<f32>,
    /// A PNG for the skin of a mesh that has texture coordinates, each pixel matched to the
    /// palette as `model.set_skin` matches one. Its width is a multiple of 4 and its height
    /// at most 480.
    #[serde(default)]
    pub png_path: Option<PathBuf>,
    /// The skin's width and height for a mesh with texture coordinates and no PNG; 128 by
    /// 128 when not given. A mesh without them has a skin of the size its layout needs.
    #[serde(default)]
    pub skin_size: Option<[i32; 2]>,
    /// The colour of the palette the skin is filled with when no PNG is given; 8 when not
    /// given.
    #[serde(default)]
    pub colour: Option<u8>,
}

impl ImportParams {
    /// The mesh of the file to import, read as its ending says, with its animation files.
    fn mesh(&self) -> Result<Mesh, ModelError> {
        let source = &self.source_path;
        let ending = source
            .extension()
            .map(|ending| ending.to_string_lossy().to_ascii_lowercase());
        let text = qs_paths::read_regular_file(source).map_err(|error| ModelError::Read {
            path: source.clone(),
            error,
        })?;
        let up = |or: Up| match self.up {
            Some(UpAxis::Y) => Up::Y,
            Some(UpAxis::Z) => Up::Z,
            None => or,
        };
        let bytes = text;
        let text = String::from_utf8_lossy(&bytes);
        // Each animation file with the name its frames take: the file's, without its ending.
        let animations =
            |paths: &[PathBuf]| -> Result<Vec<(String, Vec<u8>, PathBuf)>, ModelError> {
                paths
                    .iter()
                    .map(|path| {
                        let name = path
                            .file_stem()
                            .map_or_default(|stem| stem.to_string_lossy().into_owned());
                        let bytes = qs_paths::read_regular_file(path).map_err(|error| {
                            ModelError::Read {
                                path: path.clone(),
                                error,
                            }
                        })?;
                        Ok((name, bytes, path.clone()))
                    })
                    .collect()
            };
        // Only a model whose animations are files of their own takes animation files; what
        // is no mesh at all is said to be that first.
        let alone = || {
            if self.animation_paths.is_empty() {
                Ok(())
            } else {
                Err(ModelError::Animations {
                    path: source.clone(),
                })
            }
        };
        let mesh: Mesh = match ending.as_deref() {
            Some("obj") => {
                alone()?;
                obj::read(&text, up(Up::Y)).map_err(|error| ModelError::Obj {
                    path: source.clone(),
                    error,
                })?
            }
            Some("iqe") => {
                alone()?;
                iqe::read(&text, up(Up::Z)).map_err(|error| ModelError::Iqe {
                    path: source.clone(),
                    error,
                })?
            }
            Some("fbx") => {
                alone()?;
                fbx::read_bytes(&bytes, up(Up::Y)).map_err(|error| ModelError::Fbx {
                    path: source.clone(),
                    error,
                })?
            }
            Some("md5mesh") => {
                let model = Md5Mesh::read(&bytes).map_err(|error| ModelError::Md5 {
                    path: source.clone(),
                    error,
                })?;
                // The animation of the mesh's own name (MD5-3.1), when none is given.
                let own = source.with_extension("md5anim");
                let paths = if self.animation_paths.is_empty() && own.is_file() {
                    vec![own]
                } else {
                    self.animation_paths.clone()
                };
                let read = animations(&paths)?
                    .into_iter()
                    .map(|(name, bytes, path)| {
                        Md5Anim::read(&bytes)
                            .map(|animation| (name, animation))
                            .map_err(|error| ModelError::Md5 { path, error })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                model.mesh(&read, up(Up::Z)).map_err(ModelError::Md5Mesh)?
            }
            Some("smd") => {
                let smd = |path: &Path, bytes: &[u8]| {
                    Smd::read(&String::from_utf8_lossy(bytes)).map_err(|error| ModelError::Smd {
                        path: path.to_path_buf(),
                        error,
                    })
                };
                let model = smd(source, &bytes)?;
                let read = animations(&self.animation_paths)?
                    .into_iter()
                    .map(|(name, bytes, path)| Ok((name, smd(&path, &bytes)?)))
                    .collect::<Result<Vec<_>, ModelError>>()?;
                model
                    .mesh(&read, up(Up::Z))
                    .map_err(|error| ModelError::Smd {
                        path: source.clone(),
                        error,
                    })?
            }
            Some(_) | None => {
                return Err(ModelError::Format {
                    path: source.clone(),
                });
            }
        };
        Ok(mesh)
    }
}

/// What `model.import` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ModelImported {
    /// The model's file.
    pub path: PathBuf,
    /// How many vertices it has.
    pub vertices: usize,
    /// How many triangles.
    pub triangles: usize,
    /// Its skin's width and height.
    pub skin_size: [i32; 2],
    /// How its vertices lie on the skin.
    pub layout: SkinLayout,
    /// The names of its frames, as QuakeC's `$frame` line lists them.
    pub frame_line: String,
}

/// `model.import`.
struct ModelImport;

impl<C: HasWorkbench + HasMap + 'static> Command<C> for ModelImport {
    const ID: &'static str = MODEL_IMPORT;
    const SUMMARY: &'static str = "Makes a .mdl model in the project of a mesh in an .obj \
        file (one pose), or an .iqe, .fbx, .md5mesh or .smd file (a frame for each frame \
        of each animation, posed by its skeleton). A mesh with texture coordinates keeps them, on a PNG given as its skin \
        or one colour; one without is laid out front and back, to export, paint and set.";
    type Params = ImportParams;
    type Output = ModelImported;
    type Error = ModelError;

    fn run(
        &self,
        context: &mut C,
        params: ImportParams,
    ) -> Result<Outcome<ModelImported, C>, ModelError> {
        if context.workbench().root().is_none() {
            return Err(ModelError::NoProject);
        }
        if params
            .scale
            .is_some_and(|scale| !(scale.is_finite() && scale > 0.0))
        {
            return Err(ModelError::Scale);
        }
        let mut mesh = params.mesh()?;
        if let Some(scale) = params.scale {
            for position in mesh
                .frames
                .iter_mut()
                .flat_map(|frame| &mut frame.positions)
            {
                *position = position.map(|part| part * scale);
            }
        }
        let colour = params.colour.unwrap_or(IMPORT_SKIN_COLOUR);
        let skin = match &params.png_path {
            Some(png_path) => {
                let palette = palette(context)?;
                let png = std::fs::read(png_path).map_err(|error| ModelError::Read {
                    path: png_path.clone(),
                    error,
                })?;
                let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
                    .map_err(ModelError::Image)?
                    .into_rgba8();
                let side = |pixels: u32| i32::try_from(pixels).unwrap_or(i32::MAX);
                let (colours, _) = image.as_raw().as_chunks::<RGBA>();
                MeshSkin::Picture {
                    size: [side(image.width()), side(image.height())],
                    pixels: colours
                        .iter()
                        .map(|[red, green, blue, _]| {
                            skin_index(&palette, colour, [*red, *green, *blue])
                        })
                        .collect(),
                }
            }
            None => MeshSkin::Flat {
                colour,
                size: params.skin_size.unwrap_or(IMPORT_SKIN_SIZE),
            },
        };
        let layout = if mesh.has_texcoords() {
            SkinLayout::Own
        } else {
            SkinLayout::FrontAndBack
        };
        let model = mesh.model(&skin).map_err(ModelError::Mesh)?;
        let bytes = model.to_bytes().map_err(ModelError::Unwritable)?;
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
        Ok(Outcome::outside_history(ModelImported {
            path: params.path,
            vertices: model.stverts.len(),
            triangles: model.triangles.len(),
            skin_size: [model.header.skinwidth, model.header.skinheight],
            layout,
            frame_line: frames(&model).map(name_of).collect::<Vec<_>>().join(" "),
        }))
    }
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
    registry.register(ModelAddFrames)?;
    registry.register(ModelImport)
}

#[cfg(test)]
mod tests;
