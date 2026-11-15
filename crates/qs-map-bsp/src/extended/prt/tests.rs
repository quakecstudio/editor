// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use super::fmt_number;
use crate::{Compiler, Dialect};

// spec QBSP-15.52: numbers are written as `{fmt}` writes them, the shortest digits that read
// back, with a two-digit exponent below 1e-4 or from 1e16
#[test]
fn numbers_as_fmt_writes_them() {
    assert_eq!(fmt_number(16.0), "16");
    assert_eq!(fmt_number(-0.5), "-0.5");
    assert_eq!(fmt_number(0.0), "0");
    assert_eq!(fmt_number(0.0001), "0.0001");
    assert_eq!(fmt_number(0.00001), "1e-05");
    assert_eq!(
        fmt_number(-3.552_713_678_800_501e-15),
        "-3.552713678800501e-15"
    );
    assert_eq!(fmt_number(1e16), "1e+16");
    assert_eq!(fmt_number(123_456.75), "123456.75");
}

/// The vis tests' snake: a corridor of cells round three corners, its last cell's floor
/// `*water`, which the extended dialect makes detail.
const SNAKE: &str = r#"{
"classname" "worldspawn"
{
( -16 0 0 ) ( -16 1 0 ) ( -16 0 1 ) wall 0 0 0 1 1
( 400 0 0 ) ( 400 0 1 ) ( 400 1 0 ) wall 0 0 0 1 1
( 0 -16 0 ) ( 0 -16 1 ) ( 1 -16 0 ) wall 0 0 0 1 1
( 0 912 0 ) ( 1 912 0 ) ( 0 912 1 ) wall 0 0 0 1 1
( 0 0 -16 ) ( 1 0 -16 ) ( 0 1 -16 ) wall 0 0 0 1 1
( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) wall 0 0 0 1 1
}
{
( -16 0 0 ) ( -16 1 0 ) ( -16 0 1 ) wall 0 0 0 1 1
( 400 0 0 ) ( 400 0 1 ) ( 400 1 0 ) wall 0 0 0 1 1
( 0 -16 0 ) ( 0 -16 1 ) ( 1 -16 0 ) wall 0 0 0 1 1
( 0 912 0 ) ( 1 912 0 ) ( 0 912 1 ) wall 0 0 0 1 1
( 0 0 128 ) ( 1 0 128 ) ( 0 1 128 ) wall 0 0 0 1 1
( 0 0 144 ) ( 0 1 144 ) ( 1 0 144 ) wall 0 0 0 1 1
}
{
( -16 0 0 ) ( -16 1 0 ) ( -16 0 1 ) wall 0 0 0 1 1
( 0 0 0 ) ( 0 0 1 ) ( 0 1 0 ) wall 0 0 0 1 1
( 0 -16 0 ) ( 0 -16 1 ) ( 1 -16 0 ) wall 0 0 0 1 1
( 0 912 0 ) ( 1 912 0 ) ( 0 912 1 ) wall 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) wall 0 0 0 1 1
( 0 0 128 ) ( 0 1 128 ) ( 1 0 128 ) wall 0 0 0 1 1
}
{
( 384 0 0 ) ( 384 1 0 ) ( 384 0 1 ) wall 0 0 0 1 1
( 400 0 0 ) ( 400 0 1 ) ( 400 1 0 ) wall 0 0 0 1 1
( 0 -16 0 ) ( 0 -16 1 ) ( 1 -16 0 ) wall 0 0 0 1 1
( 0 912 0 ) ( 1 912 0 ) ( 0 912 1 ) wall 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) wall 0 0 0 1 1
( 0 0 128 ) ( 0 1 128 ) ( 1 0 128 ) wall 0 0 0 1 1
}
{
( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) wall 0 0 0 1 1
( 384 0 0 ) ( 384 0 1 ) ( 384 1 0 ) wall 0 0 0 1 1
( 0 -16 0 ) ( 0 -16 1 ) ( 1 -16 0 ) wall 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 0 1 ) wall 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) wall 0 0 0 1 1
( 0 0 128 ) ( 0 1 128 ) ( 1 0 128 ) wall 0 0 0 1 1
}
{
( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) wall 0 0 0 1 1
( 384 0 0 ) ( 384 0 1 ) ( 384 1 0 ) wall 0 0 0 1 1
( 0 896 0 ) ( 0 896 1 ) ( 1 896 0 ) wall 0 0 0 1 1
( 0 912 0 ) ( 1 912 0 ) ( 0 912 1 ) wall 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) wall 0 0 0 1 1
( 0 0 128 ) ( 0 1 128 ) ( 1 0 128 ) wall 0 0 0 1 1
}
{
( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) wall 0 0 0 1 1
( 128 0 0 ) ( 128 0 1 ) ( 128 1 0 ) wall 0 0 0 1 1
( 0 128 0 ) ( 0 128 1 ) ( 1 128 0 ) wall 0 0 0 1 1
( 0 256 0 ) ( 1 256 0 ) ( 0 256 1 ) wall 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) wall 0 0 0 1 1
( 0 0 128 ) ( 0 1 128 ) ( 1 0 128 ) wall 0 0 0 1 1
}
{
( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) wall 0 0 0 1 1
( 128 0 0 ) ( 128 0 1 ) ( 128 1 0 ) wall 0 0 0 1 1
( 0 256 0 ) ( 0 256 1 ) ( 1 256 0 ) wall 0 0 0 1 1
( 0 384 0 ) ( 1 384 0 ) ( 0 384 1 ) wall 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) wall 0 0 0 1 1
( 0 0 128 ) ( 0 1 128 ) ( 1 0 128 ) wall 0 0 0 1 1
}
{
( 128 0 0 ) ( 128 1 0 ) ( 128 0 1 ) wall 0 0 0 1 1
( 256 0 0 ) ( 256 0 1 ) ( 256 1 0 ) wall 0 0 0 1 1
( 0 128 0 ) ( 0 128 1 ) ( 1 128 0 ) wall 0 0 0 1 1
( 0 256 0 ) ( 1 256 0 ) ( 0 256 1 ) wall 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) wall 0 0 0 1 1
( 0 0 128 ) ( 0 1 128 ) ( 1 0 128 ) wall 0 0 0 1 1
}
{
( 128 0 0 ) ( 128 1 0 ) ( 128 0 1 ) wall 0 0 0 1 1
( 256 0 0 ) ( 256 0 1 ) ( 256 1 0 ) wall 0 0 0 1 1
( 0 256 0 ) ( 0 256 1 ) ( 1 256 0 ) wall 0 0 0 1 1
( 0 384 0 ) ( 1 384 0 ) ( 0 384 1 ) wall 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) wall 0 0 0 1 1
( 0 0 128 ) ( 0 1 128 ) ( 1 0 128 ) wall 0 0 0 1 1
}
{
( 128 0 0 ) ( 128 1 0 ) ( 128 0 1 ) wall 0 0 0 1 1
( 256 0 0 ) ( 256 0 1 ) ( 256 1 0 ) wall 0 0 0 1 1
( 0 512 0 ) ( 0 512 1 ) ( 1 512 0 ) wall 0 0 0 1 1
( 0 640 0 ) ( 1 640 0 ) ( 0 640 1 ) wall 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) wall 0 0 0 1 1
( 0 0 128 ) ( 0 1 128 ) ( 1 0 128 ) wall 0 0 0 1 1
}
{
( 128 0 0 ) ( 128 1 0 ) ( 128 0 1 ) wall 0 0 0 1 1
( 256 0 0 ) ( 256 0 1 ) ( 256 1 0 ) wall 0 0 0 1 1
( 0 640 0 ) ( 0 640 1 ) ( 1 640 0 ) wall 0 0 0 1 1
( 0 768 0 ) ( 1 768 0 ) ( 0 768 1 ) wall 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) wall 0 0 0 1 1
( 0 0 128 ) ( 0 1 128 ) ( 1 0 128 ) wall 0 0 0 1 1
}
{
( 256 0 0 ) ( 256 1 0 ) ( 256 0 1 ) wall 0 0 0 1 1
( 384 0 0 ) ( 384 0 1 ) ( 384 1 0 ) wall 0 0 0 1 1
( 0 512 0 ) ( 0 512 1 ) ( 1 512 0 ) wall 0 0 0 1 1
( 0 640 0 ) ( 1 640 0 ) ( 0 640 1 ) wall 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) wall 0 0 0 1 1
( 0 0 128 ) ( 0 1 128 ) ( 1 0 128 ) wall 0 0 0 1 1
}
{
( 256 0 0 ) ( 256 1 0 ) ( 256 0 1 ) wall 0 0 0 1 1
( 384 0 0 ) ( 384 0 1 ) ( 384 1 0 ) wall 0 0 0 1 1
( 0 640 0 ) ( 0 640 1 ) ( 1 640 0 ) wall 0 0 0 1 1
( 0 768 0 ) ( 1 768 0 ) ( 0 768 1 ) wall 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) wall 0 0 0 1 1
( 0 0 128 ) ( 0 1 128 ) ( 1 0 128 ) wall 0 0 0 1 1
}
{
( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) *water 0 0 0 1 1
( 128 0 0 ) ( 128 0 1 ) ( 128 1 0 ) *water 0 0 0 1 1
( 0 768 0 ) ( 0 768 1 ) ( 1 768 0 ) *water 0 0 0 1 1
( 0 896 0 ) ( 1 896 0 ) ( 0 896 1 ) *water 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) *water 0 0 0 1 1
( 0 0 8 ) ( 0 1 8 ) ( 1 0 8 ) *water 0 0 0 1 1
}
}
{
"classname" "info_player_start"
"origin" "64 64 32"
}
"#;

