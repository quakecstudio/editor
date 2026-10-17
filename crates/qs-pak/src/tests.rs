// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "test code: a failed unwrap, index, overflow or panic is a failed test"
)]

use std::io::Cursor;

use proptest::prelude::*;

use crate::fixtures::{EntrySpec, PakSpec, entry, patch};
use crate::{
    Entry, Header, PAK_CRC_INIT, PAK_CRC_POLY, PAK_ENTRY_SIZE, PAK_HEADER_SIZE, PAK_MAGIC,
    PAK_MAX_FILES, PAK_NAME_SIZE, PAK_PAK0_COUNT, PAK_PAK0_CRC, Pak, PakError,
};

/// Loads `bytes` both ways and checks the two agree.
fn load(bytes: &[u8]) -> Result<Pak, PakError> {
    let in_memory = Pak::read(bytes);
    let from_file = Pak::read_from(&mut Cursor::new(bytes));
    assert_eq!(in_memory, from_file);
    in_memory
}

/// The data of every entry, read both ways.
fn contents(bytes: &[u8], pak: &Pak) -> Vec<Result<Vec<u8>, PakError>> {
    let mut all = Vec::new();
    for entry in pak.entries() {
        let in_memory = pak.entry_data(bytes, entry).map(<[u8]>::to_vec);
        let from_file = pak.read_entry(&mut Cursor::new(bytes), entry);
        assert_eq!(in_memory, from_file);
        all.push(in_memory);
    }
    all
}

fn names(pak: &Pak) -> Vec<&[u8]> {
    pak.entries().iter().map(Entry::name).collect()
}

/// `count` entries named `f0`, `f1`, ... with no data.
fn empty_entries(count: usize) -> Vec<EntrySpec> {
    (0..count).map(|i| entry(&format!("f{i}"), b"")).collect()
}

// spec PAK-1.1: a 12-byte header of `PACK`, then `dirofs` and `dirlen` as little-endian i32.
#[test]
fn header_decodes() {
    let mut bytes = PakSpec::default().build();
    assert_eq!(&bytes[..4], b"PACK");
    assert_eq!(PAK_MAGIC, [0x50, 0x41, 0x43, 0x4B]);
    assert_eq!(PAK_HEADER_SIZE, 12);
    patch(&mut bytes, 4, 0x0102_0304);
    patch(&mut bytes, 8, -7);
    assert_eq!(
        Header::parse(&bytes).unwrap(),
        Header {
            dirofs: 0x0102_0304,
            dirlen: -7
        }
    );
}

// spec PAK-1.2: any magic but exactly `PACK` fails with `<path> is not a packfile`.
#[test]
fn wrong_magic_is_not_a_packfile() {
    for magic in [*b"pack", *b"PACk", *b"KCAP", *b"WAD2", [0; 4]] {
        let bytes = PakSpec {
            magic,
            ..PakSpec::default()
        }
        .build();
        assert_eq!(load(&bytes), Err(PakError::NotAPackfile));
    }
    assert_eq!(
        format!("id1/pak0.pak {}", PakError::NotAPackfile),
        "id1/pak0.pak is not a packfile"
    );
}

// spec PAK-1.3: a file of fewer than 12 bytes fails to load, as PAK-1.2 does.
#[test]
fn short_file_is_not_a_packfile() {
    let bytes = PakSpec::default().build();
    assert_eq!(bytes.len(), PAK_HEADER_SIZE);
    for len in 0..PAK_HEADER_SIZE {
        assert_eq!(load(&bytes[..len]), Err(PakError::NotAPackfile), "{len}");
    }
    assert!(load(&bytes).is_ok());
}

