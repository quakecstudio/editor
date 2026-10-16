// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask corpus build`: compiles the codebases an oracle compiler builds and checks
//! that each progs.dat round-trips through `qc-progs` byte for byte (PROGS-22.1).
//!
//! id's QuakeC 1.01 is compiled with id's qcc and the six rerelease codebases with the pinned
//! FTEQCC, exactly as `cargo xtask oracles id101` and `oracles rerelease` do; the outputs go
//! to `<cache>/progs/<id>/`. This is where those progs.dat files first exist, so the
//! round-trip is checked here.

use std::fs;
use std::path::Path;

use qc_progs::{Progs, write};

use super::fetch::{write_codebase, write_file};
use super::manifest::Manifest;
use crate::download::Net;
use crate::oracles::{self, Oracles, id101, rerelease};

/// The codebase id's qcc compiles.
const ID_QCC: &str = "id-1.01";

/// Whether an oracle compiler builds codebase `id`.
fn has_oracle(id: &str) -> bool {
    id == ID_QCC || rerelease::compiles(id)
}

/// The codebases `ids` names, or when it names none, every usable one an oracle compiler builds.
fn targets(m: &Manifest, ids: &[String]) -> Vec<String> {
    if !ids.is_empty() {
        return ids.to_vec();
    }
    m.codebase
        .iter()
        .filter(|e| e.usable() && has_oracle(&e.id))
        .map(|e| e.id.clone())
        .collect()
}

/// Builds the codebases in `ids`, or every one an oracle compiler builds when `ids` is empty.
pub(crate) fn cmd_build(
    root: &Path,
    m: &Manifest,
    dest: &Path,
    ids: &[String],
    net: &dyn Net,
) -> Vec<String> {
    let ids = targets(m, ids);
    let mut errs: Vec<String> = ids
        .iter()
        .filter(|id| !has_oracle(id))
        .map(|id| format!("codebase {id}: no oracle compiler builds it"))
        .collect();
    if !errs.is_empty() {
        return errs;
    }
    let oracles = match oracles::build(root) {
        Ok(oracles) => oracles,
        Err(e) => return vec![e],
    };
    for id in &ids {
        match build_one(&oracles, m, dest, id, net) {
            Ok(len) => println!(
                "ok    {id}: progs.dat ({len} bytes) round-trips through qc-progs byte for byte"
            ),
            Err(e) => errs.push(format!("codebase {id}: {e}")),
        }
    }
    errs
}

/// Fetches, compiles and checks one codebase; returns the size of its progs.dat.
fn build_one(
    oracles: &Oracles,
    m: &Manifest,
    dest: &Path,
    id: &str,
    net: &dyn Net,
) -> Result<usize, String> {
    let (dir, _) = write_codebase(m, id, dest, net)?;
    let out = dest.join("progs").join(id);
    let progs = if id == ID_QCC {
        id101::compile(oracles, &dir)?.0
    } else {
        let codebases = dir.parent().unwrap_or(dest);
        let (built, _) = rerelease::build(oracles, codebases, id)?;
        let lno_path = built.join("progs.lno");
        let lno = fs::read(&lno_path).map_err(|e| format!("{}: {e}", lno_path.display()))?;
        write_file(&out.join("progs.lno"), &lno)?;
        let progs_path = built.join("progs.dat");
        fs::read(&progs_path).map_err(|e| format!("{}: {e}", progs_path.display()))?
    };
    write_file(&out.join("progs.dat"), &progs)?;
    round_trip(&progs)?;
    Ok(progs.len())
}

