// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of the archive readers, and the archive writers the other corpus tests build their
//! fixtures with.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "test code: a failed unwrap, index, sum or cast is a failed test"
)]

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

use miniz_oxide::deflate::compress_to_vec;

use super::{Gunzip, Zip, read_tar_gz};
use crate::test_dirs::fresh;

/// A tar header for `name`, `size` bytes of type `kind`, with a valid checksum.
fn header(name: &str, prefix: &str, size: usize, kind: u8) -> [u8; 512] {
    let mut h = [0_u8; 512];
    h[..name.len()].copy_from_slice(name.as_bytes());
    h[100..108].copy_from_slice(b"0000644\0");
    h[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
    h[156] = kind;
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    h[148..156].copy_from_slice(b"        ");
    let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
    h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    h
}

fn push_entry(out: &mut Vec<u8>, header: [u8; 512], data: &[u8]) {
    out.extend_from_slice(&header);
    out.extend_from_slice(data);
    out.resize(out.len().div_ceil(512) * 512, 0);
}

/// A tar of regular files, as `git archive` writes one: a pax global header first, and a pax
/// `path` record before any name too long for the header.
pub(crate) fn tar(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let comment = b"52 comment=cccccccccccccccccccccccccccccccccccccccc\n";
    push_entry(
        &mut out,
        header("pax_global_header", "", comment.len(), b'g'),
        comment,
    );
    for (name, data) in files {
        if name.len() > 100 {
            // A pax record's length counts its own digits.
            let record = format!(" path={name}\n");
            let mut len = record.len();
            while len != record.len() + len.to_string().len() {
                len = record.len() + len.to_string().len();
            }
            let record = format!("{len}{record}");
            push_entry(
                &mut out,
                header("pax", "", record.len(), b'x'),
                record.as_bytes(),
            );
            push_entry(&mut out, header("long", "", data.len(), b'0'), data);
        } else {
            push_entry(&mut out, header(name, "", data.len(), b'0'), data);
        }
    }
    out.extend_from_slice(&[0; 1024]);
    out
}

/// `data` as a gzip stream with no optional fields.
pub(crate) fn gzip(data: &[u8]) -> Vec<u8> {
    gzip_with(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff], data)
}

fn gzip_with(head: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = head.to_vec();
    out.extend_from_slice(&compress_to_vec(data, 6));
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

pub(crate) fn tar_gz(files: &[(&str, &[u8])]) -> Vec<u8> {
    gzip(&tar(files))
}

/// A zip of `files`, deflated when `deflate`, else stored.
pub(crate) fn zip(files: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
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

fn read_all(bytes: &[u8]) -> std::io::Result<Vec<(String, Vec<u8>)>> {
    read_tar_gz(Cursor::new(bytes.to_vec()), &|_| true)
}

#[test]
fn a_tar_gz_gives_back_its_regular_files_in_order() {
    let long = format!("r-c/{}/defs.qc", "d".repeat(120));
    let archive = tar_gz(&[
        ("r-c/a.qc", b"one"),
        (&long, b"two"),
        ("r-c/b.src", &[7; 1000]),
    ]);
    let files = read_all(&archive).unwrap();
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["r-c/a.qc", long.as_str(), "r-c/b.src"]);
    assert_eq!(files[1].1, b"two");
    assert_eq!(files[2].1, vec![7; 1000]);
}

#[test]
fn only_files_the_filter_keeps_are_read() {
    let archive = tar_gz(&[
        ("r-c/qcc/qcc.c", b"compiler"),
        ("r-c/qcc/v101qc/defs.qc", b"qc"),
    ]);
    let keep = |name: &str| name.starts_with("r-c/qcc/v101qc/");
    let files = read_tar_gz(Cursor::new(archive), &keep).unwrap();
    assert_eq!(
        files,
        [("r-c/qcc/v101qc/defs.qc".to_owned(), b"qc".to_vec())]
    );
}

#[test]
fn ustar_prefixes_gnu_long_names_and_other_types_are_handled() {
    let mut t = Vec::new();
    push_entry(&mut t, header("defs.qc", "r-c/deep/dir", 2, b'0'), b"ok");
    push_entry(&mut t, header("r-c/dir/", "", 0, b'5'), b"");
    push_entry(&mut t, header("r-c/link", "", 0, b'2'), b"");
    let long = format!("r-c/{}.qc", "l".repeat(150));
    push_entry(
        &mut t,
        header("././@LongLink", "", long.len() + 1, b'L'),
        format!("{long}\0").as_bytes(),
    );
    push_entry(&mut t, header("trunc", "", 3, 0), b"gnu");
    t.extend_from_slice(&[0; 1024]);
    let files = read_all(&gzip(&t)).unwrap();
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["r-c/deep/dir/defs.qc", long.as_str()]);
}

