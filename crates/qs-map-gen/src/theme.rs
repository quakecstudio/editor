// SPDX-License-Identifier: GPL-3.0-or-later

//! The textures a level is painted with: a theme's names for each part of a room, taken from
//! the names the game has. The themes name textures of id's levels and of LibreQuake's; a
//! game with neither is painted from whatever plain textures it has.

use std::collections::BTreeSet;

use qs_map::{Surface, TexDef};

use crate::GenError;
use crate::rng::Rng;
use crate::rooms::RoomPart;

/// The `worldtype` of a level, which picks the look of its keys (id's `world.qc`:
/// "0: medieval 1: metal 2: base").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorldType {
    /// `0`.
    Medieval,
    /// `1`.
    Metal,
    /// `2`.
    Base,
}

impl WorldType {
    /// Whether its wall lights are lamps: a base's are electric, where a castle's and a
    /// temple's are torches (`misc.qc`'s torches are its medieval and runic levels').
    pub(crate) const fn lit_by_lamps(self) -> bool {
        match self {
            Self::Base => true,
            Self::Medieval | Self::Metal => false,
        }
    }

    /// The key's value.
    pub(crate) const fn value(self) -> &'static str {
        match self {
            Self::Medieval => "0",
            Self::Metal => "1",
            Self::Base => "2",
        }
    }
}

/// A theme: the names it would paint each part with, the first choices first.
struct Theme {
    name: &'static str,
    world: WorldType,
    wall: &'static [&'static str],
    floor: &'static [&'static str],
    ceiling: &'static [&'static str],
    trim: &'static [&'static str],
    door: &'static [&'static str],
    /// The light textures its lamps are, from id's and LibreQuake's levels, each with its
    /// width and height: every one is square.
    fixture: &'static [(&'static str, f64)],
    /// What holds its rooms up: id's and LibreQuake's structural metals, 64 square.
    support: &'static [&'static str],
}

/// The themes, of id's levels and then of LibreQuake's.
const THEMES: &[Theme] = &[
    Theme {
        name: "base",
        world: WorldType::Base,
        wall: &["tech04_1", "tech04_3", "tech06_1", "tech08_2", "twall2_3"],
        floor: &["sfloor4_1", "sfloor4_6", "afloor1_4", "metflor2_1"],
        // Not ceiling4: 192 square, one tile is a whole circle, too busy over a room.
        ceiling: &["ceiling5", "tech01_1"],
        trim: &["tech03_1", "comp1_3"],
        door: &["adoor01_2", "adoor03_2", "edoor01_1"],
        fixture: &[("tlight01", 32.0), ("tlight02", 64.0)],
        support: &["metal1_4", "tech08_1"],
    },
    Theme {
        name: "city",
        world: WorldType::Medieval,
        wall: &["wbrick1_5", "city2_5", "bricka2_4", "city6_8", "wall14_5"],
        floor: &["city4_7", "city4_5", "city4_2", "ground1_6", "woodflr1_5"],
        ceiling: &["wizwood1_8", "stone1_7", "citya1_1", "city6_3"],
        trim: &["wizmet1_2", "wizmet1_1"],
        door: &["door02_2", "door05_2", "dr05_2"],
        fixture: &[("light1_4", 64.0), ("light1_2", 64.0), ("light1_3", 64.0)],
        support: &["wmet1_1", "wmet3_1"],
    },
    Theme {
        name: "runic",
        world: WorldType::Metal,
        wall: &["metal4_4", "metal2_4", "metal1_7", "lgmetal"],
        floor: &["metal5_4", "metal5_2", "mmetal1_1"],
        ceiling: &["metal2_2", "cop1_3"],
        trim: &["rune1_1", "metal6_1", "cop2_3"],
        door: &["door01_2", "dr01_1", "door03_2"],
        fixture: &[("light3_7", 32.0), ("light3_8", 32.0)],
        support: &["metal5_6", "metal4_2", "metal4_3"],
    },
    Theme {
        name: "castle",
        world: WorldType::Medieval,
        wall: &[
            "med_csl_brk10b",
            "med_csl_brk11",
            "med_csl_brk14",
            "med_dbrick6b",
        ],
        floor: &[
            "med_csl_flr4_1",
            "med_csl_flr4_4",
            "med_csl_flr2_2",
            "med_cobstn1_2",
        ],
        ceiling: &["med_csl_flr1_1", "med_csl_brk12_f", "med_wood7"],
        trim: &["med_csl_trm1", "med_etrim1"],
        door: &["med_door3b", "med_door4", "med_dr2a"],
        fixture: &[],
        support: &["med_cmet4"],
    },
    Theme {
        name: "temple",
        world: WorldType::Medieval,
        wall: &["grk_wall1", "grk_wall3", "grk_brk15", "grk_brk17"],
        floor: &["grk_flr4_2", "grk_flr5_3", "grk_bl_flt1"],
        ceiling: &["grk_brk15_f", "grk_tile2_1"],
        trim: &["grk_trim1", "grk_trim2"],
        door: &["grk_door3"],
        fixture: &[],
        support: &["grk_bl_trim1"],
    },
    Theme {
        name: "ruin",
        world: WorldType::Metal,
        wall: &["may_blok1_1", "may_brk1_1", "may_wall1_2"],
        floor: &["may_flr1_1", "may_flr1_2", "may_drt1_2"],
        ceiling: &["may_blok2_1"],
        trim: &["may_trm1_1", "may_trm1_2", "may_bnd1_2"],
        door: &["may_door2_1"],
        fixture: &[],
        support: &[],
    },
    Theme {
        name: "station",
        world: WorldType::Base,
        wall: &["t_wall1ba", "t_wall2a", "t_wall3a", "t_wall05", "t_wall3aa"],
        floor: &["t_flor1a", "tek_flr3"],
        ceiling: &["t_indent01", "t_tech06", "t_tech01"],
        trim: &["t_trim1aa", "t_trim2ba", "t_trim1d"],
        door: &["edoor01_1", "fddoor01"],
        fixture: &[("tlight13", 64.0), ("tlightblfb", 32.0)],
        support: &["t_metalsheeta"],
    },
];

