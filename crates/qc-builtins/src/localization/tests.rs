// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of the rerelease's localization (RRE-1), each against QuakeSpasm's `common.c` and
//! `pr_cmds.c` at the pinned commit.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::{Localization, com_hash_string, hash_slot};

fn value(loc: &Localization, key: &str) -> Option<Vec<u8>> {
    loc.loc_get_raw_string(key.as_bytes()).map(<[u8]>::to_vec)
}

fn strings(parms: &[&str]) -> Vec<Vec<u8>> {
    parms.iter().map(|p| p.as_bytes().to_vec()).collect()
}

// spec RRE-1.7: a key is the text before the first `=`, its trailing white space trimmed; the
// value is after it, leading white space skipped, an opening quote dropped and the value ended
// at the next unescaped quote; leading blanks of a line are skipped; a byte order mark is
// skipped; comments and lines with no `=` hold nothing
#[test]
fn entries_are_read_as_loc_loadfile_reads_them() {
    let loc = Localization::loc_load_file(
        b"\xEF\xBB\xBF// a comment\n\
          \t  qc_hello = \"Hello, world\" trailing\n\
          qc_bare=bare value\n\
          qc_eq = a=b\n\
          no equals here\n\
          / lone slash = not an entry\n\
          qc_quote = \"say \\\"hi\\\" \\t\\n\"\n",
    );
    assert_eq!(loc.len(), 4);
    assert_eq!(value(&loc, "$qc_hello").unwrap(), b"Hello, world");
    assert_eq!(value(&loc, "$qc_bare").unwrap(), b"bare value");
    assert_eq!(value(&loc, "$qc_eq").unwrap(), b"a=b");
    assert_eq!(value(&loc, "$qc_quote").unwrap(), b"say \"hi\" \t\n");
    assert_eq!(value(&loc, "$no equals here"), None);
}

// spec RRE-1.7: the escapes `\n`, `\t`, `\v`, `\b` and `\f` are control characters, `\"` and
// `\'` the quotes, any other `\c` is `c`; a `\` that ends the line stays
#[test]
fn escapes_are_unescaped_in_place() {
    let loc = Localization::loc_load_file(b"k = \"\\v\\b\\f\\'\\q\"\nk2 = \"end\\\n");
    assert_eq!(value(&loc, "$k").unwrap(), b"\x0b\x08\x0c'q");
    // `\` before the line's end is copied, and the value has no closing quote.
    assert_eq!(value(&loc, "$k2").unwrap(), b"end\\");
}

// spec RRE-1.7: the unquoted value's trim zeroes the byte after each trailing blank before it
// steps back, so one trailing blank stays, and its first zero is the line's own end, so the
// file is read no further
#[test]
fn trailing_blanks_end_the_file() {
    let loc = Localization::loc_load_file(b"first = 1\na = ab  \nnext = n\n");
    assert_eq!(loc.len(), 2);
    assert_eq!(value(&loc, "$a").unwrap(), b"ab ");
    assert_eq!(value(&loc, "$next"), None);
    let loc = Localization::loc_load_file(b"d = ab \t \n");
    assert_eq!(value(&loc, "$d").unwrap(), b"ab ");
    // A quoted value is not trimmed, so the file goes on.
    let loc = Localization::loc_load_file(b"q = \"ab\"  \nnext = n\n");
    assert_eq!(value(&loc, "$next").unwrap(), b"n");
}

// spec RRE-1.7: an unquoted value an escape shortened keeps the source's last bytes, as only
// the line's end terminates it; a quoted one is terminated at its quote
#[test]
fn a_shortened_unquoted_value_keeps_its_last_bytes() {
    let loc = Localization::loc_load_file(b"b = x\\ny\nc = \"x\\ny\"\n");
    assert_eq!(value(&loc, "$b").unwrap(), b"x\nyy");
    assert_eq!(value(&loc, "$c").unwrap(), b"x\ny");
}

// spec RRE-1.7: a CR before the line feed is part of an unquoted value, not trimmed, and is
// white space after a key
#[test]
fn a_carriage_return_stays_in_an_unquoted_value() {
    let loc = Localization::loc_load_file(b"k = v\r\nq = \"v\"\r\n");
    assert_eq!(value(&loc, "$k").unwrap(), b"v\r");
    assert_eq!(value(&loc, "$q").unwrap(), b"v");
}

// spec RRE-1.2: a key listed twice keeps the first entry, which probing finds first
#[test]
fn the_first_of_a_repeated_key_wins() {
    let loc = Localization::loc_load_file(b"k = first\nk = second\nother = o\n");
    assert_eq!(loc.len(), 3);
    assert_eq!(value(&loc, "$k").unwrap(), b"first");
    assert_eq!(value(&loc, "$other").unwrap(), b"o");
}

