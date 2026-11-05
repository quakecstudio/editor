// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or index is a failed test; the WAD's offsets are small"
)]

use qs_client::{Anchor, DynamicLight, Hud, HudItem, Placed, Scene, SceneEntity, SceneParticle};
use qs_wad::{WAD_MAGIC, WAD_TYP_QPIC};

use super::{FogState, Model, Pics, Play, PlayError, fog_parse_worldspawn};
use crate::alias::tests::{MdlSpec, SkinSpec, mdl};
use crate::gpu::{FlatDraw, Fog};
use crate::md5::tests::{Md5Spec, md5mesh};
use crate::mesh::tests::{MapSpec, bsp, palette, samples};
use crate::mesh::{Image, MapMesh};
use crate::picture::tests::solid;

/// A WAD2 of picture lumps: each a name, a size and one palette index for every pixel.
fn wad(lumps: &[(&str, [i32; 2], u8)]) -> Vec<u8> {
    let mut out = WAD_MAGIC.to_vec();
    out.extend(i32::try_from(lumps.len()).unwrap().to_le_bytes());
    let mut data = Vec::new();
    let mut entries = Vec::new();
    for (name, [width, height], pixel) in lumps {
        let filepos = 12 + data.len();
        if *name == "conchars" {
            data.extend(core::iter::repeat_n(*pixel, 128 * 128));
        } else {
            data.extend(width.to_le_bytes());
            data.extend(height.to_le_bytes());
            data.extend(core::iter::repeat_n(
                *pixel,
                usize::try_from(width * height).unwrap(),
            ));
        }
        let size = 12 + data.len() - filepos;
        let mut entry = Vec::new();
        for v in [filepos, size, size] {
            entry.extend(i32::try_from(v).unwrap().to_le_bytes());
        }
        entry.extend([WAD_TYP_QPIC, 0, 0, 0]);
        let mut lump_name = [0_u8; 16];
        lump_name[..name.len()].copy_from_slice(name.as_bytes());
        entry.extend(lump_name);
        entries.push(entry);
    }
    out.extend(i32::try_from(12 + data.len()).unwrap().to_le_bytes());
    out.extend(data);
    for entry in entries {
        out.extend(entry);
    }
    out
}

fn pics() -> Pics {
    let mut pics = Pics::new();
    pics.add_wad(
        &wad(&[
            ("sbar", [320, 24], 3),
            ("num_0", [24, 24], 255),
            ("conchars", [0, 0], 1),
            ("other", [2, 2], 5),
        ]),
        &palette(),
    )
    .unwrap();
    let mut lmp = 100_i32.to_le_bytes().to_vec();
    lmp.extend(20_i32.to_le_bytes());
    lmp.extend(core::iter::repeat_n(2_u8, 2000));
    pics.add_lmp("gfx/ranking.lmp", &lmp, &palette()).unwrap();
    pics
}

fn play() -> Play {
    Play::new(palette(), pics(), b"")
}

fn hud(anchor: Anchor, items: Vec<HudItem>) -> Hud {
    Hud {
        bar: items
            .into_iter()
            .map(|item| Placed { anchor, item })
            .collect(),
        ..Hud::default()
    }
}

/// A quad's top-left corner and its size.
type Quad = ([f32; 2], [f32; 2]);

/// `play`'s 2D layer for `hud` over a frame with no blend: each draw with its quads.
fn layer(play: &mut Play, hud: &Hud) -> Vec<(FlatDraw, Vec<Quad>)> {
    let lighting = samples(&[50]);
    let bsp = bsp(&MapSpec::default(), &lighting);
    let mut mesh = MapMesh::build(&bsp, &palette()).unwrap();
    let callback = play.frame(&Scene::default(), hud, &bsp, &mut mesh, 1.0);
    let flat = callback.flat;
    flat.draws
        .iter()
        .map(|draw| {
            let first = usize::try_from(draw.first).unwrap();
            let count = usize::try_from(draw.count).unwrap();
            let quads = flat.vertices[first..first + count]
                .chunks(6)
                .map(|quad| {
                    let [x, y] = quad[0].position;
                    let [x2, y2] = quad[2].position;
                    ([x, y], [x2 - x, y2 - y])
                })
                .collect();
            (*draw, quads)
        })
        .collect()
}