/// The name the level takes when no theme's textures are there.
const PLAIN_THEME: &str = "plain";

/// What the sky's name starts with (MAP-4.5).
const SKY_MARK: &str = "sky";
/// The starts of the liquids a pit may hold; a level's is drawn from those the game has.
const LIQUIDS: &[&str] = &["*lava", "*slime", "*water"];
/// The first bytes of names that are no wall: liquids, animations and fences.
const SPECIAL_STARTS: &[char] = &['*', '+', '{'];
/// Names that are tools' own, not surfaces.
const TOOL_NAMES: &[&str] = &["clip", "trigger", "skip", "hint", "hintskip", "origin"];
/// The texture a trigger's brush is given when the game has it
/// (`docs/agent-reference/maps.md`, Textures).
const TRIGGER: &str = "trigger";

/// What a liquid does to a player in it (`client.qc`, `WaterMove`): lava does
/// `10*self.waterlevel` every 0.2 s, slime `4*self.waterlevel` every second, water nothing
/// until the air runs out 12 s under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hazard {
    /// Water: a player who falls in swims out.
    Harmless,
    /// Slime: at most 12 a second, so a player who falls in climbs out with health to spare.
    Survivable,
    /// Lava: up to 150 a second, so a player who falls in dies before climbing out.
    Deadly,
}

impl Hazard {
    /// The hazard of a liquid painted with `surface`, by its texture's name (`*lava`,
    /// `*slime`, else water; QBSP-4.1).
    pub(crate) fn of(surface: &Surface) -> Self {
        let name = surface.texture.to_ascii_lowercase();
        if name.starts_with(b"*lava") {
            Self::Deadly
        } else if name.starts_with(b"*slime") {
            Self::Survivable
        } else {
            Self::Harmless
        }
    }
}

/// The texture a lamp is painted with, and the width of its square when it is a light of
/// the theme's, which a lamp shows whole.
#[derive(Clone, Debug)]
pub(crate) struct Lamp {
    surface: Surface,
    size: Option<f64>,
}

/// How wide a lamp is: its light texture is scaled to fit it.
pub(crate) const LAMP_SIZE: f64 = 32.0;

impl Lamp {
    /// A lamp of `surface`, with no light texture of a known size: it is painted as it comes.
    pub(crate) const fn plain(surface: Surface) -> Self {
        Self {
            surface,
            size: None,
        }
    }

