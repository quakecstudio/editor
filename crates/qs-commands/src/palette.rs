// SPDX-License-Identifier: GPL-3.0-or-later

//! The command palette's search (UI-10): every command of a registry matched against what the
//! person typed, by its summary and its id, best first.

use std::ops::Range;

use crate::registry::CommandInfo;

/// One command the query matches, and where.
#[derive(Clone, Debug, PartialEq)]
pub struct PaletteMatch<'a> {
    /// The command.
    pub info: &'a CommandInfo,
    /// The bytes of its summary the query's characters matched, merged into runs.
    pub summary_hits: Vec<Range<usize>>,
    /// The bytes of its id the query's characters matched, merged into runs.
    pub id_hits: Vec<Range<usize>>,
    score: i64,
}

/// Every command of `commands` that `query` matches, best first; an empty query matches every
/// command, in the order given.
///
/// A command matches when the query's characters, ignoring case and spaces, appear in order in
/// its summary or in its id, as Zed's and VS Code's palettes match. A match scores more for
/// characters that follow one another, that start a word, that come early, and for being in
/// the summary rather than the id; a command whose id is the query, in any case, comes first; of
/// two that score the same, the shorter summary comes first, then the order given.
#[must_use]
pub fn search<'a>(
    commands: impl IntoIterator<Item = &'a CommandInfo>,
    query: &str,
) -> Vec<PaletteMatch<'a>> {
    let needle: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    let mut found: Vec<PaletteMatch<'a>> = Vec::new();
    let empty = needle.is_empty();
    for info in commands {
        let summary = fuzzy(info.summary(), &needle);
        let id = fuzzy(info.id().as_str(), &needle);
        let (score, summary_hits, id_hits) = match (summary, id) {
            (None, None) => continue,
            (Some((s, hits)), None) => (s.saturating_add(SUMMARY_BONUS), hits, Vec::new()),
            (None, Some((s, hits))) => (s, Vec::new(), hits),
            (Some((s, summary)), Some((i, id))) => {
                (s.saturating_add(SUMMARY_BONUS).max(i), summary, id)
            }
        };
        found.push(PaletteMatch {
            info,
            summary_hits,
            id_hits,
            score,
        });
    }
    if !empty {
        let query: String = needle.iter().collect();
        found.sort_by_key(|m| {
            let named = m.info.id().as_str().to_lowercase() == query;
            (!named, std::cmp::Reverse(m.score), m.info.summary().len())
        });
    }
    found
}

/// One item another finder's query matches, such as a file of the project, and where.
#[derive(Clone, Debug, PartialEq)]
pub struct Found<'a, T> {
    /// The item.
    pub item: &'a T,
    /// The bytes of its name the query's characters matched, merged into runs.
    pub name_hits: Vec<Range<usize>>,
    /// The bytes of its longer text the query's characters matched, merged into runs.
    pub text_hits: Vec<Range<usize>>,
    score: i64,
}

/// Every item of `items` that `query` matches, best first. `texts` gives each item's name and
/// a longer text, such as a file's name and its path: they are matched as [`search`] matches a
/// command's summary and its id, the name scoring more, as a finder of files puts a match in
/// the file's name before one in its folders. Of two that score the same, the shorter text
/// comes first, then the order given. An empty query matches every item, in the order given.
#[must_use]
pub fn find<'a, T>(
    items: impl IntoIterator<Item = &'a T>,
    texts: impl Fn(&T) -> (&str, &str),
    query: &str,
) -> Vec<Found<'a, T>> {
    let needle: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    let mut found: Vec<(usize, Found<'a, T>)> = Vec::new();
    for item in items {
        let (name, text) = texts(item);
        let (score, name_hits, text_hits) = match (fuzzy(name, &needle), fuzzy(text, &needle)) {
            (None, None) => continue,
            (Some((n, hits)), None) => (n.saturating_add(SUMMARY_BONUS), hits, Vec::new()),
            (None, Some((t, hits))) => (t, Vec::new(), hits),
            (Some((n, name_hits)), Some((t, text_hits))) => {
                (n.saturating_add(SUMMARY_BONUS).max(t), name_hits, text_hits)
            }
        };
        let item = Found {
            item,
            name_hits,
            text_hits,
            score,
        };
        found.push((text.len(), item));
    }
    if !needle.is_empty() {
        found.sort_by_key(|(len, m)| (std::cmp::Reverse(m.score), *len));
    }
    found.into_iter().map(|(_, m)| m).collect()
}

