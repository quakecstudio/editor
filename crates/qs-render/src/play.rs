// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/gl_draw.c, Draw_Init, Draw_Character, Draw_Pic, Draw_Fill, GL_Set2D
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/gl_rmain.c, R_DrawViewModel, R_PolyBlend, R_DrawEntitiesOnList
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/sbar.c, Sbar_DrawPic, Sbar_DeathmatchOverlay
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/gl_fog.c, Fog_ParseWorldspawn, Fog_Update, Fog_GetColor, Fog_GetDensity

//! A frame of play: the client's [`Scene`] and [`Hud`] turned into what the renderer draws,
//! `R_RenderView` and the 2D layer over it (REND-4.11, REND-7, REND-10, REND-11, PART-3.2,
//! VIEW-4.6, HUD-1 to HUD-3).

use std::sync::Arc;

use qs_bsp::Bsp;
use qs_client::{Anchor, HUD_SBAR_HEIGHT, Hud, HudItem, Placed, Scene, SceneEntity};
use qs_wad::{Conchars, QPic, WAD_CONCHARS_TRANSPARENT, Wad};

use crate::alias::{AliasEntity, AliasError, AliasKind, AliasModel, TORCHES, entity_light};
use crate::camera::Camera;
use crate::gpu::{
    AliasDraw, Assets, FlatDraw, FlatFrame, FlatVertex, Fog, MapCallback, ModelFrame, PLAYER_SKINS,
    PlayerSkin, SkinKey, SpriteDraw, SpriteFrame, View,
};
use crate::light::light_point;
use crate::look::Look;
use crate::md5::Md5Model;
use crate::mesh::{Image, MapMesh, Palette, Placement, fullbright_split};
use crate::particles::particle_triangles;
use crate::skybox::{SkyKeys, Skybox, sky_parse_worldspawn};
use crate::sprite::{Eye, SpriteEntity, SpriteError, SpriteModel, angle_vectors};
use crate::styles::LightStyles;

/// The bar's width, which the bar is centred by on a wider screen: `(vid.width - 320)>>1`
/// (HUD-1).
const BAR_WIDTH: i32 = 320;
/// A console character's side on the screen, `Draw_Character`'s 8 (HUD-3).
const CHAR_SIDE: f32 = 8.0;
/// `frow = row*0.0625`: a character's share of the 16 by 16 `conchars` grid.
const CHAR_CELL: f32 = 0.0625;
/// The character `Draw_Character` does not draw, the space.
const SPACE: u8 = 32;
/// `if (y <= -8) return`: a character wholly above the screen is not drawn.
const CHAR_OFF_TOP: i32 = -8;
/// The palette index a picture does not draw, `GL_LoadPicTexture`'s alpha (HUD-1).
const PIC_TRANSPARENT: u8 = 255;
/// The name the console characters are kept under among the pictures.
const CONCHARS: &str = "conchars";
/// The console's background (CON-3.2).
const CONBACK: &str = "gfx/conback.lmp";
/// `fov`, the field of view across: `scr_fov` (REND-3.4).
const FOV_X: f32 = 90.0;
/// A colour that leaves a texel as it is.
const WHITE: [f32; 4] = [1.0; 4];

/// Why play could not be set up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayError {
    /// A model the server precached could not be read.
    Model(String),
    /// `gfx.wad` or a picture in it could not be read.
    Wad(String),
}

impl core::fmt::Display for PlayError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Model(message) | Self::Wad(message) => f.write_str(message),
        }
    }
}

impl core::error::Error for PlayError {}

impl From<AliasError> for PlayError {
    fn from(error: AliasError) -> Self {
        Self::Model(error.to_string())
    }
}

impl From<SpriteError> for PlayError {
    fn from(error: SpriteError) -> Self {
        Self::Model(error.to_string())
    }
}

/// The 2D pictures by name: `gfx.wad`'s lumps, the `gfx/*.lmp` files and the console
/// characters, each an image the renderer is made with (HUD-1 to HUD-3).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pics {
    names: Vec<String>,
    images: Vec<Image>,
}

