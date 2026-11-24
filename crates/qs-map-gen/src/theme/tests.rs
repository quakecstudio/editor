// SPDX-License-Identifier: GPL-3.0-or-later

//! The themes against the way the games' own levels use their textures.

use super::THEMES;

/// How a texture is used in the game's own levels: the share of its area, in percent, on faces
/// that look up, stand upright and look down. The rows are what `scripts/measure-textures.py`
/// prints for id's `pak0.pak` and `pak1.pak` (the id themes) and LibreQuake's lite release's
/// (the others), from the world faces of every level, a face looking up or down when its normal
/// is within about 45 degrees of upright.
#[derive(Debug)]
struct Use {
    floor: u8,
    wall: u8,
    ceiling: u8,
}

/// The share past which a texture is mostly on one kind of face.
const HALF: u8 = 50;
/// The least share of a ceiling's area its levels put on ceilings.
const THIRD: u8 = 33;

/// The kind of face a theme paints with a texture.
#[derive(Clone, Copy, Debug)]
enum Face {
    Floor,
    Ceiling,
    Upright,
}

impl Use {
    /// The shares on floors, walls and ceilings.
    const fn of(floor: u8, wall: u8, ceiling: u8) -> Self {
        Self {
            floor,
            wall,
            ceiling,
        }
    }

    /// Whether the game puts the texture on such faces: a floor's mostly on floors, an upright
    /// face's mostly on walls, and a ceiling's on ceilings at least a third of the time and no
    /// less than on floors, since levels roof outdoor rooms with sky and few textures are
    /// mostly ceilings.
    fn fits(&self, face: Face) -> bool {
        match face {
            Face::Floor => self.floor > HALF,
            Face::Ceiling => self.ceiling >= THIRD && self.ceiling >= self.floor,
            Face::Upright => self.wall > HALF,
        }
    }
}