// spec PAK-2.1: 64-byte entries of a 56-byte name, `filepos` and `filelen`.
// spec PAK-2.2: the count is `dirlen / 64`; the remainder is read and counted by the CRC.
// spec PAK-3.3: an entry with `filelen` 0 has empty contents.
// spec PAK-4.1: the CRC covers all `dirlen` bytes, partial entry included (vector: 29531).
#[test]
fn spec_vector_two_entries_and_extra_directory_bytes() {
    let bytes = PakSpec {
        entries: vec![entry("a.txt", b"AB"), entry("b.txt", b"")],
        extra_directory: vec![b'x'; 10],
        ..PakSpec::default()
    }
    .build();
    let pak = load(&bytes).unwrap();
    assert_eq!(PAK_ENTRY_SIZE, 64);
    assert_eq!(PAK_NAME_SIZE, 56);
    assert_eq!(pak.header().dirlen, 138);
    assert_eq!(pak.file_len(), u64::try_from(bytes.len()).unwrap());
    assert_eq!(pak.header().entry_count(), 2);
    assert_eq!(
        pak.entries(),
        [
            Entry {
                name: b"a.txt".to_vec(),
                filepos: 12,
                filelen: 2
            },
            Entry {
                name: b"b.txt".to_vec(),
                filepos: 14,
                filelen: 0
            },
        ]
    );
    assert_eq!(pak.directory_crc(), 29531);
    assert_eq!(contents(&bytes, &pak), [Ok(b"AB".to_vec()), Ok(Vec::new())]);
}

// spec PAK-2.3: a `dirlen` from 0 to 63 gives an archive with no entries.
// spec PAK-4.1: the CRC of an empty directory is 0xFFFF.
#[test]
fn short_directory_has_no_entries() {
    for extra in 0..PAK_ENTRY_SIZE {
        let bytes = PakSpec {
            extra_directory: vec![0x41; extra],
            ..PakSpec::default()
        }
        .build();
        let pak = load(&bytes).unwrap();
        assert!(pak.entries().is_empty(), "{extra}");
    }
    let pak = load(&PakSpec::default().build()).unwrap();
    assert_eq!(pak.directory_crc(), 0xFFFF);
    assert_eq!(pak.directory_crc(), PAK_CRC_INIT);
}

// spec PAK-2.4: 2048 entries load; 2049 fail with `<path> has 2049 files`.
#[test]
fn entry_limit() {
    assert_eq!(PAK_MAX_FILES, 2048);
    let at = PakSpec {
        entries: empty_entries(2048),
        ..PakSpec::default()
    }
    .build();
    assert_eq!(load(&at).unwrap().entries().len(), 2048);
    let above = PakSpec {
        entries: empty_entries(2049),
        ..PakSpec::default()
    }
    .build();
    let error = load(&above).unwrap_err();
    assert_eq!(error, PakError::TooManyFiles { count: 2049 });
    assert_eq!(format!("p.pak {error}"), "p.pak has 2049 files");
}

// spec PAK-2.5: a negative `dirlen` is divided as unsigned 32 bits: -64 gives 67108863 files.
#[test]
fn negative_dirlen_counts_unsigned() {
    let mut bytes = PakSpec::default().build();
    patch(&mut bytes, 8, -64);
    assert_eq!(
        load(&bytes),
        Err(PakError::TooManyFiles { count: 67_108_863 })
    );
    for dirlen in [-1, i32::MIN, -63] {
        patch(&mut bytes, 8, dirlen);
        let Err(PakError::TooManyFiles { count }) = load(&bytes) else {
            panic!("dirlen {dirlen} loaded");
        };
        assert!(count >= 33_554_432, "{dirlen}: {count}");
    }
}

// spec PAK-2.6: a `dirlen` from 131073 to 131135 overflows id's buffer and fails.
#[test]
fn partial_entry_past_the_limit_fails() {
    let mut spec = PakSpec {
        entries: empty_entries(2048),
        ..PakSpec::default()
    };
    for extra in [1, 63] {
        spec.extra_directory = vec![0; extra];
        let bytes = spec.build();
        let dirlen = 131_072 + i32::try_from(extra).unwrap();
        assert_eq!(load(&bytes), Err(PakError::DirectoryTooLarge { dirlen }));
    }
    spec.extra_directory = Vec::new();
    assert!(load(&spec.build()).is_ok());
}