impl Pics {
    /// No pictures yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            names: Vec::new(),
            images: Vec::new(),
        }
    }

    /// Adds every picture lump of `gfx.wad` (`wad`) by its name, and `conchars` with its
    /// index 0 transparent, as `Draw_Init` loads them (WAD-4.2).
    ///
    /// # Errors
    ///
    /// [`PlayError::Wad`] when the WAD cannot be read; a lump that is not a picture is
    /// skipped.
    pub fn add_wad(&mut self, wad: &[u8], palette: &Palette) -> Result<(), PlayError> {
        let wad = Wad::read(wad).map_err(|e| PlayError::Wad(format!("gfx.wad: {e:?}")))?;
        for lump in wad.lumps() {
            let name = String::from_utf8_lossy(lump.name.as_bytes()).into_owned();
            let Ok(data) = wad.data(lump) else {
                continue;
            };
            if name == CONCHARS {
                if let Ok(chars) = Conchars::parse(data) {
                    self.add(&name, conchars_image(&chars, palette));
                }
            } else if let Ok(pic) = QPic::parse(data) {
                self.add(&name, pic_image(&pic, palette));
            }
        }
        Ok(())
    }

    /// Adds the picture file `path` (such as `gfx/ranking.lmp`), a `qpic_t`.
    ///
    /// # Errors
    ///
    /// [`PlayError::Wad`] when the file is not a picture.
    pub fn add_lmp(
        &mut self,
        path: &str,
        bytes: &[u8],
        palette: &Palette,
    ) -> Result<(), PlayError> {
        let pic = QPic::parse(bytes).map_err(|e| PlayError::Wad(format!("{path}: {e:?}")))?;
        self.add(path, pic_image(&pic, palette));
        Ok(())
    }

    /// Adds `image` as `name`, replacing one of that name.
    pub fn add(&mut self, name: &str, image: Image) {
        match self.names.iter().position(|n| n == name) {
            Some(index) => {
                if let Some(slot) = self.images.get_mut(index) {
                    *slot = image;
                }
            }
            None => {
                self.names.push(name.to_owned());
                self.images.push(image);
            }
        }
    }

    /// The index and image of the picture `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<(usize, &Image)> {
        let index = self.names.iter().position(|n| n == name)?;
        Some((index, self.images.get(index)?))
    }

    /// Every picture, by index.
    #[must_use]
    pub fn images(&self) -> &[Image] {
        &self.images
    }
}

/// A picture's pixels through `palette`, index 255 transparent.
pub(crate) fn pic_image(pic: &QPic<'_>, palette: &Palette) -> Image {
    Image {
        width: u32::try_from(pic.width).unwrap_or(0).max(1),
        height: u32::try_from(pic.height).unwrap_or(0).max(1),
        rgba: pic
            .pixels
            .iter()
            .flat_map(|&p| {
                if p == PIC_TRANSPARENT {
                    [0, 0, 0, 0]
                } else {
                    palette.rgba(p)
                }
            })
            .collect(),
    }
}

/// `conchars` through `palette`, index 0 transparent: `Draw_Init` sets every 0 to 255,
/// "proper transparent color" (WAD-4.2).
fn conchars_image(chars: &Conchars<'_>, palette: &Palette) -> Image {
    let side = u32::try_from(qs_wad::WAD_CONCHARS_SIZE).unwrap_or(128);
    let mut rgba = Vec::new();
    // The glyphs row by row of the 16 by 16 grid, each 8 rows of 8 pixels.
    let glyphs: Vec<[u8; 64]> = (0..=u8::MAX).map(|n| chars.glyph(n)).collect();
    for row in 0..16_usize {
        for line in 0..8_usize {
            for column in 0..16_usize {
                let glyph = glyphs.get(row.saturating_mul(16).saturating_add(column));
                for x in 0..8_usize {
                    let p = glyph
                        .and_then(|g| g.get(line.saturating_mul(8).saturating_add(x)))
                        .copied()
                        .unwrap_or(WAD_CONCHARS_TRANSPARENT);
                    if p == WAD_CONCHARS_TRANSPARENT {
                        rgba.extend([0, 0, 0, 0]);
                    } else {
                        rgba.extend(palette.rgba(p));
                    }
                }
            }
        }
    }
    Image {
        width: side,
        height: side,
        rgba,
    }
}

/// A precached model as the renderer draws it.
#[derive(Clone, Debug, PartialEq)]
enum Model {
    /// An `.mdl`, and the index of its first skin among the renderer's skins.
    Alias(AliasModel, usize),
    /// The MD5 model that replaces an `.mdl`, and its first skin's index (REND-12.1).
    Md5(Md5Model, usize),
    /// A `.spr`, and the index of its first frame among the renderer's frames.
    Sprite(SpriteModel, usize),
    /// A brush model of the map, `*n`: the mesh's model `n`.
    Submodel(usize),
    /// A brush model of its own BSP, whose instances the mesh holds by entity.
    External,
    /// Not precached, or not a model that is drawn.
    None,
}

