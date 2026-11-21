// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::Path;

use super::*;

// No spec statement: L-47, a script is made from its number alone, the same each time, and
// each of its frames has the base's triangles; two numbers make two scripts
#[test]
fn a_script_is_made_from_its_number() {
    for seed in 0..50 {
        let grab = case(seed);
        assert_eq!(grab, case(seed), "{seed}");
        assert!(!grab.base.is_empty() && !grab.frames.is_empty() && !grab.skins.is_empty());
        for frame in frames(&grab) {
            assert_eq!(frame.triangles.len(), grab.base.len(), "{seed}");
        }
    }
    assert_ne!(case(1), case(2));
}

// No spec statement: L-47, modelgen's `$cd` is the script's directory after the Quake
// directory `SetQdirFromPath` finds, the path through the first `quake` and the byte after
// it; a path with no such directory, or one too long for a word of a script, is an error
#[test]
fn the_script_names_its_directory_as_modelgen_expands_it() {
    let grab = case(3);
    let text = script(&grab, Path::new("/work/quake/id1")).unwrap();
    assert!(text.starts_with("$modelname thing\n$cd id1\n"), "{text}");
    assert!(text.contains("$base base\n$skin skin0\n"), "{text}");
    // The first `quake` is the one found, whatever follows it.
    let text = script(&grab, Path::new("/src/quakecstudio/target/quake/id1")).unwrap();
    assert!(text.contains("\n$cd studio/target/quake/id1\n"), "{text}");
    let error = script(&grab, Path::new("/work/id1")).unwrap_err();
    assert!(
        error.contains("no short path under a quake directory"),
        "{error}"
    );
    let long = format!("/work/quake/{}", "x".repeat(MAX_TOKEN));
    assert!(script(&grab, Path::new(&long)).is_err());
}

// No spec statement: L-47, a triangle file holds its triangles' corners where
// `LoadTriangleList` reads them, big-endian, and ends in an empty group; a picture is a
// `PBM ` form of its pixels
#[test]
fn the_files_are_as_modelgen_reads_them() {
    let triangle: Corners = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]];
    let file = tri_file(&[triangle]);
    assert_eq!(file[..4], TRI_MAGIC.to_be_bytes());
    // The magic, the start, the object's name, its count and its texture's name.
    let first = 4 + 4 + 7 + 4 + 8;
    // A point's position follows its normal.
    assert_eq!(file[first + 12..first + 16], 1.0_f32.to_be_bytes());
    assert_eq!(
        file[first + 44 + 12..first + 44 + 16],
        4.0_f32.to_be_bytes()
    );
    assert!(file.ends_with(b"group\0"));
    let count_at = file.len() - 6 - 4 - 4;
    assert_eq!(file[count_at..count_at + 4], [0; 4], "the group's count");

    let pixels = vec![9_u8; 320 * 200];
    let picture = lbm(&pixels);
    assert_eq!(&picture[..4], b"FORM");
    assert_eq!(&picture[8..16], b"PBM BMHD");
    assert!(picture.ends_with(&pixels));
}
