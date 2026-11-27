// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::{Bsp2Shown, LimitFile, LimitRow, Reading, check, read};
use crate::engines::{Pin, load};

const C: &str = "\
#define MAX_MODELS\t\t4096 // a comment
#define MAX_SOUNDS 2048
/* if (count > 99) */
cvar_t max_edicts = {\"max_edicts\", \"8192\", CVAR_NONE};
static void Load (void)
{
\tif (count > 32767)
\t\tCon_DWarning (\"%i faces exceeds standard limit of 32767.\\n\", count);
\tif (count > 32767)
\t\tCon_DWarning (\"%i nodes exceeds standard limit of 32767.\\n\", count);
\tif (count > (1u<<16)) { return; }
\tif (ssize > 0x100) Host_Error(\"Bad surface extents\");
\tcase BSP2VERSION_BSP2: bsp2 = 2;
}
#ifdef Q1BSPS
#define lightstyleindex_t unsigned short
#endif
";

struct RowSpec<'a> {
    limit: &'a str,
    quote: &'a str,
    at: Option<&'a str>,
    reading: Reading,
}

impl Default for RowSpec<'_> {
    fn default() -> Self {
        Self {
            limit: "model_precaches",
            quote: "#define MAX_MODELS 4096",
            at: None,
            reading: Reading::Number,
        }
    }
}

fn file(rows: &[RowSpec<'_>], bsp2: Option<&str>) -> LimitFile {
    LimitFile {
        source: "quakespasm".to_owned(),
        engine: "quakespasm".to_owned(),
        path: "Quake/x.c".to_owned(),
        sha256: String::new(),
        bsp2: bsp2.map(str::to_owned),
        limits: rows
            .iter()
            .map(|r| LimitRow {
                limit: r.limit.to_owned(),
                quote: r.quote.to_owned(),
                at: r.at.map(str::to_owned),
                reading: r.reading,
                past: "refused".to_owned(),
                applies: None,
            })
            .collect(),
    }
}

fn most(row: RowSpec<'_>) -> Result<u64, String> {
    read(&file(&[row], None), C).map(|facts| facts.facts[0].most)
}

fn pins() -> Vec<Pin> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    load(&root).unwrap().0
}

// spec ENGT-5.2: a quote matches the file's code token by token, comments dropped and a
// directive's whitespace collapsed, and must occur exactly once
#[test]
fn a_quote_must_occur_once() {
    assert_eq!(most(RowSpec::default()), Ok(4096));
    // Twice: both warnings start alike.
    let twice = most(RowSpec {
        quote: "if (count > 32767)",
        ..RowSpec::default()
    })
    .unwrap_err();
    assert!(twice.contains("2 times"), "{twice}");
    // A comment is no code.
    let comment = most(RowSpec {
        quote: "if (count > 99)",
        ..RowSpec::default()
    })
    .unwrap_err();
    assert!(comment.contains("0 times"), "{comment}");
    // Context makes it once; `at` picks the number.
    assert_eq!(
        most(RowSpec {
            quote: "if (count > 32767) Con_DWarning (\"%i nodes exceeds standard limit of \
                    32767.\\n\", count);",
            at: Some("count > 32767"),
            ..RowSpec::default()
        }),
        Ok(32767)
    );
    // `at` must be part of the quote.
    let outside = most(RowSpec {
        at: Some("MAX_SOUNDS"),
        ..RowSpec::default()
    })
    .unwrap_err();
    assert!(outside.contains("not part"), "{outside}");
}

// spec ENGT-5.3: each reading takes its number: a string literal's, entries less one, luxels
// to texels, a shift and an unsigned short
#[test]
fn each_reading() {
    let quote = "cvar_t max_edicts = {\"max_edicts\", \"8192\"";
    assert_eq!(
        most(RowSpec {
            quote,
            at: Some("\"8192\""),
            ..RowSpec::default()
        }),
        Ok(8192)
    );
    assert_eq!(
        most(RowSpec {
            reading: Reading::Entries,
            ..RowSpec::default()
        }),
        Ok(4095)
    );
    assert_eq!(
        most(RowSpec {
            quote: "if (ssize > 0x100)",
            reading: Reading::Luxels,
            ..RowSpec::default()
        }),
        Ok(4080)
    );
    assert_eq!(
        most(RowSpec {
            quote: "if (count > (1u<<16)) {",
            reading: Reading::Shift,
            ..RowSpec::default()
        }),
        Ok(65536)
    );
    assert_eq!(
        most(RowSpec {
            quote: "#ifdef Q1BSPS\n#define lightstyleindex_t unsigned short",
            reading: Reading::UnsignedShort,
            ..RowSpec::default()
        }),
        Ok(65536)
    );
    // No number to read.
    let none = most(RowSpec {
        quote: "case BSP2VERSION_BSP2:",
        ..RowSpec::default()
    })
    .unwrap_err();
    assert!(none.contains("no number"), "{none}");
}

// spec ENGT-5.6: a bsp2 quote found shows the engine reads BSP2; one not found is an error
#[test]
fn a_bsp2_quote() {
    let shown = read(&file(&[], Some("case BSP2VERSION_BSP2: bsp2 = 2;")), C).unwrap();
    assert_eq!(shown.bsp2, Bsp2Shown::Shown);
    assert!(read(&file(&[], Some("Mod_BSP2_Load(")), C).is_err());
}

// spec ENGT-5.1: a limit file names a builtin source, one of the five engines, and limits and
// pasts ENGT-5.4 and ENGT-5.5 know
#[test]
fn bad_limit_files_are_errors() {
    let pins = pins();
    assert!(check(&file(&[RowSpec::default()], None), &pins).is_ok());
    let mut other = file(&[RowSpec::default()], None);
    other.source = "nowhere".to_owned();
    assert!(check(&other, &pins).unwrap_err().contains("no source"));
    let mut kex = file(&[RowSpec::default()], None);
    kex.engine = "kex".to_owned();
    assert!(check(&kex, &pins).unwrap_err().contains("engine"));
    let unknown = file(
        &[RowSpec {
            limit: "brushes",
            ..RowSpec::default()
        }],
        None,
    );
    assert!(check(&unknown, &pins).unwrap_err().contains("limit"));
    let mut past = file(&[RowSpec::default()], None);
    past.limits[0].past = "crashes".to_owned();
    assert!(check(&past, &pins).unwrap_err().contains("past"));
    let mut applies = file(&[RowSpec::default()], None);
    applies.limits[0].applies = Some("bsp2".to_owned());
    assert!(check(&applies, &pins).unwrap_err().contains("applies"));
    assert!(
        check(&file(&[], None), &pins)
            .unwrap_err()
            .contains("no limit")
    );
}

// spec ENGT-5.1: the pinned limit files are each of a builtin source
#[test]
fn the_pinned_limit_files() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let (pins, files) = load(&root).unwrap();
    assert!(files.len() >= 14);
    for f in &files {
        check(f, &pins).unwrap();
    }
}