// spec RRE-1.2: many keys, each found again through the open-addressed table
#[test]
fn every_key_is_found_through_the_table() {
    let text: String = (0..300).map(|i| format!("key{i} = v{i}\n")).collect();
    let loc = Localization::loc_load_file(text.as_bytes());
    for i in 0..300 {
        assert_eq!(
            value(&loc, &format!("$key{i}")).unwrap(),
            format!("v{i}").as_bytes()
        );
    }
    assert_eq!(value(&loc, "$key300"), None);
    assert!(!loc.is_empty());
    assert_eq!(loc.len(), 300);
}

// spec RRE-1.2: a key's probe starts at its hash modulo the table's size
#[test]
fn a_probe_starts_at_the_hash_modulo_the_size() {
    assert_eq!(hash_slot(10, 4), 2);
    assert_eq!(hash_slot(7, 4), 3);
    assert_eq!(hash_slot(0xFFFF_FFFF, 600), 0xFFFF_FFFF % 600);
    assert_eq!(hash_slot(5, 0), 0);
}

// spec RRE-1.8: COM_HashString is FNV-1a over signed chars, a byte from 0x80 up sign-extended
#[test]
fn the_hash_is_fnv1a_of_signed_chars() {
    assert_eq!(com_hash_string(b""), 0x811c_9dc5);
    assert_eq!(
        com_hash_string(b"a"),
        (0x811c_9dc5_u32 ^ 0x61).wrapping_mul(0x0100_0193)
    );
    assert_eq!(
        com_hash_string(b"\xe9"),
        (0x811c_9dc5_u32 ^ 0xFFFF_FFE9).wrapping_mul(0x0100_0193)
    );
    let loc = Localization::loc_load_file(b"caf\xe9 = coffee\n");
    assert_eq!(value(&loc, "$caf\u{e9}"), None, "a different spelling");
    assert_eq!(loc.loc_get_raw_string(b"$caf\xe9").unwrap(), b"coffee");
}

// spec RRE-1.3: only a key that starts with `$` is looked up, the `$` dropped; any other text,
// and every text with no file loaded, is itself
#[test]
fn only_a_dollar_key_is_localized() {
    let loc = Localization::loc_load_file(b"qc_got = You got it\n");
    assert_eq!(loc.loc_get_string(b"$qc_got"), b"You got it");
    assert_eq!(loc.loc_get_string(b"qc_got"), b"qc_got");
    assert_eq!(loc.loc_get_string(b"$qc_missing"), b"$qc_missing");
    assert_eq!(loc.loc_get_raw_string(b"$"), None);
    assert_eq!(loc.loc_get_raw_string(b""), None);
    let none = Localization::loc_load_file(b"// nothing\n");
    assert!(none.is_empty());
    assert!(none.is_empty());
    assert_eq!(none.loc_get_string(b"$qc_got"), b"$qc_got");
    assert_eq!(Localization::default().loc_get_raw_string(b"$qc_got"), None);
}

// spec RRE-1.4: `{}` and `{N}` are placeholders, with a file loaded; a brace that is no
// placeholder is text
#[test]
fn placeholders_are_found_with_a_file_loaded() {
    let loc = Localization::loc_load_file(b"k = v\n");
    assert!(loc.loc_has_placeholders(b"x {} y"));
    assert!(loc.loc_has_placeholders(b"{12}"));
    assert!(!loc.loc_has_placeholders(b"{x} { } {1"));
    assert!(!loc.loc_has_placeholders(b""));
    assert!(!Localization::default().loc_has_placeholders(b"{}"));
    // A number that wraps negative passes over the placeholder and the byte after it.
    assert!(!loc.loc_has_placeholders(b"{4294967295}{}"));
    assert!(loc.loc_has_placeholders(b"{4294967295}x{}"));
}

// spec RRE-1.9: with placeholders the first string is LOC_Format of its localization, each
// `{N}` the localized parameter `first + 1 + N` and empty past the last; `{}` is `{0}`
#[test]
fn a_format_takes_its_arguments_by_number() {
    let loc = Localization::loc_load_file(b"qc_kill = {0} was killed by {1}{}\nqc_axe = the Axe\n");
    let parms = strings(&["$qc_kill", "Ranger", "$qc_axe"]);
    assert_eq!(
        loc.pf_var_string(&parms),
        b"Ranger was killed by the AxeRanger"
    );
    let parms = strings(&["skipped", "$qc_kill", "A"]);
    assert_eq!(loc.pf_var_string(&parms[1..]), b"A was killed by A");
    let parms = strings(&["$qc_kill"]);
    assert_eq!(loc.pf_var_string(&parms), b" was killed by ");
    // A placeholder whose number wraps negative is dropped with the byte after it copied.
    let loc = Localization::loc_load_file(b"k = <{4294967295}>{}\n");
    assert_eq!(loc.pf_var_string(&strings(&["$k", "x"])), b"<>x");
    // The byte copied after it is the next placeholder's `{`, so that one is text.
    let loc = Localization::loc_load_file(b"k = {}<{4294967295}{}>\n");
    assert_eq!(loc.pf_var_string(&strings(&["$k", "x"])), b"x<{}>");
}

