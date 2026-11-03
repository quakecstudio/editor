// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of the zip reader (FS-5.5), on archives written here.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    reason = "test code: a failed unwrap, index or cast is a failed test"
)]

use std::io::Cursor;

use super::{Zip, ZipError};
use crate::fixtures::zip;

fn open(bytes: &[u8]) -> Result<Zip<Cursor<Vec<u8>>>, ZipError> {
    Zip::open(Cursor::new(bytes.to_vec()))
}

// spec FS-5.5: a member is read by its name, stored or deflated; another name is none
#[test]
fn members_read_stored_and_deflated() {
    for deflate in [false, true] {
        let bytes = zip(
            &[
                ("localization/loc_english.txt", b"k = v\n"),
                ("x", &[5; 3000]),
            ],
            deflate,
        );
        let mut z = open(&bytes).unwrap();
        assert_eq!(
            z.read("localization/loc_english.txt").unwrap().unwrap(),
            b"k = v\n"
        );
        assert_eq!(z.read("x").unwrap().unwrap(), vec![5; 3000]);
        assert!(z.read("nosuch").unwrap().is_none());
    }
}

// spec FS-5.5: an archive with a comment after its end record is found
#[test]
fn an_archive_comment_is_passed_over() {
    let mut bytes = zip(&[("a", b"abc")], false);
    let n = bytes.len();
    bytes[n - 2..].copy_from_slice(&5_u16.to_le_bytes());
    bytes.extend_from_slice(b"hello");
    assert_eq!(open(&bytes).unwrap().read("a").unwrap().unwrap(), b"abc");
}

