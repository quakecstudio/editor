// SPDX-License-Identifier: GPL-3.0-or-later

//! Checks the WCAG AA contrast of the theme in `docs/design/tokens.toml`
//! (QA-9, UI-15, `docs/design/visual.md`).
//!
//! Every `[[contrast]]` rule lists foreground and background colour tokens;
//! each pair must reach the rule's `min` ratio in every theme, and no rule may
//! ask for less than [`AA`]. Every colour token must be in a rule or listed as
//! decorative, so a new colour cannot skip the check, and every theme must
//! define the same tokens. `[syntax]` entries must name colour tokens.
//!
//! The ratio is WCAG 2.2's: `(L1 + 0.05) / (L2 + 0.05)` over the relative
//! luminance of each sRGB colour, with no rounding.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use toml::{Table, Value};

use crate::{Report, repo};

pub(crate) const FILE: &str = "docs/design/tokens.toml";
/// WCAG 2.2 success criterion 1.4.3, level AA, for normal-size text.
const AA: f64 = 4.5;

pub(crate) fn check(root: &Path) -> Result<Report, String> {
    check_text(&repo::read(root, FILE)?)
}

/// An sRGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rgb([u8; 3]);

impl Rgb {
    /// Parses `#RRGGBB`.
    fn parse(text: &str) -> Option<Self> {
        let hex = text.strip_prefix('#')?;
        if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |index: usize| {
            hex.get(index..index.saturating_add(2))
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
        };
        Some(Self([channel(0)?, channel(2)?, channel(4)?]))
    }

    /// WCAG 2.2 relative luminance.
    fn luminance(self) -> f64 {
        let [red, green, blue] = self.0.map(|channel| {
            let value = f64::from(channel) / 255.0;
            if value <= 0.040_45 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        });
        0.2126 * red + 0.7152 * green + 0.0722 * blue
    }
}

/// The WCAG 2.2 contrast ratio of two colours, from 1 to 21.
fn ratio(first: Rgb, second: Rgb) -> f64 {
    let (first, second) = (first.luminance(), second.luminance());
    let (lighter, darker) = if first >= second {
        (first, second)
    } else {
        (second, first)
    };
    (lighter + 0.05) / (darker + 0.05)
}

/// One `[[contrast]]` rule.
struct Rule {
    name: String,
    min: f64,
    foreground: Vec<String>,
    background: Vec<String>,
}

fn check_text(text: &str) -> Result<Report, String> {
    let table: Table = text
        .parse()
        .map_err(|error| format!("{FILE} is not valid TOML: {error}"))?;
    let mut report = Report::default();
    let themes = themes(&table, &mut report)?;
    let rules = rules(&table, &mut report)?;
    let known: BTreeSet<&str> = themes
        .values()
        .flat_map(|colors| colors.keys().map(String::as_str))
        .collect();

    let mut covered = BTreeSet::new();
    for rule in &rules {
        for token in rule.foreground.iter().chain(&rule.background) {
            covered.insert(token.as_str());
            if !known.contains(token.as_str()) {
                report.problems.push(format!(
                    "{FILE}: contrast rule {:?} names {token:?}, which is not a colour token",
                    rule.name
                ));
            }
        }
    }
    for token in strings(
        table
            .get("decorative")
            .and_then(|value| value.get("tokens")),
    ) {
        covered.insert(token);
        if !known.contains(token) {
            report.problems.push(format!(
                "{FILE}: [decorative] names {token:?}, which is not a colour token"
            ));
        }
    }
    for token in &known {
        if !covered.contains(token) {
            report.problems.push(format!(
                "{FILE}: colour {token:?} is in no [[contrast]] rule; add it to one, or to \
                 [decorative] if it never carries text or a meaningful icon"
            ));
        }
    }
    check_syntax(&table, &known, &mut report);

    for (theme, colors) in &themes {
        for rule in &rules {
            check_rule(theme, colors, rule, &mut report);
        }
    }
    Ok(report)
}

