// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/sbar.c, Sbar_Init, Sbar_DrawPic, Sbar_DrawCharacter, Sbar_DrawString, Sbar_itoa, Sbar_DrawNum, Sbar_SortFrags, Sbar_ColorForMap, Sbar_SoloScoreboard, Sbar_DrawScoreboard, Sbar_DrawInventory, Sbar_DrawFrags, Sbar_DrawFace, Sbar_Draw, Sbar_IntermissionNumber, Sbar_DeathmatchOverlay, Sbar_IntermissionOverlay, Sbar_FinaleOverlay
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/screen.c, SCR_CenterPrint, SCR_DrawCenterString, SCR_CheckDrawCenterString

//! The status bar, the scoreboards and the overlays, and the centre print
//! (`docs/spec/engine/hud.md`, HUD-1 to HUD-3), as a list of pictures, characters and fills
//! at the positions id draws them, for the renderer to put on the screen.
//!
//! Implements HUD-1 to HUD-3.

use qc_vm::convert::{float_to_int, int_to_float};

use crate::client::{Score, Scores, stat};
use crate::console::ConsoleHold;

/// `SBAR_HEIGHT` (HUD-1).
pub const HUD_SBAR_HEIGHT: i32 = 24;
/// A number picture's width (HUD-1.6).
pub const HUD_NUM_WIDTH: i32 = 24;
/// Armour at or below this is red (HUD-1.2).
pub const HUD_LOW_ARMOR: i32 = 25;
/// Health at or below this is red (HUD-1.2).
pub const HUD_LOW_HEALTH: i32 = 25;
/// Ammo at or below this is red (HUD-1.2).
pub const HUD_LOW_AMMO: i32 = 10;
/// The frames a picked-up weapon flashes for (HUD-1.4).
pub const HUD_FLASH_FRAMES: i32 = 10;
/// `scr_centertime` (HUD-3.1).
pub const HUD_CENTERTIME: f32 = 2.0;
/// `scr_printspeed` (HUD-3.3).
pub const HUD_PRINTSPEED: f32 = 8.0;
/// The most characters of a centre print line (HUD-3.2).
pub const HUD_CENTER_LINE_LENGTH: usize = 40;
/// The most bytes a centre print keeps (HUD-3.1).
pub const HUD_CENTER_MAX: usize = 1023;
/// `GAME_DEATHMATCH` (HUD-1).
pub const HUD_GAME_DEATHMATCH: i32 = 1;

const IT_SHOTGUN: i32 = 1;
const IT_SHELLS: i32 = 256;
const IT_NAILS: i32 = 512;
const IT_ROCKETS: i32 = 1024;
const IT_CELLS: i32 = 2048;
const IT_ARMOR1: i32 = 8192;
const IT_ARMOR2: i32 = 16384;
const IT_ARMOR3: i32 = 32768;
const IT_INVISIBILITY: i32 = 524_288;
const IT_INVULNERABILITY: i32 = 1_048_576;
const IT_QUAD: i32 = 4_194_304;

