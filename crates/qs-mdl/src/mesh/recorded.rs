// SPDX-License-Identifier: GPL-3.0-or-later

//! What FTEQW's converter made of the fixtures of `fixtures/`, recorded by
//! `scripts/model-formats/iqm.py` from the `.iqm` its `iqmtool` wrote of each, which the
//! ported readers are held to (MDL-10).

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap, index or panic is a failed test"
)]

use super::Mesh;

/// How far a posed corner may be from the converter's: its weights are bytes and its frames'
/// channels 16 bits.
const NEAR: f32 = 0.05;

/// A triangle's corners, each `x y z u v`.
type Triangle = [[f32; 5]; 3];

/// The frames of a recording, each its triangles.
fn frames(recorded: &str) -> Vec<Vec<Triangle>> {
    let mut frames: Vec<Vec<Triangle>> = Vec::new();
    for line in recorded.lines().filter(|line| !line.starts_with('#')) {
        if line == "frame" {
            frames.push(Vec::new());
            continue;
        }
        let numbers: Vec<f32> = line
            .split_whitespace()
            .map(|word| word.parse().unwrap())
            .collect();
        let (corners, rest) = numbers.as_chunks::<5>();
        assert!(rest.is_empty(), "{line}");
        frames
            .last_mut()
            .unwrap()
            .push(<Triangle>::try_from(corners).unwrap());
    }
    frames
}

/// Checks that `mesh` is the converter's model: as many frames, each of as many triangles,
/// and each of ours one of theirs, its corners in the same order around it, within
/// [`NEAR`], each of theirs matched once.
pub(super) fn agrees(mesh: &Mesh, recorded: &str) {
    let theirs = frames(recorded);
    assert_eq!(mesh.frames.len(), theirs.len(), "frames");
    for (frame, theirs) in mesh.frames.iter().zip(theirs) {
        assert_eq!(mesh.triangles.len(), theirs.len(), "triangles");
        let mut left = theirs;
        for corners in &mesh.triangles {
            let ours = corners.map(|corner| {
                let [x, y, z] = frame.positions[corner.position];
                let [u, v] = corner.texcoord.map_or([0.0, 0.0], |at| mesh.texcoords[at]);
                [x, y, z, u, v]
            });
            let apart = |theirs: &Triangle, turn: usize| {
                ours.iter()
                    .zip(theirs.iter().cycle().skip(turn))
                    .flat_map(|(ours, theirs)| ours.iter().zip(theirs))
                    .map(|(ours, theirs)| (ours - theirs).abs())
                    .fold(0.0_f32, f32::max)
            };
            let found = left
                .iter()
                .position(|theirs| (0..3).any(|turn| apart(theirs, turn) <= NEAR));
            let Some(found) = found else {
                panic!("{}: no triangle of the converter's is {ours:?}", frame.name);
            };
            left.remove(found);
        }
    }
}
