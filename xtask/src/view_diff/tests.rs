// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code: a failed unwrap, index or match is a failed test; the test frames' offsets are small"
)]

use super::{
    PPM_HEADER, Sound, Tolerance, compare_frames, compare_sound, monster_near, parse_tolerances,
    route, start_of, tolerance_of,
};

/// A 320 by 240 binary PPM of one grey `level`, its pixel `at` set to `other`.
fn frame(level: u8, at: Option<(usize, u8)>) -> Vec<u8> {
    let mut bytes = PPM_HEADER.to_vec();
    let mut pixels = vec![level; 320 * 240 * 3];
    if let Some((at, other)) = at {
        pixels[at * 3..at * 3 + 3].fill(other);
    }
    bytes.extend(pixels);
    bytes
}

/// A quarter of a second of 16-bit stereo at 11025 Hz for each of `windows`, its samples that
/// window's level.
fn sound(windows: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for &level in windows {
        for _ in 0..2756 * 2 {
            bytes.extend(level.to_le_bytes());
        }
    }
    bytes
}

// spec VIEWDIFF-3.2: the same frame has no difference and a brightness of 1
#[test]
fn the_same_frame_matches() {
    let frames = compare_frames(&frame(100, None), &frame(100, None)).unwrap();
    assert_eq!(frames.difference, 0.0);
    assert_eq!(frames.brightness, 1.0);
}

// spec VIEWDIFF-3.1: one pixel counts a sixteenth in its block
// spec VIEWDIFF-3.2: the difference is the mean over the 4800 blocks
#[test]
fn one_pixel_counts_a_sixteenth_of_its_block() {
    let frames = compare_frames(&frame(100, None), &frame(100, Some((0, 116)))).unwrap();
    assert!(
        (frames.difference - 1.0 / 4800.0).abs() < 1e-9,
        "{frames:?}"
    );
}

// spec VIEWDIFF-3.3: a frame twice as bright fails on brightness, as GLQuake's look did; a
// dark oracle frame and a lit one of ours is infinitely bright, and two dark ones match
#[test]
fn a_lighting_fault_fails() {
    let tolerance = Tolerance {
        frame: 6.0,
        brightness: 0.15,
        sound: 0.35,
    };
    let doubled = compare_frames(&frame(40, None), &frame(80, None)).unwrap();
    assert_eq!(doubled.brightness, 2.0);
    assert!(!doubled.fits(tolerance));
    let near = compare_frames(&frame(40, None), &frame(42, None)).unwrap();
    assert!(near.fits(tolerance), "{near:?}");
    assert_eq!(
        compare_frames(&frame(0, None), &frame(1, None))
            .unwrap()
            .brightness,
        f64::INFINITY
    );
    assert_eq!(
        compare_frames(&frame(0, None), &frame(0, None))
            .unwrap()
            .brightness,
        1.0
    );
}

// spec VIEWDIFF-3.3: a file that is not a 320 by 240 binary PPM fails
#[test]
fn a_malformed_frame_fails() {
    let good = frame(1, None);
    assert!(compare_frames(b"P3\n320 240\n255\n", &good).is_err());
    assert!(compare_frames(&good, &good[..good.len() - 1]).is_err());
    let mut small = b"P6\n160 120\n255\n".to_vec();
    small.extend(vec![0; 160 * 120 * 3]);
    assert!(compare_frames(&good, &small).is_err());
}

// spec VIEWDIFF-4.2: two silent recordings pass, one silent fails, a short one fails
#[test]
fn silence_is_compared() {
    assert_eq!(
        compare_sound(&sound(&[0; 14]), &sound(&[0; 14])),
        Sound::Silent
    );
    assert_eq!(
        compare_sound(&sound(&[0; 14]), &sound(&[9; 14])),
        Sound::OneSilent
    );
    assert_eq!(
        compare_sound(&sound(&[9; 14]), &sound(&[9; 2])),
        Sound::Short
    );
    assert_eq!(compare_sound(&[], &sound(&[9; 14])), Sound::Short);
}

// spec VIEWDIFF-4.1: only the last 12 windows count
// spec VIEWDIFF-4.3: the difference is relative to the oracle's level, at the best shift of up
// to two windows
#[test]
fn sound_is_compared_over_the_last_windows_at_the_best_shift() {
    let mut early = vec![0_i16; 14];
    early[0] = 900;
    let mut later = vec![100_i16; 14];
    later[1] = 5000;
    let Sound::Difference(same) = compare_sound(&sound(&later), &sound(&later)) else {
        panic!("not compared");
    };
    assert_eq!(same, 0.0);
    // A loud first window, before the last 12, is not heard.
    assert_eq!(
        compare_sound(&sound(&early), &sound(&[0; 14])),
        Sound::Silent
    );
    let pattern: Vec<i16> = (0..14).map(|n| if n % 4 == 0 { 1000 } else { 0 }).collect();
    let mut shifted = vec![0_i16];
    shifted.extend(&pattern[..13]);
    let Sound::Difference(by_one) = compare_sound(&sound(&pattern), &sound(&shifted)) else {
        panic!("not compared");
    };
    assert!(by_one < 0.2, "{by_one}");
    let Sound::Difference(twice) = compare_sound(&sound(&[100; 14]), &sound(&[200; 14])) else {
        panic!("not compared");
    };
    assert!((twice - 1.0).abs() < 1e-9, "{twice}");
}