/// A console character's width and height in pixels.
pub(crate) const CHAR_SIZE: i32 = 8;
/// The bar's width, which its layout is in (HUD-1).
const BAR_WIDTH: i32 = 320;
/// The digits `Sbar_DrawNum` draws of the armour, health, ammo and intermission numbers.
const NUMBER_DIGITS: usize = 3;
/// The armour shown while invulnerable (HUD-1.2).
const INVULNERABLE_ARMOR: i32 = 666;
/// Where the status bar draws the armour, the face, the health, the ammo's picture and the
/// ammo (HUD-1.2, HUD-1.3).
const ARMOR_X: i32 = 24;
const FACE_X: i32 = 112;
const HEALTH_X: i32 = 136;
const AMMO_PIC_X: i32 = 224;
const AMMO_X: i32 = 248;
/// The inventory bar's top and its icons' (HUD-1.4).
const INVENTORY_Y: i32 = -24;
const INVENTORY_ICONS_Y: i32 = -16;
/// The weapons' spacing, and their flash: ten frames a second, cycling every five (HUD-1.4).
const WEAPON_SPACING: i32 = 24;
const FLASH_RATE: f32 = 10.0;
const FLASH_CYCLE: i32 = 5;
/// The ammo counts, each six characters apart (HUD-1.4).
const AMMO_KINDS: i32 = 4;
const AMMO_COLUMNS: i32 = 6;
/// The small yellow digits are characters 18 to 27 (HUD-1.4).
const SMALL_DIGITS: u8 = 18;
/// The items' first bit, `IT_KEY1`, and where they are drawn (HUD-1.4).
const ITEM_FIRST_BIT: u32 = 17;
const ITEMS_X: i32 = 192;
const ITEM_SPACING: i32 = 16;
/// The sigils' first bit, their count and where they are drawn (HUD-1.4).
const SIGIL_FIRST_BIT: u32 = 28;
const SIGILS: u32 = 4;
const SIGILS_X: i32 = BAR_WIDTH - 32;
/// `Sbar_DrawCharacter` draws 4 to the right (HUD-1.5).
const BAR_CHAR_NUDGE: i32 = 4;
/// How far left of its block's end the frag list's right marker is (HUD-1.5).
const FRAG_MARK_INSET: i32 = 4;
/// `Sbar_ColorForMap`'s offset into a colour row (HUD-1.5).
const MAP_COLOR_OFFSET: i32 = 8;
/// A player's colours: the top in the high nibble, the bottom in the low (HUD-1.5).
const TOP_COLOR_MASK: i32 = 0xf0;
const BOTTOM_COLOR_MASK: i32 = 0x0f;
const COLOR_ROW_SHIFT: i32 = 4;
/// `Sbar_DrawFrags`: the column of the first block and how far above the bar it is, the
/// blocks shown, each four columns wide, a block's offset, size and the markers around the
/// viewer's own (HUD-1.5).
const FRAGS_COLUMN: i32 = 23;
const FRAGS_ABOVE: i32 = 23;
const FRAG_BLOCKS: usize = 4;
const FRAG_BLOCK_COLUMNS: i32 = 4;
const FRAG_BLOCK_X: i32 = 10;
const FRAG_BLOCK_WIDTH: i32 = 28;
const FRAG_TOP_HEIGHT: i32 = 4;
const FRAG_BOTTOM_HEIGHT: i32 = 3;
const FRAG_MARK_LEFT: u8 = 16;
const FRAG_MARK_RIGHT: u8 = 17;
/// `Sbar_DrawFace`: full health, the health each face stands for, and the faces (HUD-1.3).
const FULL_HEALTH: i32 = 100;
const HEALTH_PER_FACE: i32 = 20;
const FACES: i32 = 5;
/// `Sbar_DrawScoreboard`: the rows' left edge and heights, the time's left edge and the level
/// name's centre (HUD-2.1).
const SCORE_X: i32 = 8;
const SCORE_ROW1_Y: i32 = 4;
const SCORE_ROW2_Y: i32 = 12;
const SCORE_TIME_X: i32 = 184;
const SCORE_LEVEL_CENTRE: i32 = 232;
/// Seconds a minute, and a decimal digit's base.
const MINUTE: f32 = 60.0;
const DECIMAL: i32 = 10;
/// `Sbar_DeathmatchOverlay`: the ranking picture's top, the first row, the blocks' size, the
/// names' offset, the row height and the viewer's marker (HUD-2.2).
const RANKING_Y: i32 = 8;
const RANKING_X: i32 = 80;
const RANKING_TOP: i32 = 40;
const RANKING_BLOCK_WIDTH: i32 = 40;
const RANKING_BLOCK_HEIGHT: i32 = 4;
const RANKING_NAME_X: i32 = 64;
const RANKING_ROW: i32 = 10;
const RANKING_MARK: u8 = 12;
/// `Sbar_IntermissionOverlay` and `Sbar_FinaleOverlay`'s layout (HUD-2.3).
const FINALE_Y: i32 = 16;
const COMPLETE_X: i32 = 64;
const COMPLETE_Y: i32 = 24;
const INTER_Y: i32 = 56;
const INTER_NUMBER_X: i32 = 160;
const INTER_TIME_Y: i32 = 64;
const INTER_COLON_X: i32 = 234;
const INTER_TENS_X: i32 = 246;
const INTER_UNITS_X: i32 = 266;
const INTER_SECRETS_Y: i32 = 104;
const INTER_MONSTERS_Y: i32 = 144;
const INTER_SLASH_X: i32 = 232;
const INTER_TOTAL_X: i32 = 240;
/// `SCR_DrawCenterString`: no limit on the characters, the lines that are drawn at 35% of the
/// height, and the top of a longer print (HUD-3.3).
const CENTER_UNLIMITED: i32 = 9999;
const CENTER_SHORT_LINES: usize = 4;
const CENTER_HEIGHT_SHARE: f32 = 0.35;
const CENTER_TALL_Y: i32 = 48;
/// `%3i`'s columns.
const THREE_COLUMNS: usize = 3;

/// The colour of a number's pictures, `Sbar_DrawNum`'s `color` (HUD-1.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NumberColor {
    /// `num_`.
    Normal,
    /// `anum_`, a low value.
    Red,
}

impl NumberColor {
    /// Red when `value` is at most `low` (HUD-1.2).
    const fn warning(value: i32, low: i32) -> Self {
        if value <= low {
            Self::Red
        } else {
            Self::Normal
        }
    }
}