/// What the renderer needs besides the mesh to draw a client's frames.
#[derive(Clone, Debug, PartialEq)]
pub struct Play {
    palette: Palette,
    pics: Pics,
    /// Each precached model, by the client's index.
    models: Vec<Model>,
    skins: Vec<Image>,
    /// Each skin's fullbright texels, when the look lights them and it has any (REND-13.4).
    skin_glows: Vec<Option<Image>>,
    frames: Vec<Image>,
    /// Each external brush model instance: its entity number and the mesh's model index.
    instances: Vec<(usize, usize)>,
    /// The map's fog, and its fades (REND-10.1 to REND-10.3).
    pub fog: FogState,
    /// `r_wateralpha` (REND-9.1).
    pub water_alpha: f32,
    /// `skyfog` (REND-8.1).
    pub skyfog: f32,
    /// The 2D layer's size, `vid.width` and `vid.height` (HUD-1).
    pub screen: [i32; 2],
    /// QuakeSpasm's `gl_zfix` for brush models (REND-1.12).
    pub zfix: bool,
    /// Whether an `.mdl` is drawn as the rerelease's MD5 model where there is one
    /// (REND-12.1).
    pub md5_models: bool,
    /// The look a skin's fullbright texels and the view's width are drawn in, the mesh's
    /// (REND-13); set before [`Play::load_models`].
    pub look: Look,
    /// Each player slot's translated skin (REND-4.13).
    player_skins: PlayerSkins,
    /// The skybox the worldspawn names (REND-8.2), and the box once read.
    sky: SkyKeys,
    skybox: Option<Skybox>,
}

/// Each player slot's translated skin and what it was made from (REND-4.13).
#[derive(Clone, Debug, Default, PartialEq)]
struct PlayerSkins([Option<PlayerSkin>; PLAYER_SKINS]);

impl PlayerSkins {
    /// Slot `slot`'s skin, `model`'s `key.skin` in `key.colors`, made again only when it is
    /// not what the slot holds (`CL_ParseUpdate` and `CL_NewTranslation` calling
    /// `R_TranslatePlayerSkin`).
    fn wear(
        &mut self,
        slot: usize,
        key: SkinKey,
        model: &AliasModel,
        palette: &Palette,
    ) -> Option<PlayerSkin> {
        let held = self.0.get_mut(slot)?;
        if held.as_ref().is_none_or(|skin| skin.key != key) {
            let image = model.translated(key.skin, key.colors, palette)?;
            *held = Some(PlayerSkin {
                slot,
                key,
                image: Arc::new(image),
            });
        }
        held.clone()
    }
}

impl Play {
    /// Play with `pics` and the fog the map's worldspawn sets (REND-10.1), with GLQuake's
    /// 640 by 480 2D layer.
    #[must_use]
    pub fn new(palette: Palette, pics: Pics, entities: &[u8]) -> Self {
        let sky = sky_parse_worldspawn(entities);
        Self {
            palette,
            pics,
            models: Vec::new(),
            skins: Vec::new(),
            skin_glows: Vec::new(),
            frames: Vec::new(),
            instances: Vec::new(),
            fog: FogState::new(fog_parse_worldspawn(entities)),
            water_alpha: 1.0,
            skyfog: sky.skyfog,
            screen: [640, 480],
            zfix: true,
            md5_models: true,
            look: Look::default(),
            player_skins: PlayerSkins::default(),
            sky,
            skybox: None,
        }
    }

