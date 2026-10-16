// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary bytes to the sprite reader (SPR-1, SPR-2) and to the MD5 mesh and
//! animation readers (MD5-1 to MD5-3), each as given, after a valid start, and for MD5 as
//! tokens the bytes pick from the formats' keywords, so the fuzzer reaches past the headers.
//! Fails on a panic, and on a frame whose pixels do not match its size.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qs_mdl::spr::{SpriteFrame, SpriteFrameEntry};
use qs_mdl::{Md5Anim, Md5Mesh, Sprite};

/// A sprite header of version 1 with one frame entry, the start of SPR-1.1.
const SPR_START: &[u8] =
    b"IDSP\x01\0\0\0\x02\0\0\0\0\0\0\0\x10\0\0\0\x10\0\0\0\x01\0\0\0\0\0\0\0\0\0\0\0";
const MESH_START: &[u8] =
    b"MD5Version 10 numJoints 1 numMeshes 1 joints { \"r\" -1 ( 0 0 0 ) ( 0 0 0 ) } ";
const ANIM_START: &[u8] =
    b"MD5Version 10 numFrames 2 numJoints 1 frameRate 24 numAnimatedComponents 6 ";

/// Tokens of both files, which each input byte picks from.
const VOCABULARY: [&str; 24] = [
    "mesh",
    "{",
    "}",
    "(",
    ")",
    "shader",
    "numverts",
    "vert",
    "numtris",
    "tri",
    "numweights",
    "weight",
    "hierarchy",
    "bounds",
    "baseframe",
    "frame",
    "0",
    "1",
    "2",
    "63",
    "-1",
    "0.5",
    "\"r\"",
    "//",
];

/// Text of the tokens `data` picks, one byte each.
fn words(data: &[u8]) -> Vec<u8> {
    let mut text = Vec::new();
    for &byte in data {
        text.extend_from_slice(VOCABULARY[usize::from(byte) % VOCABULARY.len()].as_bytes());
        text.push(if byte >= 0xF0 { b'\n' } else { b' ' });
    }
    text
}

fn check(frame: &SpriteFrame<'_>) {
    let width = usize::try_from(frame.width).unwrap();
    let height = usize::try_from(frame.height).unwrap();
    assert_eq!(frame.pixels.len(), width * height);
    let _extents = (frame.up(), frame.down(), frame.left(), frame.right());
}

fn sprite(data: &[u8]) {
    let Ok(sprite) = Sprite::read(data) else {
        return;
    };
    let _bounds = sprite.bounds();
    for entry in &sprite.frames {
        match entry {
            SpriteFrameEntry::Single(frame) => check(frame),
            SpriteFrameEntry::Group(group) => {
                assert_eq!(group.intervals.len(), group.frames.len());
                group.frames.iter().for_each(check);
            }
        }
    }
}

fn md5(mesh: &[u8], anim: &[u8]) {
    let mesh = Md5Mesh::read(mesh);
    if let Ok(mesh) = &mesh {
        for surface in &mesh.meshes {
            for index in 0..surface.num_verts.min(64) {
                assert!(surface.vert(index).is_some());
            }
            assert!(surface.tri(surface.num_tris).is_none());
        }
    }
    if let Ok(anim) = Md5Anim::read(anim) {
        for frame in 0..anim.num_frames.min(4) {
            for joint in 0..anim.hierarchy.len() {
                assert!(anim.pose(frame, joint).is_some());
            }
        }
        if let Ok(mesh) = &mesh {
            let _fits = anim.check_mesh(mesh);
        }
    }
}

fn prefixed(start: &[u8], data: &[u8]) -> Vec<u8> {
    let mut bytes = start.to_vec();
    bytes.extend_from_slice(data);
    bytes
}

fuzz_target!(|data: &[u8]| {
    sprite(data);
    sprite(&prefixed(SPR_START, data));
    md5(data, data);
    md5(&prefixed(MESH_START, data), &prefixed(ANIM_START, data));
    let text = words(data);
    md5(&prefixed(MESH_START, &text), &prefixed(ANIM_START, &text));
});