    /// A lamp of `surface`, a light texture `size` square.
    #[cfg(test)]
    pub(crate) const fn sized(surface: Surface, size: f64) -> Self {
        Self {
            surface,
            size: Some(size),
        }
    }

    /// The surface of a lamp [`LAMP_SIZE`] wide whose face, on its texture's axes, starts at
    /// `s` along them and at `t` down them: the light scaled to the lamp and shifted to start
    /// at its corner, whole on it. A face's texture runs `s = p . s_axis / scale + shift`, and
    /// down a face, as on a floor's `-y` and a wall's `-z`, its `t` runs from the far edge, so
    /// the corner `t` starts at is the face's lowest.
    pub(crate) fn on(&self, [s, t]: [f64; 2]) -> Surface {
        let Some(size) = self.size else {
            return self.surface.clone();
        };
        let scale = LAMP_SIZE / size;
        Surface {
            texture: self.surface.texture.clone(),
            texdef: TexDef::Standard {
                shift: [(-s / scale).rem_euclid(size), (t / scale).rem_euclid(size)],
                rotate: 0.0,
                scale: [scale, scale],
            },
        }
    }
}

/// The names a level is painted with.
#[derive(Clone, Debug)]
pub(crate) struct Paints {
    /// The theme's name.
    pub(crate) name: &'static str,
    /// The level's `worldtype`.
    pub(crate) world: WorldType,
    /// The walls.
    pub(crate) wall: Surface,
    /// The floors.
    pub(crate) floor: Surface,
    /// The ceilings.
    pub(crate) ceiling: Surface,
    /// Bands and edges: cornices, skirtings, coffers, a dome's rings, plinths.
    pub(crate) trim: Surface,
    /// Lamps: the theme's light texture, else its trim.
    pub(crate) fixture: Lamp,
    /// What holds a room up and stands in it: pillars, pilasters, beams, columns, rails.
    /// A second wall of the theme, plainer than its trim, which id's levels use sparingly; the
    /// trim when the game has none of the theme's, so it still stands out from the walls.
    pub(crate) support: Surface,
    /// Doors.
    pub(crate) door: Surface,
    /// What a pit holds; none when the game has no liquid.
    pub(crate) liquid: Option<Surface>,
    /// The sky; none when the game has none.
    pub(crate) sky: Option<Surface>,
    /// The game's `trigger` texture, when it has one.
    trigger: Option<Surface>,
}

/// The surface of the texture `name`, aligned to the world.
pub(crate) fn surface(name: &str) -> Surface {
    Surface {
        texture: name.as_bytes().to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0, 0.0],
            rotate: 0.0,
            scale: [1.0, 1.0],
        },
    }
}

/// The textures there are, each name in lower case.
struct Names(BTreeSet<String>);

impl Names {
    /// The first of `wanted` that is there, the choice among those there made by `rng`.
    fn one_of(&self, wanted: &[&'static str], rng: &mut Rng) -> Option<&'static str> {
        let there: Vec<&'static str> = wanted
            .iter()
            .copied()
            .filter(|name| self.0.contains(*name))
            .collect();
        rng.pick(&there).copied()
    }

    /// A name that starts with one of `starts`: the start is drawn, then a name of that
    /// start, and a start no name has is passed over.
    fn starting(&self, starts: &[&str], rng: &mut Rng) -> Option<String> {
        let mut order: Vec<&str> = starts.to_vec();
        rng.shuffle(&mut order);
        order.iter().find_map(|start| {
            let there: Vec<&String> = self.0.iter().filter(|n| n.starts_with(start)).collect();
            rng.pick(&there).map(|name| (*name).clone())
        })
    }

    /// The names that paint a wall: no liquid, sky, animation, fence or tool's texture.
    fn plain(&self) -> Vec<&String> {
        self.0
            .iter()
            .filter(|name| {
                !name.starts_with(SPECIAL_STARTS)
                    && !name.starts_with(SKY_MARK)
                    && !TOOL_NAMES.contains(&name.as_str())
            })
            .collect()
    }
}

impl WorldType {
    /// The parts its rooms may be dressed with besides those every room takes
    /// ([`RoomPart::EVERY_ROOM`]): a base's walls are trimmed and its halls columned; a
    /// castle's and a runic temple's rooms grow bays with braziers in them.
    pub(crate) const fn parts(self) -> &'static [RoomPart] {
        match self {
            Self::Base => &[
                RoomPart::Cornice,
                RoomPart::Skirting,
                RoomPart::Columns,
                RoomPart::Beams,
            ],
            Self::Medieval => &[
                RoomPart::Bays,
                RoomPart::Braziers,
                RoomPart::Skirting,
                RoomPart::Columns,
                RoomPart::Beams,
            ],
            Self::Metal => &[
                RoomPart::Bays,
                RoomPart::Braziers,
                RoomPart::Cornice,
                RoomPart::Columns,
                RoomPart::Beams,
            ],
        }
    }
}

impl Paints {
    /// What a trigger's brush is painted with: the game's `trigger` texture, else its walls',
    /// since a trigger is not drawn whatever its texture (`InitTrigger` clears its model).
    pub(crate) fn trigger(&self) -> &Surface {
        self.trigger.as_ref().unwrap_or(&self.wall)
    }