/// How the centre print is shown: for its time, or typed out through an intermission
/// (HUD-3.2, HUD-3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CenterMode {
    /// Shown until its time is up.
    Timed,
    /// The intermission's: typed out at `HUD_PRINTSPEED` and never taken down.
    Intermission,
}

/// The weapon pictures' names, `inv_<weapon>` (HUD-1.4).
pub const HUD_WEAPONS: [&str; 7] = [
    "shotgun", "sshotgun", "nailgun", "snailgun", "rlaunch", "srlaunch", "lightng",
];
/// The item pictures for bits 17 to 22 (HUD-1.4).
pub const HUD_ITEMS: [&str; 6] = [
    "sb_key1",
    "sb_key2",
    "sb_invis",
    "sb_invuln",
    "sb_suit",
    "sb_quad",
];

/// One thing to draw on the screen, in the bar's 320 by 200 coordinates (HUD-1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HudItem {
    /// A `gfx.wad` picture by name at a position, drawn with its transparent pixels cut.
    Pic {
        /// Its name in the WAD, such as `sbar` or `num_3`.
        name: String,
        /// Its left edge.
        x: i32,
        /// Its top edge.
        y: i32,
    },
    /// A `gfx/*.lmp` picture by path, centred when `x` is `None`.
    Lmp {
        /// Its path, such as `gfx/ranking.lmp`.
        path: String,
        /// Its left edge, or centred on the screen.
        x: Option<i32>,
        /// Its top edge.
        y: i32,
    },
    /// A character of the console font at a position.
    Char {
        /// Its left edge.
        x: i32,
        /// Its top edge.
        y: i32,
        /// The character.
        ch: u8,
    },
    /// The console's background, `gfx/conback.lmp` across the screen with its top at
    /// `lines - height`, translucent when `lines` is at most three quarters of `height`
    /// (CON-3.2).
    ConsoleBack {
        /// The console's height in pixels.
        lines: i32,
        /// The screen's height.
        height: i32,
        /// `con_forcedup`: the console up with no game to show (CON-3.1).
        forced: ConsoleHold,
    },
    /// `Draw_FadeScreen`: the screen darkened, black at alpha 0.8 over it (MENU-1.3).
    Fade,
    /// A rectangle of a palette colour.
    Fill {
        /// Its left edge.
        x: i32,
        /// Its top edge.
        y: i32,
        /// Its width.
        w: i32,
        /// Its height.
        h: i32,
        /// Its palette colour.
        color: i32,
    },
}

/// Where a bar item is placed: on the bar at the bottom, centred on screens wider than 320
/// except in deathmatch (HUD-1), or on the whole screen (HUD-2, HUD-3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// Relative to the bar's bottom-left corner: `y` is from `HUD_SBAR_HEIGHT` above the
    /// screen's bottom, and `x` is offset by `(width - 320) / 2` unless deathmatch.
    Bar,
    /// Relative to the screen's top-left corner.
    Screen,
    /// Relative to the screen's top-left corner with `x` offset by `(width - 320) / 2`, as
    /// the deathmatch overlay is (HUD-2.2).
    Centered,
}

/// An item with where it is anchored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    /// How its position is read.
    pub anchor: Anchor,
    /// What to draw.
    pub item: HudItem,
}

/// What the status bar reads of the client (HUD-1).
#[derive(Clone, Debug, PartialEq)]
pub struct HudInput<'a> {
    /// `cl.time`.
    pub time: f32,
    /// `cl.items`.
    pub items: i32,
    /// `cl.item_gettime`.
    pub item_gettime: &'a [f32; 32],
    /// `cl.stats`.
    pub stats: &'a [i32; 32],
    /// `cl.faceanimtime` (VIEW-4.1).
    pub faceanimtime: f32,
    /// `cl.maxclients`.
    pub maxclients: usize,
    /// `cl.gametype`.
    pub gametype: i32,
    /// The scoreboard.
    pub scores: &'a [Score],
    /// `cl.viewentity`.
    pub viewentity: usize,
    /// The level name.
    pub levelname: &'a [u8],
    /// `cl.intermission`.
    pub intermission: i32,
    /// `cl.completed_time`.
    pub completed_time: f32,
    /// `+showscores`.
    pub showscores: Scores,
    /// `sb_lines`: how many rows of the bar the view size leaves, 0, 24 or more.
    pub lines: i32,
}

/// `Sbar_itoa`: a number's decimal text, `-` first when negative.
fn itoa(num: i32) -> Vec<u8> {
    num.to_string().into_bytes()
}

