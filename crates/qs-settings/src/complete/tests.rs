// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use super::{SettingsCompletion, header_name};
use crate::{Setting, SettingsFile};

/// The labels offered at the end of `text`.
fn labels(text: &str) -> Vec<String> {
    SettingsFile::completions(text, text.len())
        .into_iter()
        .map(|completion| completion.label)
        .collect()
}

/// The completion labelled `label` at the end of `text`.
fn offered(text: &str, label: &str) -> Option<SettingsCompletion> {
    SettingsFile::completions(text, text.len())
        .into_iter()
        .find(|completion| completion.label == label)
}

// No spec statement: UI-24, QA-10: before any table a line offers the tables as headers, and
// in a header their names, each with what it holds
#[test]
fn tables_are_offered_before_any_and_in_a_header() {
    let headers = labels("");
    for header in [
        "[appearance]",
        "[editor]",
        "[game]",
        "[agent]",
        "[updates]",
        "[log]",
    ] {
        assert!(
            headers.iter().any(|h| h == header),
            "{header} in {headers:?}"
        );
    }
    let names = labels("[appearance]\ntheme = \"dark\"\n[");
    assert!(names.iter().any(|name| name == "editor"), "{names:?}");
    assert!(!offered("[", "game").unwrap().detail.is_empty());
}

// No spec statement: UI-24, QA-10: in a table a line offers its keys, each with its setting's
// summary; the table is the last header above, its first part for a dotted one
#[test]
fn a_tables_keys_are_offered_with_their_summary() {
    assert_eq!(labels("[updates]\n"), ["check", "skip_version"]);
    let check = offered("[updates]\n", "check").unwrap();
    assert_eq!(check.detail, Setting::UpdateCheck.summary());
    assert_eq!(
        labels("[updates]\ncheck = \"off\"\n[editor]\n  "),
        ["key_mode"]
    );
    let agent = labels("[agent.tool_permissions]\ndefault = \"ask\"\n[agent]\n");
    assert!(agent.iter().any(|key| key == "profile"), "{agent:?}");
    // A dotted header's keys are its own table's, and an array of tables' those of one table.
    let rules = labels("[agent.tool_permissions]\n");
    assert!(rules.iter().any(|key| key == "default"), "{rules:?}");
    assert!(!rules.iter().any(|key| key == "profile"), "{rules:?}");
    let external = labels("[[agent.external]]\n");
    assert!(external.iter().any(|key| key == "command"), "{external:?}");
    assert!(labels("[agent.nothing]\n").is_empty());
    assert!(labels("[nothing]\n").is_empty());
}

// No spec statement: UI-24, QA-10: after `key =` a key whose type is a set of choices offers
// them, each with what it does, quoted as TOML writes a string unless a quote is open; a key
// of any other type, or one the table does not have, offers nothing
#[test]
fn a_keys_values_are_offered_when_they_are_a_set() {
    assert_eq!(labels("[appearance]\ntheme = "), ["\"dark\"", "\"light\""]);
    assert_eq!(labels("[appearance]\ntheme = \""), ["dark", "light"]);
    let daily = offered("[updates]\ncheck = ", "\"daily\"").unwrap();
    assert!(daily.detail.contains("once a day"), "{daily:?}");
    assert_eq!(labels("[editor]\nkey_mode="), ["\"standard\"", "\"vim\""]);
    assert_eq!(
        labels("[agent.tool_permissions]\ndefault = "),
        ["\"allow\"", "\"ask\"", "\"deny\""]
    );
    assert!(labels("[log]\nfilter = ").is_empty());
    assert!(labels("[log]\nnothing = ").is_empty());
    assert!(labels("theme = ").is_empty());
}

// No spec statement: UI-24: the place is where the offset is, not the text's end, and an
// offset past the end is the end
#[test]
fn the_place_is_the_offsets() {
    let text = "[appearance]\ntheme = \n[updates]\n";
    let at = text.find("= ").unwrap() + 2;
    let labels: Vec<String> = SettingsFile::completions(text, at)
        .into_iter()
        .map(|completion| completion.label)
        .collect();
    assert_eq!(labels, ["\"dark\"", "\"light\""]);
    assert_eq!(
        SettingsFile::completions(text, text.len() + 10).len(),
        SettingsFile::completions(text, text.len()).len()
    );
}

#[test]
fn a_headers_table_is_its_dotted_path() {
    assert_eq!(header_name("[agent]"), Some("agent"));
    assert_eq!(
        header_name("  [[agent.external]]  "),
        Some("agent.external")
    );
    assert_eq!(header_name("[ game ]"), Some("game"));
    assert_eq!(header_name("theme = \"dark\""), None);
}