    /// Reads every model the client precached, `names` by index, through `read`: alias
    /// models and sprites as images for the renderer, `*n` as the mesh's model `n`, and any
    /// other `.bsp` as an external brush model whose instances [`Play::add_instance`] places.
    /// With [`Play::md5_models`], an `.mdl` is its MD5 model where `rank`, the search-path
    /// element a file is found in, the first 0, does not put the `.mdl` before it
    /// (REND-12.1).
    ///
    /// # Errors
    ///
    /// [`PlayError::Model`] for a model that cannot be read.
    pub fn load_models(
        &mut self,
        names: &[String],
        read: &dyn Fn(&str) -> Option<Vec<u8>>,
        rank: &dyn Fn(&str) -> Option<usize>,
    ) -> Result<(), PlayError> {
        self.models.clear();
        for name in names {
            let torch = TORCHES.contains(&name.as_str());
            let replacement = match self.md5_replacement(name, read, rank) {
                Some((md5, bytes)) => {
                    match Md5Model::read(&md5, &bytes, read, &self.palette, torch) {
                        Ok(model) => Some(model),
                        // REND-12.1: an MD5 model that does not read leaves its `.mdl`.
                        Err(_) if read(name).is_some() => None,
                        Err(error) => return Err(error.into()),
                    }
                }
                None => None,
            };
            let model = if let Some(n) = name.strip_prefix('*') {
                n.parse().map_or(Model::None, Model::Submodel)
            } else if let Some(model) = replacement {
                let first = self.skins.len();
                self.skins.extend_from_slice(model.images());
                self.skin_glows.resize(self.skins.len(), None);
                Model::Md5(model, first)
            } else if name.ends_with(".mdl") {
                let bytes =
                    read(name).ok_or_else(|| PlayError::Model(format!("{name}: missing")))?;
                let model = AliasModel::read(name, &bytes, &self.palette)?;
                let first = self.skins.len();
                for (image, texels) in model.images().iter().zip(model.texels()) {
                    // REND-13.4: a skin with fullbright texels glows with them.
                    let split = self
                        .look
                        .fullbrights()
                        .then(|| {
                            fullbright_split(texels, (image.width, image.height), &self.palette)
                        })
                        .flatten();
                    match split {
                        Some([lit, glow]) => {
                            self.skins.push(lit);
                            self.skin_glows.push(Some(glow));
                        }
                        None => {
                            self.skins.push(image.clone());
                            self.skin_glows.push(None);
                        }
                    }
                }
                Model::Alias(model, first)
            } else if name.ends_with(".spr") {
                let bytes =
                    read(name).ok_or_else(|| PlayError::Model(format!("{name}: missing")))?;
                let model = SpriteModel::read(name, &bytes, &self.palette)?;
                let first = self.frames.len();
                self.frames.extend_from_slice(model.images());
                Model::Sprite(model, first)
            } else if name.ends_with(".bsp") {
                Model::External
            } else {
                Model::None
            };
            self.models.push(model);
        }
        Ok(())
    }

    /// Reads the skybox the worldspawn names through `read` (REND-8.2); a map naming none,
    /// or one whose faces are all missing, keeps the scrolling layers.
    pub fn load_sky(&mut self, read: &dyn Fn(&str) -> Option<Vec<u8>>) {
        self.skybox = Skybox::load(&self.sky.name, read);
    }

    /// `r_replacemodels`' look for `name`'s MD5 model, `progs/x.md5mesh` for `progs/x.mdl`:
    /// its name and bytes when it is found and the `.mdl` is not found on an earlier
    /// search-path element ("Ignoring %s from lower priority path"), with [`Play::md5_models`] on
    /// (REND-12.1).
    fn md5_replacement(
        &self,
        name: &str,
        read: &dyn Fn(&str) -> Option<Vec<u8>>,
        rank: &dyn Fn(&str) -> Option<usize>,
    ) -> Option<(String, Vec<u8>)> {
        let stem = name.strip_suffix(".mdl").filter(|_| self.md5_models)?;
        let md5 = format!("{stem}.md5mesh");
        let found = rank(&md5)?;
        if rank(name).is_some_and(|mdl| mdl < found) {
            return None;
        }
        let bytes = read(&md5)?;
        Some((md5, bytes))
    }

    /// Records that entity `entity`'s external brush model is the mesh's model `mesh_model`.
    pub fn add_instance(&mut self, entity: usize, mesh_model: usize) {
        self.instances.push((entity, mesh_model));
    }