/// The colour tokens of each theme in `[color.<theme>]`.
fn themes(
    table: &Table,
    report: &mut Report,
) -> Result<BTreeMap<String, BTreeMap<String, Rgb>>, String> {
    let Some(color) = table.get("color").and_then(Value::as_table) else {
        return Err(format!("{FILE} has no [color] table"));
    };
    if color.is_empty() {
        return Err(format!("{FILE}: [color] defines no themes"));
    }
    let mut themes = BTreeMap::new();
    for (theme, tokens) in color {
        let Some(tokens) = tokens.as_table() else {
            return Err(format!(
                "{FILE}: color.{theme} must be a table of colour tokens"
            ));
        };
        let mut colors = BTreeMap::new();
        for (token, value) in tokens {
            match value.as_str().and_then(Rgb::parse) {
                Some(rgb) => {
                    colors.insert(token.clone(), rgb);
                }
                None => report.problems.push(format!(
                    "{FILE}: color.{theme}.{token} must be a \"#RRGGBB\" string, not {value}"
                )),
            }
        }
        let names: BTreeSet<String> = tokens.keys().cloned().collect();
        themes.insert(theme.clone(), (colors, names));
    }
    let all: BTreeSet<String> = themes
        .values()
        .flat_map(|(_, names)| names.iter().cloned())
        .collect();
    for (theme, (_, names)) in &themes {
        for missing in all.difference(names) {
            report.problems.push(format!(
                "{FILE}: theme {theme} does not define {missing:?}; every theme defines every \
                 colour token"
            ));
        }
    }
    Ok(themes
        .into_iter()
        .map(|(theme, (colors, _))| (theme, colors))
        .collect())
}

fn rules(table: &Table, report: &mut Report) -> Result<Vec<Rule>, String> {
    let Some(entries) = table.get("contrast").and_then(Value::as_array) else {
        return Err(format!("{FILE} has no [[contrast]] rules"));
    };
    let mut rules = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .map_or_else(|| format!("#{}", index.saturating_add(1)), str::to_owned);
        let min = entry.get("min").and_then(|value| {
            value.as_float().or_else(|| {
                value
                    .as_integer()
                    .and_then(|int| i32::try_from(int).ok())
                    .map(f64::from)
            })
        });
        let Some(min) = min else {
            report.problems.push(format!(
                "{FILE}: contrast rule {name:?} needs a numeric min"
            ));
            continue;
        };
        if min < AA {
            report.problems.push(format!(
                "{FILE}: contrast rule {name:?} asks for {min}:1; WCAG AA needs at least {AA}:1"
            ));
        }
        let foreground: Vec<String> = strings(entry.get("foreground"))
            .map(str::to_owned)
            .collect();
        let background: Vec<String> = strings(entry.get("background"))
            .map(str::to_owned)
            .collect();
        if foreground.is_empty() || background.is_empty() {
            report.problems.push(format!(
                "{FILE}: contrast rule {name:?} needs a non-empty foreground and background list"
            ));
            continue;
        }
        rules.push(Rule {
            name,
            min,
            foreground,
            background,
        });
    }
    Ok(rules)
}

fn check_rule(theme: &str, colors: &BTreeMap<String, Rgb>, rule: &Rule, report: &mut Report) {
    let mut lowest: Option<(f64, &str, &str)> = None;
    let mut failed = false;
    for foreground in &rule.foreground {
        for background in &rule.background {
            let (Some(&fg), Some(&bg)) = (colors.get(foreground), colors.get(background)) else {
                failed = true;
                continue;
            };
            let value = ratio(fg, bg);
            if value < rule.min {
                failed = true;
                report.problems.push(format!(
                    "{theme}: {foreground} on {background} is {value:.2}:1, below {}:1 ({})",
                    rule.min, rule.name
                ));
            }
            if lowest.is_none_or(|(low, _, _)| value < low) {
                lowest = Some((value, foreground, background));
            }
        }
    }
    if let (false, Some((value, foreground, background))) = (failed, lowest) {
        report.passed.push(format!(
            "{theme}: {}: {} pairs at {}:1 or more, lowest {value:.2}:1 ({foreground} on \
             {background})",
            rule.name,
            rule.foreground.len().saturating_mul(rule.background.len()),
            rule.min
        ));
    }
}

