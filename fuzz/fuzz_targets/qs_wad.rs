// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary bytes to the WAD2 reader (WAD-1 to WAD-3) and every lump it accepts to the
//! lump decoders (WAD-4). Fails on a panic, and on a lump its own name does not find.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qs_wad::{Conchars, MipTexHeader, QPic, Wad};

fuzz_target!(|data: &[u8]| {
    let Ok(wad) = Wad::read(data) else {
        return;
    };
    for (index, lump) in wad.lumps().iter().enumerate() {
        assert!(wad.find(lump.name.as_bytes()).is_ok());
        assert_eq!(wad.lump(index), Ok(lump));
        let Ok(bytes) = wad.data(lump) else {
            continue;
        };
        if let Ok(pic) = QPic::parse(bytes) {
            assert_eq!(pic.pixels.len(), pic.width * pic.height);
        }
        if let Ok(conchars) = Conchars::parse(bytes) {
            for n in [0, 0x41, 0xFF] {
                let _glyph = conchars.glyph(n);
            }
        }
        assert_eq!(MipTexHeader::parse(bytes).is_ok(), bytes.len() >= 40);
    }
    assert!(wad.lump(wad.lumps().len()).is_err());
});
