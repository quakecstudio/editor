// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads Quake's models other than brush models: `.spr` sprites ([`spr`]) and the 2021
//! rerelease's `.md5mesh` and `.md5anim` enhanced models ([`md5`]). Alias models (`.mdl`)
//! join them in P5-02.
//!
//! Every reader takes the file's bytes, returns a view that borrows them where it can, and
//! rejects malformed input with an error, never a panic. The formats are specified in
//! `docs/spec/spr.md` (SPR-1, SPR-2) and `docs/spec/md5.md` (MD5-1 to MD5-3).

pub mod md5;
pub mod spr;

pub use md5::{Md5Anim, Md5Error, Md5Mesh};
pub use spr::{SprError, Sprite};