/// Reads a progs.dat with `qc-progs` and writes it back; the bytes must be the same
/// (PROGS-22.1).
pub(crate) fn round_trip(progs: &[u8]) -> Result<(), String> {
    let read = Progs::read(progs).map_err(|e| format!("qc-progs refuses it: {e:?}"))?;
    let written = write(&read.to_file()).map_err(|e| format!("qc-progs cannot write it: {e:?}"))?;
    if written == progs {
        return Ok(());
    }
    let first = progs
        .iter()
        .zip(&written)
        .position(|(a, b)| a != b)
        .unwrap_or(progs.len().min(written.len()));
    Err(format!(
        "written back it is {} bytes against {}, differing first at byte {first}",
        written.len(),
        progs.len()
    ))
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use qc_progs::{Def, ProgsFile, Statement, write};

    use std::path::Path;

    use std::process::Command;

    use super::super::fetch::tests::{
        DEFS, FakeNet, TempDir, archive_url, manifest, standard_archive,
    };
    use super::super::manifest::Manifest;
    use super::{build_one, cmd_build, has_oracle, round_trip, targets};
    use crate::exec::{Fake, Reply};
    use crate::oracles::sha256::hex_digest;
    use crate::oracles::testing::fake_oracles;

    #[test]
    fn only_codebases_an_oracle_compiles_are_built() {
        assert!(has_oracle("id-1.01"));
        assert!(has_oracle("rerelease-mg3"));
        assert!(!has_oracle("copper"));
        assert!(!has_oracle("quakec-1.06"));
    }

    #[test]
    fn with_no_ids_every_usable_codebase_an_oracle_builds_is_built() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let manifest = Manifest::load(&root.join(super::super::MANIFEST)).unwrap();
        let all = targets(&manifest, &[]);
        assert!(all.contains(&"id-1.01".to_owned()), "{all:?}");
        for id in &all {
            assert!(has_oracle(id), "{id}");
            let entry = manifest.codebase.iter().find(|e| &e.id == id).unwrap();
            assert!(entry.usable(), "{id}");
        }
        let named = ["copper".to_owned()];
        assert_eq!(targets(&manifest, &named), named);
    }

    #[test]
    fn id_1_01_is_written_out_compiled_by_qcc_and_kept() {
        let progs = write(&ProgsFile {
            statements: vec![Statement::default(); 3],
            strings: vec![0],
            ..ProgsFile::default()
        })
        .unwrap();
        let mut m = manifest(&hex_digest(DEFS));
        m.codebase[0].id = "id-1.01".to_owned();
        let net = FakeNet::serving(&archive_url(), &standard_archive());
        let written = progs.clone();
        let qcc = Fake::new(move |command: &Command| Reply {
            files: vec![(
                command
                    .get_current_dir()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("progs.dat"),
                written.clone(),
            )],
            ..Reply::default()
        });
        let tmp = TempDir::new();
        let oracles = fake_oracles(&tmp.0.join("work"), &qcc);
        let dest = tmp.0.join("cache");
        assert_eq!(
            build_one(&oracles, &m, &dest, "id-1.01", &net),
            Ok(progs.len())
        );
        let kept = std::fs::read(dest.join("progs/id-1.01/progs.dat")).unwrap();
        assert_eq!(kept, progs);
        assert_eq!(qcc.seen.borrow().as_slice(), ["qcc"]);
    }

    #[test]
    fn a_codebase_no_oracle_builds_is_refused_before_any_build() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let manifest = Manifest::load(&root.join(super::super::MANIFEST)).unwrap();
        let ids = ["copper".to_owned(), "quakec-1.06".to_owned()];
        let dest = root.join("target/xtask-tests/corpus-build");
        assert_eq!(
            cmd_build(root, &manifest, &dest, &ids, &FakeNet::default()),
            [
                "codebase copper: no oracle compiler builds it",
                "codebase quakec-1.06: no oracle compiler builds it",
            ]
        );
    }

    // spec PROGS-22.1: a file the reader accepts is written back byte for byte.
    #[test]
    fn a_progs_written_by_qc_progs_round_trips() {
        let file = ProgsFile {
            statements: vec![Statement::default(); 3],
            strings: vec![0],
            ..ProgsFile::default()
        };
        let bytes = write(&file).unwrap();
        assert_eq!(round_trip(&bytes), Ok(()));
    }

    #[test]
    fn a_file_qc_progs_refuses_or_changes_is_reported() {
        let err = round_trip(b"not a progs").unwrap_err();
        assert!(err.starts_with("qc-progs refuses it"), "{err}");
        // spec PROGS-22.2: a file whose tables overlap is written back in qcc's layout.
        let file = ProgsFile {
            statements: vec![Statement::default(); 3],
            global_defs: vec![Def::default()],
            field_defs: vec![Def::default()],
            strings: vec![0],
            ..ProgsFile::default()
        };
        let mut bytes = write(&file).unwrap();
        let globaldefs: [u8; 4] = bytes[16..20].try_into().unwrap();
        bytes[24..28].copy_from_slice(&globaldefs);
        let err = round_trip(&bytes).unwrap_err();
        assert!(err.contains("differing first at byte 24"), "{err}");
        assert!(err.contains(&format!("against {}", bytes.len())), "{err}");
    }
}
