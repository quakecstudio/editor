// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap, index or panic is a failed test"
)]

use super::*;

/// Every `.mdl` of the PAKs named in `QS_MDL_PAKS`, separated by `:`, with its name.
fn corpus() -> Vec<(String, Vec<u8>)> {
    let Ok(paks) = std::env::var("QS_MDL_PAKS") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for pak in paks.split(':').filter(|pak| !pak.is_empty()) {
        let data = std::fs::read(pak).unwrap();
        let word = |at: usize| {
            usize::try_from(i32::from_le_bytes(data[at..at + 4].try_into().unwrap())).unwrap()
        };
        let (dir, len) = (word(4), word(8));
        for entry in (dir..dir + len).step_by(64) {
            let name: Vec<u8> = data[entry..entry + 56]
                .iter()
                .copied()
                .take_while(|&byte| byte != 0)
                .collect();
            let name = String::from_utf8_lossy(&name).into_owned();
            if name.ends_with(".mdl") {
                let (at, size) = (word(entry + 56), word(entry + 60));
                out.push((format!("{pak}:{name}"), data[at..at + size].to_vec()));
            }
        }
    }
    out
}

// spec MDL-6.1: a model read and written with nothing changed is its file, byte for byte;
// run over a game's models with QS_MDL_PAKS, and over none without it
#[test]
fn a_games_models_are_written_back_as_read() {
    let mut count = 0;
    for (name, bytes) in corpus() {
        let model = Model::read(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
        let written = model.to_bytes().unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(written == bytes, "{name}: {} bytes for {}", written.len(), bytes.len());
        count += 1;
    }
    eprintln!("{count} models written back as read");
}
