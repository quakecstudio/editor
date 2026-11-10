// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary bytes to the PAK reader (PAK-1 to PAK-3), in memory and through a seekable
//! file, and the files an archive read holds back to the writer (PAK-6). Fails on a panic, on
//! the two loaders disagreeing, on an entry that reads differently the two ways, and on a
//! written archive that does not read back as the files it was given.

#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use qs_pak::{Pak, write_pak};

fuzz_target!(|data: &[u8]| {
    let in_memory = Pak::read(data);
    let from_file = Pak::read_from(&mut Cursor::new(data));
    assert_eq!(in_memory, from_file);
    let Ok(pak) = in_memory else {
        return;
    };
    for entry in pak.entries() {
        let direct = pak.entry_data(data, entry).map(<[u8]>::to_vec);
        let seeked = pak.read_entry(&mut Cursor::new(data), entry);
        assert_eq!(direct, seeked);
        if let Some(found) = pak.find(entry.name()) {
            assert_eq!(found.name(), entry.name());
        }
    }
    // PAK-6: the entries read, written again, read back as they were given, or are refused.
    let files: Vec<(Vec<u8>, Vec<u8>)> = pak
        .entries()
        .iter()
        .filter_map(|e| Some((e.name().to_vec(), pak.entry_data(data, e).ok()?.to_vec())))
        .collect();
    if let Ok(written) = write_pak(&files) {
        let again = Pak::read(&written).expect("a written archive reads");
        let mut sorted = files;
        sorted.sort();
        let read: Vec<(Vec<u8>, Vec<u8>)> = again
            .entries()
            .iter()
            .map(|e| (e.name().to_vec(), again.entry_data(&written, e).expect("in bounds").to_vec()))
            .collect();
        assert_eq!(read, sorted);
    }
});
