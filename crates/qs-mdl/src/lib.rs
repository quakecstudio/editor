// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads Quake's models other than brush models: `.mdl` alias models ([`mdl`]), `.spr`
//! sprites ([`spr`]) and the 2021 rerelease's `.md5mesh` and `.md5anim` enhanced models
//! ([`md5`]).
//!
//! Every reader takes the file's bytes, returns a view that borrows them where it can, and
//! rejects malformed input with an error, never a panic. The formats are specified in
//! `docs/spec/mdl.md` (MDL-1 to MDL-5), `docs/spec/spr.md` (SPR-1, SPR-2) and `docs/spec/md5.md` (MD5-1 to MD5-3).

pub mod md5;
pub mod mdl;
pub mod spr;

pub use md5::{Md5Anim, Md5Error, Md5Mesh};
pub use mdl::{Mdl, MdlError};
pub use spr::{SprError, Sprite};