    /// The images the renderer is made with.
    #[must_use]
    pub fn assets(&self) -> Assets<'_> {
        Assets {
            skins: &self.skins,
            skin_glows: &self.skin_glows,
            frames: &self.frames,
            pics: self.pics.images(),
            skybox: self.skybox.as_ref(),
        }
    }

    /// One frame: `scene` and `hud` as the renderer draws them on an image `aspect` wide per
    /// unit of height, the alias models lit from `bsp`, and `mesh` relit for the frame's
    /// styles and dynamic lights (REND-5.5, REND-7).
    #[must_use]
    #[expect(
        clippy::too_many_lines,
        reason = "R_DrawEntitiesOnList, R_DrawViewModel and the 2D layer in R_RenderView's order, read as one"
    )]
    pub fn frame(
        &mut self,
        scene: &Scene,
        hud: &Hud,
        bsp: &Bsp<'_>,
        mesh: &mut MapMesh,
        aspect: f32,
    ) -> MapCallback {
        let styles = LightStyles::at(
            |style| {
                scene
                    .lightstyles
                    .get(style)
                    .map(Vec::as_slice)
                    .filter(|s| !s.is_empty())
            },
            scene.time,
        );
        let lightmap = mesh.relight_with(&styles, &scene.lights);
        let refdef = &scene.view;
        let (forward, right, up) = angle_vectors(refdef.angles);
        let eye = Eye {
            origin: refdef.origin,
            forward,
            right,
            up,
        };
        let mut models = ModelFrame::default();
        let mut sprites = SpriteFrame::default();
        let mut placements: Vec<Option<[[f32; 4]; 4]>> = vec![None; mesh.models.len()];
        if let Some(world) = placements.first_mut() {
            *world = Some(Placement::WORLD.matrix());
        }
        let mut player_skins = Vec::new();
        let light = |origin| light_point(bsp, &styles, origin);
        // R_DrawEntitiesOnList: the alias and brush models, then the sprites.
        for entity in scene.entities.iter().chain(&scene.statics) {
            match self.models.get(entity.model) {
                Some(drawn @ (Model::Alias(..) | Model::Md5(..))) => {
                    let kind = if entity.entity >= 1 && entity.entity <= scene.maxclients {
                        AliasKind::Player
                    } else {
                        AliasKind::Other
                    };
                    let lit =
                        entity_light(light(entity.origin), kind, entity.origin, &scene.lights);
                    let alias = AliasEntity {
                        origin: entity.origin,
                        angles: entity.angles,
                        frame: index_of(entity.frame),
                        skin: index_of(entity.skin),
                        light: lit,
                        blend: entity.poses,
                        kind,
                    };
                    drawn.draw_alias(&alias, scene.time, &mut models);
                    let Model::Alias(model, _) = drawn else {
                        continue;
                    };
                    // REND-4.13: a player wearing colours is drawn with its slot's skin.
                    let slot = entity.entity.checked_sub(1).filter(|&s| {
                        kind == AliasKind::Player && entity.colormap != 0 && s < PLAYER_SKINS
                    });
                    if let (Some(slot), Some(draw)) = (slot, models.draws.last_mut()) {
                        let key = SkinKey {
                            model: entity.model,
                            skin: alias.skin,
                            colors: scene.colors.get(slot).copied().unwrap_or(0),
                        };
                        player_skins.extend(self.player_skins.wear(
                            slot,
                            key,
                            model,
                            &self.palette,
                        ));
                        draw.skin = self.skins.len().saturating_add(slot);
                    }
                }
                Some(Model::Sprite(model, first)) => {
                    let sprite = SpriteEntity {
                        origin: entity.origin,
                        angles: entity.angles,
                        frame: entity.frame,
                        syncbase: entity.syncbase,
                    };
                    let start = sprites.vertices.len();
                    if let Some(image) =
                        model.draw(&sprite, scene.time, &eye, &mut sprites.vertices)
                    {
                        sprites.draws.push(SpriteDraw {
                            image: first.saturating_add(image),
                            first: u32_of(start),
                            count: u32_of(sprites.vertices.len().saturating_sub(start)),
                            oriented: model.is_oriented(),
                        });
                    }
                }
                Some(Model::Submodel(n)) => {
                    self.place(&mut placements, *n, entity);
                }
                Some(Model::External) => {
                    if let Some(&(_, n)) = self.instances.iter().find(|(e, _)| *e == entity.entity)
                    {
                        self.place(&mut placements, n, entity);
                    }
                }
                Some(Model::None) | None => {}
            }
        }
        // R_DrawViewModel (REND-4.11).
        let mut weapon = ModelFrame::default();
        if let Some(drawn @ (Model::Alias(..) | Model::Md5(..))) =
            self.models.get(refdef.weapon.model)
        {
            let lit = entity_light(
                light(refdef.weapon.origin),
                AliasKind::ViewModel,
                refdef.weapon.origin,
                &scene.lights,
            );
            let alias = AliasEntity {
                origin: refdef.weapon.origin,
                angles: refdef.weapon.angles,
                frame: index_of(refdef.weapon.frame),
                skin: 0,
                light: lit,
                blend: scene.weapon_poses,
                kind: AliasKind::ViewModel,
            };
            drawn.draw_alias(&alias, scene.time, &mut weapon);
        }
        let screen = self.screen.map(f32_of_i32);
        let mut flat = FlatFrame::default();
        // R_PolyBlend: `if (!v_blend[3]) return`.
        if scene.blend[3] != 0.0 {
            quad(
                &mut flat,
                None,
                true,
                [0.0, 0.0],
                screen,
                [[0.0, 0.0], [1.0, 1.0]],
                scene.blend,
            );
        }
        self.draw_hud(hud, &mut flat);
        let mut view = View::new(
            // REND-3.4, REND-13.6: `fov` across, widened as QuakeSpasm's `fov_adapt` widens it.
            Camera::clip_from_view(
                refdef.origin,
                refdef.angles,
                mesh.look.fov_x(FOV_X, aspect),
                aspect,
            ),
            refdef.origin,
            scene.time,
        );
        view.fog = self.fog.at(scene.time);
        view.water_alpha = self.water_alpha;
        view.skyfog = self.skyfog;
        view.screen = screen;
        MapCallback {
            view,
            models,
            weapon,
            sprites,
            particles: particle_triangles(&scene.particles, &self.palette, &eye),
            flat,
            placements,
            lightmap,
            player_skins,
        }
    }

    /// A brush model at its entity (REND-1.9, REND-1.12).
    fn place(&self, placements: &mut [Option<[[f32; 4]; 4]>], n: usize, entity: &SceneEntity) {
        let Some(slot) = placements.get_mut(n) else {
            return;
        };
        let placed = Placement {
            model: n,
            origin: entity.origin,
            angles: entity.angles,
        };
        let placed = if self.zfix { placed.zfixed() } else { placed };
        *slot = Some(placed.matrix());
    }

    /// The status bar, the overlay and the centre print as quads (HUD-1 to HUD-3).
    fn draw_hud(&self, hud: &Hud, flat: &mut FlatFrame) {
        let [width, height] = self.screen;
        // `Sbar_DrawPic`: `x + ((vid.width - 320)>>1)` unless deathmatch, `y + (vid.height -
        // SBAR_HEIGHT)` (HUD-1); the deathmatch overlay centres too (HUD-2.2).
        let centring = width.saturating_sub(BAR_WIDTH) >> 1;
        for Placed { anchor, item } in hud.bar.iter().chain(&hud.overlay) {
            let (dx, dy) = match anchor {
                Anchor::Bar => (
                    if hud.deathmatch { 0 } else { centring },
                    height.saturating_sub(HUD_SBAR_HEIGHT),
                ),
                Anchor::Screen => (0, 0),
                Anchor::Centered => (centring, 0),
            };
            self.draw_item(item, dx, dy, flat);
        }
        for item in &hud.center {
            self.draw_item(item, 0, 0, flat);
        }
        // `SCR_DrawConsole` after `Sbar_Draw`, then `M_Draw` (CON-3).
        for Placed { item, .. } in hud.console.iter().chain(&hud.menu) {
            self.draw_item(item, 0, 0, flat);
        }
    }

    /// One item at its anchor's offset.
    fn draw_item(&self, item: &HudItem, dx: i32, dy: i32, flat: &mut FlatFrame) {
        match item {
            HudItem::Pic { name, x, y } => {
                if let Some((pic, image)) = self.pics.get(name) {
                    let at = [x.saturating_add(dx), y.saturating_add(dy)].map(f32_of_i32);
                    let size = [f32_of_u32(image.width), f32_of_u32(image.height)];
                    quad(
                        flat,
                        Some(pic),
                        false,
                        at,
                        size,
                        [[0.0, 0.0], [1.0, 1.0]],
                        WHITE,
                    );
                }
            }
            HudItem::Lmp { path, x, y } => {
                if let Some((pic, image)) = self.pics.get(path) {
                    // `Draw_Pic ((vid.width - pic->width)/2, ...)` when centred.
                    let width = i32::try_from(image.width).unwrap_or(0);
                    let x =
                        x.unwrap_or_else(|| self.screen[0].saturating_sub(width).wrapping_div(2));
                    let at = [x.saturating_add(dx), y.saturating_add(dy)].map(f32_of_i32);
                    let size = [f32_of_u32(image.width), f32_of_u32(image.height)];
                    quad(
                        flat,
                        Some(pic),
                        false,
                        at,
                        size,
                        [[0.0, 0.0], [1.0, 1.0]],
                        WHITE,
                    );
                }
            }
            HudItem::Char { x, y, ch } => {
                // `Draw_Character`: the space is not drawn, nor a character above the screen.
                let y = y.saturating_add(dy);
                if *ch == SPACE || y <= CHAR_OFF_TOP {
                    return;
                }
                let Some((pic, _)) = self.pics.get(CONCHARS) else {
                    return;
                };
                let row = f32::from(ch >> 4) * CHAR_CELL;
                let column = f32::from(ch & 15) * CHAR_CELL;
                let at = [f32_of_i32(x.saturating_add(dx)), f32_of_i32(y)];
                let texture = [[column, row], [column + CHAR_CELL, row + CHAR_CELL]];
                quad(flat, Some(pic), false, at, [CHAR_SIDE; 2], texture, WHITE);
            }
            HudItem::ConsoleBack { lines, height } => {
                // `Draw_ConsoleBackground`: opaque below three quarters, else `Draw_AlphaPic`
                // at `1.2 * lines / y` (CON-3.2).
                if let Some((pic, _)) = self.pics.get(CONBACK) {
                    let three_quarters = height.saturating_mul(3) >> 2;
                    let alpha = if *lines > three_quarters {
                        1.0
                    } else {
                        (1.2 * f32_of_i32(*lines) / f32_of_i32(three_quarters.max(1))).min(1.0)
                    };
                    let at = [0.0, f32_of_i32(lines.saturating_sub(*height))];
                    let size = [f32_of_i32(self.screen[0]), f32_of_i32(*height)];
                    // `Draw_AlphaPic` blends with the alpha test off; `Draw_Pic` tests it.
                    let translucent = *lines <= three_quarters;
                    quad(
                        flat,
                        Some(pic),
                        translucent,
                        at,
                        size,
                        [[0.0, 0.0], [1.0, 1.0]],
                        [1.0, 1.0, 1.0, alpha],
                    );
                }
            }
            HudItem::Fade => {
                // `Draw_FadeScreen`: "glColor4f (0, 0, 0, 0.8)" over the whole screen, blended.
                let size = [self.screen[0], self.screen[1]].map(f32_of_i32);
                quad(
                    flat,
                    None,
                    true,
                    [0.0, 0.0],
                    size,
                    [[0.0, 0.0], [1.0, 1.0]],
                    [0.0, 0.0, 0.0, 0.8],
                );
            }
            HudItem::Fill { x, y, w, h, color } => {
                // `Draw_Fill`: `host_basepal[c*3]/255.0`, the colour before the gamma.
                let [r, g, b, _] = self.palette.rgba(u8::try_from(*color).unwrap_or(0));
                let color = [
                    f32::from(r) / 255.0,
                    f32::from(g) / 255.0,
                    f32::from(b) / 255.0,
                    1.0,
                ];
                let at = [x.saturating_add(dx), y.saturating_add(dy)].map(f32_of_i32);
                quad(
                    flat,
                    None,
                    false,
                    at,
                    [*w, *h].map(f32_of_i32),
                    [[0.0, 0.0], [1.0, 1.0]],
                    color,
                );
            }
        }
    }
}