/// `Sbar_DrawNum`'s pictures (HUD-1.6): the last `digits` digits of `num`, right-aligned,
/// red when `color` is [`NumberColor::Red`].
fn number(
    out: &mut Vec<Placed>,
    anchor: Anchor,
    x: i32,
    y: i32,
    num: i32,
    digits: usize,
    color: NumberColor,
) {
    let text = itoa(num);
    let shown = if text.len() > digits {
        text.get(text.len().saturating_sub(digits)..)
            .unwrap_or_default()
    } else {
        &text
    };
    let missing = i32::try_from(digits.saturating_sub(shown.len())).unwrap_or(0);
    let mut x = x.saturating_add(missing.saturating_mul(HUD_NUM_WIDTH));
    let prefix = match color {
        NumberColor::Red => "anum_",
        NumberColor::Normal => "num_",
    };
    for &c in shown {
        let name = if c == b'-' {
            format!("{prefix}minus")
        } else {
            format!("{prefix}{}", char::from(c))
        };
        out.push(Placed {
            anchor,
            item: HudItem::Pic { name, x, y },
        });
        x = x.saturating_add(HUD_NUM_WIDTH);
    }
}

/// A picture by name at a position.
fn pic(out: &mut Vec<Placed>, anchor: Anchor, name: &str, x: i32, y: i32) {
    out.push(Placed {
        anchor,
        item: HudItem::Pic {
            name: name.to_owned(),
            x,
            y,
        },
    });
}

/// `Sbar_DrawCharacter`: a character of the bar, 4 to the right as id draws it (HUD-1.5).
fn bar_char(out: &mut Vec<Placed>, x: i32, y: i32, ch: u8) {
    out.push(Placed {
        anchor: Anchor::Bar,
        item: HudItem::Char {
            x: x.saturating_add(BAR_CHAR_NUDGE),
            y,
            ch,
        },
    });
}

/// `Sbar_DrawString`: a string of the bar from `x`.
fn bar_string(out: &mut Vec<Placed>, x: i32, y: i32, text: &[u8]) {
    for (i, &ch) in text.iter().enumerate() {
        let dx = i32::try_from(i).unwrap_or(0).saturating_mul(CHAR_SIZE);
        out.push(Placed {
            anchor: Anchor::Bar,
            item: HudItem::Char {
                x: x.saturating_add(dx),
                y,
                ch,
            },
        });
    }
}

/// `Sbar_SortFrags`: the client slots with a name, by frags descending, a stable bubble
/// sort (HUD-1.5).
#[must_use]
pub fn sort_frags(scores: &[Score]) -> Vec<usize> {
    let mut order: Vec<usize> = scores
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.name.is_empty())
        .map(|(i, _)| i)
        .collect();
    let n = order.len();
    for i in 0..n {
        for j in 0..n.saturating_sub(1).saturating_sub(i) {
            let (Some(&a), Some(&b)) = (order.get(j), order.get(j.saturating_add(1))) else {
                break;
            };
            let fa = scores.get(a).map_or(0, |s| s.frags);
            let fb = scores.get(b).map_or(0, |s| s.frags);
            if fa < fb {
                order.swap(j, j.saturating_add(1));
            }
        }
    }
    order
}

/// `Sbar_ColorForMap`: a colour row plus 8 (HUD-1.5).
#[must_use]
pub const fn color_for_map(m: i32) -> i32 {
    m.saturating_add(MAP_COLOR_OFFSET)
}

/// `%3i` of a number: three characters, right-aligned, spaces not drawn.
fn three(num: i32) -> [Option<u8>; THREE_COLUMNS] {
    let text = format!("{num:3}");
    let mut out = [None; THREE_COLUMNS];
    for (slot, c) in out.iter_mut().zip(
        text.bytes()
            .rev()
            .take(THREE_COLUMNS)
            .collect::<Vec<_>>()
            .into_iter()
            .rev(),
    ) {
        *slot = (c != b' ').then_some(c);
    }
    out
}