// spec PAK-2.7: a directory outside the file (negative `dirofs` or past the end) fails.
#[test]
fn directory_outside_the_file() {
    let good = PakSpec {
        entries: vec![entry("a", b"x")],
        ..PakSpec::default()
    }
    .build();
    let dirofs = i32::try_from(good.len() - PAK_ENTRY_SIZE).unwrap();
    for (dirofs, dirlen) in [(-1, 64), (i32::MIN, 0), (dirofs + 1, 64), (dirofs, 65)] {
        let mut bytes = good.clone();
        patch(&mut bytes, 4, dirofs);
        patch(&mut bytes, 8, dirlen);
        assert_eq!(
            load(&bytes),
            Err(PakError::DirectoryOutOfBounds { dirofs, dirlen }),
            "{dirofs} {dirlen}"
        );
    }
    let mut empty_at_end = good;
    let end = i32::try_from(empty_at_end.len()).unwrap();
    patch(&mut empty_at_end, 4, end);
    patch(&mut empty_at_end, 8, 0);
    assert!(load(&empty_at_end).unwrap().entries().is_empty());
}

// spec PAK-2.8: the name is the bytes before the first NUL; bytes after it are ignored.
#[test]
fn name_stops_at_first_nul() {
    let bytes = PakSpec {
        entries: vec![EntrySpec {
            name: b"abc\0garbage".to_vec(),
            data: Vec::new(),
        }],
        ..PakSpec::default()
    }
    .build();
    assert_eq!(names(&load(&bytes).unwrap()), [b"abc".as_slice()]);
}

// spec PAK-2.9: a 56-byte name field with no NUL fails the load.
#[test]
fn unterminated_name_fails() {
    let full = vec![b'n'; PAK_NAME_SIZE];
    let bytes = PakSpec {
        entries: vec![
            entry("ok", b""),
            EntrySpec {
                name: full.clone(),
                data: Vec::new(),
            },
        ],
        ..PakSpec::default()
    }
    .build();
    assert_eq!(load(&bytes), Err(PakError::UnterminatedName { index: 1 }));
    let mut fits = full;
    fits.truncate(PAK_NAME_SIZE - 1);
    let bytes = PakSpec {
        entries: vec![EntrySpec {
            name: fits.clone(),
            data: Vec::new(),
        }],
        ..PakSpec::default()
    }
    .build();
    assert_eq!(names(&load(&bytes).unwrap()), [fits.as_slice()]);
}

// spec PAK-2.10: names are raw bytes: no case folding or separator conversion; NUL first is "".
// spec PAK-3.1: lookups are byte-for-byte, so case and separators must match.
#[test]
fn names_are_raw_bytes() {
    let bytes = PakSpec {
        entries: vec![
            entry("MAPS/E1M1.BSP", b"Q"),
            entry("maps/e1m1.bsp", b"q"),
            entry("maps\\x.bsp", b"b"),
            entry(" sp ", b"s"),
            EntrySpec {
                name: vec![0xC9, b'/', 0xFF],
                data: b"h".to_vec(),
            },
            entry("", b"e"),
        ],
        ..PakSpec::default()
    }
    .build();
    let pak = load(&bytes).unwrap();
    assert_eq!(
        names(&pak),
        [
            b"MAPS/E1M1.BSP".as_slice(),
            b"maps/e1m1.bsp",
            b"maps\\x.bsp",
            b" sp ",
            &[0xC9, b'/', 0xFF],
            b"",
        ]
    );
    let data = |name: &[u8]| pak.entry_data(&bytes, pak.find(name).unwrap()).unwrap();
    assert_eq!(data(b"MAPS/E1M1.BSP"), b"Q");
    assert_eq!(data(b"maps/e1m1.bsp"), b"q");
    assert_eq!(data(b""), b"e");
    assert!(pak.find(b"Maps/E1m1.bsp").is_none());
    assert!(pak.find(b"maps/x.bsp").is_none());
    assert!(pak.find(b"sp").is_none());
}

