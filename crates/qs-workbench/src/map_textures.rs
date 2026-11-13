// SPDX-License-Identifier: GPL-3.0-or-later

//! The textures of the open map, for the texture browser (P6-10, MAP-5): every texture of the
//! WADs its world names, found in the project, beside the map or in the game, and of the BSP
//! it was opened from; each with how many faces use it; decoded through the game's palette only
//! when asked. Each WAD and BSP is read once, when the sources are made.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use qs_bsp::{Bsp, BspLoader};
use qs_commands::{Command, NoParams, Outcome, Registry};
use qs_map::Compiler;
use qs_vfs::{FS_GAMENAME, Location, StdFs, Vfs};
use qs_wad::texture::{DecodedTexture, Palette};
use qs_wad::{MapWad, WAD_TYP_MIPTEX, WAD_TYP_MIPTEX_PALETTE};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::map_edit::{MapDocument, MapSource};
use crate::{HasWorkbench, WorkbenchError};

/// The id of `map.textures`.
pub const MAP_TEXTURES: &str = "map.textures";

/// The game's palette, `gfx/palette.lmp` (REND-1.8).
const PALETTE_LMP: &str = "gfx/palette.lmp";

/// Where a texture of the browser comes from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TextureOrigin {
    /// A WAD the world names, by the name the world gives it.
    Wad {
        /// The WAD's name as the world writes it.
        wad: String,
    },
    /// The BSP the map was opened from.
    Bsp,
    /// No WAD or BSP holds it: a name a face uses that nothing found.
    Missing,
}

/// One texture of the browser.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BrowserTexture {
    /// Its name.
    pub name: String,
    /// Where it comes from.
    pub origin: TextureOrigin,
    /// Its width in pixels; 0 when missing.
    pub width: u32,
    /// Its height in pixels; 0 when missing.
    pub height: u32,
    /// How many of the map's faces use it.
    pub uses: usize,
}

/// What became of a WAD the world names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum WadState {
    /// No file of that name was found.
    Missing,
    /// A file was found, but it is no WAD2 or WAD3 QuakeSpasm reads (WAD3-1).
    Unreadable,
    /// It was read.
    Read {
        /// Its textures.
        textures: usize,
    },
}

/// A WAD the world names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WadFound {
    /// Its name as the world writes it.
    pub name: String,
    /// The file it was read from, when it is a file of its own and not in a PAK; the one to
    /// watch for another program's writes.
    pub path: Option<PathBuf>,
    /// What became of it.
    pub state: WadState,
}

/// What `map.textures` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MapTextures {
    /// The WADs the world names, in its order.
    pub wads: Vec<WadFound>,
    /// Every texture: the WADs' in their order, then the BSP's, then the names no WAD holds,
    /// each name once.
    pub textures: Vec<BrowserTexture>,
}

/// One texture the sources hold, read when they were made.
#[derive(Clone, Debug)]
struct Held {
    name: String,
    origin: TextureOrigin,
    width: u32,
    height: u32,
    /// Its first level's palette indices, as many as the file gives.
    pixels: Vec<u8>,
    /// The palette a WAD3 texture carries (WAD-8.4).
    palette: Option<Palette>,
}

/// A WAD's name as the world writes it, and its file, when one was found: its host path, when
/// it is a file of its own, and its bytes.
type WadRead = (String, Option<(Option<PathBuf>, Vec<u8>)>);

/// The files the open map's textures come from: its WADs, its BSP and the palette, each read
/// once.
#[derive(Clone, Debug, Default)]
pub struct TextureSources {
    wads: Vec<WadFound>,
    held: Vec<Held>,
    /// Each held texture by its name in lower case.
    by_name: HashMap<String, usize>,
    palette: Option<Palette>,
    /// The WAD paths and the game folder the sources were made for.
    made_for: (Vec<Vec<u8>>, Option<PathBuf>),
}

/// The palette textures are shown and made in: the project's own `gfx/palette.lmp`, first on
/// the engine's search path as a mod's is, else the game's; none when neither reads.
pub(crate) fn game_palette(project: Option<&Path>, game: Option<&Game<'_>>) -> Option<Palette> {
    project
        .and_then(|root| std::fs::read(root.join(PALETTE_LMP)).ok())
        .or_else(|| {
            game.and_then(|game| game.file(PALETTE_LMP))
                .map(|(_, bytes)| bytes)
        })
        .and_then(|bytes| Palette::from_lmp(&bytes).ok())
}