impl Model {
    /// An alias or an MD5 model's triangles for `entity` into `out`, a run for each skin
    /// (REND-4, REND-13); nothing for another model.
    fn draw_alias(&self, entity: &AliasEntity, time: f32, out: &mut ModelFrame) {
        match self {
            Self::Alias(model, first) => draw_alias(model, *first, entity, time, out),
            Self::Md5(model, first) => {
                let mut start = out.vertices.len();
                for (count, image) in model.draw(entity, f64::from(time), &mut out.vertices) {
                    out.draws.push(AliasDraw {
                        skin: first.saturating_add(image),
                        first: u32_of(start),
                        count: u32_of(count),
                    });
                    start = start.saturating_add(count);
                }
            }
            Self::Sprite(..) | Self::Submodel(_) | Self::External | Self::None => {}
        }
    }
}

/// An entity's alias triangles into `out`, drawn with its skin (REND-4).
fn draw_alias(
    model: &AliasModel,
    first_skin: usize,
    entity: &AliasEntity,
    time: f32,
    out: &mut ModelFrame,
) {
    let start = out.vertices.len();
    let image = model.draw(entity, f64::from(time), &mut out.vertices);
    out.draws.push(AliasDraw {
        skin: first_skin.saturating_add(image),
        first: u32_of(start),
        count: u32_of(out.vertices.len().saturating_sub(start)),
    });
}