fn check_syntax(table: &Table, known: &BTreeSet<&str>, report: &mut Report) {
    let Some(syntax) = table.get("syntax").and_then(Value::as_table) else {
        return;
    };
    for (category, entry) in syntax {
        let color = entry.get("color").and_then(Value::as_str);
        if !color.is_some_and(|color| known.contains(color)) {
            report.problems.push(format!(
                "{FILE}: syntax.{category} must name a colour token in its color field"
            ));
        }
    }
}

fn strings(value: Option<&Value>) -> impl Iterator<Item = &str> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use super::*;

    fn rgb(text: &str) -> Rgb {
        Rgb::parse(text).unwrap()
    }

    #[test]
    fn ratios_match_wcag() {
        assert!((ratio(rgb("#000000"), rgb("#FFFFFF")) - 21.0).abs() < 1e-9);
        assert!((ratio(rgb("#FFFFFF"), rgb("#000000")) - 21.0).abs() < 1e-9);
        assert!((ratio(rgb("#777777"), rgb("#777777")) - 1.0).abs() < 1e-9);
        // #767676 is the lightest grey that passes AA on white; #777777 just fails.
        assert!(ratio(rgb("#767676"), rgb("#FFFFFF")) >= AA);
        assert!(ratio(rgb("#777777"), rgb("#FFFFFF")) < AA);
        // The theme board's figures (mockups/06-theme-and-chrome.png), against surface.panel.
        let panel = rgb("#211F1E");
        for (color, board) in [
            ("#E8E3DC", 12.9),
            ("#ABA49A", 6.7),
            ("#8E877D", 4.6),
            ("#EE8B45", 6.6),
            ("#6FA8DC", 6.5),
            ("#8CC47E", 8.1),
            ("#E2B04A", 8.2),
            ("#EC7A6C", 5.9),
        ] {
            let value = ratio(rgb(color), panel);
            assert!((value - board).abs() < 0.06, "{color}: {value}");
        }
    }

    #[test]
    fn dark_channels_use_the_linear_segment() {
        let expected = 10.0 / 255.0 / 12.92;
        assert!((rgb("#0A0A0A").luminance() - expected).abs() < 1e-12);
    }

    fn rule(min: f64, foreground: &[&str], background: &[&str]) -> Rule {
        let names = |list: &[&str]| list.iter().map(|name| (*name).to_owned()).collect();
        Rule {
            name: "text".to_owned(),
            min,
            foreground: names(foreground),
            background: names(background),
        }
    }

    #[test]
    fn a_pair_exactly_at_the_minimum_passes() {
        let colors = BTreeMap::from([("a".to_owned(), rgb("#777777"))]);
        let mut report = Report::default();
        check_rule("dark", &colors, &rule(1.0, &["a"], &["a"]), &mut report);
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert_eq!(report.passed.len(), 1);
    }

    #[test]
    fn the_first_lowest_pair_is_reported() {
        let colors = BTreeMap::from([
            ("black".to_owned(), rgb("#000000")),
            ("grey".to_owned(), rgb("#777777")),
            ("grey2".to_owned(), rgb("#777777")),
            ("white".to_owned(), rgb("#FFFFFF")),
        ]);
        let mut report = Report::default();
        let pairs = rule(1.0, &["black", "grey", "grey2", "white"], &["white"]);
        check_rule("dark", &colors, &pairs, &mut report);
        assert_eq!(report.passed.len(), 1, "{report:?}");
        let line = report.passed.first().unwrap();
        assert!(line.contains("lowest 1.00:1 (white on white)"), "{line}");
        let mut report = Report::default();
        let pairs = rule(1.0, &["black", "grey", "grey2"], &["white"]);
        check_rule("dark", &colors, &pairs, &mut report);
        assert_eq!(report.passed.len(), 1, "{report:?}");
        let line = report.passed.first().unwrap();
        assert!(line.contains("(grey on white)"), "{line}");
    }

    #[test]
    fn colours_parse_only_as_six_hex_digits() {
        assert_eq!(Rgb::parse("#1a1918"), Some(Rgb([0x1A, 0x19, 0x18])));
        for bad in [
            "1A1918", "#1A191", "#1A19180", "#GG1918", "", "#", "#+1+2+3",
        ] {
            assert_eq!(Rgb::parse(bad), None, "{bad}");
        }
    }

    const PASSING: &str = r##"
