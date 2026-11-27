// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads each engine's map limits from its pinned C files (ENGT-5): the files of
//! `sources.toml`'s `[[limit_file]]` entries, each of the repository and commit of a builtin
//! source. A limit is read from a quote of the file's code, which must occur in it exactly once;
//! only the number is kept (ENGT-5.3), never the quote's text, which stays in `sources.toml`
//! and the spec.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Deserialize;

use super::{Pin, c};

/// The engines that have limit files (ENGT-5.1); id's limits are written from BSP-16 and
/// LIMIT-6 in `qc-engines`, and KEX has no source.
const ENGINES: [(&str, &str); 5] = [
    ("quakespasm", "QuakeSpasm"),
    ("ironwail", "Ironwail"),
    ("vkquake", "VkQuake"),
    ("fteqw", "Fteqw"),
    ("darkplaces", "DarkPlaces"),
];

/// Each limit's id in `sources.toml` and its `MapLimit` variant (ENGT-5.4).
const LIMITS: [(&str, &str); 22] = [
    ("models", "Models"),
    ("entities", "Entities"),
    ("entity_text", "EntityText"),
    ("planes", "Planes"),
    ("nodes", "Nodes"),
    ("clipnodes", "Clipnodes"),
    ("leafs", "Leafs"),
    ("visleafs", "Visleafs"),
    ("vertexes", "Vertexes"),
    ("faces", "Faces"),
    ("marksurfaces", "Marksurfaces"),
    ("texinfo", "Texinfo"),
    ("edges", "Edges"),
    ("surfedges", "Surfedges"),
    ("textures", "Textures"),
    ("lighting", "Lighting"),
    ("visibility", "Visibility"),
    ("face_extent", "FaceExtent"),
    ("edicts", "Edicts"),
    ("model_precaches", "ModelPrecaches"),
    ("sound_precaches", "SoundPrecaches"),
    ("light_styles", "LightStyles"),
];

/// What an engine does with a map past a limit, by its id and `Past` variant (ENGT-5.5).
const PASTS: [(&str, &str); 4] = [
    ("refused", "Refused"),
    ("warned", "Warned"),
    ("tools", "Tools"),
    ("undefined", "Undefined"),
];

/// The texels of one luxel, a light sample (BSP-10).
const TEXELS_PER_LUXEL: u64 = 16;
/// The values of an `unsigned short`.
const UNSIGNED_SHORT_VALUES: u64 = 1 << 16;

/// One file of an engine's limits.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LimitFile {
    /// The builtin source whose repository, commit and license the file shares.
    pub(crate) source: String,
    /// The engine whose limits it gives.
    pub(crate) engine: String,
    pub(crate) path: String,
    pub(crate) sha256: String,
    /// A quote of code that shows the engine reads BSP2.
    #[serde(default)]
    pub(crate) bsp2: Option<String>,
    #[serde(default)]
    pub(crate) limits: Vec<LimitRow>,
}

/// How a limit's number is taken (ENGT-5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Reading {
    /// The number is the most.
    Number,
    /// The number is a list's entries, entry 0 kept empty: one fewer is the most.
    Entries,
    /// The number is a lightmap's most luxels on an axis, so the most texels are one fewer
    /// times 16.
    Luxels,
    /// The quote shifts its first number left by its second.
    Shift,
    /// The quote declares an index type `unsigned short`: 65536 values.
    UnsignedShort,
}

impl Reading {
    /// The most `numbers` give.
    fn most(self, numbers: &[u64]) -> Option<u64> {
        let first = numbers.first().copied();
        match self {
            Self::Number => first,
            Self::Entries => first?.checked_sub(1),
            Self::Luxels => first?.checked_sub(1)?.checked_mul(TEXELS_PER_LUXEL),
            Self::Shift => {
                let shift = u32::try_from(*numbers.get(1)?).ok()?;
                first?.checked_shl(shift)
            }
            Self::UnsignedShort => Some(UNSIGNED_SHORT_VALUES),
        }
    }
}

/// One limit of a file.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LimitRow {
    pub(crate) limit: String,
    pub(crate) quote: String,
    /// The part of the quote the number is read from; the whole quote when not given.
    #[serde(default)]
    pub(crate) at: Option<String>,
    pub(crate) reading: Reading,
    pub(crate) past: String,
    /// `bsp29` when the check reads BSP29 lumps only.
    #[serde(default)]
    pub(crate) applies: Option<String>,
}

/// A limit read from a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fact {
    pub(crate) limit: &'static str,
    pub(crate) most: u64,
    pub(crate) past: &'static str,
    pub(crate) bsp29_only: Bsp29Only,
    pub(crate) source: String,
    pub(crate) path: String,
}

/// Whether a fact's check reads BSP29 lumps only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bsp29Only {
    Yes,
    No,
}

/// What one file gives.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FileFacts {
    pub(crate) facts: Vec<Fact>,
    /// Whether the file shows the engine reads BSP2.
    pub(crate) bsp2: Bsp2Shown,
}