#[test]
fn gzip_optional_header_fields_are_skipped() {
    let tar = tar(&[("r-c/a.qc", b"x")]);
    // FHCRC, FEXTRA, FNAME and FCOMMENT.
    let mut head = vec![0x1f, 0x8b, 8, 0b1_1110, 0, 0, 0, 0, 0, 0xff];
    head.extend_from_slice(&[3, 0, 1, 2, 3]);
    head.extend_from_slice(b"name.tar\0comment\0");
    head.extend_from_slice(&[0xab, 0xcd]);
    let files = read_all(&gzip_with(&head, &tar)).unwrap();
    assert_eq!(files.len(), 1);
}

#[test]
fn malformed_gzip_streams_are_errors() {
    let good = tar_gz(&[("r-c/a.qc", b"x")]);
    let err = |bytes: &[u8]| read_all(bytes).unwrap_err().to_string();
    assert!(err(b"PK\x03\x04 not gzip").contains("not a gzip stream"));
    assert!(read_all(&good[..5]).is_err());
    assert!(err(&good[..good.len() - 12]).contains("truncated"));
    let mut wrong_len = good.clone();
    let n = wrong_len.len();
    wrong_len[n - 4] ^= 1;
    assert!(err(&wrong_len).contains("does not match"));
    let mut no_trailer = good.clone();
    no_trailer.truncate(n - 6);
    assert!(err(&no_trailer).contains("truncated gzip trailer"));
    let mut corrupt = good[..10].to_vec();
    corrupt.extend_from_slice(&[0xff; 64]);
    assert!(err(&corrupt).contains("corrupt deflate data"));
}