/// A quad of the 2D layer at `at` of `size`, its texture from `texture[0]` to `texture[1]`,
/// joined to the last draw when it uses the same picture and blending.
fn quad(
    flat: &mut FlatFrame,
    pic: Option<usize>,
    blend: bool,
    at: [f32; 2],
    size: [f32; 2],
    texture: [[f32; 2]; 2],
    color: [f32; 4],
) {
    let [x, y] = at;
    let [w, h] = size;
    let [[s0, t0], [s1, t1]] = texture;
    let corner = |position, texture| FlatVertex {
        position,
        texture,
        color,
    };
    let corners = [
        corner([x, y], [s0, t0]),
        corner([x + w, y], [s1, t0]),
        corner([x + w, y + h], [s1, t1]),
        corner([x, y + h], [s0, t1]),
    ];
    let first = u32_of(flat.vertices.len());
    flat.vertices.extend([
        corners[0], corners[1], corners[2], corners[0], corners[2], corners[3],
    ]);
    match flat.draws.last_mut() {
        Some(last) if last.pic == pic && last.blend == blend => {
            last.count = last.count.saturating_add(6);
        }
        _ => flat.draws.push(FlatDraw {
            pic,
            blend,
            first,
            count: 6,
        }),
    }
}

/// The fog and its fade, `Fog_Update`, `Fog_GetColor` and `Fog_GetDensity` (REND-10.2,
/// REND-10.3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogState {
    /// The values faded to.
    fog: Fog,
    /// The values faded from.
    old: Fog,
    /// `fade_time`: how long the fade takes.
    fade_time: f32,
    /// `fade_done`: when it ends.
    fade_done: f32,
}

