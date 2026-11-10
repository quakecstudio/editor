// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use proptest::prelude::*;

use super::{PakWriteError, write_pak};
use crate::{PAK_HEADER_SIZE, PAK_MAX_FILES, Pak};

/// Each entry's name and contents, read back from `bytes` in directory order.
fn read_back(bytes: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let pak = Pak::read(bytes).unwrap();
    pak.entries()
        .iter()
        .map(|entry| {
            (
                entry.name().to_vec(),
                pak.entry_data(bytes, entry).unwrap().to_vec(),
            )
        })
        .collect()
}

// spec PAK-6.1: the files are read back with their names and contents, in the order of their
// names' bytes, whatever order they were given in
#[test]
fn files_read_back_sorted_by_name() {
    let files = [
        ("progs.dat", b"progs".to_vec()),
        ("maps/start.bsp", b"bsp".to_vec()),
        ("gfx/conchars.lmp", Vec::new()),
    ];
    let bytes = write_pak(&files).unwrap();
    assert_eq!(
        read_back(&bytes),
        [
            (b"gfx/conchars.lmp".to_vec(), Vec::new()),
            (b"maps/start.bsp".to_vec(), b"bsp".to_vec()),
            (b"progs.dat".to_vec(), b"progs".to_vec()),
        ]
    );
}

// spec PAK-6.2: the header, then the contents in name order, then the directory at the end;
// no timestamps or padding, so the bytes are the same for any order the files are given in
#[test]
fn the_layout_is_fixed_and_the_same_for_any_order() {
    let a = write_pak(&[("b", "22"), ("a", "1")]).unwrap();
    let b = write_pak(&[("a", "1"), ("b", "22")]).unwrap();
    assert_eq!(a, b);
    assert_eq!(&a[..4], b"PACK");
    let dirofs = i32::from_le_bytes(a[4..8].try_into().unwrap());
    let dirlen = i32::from_le_bytes(a[8..12].try_into().unwrap());
    assert_eq!(&a[PAK_HEADER_SIZE..PAK_HEADER_SIZE + 3], b"122");
    assert_eq!(dirofs, 15);
    assert_eq!(dirlen, 128);
    assert_eq!(a.len(), 15 + 128);
    let empty = write_pak::<&str, &str>(&[]).unwrap();
    assert_eq!(empty, b"PACK\x0c\0\0\0\0\0\0\0");
    assert!(Pak::read(&empty).unwrap().entries().is_empty());
}

// spec PAK-6.3: a name is 1 to 55 bytes with no NUL; 55 bytes fit, 56 do not
#[test]
fn names_the_field_cannot_hold_are_refused() {
    assert_eq!(write_pak(&[("", "x")]), Err(PakWriteError::EmptyName));
    assert!(matches!(
        write_pak(&[(b"a\0b".as_slice(), b"x".as_slice())]),
        Err(PakWriteError::NameHasNul { .. })
    ));
    let longest = "n".repeat(55);
    assert_eq!(
        read_back(&write_pak(&[(&longest, "x")]).unwrap())[0].0,
        longest.as_bytes()
    );
    assert!(matches!(
        write_pak(&[("n".repeat(56), "x")]),
        Err(PakWriteError::NameTooLong { .. })
    ));
}

// spec PAK-6.1: a name given twice is refused
#[test]
fn a_name_given_twice_is_refused() {
    assert_eq!(
        write_pak(&[("a", "1"), ("b", "2"), ("a", "3")]),
        Err(PakWriteError::Duplicate {
            name: b"a".to_vec()
        })
    );
}

// spec PAK-6.4: 2048 files are written and read back; 2049 are refused
#[test]
fn at_most_2048_files_are_written() {
    let names: Vec<String> = (0..=PAK_MAX_FILES).map(|n| format!("f{n:04}")).collect();
    let files: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), "")).collect();
    let most = &files[..2048];
    assert_eq!(read_back(&write_pak(most).unwrap()).len(), 2048);
    assert_eq!(
        write_pak(&files),
        Err(PakWriteError::TooManyFiles { count: 2049 })
    );
}

proptest! {
    // spec PAK-6.1, PAK-6.2: any tree of distinct valid names round-trips byte for byte
    #[test]
    fn any_tree_round_trips(
        tree in proptest::collection::btree_map(
            "[a-z0-9_/.]{1,55}",
            proptest::collection::vec(any::<u8>(), 0..300),
            0..40,
        )
    ) {
        let files: Vec<(Vec<u8>, Vec<u8>)> =
            tree.into_iter().map(|(name, data)| (name.into_bytes(), data)).collect();
        let bytes = write_pak(&files).unwrap();
        prop_assert_eq!(read_back(&bytes), files);
    }
}