/// The game at a base directory: its search path, when it can be built, and the directory a
/// loose file is looked for in.
pub(crate) struct Game<'a> {
    base: &'a Path,
    vfs: Option<Vfs<StdFs>>,
}

impl<'a> Game<'a> {
    pub(crate) fn at(base: &'a Path) -> Self {
        let args = base.to_string_lossy().into_owned();
        let vfs = Vfs::new(StdFs, &["-basedir", args.as_str()], ".").ok();
        Self { base, vfs }
    }

    /// The game directory a loose file is read from: the search path's, else `id1` when the
    /// search path cannot be built.
    fn dir(&self) -> PathBuf {
        self.vfs.as_ref().map_or_else(
            || self.base.join(FS_GAMENAME),
            |vfs| PathBuf::from(vfs.game_dir()),
        )
    }

    /// The file `name`: on the search path, else loose in the game directory; with its host
    /// path when it is a file of its own. A shareware search path finds no loose file in a
    /// directory (FS-3.4), which a map's WAD or the palette may be, so the game directory is
    /// tried too.
    fn file(&self, name: &str) -> Option<(Option<PathBuf>, Vec<u8>)> {
        if let Some(vfs) = &self.vfs
            && let Some(file) = vfs.find(name.as_bytes())
            && let Ok(bytes) = vfs.read(&file)
        {
            let host = match file.location() {
                Location::Loose(path) => Some(path.clone()),
                Location::Entry(_) => None,
            };
            return Some((host, bytes));
        }
        let path = self.dir().join(name);
        std::fs::read(&path).ok().map(|bytes| (Some(path), bytes))
    }
}

/// The WAD `name` names: as written when it is absolute, then, by the name as written or for
/// an absolute path written on another machine by its file name alone, under the project,
/// beside the map, under the game's base directory, then on the game's search path or loose in
/// its game directory.
pub(crate) fn find_wad(
    name: &str,
    project: Option<&Path>,
    map_dir: Option<&Path>,
    game: Option<&Game<'_>>,
) -> Option<(Option<PathBuf>, Vec<u8>)> {
    let path = Path::new(name);
    if path.is_absolute()
        && let Ok(bytes) = std::fs::read(path)
    {
        return Some((Some(path.to_path_buf()), bytes));
    }
    let relative = if path.is_absolute() || name.contains(':') {
        // An absolute path from another machine, Windows' included: its file name.
        Path::new(name.rsplit(['/', '\\']).next().unwrap_or(name)).to_path_buf()
    } else {
        path.to_path_buf()
    };
    let mut candidates = Vec::new();
    candidates.extend(project.map(|root| root.join(&relative)));
    candidates.extend(map_dir.map(|dir| dir.join(&relative)));
    candidates.extend(game.map(|game| game.base.join(&relative)));
    for candidate in candidates {
        if let Ok(bytes) = std::fs::read(&candidate) {
            return Some((Some(candidate), bytes));
        }
    }
    game?.file(&relative.to_string_lossy())
}

/// The textures of the map WAD `bytes`, in directory order, read as QuakeSpasm reads them
/// (WAD3-3); `None` for a file that is no WAD.
fn wad_textures(bytes: &[u8], wad: &str) -> Option<Vec<Held>> {
    let read = MapWad::read(bytes).ok()?;
    let mut out = Vec::new();
    for entry in read.entries() {
        if entry.ty != WAD_TYP_MIPTEX && entry.ty != WAD_TYP_MIPTEX_PALETTE {
            continue;
        }
        let Some(texture) = read.texture(entry.name.as_bytes()) else {
            continue;
        };
        out.push(Held {
            name: String::from_utf8_lossy(entry.name.as_bytes()).into_owned(),
            origin: TextureOrigin::Wad {
                wad: wad.to_owned(),
            },
            width: texture.width,
            height: texture.height,
            pixels: texture.pixels.to_vec(),
            palette: texture.palette.map(Palette::from_wad3),
        });
    }
    Some(out)
}