const MEASURED: &[(&str, Use)] = &[
    ("afloor1_4", Use::of(100, 0, 0)),
    ("bricka2_4", Use::of(2, 90, 9)),
    ("ceiling5", Use::of(0, 3, 97)),
    ("city2_5", Use::of(1, 80, 19)),
    ("city4_2", Use::of(89, 8, 3)),
    ("city4_5", Use::of(84, 6, 10)),
    ("city4_7", Use::of(83, 6, 11)),
    ("city6_3", Use::of(0, 1, 99)),
    ("city6_8", Use::of(3, 92, 5)),
    ("citya1_1", Use::of(2, 25, 73)),
    ("comp1_3", Use::of(8, 92, 0)),
    ("cop1_3", Use::of(0, 16, 83)),
    ("cop2_3", Use::of(4, 81, 15)),
    ("grk_bl_flt1", Use::of(98, 1, 1)),
    ("grk_bl_trim1", Use::of(1, 99, 0)),
    ("grk_brk15", Use::of(0, 100, 0)),
    ("grk_brk15_f", Use::of(12, 4, 84)),
    ("grk_brk17", Use::of(3, 91, 6)),
    ("grk_flr4_2", Use::of(100, 0, 0)),
    ("grk_flr5_3", Use::of(100, 0, 0)),
    ("grk_tile2_1", Use::of(0, 0, 100)),
    ("grk_trim1", Use::of(0, 100, 0)),
    ("grk_trim2", Use::of(0, 100, 0)),
    ("grk_wall1", Use::of(0, 100, 0)),
    ("grk_wall3", Use::of(0, 100, 0)),
    ("ground1_6", Use::of(94, 6, 0)),
    ("lgmetal", Use::of(6, 81, 13)),
    ("may_blok1_1", Use::of(0, 80, 20)),
    ("may_blok2_1", Use::of(0, 52, 48)),
    ("may_bnd1_2", Use::of(4, 78, 17)),
    ("may_brk1_1", Use::of(0, 83, 17)),
    ("may_drt1_2", Use::of(100, 0, 0)),
    ("may_flr1_1", Use::of(88, 1, 11)),
    ("may_flr1_2", Use::of(67, 0, 33)),
    ("may_trm1_1", Use::of(3, 95, 2)),
    ("may_trm1_2", Use::of(1, 98, 0)),
    ("may_wall1_2", Use::of(0, 99, 0)),
    ("med_cmet4", Use::of(22, 55, 22)),
    ("med_cobstn1_2", Use::of(100, 0, 0)),
    ("med_csl_brk10b", Use::of(1, 87, 13)),
    ("med_csl_brk11", Use::of(0, 99, 1)),
    ("med_csl_brk12_f", Use::of(0, 1, 99)),
    ("med_csl_brk14", Use::of(0, 100, 0)),
    ("med_csl_flr1_1", Use::of(0, 0, 100)),
    ("med_csl_flr2_2", Use::of(100, 0, 0)),
    ("med_csl_flr4_1", Use::of(100, 0, 0)),
    ("med_csl_flr4_4", Use::of(62, 0, 38)),
    ("med_csl_trm1", Use::of(0, 99, 1)),
    ("med_dbrick6b", Use::of(24, 73, 3)),
    ("med_etrim1", Use::of(20, 60, 20)),
    ("med_wood7", Use::of(50, 0, 50)),
    ("metal1_4", Use::of(15, 60, 26)),
    ("metal1_7", Use::of(9, 88, 3)),
    ("metal2_2", Use::of(6, 16, 78)),
    ("metal2_4", Use::of(7, 83, 11)),
    ("metal4_2", Use::of(0, 99, 0)),
    ("metal4_3", Use::of(0, 97, 2)),
    ("metal4_4", Use::of(4, 92, 5)),
    ("metal5_2", Use::of(75, 7, 18)),
    ("metal5_4", Use::of(95, 1, 4)),
    ("metal5_6", Use::of(2, 94, 4)),
    ("metal6_1", Use::of(0, 99, 1)),
    ("metflor2_1", Use::of(67, 25, 9)),
    ("mmetal1_1", Use::of(98, 2, 0)),
    ("rune1_1", Use::of(0, 100, 0)),
    ("sfloor4_1", Use::of(60, 26, 14)),
    ("sfloor4_6", Use::of(66, 34, 1)),
    ("stone1_7", Use::of(4, 10, 86)),
    ("t_flor1a", Use::of(100, 0, 0)),
    ("t_indent01", Use::of(0, 0, 100)),
    ("t_metalsheeta", Use::of(10, 79, 11)),
    ("t_tech01", Use::of(0, 13, 87)),
    ("t_tech06", Use::of(0, 0, 100)),
    ("t_trim1aa", Use::of(0, 92, 8)),
    ("t_trim1d", Use::of(2, 97, 1)),
    ("t_trim2ba", Use::of(2, 80, 18)),
    ("t_wall05", Use::of(0, 93, 7)),
    ("t_wall1ba", Use::of(0, 100, 0)),
    ("t_wall2a", Use::of(0, 100, 0)),
    ("t_wall3a", Use::of(0, 100, 0)),
    ("t_wall3aa", Use::of(0, 100, 0)),
    ("tech01_1", Use::of(0, 19, 81)),
    ("tech03_1", Use::of(0, 100, 0)),
    ("tech04_1", Use::of(10, 62, 28)),
    ("tech04_3", Use::of(6, 75, 20)),
    ("tech06_1", Use::of(0, 99, 1)),
    ("tech08_1", Use::of(0, 100, 0)),
    ("tech08_2", Use::of(0, 100, 0)),
    ("tek_flr3", Use::of(92, 0, 8)),
    ("twall2_3", Use::of(0, 100, 0)),
    ("wall14_5", Use::of(6, 85, 10)),
    ("wbrick1_5", Use::of(6, 85, 9)),
    ("wizmet1_1", Use::of(18, 76, 7)),
    ("wizmet1_2", Use::of(11, 67, 22)),
    ("wizwood1_8", Use::of(4, 12, 84)),
    ("wmet1_1", Use::of(20, 62, 18)),
    ("wmet3_1", Use::of(0, 100, 0)),
    ("woodflr1_5", Use::of(73, 27, 0)),
];

// No spec statement: L-74, every theme paints a floor, a ceiling and a wall with a texture the
// game's own levels put on such faces, so no inlay or light panel is tiled across a room
#[test]
fn every_theme_paints_each_face_as_its_game_does() {
    let mut wrong = Vec::new();
    for theme in THEMES {
        for (face, names) in [
            (Face::Upright, theme.wall),
            (Face::Floor, theme.floor),
            (Face::Ceiling, theme.ceiling),
            (Face::Upright, theme.trim),
            (Face::Upright, theme.support),
        ] {
            for name in names {
                let used = MEASURED.iter().find(|(measured, _)| measured == name);
                if !used.is_some_and(|(_, used)| used.fits(face)) {
                    wrong.push(format!("{} {face:?} {name}: {used:?}", theme.name));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

// No spec statement: L-74, a theme's supports are no wall or trim of it, so a pillar or a beam
// stands out from the wall behind it and the bands around it
#[test]
fn a_themes_supports_are_none_of_its_walls_or_trims() {
    for theme in THEMES {
        for support in theme.support {
            assert!(
                !theme.wall.contains(support) && !theme.trim.contains(support),
                "{}: {support}",
                theme.name
            );
        }
    }
}