#[test]
fn pictures_come_from_the_wad_and_the_lmp_files() {
    let pics = pics();
    let (index, sbar) = pics.get("sbar").unwrap();
    assert_eq!(index, 0);
    assert_eq!((sbar.width, sbar.height), (320, 24));
    assert_eq!(&sbar.rgba[..4], [3, 6, 9, 255]);
    // Index 255 is transparent; `conchars` keeps index 1 and makes 0 transparent.
    assert_eq!(&pics.get("num_0").unwrap().1.rgba[..4], [0, 0, 0, 0]);
    let (_, chars) = pics.get("conchars").unwrap();
    assert_eq!((chars.width, chars.height), (128, 128));
    assert_eq!(&chars.rgba[..4], [1, 2, 3, 255]);
    let (_, ranking) = pics.get("gfx/ranking.lmp").unwrap();
    assert_eq!((ranking.width, ranking.height), (100, 20));
    assert_eq!(pics.get("missing"), None);
    assert_eq!(pics.images().len(), 5);
    // A replaced picture keeps its index.
    let mut pics = pics;
    pics.add(
        "sbar",
        Image {
            width: 1,
            height: 1,
            rgba: vec![9; 4],
        },
    );
    assert_eq!(pics.get("sbar").unwrap().0, 0);
    assert_eq!(pics.images().len(), 5);
    let mut bad = Pics::new();
    assert!(matches!(
        bad.add_wad(b"WAD3", &palette()),
        Err(PlayError::Wad(_))
    ));
    assert!(matches!(
        bad.add_lmp("x.lmp", b"", &palette()),
        Err(PlayError::Wad(ref m)) if m.starts_with("x.lmp: ")
    ));
}

// spec WAD-4.2: the console characters' index 0 is transparent
#[test]
fn conchars_index_0_is_transparent() {
    let mut pics = Pics::new();
    pics.add_wad(&wad(&[("conchars", [0, 0], 0)]), &palette())
        .unwrap();
    let (_, chars) = pics.get("conchars").unwrap();
    assert!(chars.rgba.iter().all(|&b| b == 0));
}

// spec HUD-1.1: the bar's pictures are drawn at `x + (vid.width - 320) / 2` and
// `y + vid.height - SBAR_HEIGHT`, from the screen's left in deathmatch; a deathmatch overlay
// is centred too (HUD-2.2)
#[test]
fn bar_items_are_centred_at_the_bottom() {
    let mut play = play();
    let sbar = HudItem::Pic {
        name: "sbar".to_owned(),
        x: 0,
        y: 0,
    };
    let num = HudItem::Pic {
        name: "num_0".to_owned(),
        x: 24,
        y: -24,
    };
    let bar = layer(
        &mut play,
        &hud(Anchor::Bar, vec![sbar.clone(), num.clone()]),
    );
    assert_eq!(bar.len(), 2, "two pictures, two draws");
    assert_eq!(bar[0].1, [([160.0, 456.0], [320.0, 24.0])]);
    assert_eq!(bar[0].0.pic, Some(0));
    assert!(!bar[0].0.blend);
    assert_eq!(bar[1].1, [([184.0, 432.0], [24.0, 24.0])]);
    let deathmatch = Hud {
        deathmatch: true,
        ..hud(Anchor::Bar, vec![sbar.clone()])
    };
    assert_eq!(
        layer(&mut play, &deathmatch)[0].1,
        [([0.0, 456.0], [320.0, 24.0])]
    );
    assert_eq!(
        layer(&mut play, &hud(Anchor::Centered, vec![sbar.clone()]))[0].1,
        [([160.0, 0.0], [320.0, 24.0])]
    );
    assert_eq!(
        layer(&mut play, &hud(Anchor::Screen, vec![sbar.clone()]))[0].1,
        [([0.0, 0.0], [320.0, 24.0])]
    );
    // A picture that is not loaded draws nothing.
    let missing = HudItem::Pic {
        name: "nope".to_owned(),
        x: 0,
        y: 0,
    };
    assert!(layer(&mut play, &hud(Anchor::Bar, vec![missing])).is_empty());
    // Two pictures of one texture join one draw.
    let twice = layer(&mut play, &hud(Anchor::Bar, vec![sbar.clone(), sbar]));
    assert_eq!(twice.len(), 1);
    assert_eq!(twice[0].0.count, 12);
}

