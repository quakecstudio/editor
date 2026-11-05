// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::path::PathBuf;

use super::{Args, engine_style};

fn parse(words: &[&str]) -> Result<Args, clap::Error> {
    let line = core::iter::once("preview-host")
        .chain(words.iter().copied())
        .map(str::to_owned);
    <Args as clap::Parser>::try_parse_from(engine_style(line))
}

// spec BRIDGE-1.1: the IDE launches the host with `-bridge <path>`; ADR-0008's `-basedir`,
// `-game` and `-kpf` are the engine's single-dash options
#[test]
fn the_engine_options_take_one_dash() {
    let args = parse(&[
        "-basedir",
        "/run/rerelease",
        "-game",
        "mymod",
        "-kpf",
        "/g/QuakeEX.kpf",
        "-bridge",
        "/tmp/b.sock",
        "e1m1",
    ])
    .unwrap();
    assert_eq!(args.basedir, Some(PathBuf::from("/run/rerelease")));
    assert_eq!(args.game.as_deref(), Some("mymod"));
    assert_eq!(args.kpf, Some(PathBuf::from("/g/QuakeEX.kpf")));
    assert_eq!(args.bridge, Some(PathBuf::from("/tmp/b.sock")));
    assert_eq!(args.map, "e1m1");
    assert_eq!(
        parse(&["--game", "mymod"]).unwrap().game.as_deref(),
        Some("mymod")
    );
    assert_eq!(parse(&[]).unwrap().map, "start");
}

// ADR-0008: `-game` is a single directory name, never a path
#[test]
fn a_game_that_is_a_path_is_refused() {
    for path in ["../other", "a/b", "C:\\games", "..", ".", ""] {
        assert!(parse(&["-game", path]).is_err(), "{path:?}");
    }
    assert!(parse(&["-unknown"]).is_err());
}
