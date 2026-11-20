// SPDX-License-Identifier: GPL-3.0-or-later

//! The textures of the open map, for the texture browser (P6-10, MAP-5): every texture of the
//! WADs its world names, found in the project, beside the map or in the game, and of the BSP
//! it was opened from; each with how many faces use it; decoded through the game's palette only
//! when asked. Each WAD and BSP is read once, when the sources are made.
//!
//! Quake ships no texture WAD: each level's textures are in its BSP, in `pak0.pak` and
//! `pak1.pak`. So the textures of every level on the game's search path are listed too
//! (P6-43), read in place, each name once, after the WADs' and the open BSP's; a new map can be
//! textured from the game alone, and a compile finds those textures in memory
//! ([`GameTextures::wad_of`]).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use qs_bsp::{Bsp, BspLoader};
use qs_commands::{Command, NoParams, Outcome};
use qs_map::Compiler;
use qs_vfs::{FS_GAMENAME, Location, StdFs, Vfs};
use qs_wad::texture::{DecodedTexture, Palette};
use qs_wad::write::WadBuilder;
use qs_wad::{MapWad, WAD_NAME_SIZE, WAD_TYP_MIPTEX, WAD_TYP_MIPTEX_PALETTE};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{HasMap, MapEditError, MapProject};
use crate::{MapDocument, MapSource};

/// The id of `map.list_textures`.
pub const MAP_TEXTURES: &str = "map.list_textures";

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
    /// A level of the game, read from its BSP on the game's search path (P6-43).
    Game {
        /// The level's name, as `map` takes it: `e1m1`.
        map: String,
    },
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

/// What `map.list_textures` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MapTextures {
    /// The WADs the world names, in its order.
    pub wads: Vec<WadFound>,
    /// Every texture: the WADs' in their order, then the BSP's, then the game's levels', then
    /// the names nothing holds, each name once.
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

/// The folder of the search path that holds the levels.
const MAPS_FOLDER: &str = "maps/";
/// The ending of a level's file.
const BSP_ENDING: &str = ".bsp";
/// The start of the name of a frame of an animated texture, before its frame's character.
const ANIMATED: &str = "+";
/// The start of the name of a BSP that is a model, an ammo or health box, and no level.
const BOX_MODEL: &str = "b_";

/// The textures of the game's levels (P6-43): those of every `maps/*.bsp` on the game's search
/// path, each name once, the first level holding it by the levels' names; read in place, and
/// kept whole, so a compile can be given them as a WAD.
#[derive(Clone, Debug, Default)]
pub struct GameTextures {
    /// Each texture as the browser holds it, with its `miptex_t` whole: header and four levels.
    textures: Vec<(Held, Vec<u8>)>,
    /// Each by its name in lower case.
    by_name: HashMap<String, usize>,
}

impl GameTextures {
    /// The textures of the levels of `game`.
    #[must_use]
    pub fn read(game: &Game<'_>) -> Self {
        let mut textures = Self::default();
        for (map, bytes) in game.levels() {
            textures.take(&map, &bytes);
        }
        textures
    }

    /// Takes the textures of the level `map`, whose BSP is `bytes`, that no level before it
    /// held. id's own loader reads them, which keeps all four levels of each; a BSP it does
    /// not read, and a texture kept in a WAD outside its BSP, give none.
    fn take(&mut self, map: &str, bytes: &[u8]) {
        let Ok(bsp) = Bsp::read(bytes, BspLoader::GlQuake) else {
            return;
        };
        for texture in bsp.textures.iter().flatten() {
            let name = String::from_utf8_lossy(&texture.name).into_owned();
            let key = name.to_lowercase();
            if self.by_name.contains_key(&key) {
                continue;
            }
            let mut lump = Vec::with_capacity(texture.pixels.len().saturating_add(WAD_NAME_SIZE));
            let mut field = [0_u8; WAD_NAME_SIZE];
            for (slot, byte) in field.iter_mut().zip(&texture.name) {
                *slot = *byte;
            }
            lump.extend_from_slice(&field);
            for word in [texture.width, texture.height]
                .into_iter()
                .chain(texture.offsets)
            {
                lump.extend_from_slice(&word.to_le_bytes());
            }
            lump.extend_from_slice(texture.pixels);
            let held = Held {
                name,
                origin: TextureOrigin::Game {
                    map: map.to_owned(),
                },
                width: texture.width,
                height: texture.height,
                pixels: texture.pixels.to_vec(),
                palette: None,
            };
            self.by_name.insert(key, self.textures.len());
            self.textures.push((held, lump));
        }
    }

    /// A WAD of the textures of `names` the levels hold, for a compile whose WADs lack them;
    /// none when they hold none of them. A frame of an animation (`+0slip`) brings the
    /// animation's other frames the levels hold (`+1slip`, `+aslip`), which the compiler
    /// writes only from the WAD it is given (BSP-4.8).
    #[must_use]
    pub fn wad_of<'n>(&self, names: impl IntoIterator<Item = &'n [u8]>) -> Option<Vec<u8>> {
        let mut builder = WadBuilder::new();
        let mut any = None;
        for name in names {
            let key = String::from_utf8_lossy(name).to_lowercase();
            let frames = animation_of(&key);
            for (held, lump) in &self.textures {
                let own = held.name.to_lowercase();
                let frame_of_it = frames.is_some() && animation_of(&own) == frames;
                // A name asked for twice, or a frame brought twice, is in the WAD once.
                if (own == key || frame_of_it)
                    && builder
                        .add(held.name.as_bytes(), WAD_TYP_MIPTEX, lump.clone())
                        .is_ok()
                {
                    any = Some(());
                }
            }
        }
        any.and_then(|()| builder.to_bytes().ok())
    }

    /// How many textures the levels hold.
    #[must_use]
    pub fn len(&self) -> usize {
        self.textures.len()
    }

    /// Whether the levels hold none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.textures.is_empty()
    }
}