// spec HUD-2.3: a `.lmp` with no x is centred, `(vid.width - pic->width) / 2`
#[test]
fn lmp_pictures_centre_when_asked() {
    let mut play = play();
    let centred = HudItem::Lmp {
        path: "gfx/ranking.lmp".to_owned(),
        x: None,
        y: 8,
    };
    let placed = HudItem::Lmp {
        path: "gfx/ranking.lmp".to_owned(),
        x: Some(64),
        y: 24,
    };
    let out = layer(&mut play, &hud(Anchor::Screen, vec![centred, placed]));
    assert_eq!(
        out[0].1,
        [([270.0, 8.0], [100.0, 20.0]), ([64.0, 24.0], [100.0, 20.0])]
    );
}

// spec HUD-3.2: a character is 8 by 8 at its position from the 16 by 16 grid of `conchars`,
// a space and a character above the screen are not drawn
#[test]
fn characters_come_from_the_console_font() {
    let mut play = play();
    let ch = |x, y, ch| HudItem::Char { x, y, ch };
    let out = layer(
        &mut play,
        &hud(
            Anchor::Screen,
            vec![
                ch(10, 10, b'A'),
                ch(18, 10, b' '),
                ch(26, -8, b'B'),
                ch(26, -7, b'C'),
            ],
        ),
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0.pic, Some(2));
    assert_eq!(
        out[0].1,
        [([10.0, 10.0], [8.0, 8.0]), ([26.0, -7.0], [8.0, 8.0])]
    );
    // 'A' is 65: row 4, column 1 of the grid, each cell a 16th.
    let lighting = samples(&[50]);
    let bsp = bsp(&MapSpec::default(), &lighting);
    let mut mesh = MapMesh::build(&bsp, &palette()).unwrap();
    let callback = play.frame(
        &Scene::default(),
        &hud(Anchor::Screen, vec![ch(0, 0, b'A')]),
        &bsp,
        &mut mesh,
        1.0,
    );
    let v = &callback.flat.vertices;
    assert_eq!(v[0].texture, [0.0625, 0.25]);
    assert_eq!(v[2].texture, [0.125, 0.3125]);
}

// spec HUD-1.5: a fill is a rectangle of the palette colour, untextured
#[test]
fn fills_are_palette_colours() {
    let mut play = play();
    let out = layer(
        &mut play,
        &hud(
            Anchor::Screen,
            vec![HudItem::Fill {
                x: 10,
                y: 20,
                w: 28,
                h: 4,
                color: 10,
            }],
        ),
    );
    assert_eq!(out[0].0.pic, None);
    assert!(!out[0].0.blend);
    assert_eq!(out[0].1, [([10.0, 20.0], [28.0, 4.0])]);
    let lighting = samples(&[50]);
    let bsp = bsp(&MapSpec::default(), &lighting);
    let mut mesh = MapMesh::build(&bsp, &palette()).unwrap();
    let callback = play.frame(
        &Scene::default(),
        &hud(
            Anchor::Screen,
            vec![HudItem::Fill {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
                color: 10,
            }],
        ),
        &bsp,
        &mut mesh,
        1.0,
    );
    assert_eq!(
        callback.flat.vertices[0].color,
        [10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 1.0]
    );
}

// spec VIEW-4.6: the blend is drawn over the whole view before the bar, unless its alpha is 0
#[test]
fn the_view_blend_comes_first_when_it_has_alpha() {
    let mut play = play();
    let lighting = samples(&[50]);
    let bsp = bsp(&MapSpec::default(), &lighting);
    let mut mesh = MapMesh::build(&bsp, &palette()).unwrap();
    let hud = hud(
        Anchor::Screen,
        vec![HudItem::Pic {
            name: "sbar".to_owned(),
            x: 0,
            y: 0,
        }],
    );
    let scene = Scene {
        blend: [1.0, 0.5, 0.0, 0.3],
        ..Scene::default()
    };
    let callback = play.frame(&scene, &hud, &bsp, &mut mesh, 1.0);
    assert_eq!(callback.flat.draws.len(), 2);
    assert_eq!(callback.flat.draws[0].pic, None);
    assert!(callback.flat.draws[0].blend);
    let [x, y] = callback.flat.vertices[2].position;
    assert_eq!([x, y], [640.0, 480.0]);
    assert_eq!(callback.flat.vertices[0].color, [1.0, 0.5, 0.0, 0.3]);
    let none = play.frame(&Scene::default(), &hud, &bsp, &mut mesh, 1.0);
    assert_eq!(none.flat.draws.len(), 1);
}