// spec PAK-2.11: duplicate names are kept in order.
// spec PAK-3.1: a lookup returns the first entry with the name.
#[test]
fn duplicates_keep_order_and_first_wins() {
    let bytes = PakSpec {
        entries: vec![entry("a.txt", b"first"), entry("a.txt", b"second")],
        ..PakSpec::default()
    }
    .build();
    let pak = load(&bytes).unwrap();
    assert_eq!(names(&pak), [b"a.txt".as_slice(), b"a.txt"]);
    let found = pak.find(b"a.txt").unwrap();
    assert_eq!(pak.entry_data(&bytes, found).unwrap(), b"first");
    assert_eq!(
        contents(&bytes, &pak),
        [Ok(b"first".to_vec()), Ok(b"second".to_vec())]
    );
}

// spec PAK-2.12: bad `filepos` and `filelen` never stop the archive loading or listing.
// spec PAK-3.4: reading an entry whose range is outside the file is an error.
#[test]
fn bad_entry_range_loads_but_does_not_read() {
    let good = PakSpec {
        entries: vec![entry("a", b"xy"), entry("b", b"z")],
        ..PakSpec::default()
    }
    .build();
    let entry_b = good.len() - PAK_ENTRY_SIZE;
    let len = i32::try_from(good.len()).unwrap();
    for (filepos, filelen) in [(-1, 1), (0, -1), (0, i32::MIN), (len, 1), (len - 1, 2)] {
        let mut bytes = good.clone();
        patch(&mut bytes, entry_b + PAK_NAME_SIZE, filepos);
        patch(&mut bytes, entry_b + PAK_NAME_SIZE + 4, filelen);
        let pak = load(&bytes).unwrap();
        assert_eq!(names(&pak), [b"a".as_slice(), b"b"]);
        assert_eq!(
            contents(&bytes, &pak),
            [
                Ok(b"xy".to_vec()),
                Err(PakError::EntryOutOfBounds { filepos, filelen })
            ]
        );
    }
}

// spec PAK-3.2: contents are `filelen` bytes at `filepos`; ranges may overlap the header and
// the directory.
#[test]
fn entries_may_overlap_anything() {
    let mut bytes = PakSpec {
        entries: vec![entry("a", b""), entry("b", b"")],
        ..PakSpec::default()
    }
    .build();
    let dir = bytes.len() - 2 * PAK_ENTRY_SIZE;
    patch(&mut bytes, dir + PAK_NAME_SIZE, 0);
    patch(&mut bytes, dir + PAK_NAME_SIZE + 4, 6);
    patch(&mut bytes, dir + PAK_ENTRY_SIZE + PAK_NAME_SIZE, 2);
    let tail = i32::try_from(bytes.len() - 2).unwrap();
    patch(&mut bytes, dir + PAK_ENTRY_SIZE + PAK_NAME_SIZE + 4, tail);
    let pak = load(&bytes).unwrap();
    let all = contents(&bytes, &pak);
    assert_eq!(all[0].as_deref().unwrap(), &bytes[..6]);
    assert_eq!(all[1].as_deref().unwrap(), &bytes[2..]);
}

// spec PAK-4.1: CRC-16, polynomial 0x1021, not reflected, initial 0xFFFF, no final XOR.
#[test]
fn directory_crc_is_ccitt_false() {
    assert_eq!(PAK_CRC_POLY, 0x1021);
    assert_eq!(PAK_CRC_INIT, 0xFFFF);
    let bytes = PakSpec {
        extra_directory: b"123456789".to_vec(),
        ..PakSpec::default()
    }
    .build();
    // The CRC-16/CCITT-FALSE check value.
    assert_eq!(load(&bytes).unwrap().directory_crc(), 0x29B1);
}

// spec PAK-4.2: an archive is modified unless it has 339 entries and directory CRC 32981.
#[test]
fn modified_unless_count_and_crc_match_pak0() {
    assert_eq!(PAK_PAK0_COUNT, 339);
    assert_eq!(PAK_PAK0_CRC, 32981);
    assert!(load(&PakSpec::default().build()).unwrap().is_modified());
    let mut pak = load(
        &PakSpec {
            entries: empty_entries(PAK_PAK0_COUNT),
            ..PakSpec::default()
        }
        .build(),
    )
    .unwrap();
    assert_ne!(pak.directory_crc(), PAK_PAK0_CRC);
    assert!(pak.is_modified());
    pak.directory_crc = PAK_PAK0_CRC;
    assert!(!pak.is_modified());
    pak.entries.pop();
    assert!(pak.is_modified());
}