    /// Paints of these surfaces, for a room or a passage built on its own: no level, so no
    /// theme's name and no trigger.
    pub(crate) fn named(
        wall: Surface,
        floor: Surface,
        ceiling: Surface,
        trim: Surface,
        door: Surface,
        liquid: Option<Surface>,
        sky: Option<Surface>,
    ) -> Self {
        Self {
            name: PLAIN_THEME,
            world: WorldType::Medieval,
            support: wall.clone(),
            fixture: Lamp::plain(trim.clone()),
            wall,
            floor,
            ceiling,
            trim,
            door,
            liquid,
            sky,
            trigger: None,
        }
    }

    /// The paints for a level, from the names in `textures`: a theme whose walls, floors and
    /// ceilings are all there, chosen by `rng`, else any plain textures.
    ///
    /// # Errors
    ///
    /// [`GenError::NoTextures`] when the game has no plain texture at all.
    pub(crate) fn choose(textures: &[String], rng: &mut Rng) -> Result<Self, GenError> {
        let names = Names(textures.iter().map(|name| name.to_lowercase()).collect());
        let liquid = names.starting(LIQUIDS, rng).map(|name| surface(&name));
        let sky = names.starting(&[SKY_MARK], rng).map(|name| surface(&name));
        let trigger = names.0.contains(TRIGGER).then(|| surface(TRIGGER));
        let mut themes: Vec<&Theme> = THEMES.iter().collect();
        rng.shuffle(&mut themes);
        for theme in themes {
            let (Some(wall), Some(floor), Some(ceiling)) = (
                names.one_of(theme.wall, rng),
                names.one_of(theme.floor, rng),
                names.one_of(theme.ceiling, rng),
            ) else {
                continue;
            };
            let trim = names.one_of(theme.trim, rng).unwrap_or(wall);
            let door = names.one_of(theme.door, rng).unwrap_or(trim);
            let support = names.one_of(theme.support, rng).unwrap_or(trim);
            let lights: Vec<&'static str> = theme.fixture.iter().map(|(name, _)| *name).collect();
            let fixture = names.one_of(&lights, rng).map_or_else(
                || Lamp::plain(surface(trim)),
                |name| Lamp {
                    surface: surface(name),
                    size: theme
                        .fixture
                        .iter()
                        .find(|(light, _)| *light == name)
                        .map(|(_, size)| *size),
                },
            );
            return Ok(Self {
                name: theme.name,
                world: theme.world,
                fixture,
                support: surface(support),
                wall: surface(wall),
                floor: surface(floor),
                ceiling: surface(ceiling),
                trim: surface(trim),
                door: surface(door),
                liquid,
                sky,
                trigger,
            });
        }
        let plain = names.plain();
        let mut one = || rng.pick(&plain).map(|name| surface(name));
        let wall = one().ok_or(GenError::NoTextures)?;
        let floor = one().ok_or(GenError::NoTextures)?;
        let ceiling = one().ok_or(GenError::NoTextures)?;
        let trim = one().ok_or(GenError::NoTextures)?;
        Ok(Self {
            name: PLAIN_THEME,
            world: WorldType::Medieval,
            door: trim.clone(),
            support: wall.clone(),
            fixture: Lamp::plain(trim.clone()),
            wall,
            floor,
            ceiling,
            trim,
            liquid,
            sky,
            trigger,
        })
    }
}

#[cfg(test)]
mod tests;