// spec REND-7.1: the frame's lights relight the mesh; the particles and the world's placement
// are carried
#[test]
fn a_frame_carries_the_lights_the_particles_and_the_world() {
    let mut play = play();
    let lighting = samples(&[50]);
    let bsp = bsp(&MapSpec::default(), &lighting);
    let mut mesh = MapMesh::build(&bsp, &palette()).unwrap();
    let scene = Scene {
        lights: vec![DynamicLight {
            origin: [0.0, 0.0, 3.0],
            radius: 5.0,
            minlight: 0.0,
        }],
        particles: vec![SceneParticle {
            origin: [1.0, 2.0, 3.0],
            color: 4,
        }],
        entities: vec![SceneEntity {
            model: 7,
            ..SceneEntity::default()
        }],
        ..Scene::default()
    };
    let callback = play.frame(&scene, &Hud::default(), &bsp, &mut mesh, 1.0);
    assert!(callback.lightmap.is_some());
    assert_eq!(callback.particles.len(), 3);
    assert_eq!(callback.placements.len(), 1);
    assert!(callback.placements[0].is_some());
    assert!(callback.models.draws.is_empty(), "no model 7");
    assert!(callback.weapon.draws.is_empty());
    assert_eq!(callback.view.screen, [640.0, 480.0]);
    assert_eq!(callback.view.fog.density, 0.0);
    let again = play.frame(&Scene::default(), &Hud::default(), &bsp, &mut mesh, 1.0);
    assert!(again.lightmap.is_some(), "the light left");
    let still = play.frame(&Scene::default(), &Hud::default(), &bsp, &mut mesh, 1.0);
    assert!(still.lightmap.is_none());
}

// spec REND-10.1: no fog without the key; `fog` read as density then red, green, blue, a
// shorter value setting the leading fields
#[test]
fn worldspawn_fog_is_read_like_sscanf() {
    let none = fog_parse_worldspawn(b"{ \"classname\" \"worldspawn\" }");
    assert_eq!(none, Fog::default());
    assert_eq!(none.density, 0.0);
    assert_eq!(none.color, [0.3; 3]);
    let full = fog_parse_worldspawn(b"{ \"fog\" \"0.05 0.1 0.2 0.3\" }");
    assert_eq!(full.density, 0.05);
    assert_eq!(full.color, [0.1, 0.2, 0.3]);
    let short = fog_parse_worldspawn(b"{ \"fog\" \"0.5 0.9\" }");
    assert_eq!(short.density, 0.5);
    assert_eq!(short.color, [0.9, 0.3, 0.3]);
    let stopped = fog_parse_worldspawn(b"{ \"fog\" \"0.5 x 0.9\" }");
    assert_eq!((stopped.density, stopped.color), (0.5, [0.3; 3]));
    // Only the worldspawn, the first entity, is read.
    let later = fog_parse_worldspawn(b"{ \"classname\" \"worldspawn\" } { \"fog\" \"0.5\" }");
    assert_eq!(later.density, 0.0);
}

// spec REND-10.2: `Fog_Update` with a time fades from the values in force over that time; with
// 0 it sets them at once
// spec REND-10.3: the fade is `f * old + (1 - f) * new`, the colour clamped and rounded to a
// 255th
#[test]
fn fog_fades_over_its_time() {
    let fog = |density, grey| Fog {
        color: [grey; 3],
        density,
    };
    // What `at` returns: the grey rounded to a 255th.
    let seen = |density, grey: f32| Fog {
        color: [(grey * 255.0).round() / 255.0; 3],
        density,
    };
    let mut state = FogState::new(fog(0.0, 0.0));
    state.update(fog(1.0, 1.0), 0.0, 5.0);
    assert_eq!(state.at(5.0), seen(1.0, 1.0));
    state.update(fog(0.0, 0.0), 2.0, 10.0);
    assert_eq!(state.at(10.0), seen(1.0, 1.0));
    assert_eq!(state.at(11.0), seen(0.5, 0.5));
    assert_eq!(state.at(11.5), seen(0.25, 0.25));
    assert_eq!(state.at(12.0), seen(0.0, 0.0));
    assert_eq!(state.at(20.0), seen(0.0, 0.0));
    // A fade started during a fade starts from where it has got to.
    state.update(fog(1.0, 1.0), 2.0, 30.0);
    state.update(fog(0.0, 0.0), 1.0, 31.0);
    assert_eq!(state.at(31.0), seen(0.5, 0.5));
    assert_eq!(state.at(31.5), seen(0.25, 0.25));
    // The colour is clamped and rounded to the nearest 255th; the density is not.
    state.update(fog(2.0, 2.0), 0.0, 40.0);
    assert_eq!(state.at(40.0), seen(2.0, 1.0));
    state.update(fog(0.001, 0.001), 0.0, 40.0);
    assert_eq!(state.at(40.0), seen(0.001, 0.0));
    state.update(fog(0.0, 0.5), 0.0, 40.0);
    assert_eq!(state.at(40.0).color, [128.0 / 255.0; 3]);
}