impl HudInput<'_> {
    /// `Sbar_Draw` (HUD-1.1 to HUD-1.6) and the scoreboards it draws (HUD-2.1, HUD-2.2): the
    /// status bar's items, in drawing order; `screen_height` places the frag list's blocks.
    #[must_use]
    pub fn status_bar(&self, screen_height: i32) -> Vec<Placed> {
        let mut out = Vec::new();
        let stats = self.stats;
        if self.lines > HUD_SBAR_HEIGHT {
            inventory(&mut out, self);
            if self.maxclients != 1 {
                frags(&mut out, self, screen_height);
            }
        }
        if self.showscores == Scores::Shown || stats[stat::HEALTH] <= 0 {
            pic(&mut out, Anchor::Bar, "scorebar", 0, 0);
            scoreboard(&mut out, self);
        } else if self.lines != 0 {
            pic(&mut out, Anchor::Bar, "sbar", 0, 0);
            // HUD-1.2: armour.
            if self.items & IT_INVULNERABILITY != 0 {
                number(
                    &mut out,
                    Anchor::Bar,
                    ARMOR_X,
                    0,
                    INVULNERABLE_ARMOR,
                    NUMBER_DIGITS,
                    NumberColor::Red,
                );
                pic(&mut out, Anchor::Bar, "disc", 0, 0);
            } else {
                let armor = stats[stat::ARMOR];
                number(
                    &mut out,
                    Anchor::Bar,
                    ARMOR_X,
                    0,
                    armor,
                    NUMBER_DIGITS,
                    NumberColor::warning(armor, HUD_LOW_ARMOR),
                );
                if self.items & IT_ARMOR3 != 0 {
                    pic(&mut out, Anchor::Bar, "sb_armor3", 0, 0);
                } else if self.items & IT_ARMOR2 != 0 {
                    pic(&mut out, Anchor::Bar, "sb_armor2", 0, 0);
                } else if self.items & IT_ARMOR1 != 0 {
                    pic(&mut out, Anchor::Bar, "sb_armor1", 0, 0);
                }
            }
            face(&mut out, self);
            let health = stats[stat::HEALTH];
            number(
                &mut out,
                Anchor::Bar,
                HEALTH_X,
                0,
                health,
                NUMBER_DIGITS,
                NumberColor::warning(health, HUD_LOW_HEALTH),
            );
            let ammo_pic = [
                (IT_SHELLS, "sb_shells"),
                (IT_NAILS, "sb_nails"),
                (IT_ROCKETS, "sb_rocket"),
                (IT_CELLS, "sb_cells"),
            ]
            .into_iter()
            .find(|(bit, _)| self.items & bit != 0);
            if let Some((_, name)) = ammo_pic {
                pic(&mut out, Anchor::Bar, name, AMMO_PIC_X, 0);
            }
            let ammo = stats[stat::AMMO];
            number(
                &mut out,
                Anchor::Bar,
                AMMO_X,
                0,
                ammo,
                NUMBER_DIGITS,
                NumberColor::warning(ammo, HUD_LOW_AMMO),
            );
        }
        out
    }

    /// `Sbar_IntermissionOverlay` and `Sbar_FinaleOverlay` (HUD-2.3): the overlay for the
    /// intermission kind, 1 for the intermission and 2 or 3 for the finale and cutscene.
    #[must_use]
    pub fn intermission_overlay(&self) -> Vec<Placed> {
        let mut out = Vec::new();
        if self.intermission != 1 {
            out.push(Placed {
                anchor: Anchor::Screen,
                item: HudItem::Lmp {
                    path: "gfx/finale.lmp".to_owned(),
                    x: None,
                    y: FINALE_Y,
                },
            });
            return out;
        }
        if self.gametype == HUD_GAME_DEATHMATCH {
            deathmatch_overlay(&mut out, self);
            return out;
        }
        out.push(Placed {
            anchor: Anchor::Screen,
            item: HudItem::Lmp {
                path: "gfx/complete.lmp".to_owned(),
                x: Some(COMPLETE_X),
                y: COMPLETE_Y,
            },
        });
        out.push(Placed {
            anchor: Anchor::Screen,
            item: HudItem::Lmp {
                path: "gfx/inter.lmp".to_owned(),
                x: Some(0),
                y: INTER_Y,
            },
        });
        let dig = float_to_int(self.completed_time / MINUTE);
        let screen_number = |out: &mut Vec<Placed>, x: i32, y: i32, num: i32| {
            number(
                out,
                Anchor::Screen,
                x,
                y,
                num,
                NUMBER_DIGITS,
                NumberColor::Normal,
            );
        };
        screen_number(&mut out, INTER_NUMBER_X, INTER_TIME_Y, dig);
        let minute = float_to_int(MINUTE);
        let num = float_to_int(self.completed_time - int_to_float(dig.saturating_mul(minute)));
        pic(
            &mut out,
            Anchor::Screen,
            "num_colon",
            INTER_COLON_X,
            INTER_TIME_Y,
        );
        pic(
            &mut out,
            Anchor::Screen,
            &format!("num_{}", num.checked_div(DECIMAL).unwrap_or(0)),
            INTER_TENS_X,
            INTER_TIME_Y,
        );
        pic(
            &mut out,
            Anchor::Screen,
            &format!("num_{}", num.checked_rem(DECIMAL).unwrap_or(0)),
            INTER_UNITS_X,
            INTER_TIME_Y,
        );
        let stats = self.stats;
        for (y, done, total) in [
            (INTER_SECRETS_Y, stat::SECRETS, stat::TOTALSECRETS),
            (INTER_MONSTERS_Y, stat::MONSTERS, stat::TOTALMONSTERS),
        ] {
            screen_number(
                &mut out,
                INTER_NUMBER_X,
                y,
                stats.get(done).copied().unwrap_or(0),
            );
            pic(&mut out, Anchor::Screen, "num_slash", INTER_SLASH_X, y);
            screen_number(
                &mut out,
                INTER_TOTAL_X,
                y,
                stats.get(total).copied().unwrap_or(0),
            );
        }
        out
    }
}

