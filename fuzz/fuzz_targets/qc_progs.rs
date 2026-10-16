// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary bytes to the progs.dat reader (PROGS-15) and writes back what it accepts
//! (PROGS-22). Fails on a panic, on a file the reader accepts but the writer cannot write, on
//! a written file the reader rejects, and on a round trip that changes a byte or a record.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qc_progs::{Progs, ReadOptions, WriteError, write};

fuzz_target!(|data: &[u8]| {
    for check_header_crc in [true, false] {
        let options = ReadOptions { check_header_crc };
        let Ok(progs) = Progs::read_with(data, &options) else {
            continue;
        };
        let file = progs.to_file();
        let written = match write(&file) {
            Ok(written) => written,
            // FTE's opaque tables cannot be moved when the source layout is not kept.
            Err(WriteError::OpaqueTableWithoutLayout) if file.layout.is_none() => continue,
            Err(error) => panic!("an accepted file could not be written: {error}"),
        };
        if file.layout.is_some() {
            assert_eq!(written, data, "a file with a layout must write back unchanged");
        }
        let reread = match Progs::read_with(&written, &options) {
            Ok(reread) => reread.to_file(),
            Err(error) => panic!("a written file was rejected: {error}"),
        };
        assert_eq!(reread.statements, file.statements);
        assert_eq!(reread.global_defs, file.global_defs);
        assert_eq!(reread.field_defs, file.field_defs);
        assert_eq!(reread.functions, file.functions);
        assert_eq!(reread.globals, file.globals);
        assert_eq!(reread.fte, file.fte);
        assert!(reread.strings.starts_with(&file.strings));
    }
});