/// The textures a BSP holds in its own lump.
fn bsp_textures(bytes: &[u8]) -> Vec<Held> {
    let Ok(bsp) = Bsp::read(bytes, BspLoader::default()) else {
        return Vec::new();
    };
    bsp.textures
        .iter()
        .flatten()
        .map(|texture| Held {
            name: String::from_utf8_lossy(&texture.name).into_owned(),
            origin: TextureOrigin::Bsp,
            width: texture.width,
            height: texture.height,
            pixels: texture.pixels.to_vec(),
            palette: texture.palette.map(Palette::from_wad3),
        })
        .collect()
}

impl TextureSources {
    /// The sources of `document`'s textures: the WADs its world names (MAP-6.3, split at `;` as
    /// ericw-tools splits them), its BSP when it was opened from one, and the palette: the
    /// project's `gfx/palette.lmp`, else that of the game at `game`, the base directory holding
    /// `id1`. `chosen` is the game folder chosen for Play, if any, which the sources are made
    /// again for when it changes.
    #[must_use]
    pub fn load(
        document: &MapDocument,
        project: Option<&Path>,
        game: Option<&Path>,
        chosen: Option<&Path>,
    ) -> Self {
        let paths = document.file().wad_paths(Compiler::EricwTools);
        let game_at = game.map(Game::at);
        let map_dir = document.path().parent();
        let mut files = Vec::new();
        for raw in &paths {
            let name = String::from_utf8_lossy(raw).into_owned();
            let read = find_wad(&name, project, map_dir, game_at.as_ref());
            files.push((name, read));
        }
        let bsp = match document.source() {
            MapSource::Bsp(bytes) => Some(bytes.clone()),
            MapSource::Map => None,
        };
        let palette = game_palette(project, game_at.as_ref());
        let mut sources = Self::from_read(files, bsp.as_deref(), palette);
        sources.made_for = (paths, chosen.map(Path::to_path_buf));
        sources
    }

    /// Sources of these WADs, each a name and its file when one was found, this BSP and this
    /// palette.
    fn from_read(files: Vec<WadRead>, bsp: Option<&[u8]>, palette: Option<Palette>) -> Self {
        let mut sources = Self {
            palette,
            ..Self::default()
        };
        for (name, read) in files {
            let (path, state) = match read {
                Some((path, bytes)) => match wad_textures(&bytes, &name) {
                    Some(textures) => {
                        let state = WadState::Read {
                            textures: textures.len(),
                        };
                        for texture in textures {
                            sources.hold(texture);
                        }
                        (path, state)
                    }
                    None => (path, WadState::Unreadable),
                },
                None => (None, WadState::Missing),
            };
            sources.wads.push(WadFound { name, path, state });
        }
        for texture in bsp.map(bsp_textures).unwrap_or_default() {
            sources.hold(texture);
        }
        sources
    }

    /// Sources with these files, for tests and callers that have the bytes already.
    #[must_use]
    pub fn from_files(
        wads: Vec<(String, Vec<u8>)>,
        bsp: Option<&[u8]>,
        palette: Option<Palette>,
    ) -> Self {
        let files = wads
            .into_iter()
            .map(|(name, bytes)| (name, Some((None, bytes))))
            .collect();
        Self::from_read(files, bsp, palette)
    }

    /// Keeps `texture` unless one of its name is held already: the first WAD holding a name
    /// wins, then the BSP.
    fn hold(&mut self, texture: Held) {
        let key = texture.name.to_lowercase();
        if self.by_name.contains_key(&key) {
            return;
        }
        self.by_name.insert(key, self.held.len());
        self.held.push(texture);
    }

    /// Whether `document`'s world now names other WADs, or another game folder was chosen,
    /// than the sources were made for.
    #[must_use]
    pub fn is_stale(&self, document: &MapDocument, chosen: Option<&Path>) -> bool {
        let (paths, game) = &self.made_for;
        *paths != document.file().wad_paths(Compiler::EricwTools) || game.as_deref() != chosen
    }

    /// The WAD files to watch for another program's writes.
    #[must_use]
    pub fn watched(&self) -> Vec<PathBuf> {
        self.wads
            .iter()
            .filter_map(|wad| wad.path.clone())
            .collect()
    }