/// `Sbar_DrawInventory` (HUD-1.4).
fn inventory(out: &mut Vec<Placed>, input: &HudInput<'_>) {
    pic(out, Anchor::Bar, "ibar", 0, INVENTORY_Y);
    for (i, weapon) in HUD_WEAPONS.iter().enumerate() {
        let bit = IT_SHOTGUN << i;
        if input.items & bit == 0 {
            continue;
        }
        let gettime = input.item_gettime.get(i).copied().unwrap_or(0.0);
        let mut flashon = float_to_int((input.time - gettime) * FLASH_RATE);
        if flashon >= HUD_FLASH_FRAMES {
            flashon = i32::from(input.stats[stat::ACTIVEWEAPON] == bit);
        } else {
            flashon = flashon
                .checked_rem(FLASH_CYCLE)
                .unwrap_or(0)
                .saturating_add(2);
        }
        let name = match flashon {
            0 => format!("inv_{weapon}"),
            1 => format!("inv2_{weapon}"),
            k => format!("inva{}_{weapon}", k.saturating_sub(1)),
        };
        let x = i32::try_from(i).unwrap_or(0).saturating_mul(WEAPON_SPACING);
        pic(out, Anchor::Bar, &name, x, INVENTORY_ICONS_Y);
    }
    for i in 0..AMMO_KINDS {
        let value = input
            .stats
            .get(stat::SHELLS.saturating_add(usize::try_from(i).unwrap_or(0)))
            .copied()
            .unwrap_or(0);
        for (d, c) in three(value).into_iter().enumerate() {
            let Some(c) = c else { continue };
            let x = (AMMO_COLUMNS
                .saturating_mul(i)
                .saturating_add(i32::try_from(d).unwrap_or(0))
                .saturating_add(1))
            .saturating_mul(CHAR_SIZE)
            .saturating_sub(2);
            let digit = SMALL_DIGITS.saturating_add(c.saturating_sub(b'0'));
            bar_char(out, x, INVENTORY_Y, digit);
        }
    }
    for (i, item) in HUD_ITEMS.iter().enumerate() {
        let bit = 1_i32.wrapping_shl(ITEM_FIRST_BIT.saturating_add(u32::try_from(i).unwrap_or(0)));
        if input.items & bit != 0 {
            let x =
                ITEMS_X.saturating_add(i32::try_from(i).unwrap_or(0).saturating_mul(ITEM_SPACING));
            pic(out, Anchor::Bar, item, x, INVENTORY_ICONS_Y);
        }
    }
    for i in 0..SIGILS {
        let bit = 1_i32.wrapping_shl(SIGIL_FIRST_BIT.saturating_add(i));
        if input.items & bit != 0 {
            let x =
                SIGILS_X.saturating_add(i32::try_from(i).unwrap_or(0).saturating_mul(CHAR_SIZE));
            pic(
                out,
                Anchor::Bar,
                &format!("sb_sigil{}", i.saturating_add(1)),
                x,
                INVENTORY_ICONS_Y,
            );
        }
    }
}