impl FogState {
    /// `fog` with no fade.
    #[must_use]
    pub const fn new(fog: Fog) -> Self {
        Self {
            fog,
            old: fog,
            fade_time: 0.0,
            fade_done: 0.0,
        }
    }

    /// `Fog_Update (density, red, green, blue, time)` at `now` (`cl.time`): with `time`
    /// above 0 a fade from the values now in force (a fade in progress from where it has
    /// got to) to `fog` over `time` seconds; with `time` 0, `fog` at once (REND-10.2).
    pub fn update(&mut self, fog: Fog, time: f32, now: f32) {
        if time > 0.0 {
            // "save previous settings for fade": from where a fade in progress is now.
            if self.fade_done > now {
                let reached = self.at_raw(now);
                self.old = reached;
            } else {
                self.old = self.fog;
            }
        }
        self.fog = fog;
        self.fade_time = time;
        self.fade_done = now + time;
    }

    /// The colour and density at `now`: while a fade is in progress, `f * old + (1 - f) *
    /// new` with `f = (fade_done - cl.time) / fade_time`; the colour's channels clamped to 0
    /// to 1 and rounded to the nearest 1/255 (REND-10.3).
    #[must_use]
    pub fn at(&self, now: f32) -> Fog {
        let mut fog = self.at_raw(now);
        for channel in &mut fog.color {
            // "find closest 24-bit RGB value, so solid-colored sky can match the fog perfectly"
            *channel = (channel.clamp(0.0, 1.0) * 255.0).round() / 255.0;
        }
        fog
    }

    /// The mix before the clamp and the rounding.
    fn at_raw(&self, now: f32) -> Fog {
        if self.fade_done > now {
            let f = (self.fade_done - now) / self.fade_time;
            let mut color = self.fog.color;
            for (channel, (old, new)) in color
                .iter_mut()
                .zip(self.old.color.into_iter().zip(self.fog.color))
            {
                *channel = f * old + (1.0 - f) * new;
            }
            Fog {
                color,
                density: f * self.old.density + (1.0 - f) * self.fog.density,
            }
        } else {
            self.fog
        }
    }
}

/// `Fog_ParseWorldspawn`: the worldspawn's `fog` key read with `sscanf ("%f %f %f %f")`
/// as density, red, green and blue, a shorter value setting only the leading fields; no
/// fog and grey without it (REND-10.1).
#[must_use]
pub fn fog_parse_worldspawn(entities: &[u8]) -> Fog {
    let mut fog = Fog::default();
    let text = String::from_utf8_lossy(entities);
    // The worldspawn is the first entity.
    let Some(block) = text.split('}').next() else {
        return fog;
    };
    let mut fields = block.split('"').skip(1).step_by(2);
    while let (Some(key), Some(value)) = (fields.next(), fields.next()) {
        if key != "fog" {
            continue;
        }
        let mut numbers = value
            .split_whitespace()
            .map_while(|n| n.parse::<f32>().ok());
        if let Some(density) = numbers.next() {
            fog.density = density;
        }
        for channel in &mut fog.color {
            match numbers.next() {
                Some(value) => *channel = value,
                None => break,
            }
        }
    }
    fog
}

/// A client's frame or skin as the index the renderer draws, toward zero; a negative one is
/// out of every model's range, which draws 0 (REND-4.2, REND-4.4).
fn index_of(value: i32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

fn u32_of(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a screen position in pixels, exact in f32"
)]
const fn f32_of_i32(value: i32) -> f32 {
    value as f32
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a picture's size in pixels, exact in f32"
)]
const fn f32_of_u32(value: u32) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests;
