// SPDX-License-Identifier: GPL-3.0-or-later

//! The textures of the open map, for the texture browser (P6-10, MAP-5): every texture of the
//! WADs its world names, found in the project, beside the map or in the game, and of the BSP
//! it was opened from; each with how many faces use it; decoded through the game's palette only
//! when asked.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use qs_bsp::{Bsp, BspLoader};
use qs_commands::{Command, NoParams, Outcome, Registry};
use qs_map::Compiler;
use qs_vfs::{FS_GAMENAME, Location, StdFs, Vfs};
use qs_wad::texture::{DecodedTexture, Palette};
use qs_wad::{MipTexHeader, WAD_TYP_MIPTEX, Wad};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::map_edit::{MapDocument, MapSource};
use crate::{HasWorkbench, WorkbenchError};

/// The id of `map.textures`.
pub const MAP_TEXTURES: &str = "map.textures";

/// The game's palette, `gfx/palette.lmp` (REND-1.8).
const PALETTE_LMP: &str = "gfx/palette.lmp";

/// The game at a base directory: its search path, when it can be built, and the directory a
/// loose file is looked for in.
struct Game<'a> {
    base: &'a Path,
    vfs: Option<Vfs<StdFs>>,
}

impl<'a> Game<'a> {
    fn at(base: &'a Path) -> Self {
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

/// A WAD the world names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WadFound {
    /// Its name as the world writes it.
    pub name: String,
    /// The file it was read from, when it is a file of its own and not in a PAK; the one to
    /// watch for another program's writes.
    pub path: Option<PathBuf>,
    /// Whether it was found and read as a WAD2.
    pub found: bool,
    /// Its textures.
    pub textures: usize,
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

/// A WAD read.
#[derive(Clone, Debug)]
struct WadFile {
    found: WadFound,
    bytes: Option<Vec<u8>>,
}

/// The files the open map's textures come from: its WADs, its BSP and the game's palette.
#[derive(Clone, Debug, Default)]
pub struct TextureSources {
    wads: Vec<WadFile>,
    bsp: Option<Vec<u8>>,
    palette: Option<Palette>,
}

/// Where a WAD path is looked for: as it is when absolute, else under the project, beside the
/// map, under the game's base directory, then on the game's search path.
fn find_wad(
    name: &str,
    project: Option<&Path>,
    map_dir: Option<&Path>,
    game: Option<&Game<'_>>,
) -> Option<(Option<PathBuf>, Vec<u8>)> {
    let path = Path::new(name);
    let mut candidates = Vec::new();
    if path.is_absolute() {
        candidates.push(path.to_path_buf());
    } else {
        candidates.extend(project.map(|root| root.join(path)));
        candidates.extend(map_dir.map(|dir| dir.join(path)));
        candidates.extend(game.map(|game| game.base.join(path)));
    }
    for candidate in candidates {
        if let Ok(bytes) = std::fs::read(&candidate) {
            return Some((Some(candidate), bytes));
        }
    }
    game?.file(name)
}

impl TextureSources {
    /// The sources of `document`'s textures: the WADs its world names (MAP-6.3, split at `;` as
    /// ericw-tools splits them), its BSP when it was opened from one, and the palette of the
    /// game at `game`, the base directory holding `id1`.
    #[must_use]
    pub fn load(document: &MapDocument, project: Option<&Path>, game: Option<&Path>) -> Self {
        let game = game.map(Game::at);
        let map_dir = document.path().parent();
        let mut wads = Vec::new();
        for raw in document.file().wad_paths(Compiler::EricwTools) {
            let name = String::from_utf8_lossy(&raw).into_owned();
            let read = find_wad(&name, project, map_dir, game.as_ref());
            let (path, bytes) = match read {
                Some((path, bytes)) if Wad::read(&bytes).is_ok() => (path, Some(bytes)),
                Some((path, _)) => (path, None),
                None => (None, None),
            };
            let textures = bytes.as_deref().map_or(0, |b| wad_textures(b).len());
            wads.push(WadFile {
                found: WadFound {
                    name,
                    path,
                    found: bytes.is_some(),
                    textures,
                },
                bytes,
            });
        }
        let bsp = match document.source() {
            MapSource::Bsp(bytes) => Some(bytes.clone()),
            MapSource::Map => None,
        };
        let palette = game
            .as_ref()
            .and_then(|game| game.file(PALETTE_LMP))
            .and_then(|(_, bytes)| Palette::from_lmp(&bytes).ok());
        Self { wads, bsp, palette }
    }

    /// Sources with these files, for tests and callers that have the bytes already.
    #[must_use]
    pub fn from_files(
        wads: Vec<(String, Vec<u8>)>,
        bsp: Option<Vec<u8>>,
        palette: Option<Palette>,
    ) -> Self {
        let wads = wads
            .into_iter()
            .map(|(name, bytes)| {
                let ok = Wad::read(&bytes).is_ok();
                WadFile {
                    found: WadFound {
                        name,
                        path: None,
                        found: ok,
                        textures: if ok { wad_textures(&bytes).len() } else { 0 },
                    },
                    bytes: ok.then_some(bytes),
                }
            })
            .collect();
        Self { wads, bsp, palette }
    }

    /// The WAD files to watch for another program's writes.
    #[must_use]
    pub fn watched(&self) -> Vec<PathBuf> {
        self.wads
            .iter()
            .filter_map(|wad| wad.found.path.clone())
            .collect()
    }

    /// Whether a palette was found, without which no texture can be shown in its colours.
    #[must_use]
    pub const fn has_palette(&self) -> bool {
        self.palette.is_some()
    }

    /// Every texture with how many of `document`'s faces use it: the WADs' in their order, the
    /// first WAD holding a name winning, then the BSP's, then the names no WAD or BSP holds.
    #[must_use]
    pub fn listing(&self, document: &MapDocument) -> MapTextures {
        let uses = texture_uses(document);
        let mut textures: Vec<BrowserTexture> = Vec::new();
        let mut add = |name: String, origin: TextureOrigin, width: u32, height: u32| {
            if textures.iter().any(|t| t.name.eq_ignore_ascii_case(&name)) {
                return;
            }
            let count = uses
                .iter()
                .filter(|(used, _)| used.eq_ignore_ascii_case(&name))
                .map(|(_, count)| *count)
                .sum();
            textures.push(BrowserTexture {
                name,
                origin,
                width,
                height,
                uses: count,
            });
        };
        for wad in &self.wads {
            for (name, width, height) in wad.bytes.as_deref().map(wad_textures).unwrap_or_default()
            {
                add(
                    name,
                    TextureOrigin::Wad {
                        wad: wad.found.name.clone(),
                    },
                    width,
                    height,
                );
            }
        }
        if let Some(bsp) = self.bsp.as_deref().and_then(read_bsp) {
            for texture in bsp.textures.iter().flatten() {
                add(
                    String::from_utf8_lossy(&texture.name).into_owned(),
                    TextureOrigin::Bsp,
                    texture.width,
                    texture.height,
                );
            }
        }
        for name in uses.keys() {
            add(name.clone(), TextureOrigin::Missing, 0, 0);
        }
        MapTextures {
            wads: self.wads.iter().map(|wad| wad.found.clone()).collect(),
            textures,
        }
    }

    /// The texture `name` in the game palette's colours, from the first WAD holding it, else
    /// the BSP; `None` with no palette or no such texture.
    #[must_use]
    pub fn decode(&self, name: &str) -> Option<DecodedTexture> {
        let palette = self.palette.as_ref()?;
        for wad in &self.wads {
            let Some(bytes) = wad.bytes.as_deref() else {
                continue;
            };
            let Ok(read) = Wad::read(bytes) else {
                continue;
            };
            let Ok(lump) = read.find(name.as_bytes()) else {
                continue;
            };
            if lump.lump_type != WAD_TYP_MIPTEX {
                continue;
            }
            if let Ok(decoded) = read
                .data(lump)
                .map_err(|_| ())
                .and_then(|data| palette.decode_miptex(data).map_err(|_| ()))
            {
                return Some(decoded);
            }
        }
        let bsp = self.bsp.as_deref().and_then(read_bsp)?;
        let texture = bsp
            .textures
            .iter()
            .flatten()
            .find(|t| t.name.eq_ignore_ascii_case(name.as_bytes()))?;
        let area = usize::try_from(texture.width.saturating_mul(texture.height)).ok()?;
        let pixels = texture.pixels.get(..area)?;
        Some(palette.decode_pixels(&texture.name, texture.width, texture.height, pixels))
    }
}

/// A BSP read as QuakeSpasm reads it.
fn read_bsp(bytes: &[u8]) -> Option<Bsp<'_>> {
    Bsp::read(bytes, BspLoader::default()).ok()
}

/// The name, width and height of each texture lump of the WAD `bytes`, in directory order.
fn wad_textures(bytes: &[u8]) -> Vec<(String, u32, u32)> {
    let Ok(wad) = Wad::read(bytes) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for lump in wad.lumps() {
        if lump.lump_type != WAD_TYP_MIPTEX {
            continue;
        }
        let Ok(header) = wad.data(lump).and_then(MipTexHeader::parse) else {
            continue;
        };
        out.push((
            String::from_utf8_lossy(lump.name.as_bytes()).into_owned(),
            header.width,
            header.height,
        ));
    }
    out
}

/// How many faces of `document` use each texture name, by name.
fn texture_uses(document: &MapDocument) -> BTreeMap<String, usize> {
    let mut uses = BTreeMap::new();
    for entity in &document.file().entities {
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
        let game = context
            .preview()
            .and_then(|preview| preview.game_base_path());
        let workbench = context.workbench();
        let document = workbench.map().ok_or(WorkbenchError::NoMap)?;
        let sources = TextureSources::load(document, workbench.root(), game.as_deref());
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