/// `Sbar_DrawFrags` (HUD-1.5).
fn frags(out: &mut Vec<Placed>, input: &HudInput<'_>, screen_height: i32) {
    let order = sort_frags(input.scores);
    let mut x = FRAGS_COLUMN;
    let y = screen_height
        .saturating_sub(HUD_SBAR_HEIGHT)
        .saturating_sub(FRAGS_ABOVE);
    for &k in order.iter().take(FRAG_BLOCKS) {
        let Some(s) = input.scores.get(k) else {
            continue;
        };
        let (top, bottom) = s.map_colors();
        let block_x = x.saturating_mul(CHAR_SIZE).saturating_add(FRAG_BLOCK_X);
        out.push(Placed {
            anchor: Anchor::Bar,
            item: HudItem::Fill {
                x: block_x,
                y: y.saturating_sub(screen_height.saturating_sub(HUD_SBAR_HEIGHT)),
                w: FRAG_BLOCK_WIDTH,
                h: FRAG_TOP_HEIGHT,
                color: top,
            },
        });
        out.push(Placed {
            anchor: Anchor::Bar,
            item: HudItem::Fill {
                x: block_x,
                y: y.saturating_add(FRAG_TOP_HEIGHT)
                    .saturating_sub(screen_height.saturating_sub(HUD_SBAR_HEIGHT)),
                w: FRAG_BLOCK_WIDTH,
                h: FRAG_BOTTOM_HEIGHT,
                color: bottom,
            },
        });
        let text = format!("{:3}", s.frags);
        for (j, ch) in text.bytes().enumerate() {
            let column = x
                .saturating_add(1)
                .saturating_add(i32::try_from(j).unwrap_or(0));
            bar_char(out, column.saturating_mul(CHAR_SIZE), INVENTORY_Y, ch);
        }
        if k == input.viewentity.wrapping_sub(1) {
            let left = x.saturating_mul(CHAR_SIZE).saturating_add(2);
            bar_char(out, left, INVENTORY_Y, FRAG_MARK_LEFT);
            let right = x
                .saturating_add(FRAG_BLOCK_COLUMNS)
                .saturating_mul(CHAR_SIZE)
                .saturating_sub(FRAG_MARK_INSET);
            bar_char(out, right, INVENTORY_Y, FRAG_MARK_RIGHT);
        }
        x = x.saturating_add(FRAG_BLOCK_COLUMNS);
    }
}

/// `Sbar_DrawFace` (HUD-1.3).
fn face(out: &mut Vec<Placed>, input: &HudInput<'_>) {
    let both = IT_INVISIBILITY | IT_INVULNERABILITY;
    let name = if input.items & both == both {
        "face_inv2".to_owned()
    } else if input.items & IT_QUAD != 0 {
        "face_quad".to_owned()
    } else if input.items & IT_INVISIBILITY != 0 {
        "face_invis".to_owned()
    } else if input.items & IT_INVULNERABILITY != 0 {
        "face_invul2".to_owned()
    } else {
        let health = input.stats[stat::HEALTH];
        let most = FACES.saturating_sub(1);
        let f = if health >= FULL_HEALTH {
            most
        } else {
            health
                .checked_div(HEALTH_PER_FACE)
                .unwrap_or(0)
                .clamp(0, most)
        };
        let n = FACES.saturating_sub(f);
        if input.time <= input.faceanimtime {
            format!("face_p{n}")
        } else {
            format!("face{n}")
        }
    };
    pic(out, Anchor::Bar, &name, FACE_X, 0);
}

/// `Sbar_DrawScoreboard` (HUD-2.1): the solo scoreboard, then the deathmatch overlay in
/// deathmatch.
fn scoreboard(out: &mut Vec<Placed>, input: &HudInput<'_>) {
    let stats = input.stats;
    let text = format!(
        "Monsters:{:3} /{:3}",
        stats[stat::MONSTERS],
        stats[stat::TOTALMONSTERS]
    );
    bar_string(out, SCORE_X, SCORE_ROW1_Y, text.as_bytes());
    let text = format!(
        "Secrets :{:3} /{:3}",
        stats[stat::SECRETS],
        stats[stat::TOTALSECRETS]
    );
    bar_string(out, SCORE_X, SCORE_ROW2_Y, text.as_bytes());
    let minutes = float_to_int(input.time / MINUTE);
    let seconds = float_to_int(input.time - MINUTE * int_to_float(minutes));
    let tens = seconds.checked_div(DECIMAL).unwrap_or(0);
    let units = seconds.saturating_sub(DECIMAL.saturating_mul(tens));
    let text = format!("Time :{minutes:3}:{tens}{units}");
    bar_string(out, SCORE_TIME_X, SCORE_ROW1_Y, text.as_bytes());
    let l = i32::try_from(input.levelname.len()).unwrap_or(0);
    let half = CHAR_SIZE >> 1;
    bar_string(
        out,
        SCORE_LEVEL_CENTRE.saturating_sub(l.saturating_mul(half)),
        SCORE_ROW2_Y,
        input.levelname,
    );
    if input.gametype == HUD_GAME_DEATHMATCH {
        deathmatch_overlay(out, input);
    }
}