// spec VIEWDIFF-5.2: a map's row, else the `*` row; a table without either is refused
#[test]
fn tolerances_are_read_per_map() {
    let doc = "<!-- view-diff -->\n| Map | Frame | Brightness | Sound | Why |\n| --- | --- | --- | --- | --- |\n\
               | `*` | 6 | 0.15 | 0.35 | |\n| `e4m7` | 12 | 0.15 | 0.5 | a sky |\n<!-- /view-diff -->\n";
    let rows = parse_tolerances(doc).unwrap();
    assert_eq!(tolerance_of(&rows, "e4m7").unwrap().frame, 12.0);
    assert_eq!(tolerance_of(&rows, "e4m7").unwrap().sound, 0.5);
    assert_eq!(tolerance_of(&rows, "e1m1").unwrap().sound, 0.35);
    assert!(tolerance_of(&rows[1..], "e1m1").is_err());
    assert!(parse_tolerances("no table").is_err());
    assert!(parse_tolerances(&doc.replace("| 0.35 |", "| loud |")).is_err());
    assert!(parse_tolerances(&doc.replace(" a sky |", "")).is_err());
}

// spec VIEWDIFF-5.1: a route settles, then shoots the start's view and the one behind it, then
// holds for the sound
#[test]
fn a_route_shoots_the_start_both_ways() {
    let text = route("e1m1", ([480.0, -352.0, 88.0], 90.0), "id");
    assert_eq!(
        text,
        "map e1m1\nframes 216\nplace 480 -352 88 0 90\nframes 18\nshot id-ahead.ppm\n\
         place 480 -352 88 0 270\nframes 18\nshot id-behind.ppm\nframes 216\n"
    );
    let entities = "{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"info_player_start\"\n\
                    \"origin\" \"480 -352 88\"\n\"angle\" \"90\"\n}\n";
    assert_eq!(start_of(entities), Some(([480.0, -352.0, 88.0], 90.0)));
    assert_eq!(
        start_of("{\n\"classname\" \"info_player_start\"\n\"origin\" \"1 2 3\"\n}\n"),
        Some(([1.0, 2.0, 3.0], 0.0))
    );
    assert_eq!(start_of("{\n\"classname\" \"worldspawn\"\n}\n"), None);
}

/// The view oracle's harness.
fn harness() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    std::fs::read_to_string(root.join("oracles/engine/view_host.c")).unwrap()
}

// spec VIEWDIFF-1.3: the oracle copies out what the card played, by the clock, at 11025 Hz in
// 16-bit stereo
// spec VIEWDIFF-2.1: at 320 by 240 with square pixels
#[test]
fn the_oracle_plays_and_draws_as_the_host_does() {
    let c = harness();
    for line in [
        "shm->samplebits = 16;",
        "shm->speed = 11025;",
        "shm->channels = 2;",
        "long until = (long)(qs_time * 11025.0);",
        "#define QS_WIDTH 320",
        "#define QS_HEIGHT 240",
        "vid.aspect = 1.0;",
    ] {
        assert!(c.contains(line), "{line}");
    }
}

// spec VIEWDIFF-2.2: no status bar, crosshair, weapon or notices
// spec VIEWDIFF-2.4: drawing only from a place to its shot, the console up
#[test]
fn the_oracle_draws_only_the_view_from_a_place_to_its_shot() {
    let c = harness();
    let place = c.find("place(v);").unwrap();
    for setting in [
        r#"Cvar_Set("viewsize", "120");"#,
        r#"Cvar_Set("crosshair", "0");"#,
        r#"Cvar_Set("r_drawviewmodel", "0");"#,
        r#"Cvar_Set("con_notifytime", "0");"#,
    ] {
        assert!(c[place..].contains(setting), "{setting}");
    }
    let shot = c.find("write_shot(text + 5)").unwrap();
    let drawing = c[place..].find("block_drawing = false;").unwrap() + place;
    let blocked = c[shot..].find("block_drawing = true;").unwrap() + shot;
    assert!(drawing < shot && blocked > shot);
    assert!(c[place..drawing].contains("scr_con_current = 0;"));
}

// spec VIEWDIFF-2.5: no demos at start
#[test]
fn the_oracle_plays_no_demos() {
    assert!(harness().contains("cls.demonum = -1;"));
}

// spec VIEWDIFF-2.3: every id file of the view oracle calls rand through the shared generator
#[test]
fn the_oracle_shares_the_generator() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let source = std::fs::read_to_string(root.join("xtask/src/oracles.rs")).unwrap();
    let view = &source[source.find("fn compile_view(").unwrap()..];
    assert!(view.contains(r#"let rand = root.join("oracles/engine/rand.h");"#));
    assert!(view.contains("Code::Upstream | Code::UpstreamCommon => Some(rand.as_path()),"));
    assert!(source.contains(r#"const VIEW_HARNESS: [&str; 2] = ["oracles/engine/view_host.c", "oracles/engine/rand.c"];"#));
}

// spec VIEWDIFF-5.3: a monster within 1000 units of the start is in earshot; one at 1000 or
// farther, or any other entity, is not
#[test]
fn a_monster_in_earshot_is_found() {
    let text = |class: &str, origin: &str| {
        format!("{{\n\"classname\" \"{class}\"\n\"origin\" \"{origin}\"\n}}\n")
    };
    let start = [0.0, 0.0, 0.0];
    assert!(monster_near(&text("monster_ogre", "300 400 0"), start));
    assert!(!monster_near(&text("monster_ogre", "600 800 0"), start));
    assert!(!monster_near(&text("item_health", "10 0 0"), start));
    assert!(!monster_near(&text("monster_ogre", "nowhere"), start));
}

// spec VIEWDIFF-2.6: the view oracle is not built on Windows
#[test]
fn the_oracle_is_not_built_on_windows() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let source = std::fs::read_to_string(root.join("xtask/src/oracles.rs")).unwrap();
    assert!(source.contains("if !cfg!(windows) {\n        let view_objects = compile_view("));
}