// spec PAK-5.1: the rerelease's archives are plain PAK archives of at most 2048 entries; a
// rerelease-sized directory of long names loads under PAK-1 to PAK-3 with no extension.
#[test]
fn rerelease_sized_archive_loads() {
    let entries: Vec<EntrySpec> = (0..PAK_MAX_FILES)
        .map(|i| {
            entry(
                &format!("maps/{i:0>46}.bsp"),
                &[u8::try_from(i % 251).unwrap()],
            )
        })
        .collect();
    let bytes = PakSpec {
        entries,
        ..PakSpec::default()
    }
    .build();
    let pak = load(&bytes).unwrap();
    assert_eq!(pak.entries().len(), 2048);
    let last = pak.entries().last().unwrap();
    assert_eq!(
        pak.entry_data(&bytes, last).unwrap(),
        [u8::try_from(2047 % 251).unwrap()]
    );
}

/// Reads a file that fails partway, for the I/O error path.
struct Failing;

impl std::io::Read for Failing {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::ErrorKind::PermissionDenied.into())
    }
}

impl std::io::Seek for Failing {
    fn seek(&mut self, _: std::io::SeekFrom) -> std::io::Result<u64> {
        Ok(100)
    }
}

#[test]
fn io_errors_are_values() {
    let error = Pak::read_from(&mut Failing).unwrap_err();
    assert_eq!(error, PakError::Io(std::io::ErrorKind::PermissionDenied));
    assert!(error.to_string().starts_with("could not be read"));
    let bytes = PakSpec {
        entries: vec![entry("a", b"xy")],
        ..PakSpec::default()
    }
    .build();
    let pak = load(&bytes).unwrap();
    let truncated = &bytes[..13];
    assert_eq!(
        pak.read_entry(&mut Cursor::new(truncated), &pak.entries()[0]),
        Err(PakError::Io(std::io::ErrorKind::UnexpectedEof))
    );
}

#[test]
fn errors_display_after_the_path() {
    let cases = [
        (PakError::TooManyFiles { count: 3000 }, "has 3000 files"),
        (
            PakError::DirectoryTooLarge { dirlen: 131_073 },
            "has a 131073-byte directory, more than 2048 entries can fill",
        ),
        (
            PakError::DirectoryOutOfBounds {
                dirofs: -1,
                dirlen: 64,
            },
            "has a 64-byte directory at offset -1, outside the file",
        ),
        (
            PakError::UnterminatedName { index: 3 },
            "has an entry (3) whose name has no NUL",
        ),
        (
            PakError::EntryOutOfBounds {
                filepos: 5,
                filelen: 9,
            },
            "has an entry of 9 bytes at offset 5, outside the file",
        ),
    ];
    for (error, message) in cases {
        assert_eq!(error.to_string(), message);
    }
}

// Adversarial reviewer tests (P1-R0, TEST-9): added by the independent readiness review of
// the L0 formats layer, probing extreme values and the order of the directory checks. They
// passed on first run, so they are kept as regression tests.

