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

use crate::client::{Score, stat};

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
    },
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
    pub showscores: bool,
    /// `sb_lines`: how many rows of the bar the view size leaves, 0, 24 or more.
    pub lines: i32,
}

/// `Sbar_itoa`: a number's decimal text, `-` first when negative.
fn itoa(num: i32) -> Vec<u8> {
    num.to_string().into_bytes()
}

/// `Sbar_DrawNum`'s pictures (HUD-1.6): the last `digits` digits of `num`, right-aligned,
/// red when `color` is 1.
fn number(
    out: &mut Vec<Placed>,
    anchor: Anchor,
    x: i32,
    y: i32,
    num: i32,
    digits: usize,
    red: bool,
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
    let prefix = if red { "anum_" } else { "num_" };
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
            x: x.saturating_add(4),
            y,
            ch,
        },
    });
}

/// `Sbar_DrawString`: a string of the bar from `x`.
fn bar_string(out: &mut Vec<Placed>, x: i32, y: i32, text: &[u8]) {
    for (i, &ch) in text.iter().enumerate() {
        let dx = i32::try_from(i).unwrap_or(0).saturating_mul(8);
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
    m.saturating_add(8)
}

/// `%3i` of a number: three characters, right-aligned, spaces not drawn.
fn three(num: i32) -> [Option<u8>; 3] {
    let text = format!("{num:3}");
    let mut out = [None; 3];
    for (slot, c) in out.iter_mut().zip(
        text.bytes()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev(),
    ) {
        *slot = (c != b' ').then_some(c);
    }
    out
}

/// `Sbar_Draw` (HUD-1.1 to HUD-1.6) and the scoreboards it draws (HUD-2.1, HUD-2.2): the
/// status bar's items, in drawing order; `screen_height` places the frag list's blocks.
#[must_use]
pub fn status_bar(input: &HudInput<'_>, screen_height: i32) -> Vec<Placed> {
    let mut out = Vec::new();
    let stats = input.stats;
    if input.lines > HUD_SBAR_HEIGHT {
        inventory(&mut out, input);
        if input.maxclients != 1 {
            frags(&mut out, input, screen_height);
        }
    }
    if input.showscores || stats[stat::HEALTH] <= 0 {
        pic(&mut out, Anchor::Bar, "scorebar", 0, 0);
        scoreboard(&mut out, input);
    } else if input.lines != 0 {
        pic(&mut out, Anchor::Bar, "sbar", 0, 0);
        // HUD-1.2: armour.
        if input.items & IT_INVULNERABILITY != 0 {
            number(&mut out, Anchor::Bar, 24, 0, 666, 3, true);
            pic(&mut out, Anchor::Bar, "disc", 0, 0);
        } else {
            let armor = stats[stat::ARMOR];
            number(
                &mut out,
                Anchor::Bar,
                24,
                0,
                armor,
                3,
                armor <= HUD_LOW_ARMOR,
            );
            if input.items & IT_ARMOR3 != 0 {
                pic(&mut out, Anchor::Bar, "sb_armor3", 0, 0);
            } else if input.items & IT_ARMOR2 != 0 {
                pic(&mut out, Anchor::Bar, "sb_armor2", 0, 0);
            } else if input.items & IT_ARMOR1 != 0 {
                pic(&mut out, Anchor::Bar, "sb_armor1", 0, 0);
            }
        }
        face(&mut out, input);
        let health = stats[stat::HEALTH];
        number(
            &mut out,
            Anchor::Bar,
            136,
            0,
            health,
            3,
            health <= HUD_LOW_HEALTH,
        );
        let ammo_pic = [
            (IT_SHELLS, "sb_shells"),
            (IT_NAILS, "sb_nails"),
            (IT_ROCKETS, "sb_rocket"),
            (IT_CELLS, "sb_cells"),
        ]
        .into_iter()
        .find(|(bit, _)| input.items & bit != 0);
        if let Some((_, name)) = ammo_pic {
            pic(&mut out, Anchor::Bar, name, 224, 0);
        }
        let ammo = stats[stat::AMMO];
        number(&mut out, Anchor::Bar, 248, 0, ammo, 3, ammo <= HUD_LOW_AMMO);
    }
    out
}

/// `Sbar_DrawInventory` (HUD-1.4).
fn inventory(out: &mut Vec<Placed>, input: &HudInput<'_>) {
    pic(out, Anchor::Bar, "ibar", 0, -24);
    for (i, weapon) in HUD_WEAPONS.iter().enumerate() {
        let bit = IT_SHOTGUN << i;
        if input.items & bit == 0 {
            continue;
        }
        let gettime = input.item_gettime.get(i).copied().unwrap_or(0.0);
        let mut flashon = float_to_int((input.time - gettime) * 10.0);
        if flashon >= HUD_FLASH_FRAMES {
            flashon = i32::from(input.stats[stat::ACTIVEWEAPON] == bit);
        } else {
            flashon = flashon.checked_rem(5).unwrap_or(0).saturating_add(2);
        }
        let name = match flashon {
            0 => format!("inv_{weapon}"),
            1 => format!("inv2_{weapon}"),
            k => format!("inva{}_{weapon}", k.saturating_sub(1)),
        };
        let x = i32::try_from(i).unwrap_or(0).saturating_mul(24);
        pic(out, Anchor::Bar, &name, x, -16);
    }
    for i in 0..4_i32 {
        let value = input
            .stats
            .get(stat::SHELLS.saturating_add(usize::try_from(i).unwrap_or(0)))
            .copied()
            .unwrap_or(0);
        for (d, c) in three(value).into_iter().enumerate() {
            let Some(c) = c else { continue };
            let x = (6_i32
                .saturating_mul(i)
                .saturating_add(i32::try_from(d).unwrap_or(0))
                .saturating_add(1))
            .saturating_mul(8)
            .saturating_sub(2);
            // The small yellow digits are characters 18 to 27.
            bar_char(out, x, -24, 18_u8.saturating_add(c.saturating_sub(b'0')));
        }
    }
    for (i, item) in HUD_ITEMS.iter().enumerate() {
        let bit = 1_i32.wrapping_shl(17_u32.saturating_add(u32::try_from(i).unwrap_or(0)));
        if input.items & bit != 0 {
            let x = 192_i32.saturating_add(i32::try_from(i).unwrap_or(0).saturating_mul(16));
            pic(out, Anchor::Bar, item, x, -16);
        }
    }
    for i in 0..4_u32 {
        let bit = 1_i32.wrapping_shl(28_u32.saturating_add(i));
        if input.items & bit != 0 {
            let x = (320 - 32_i32).saturating_add(i32::try_from(i).unwrap_or(0).saturating_mul(8));
            pic(
                out,
                Anchor::Bar,
                &format!("sb_sigil{}", i.saturating_add(1)),
                x,
                -16,
            );
        }
    }
}

/// `Sbar_DrawFrags` (HUD-1.5).
fn frags(out: &mut Vec<Placed>, input: &HudInput<'_>, screen_height: i32) {
    let order = sort_frags(input.scores);
    let mut x = 23_i32;
    let y = screen_height
        .saturating_sub(HUD_SBAR_HEIGHT)
        .saturating_sub(23);
    for &k in order.iter().take(4) {
        let Some(s) = input.scores.get(k) else {
            continue;
        };
        let top = color_for_map(s.colors & 0xf0);
        let bottom = color_for_map((s.colors & 15) << 4);
        let block_x = x.saturating_mul(8).saturating_add(10);
        out.push(Placed {
            anchor: Anchor::Bar,
            item: HudItem::Fill {
                x: block_x,
                y: y.saturating_sub(screen_height.saturating_sub(HUD_SBAR_HEIGHT)),
                w: 28,
                h: 4,
                color: top,
            },
        });
        out.push(Placed {
            anchor: Anchor::Bar,
            item: HudItem::Fill {
                x: block_x,
                y: y.saturating_add(4)
                    .saturating_sub(screen_height.saturating_sub(HUD_SBAR_HEIGHT)),
                w: 28,
                h: 3,
                color: bottom,
            },
        });
        let text = format!("{:3}", s.frags);
        for (j, ch) in text.bytes().enumerate() {
            let column = x
                .saturating_add(1)
                .saturating_add(i32::try_from(j).unwrap_or(0));
            bar_char(out, column.saturating_mul(8), -24, ch);
        }
        if k == input.viewentity.wrapping_sub(1) {
            bar_char(out, x.saturating_mul(8).saturating_add(2), -24, 16);
            bar_char(
                out,
                x.saturating_add(4).saturating_mul(8).saturating_sub(4),
                -24,
                17,
            );
        }
        x = x.saturating_add(4);
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
        let f = if health >= 100 {
            4
        } else {
            health.checked_div(20).unwrap_or(0).clamp(0, 4)
        };
        let n = 5_i32.saturating_sub(f);
        if input.time <= input.faceanimtime {
            format!("face_p{n}")
        } else {
            format!("face{n}")
        }
    };
    pic(out, Anchor::Bar, &name, 112, 0);
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
    bar_string(out, 8, 4, text.as_bytes());
    let text = format!(
        "Secrets :{:3} /{:3}",
        stats[stat::SECRETS],
        stats[stat::TOTALSECRETS]
    );
    bar_string(out, 8, 12, text.as_bytes());
    let minutes = float_to_int(input.time / 60.0);
    let seconds = float_to_int(input.time - 60.0 * int_to_float(minutes));
    let tens = seconds.checked_div(10).unwrap_or(0);
    let units = seconds.saturating_sub(10_i32.saturating_mul(tens));
    let text = format!("Time :{minutes:3}:{tens}{units}");
    bar_string(out, 184, 4, text.as_bytes());
    let l = i32::try_from(input.levelname.len()).unwrap_or(0);
    bar_string(
        out,
        232_i32.saturating_sub(l.saturating_mul(4)),
        12,
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
            y: 8,
        },
    });
    let order = sort_frags(input.scores);
    let x = 80;
    let mut y = 40;
    for &k in &order {
        let Some(s) = input.scores.get(k) else {
            continue;
        };
        let top = color_for_map(s.colors & 0xf0);
        let bottom = color_for_map((s.colors & 15) << 4);
        out.push(Placed {
            anchor: Anchor::Centered,
            item: HudItem::Fill {
                x,
                y,
                w: 40,
                h: 4,
                color: top,
            },
        });
        out.push(Placed {
            anchor: Anchor::Centered,
            item: HudItem::Fill {
                x,
                y: y.saturating_add(4),
                w: 40,
                h: 4,
                color: bottom,
            },
        });
        let text = format!("{:3}", s.frags);
        for (j, ch) in text.bytes().enumerate() {
            out.push(Placed {
                anchor: Anchor::Centered,
                item: HudItem::Char {
                    x: (x.saturating_add(8))
                        .saturating_add(i32::try_from(j).unwrap_or(0).saturating_mul(8)),
                    y,
                    ch,
                },
            });
        }
        if k == input.viewentity.wrapping_sub(1) {
            out.push(Placed {
                anchor: Anchor::Centered,
                item: HudItem::Char {
                    x: x.saturating_sub(8),
                    y,
                    ch: 12,
                },
            });
        }
        for (j, &ch) in s.name.iter().enumerate() {
            out.push(Placed {
                anchor: Anchor::Centered,
                item: HudItem::Char {
                    x: (x.saturating_add(64))
                        .saturating_add(i32::try_from(j).unwrap_or(0).saturating_mul(8)),
                    y,
                    ch,
                },
            });
        }
        y = y.saturating_add(10);
    }
}