// spec RRE-1.9: with no placeholder every parameter from `first` is localized and appended;
// none from past the last parameter
#[test]
fn without_placeholders_the_strings_are_joined() {
    let loc = Localization::loc_load_file(b"qc_a = Alpha\n");
    let parms = strings(&["$qc_a", " and ", "$qc_b"]);
    assert_eq!(loc.pf_var_string(&parms), b"Alpha and $qc_b");
    assert!(loc.pf_var_string(&parms[3..]).is_empty());
}

// spec RRE-1.9: the text is cut to 1023 bytes, in either path; an argument that does not fit
// is cut, and the format's rest dropped
#[test]
fn the_text_is_cut_to_1023_bytes() {
    let long = "x".repeat(1500);
    let loc = Localization::loc_load_file(b"k = [{}]{}\n");
    let joined = loc.pf_var_string(&strings(&[&long, "tail"]));
    assert_eq!(joined.len(), 1023);
    let formatted = loc.pf_var_string(&strings(&["$k", &long]));
    assert_eq!(formatted.len(), 1023);
    assert!(formatted.starts_with(b"[x"));
    let short = loc.pf_var_string(&strings(&["$k", "ab"]));
    assert_eq!(short, b"[ab]ab");
}

// spec RRE-1.2: a key whose probe starts at the table's last slot wraps to slot 0, and both
// keys that start there are found again (P2-R1B review)
#[test]
fn reviewer_a_probe_wraps_past_the_last_slot() {
    let last: Vec<String> = (0..10_000)
        .map(|i| format!("k{i}"))
        .filter(|key| hash_slot(com_hash_string(key.as_bytes()), 4) == 3)
        .take(2)
        .collect();
    assert_eq!(last.len(), 2);
    let file = format!("{} = first\n{} = second\n", last[0], last[1]);
    let loc = Localization::loc_load_file(file.as_bytes());
    assert_eq!(loc.len(), 2);
    assert_eq!(value(&loc, &format!("${}", last[0])).unwrap(), b"first");
    assert_eq!(value(&loc, &format!("${}", last[1])).unwrap(), b"second");
    // A missing key that starts at the last slot probes 3, then 0, then stops at an empty slot.
    let missing = (0..10_000)
        .map(|i| format!("m{i}"))
        .find(|key| hash_slot(com_hash_string(key.as_bytes()), 4) == 3)
        .unwrap();
    assert_eq!(value(&loc, &format!("${missing}")), None);
}

// spec RRE-1.3: `$` alone looks up the empty key, which a line starting with `=` defines; a
// second `$` is part of the key (P2-R1B review)
#[test]
fn reviewer_the_empty_key_and_a_doubled_dollar() {
    let loc = Localization::loc_load_file(b"= empty\n$k = dollar\nk = plain\n");
    assert_eq!(value(&loc, "$").unwrap(), b"empty");
    assert_eq!(value(&loc, "$$k").unwrap(), b"dollar");
    assert_eq!(value(&loc, "$k").unwrap(), b"plain");
    assert_eq!(value(&loc, ""), None);
    assert_eq!(Localization::default().loc_get_string(b"$k"), b"$k");
}

// spec RRE-1.7: the file's text ends at its first zero, so an entry after it is not read; an
// `=` on a comment line makes no entry; a value that ends the line or the file is empty
// (P2-R1B review)
#[test]
fn reviewer_the_text_ends_at_its_first_zero() {
    let loc = Localization::loc_load_file(b"a = 1\n/b = 2\nc =\nd = \"\0e = 3\n");
    assert_eq!(value(&loc, "$a").unwrap(), b"1");
    assert_eq!(value(&loc, "$b"), None);
    assert_eq!(value(&loc, "$/b"), None);
    assert_eq!(value(&loc, "$c").unwrap(), b"");
    assert_eq!(value(&loc, "$d").unwrap(), b"");
    assert_eq!(value(&loc, "$e"), None);
    assert_eq!(loc.len(), 3);
}
