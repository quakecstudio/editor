// SPDX-License-Identifier: GPL-3.0-or-later

use super::{Words, atoi, tokenize};

fn argv(text: &str) -> Vec<String> {
    tokenize(text.as_bytes())
        .argv
        .into_iter()
        .map(|w| String::from_utf8(w).unwrap_or_default())
        .collect()
}

// spec PHYS-11.1: a client command is its first line's words as COM_Parse reads them; a ';'
// separates nothing, a newline ends the command, and the text after the first word is kept
#[test]
fn a_command_is_tokenized_as_cmd_tokenizestring_does() {
    assert_eq!(argv("noclip;god"), ["noclip;god"]);
    assert_eq!(argv("  give  h 100\nkill"), ["give", "h", "100"]);
    assert_eq!(argv("name \"a b\"\n"), ["name", "a b"]);
    assert_eq!(argv(""), Vec::<String>::new());
    assert_eq!(argv("\ngod"), Vec::<String>::new());
    let words = tokenize(b"name bob the great\n");
    assert_eq!(
        words,
        Words {
            argv: ["name", "bob", "the", "great"]
                .map(|w| w.as_bytes().to_vec())
                .to_vec(),
            args: b"bob the great\n".to_vec(),
        }
    );
}

// spec PHYS-11.6: give's number is C's atoi: spaces, a sign, digits, then anything
#[test]
fn atoi_reads_as_c_does() {
    assert_eq!(atoi(b"100"), 100);
    assert_eq!(atoi(b"  -42x"), -42);
    assert_eq!(atoi(b"+7"), 7);
    assert_eq!(atoi(b"x9"), 0);
    assert_eq!(atoi(b""), 0);
}