[color.dark]
bg = "#000000"
fg = "#FFFFFF"
line = "#111111"
[color.light]
bg = "#FFFFFF"
fg = "#000000"
line = "#EEEEEE"
[[contrast]]
name = "text"
min = 4.5
foreground = ["fg"]
background = ["bg"]
[decorative]
tokens = ["line"]
[syntax]
plain = { color = "fg" }
"##;

    fn problems(text: &str) -> Vec<String> {
        check_text(text).unwrap().problems
    }

    #[test]
    fn a_theme_whose_pairs_pass_passes() {
        let report = check_text(PASSING).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert_eq!(report.passed.len(), 2, "{:?}", report.passed);
        assert!(
            report
                .passed
                .iter()
                .any(|line| line.contains("light: text: 1 pairs"))
        );
    }

    #[test]
    fn a_pair_below_aa_fails_and_names_theme_pair_and_ratio() {
        let text = PASSING.replace("fg = \"#000000\"", "fg = \"#777777\"");
        let found = problems(&text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found
                .first()
                .unwrap()
                .starts_with("light: fg on bg is 4.48:1, below 4.5:1"),
            "{found:?}"
        );
    }

    #[test]
    fn a_rule_below_aa_fails() {
        let text = PASSING.replace("min = 4.5", "min = 3");
        let found = problems(&text);
        assert!(
            found.iter().any(|line| line.contains("asks for 3:1")),
            "{found:?}"
        );
    }

    #[test]
    fn a_rule_without_min_or_pairs_fails() {
        assert!(
            problems(&PASSING.replace("min = 4.5\n", ""))
                .iter()
                .any(|line| line.contains("needs a numeric min"))
        );
        assert!(
            problems(&PASSING.replace("foreground = [\"fg\"]", "foreground = []"))
                .iter()
                .any(|line| line.contains("non-empty foreground"))
        );
    }

    #[test]
    fn an_unknown_token_in_a_rule_fails() {
        let found = problems(&PASSING.replace("background = [\"bg\"]", "background = [\"bgg\"]"));
        assert!(
            found
                .iter()
                .any(|line| line.contains("names \"bgg\", which is not")),
            "{found:?}"
        );
    }

    #[test]
    fn a_colour_in_no_rule_fails() {
        let found = problems(&PASSING.replace("tokens = [\"line\"]", "tokens = []"));
        assert!(
            found
                .iter()
                .any(|line| line.contains("colour \"line\" is in no")),
            "{found:?}"
        );
        let found = problems(&PASSING.replace("tokens = [\"line\"]", "tokens = [\"lin\"]"));
        assert!(
            found
                .iter()
                .any(|line| line.contains("[decorative] names \"lin\"")),
            "{found:?}"
        );
    }

    #[test]
    fn themes_must_define_the_same_tokens() {
        let found = problems(&PASSING.replace("line = \"#EEEEEE\"\n", ""));
        assert!(
            found
                .iter()
                .any(|line| line.contains("theme light does not define \"line\"")),
            "{found:?}"
        );
    }

    #[test]
    fn a_malformed_colour_fails() {
        let found = problems(&PASSING.replace("#EEEEEE", "#EEE"));
        assert!(
            found
                .iter()
                .any(|line| line.contains("color.light.line must be a \"#RRGGBB\"")),
            "{found:?}"
        );
    }

    #[test]
    fn syntax_must_name_a_colour_token() {
        let found = problems(&PASSING.replace("{ color = \"fg\" }", "{ color = \"ink\" }"));
        assert!(
            found.iter().any(|line| line.contains("syntax.plain")),
            "{found:?}"
        );
    }

    #[test]
    fn a_file_without_colours_or_rules_is_an_error() {
        assert!(check_text("x = 1").is_err());
        assert!(check_text("[color]").is_err());
        assert!(check_text("[color.dark]\nbg = \"#000000\"").is_err());
        assert!(check_text("[color]\ndark = 1\n[[contrast]]").is_err());
        assert!(check_text("not toml =").is_err());
    }

    #[test]
    fn the_repository_tokens_pass() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap_or_else(|| Path::new("."));
        let report = check(root).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        for theme in ["dark", "light"] {
            assert!(
                report.passed.iter().any(|line| line.starts_with(theme)),
                "{theme}: {:?}",
                report.passed
            );
        }
    }
}