// spec REND-4.13: a player entity wearing colours is drawn with its slot's translated skin,
// sent each frame and remade only when its model, skin or colours change; heads and
// non-players keep theirs
#[test]
fn players_wear_their_slots_translated_skins() {
    let mut play = play();
    let bytes = mdl(&MdlSpec {
        skins: vec![SkinSpec::Single([0].into_iter().chain([16; 15]).collect())],
        ..MdlSpec::default()
    });
    play.load_models(
        &[String::new(), "progs/player.mdl".to_owned()],
        &|name| name.ends_with(".mdl").then(|| bytes.clone()),
        &|name| name.ends_with(".mdl").then_some(0),
    )
    .unwrap();
    let lighting = samples(&[50]);
    let bsp = bsp(&MapSpec::default(), &lighting);
    let mut mesh = MapMesh::build(&bsp, &palette()).unwrap();
    let entity = |entity, colormap| SceneEntity {
        entity,
        model: 1,
        colormap,
        ..SceneEntity::default()
    };
    let mut scene = Scene {
        maxclients: 2,
        colors: vec![0, 0x40],
        entities: vec![entity(2, 2), entity(1, 0), entity(3, 3)],
        ..Scene::default()
    };
    let skins = play.skins.len();
    let first = play.frame(&scene, &Hud::default(), &bsp, &mut mesh, 1.0);
    let drawn: Vec<usize> = first.models.draws.iter().map(|d| d.skin).collect();
    assert_eq!(drawn, [skins + 1, 0, 0]);
    assert_eq!(first.player_skins.len(), 1);
    let worn = &first.player_skins[0];
    assert_eq!(worn.slot, 1);
    assert_eq!(
        (worn.key.model, worn.key.skin, worn.key.colors),
        (1, 0, 0x40)
    );
    assert_eq!(worn.image.rgba[4], palette().rgba(64)[0]);
    // Each frame sends the slot's skin, so a renderer made later or a frame not painted
    // catches up; unchanged colours send the same image, not a new translation.
    let again = play.frame(&scene, &Hud::default(), &bsp, &mut mesh, 1.0);
    assert_eq!(again.models.draws[0].skin, skins + 1);
    assert_eq!(again.player_skins.len(), 1);
    assert!(std::sync::Arc::ptr_eq(
        &again.player_skins[0].image,
        &worn.image
    ));
    scene.colors[1] = 0x50;
    let changed = play.frame(&scene, &Hud::default(), &bsp, &mut mesh, 1.0);
    assert_eq!(changed.player_skins[0].key.colors, 0x50);
    assert_eq!(changed.player_skins[0].image.rgba[4], palette().rgba(80)[0]);
}

