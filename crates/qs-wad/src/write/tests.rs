// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use proptest::prelude::*;

use super::{WadBuilder, WadWriteError, field};
use crate::{WAD_TYP_LABEL, WAD_TYP_MIPTEX, WAD_TYP_QPIC, Wad, w_cleanup_name};

// spec WAD-5.1, WAD-5.2: the lumps follow the header in order, the directory comes last, and
// each entry gives its lump's place, length and type, uncompressed, its name as given
#[test]
fn lumps_are_written_in_order_with_the_directory_last() {
    let mut wad = WadBuilder::new();
    wad.add(b"First", WAD_TYP_QPIC, vec![1, 2, 3]).unwrap();
    wad.add(b"empty", WAD_TYP_LABEL, Vec::new()).unwrap();
    wad.add(b"brick_1", WAD_TYP_MIPTEX, vec![9; 5]).unwrap();
    let bytes = wad.to_bytes().unwrap();
    assert_eq!(&bytes[..4], b"WAD2");
    assert_eq!(i32::from_le_bytes(bytes[4..8].try_into().unwrap()), 3);
    assert_eq!(i32::from_le_bytes(bytes[8..12].try_into().unwrap()), 12 + 8);
    assert_eq!(bytes.len(), 20 + 3 * 32);
    let read = Wad::read(&bytes).unwrap();
    let lumps = read.lumps();
    assert_eq!(lumps.len(), 3);
    assert_eq!(
        lumps.iter().map(|l| l.filepos).collect::<Vec<_>>(),
        [12, 15, 15]
    );
    assert_eq!(
        lumps
            .iter()
            .map(|l| (l.disksize, l.size))
            .collect::<Vec<_>>(),
        [(3, 3), (0, 0), (5, 5)]
    );
    assert_eq!(
        lumps.iter().map(|l| l.lump_type).collect::<Vec<_>>(),
        [WAD_TYP_QPIC, WAD_TYP_LABEL, WAD_TYP_MIPTEX]
    );
    assert!(lumps.iter().all(|l| l.compression == 0));
    assert_eq!(read.data(&lumps[0]).unwrap(), [1, 2, 3]);
    assert_eq!(read.data(&lumps[2]).unwrap(), [9; 5]);
    assert_eq!(lumps[0].name, w_cleanup_name(b"first"));
    // The name in the file is as given, its case kept, the rest of the field NUL.
    let entry = &bytes[20..52];
    assert_eq!(&entry[12..16], [WAD_TYP_QPIC, 0, 0, 0]);
    assert_eq!(&entry[16..32], b"First\0\0\0\0\0\0\0\0\0\0\0");
}

// spec WAD-5.3: an empty name, one of 16 bytes, one holding a NUL, or one an earlier lump has
// with case ignored is refused; 15 bytes is the longest
#[test]
fn bad_and_doubled_names_are_refused() {
    let mut wad = WadBuilder::new();
    for name in [&b""[..], &[b'a'; 16], b"a\0b"] {
        assert_eq!(
            wad.add(name, WAD_TYP_QPIC, Vec::new()),
            Err(WadWriteError::BadName {
                name: name.to_vec()
            })
        );
    }
    wad.add(&[b'a'; 15], WAD_TYP_QPIC, Vec::new()).unwrap();
    wad.add(b"Rock", WAD_TYP_QPIC, Vec::new()).unwrap();
    assert_eq!(
        wad.add(b"ROCK", WAD_TYP_QPIC, Vec::new()),
        Err(WadWriteError::DuplicateName {
            name: b"ROCK".to_vec()
        })
    );
    assert!(
        WadWriteError::DuplicateName {
            name: b"ROCK".to_vec()
        }
        .to_string()
        .contains("ROCK")
    );
    assert!(
        WadWriteError::BadName { name: Vec::new() }
            .to_string()
            .contains("1 to 15")
    );
    assert!(WadWriteError::TooLarge.to_string().contains("2 GiB"));
}

// spec WAD-5.4: a size or offset past a signed 32-bit integer is refused
#[test]
fn sizes_past_32_bits_are_refused() {
    assert_eq!(field(0x7fff_ffff), Ok(i32::MAX));
    assert_eq!(field(0x8000_0000), Err(WadWriteError::TooLarge));
    assert_eq!(field(usize::MAX), Err(WadWriteError::TooLarge));
}

// spec WAD-5.1: an empty WAD is its header, the directory right after it
#[test]
fn an_empty_wad_is_its_header() {
    let bytes = WadBuilder::new().to_bytes().unwrap();
    assert_eq!(bytes, b"WAD2\0\0\0\0\x0c\0\0\0");
    assert!(Wad::read(&bytes).unwrap().lumps().is_empty());
}

proptest! {
    // spec WAD-5.1, WAD-5.2: any lumps written read back with the same data, types and names
    #[test]
    fn written_lumps_read_back(
        lumps in prop::collection::vec(
            (prop::collection::vec(any::<u8>(), 0..64), any::<u8>()), 0..8)
    ) {
        let mut wad = WadBuilder::new();
        for (index, (data, lump_type)) in lumps.iter().enumerate() {
            wad.add(format!("lump{index}").as_bytes(), *lump_type, data.clone()).unwrap();
        }
        let bytes = wad.to_bytes().unwrap();
        let read = Wad::read(&bytes).unwrap();
        prop_assert_eq!(read.lumps().len(), lumps.len());
        for (index, (lump, (data, lump_type))) in read.lumps().iter().zip(&lumps).enumerate() {
            prop_assert_eq!(read.data(lump).unwrap(), data.as_slice());
            prop_assert_eq!(lump.lump_type, *lump_type);
            prop_assert_eq!(lump.name, w_cleanup_name(format!("lump{index}").as_bytes()));
        }
    }
}