/// The animation the texture `name` is a frame of: its name from the third character on, after
/// the `+` and the frame's character (BSP-4.8); none for a texture that is no frame.
fn animation_of(name: &str) -> Option<&str> {
    let rest = name.strip_prefix(ANIMATED)?;
    let (at, _) = rest.char_indices().nth(1)?;
    Some(rest.split_at(at).1)
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
    palette_read(project, game).map(|(palette, _)| palette)
}

/// The palette of a project or its game, with the bytes it was read from.
fn palette_read(project: Option<&Path>, game: Option<&Game<'_>>) -> Option<(Palette, Vec<u8>)> {
    let bytes = project
        .and_then(|root| std::fs::read(root.join(PALETTE_LMP)).ok())
        .or_else(|| {
            game.and_then(|game| game.file(PALETTE_LMP))
                .map(|(_, bytes)| bytes)
        })?;
    let palette = Palette::from_lmp(&bytes).ok()?;
    Some((palette, bytes))
}

/// The bytes of the palette a map's light colours texels by (LIGHT-11.6): the project's own
/// `gfx/palette.lmp`, else that of the game at the base directory `game`; none when neither
/// reads as a palette.
#[must_use]
pub fn light_palette(project: Option<&Path>, game: Option<&Path>) -> Option<Vec<u8>> {
    let game = game.map(Game::at);
    palette_read(project, game.as_ref()).map(|(_, bytes)| bytes)
}

/// The game at a base directory: its search path, when it can be built, and the directory a
/// loose file is looked for in.
pub struct Game<'a> {
    base: &'a Path,
    vfs: Option<Vfs<StdFs>>,
}

impl<'a> Game<'a> {
    /// The game whose base folder is `base`, its search path read as the engine reads it.
    #[must_use]
    pub fn at(base: &'a Path) -> Self {
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

    /// Each level on the search path, by its name in lower case and in the names' order, with
    /// its BSP's bytes: a `maps/*.bsp` that is no box model, the first file of a name on the
    /// search path, as the engine would load it.
    fn levels(&self) -> Vec<(String, Vec<u8>)> {
        let Some(vfs) = &self.vfs else {
            return Vec::new();
        };
        let mut levels: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for file in vfs.files() {
            let name = String::from_utf8_lossy(file.name()).to_lowercase();
            let Some(map) = name
                .strip_prefix(MAPS_FOLDER)
                .and_then(|name| name.strip_suffix(BSP_ENDING))
                .filter(|map| !map.contains('/') && !map.starts_with(BOX_MODEL))
            else {
                continue;
            };
            if levels.contains_key(map) {
                continue;
            }
            if let Ok(bytes) = vfs.read(&file) {
                levels.insert(map.to_owned(), bytes);
            }
        }
        levels.into_iter().collect()
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
pub fn find_wad(
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
        // The game's own levels last: a WAD of the map's, or its BSP, has the say over a name.
        let levels = game_at.as_ref().map_or_default(GameTextures::read);
        for (texture, _) in levels.textures {
            sources.hold(texture);
        }
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
        for texture in bsp.map_or_default(bsp_textures) {
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
    /// wins, then the BSP, then the game's levels.
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

    /// The game palette textures are shown in, when one was found.
    #[must_use]
    pub const fn palette(&self) -> Option<&Palette> {
        self.palette.as_ref()
    }

    /// The width and height of the texture `name`, in texels; none when no source holds it.
    #[must_use]
    pub fn size(&self, name: &str) -> Option<[u32; 2]> {
        let held = self.held.get(*self.by_name.get(&name.to_lowercase())?)?;
        Some([held.width, held.height])
    }

    /// The texture `name` in its colours: a WAD3 texture's own palette, else the game's
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

/// How many faces of the map `file` use each texture name, by name.
pub(crate) fn uses_in(file: &qs_map::MapFile) -> BTreeMap<String, usize> {
    let mut uses = BTreeMap::new();
    for entity in &file.entities {
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

impl MapDocument {
    /// How many faces use each texture name, by name.
    #[must_use]
    pub fn texture_uses(&self) -> BTreeMap<String, usize> {
        uses_in(self.file())
    }
}

/// `map.list_textures`.
pub(super) struct MapTexturesCommand;

impl<C: HasMap + 'static> Command<C> for MapTexturesCommand {
    const ID: &'static str = MAP_TEXTURES;
    const ACCESS: qs_commands::Access = qs_commands::Access::Read;
    const SUMMARY: &'static str = "Lists the open map's textures: those of the WADs its world \
        names, found in the project, beside the map or in the game, of the BSP it was opened \
        from, and of the game's own levels, each once with where it comes from, its size and \
        how many faces use it, and the names its faces use that nothing holds.";
    type Params = NoParams;
    type Output = MapTextures;
    type Error = MapEditError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<MapTextures, C>, MapEditError> {
        let game = context.game_folders();
        let root = context.map_project().project_root();
        let document = context.map_store().map().ok_or(MapEditError::NoMap)?;
        let sources = TextureSources::load(
            document,
            root.as_deref(),
            game.base.as_deref(),
            game.chosen.as_deref(),
        );
        Ok(Outcome::unchanged(sources.listing(document)))
    }
}

#[cfg(test)]
mod tests;