// spec REND-8.1: the worldspawn's `skyfog` is the fog's share of the sky
// spec REND-8.2: the skybox the worldspawn names is read through the game's files and given
// to the renderer; a map naming none, or one with no face, keeps the layers
#[test]
fn the_worldspawn_sets_the_sky() {
    let read = |name: &str| (name == "gfx/env/cloudsdn.tga").then(|| solid([1, 1], [1; 4]));
    let mut named = Play::new(palette(), pics(), br#"{ "sky" "clouds" "skyfog" "0.2" }"#);
    assert_eq!(named.skyfog, 0.2);
    assert!(named.assets().skybox.is_none(), "not read yet");
    named.load_sky(&read);
    assert_eq!(named.assets().skybox.unwrap().size(), [1, 1]);
    let mut missing = Play::new(palette(), pics(), br#"{ "sky" "gone" }"#);
    missing.load_sky(&read);
    assert!(missing.assets().skybox.is_none());
    let mut plain = play();
    assert_eq!(plain.skyfog, 0.5);
    plain.load_sky(&read);
    assert!(plain.assets().skybox.is_none());
}

// spec REND-12.1: with MD5 models on, an `.mdl` is drawn as `<stem>.md5mesh` when it is
// found and the `.mdl` is not on an earlier search-path element; off, or with no MD5
// model, the `.mdl` is drawn
#[test]
fn md5_models_replace_their_mdl() {
    let alias = mdl(&MdlSpec::default());
    let mesh = md5mesh(&Md5Spec {
        frames: None,
        ..Md5Spec::default()
    });
    let names = [String::new(), "progs/ogre.mdl".to_owned()];
    let kind = |md5_models: bool, mdl_rank: Option<usize>, md5_rank: Option<usize>| {
        let mut play = play();
        play.md5_models = md5_models;
        let read = |name: &str| match name {
            "progs/ogre.mdl" => mdl_rank.map(|_| alias.clone()),
            "progs/ogre.md5mesh" => md5_rank.map(|_| mesh.clone()),
            _ => None,
        };
        let rank = |name: &str| match name {
            "progs/ogre.mdl" => mdl_rank,
            "progs/ogre.md5mesh" => md5_rank,
            _ => None,
        };
        play.load_models(&names, &read, &rank).unwrap();
        match &play.models[1] {
            Model::Alias(..) => "mdl",
            Model::Md5(..) => "md5",
            _ => "other",
        }
    };
    assert_eq!(kind(true, Some(1), Some(1)), "md5", "the same element");
    assert_eq!(kind(true, Some(2), Some(1)), "md5", "the MD5 earlier");
    assert_eq!(kind(true, Some(0), Some(1)), "mdl", "the .mdl earlier");
    assert_eq!(kind(true, None, Some(0)), "md5", "no .mdl");
    assert_eq!(kind(true, Some(0), None), "mdl", "no MD5");
    assert_eq!(kind(false, Some(1), Some(1)), "mdl", "MD5 models off");
}

// spec REND-12.3: an MD5 entity is drawn as an alias model, a run per part with its skin
#[test]
fn md5_entities_are_drawn_with_their_skins() {
    let mut play = play();
    let mesh = md5mesh(&Md5Spec {
        frames: None,
        ..Md5Spec::default()
    });
    play.load_models(
        &[String::new(), "progs/ogre.mdl".to_owned()],
        &|name| (name == "progs/ogre.md5mesh").then(|| mesh.clone()),
        &|name| (name == "progs/ogre.md5mesh").then_some(0),
    )
    .unwrap();
    assert_eq!(play.skins.len(), 1, "the white skin of a part with none");
    let lighting = samples(&[50]);
    let bsp = bsp(&MapSpec::default(), &lighting);
    let mut mesh = MapMesh::build(&bsp, &palette()).unwrap();
    let scene = Scene {
        entities: vec![SceneEntity {
            entity: 5,
            model: 1,
            ..SceneEntity::default()
        }],
        ..Scene::default()
    };
    let callback = play.frame(&scene, &Hud::default(), &bsp, &mut mesh, 1.0);
    assert_eq!(callback.models.draws.len(), 1);
    assert_eq!(callback.models.draws[0].skin, 0);
    assert_eq!(callback.models.draws[0].count, 3);
    assert_eq!(callback.models.vertices.len(), 3);
}

// spec REND-12.1: an MD5 model that does not read leaves its `.mdl`; with no `.mdl` it is an
// error naming the file
#[test]
fn an_md5_model_that_does_not_read_leaves_its_mdl() {
    let alias = mdl(&MdlSpec::default());
    let load = |with_mdl: bool| {
        let mut play = play();
        let read = |name: &str| match name {
            "progs/ogre.mdl" => with_mdl.then(|| alias.clone()),
            "progs/ogre.md5mesh" => Some(b"MD5V broken".to_vec()),
            _ => None,
        };
        let names = [String::new(), "progs/ogre.mdl".to_owned()];
        play.load_models(&names, &read, &|_| Some(0))
            .map(|()| matches!(play.models[1], Model::Alias(..)))
    };
    assert_eq!(load(true), Ok(true));
    assert!(
        matches!(load(false), Err(PlayError::Model(ref m)) if m.starts_with("progs/ogre.md5mesh"))
    );
}