#[test]
fn malformed_tars_are_errors() {
    let err = |tar: &[u8]| read_all(&gzip(tar)).unwrap_err().to_string();
    let mut bad_sum = Vec::new();
    let mut h = header("r-c/a.qc", "", 1, b'0');
    h[0] = b'R';
    push_entry(&mut bad_sum, h, b"x");
    assert!(err(&bad_sum).contains("checksum"));

    let mut not_octal = header("r-c/a.qc", "", 1, b'0');
    not_octal[124] = b'9';
    not_octal[148..156].copy_from_slice(b"        ");
    let sum: u32 = not_octal.iter().map(|&b| u32::from(b)).sum();
    not_octal[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    let mut t = Vec::new();
    push_entry(&mut t, not_octal, b"x");
    assert!(err(&t).contains("not octal"));

    let mut short = Vec::new();
    short.extend_from_slice(&header("r-c/a.qc", "", 4000, b'0'));
    short.extend_from_slice(&[1; 100]);
    assert!(err(&short).contains("truncated tar entry"));
    let mut skipped = Vec::new();
    skipped.extend_from_slice(&header("r-c/a.qc", "", 4000, b'5'));
    assert!(err(&skipped).contains("truncated tar entry"));

    assert!(err(&[0; 100]).contains("truncated tar header"));
    let mut no_end = Vec::new();
    push_entry(&mut no_end, header("r-c/a.qc", "", 1, b'0'), b"x");
    assert!(err(&no_end).contains("no end-of-archive block"));
}

fn temp(name: &str, bytes: &[u8]) -> PathBuf {
    let path = fresh(&format!("corpus-archive/{name}")).join(name);
    fs::write(&path, bytes).unwrap();
    path
}

fn open(name: &str, bytes: &[u8]) -> std::io::Result<Zip> {
    Zip::open(fs::File::open(temp(name, bytes))?)
}

#[test]
fn zip_members_read_stored_and_deflated() {
    for deflate in [false, true] {
        let bytes = zip(
            &[("lite/id1/pak0.pak", b"PACK data"), ("lite/x", &[5; 3000])],
            deflate,
        );
        let mut z = open(&format!("ok-{deflate}.zip"), &bytes).unwrap();
        assert_eq!(z.read("lite/id1/pak0.pak").unwrap().unwrap(), b"PACK data");
        assert_eq!(z.read("lite/x").unwrap().unwrap(), vec![5; 3000]);
        assert!(z.read("lite/nosuch").unwrap().is_none());
    }
}

#[test]
fn malformed_zips_are_errors() {
    let err = |name: &str, bytes: &[u8]| open(name, bytes).err().unwrap().to_string();
    assert!(err("not.zip", b"not a zip").contains("no end of central directory"));

    let good = zip(&[("a", b"abc")], false);
    let eocd = good.len() - 22;
    let mut zip64 = good.clone();
    zip64[eocd + 16..eocd + 20].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(err("zip64.zip", &zip64).contains("zip64"));
    let mut outside = good.clone();
    outside[eocd + 16..eocd + 20].copy_from_slice(&1000_u32.to_le_bytes());
    assert!(err("outside.zip", &outside).contains("outside the file"));
    let mut bad_cd = good.clone();
    let cd = u32::from_le_bytes(good[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
    bad_cd[cd] = b'X';
    assert!(err("badcd.zip", &bad_cd).contains("central directory"));
    let mut member64 = good.clone();
    member64[cd + 20..cd + 24].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(err("member64.zip", &member64).contains("zip64"));

    let read_err = |name: &str, bytes: &[u8]| {
        open(name, bytes)
            .unwrap()
            .read("a")
            .unwrap_err()
            .to_string()
    };
    let mut encrypted = good.clone();
    encrypted[cd + 8] = 1;
    assert!(read_err("enc.zip", &encrypted).contains("encrypted"));
    let mut method = good.clone();
    method[cd + 10] = 12;
    assert!(read_err("method.zip", &method).contains("unsupported compression"));
    let mut local = good.clone();
    local[0] = b'X';
    assert!(read_err("local.zip", &local).contains("local header"));
    let mut size = good.clone();
    size[cd + 24] = 4;
    assert!(read_err("size.zip", &size).contains("stated size"));

    let deflated = zip(&[("a", &[9; 500])], true);
    let eocd = deflated.len() - 22;
    let cd = u32::from_le_bytes(deflated[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
    let mut small = deflated.clone();
    small[cd + 24..cd + 28].copy_from_slice(&10_u32.to_le_bytes());
    assert!(read_err("small.zip", &small).contains("larger than its stated size"));
    let mut corrupt = deflated.clone();
    corrupt[34] = 0xff;
    corrupt[35] = 0xff;
    assert!(read_err("corrupt.zip", &corrupt).contains("deflate"));
}

/// A reader that gives one byte per call, as a slow network or pipe may.
struct Trickle(Cursor<Vec<u8>>);

impl std::io::Read for Trickle {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let one = out.len().min(1);
        self.0.read(&mut out[..one])
    }
}

/// What a [`Gunzip`] gives through `out`-sized reads, and the error that stopped it, if any.
fn gunzip_pieces(input: impl std::io::Read, out: usize) -> (Vec<u8>, Option<String>) {
    use std::io::Read;
    let mut gunzip = Gunzip::new(input).unwrap();
    let mut data = Vec::new();
    let mut buffer = vec![0; out];
    loop {
        match gunzip.read(&mut buffer) {
            Ok(0) => return (data, None),
            Ok(n) => data.extend_from_slice(&buffer[..n]),
            Err(error) => return (data, Some(error.to_string())),
        }
    }
}

/// Everything a [`Gunzip`] gives through `out`-sized reads, or its error.
fn gunzip_in_pieces(input: impl std::io::Read, out: usize) -> Result<Vec<u8>, String> {
    match gunzip_pieces(input, out) {
        (data, None) => Ok(data),
        (_, Some(error)) => Err(error),
    }
}

#[test]
fn gzip_read_a_byte_at_a_time_inflates_whole() {
    let data: Vec<u8> = (0..5000_u32).map(|i| (i * 7 % 251) as u8).collect();
    let packed = gzip(&data);
    for out in [1, 7, 4096] {
        assert_eq!(
            gunzip_in_pieces(Trickle(Cursor::new(packed.clone())), out).unwrap(),
            data
        );
        assert_eq!(
            gunzip_in_pieces(Cursor::new(packed.clone()), out).unwrap(),
            data
        );
    }
    let cut = packed[..packed.len() - 20].to_vec();
    for out in [1, 4096] {
        let error = gunzip_in_pieces(Trickle(Cursor::new(cut.clone())), out).unwrap_err();
        assert!(error.contains("truncated"), "{error}");
    }
    // A truncated stream still gives every byte its input holds before the error.
    let (one, _) = gunzip_pieces(Trickle(Cursor::new(cut.clone())), 1);
    let (whole, _) = gunzip_pieces(Trickle(Cursor::new(cut)), 4096);
    assert!(
        data.starts_with(&one) && one.len() >= whole.len(),
        "{} {}",
        one.len(),
        whole.len()
    );
}