/// `Sbar_IntermissionOverlay` and `Sbar_FinaleOverlay` (HUD-2.3): the overlay for the
/// intermission `kind`, 1 for the intermission and 2 or 3 for the finale and cutscene.
#[must_use]
pub fn intermission_overlay(input: &HudInput<'_>) -> Vec<Placed> {
    let mut out = Vec::new();
    if input.intermission != 1 {
        out.push(Placed {
            anchor: Anchor::Screen,
            item: HudItem::Lmp {
                path: "gfx/finale.lmp".to_owned(),
                x: None,
                y: 16,
            },
        });
        return out;
    }
    if input.gametype == HUD_GAME_DEATHMATCH {
        deathmatch_overlay(&mut out, input);
        return out;
    }
    out.push(Placed {
        anchor: Anchor::Screen,
        item: HudItem::Lmp {
            path: "gfx/complete.lmp".to_owned(),
            x: Some(64),
            y: 24,
        },
    });
    out.push(Placed {
        anchor: Anchor::Screen,
        item: HudItem::Lmp {
            path: "gfx/inter.lmp".to_owned(),
            x: Some(0),
            y: 56,
        },
    });
    let dig = float_to_int(input.completed_time / 60.0);
    number(&mut out, Anchor::Screen, 160, 64, dig, 3, false);
    let num = float_to_int(input.completed_time - int_to_float(dig.saturating_mul(60)));
    pic(&mut out, Anchor::Screen, "num_colon", 234, 64);
    pic(
        &mut out,
        Anchor::Screen,
        &format!("num_{}", num.checked_div(10).unwrap_or(0)),
        246,
        64,
    );
    pic(
        &mut out,
        Anchor::Screen,
        &format!("num_{}", num.checked_rem(10).unwrap_or(0)),
        266,
        64,
    );
    let stats = input.stats;
    number(
        &mut out,
        Anchor::Screen,
        160,
        104,
        stats[stat::SECRETS],
        3,
        false,
    );
    pic(&mut out, Anchor::Screen, "num_slash", 232, 104);
    number(
        &mut out,
        Anchor::Screen,
        240,
        104,
        stats[stat::TOTALSECRETS],
        3,
        false,
    );
    number(
        &mut out,
        Anchor::Screen,
        160,
        144,
        stats[stat::MONSTERS],
        3,
        false,
    );
    pic(&mut out, Anchor::Screen, "num_slash", 232, 144);
    number(
        &mut out,
        Anchor::Screen,
        240,
        144,
        stats[stat::TOTALMONSTERS],
        3,
        false,
    );
    out
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
    /// characters to draw this frame on a screen `width` by `height`, none once the time
    /// is up outside an intermission; `frametime` is taken off the time first.
    pub fn draw(
        &mut self,
        frametime: f32,
        time: f32,
        intermission: bool,
        width: i32,
        height: i32,
    ) -> Vec<HudItem> {
        self.time_off -= frametime;
        if self.time_off <= 0.0 && !intermission {
            return Vec::new();
        }
        let mut remaining = if intermission {
            float_to_int(HUD_PRINTSPEED * (time - self.start))
        } else {
            9999
        };
        let mut out = Vec::new();
        let mut y = if self.lines <= 4 {
            float_to_int(int_to_float(height) * 0.35)
        } else {
            48
        };
        for line in self.text.split(|&b| b == b'\n') {
            let shown = line
                .get(..line.len().min(HUD_CENTER_LINE_LENGTH))
                .unwrap_or_default();
            let l = i32::try_from(shown.len()).unwrap_or(0);
            let mut x = width
                .saturating_sub(l.saturating_mul(8))
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
                x = x.saturating_add(8);
            }
            y = y.saturating_add(8);
        }
        out
    }
}
