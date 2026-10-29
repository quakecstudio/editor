// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/pr_comp.c, PR_ParseFunctionCall, PrecacheSound, PrecacheModel, PrecacheFile

//! The precache lists (QC-26): the files a program's `precache_sound`, `precache_model` and
//! `precache_file` calls name, which `qcc -copy` and `-pak` collect. They are not part of the
//! progs.

use crate::program::Program;

/// One file a precache call names, and the pak block it goes in (QC-26.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Precache {
    /// The name, the compile-time string of the call's first argument (QC-26.1).
    pub name: Vec<u8>,
    /// The pak block, 1 to 9 (QC-26.2).
    pub block: u8,
}

/// The three precache lists, each in the order the names were first met (QC-26.1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Precaches {
    /// `precache_sound` names.
    pub sounds: Vec<Precache>,
    /// `precache_model` names.
    pub models: Vec<Precache>,
    /// `precache_file` names.
    pub files: Vec<Precache>,
}

impl Precaches {
    /// The check `PR_ParseFunctionCall` makes of a call's first argument, at global `ofs`,
    /// when the called definition is named `function` (QC-26.1): a name starting
    /// `precache_sound`, `precache_model` or `precache_file` adds the argument's compile-time
    /// string to that list, once per distinct string, in the block the byte after the prefix
    /// names (QC-26.2). An argument at global 0 adds nothing (`if (!e->ofs) return;`), and one
    /// whose word is no offset into the string table adds nothing (QC-26.5).
    pub(crate) fn record(&mut self, function: &str, program: &Program, ofs: u32) {
        let name = function.as_bytes();
        let (list, prefix) = if name.starts_with(b"precache_sound") {
            (&mut self.sounds, 14)
        } else if name.starts_with(b"precache_model") {
            (&mut self.models, 14)
        } else if name.starts_with(b"precache_file") {
            (&mut self.files, 13)
        } else {
            return;
        };
        if ofs == 0 {
            return;
        }
        let Some(text) = program.string_at(program.word(ofs)) else {
            return;
        };
        if list.iter().any(|p| p.name == text) {
            return;
        }
        let block = match name.get(prefix) {
            Some(&digit @ b'1'..=b'9') => digit.saturating_sub(b'0'),
            _ => 1,
        };
        list.push(Precache {
            name: text.to_vec(),
            block,
        });
    }
}
