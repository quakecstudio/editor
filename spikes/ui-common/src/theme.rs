// SPDX-License-Identifier: GPL-3.0-or-later
//! Dark theme colours from the approved mockups (`docs/design/mockups/06-theme-and-chrome.png`
//! and `Tokens.dc.html`), as `0xRRGGBB`.

use crate::document::RowKind;
use crate::lexer::TokenKind;

pub const SURFACE_EDITOR: u32 = 0x1A_1918;
pub const SURFACE_PANEL: u32 = 0x21_1F1E;
pub const SURFACE_RAISED: u32 = 0x2A_2826;
pub const TEXT: u32 = 0xE8_E3DC;
pub const TEXT_MUTED: u32 = 0xAB_A49A;
pub const TEXT_FAINT: u32 = 0x8E_877D;
pub const ACCENT: u32 = 0xEE_8B45;
pub const FOCUS: u32 = 0x6F_A8DC;
pub const OK: u32 = 0x8C_C47E;
pub const ERROR: u32 = 0xEC_7A6C;
pub const DIFF_ADDED_BG: u32 = 0x22_301E;
pub const DIFF_REMOVED_BG: u32 = 0x3A_2220;
pub const SELECTION_LINE_BG: u32 = 0x24_2220;

/// Code colours: keywords copper, types and vectors teal, functions gold (builtins italic),
/// constants and numbers lilac, strings green, comments grey italic.
#[must_use]
pub fn token_color(kind: TokenKind) -> u32 {
    match kind {
        TokenKind::Keyword => 0xE0_956A,
        TokenKind::Type | TokenKind::Vector => 0x86_BFD0,
        TokenKind::Function | TokenKind::Builtin => 0xE8_C77E,
        TokenKind::Constant | TokenKind::Number => 0xCD_A4DE,
        TokenKind::String => 0xA9_CB82,
        TokenKind::Comment => 0x93_8C82,
        TokenKind::Field | TokenKind::Identifier | TokenKind::Punctuation => TEXT,
        TokenKind::Whitespace => TEXT_FAINT,
    }
}

/// The background of a display row, if it differs from the editor surface.
#[must_use]
pub fn row_background(kind: RowKind) -> Option<u32> {
    match kind {
        RowKind::Normal => None,
        RowKind::Added => Some(DIFF_ADDED_BG),
        RowKind::Removed => Some(DIFF_REMOVED_BG),
    }
}

/// Splits `0xRRGGBB` into linear-ish `[r, g, b]` floats for frameworks that want them.
#[must_use]
pub fn rgb_f32(color: u32) -> [f32; 3] {
    let channel =
        |shift: u32| f32::from(u8::try_from((color >> shift) & 0xFF).unwrap_or(0)) / 255.0;
    [channel(16), channel(8), channel(0)]
}
