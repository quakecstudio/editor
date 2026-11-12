// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary bytes to the WAD2 reader (WAD-1 to WAD-3) and every lump it accepts to the
//! lump decoders (WAD-4), and to the map WAD reader (WAD3-1 to WAD3-3), as given and as a WAD3,
//! reading every entry's texture. Fails on a panic, on a lump its own name does not find, and
//! on a texture with more pixels than its size, or a palette longer than its count gives.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qs_wad::{Conchars, MapWad, MipTexHeader, QPic, Wad};

fn map_wad(data: &[u8]) {
    let Ok(wad) = MapWad::read(data) else {
        return;
    };
    for entry in wad.entries() {
        let Some(texture) = wad.texture(entry.name.as_bytes()) else {
            continue;
        };
        let area = u64::from(texture.width) * u64::from(texture.height);
        assert!(texture.pixels.len() as u64 <= area);
        if let Some(palette) = texture.palette {
            assert!(palette.len() <= 2 + 3 * usize::from(u16::MAX));
        }
    }
}

fuzz_target!(|data: &[u8]| {
    map_wad(data);
    if data.len() >= 4 {
        let mut wad3 = data.to_vec();
        wad3[..4].copy_from_slice(b"WAD3");
        map_wad(&wad3);
    }
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
