// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-2001 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/pr_edict.c, PR_PatchRereleaseBuiltins

//! The 2021 rerelease's builtins: the functions its progs names that the engine gives builtin
//! numbers when it loads the progs, and the numbers the rerelease's table adds (RRE-4).

/// `exbuiltins`: each function name, the `first_statement` it must have, and the one it is
/// given, `-N` for builtin `N` (RRE-4.1).
const EX_BUILTINS: [(&str, i32, i32); 21] = [
    // "Update-1 adds the following builtins with new ids. Patch them to use old indices."
    ("centerprint", -90, -73),
    ("bprint", -91, -23),
    ("sprint", -92, -24),
    // "Update-3 changes its unique builtins to be looked up by name instead of builtin
    // numbers, to avoid conflict with other engines."
    ("ex_centerprint", 0, -73),
    ("ex_bprint", 0, -23),
    ("ex_sprint", 0, -24),
    ("ex_finaleFinished", 0, -79),
    ("ex_localsound", 0, -80),
    ("ex_draw_point", 0, -81),
    ("ex_draw_line", 0, -82),
    ("ex_draw_arrow", 0, -83),
    ("ex_draw_ray", 0, -84),
    ("ex_draw_circle", 0, -85),
    ("ex_draw_bounds", 0, -86),
    ("ex_draw_worldtext", 0, -87),
    ("ex_draw_sphere", 0, -88),
    ("ex_draw_cylinder", 0, -89),
    ("ex_CheckPlayerEXFlags", 0, -90),
    ("ex_walkpathtogoal", 0, -91),
    ("ex_bot_movetopoint", 0, -92),
    ("ex_bot_followentity", 0, -92),
];

/// The rerelease's builtin table runs to 92: `finaleFinished` (79), `localsound` (80), the
/// `draw_*` slots (81 to 89), `CheckPlayerEXFlags` (90), `walkpathtogoal` (91) and one more
/// `PF_Fixme` (92) after id's 78 (RRE-4.2).
pub const RRE_NUM_BUILTINS: u32 = 93;

/// The builtin table a progs runs with (RRE-4.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BuiltinTable {
    /// id's 79 builtins (LIMIT-5.1), for a progs [`patch_builtins`] did not change.
    #[default]
    Id,
    /// The rerelease's, to 92, for a progs whose named functions [`patch_builtins`] gave
    /// builtins.
    Rerelease,
}

/// The size of a function record, `dfunction_t` (PROGS-6).
const FUNCTION_SIZE: usize = 36;

/// `PR_PatchRereleaseBuiltins` on the bytes of a progs (RRE-4.1): for each row of the table,
/// the first function of the name, `ED_FindFunction`'s, is given the row's builtin when its
/// `first_statement` is the row's. Returns the table the progs then runs with: the
/// rerelease's when any function was patched, else id's. A progs whose header or function
/// table cannot be read is left as it is.
pub fn patch_builtins(bytes: &mut [u8]) -> BuiltinTable {
    let (table, patches) = {
        let Ok(progs) = qc_progs::Progs::read(bytes) else {
            return BuiltinTable::Id;
        };
        let Ok(table) = usize::try_from(progs.header().ofs_functions) else {
            return BuiltinTable::Id;
        };
        let mut patches = Vec::new();
        for (name, first, patch) in EX_BUILTINS {
            let found = progs
                .functions()
                .iter()
                .enumerate()
                .find(|(_, f)| progs.string(f.s_name) == Some(name.as_bytes()));
            if let Some((index, f)) = found
                && f.first_statement == first
            {
                patches.push((index, patch));
            }
        }
        (table, patches)
    };
    let mut patched = BuiltinTable::Id;
    for (index, patch) in patches {
        let at = index
            .checked_mul(FUNCTION_SIZE)
            .and_then(|offset| table.checked_add(offset));
        if let Some(cell) = at.and_then(|at| bytes.get_mut(at..at.saturating_add(4))) {
            cell.copy_from_slice(&patch.to_le_bytes());
            patched = BuiltinTable::Rerelease;
        }
    }
    patched
}

#[cfg(test)]
mod tests;