// spec PAK-2.12, PAK-3.4: an entry whose `filepos` and `filelen` are both i32::MAX is still
// listed, but reading it is an error and never overflows the 64-bit range arithmetic.
#[test]
fn reviewer_extreme_entry_range_is_listed_but_unreadable() {
    let mut bytes = PakSpec {
        entries: vec![entry("a", b"x")],
        ..PakSpec::default()
    }
    .build();
    // The single entry's 64-byte directory record begins at dirofs; patch its filepos and
    // filelen (the last two little-endian i32 of the record) to i32::MAX.
    let dir_record = bytes.len() - PAK_ENTRY_SIZE;
    patch(&mut bytes, dir_record + PAK_NAME_SIZE, i32::MAX);
    patch(&mut bytes, dir_record + PAK_NAME_SIZE + 4, i32::MAX);
    let pak = load(&bytes).unwrap();
    // Listed with its declared, impossible range (PAK-2.12).
    assert_eq!(pak.entries().len(), 1);
    assert_eq!(pak.entries()[0].filepos, i32::MAX);
    assert_eq!(pak.entries()[0].filelen, i32::MAX);
    // Reading it is an error, not a panic or an overflow (PAK-3.4); i32::MAX + i32::MAX fits
    // in u64, so the end is well-defined and simply past the end of the file.
    assert_eq!(
        contents(&bytes, &pak),
        vec![Err(PakError::EntryOutOfBounds {
            filepos: i32::MAX,
            filelen: i32::MAX,
        })]
    );
}

// spec PAK-2.4, PAK-2.6, PAK-2.7: the checks run in id's `COM_LoadPackFile` order (count,
// then directory size, then bounds), so a directory of exactly 2048 whole entries
// (dirlen 131072) placed one byte past the end of the file fails as out of bounds, not as
// too large: it passes the count and size checks and only the bounds check rejects it.
#[test]
fn reviewer_full_directory_past_eof_fails_out_of_bounds() {
    let spec = PakSpec {
        entries: empty_entries(2048),
        ..PakSpec::default()
    };
    let mut bytes = spec.build();
    let dirlen = i32::try_from(2048 * PAK_ENTRY_SIZE).unwrap();
    assert_eq!(dirlen, 131_072);
    // Move the directory one byte past where it was written, so dirofs + dirlen exceeds the
    // file by one byte while dirlen itself stays at the exact 131072 limit.
    let dirofs = i32::try_from(bytes.len() - 2048 * PAK_ENTRY_SIZE).unwrap();
    patch(&mut bytes, 4, dirofs + 1);
    assert_eq!(
        load(&bytes),
        Err(PakError::DirectoryOutOfBounds {
            dirofs: dirofs + 1,
            dirlen,
        })
    );
    // The same archive with the directory in place loads: dirlen 131072 is at the limit.
    assert!(load(&spec.build()).is_ok());
}

fn entry_specs() -> impl Strategy<Value = Vec<EntrySpec>> {
    prop::collection::vec(
        (
            prop::collection::vec(1_u8..=255, 0..PAK_NAME_SIZE),
            prop::collection::vec(any::<u8>(), 0..64),
        )
            .prop_map(|(name, data)| EntrySpec { name, data }),
        0..24,
    )
}

proptest! {
    // spec PAK-1.2: arbitrary bytes never panic either loader, and both give the same answer.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        if let Ok(pak) = load(&bytes) {
            contents(&bytes, &pak);
        }
    }

    // spec PAK-2.1: a header followed by arbitrary bytes never panics either loader.
    #[test]
    fn arbitrary_after_magic_never_panics(
        rest in prop::collection::vec(any::<u8>(), 0..512),
        dirofs in -16_i32..600,
        dirlen in prop_oneof![-128_i32..600, Just(i32::MIN), Just(i32::MAX)],
    ) {
        let mut bytes = PAK_MAGIC.to_vec();
        bytes.extend_from_slice(&dirofs.to_le_bytes());
        bytes.extend_from_slice(&dirlen.to_le_bytes());
        bytes.extend_from_slice(&rest);
        if let Ok(pak) = load(&bytes) {
            contents(&bytes, &pak);
        }
    }

    // spec PAK-3.2: every built entry reads back with its name and data, in order.
    #[test]
    fn built_archives_read_back(entries in entry_specs()) {
        let bytes = PakSpec { entries: entries.clone(), ..PakSpec::default() }.build();
        let pak = load(&bytes).unwrap();
        prop_assert_eq!(pak.entries().len(), entries.len());
        let all = contents(&bytes, &pak);
        for ((read, spec), data) in pak.entries().iter().zip(&entries).zip(all) {
            prop_assert_eq!(read.name(), spec.name.as_slice());
            prop_assert_eq!(data.unwrap(), spec.data.clone());
        }
    }
}