/// ericw-tools' qbsp's portal file for [`SNAKE`]
/// (`ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266`, `ericw-qbsp snake.map`).
const SNAKE_PRT2: &str = r#"PRT2
9
7
6
4 0 2 (256 896 0) (256 768 0) (256 768 128) (256 896 128) 
4 1 4 (256 128 0) (256 0 0) (256 0 128) (256 128 128) 
4 1 3 (256 512 0) (256 384 0) (256 384 128) (256 512 128) 
4 2 5 (128 896 0) (128 768 0) (128 768 128) (128 896 128) 
4 3 5 (128 512 0) (128 384 0) (128 384 128) (128 512 128) 
4 4 6 (128 128 0) (128 0 0) (128 0 128) (128 128 128) 
0 -1
1 -1
2 -1
3 -1
4 -1
5 6 7 -1
8 -1
"#;

/// ericw-tools' qbsp's portal file for [`SNAKE`] with a solid `floor` in place of the water.
const SNAKE_FLOOR_PRT1: &str = r#"PRT1
8
7
4 0 2 (256 896 0) (256 768 0) (256 768 128) (256 896 128) 
4 1 4 (256 128 0) (256 0 0) (256 0 128) (256 128 128) 
4 1 3 (256 512 0) (256 384 0) (256 384 128) (256 512 128) 
4 2 5 (128 768 8) (128 768 128) (128 896 128) (128 896 8) 
4 3 6 (128 512 0) (128 384 0) (128 384 128) (128 512 128) 
4 4 7 (128 128 0) (128 0 0) (128 0 128) (128 128 128) 
4 5 6 (0 768 128) (0 768 8) (128 768 8) (128 768 128) 
"#;

// spec QBSP-15.52: the snake's portal file is ericw-tools' qbsp's byte for byte: PRT2 by cluster
// with its cluster map when the water makes detail, PRT1 by leaf when the floor is solid
#[test]
fn portal_files_are_ericw_tools() {
    let portal_file = |map: &str| {
        let compiled = Compiler::default()
            .with_dialect(Dialect::Extended)
            .compile(map.as_bytes(), None)
            .unwrap();
        String::from_utf8(compiled.portal_file.unwrap()).unwrap()
    };
    assert_eq!(portal_file(SNAKE), SNAKE_PRT2);
    assert_eq!(
        portal_file(&SNAKE.replace("*water", "floor")),
        SNAKE_FLOOR_PRT1
    );
}