/// What a match in the summary is worth over the same match in the id.
const SUMMARY_BONUS: i64 = 5;
/// What a matched character is worth.
const CHAR: i64 = 1;
/// What a matched character right after the last one is worth on top.
const RUN: i64 = 8;
/// What a matched character at a word's start is worth on top.
const WORD_START: i64 = 6;
/// The most a match loses for starting late in the text.
const LATE: i64 = 10;

/// Matches `needle`, lowercase, against `text`: its score and the byte ranges matched, or `None`
/// when `text` does not hold the needle's characters in order. Two passes are tried and the
/// better kept: one where each character takes the place right after the last, else the
/// earliest that starts a word, else the earliest; and one where each takes the earliest, which
/// finds a match whenever there is one.
fn fuzzy(text: &str, needle: &[char]) -> Option<(i64, Vec<Range<usize>>)> {
    let words = pass(text, needle, Prefer::WordStarts);
    let earliest = pass(text, needle, Prefer::Earliest);
    match (words, earliest) {
        (Some(w), Some(e)) => Some(if e.0 > w.0 { e } else { w }),
        (w, e) => w.or(e),
    }
}

/// Where a pass of [`fuzzy`] places each character.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Prefer {
    /// Right after the last, else at a word's start, else the earliest.
    WordStarts,
    /// The earliest.
    Earliest,
}

