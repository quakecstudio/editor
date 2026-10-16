// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary bytes to the PAK reader (PAK-1 to PAK-3), in memory and through a seekable
//! file. Fails on a panic, on the two loaders disagreeing, and on an entry that reads
//! differently the two ways.

#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use qs_pak::Pak;

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
});
