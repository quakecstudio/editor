// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of the zip reader (FS-5.5), on archives written here.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    reason = "test code: a failed unwrap, index or cast is a failed test"
)]

use std::io::Cursor;

use miniz_oxide::deflate::compress_to_vec;

use super::{Zip, ZipError};

/// A zip of `files`, each stored or, with `deflate`, deflated.
fn zip(files: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let (method, body) = if deflate {
            (8_u16, compress_to_vec(data, 6))
        } else {
            (0, data.to_vec())
        };
        let offset = out.len() as u32;
        let mut fixed = Vec::new();
        fixed.extend_from_slice(&20_u16.to_le_bytes());
        fixed.extend_from_slice(&0_u16.to_le_bytes());
        fixed.extend_from_slice(&method.to_le_bytes());
        fixed.extend_from_slice(&[0; 8]);
        fixed.extend_from_slice(&(body.len() as u32).to_le_bytes());
        fixed.extend_from_slice(&(data.len() as u32).to_le_bytes());
        fixed.extend_from_slice(&(name.len() as u16).to_le_bytes());
        fixed.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
        out.extend_from_slice(&fixed);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&body);
        central.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
        central.extend_from_slice(&20_u16.to_le_bytes());
        central.extend_from_slice(&fixed);
        central.extend_from_slice(&[0; 10]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50_u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out
}

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
    for (at, len) in [(eocd + 10, 2), (eocd + 12, 4)] {
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