/// One pass of [`fuzzy`].
fn pass(text: &str, needle: &[char], prefer: Prefer) -> Option<(i64, Vec<Range<usize>>)> {
    if needle.is_empty() {
        return Some((0, Vec::new()));
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let lower: Vec<char> = chars
        .iter()
        .map(|(_, c)| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let starts_word = |i: usize| {
        i == 0
            || i.checked_sub(1)
                .and_then(|p| chars.get(p))
                .is_some_and(|(_, c)| !c.is_alphanumeric())
    };
    let mut at = 0_usize;
    let mut last: Option<usize> = None;
    let mut score = 0_i64;
    let mut hits: Vec<Range<usize>> = Vec::new();
    for want in needle {
        let rest = at..lower.len();
        let next_char = last
            .map(|l| l.saturating_add(1))
            .filter(|n| lower.get(*n) == Some(want));
        let word = rest
            .clone()
            .find(|&i| lower.get(i) == Some(want) && starts_word(i));
        let any = rest.clone().find(|&i| lower.get(i) == Some(want));
        let i = match prefer {
            Prefer::WordStarts => next_char.or(word).or(any)?,
            Prefer::Earliest => any?,
        };
        score = score.saturating_add(CHAR);
        if Some(i) == last.map(|l| l.saturating_add(1)) {
            score = score.saturating_add(RUN);
        }
        if starts_word(i) {
            score = score.saturating_add(WORD_START);
        }
        if last.is_none() {
            let late = i64::try_from(i).unwrap_or(LATE).min(LATE);
            score = score.saturating_sub(late);
        }
        let (start, c) = chars.get(i).copied()?;
        let end = start.saturating_add(c.len_utf8());
        match hits.last_mut() {
            Some(prev) if prev.end == start => prev.end = end,
            Some(_) | None => hits.push(start..end),
        }
        last = Some(i);
        at = i.saturating_add(1);
    }
    Some((score, hits))
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use super::{find, fuzzy, search};
    use crate::{Command, NoParams, Outcome, Registry};

    #[derive(Debug)]
    struct Never;

    impl std::fmt::Display for Never {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("never")
        }
    }

    impl std::error::Error for Never {}

    macro_rules! command {
        ($name:ident, $id:literal, $summary:literal) => {
            struct $name;
            impl Command<()> for $name {
                const ID: &'static str = $id;
                const SUMMARY: &'static str = $summary;
                type Params = NoParams;
                type Output = ();
                type Error = Never;
                fn run(&self, _: &mut (), _: NoParams) -> Result<Outcome<(), ()>, Never> {
                    Ok(Outcome::unchanged(()))
                }
            }
        };
    }

    command!(Start, "play.start", "Play");
    command!(
        Here,
        "map.play_from_here",
        "Play from here in the map editor"
    );
    command!(
        Show,
        "engine.show_window",
        "Display the preview host window"
    );
    command!(Pause, "play.pause", "Pause the running game");

    fn registry() -> Registry<()> {
        let mut registry = Registry::new();
        registry.register(Start).unwrap();
        registry.register(Here).unwrap();
        registry.register(Show).unwrap();
        registry.register(Pause).unwrap();
        registry
    }

    fn ids(registry: &Registry<()>, query: &str) -> Vec<String> {
        search(registry.commands(), query)
            .iter()
            .map(|m| m.info.id().as_str().to_owned())
            .collect()
    }

    command!(
        Save,
        "map.save",
        "Writes the open map's edits to its file, and its entities beside a BSP"
    );
    command!(SavePrefab, "map.save_prefab", "Writes a prefab");

    // UI-10: a command whose id is the query comes before one that only starts with it, in any
    // case, though the other scores as well and has the shorter summary.
    #[test]
    fn the_command_named_comes_first() {
        let mut registry = Registry::new();
        registry.register(SavePrefab).unwrap();
        registry.register(Save).unwrap();
        assert_eq!(ids(&registry, "map.save"), ["map.save", "map.save_prefab"]);
        assert_eq!(ids(&registry, "MAP.SAVE"), ["map.save", "map.save_prefab"]);
        assert_eq!(
            ids(&registry, "map.save_prefab"),
            ["map.save_prefab"],
            "the other does not match"
        );
    }

    // UI-10: an empty query lists every command, in the registry's order.
    #[test]
    fn an_empty_query_lists_every_command() {
        let registry = registry();
        let all: Vec<String> = registry
            .commands()
            .map(|c| c.id().as_str().to_owned())
            .collect();
        assert_eq!(ids(&registry, ""), all);
        assert_eq!(ids(&registry, "  "), all, "spaces are ignored");
    }

    // UI-10: the query's characters in order, ignoring case, in the summary or the id; a
    // summary that starts with them comes before one that holds them later.
    #[test]
    fn a_query_matches_in_order_and_ranks_starts_first() {
        let registry = registry();
        let found = ids(&registry, "PLA");
        assert_eq!(found.first().map(String::as_str), Some("play.start"));
        assert!(found.contains(&"map.play_from_here".to_owned()));
        assert!(
            found.contains(&"engine.show_window".to_owned()),
            "Dis-pla-y"
        );
        assert!(!found.contains(&"edit.undo".to_owned()));
        assert_eq!(ids(&registry, "zzz"), Vec::<String>::new());
        assert_eq!(
            ids(&registry, "alp"),
            Vec::<String>::new(),
            "in order, not anywhere"
        );
        assert_eq!(
            ids(&registry, "play.pause").first().map(String::as_str),
            Some("play.pause"),
            "a command's own id finds it first"
        );
        assert_eq!(
            ids(&registry, "pfh").first().map(String::as_str),
            Some("map.play_from_here"),
            "word starts"
        );
    }

    // UI-10: the matched characters are given as byte ranges, merged into runs.
    #[test]
    fn the_hits_are_runs_of_bytes() {
        let registry = registry();
        let found = search(registry.commands(), "pla");
        let start = found
            .iter()
            .find(|m| m.info.id().as_str() == "play.start")
            .unwrap();
        assert_eq!(start.summary_hits, vec![0_usize..3]);
        assert_eq!(start.id_hits, vec![0_usize..3]);
        let show = found
            .iter()
            .find(|m| m.info.id().as_str() == "engine.show_window")
            .unwrap();
        assert_eq!(show.summary_hits, vec![3_usize..6], "Dis-pla-y");
        assert!(show.id_hits.is_empty());
        let here = search(registry.commands(), "pf")
            .into_iter()
            .find(|m| m.info.id().as_str() == "map.play_from_here")
            .unwrap();
        assert_eq!(here.summary_hits, [0..1, 5..6], "P-lay f-rom");
    }

    // UI-10: a match in the summary outranks the same match only in an id.
    #[test]
    fn the_summary_outranks_the_id() {
        let registry = registry();
        let found = ids(&registry, "running");
        assert_eq!(found, ["play.pause"]);
        let by_id = ids(&registry, "engine");
        assert_eq!(by_id, ["engine.show_window"]);
    }

    fn needle(query: &str) -> Vec<char> {
        query.chars().collect()
    }

    // UI-10: a matched character scores 1, 8 more right after the last, 6 more at a word's
    // start, and the match loses a point for each character before it starts, up to 10.
    #[test]
    fn a_match_scores_its_runs_word_starts_and_start() {
        assert_eq!(
            fuzzy("ab", &needle("ab")).map(|m| m.0),
            Some(16),
            "1+6, then 1+8"
        );
        assert_eq!(fuzzy("a b", &needle("b")).map(|m| m.0), Some(5), "1+6-2");
        assert_eq!(
            fuzzy("a-b", &needle("b")).map(|m| m.0),
            Some(5),
            "after punctuation"
        );
        assert_eq!(
            fuzzy("ab", &needle("b")).map(|m| m.0),
            Some(0),
            "1-1, inside a word"
        );
        assert_eq!(fuzzy("a", &needle("")).map(|m| m.0), Some(0));
        assert_eq!(fuzzy("a", &needle("b")), None);
        let late = "x".repeat(20) + "b";
        assert_eq!(
            fuzzy(&late, &needle("b")).map(|m| m.0),
            Some(-9),
            "at most 10 lost"
        );
    }

    // UI-10: of the two ways to place the characters, the better scoring is kept, whichever
    // it is.
    #[test]
    fn the_better_of_two_placements_is_kept() {
        let words = fuzzy("superb preview", &needle("pr")).unwrap();
        assert_eq!(words.1, vec![7_usize..9], "the run at a word's start");
        let earliest = fuzzy("display the preview player", &needle("pla")).unwrap();
        assert_eq!(earliest.1, vec![3_usize..6], "the run in Dis-pla-y");
    }

    // UI-10: a finder matches each item's name and its longer text as the palette matches a
    // summary and an id: a match in a file's name comes before one in its folders, the shorter
    // of two equal matches comes first, an empty query keeps the order given and no match is
    // no item
    #[test]
    fn find_ranks_any_items_by_their_name_then_their_text() {
        let files = [
            ("items.qc", "src/items.qc"),
            ("weapons.qc", "src/weapons.qc"),
            ("world.qc", "weapons/world.qc"),
            ("defs.qc", "defs.qc"),
        ];
        let paths = |query: &str| -> Vec<&str> {
            find(&files, |f| *f, query)
                .iter()
                .map(|m| m.item.1)
                .collect()
        };
        assert_eq!(paths(""), files.map(|f| f.1));
        assert_eq!(paths("weap"), ["src/weapons.qc", "weapons/world.qc"]);
        assert_eq!(paths("w o r l d"), ["weapons/world.qc"]);
        assert!(paths("zzz").is_empty());
        let found = find(&files, |f| *f, "defs");
        let defs = found.first().unwrap();
        assert_eq!(defs.name_hits, vec![0_usize..4]);
        assert_eq!(defs.text_hits, vec![0_usize..4]);
        let folder = find(&files, |f| *f, "src");
        let items = folder.first().unwrap();
        assert!(items.name_hits.is_empty(), "only the path holds it");
        assert_eq!(items.text_hits, vec![0_usize..3]);
        // Two equal matches: the shorter text first.
        let tied = [("x.qc", "aa/x.qc"), ("x.qc", "a/x.qc")];
        let order: Vec<&str> = find(&tied, |f| *f, "x").iter().map(|m| m.item.1).collect();
        assert_eq!(order, ["a/x.qc", "aa/x.qc"]);
    }
}
