// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask corpus build`: compiles the codebases an oracle compiler builds and checks
//! that each progs.dat round-trips through `qc-progs` byte for byte (PROGS-22.1), and that
//! each FTEQCC-written `.lno` round-trips through `qc-lno` byte for byte (LNO-6.2) and matches
//! its progs.dat header counts (LNO-4.1).
//!
//! id's QuakeC 1.01 is compiled with id's qcc and the six rerelease codebases with the pinned
//! FTEQCC, exactly as `cargo xtask oracles id101` and `oracles rerelease` do; the outputs go
//! to `<cache>/progs/<id>/`. This is where those progs.dat files first exist, so the
//! round-trip is checked here. id's qcc writes no `.lno` (LNO-1), so only the FTEQCC-written
//! codebases carry one and are checked against it.

use std::fs;
use std::path::Path;

use qc_lno::LnoFile;
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
    let (progs, lno) = if id == ID_QCC {
        (id101::compile(oracles, &dir)?.0, None)
    } else {
        let codebases = dir.parent().unwrap_or(dest);
        let (built, _) = rerelease::build(oracles, codebases, id)?;
        let lno_path = built.join("progs.lno");
        let lno = fs::read(&lno_path).map_err(|e| format!("{}: {e}", lno_path.display()))?;
        write_file(&out.join("progs.lno"), &lno)?;
        let progs_path = built.join("progs.dat");
        let progs = fs::read(&progs_path).map_err(|e| format!("{}: {e}", progs_path.display()))?;
        (progs, Some(lno))
    };
    write_file(&out.join("progs.dat"), &progs)?;
    round_trip(&progs)?;
    if let Some(lno) = lno {
        lno_round_trip(&lno, &progs)?;
        let bytes = lno.len();
        println!("ok    {id}: progs.lno ({bytes} bytes) round-trips through qc-lno, counts match");
    }
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

/// Reads an FTEQCC-written `.lno` with `qc-lno`, writes it back and checks the bytes are
/// identical (LNO-6.2: "A file the reader accepts is written back byte for byte, and reading
/// what the writer wrote gives back the counts, tables and trailing bytes it was given."),
/// then checks it matches its `progs.dat` header counts (LNO-4.1: "A `.lno` belongs to a progs
/// when all four counts equal that progs' header fields `numglobaldefs`, `numglobals`,
/// `numfielddefs` and `numstatements` (PROGS-2.1), compared as unsigned 32-bit values.").
///
/// Reports the first differing byte on a round-trip mismatch and the first differing count on
/// a header mismatch; the caller prefixes the codebase id.
pub(crate) fn lno_round_trip(lno: &[u8], progs: &[u8]) -> Result<(), String> {
    let read = LnoFile::read(lno).map_err(|e| format!("qc-lno refuses the .lno: {e}"))?;
    let written = read
        .write()
        .map_err(|e| format!("qc-lno cannot write the .lno back: {e}"))?;
    if written != lno {
        let first = lno
            .iter()
            .zip(&written)
            .position(|(a, b)| a != b)
            .unwrap_or(lno.len().min(written.len()));
        return Err(format!(
            "the .lno written back is {} bytes against {}, differing first at byte {first}",
            written.len(),
            lno.len()
        ));
    }
    let progs = Progs::read(progs).map_err(|e| format!("qc-progs refuses the progs.dat: {e:?}"))?;
    read.matches_header(progs.header())
        .map_err(|e| format!("the .lno does not match the progs.dat: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use qc_lno::LnoFile;
    use qc_progs::{Def, ProgsFile, Statement, write};

    use std::path::Path;

    use std::process::Command;

    use super::super::fetch::tests::{
        DEFS, FakeNet, TempDir, archive_url, manifest, standard_archive,
    };
    use super::super::manifest::Manifest;
    use super::{build_one, cmd_build, has_oracle, lno_round_trip, round_trip, targets};
    use crate::exec::{Fake, Reply};
    use crate::oracles::sha256::hex_digest;
    use crate::oracles::testing::fake_oracles;
    use crate::test_dirs::fresh;

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
        assert_eq!(qcc.seen.lock().unwrap().as_slice(), ["qcc"]);
    }

    #[test]
    fn a_codebase_no_oracle_builds_is_refused_before_any_build() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let manifest = Manifest::load(&root.join(super::super::MANIFEST)).unwrap();
        let ids = ["copper".to_owned(), "quakec-1.06".to_owned()];
        let dest = fresh("corpus/build");
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

    /// A progs.dat whose header counts a matching `.lno` must share: `numglobaldefs` 1,
    /// `numglobals` 0, `numfielddefs` 1 and `numstatements` 2, from the table lengths.
    fn progs_with_two_statements() -> Vec<u8> {
        let file = ProgsFile {
            statements: vec![Statement::default(); 2],
            global_defs: vec![Def::default(); 1],
            field_defs: vec![Def::default(); 1],
            strings: vec![0],
            ..ProgsFile::default()
        };
        write(&file).unwrap()
    }

    // spec LNO-6.2: a file the reader accepts is written back byte for byte.
    // spec LNO-4.1: a .lno matches when its four counts equal the progs' header fields.
    #[test]
    fn an_lno_that_round_trips_and_matches_its_progs_passes() {
        let progs = progs_with_two_statements();
        let lno = LnoFile {
            numglobaldefs: 1,
            numglobals: 0,
            numfielddefs: 1,
            lines: vec![0, 5],
            columns: None,
            trailing: Vec::new(),
        }
        .write()
        .unwrap();
        assert_eq!(lno_round_trip(&lno, &progs), Ok(()));
    }

    // spec LNO-4.1: a .lno matches only when all four counts equal the progs' header fields.
    // spec LNO-4.2: the first differing field, in header order, is reported.
    #[test]
    fn an_lno_whose_counts_differ_fails_naming_the_field() {
        let progs = progs_with_two_statements();
        let lno = LnoFile {
            numglobaldefs: 1,
            numglobals: 7,
            numfielddefs: 1,
            lines: vec![0, 5],
            columns: None,
            trailing: Vec::new(),
        }
        .write()
        .unwrap();
        let err = lno_round_trip(&lno, &progs).unwrap_err();
        assert!(err.contains("does not match the progs.dat"), "{err}");
        assert!(err.contains("numglobals"), "{err}");
        assert!(err.contains("the .lno's numglobals is 7"), "{err}");
    }

    // spec LNO-4.1: numstatements is the line table length, checked against the progs.
    #[test]
    fn an_lno_with_the_wrong_statement_count_fails() {
        let progs = progs_with_two_statements();
        let lno = LnoFile {
            numglobaldefs: 1,
            numglobals: 0,
            numfielddefs: 1,
            lines: vec![0, 5, 9],
            columns: None,
            trailing: Vec::new(),
        }
        .write()
        .unwrap();
        let err = lno_round_trip(&lno, &progs).unwrap_err();
        assert!(err.contains("numstatements"), "{err}");
    }

    // spec LNO-5.1: a file shorter than the header is rejected with a typed error.
    #[test]
    fn an_lno_qc_lno_refuses_is_reported() {
        let progs = progs_with_two_statements();
        let err = lno_round_trip(b"short", &progs).unwrap_err();
        assert!(err.starts_with("qc-lno refuses the .lno"), "{err}");
    }
}