/// `Sbar_DeathmatchOverlay` (HUD-2.2).
fn deathmatch_overlay(out: &mut Vec<Placed>, input: &HudInput<'_>) {
    out.push(Placed {
        anchor: Anchor::Screen,
        item: HudItem::Lmp {
            path: "gfx/ranking.lmp".to_owned(),
            x: None,
            y: RANKING_Y,
        },
    });
    let order = sort_frags(input.scores);
    let x = RANKING_X;
    let mut y = RANKING_TOP;
    for &k in &order {
        let Some(s) = input.scores.get(k) else {
            continue;
        };
        let (top, bottom) = s.map_colors();
        out.push(Placed {
            anchor: Anchor::Centered,
            item: HudItem::Fill {
                x,
                y,
                w: RANKING_BLOCK_WIDTH,
                h: RANKING_BLOCK_HEIGHT,
                color: top,
            },
        });
        out.push(Placed {
            anchor: Anchor::Centered,
            item: HudItem::Fill {
                x,
                y: y.saturating_add(RANKING_BLOCK_HEIGHT),
                w: RANKING_BLOCK_WIDTH,
                h: RANKING_BLOCK_HEIGHT,
                color: bottom,
            },
        });
        let text = format!("{:3}", s.frags);
        for (j, ch) in text.bytes().enumerate() {
            out.push(Placed {
                anchor: Anchor::Centered,
                item: HudItem::Char {
                    x: (x.saturating_add(CHAR_SIZE))
                        .saturating_add(i32::try_from(j).unwrap_or(0).saturating_mul(CHAR_SIZE)),
                    y,
                    ch,
                },
            });
        }
        if k == input.viewentity.wrapping_sub(1) {
            out.push(Placed {
                anchor: Anchor::Centered,
                item: HudItem::Char {
                    x: x.saturating_sub(CHAR_SIZE),
                    y,
                    ch: RANKING_MARK,
                },
            });
        }
        for (j, &ch) in s.name.iter().enumerate() {
            out.push(Placed {
                anchor: Anchor::Centered,
                item: HudItem::Char {
                    x: (x.saturating_add(RANKING_NAME_X))
                        .saturating_add(i32::try_from(j).unwrap_or(0).saturating_mul(CHAR_SIZE)),
                    y,
                    ch,
                },
            });
        }
        y = y.saturating_add(RANKING_ROW);
    }
}

impl Score {
    /// `Sbar_ColorForMap` of the shirt's and the trousers' colour rows (HUD-1.5).
    const fn map_colors(&self) -> (i32, i32) {
        (
            color_for_map(self.colors & TOP_COLOR_MASK),
            color_for_map((self.colors & BOTTOM_COLOR_MASK) << COLOR_ROW_SHIFT),
        )
    }
}

/// The centre print (HUD-3).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CenterPrint {
    text: Vec<u8>,
    /// `scr_centertime_off`.
    time_off: f32,
    /// `scr_centertime_start`.
    start: f32,
    lines: usize,
}

impl CenterPrint {
    /// `SCR_CenterPrint (str)` (HUD-3.1).
    pub fn set(&mut self, text: &[u8], time: f32) {
        self.text = text
            .get(..text.len().min(HUD_CENTER_MAX))
            .unwrap_or_default()
            .to_vec();
        self.time_off = HUD_CENTERTIME;
        self.start = time;
        self.lines = self
            .text
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
            .saturating_add(1);
    }

    /// The lines the text has.
    #[must_use]
    pub const fn lines(&self) -> usize {
        self.lines
    }

    /// `SCR_CheckDrawCenterString` then `SCR_DrawCenterString` (HUD-3.2, HUD-3.3): the
    /// characters to draw this frame on a screen `width` by `height` in `mode`, none once the
    /// time is up outside an intermission; `frametime` is taken off the time first.
    pub fn draw(
        &mut self,
        frametime: f32,
        time: f32,
        mode: CenterMode,
        width: i32,
        height: i32,
    ) -> Vec<HudItem> {
        self.time_off -= frametime;
        if self.time_off <= 0.0 && mode == CenterMode::Timed {
            return Vec::new();
        }
        let mut remaining = match mode {
            CenterMode::Intermission => float_to_int(HUD_PRINTSPEED * (time - self.start)),
            CenterMode::Timed => CENTER_UNLIMITED,
        };
        let mut out = Vec::new();
        let mut y = if self.lines <= CENTER_SHORT_LINES {
            float_to_int(int_to_float(height) * CENTER_HEIGHT_SHARE)
        } else {
            CENTER_TALL_Y
        };
        for line in self.text.split(|&b| b == b'\n') {
            let shown = line
                .get(..line.len().min(HUD_CENTER_LINE_LENGTH))
                .unwrap_or_default();
            let l = i32::try_from(shown.len()).unwrap_or(0);
            let mut x = width
                .saturating_sub(l.saturating_mul(CHAR_SIZE))
                .checked_div(2)
                .unwrap_or(0);
            for &ch in shown {
                out.push(HudItem::Char { x, y, ch });
                // `if (!remaining--) return`: the character is drawn, then the count checked.
                let was = remaining;
                remaining = remaining.saturating_sub(1);
                if was == 0 {
                    return out;
                }
                x = x.saturating_add(CHAR_SIZE);
            }
            y = y.saturating_add(CHAR_SIZE);
        }
        out
    }
}