// spec FS-5.5: zip64, an outside or truncated central directory, an encrypted member, another
// method, a bad local header, a wrong size and corrupt deflate data are errors
#[test]
fn malformed_zips_are_errors() {
    let err = |bytes: &[u8]| open(bytes).err().unwrap();
    assert!(matches!(err(b"not a zip"), ZipError::NotZip));

    let good = zip(&[("a", b"abc")], false);
    let eocd = good.len() - 22;
    let mut zip64 = good.clone();
    zip64[eocd + 16..eocd + 20].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(err(&zip64), ZipError::Zip64));
    // A zip64 archive marks both entry counts, or the directory's size.
    for (at, len) in [(eocd + 8, 4), (eocd + 12, 4)] {
        let mut zip64 = good.clone();
        zip64[at..at + len].fill(0xff);
        assert!(matches!(err(&zip64), ZipError::Zip64), "field at {at}");
    }
    // A central directory that ends at the file's end lies inside it, and is then misread.
    let mut at_end = good.clone();
    let cd_size = u32::from_le_bytes(good[eocd + 12..eocd + 16].try_into().unwrap());
    let ends = good.len() as u32 - cd_size;
    at_end[eocd + 16..eocd + 20].copy_from_slice(&ends.to_le_bytes());
    assert!(matches!(err(&at_end), ZipError::CentralTruncated));
    let mut outside = good.clone();
    outside[eocd + 16..eocd + 20].copy_from_slice(&1000_u32.to_le_bytes());
    assert!(matches!(err(&outside), ZipError::CentralOutside));
    let mut bad_cd = good.clone();
    let cd = u32::from_le_bytes(good[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
    bad_cd[cd] = b'X';
    assert!(matches!(err(&bad_cd), ZipError::CentralTruncated));
    let mut member64 = good.clone();
    member64[cd + 20..cd + 24].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(err(&member64), ZipError::Zip64));

    let read_err = |bytes: &[u8]| open(bytes).unwrap().read("a").unwrap_err();
    let mut encrypted = good.clone();
    encrypted[cd + 8] = 1;
    assert!(matches!(read_err(&encrypted), ZipError::Encrypted));
    let mut method = good.clone();
    method[cd + 10] = 12;
    assert!(matches!(read_err(&method), ZipError::Method(12)));
    let mut local = good.clone();
    local[0] = b'X';
    assert!(matches!(read_err(&local), ZipError::LocalHeader));
    let mut size = good.clone();
    size[cd + 24] = 4;
    assert!(matches!(read_err(&size), ZipError::Size));
    let mut short = good.clone();
    short[cd + 20] = 200;
    assert!(matches!(read_err(&short), ZipError::Truncated));

    let deflated = zip(&[("a", &[9; 500])], true);
    let eocd = deflated.len() - 22;
    let cd = u32::from_le_bytes(deflated[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
    let mut small = deflated.clone();
    small[cd + 24..cd + 28].copy_from_slice(&10_u32.to_le_bytes());
    assert!(matches!(read_err(&small), ZipError::Larger));
    let mut corrupt = deflated.clone();
    corrupt[34] = 0xff;
    corrupt[35] = 0xff;
    assert!(matches!(read_err(&corrupt), ZipError::Corrupt));
    assert!(!ZipError::Method(12).to_string().is_empty());
}

// spec FS-5.5: a source shorter than the end record, a multi-disk archive, a member whose bytes
// are not its CRC-32, strong encryption and patch data are errors, as miniz has them
#[test]
fn miniz_refusals_are_errors() {
    assert!(matches!(
        open(b"PK\x05\x06").err().unwrap(),
        ZipError::NotZip
    ));
    let good = zip(&[("a", b"abc")], false);
    let eocd = good.len() - 22;
    let mut disk = good.clone();
    disk[eocd + 4] = 2;
    assert!(matches!(open(&disk).err().unwrap(), ZipError::Multidisk));
    let mut both_one = good.clone();
    both_one[eocd + 4] = 1;
    both_one[eocd + 6] = 1;
    assert!(open(&both_one).is_ok(), "disk 1 of 1 is one disk");
    let mut entries = good.clone();
    entries[eocd + 8] = 2;
    assert!(matches!(open(&entries).err().unwrap(), ZipError::Multidisk));
    let mut small_cd = good.clone();
    small_cd[eocd + 12..eocd + 16].copy_from_slice(&10_u32.to_le_bytes());
    assert!(matches!(
        open(&small_cd).err().unwrap(),
        ZipError::CentralTruncated
    ));
    for (this, cd) in [(1, 0), (0, 1), (2, 2), (2, 0)] {
        let mut disks = good.clone();
        disks[eocd + 4] = this;
        disks[eocd + 6] = cd;
        assert!(
            matches!(open(&disks).err(), Some(ZipError::Multidisk)),
            "disks {this} and {cd}"
        );
    }
    // An archive of no members is the end record alone, 22 bytes; a member of an empty name
    // is a record of 46 bytes.
    let no_members = zip(&[], false);
    assert_eq!(no_members.len(), 22);
    assert!(open(&no_members).unwrap().read("a").unwrap().is_none());
    assert_eq!(
        open(&zip(&[("", b"x")], false))
            .unwrap()
            .read("")
            .unwrap()
            .unwrap(),
        b"x"
    );

    let cd = u32::from_le_bytes(good[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
    let read_err = |bytes: &[u8]| open(bytes).unwrap().read("a").unwrap_err();
    let mut crc = good.clone();
    crc[30 + 1] = b'X';
    assert!(matches!(read_err(&crc), ZipError::Crc));
    let mut strong = good.clone();
    strong[cd + 8] = 1 << 6;
    assert!(matches!(read_err(&strong), ZipError::Encrypted));
    let mut patch = good.clone();
    patch[cd + 8] = 1 << 5;
    assert!(matches!(read_err(&patch), ZipError::Patch));
}

// spec FS-5.5: a member's CRC-32 is zip's, the check value of "123456789" being 0xCBF43926
#[test]
fn the_crc_is_zips() {
    use super::crc32;
    assert_eq!(crc32(b""), 0);
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(
        crc32(b"The quick brown fox jumps over the lazy dog"),
        0x414F_A339
    );
}

// spec FS-5.5: a member of no compressed bytes reads as empty before any other check, and a name
// is matched exactly, else ignoring ASCII case, as miniz without MZ_ZIP_FLAG_CASE_SENSITIVE
#[test]
fn empty_members_and_names_of_another_case_are_read() {
    let empty = zip(&[("e", b"")], true);
    let mut z = open(&empty).unwrap();
    assert_eq!(z.read("e").unwrap().unwrap(), b"");
    let cased = zip(&[("Dir/File.TXT", b"one"), ("dir/file.txt", b"two")], false);
    let mut z = open(&cased).unwrap();
    assert_eq!(z.read("dir/file.txt").unwrap().unwrap(), b"two");
    assert_eq!(z.read("DIR/FILE.txt").unwrap().unwrap(), b"one");
    assert!(z.read("dir/other.txt").unwrap().is_none());
}

// spec FS-5.5: an error reading the source keeps the system's message
#[test]
fn a_read_error_keeps_its_message() {
    let error = ZipError::from(std::io::Error::other("disk on fire"));
    assert_eq!(error.to_string(), "disk on fire");
    assert!(matches!(
        error,
        ZipError::Io {
            kind: std::io::ErrorKind::Other,
            ..
        }
    ));
}

// spec FS-5.5: any bytes at all open and read as a zip or fail with an error, never a panic;
// any members written are read back
mod properties {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "test code: a failed unwrap, index or sum is a failed test"
    )]

    use proptest::prelude::*;

    use super::{open, zip};

    proptest! {
        #[test]
        fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            if let Ok(mut z) = open(&bytes) {
                drop(z.read("a"));
            }
        }

        #[test]
        fn written_members_read_back(
            data in proptest::collection::vec(any::<u8>(), 0..2048),
            deflate in any::<bool>(),
            damage in any::<(usize, u8)>(),
        ) {
            let bytes = zip(&[("m", &data)], deflate);
            prop_assert_eq!(open(&bytes).unwrap().read("m").unwrap().unwrap(), data.clone());
            // A damaged copy is read back, read as an error or not opened, never a panic.
            let mut damaged = bytes.clone();
            let at = damage.0 % damaged.len();
            damaged[at] ^= damage.1;
            if let Ok(mut z) = open(&damaged) {
                drop(z.read("m"));
            }
        }
    }
}