    /// Whether a palette was found, without which no texture can be shown in its colours.
    #[must_use]
    pub const fn has_palette(&self) -> bool {
        self.palette.is_some()
    }

    /// Every texture with how many of `document`'s faces use it: the WADs' in their order, the
    /// first WAD holding a name winning, then the BSP's, then the names no WAD or BSP holds,
    /// in order of name.
    #[must_use]
    pub fn listing(&self, document: &MapDocument) -> MapTextures {
        let mut uses: HashMap<String, usize> = HashMap::new();
        let mut missing: BTreeMap<String, usize> = BTreeMap::new();
        for (name, count) in document.texture_uses() {
            let key = name.to_lowercase();
            if self.by_name.contains_key(&key) {
                let total = uses.entry(key).or_default();
                *total = total.saturating_add(count);
            } else {
                let total = missing.entry(name).or_default();
                *total = total.saturating_add(count);
            }
        }
        let mut textures: Vec<BrowserTexture> = self
            .held
            .iter()
            .map(|held| BrowserTexture {
                name: held.name.clone(),
                origin: held.origin.clone(),
                width: held.width,
                height: held.height,
                uses: uses
                    .get(&held.name.to_lowercase())
                    .copied()
                    .unwrap_or_default(),
            })
            .collect();
        textures.extend(missing.into_iter().map(|(name, uses)| BrowserTexture {
            name,
            origin: TextureOrigin::Missing,
            width: 0,
            height: 0,
            uses,
        }));
        MapTextures {
            wads: self.wads.clone(),
            textures,
        }
    }

    /// The texture `name` in its colours: a WAD3 texture's own palette, else the game's
    /// The game palette textures are shown in, when one was found.
    #[must_use]
    pub const fn palette(&self) -> Option<&Palette> {
        self.palette.as_ref()
    }

    /// (WAD-8); `None` with no palette, no such texture, or one that does not decode (WAD-8.3).
    #[must_use]
    pub fn decode(&self, name: &str) -> Option<DecodedTexture> {
        let held = self.held.get(*self.by_name.get(&name.to_lowercase())?)?;
        let palette = held.palette.as_ref().or(self.palette.as_ref())?;
        palette
            .decode_pixels(held.name.as_bytes(), held.width, held.height, &held.pixels)
            .ok()
    }
}

impl MapDocument {
    /// How many faces use each texture name, by name.
    #[must_use]
    pub fn texture_uses(&self) -> BTreeMap<String, usize> {
        let mut uses = BTreeMap::new();
        for entity in &self.file().entities {
            for brush in entity.brushes() {
                for face in &brush.faces {
                    let name = String::from_utf8_lossy(face.texture()).into_owned();
                    let count: &mut usize = uses.entry(name).or_default();
                    *count = count.saturating_add(1);
                }
            }
        }
        uses
    }
}

/// `map.textures`.
struct MapTexturesCommand;

impl<C: HasWorkbench + 'static> Command<C> for MapTexturesCommand {
    const ID: &'static str = MAP_TEXTURES;
    const SUMMARY: &'static str = "Lists the open map's textures: those of the WADs its world \
        names, found in the project, beside the map or in the game, and of the BSP it was \
        opened from, each with its size and how many faces use it, and the names its faces use \
        that nothing holds.";
    type Params = NoParams;
    type Output = MapTextures;
    type Error = WorkbenchError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<MapTextures, C>, WorkbenchError> {
        let (game, chosen) = context.preview().map_or((None, None), |preview| {
            (
                preview.game_base_path(),
                preview.game_folder().map(Path::to_path_buf),
            )
        });
        let workbench = context.workbench();
        let document = workbench.map().ok_or(WorkbenchError::NoMap)?;
        let sources = TextureSources::load(
            document,
            workbench.root(),
            game.as_deref(),
            chosen.as_deref(),
        );
        Ok(Outcome::unchanged(sources.listing(document)))
    }
}

/// Registers `map.textures`.
pub(crate) fn register_map_textures<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), qs_commands::RegisterError> {
    registry.register(MapTexturesCommand)
}

#[cfg(test)]
mod tests;
