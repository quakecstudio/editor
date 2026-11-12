// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use super::Compiler;
use crate::Dialect;

#[test]
fn scratch_maps() {
    let Ok(dir) = std::env::var("QS_MAPS") else {
        return;
    };
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "map") {
            continue;
        }
        let map = std::fs::read(&path).unwrap();
        let stem = path.with_extension("");
        match Compiler::default()
            .with_dialect(Dialect::Extended)
            .compile(&map, None)
        {
            Ok(compiled) => {
                std::fs::write(format!("{}.ext.bsp", stem.display()), &compiled.bsp).unwrap();
            }
            Err(error) => {
                std::fs::write(format!("{}.ext.err", stem.display()), error.to_string()).unwrap();
            }
        }
        match Compiler::default()
            .with_dialect(Dialect::Id)
            .compile(&map, None)
        {
            Ok(compiled) => {
                std::fs::write(format!("{}.ours.bsp", stem.display()), &compiled.bsp).unwrap();
                if let Some(prt) = compiled.portal_file {
                    std::fs::write(format!("{}.ours.prt", stem.display()), prt).unwrap();
                }
            }
            Err(error) => {
                std::fs::write(format!("{}.ours.err", stem.display()), error.to_string()).unwrap();
            }
        }
    }
}
