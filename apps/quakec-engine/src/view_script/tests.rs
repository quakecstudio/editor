// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::path::PathBuf;

use super::{Step, ViewScript};

// spec VIEWDIFF-1.1: the first line names the map; blank lines and comments are skipped
// spec VIEWDIFF-1.4: frames, place, fog and shot, in order
#[test]
fn a_script_reads_its_map_and_steps() {
    let script = ViewScript::parse(
        "# e1m1's start\nmap e1m1\n\nframes 72\nplace 480 -352 88 0 90\nfog 0\nshot a.ppm\n",
    )
    .unwrap();
    assert_eq!(script.map, "e1m1");
    assert_eq!(
        script.steps,
        [
            Step::Frames(72),
            Step::Place {
                origin: [480.0, -352.0, 88.0],
                angles: [0.0, 90.0],
            },
            Step::Fog(0.0),
            Step::Shot(PathBuf::from("a.ppm")),
        ]
    );
}

// spec VIEWDIFF-1.1: a script that does not start with `map <name>` is refused
// spec VIEWDIFF-1.4: so is any other line, a count that is not a number, a place without five
// finite numbers, a fog without one finite number and a shot without a file
#[test]
fn a_malformed_script_is_refused() {
    for text in [
        "",
        "frames 2",
        "map",
        "map a b",
        "map e1m1\nframes x",
        "map e1m1\nframes -1",
        "map e1m1\nplace 1 2 3 4",
        "map e1m1\nplace 1 2 3 4 5 6",
        "map e1m1\nplace 1 2 3 4 NaN",
        "map e1m1\nshot  ",
        "map e1m1\nfog",
        "map e1m1\nfog x",
        "map e1m1\nfog inf",
        "map e1m1\nfog 0 0.5 0.5 0.7",
        "map e1m1\ncmd god",
    ] {
        assert!(ViewScript::parse(text).is_err(), "{text:?}");
    }
}