/// Whether a file's `bsp2` quote shows the engine reads BSP2 (ENGT-5.6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Bsp2Shown {
    Shown,
    #[default]
    NotShown,
}

/// The variant named `id` in `table`.
fn variant(table: &[(&str, &'static str)], id: &str, what: &str) -> Result<&'static str, String> {
    table
        .iter()
        .find(|(name, _)| *name == id)
        .map(|&(_, variant)| variant)
        .ok_or_else(|| format!("unknown {what} {id:?}"))
}

/// Checks a limit file against the builtin sources (ENGT-5.1): its source is one of `pins`,
/// its engine one of [`ENGINES`], and each row names a known limit and past.
pub(crate) fn check(file: &LimitFile, pins: &[Pin]) -> Result<(), String> {
    let what = format!("{} {}", file.source, file.path);
    if !pins.iter().any(|p| p.id == file.source) {
        return Err(format!("{what}: no source {:?}", file.source));
    }
    variant(&ENGINES, &file.engine, "engine").map_err(|e| format!("{what}: {e}"))?;
    if file.limits.is_empty() && file.bsp2.is_none() {
        return Err(format!("{what}: gives no limit and no bsp2 quote"));
    }
    for row in &file.limits {
        variant(&LIMITS, &row.limit, "limit").map_err(|e| format!("{what}: {e}"))?;
        variant(&PASTS, &row.past, "past").map_err(|e| format!("{what}: {e}"))?;
        if row.applies.as_deref().is_some_and(|a| a != "bsp29") {
            return Err(format!("{what}: applies is bsp29 or not given"));
        }
    }
    Ok(())
}

/// The facts of `file`, whose text is `text` (ENGT-5.2, ENGT-5.3).
pub(crate) fn read(file: &LimitFile, text: &str) -> Result<FileFacts, String> {
    let what = format!("{} {}", file.source, file.path);
    let mut out = FileFacts::default();
    if let Some(quote) = &file.bsp2 {
        c::quoted_numbers(text, quote, quote).map_err(|e| format!("{what}: {e}"))?;
        out.bsp2 = Bsp2Shown::Shown;
    }
    for row in &file.limits {
        let at = row.at.as_deref().unwrap_or(&row.quote);
        let numbers =
            c::quoted_numbers(text, &row.quote, at).map_err(|e| format!("{what}: {e}"))?;
        let most = row
            .reading
            .most(&numbers)
            .ok_or_else(|| format!("{what}: {at:?} gives no number to read"))?;
        out.facts.push(Fact {
            limit: variant(&LIMITS, &row.limit, "limit")?,
            most,
            past: variant(&PASTS, &row.past, "past")?,
            bsp29_only: if row.applies.is_some() {
                Bsp29Only::Yes
            } else {
                Bsp29Only::No
            },
            source: file.source.clone(),
            path: file.path.clone(),
        });
    }
    Ok(out)
}

/// Writes each engine's limit table and whether it reads BSP2 (before `rustfmt`), the
/// engines in [`ENGINES`]'s order and each engine's facts in file order.
pub(crate) fn render(out: &mut String, read: &[(LimitFile, FileFacts)]) {
    let mut by_engine: BTreeMap<&str, (Vec<&Fact>, Bsp2Shown)> = BTreeMap::new();
    for (file, facts) in read {
        let entry = by_engine.entry(file.engine.as_str()).or_default();
        entry.0.extend(&facts.facts);
        if facts.bsp2 == Bsp2Shown::Shown {
            entry.1 = Bsp2Shown::Shown;
        }
    }
    for (id, _) in ENGINES {
        let (facts, bsp2) = by_engine.get(id).cloned().unwrap_or_default();
        let name = id.to_ascii_uppercase();
        _ = writeln!(
            out,
            "/// The map limits read from `{id}`'s limit files (ENGT-5), in file order.\n\
             pub(crate) const {name}_LIMITS: &[LimitFact] = &["
        );
        for fact in facts {
            let applies = match fact.bsp29_only {
                Bsp29Only::Yes => "Applies::Bsp29",
                Bsp29Only::No => "Applies::AllFormats",
            };
            _ = writeln!(
                out,
                "LimitFact {{ limit: MapLimit::{}, most: {}, past: Past::{}, applies: {applies}, \
                 source: {:?}, path: {:?} }},",
                fact.limit, fact.most, fact.past, fact.source, fact.path
            );
        }
        out.push_str("];\n\n");
        let reads = match bsp2 {
            Bsp2Shown::Shown => "Bsp2::Reads",
            Bsp2Shown::NotShown => "Bsp2::Unknown",
        };
        _ = writeln!(
            out,
            "/// Whether `{id}` reads BSP2 (ENGT-5.6).\n\
             pub(crate) const {name}_BSP2: Bsp2 = {reads};\n"
        );
    }
}

#[cfg(test)]
mod tests;
