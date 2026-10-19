// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask oracles rerelease <dir>`: compiles the six rerelease codebases with the pinned
//! FTEQCC and keeps each `progs.dat` and `progs.lno` as an oracle artifact (P1-26, LIC-8).
//!
//! `<dir>` holds one directory per codebase, named by its id in `corpus/manifest.toml`, as
//! `cargo xtask corpus fetch <id>` writes them. Each is compiled with
//! [`FLAGS`]: `-Tid` targets id's version 6 format, where FTEQCC refuses any opcode id's VM
//! lacks, and `-flno` asks for the `.lno` file (`docs/spec/lno.md` LNO-1.1). The outputs go to
//! `target/oracles/rerelease/<id>/`; when the files there already have the recorded digests (a
//! CI cache hit) they are checked without compiling again.
//!
//! Every progs.dat must load with `qc-progs`, be version 6, and use only opcodes 0 to 65
//! (PROGS-8); every `.lno` must have LNO-2's header with the progs's counts and LNO-3.1's line
//! table and nothing more. Both must have the SHA-256 recorded when this check was made, so a
//! change to the build of FTEQCC shows; the progs.dat's digest skips the text FTEQCC writes
//! after the header, which holds the date of the compile and of the FTEQCC build.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use qc_progs::opcode::Opcode;
use qc_progs::{Header, PROGS_HEADER_SIZE, PROGS_VERSION, Progs};

use super::{Oracles, sha256};

/// The arguments every codebase is compiled with, run in the codebase's directory.
const FLAGS: [&str; 4] = ["-Tid", "-flno", "-o", "../progs.dat"];

/// One rerelease codebase and the digests of what FTEQCC builds from it.
struct Codebase {
    id: &'static str,
    /// Arguments after [`FLAGS`].
    extra: &'static [&'static str],
    /// SHA-256 of the progs.dat with the text between the header and the first table zeroed.
    progs: &'static str,
    /// SHA-256 of the progs.lno.
    lno: &'static str,
}

/// The six codebases of `id-Software/quake-rerelease-qc`, at the commit `corpus/manifest.toml`
/// pins, with the digests recorded from the first build of this oracle.
const CODEBASES: [Codebase; 6] = [
    Codebase {
        id: "rerelease",
        extra: &[],
        progs: "7534f6b9bcef91c8ec75d6d1d4c3bcaddd737441a225188b2f16dfd2af70f8be",
        lno: "9f4ff421b198640aa6309799279fd4dfbd8948c68bea8dabef9fae18062d4969",
    },
    Codebase {
        id: "rerelease-hipnotic",
        extra: &[],
        progs: "35db5b2bc271a66d7fb1009576ae8e0249d82d82ee8f3faea25e6ec3fe55b25c",
        lno: "0d95805e13cce5d1d8671603777080a8309d746ca963406c508db4974bc6d9bb",
    },
    Codebase {
        id: "rerelease-rogue",
        extra: &[],
        progs: "9c38bd73a265dcb468044661f33a1da4c98642679a0d070e9680215e7759bf4f",
        lno: "79922c2db3af337083448d8ba98d3a4a066c087fcfece3ec9aeb9236c692dfab",
    },
    Codebase {
        id: "rerelease-mg1",
        extra: &[],
        progs: "38986d4982851f89dca3d89f5e4dac92966ac9b3713d9a170be3b4bc133e21d6",
        lno: "8658c61af4fc3847b1fcd992ace8b63f2eff04f73c1d8bb95d2d31d02944a530",
    },
    Codebase {
        id: "rerelease-ctf",
        extra: &[],
        progs: "03b8db147fba6ceb26b04f607bb37a351db333e907713805cf7ed5e877511eea",
        lno: "62fe8c42a530253335be0f985db2559b9d82da6d2f3866fc899098616f274cf6",
    },
    // Dawn of the Machine uses functions and constants before it declares them (`ai.qc` uses
    // `CORPSE_FLAG_A` from `monsters/mg3_hknight_infected.qc` and calls `OrbCheckAttack` from
    // `monsters/mg3_orb.qc`), which FTEQCC accepts only when it collects prototypes first.
    Codebase {
        id: "rerelease-mg3",
        extra: &["-Fautoproto"],
        progs: "ec625f9bc76564676d12b7a472da96b70f5fa44214d46fa6c2372efcaebc91c4",
        lno: "8a7c7646db787573d6357d44acc50df919ec5878ab897f239abc1cf406ebc085",
    },
];

/// LNO-2.1's magic.
const LNO_MAGIC: &[u8; 4] = b"LNOF";
/// LNO-2.2's only version.
const LNO_VERSION: u32 = 1;
/// LNO-2's header: the magic and five 32-bit integers.
const LNO_HEADER_SIZE: usize = 24;

pub(crate) fn check(oracles: &Oracles, codebases: &Path) -> Result<bool, String> {
    let mut ok = true;
    for codebase in &CODEBASES {
        ok &= check_one(oracles, codebases, codebase)?;
    }
    Ok(ok)
}

/// Whether `id` is one of the rerelease codebases this oracle compiles.
pub(crate) fn compiles(id: &str) -> bool {
    CODEBASES.iter().any(|c| c.id == id)
}

/// Whether FTEQCC compiles the rerelease codebase `id` with `-Fautoproto` (FTE-1.2).
pub(crate) fn autoprototypes(id: &str) -> bool {
    CODEBASES
        .iter()
        .any(|c| c.id == id && c.extra.contains(&"-Fautoproto"))
}

/// Compiles the rerelease codebase `id`, found in `codebases/<id>`, unless the outputs already
/// have the recorded digests. Returns the directory holding `progs.dat` and `progs.lno`, and
/// whether they were already there.
pub(crate) fn build(
    oracles: &Oracles,
    codebases: &Path,
    id: &str,
) -> Result<(PathBuf, bool), String> {
    let codebase = CODEBASES
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("{id} is not a rerelease codebase"))?;
    let out = oracles.rerelease.join(id);
    let cached = match (
        fs::read(out.join("progs.dat")),
        fs::read(out.join("progs.lno")),
    ) {
        (Ok(progs), Ok(lno)) => {
            progs_digest(&progs).is_some_and(|d| d == codebase.progs)
                && sha256::hex_digest(&lno) == codebase.lno
        }
        _ => false,
    };
    if !cached {
        compile(oracles, &codebases.join(id), codebase, &out)?;
    }
    Ok((out, cached))
}

fn check_one(oracles: &Oracles, codebases: &Path, codebase: &Codebase) -> Result<bool, String> {
    let id = codebase.id;
    let (out, cached) = build(oracles, codebases, id)?;
    let progs_path = out.join("progs.dat");
    let lno_path = out.join("progs.lno");
    let progs = fs::read(&progs_path).map_err(|e| format!("{}: {e}", progs_path.display()))?;
    let lno = fs::read(&lno_path).map_err(|e| format!("{}: {e}", lno_path.display()))?;

    let loaded = Progs::read(&progs).map_err(|e| format!("{id}: qc-progs refuses it: {e:?}"))?;
    let problems = problems(&loaded, &progs, &lno, (codebase.progs, codebase.lno));
    if problems.is_empty() {
        println!(
            "ok    {id}: {} statements, version 6, vanilla opcodes only, .lno matches{}",
            loaded.header().numstatements,
            if cached { " (cached)" } else { "" }
        );
        return Ok(true);
    }
    for problem in problems {
        println!("FAIL  {id}: {problem}");
    }
    Ok(false)
}

/// What is wrong with FTEQCC's outputs: the progs `loaded` from `progs`, and `lno`, whose
/// digests should be `want` ([`Codebase::progs`] and [`Codebase::lno`]).
fn problems(loaded: &Progs, progs: &[u8], lno: &[u8], want: (&str, &str)) -> Vec<String> {
    let (want_progs, want_lno) = want;
    let mut problems = Vec::new();
    let header = *loaded.header();
    if header.version != PROGS_VERSION {
        problems.push(format!("progs.dat is version {}, not 6", header.version));
    }
    problems.extend(extended_opcodes(loaded));
    problems.extend(lno_problems(lno, &header));
    let progs_got = progs_digest(progs).unwrap_or_default();
    if progs_got != want_progs {
        problems.push(format!(
            "progs.dat has sha256 {progs_got} (header text zeroed), expected {want_progs}"
        ));
    }
    let lno_got = sha256::hex_digest(lno);
    if lno_got != want_lno {
        problems.push(format!(
            "progs.lno has sha256 {lno_got}, expected {want_lno}"
        ));
    }
    problems
}

/// Copies the codebase to a scratch directory, compiles it there and copies the outputs to
/// `out`.
fn compile(
    oracles: &Oracles,
    source: &Path,
    codebase: &Codebase,
    out: &Path,
) -> Result<(), String> {
    if !source.join("progs.src").is_file() {
        return Err(format!("no progs.src in {}", source.display()));
    }
    let work = oracles.work.join("rerelease").join(codebase.id);
    if work.exists() {
        fs::remove_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    }
    let src = work.join("src");
    copy_tree(source, &src)?;
    let output = oracles
        .exec
        .output(
            Command::new(&oracles.fteqcc)
                .args(FLAGS)
                .args(codebase.extra)
                .current_dir(&src),
        )
        .map_err(|e| format!("cannot run fteqcc: {e}"))?;
    if !output.status.success() || !work.join("progs.dat").is_file() {
        println!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Err(format!(
            "fteqcc did not compile {} ({})",
            source.display(),
            output.status
        ));
    }
    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    for name in ["progs.dat", "progs.lno"] {
        let from = work.join(name);
        fs::copy(&from, out.join(name)).map_err(|e| format!("{}: {e}", from.display()))?;
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let Some(name) = path.file_name() else {
            continue;
        };
        if path.is_dir() {
            copy_tree(&path, &to.join(name))?;
        } else {
            fs::copy(&path, to.join(name)).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// The SHA-256 of a progs.dat with the bytes between the header and the first table zeroed:
/// FTEQCC writes its copyright text there (`qccmain.c`, `WriteData`), which holds the date.
fn progs_digest(progs: &[u8]) -> Option<String> {
    let header = *Progs::read(progs).ok()?.header();
    let first = [
        header.ofs_statements,
        header.ofs_globaldefs,
        header.ofs_fielddefs,
        header.ofs_functions,
        header.ofs_strings,
        header.ofs_globals,
    ]
    .into_iter()
    .filter_map(|ofs| usize::try_from(ofs).ok())
    .min()?;
    let mut bytes = progs.to_vec();
    for byte in bytes.get_mut(PROGS_HEADER_SIZE..first)? {
        *byte = 0;
    }
    Some(sha256::hex_digest(&bytes))
}

/// Every statement whose opcode id's VM does not have (PROGS-8), as problems.
fn extended_opcodes(progs: &Progs<'_>) -> Vec<String> {
    let mut problems = Vec::new();
    for (index, statement) in progs.statements().iter().enumerate() {
        if Opcode::from_number(statement.op).is_none() {
            problems.push(format!(
                "statement {index} has opcode {}, which id's VM lacks",
                statement.op
            ));
        }
    }
    problems
}

/// What is wrong with a `.lno` FTEQCC wrote beside a progs with `header`: LNO-2's header, the
/// counts of LNO-2.3, and a line table of LNO-3.1 with no column table (LNO-3.3).
fn lno_problems(lno: &[u8], header: &Header) -> Vec<String> {
    let word = |index: usize| -> Option<u32> {
        let start = index.checked_mul(4)?;
        let bytes: [u8; 4] = lno.get(start..start.checked_add(4)?)?.try_into().ok()?;
        Some(u32::from_le_bytes(bytes))
    };
    let mut problems = Vec::new();
    if lno.get(..4) != Some(LNO_MAGIC.as_slice()) {
        problems.push("progs.lno does not start with LNOF (LNO-2.1)".to_owned());
    }
    if word(1) != Some(LNO_VERSION) {
        problems.push(format!(
            "progs.lno has version {:?}, not 1 (LNO-2.2)",
            word(1)
        ));
    }
    let counts = [
        ("numglobaldefs", header.numglobaldefs),
        ("numglobals", header.numglobals),
        ("numfielddefs", header.numfielddefs),
        ("numstatements", header.numstatements),
    ];
    for (index, (name, want)) in (2..).zip(counts) {
        if word(index).and_then(|got| i32::try_from(got).ok()) != Some(want) {
            problems.push(format!(
                "progs.lno's {name} is {:?}, the progs.dat's is {want} (LNO-2.3)",
                word(index)
            ));
        }
    }
    let size = usize::try_from(header.numstatements)
        .ok()
        .and_then(|n| n.checked_mul(4))
        .and_then(|table| table.checked_add(LNO_HEADER_SIZE));
    if size != Some(lno.len()) {
        problems.push(format!(
            "progs.lno is {} bytes, expected {size:?}: the header and one line per statement \
             (LNO-3.1, LNO-3.3)",
            lno.len()
        ));
    }
    problems
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "test code: a failed unwrap, index or sum is a failed test"
    )]

    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use qc_progs::{Header, Progs, ProgsFile, Statement, write};

    use super::super::testing::{fake_oracles, fresh_dir};
    use super::{
        CODEBASES, build, check, compiles, extended_opcodes, lno_problems, problems, progs_digest,
    };
    use crate::exec::{Fake, Reply};

    /// A fake FTEQCC that writes `progs` and `lno` where `-o ../progs.dat` puts them.
    fn fteqcc(progs: Vec<u8>, lno: Vec<u8>) -> impl Fn(&Command) -> Reply {
        move |command: &Command| {
            let work = command.get_current_dir().unwrap().parent().unwrap();
            Reply {
                files: vec![
                    (work.join("progs.dat"), progs.clone()),
                    (work.join("progs.lno"), lno.clone()),
                ],
                ..Reply::default()
            }
        }
    }

    /// A directory of the six codebases, each a `progs.src` and a file in a subdirectory.
    fn codebases(dir: &Path) -> std::path::PathBuf {
        let codebases = dir.join("codebases");
        for codebase in &CODEBASES {
            let source = codebases.join(codebase.id);
            fs::create_dir_all(source.join("sub")).unwrap();
            fs::write(source.join("progs.src"), "../progs.dat\nsub/a.qc\n").unwrap();
            fs::write(source.join("sub/a.qc"), "void() main = {};\n").unwrap();
        }
        codebases
    }

    #[test]
    fn every_codebase_is_compiled_in_a_copy_and_checked() {
        let dir = fresh_dir("rerelease-check");
        let codebases = codebases(&dir);
        let bytes = progs(&[0, 66]);
        let header = *Progs::read(&bytes).unwrap().header();
        let fake = Fake::new(fteqcc(bytes, lno(&header)));
        let oracles = fake_oracles(&dir.join("work"), &fake);
        // FTEQCC's outputs differ from the recorded ones, and hold an extended opcode.
        assert!(!check(&oracles, &codebases).unwrap());
        {
            let seen = fake.seen.borrow();
            assert_eq!(seen.len(), 6);
            assert!(
                seen.iter()
                    .all(|line| line.starts_with("fteqcc -Tid -flno -o ../progs.dat"))
            );
            assert!(seen.iter().any(|line| line.ends_with("-Fautoproto")));
        }
        let kept = oracles.rerelease.join("rerelease-mg1");
        assert!(kept.join("progs.dat").is_file() && kept.join("progs.lno").is_file());
        assert!(
            oracles
                .work
                .join("rerelease/rerelease/src/sub/a.qc")
                .is_file()
        );
        // Built again: the outputs do not have the recorded digests, so FTEQCC runs again.
        let (out, cached) = build(&oracles, &codebases, "rerelease").unwrap();
        assert!(!cached);
        assert_eq!(out, oracles.rerelease.join("rerelease"));
    }

    #[test]
    fn a_codebase_that_does_not_compile_is_an_error() {
        let dir = fresh_dir("rerelease-errors");
        let codebases = codebases(&dir);
        let fails = Fake::new(|_: &Command| Reply {
            code: 1,
            stdout: "error".to_owned(),
            ..Reply::default()
        });
        let oracles = fake_oracles(&dir.join("work"), &fails);
        let error = build(&oracles, &codebases, "rerelease").unwrap_err();
        assert!(error.contains("fteqcc did not compile"), "{error}");
        assert!(
            build(&oracles, &codebases, "quake")
                .unwrap_err()
                .contains("not a rerelease")
        );
        fs::remove_file(codebases.join("rerelease-ctf/progs.src")).unwrap();
        let error = build(&oracles, &codebases, "rerelease-ctf").unwrap_err();
        assert!(error.contains("no progs.src"), "{error}");
        let cannot_run = Fake::new(|_: &Command| Reply {
            cannot_run: true,
            ..Reply::default()
        });
        let oracles = fake_oracles(&dir.join("work"), &cannot_run);
        assert!(
            build(&oracles, &codebases, "rerelease")
                .unwrap_err()
                .contains("cannot run")
        );
        assert!(compiles("rerelease-mg3"));
        assert!(!compiles("id-1.01"));
    }

    /// A version 6 progs.dat holding statements with these opcodes.
    fn progs(ops: &[u32]) -> Vec<u8> {
        let file = ProgsFile {
            statements: ops
                .iter()
                .map(|&op| Statement {
                    op,
                    ..Statement::default()
                })
                .collect(),
            strings: vec![0],
            ..ProgsFile::default()
        };
        write(&file).unwrap()
    }

    /// `progs` with `lead` inserted between the header and the first table, as FTEQCC's
    /// copyright text is, and every table offset moved past it.
    fn with_lead(progs: &[u8], lead: &[u8]) -> Vec<u8> {
        let mut out = progs[..60].to_vec();
        for field in [2_usize, 4, 6, 8, 10, 12] {
            let at = field * 4;
            let ofs = i32::from_le_bytes(out[at..at + 4].try_into().unwrap());
            let moved = ofs + i32::try_from(lead.len()).unwrap();
            out[at..at + 4].copy_from_slice(&moved.to_le_bytes());
        }
        out.extend_from_slice(lead);
        out.extend_from_slice(&progs[60..]);
        out
    }

    /// The `.lno` FTEQCC writes for a progs with `header`, every statement on line 1.
    fn lno(header: &Header) -> Vec<u8> {
        let mut out = b"LNOF".to_vec();
        for word in [
            1,
            header.numglobaldefs,
            header.numglobals,
            header.numfielddefs,
            header.numstatements,
        ] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        for _ in 0..header.numstatements {
            out.extend_from_slice(&1_i32.to_le_bytes());
        }
        out
    }

    #[test]
    fn vanilla_opcodes_pass_and_extended_ones_are_named() {
        let bytes = progs(&[0, 65]);
        assert!(extended_opcodes(&Progs::read(&bytes).unwrap()).is_empty());
        let bytes = progs(&[0, 66, 65, 300]);
        let problems = extended_opcodes(&Progs::read(&bytes).unwrap());
        assert_eq!(
            problems,
            [
                "statement 1 has opcode 66, which id's VM lacks",
                "statement 3 has opcode 300, which id's VM lacks",
            ]
        );
    }

    #[test]
    fn the_digest_ignores_the_text_after_the_header_only() {
        let bytes = progs(&[1, 2]);
        let a = with_lead(&bytes, b"Compiled [2026/10/16]\0\0\0");
        let b = with_lead(&bytes, b"Compiled [2027/01/01]\0\0\0");
        assert!(Progs::read(&a).is_ok());
        assert_eq!(progs_digest(&a), progs_digest(&b));
        assert!(progs_digest(&a).is_some());
        let other = with_lead(&progs(&[1, 3]), b"Compiled [2026/10/16]\0\0\0");
        assert_ne!(progs_digest(&a), progs_digest(&other));
        assert_eq!(progs_digest(b"not a progs"), None);
    }

    #[test]
    fn outputs_with_the_recorded_digests_pass_and_each_difference_is_named() {
        let bytes = progs(&[0, 1, 2]);
        let loaded = Progs::read(&bytes).unwrap();
        let good_lno = lno(loaded.header());
        let progs_sha = progs_digest(&bytes).unwrap();
        let lno_sha = super::sha256::hex_digest(&good_lno);
        let found = |want: (&str, &str)| problems(&loaded, &bytes, &good_lno, want);
        assert_eq!(found((&progs_sha, &lno_sha)), Vec::<String>::new());
        let wrong = found(("00", &lno_sha));
        assert_eq!(wrong.len(), 1, "{wrong:?}");
        assert!(wrong[0].starts_with("progs.dat has sha256"), "{wrong:?}");
        let wrong = found((&progs_sha, "00"));
        assert_eq!(wrong.len(), 1, "{wrong:?}");
        assert!(wrong[0].starts_with("progs.lno has sha256"), "{wrong:?}");
    }

    #[test]
    fn a_lno_with_the_progs_counts_passes() {
        let bytes = progs(&[0, 1, 2]);
        let header = *Progs::read(&bytes).unwrap().header();
        assert_eq!(lno_problems(&lno(&header), &header), Vec::<String>::new());
    }

    #[test]
    fn each_lno_defect_is_reported() {
        let bytes = progs(&[0, 1, 2]);
        let header = *Progs::read(&bytes).unwrap().header();
        let good = lno(&header);

        let mut magic = good.clone();
        magic[3] = b'X';
        assert_eq!(lno_problems(&magic, &header).len(), 1);

        let mut version = good.clone();
        version[4] = 2;
        assert_eq!(lno_problems(&version, &header).len(), 1);

        let mut count = good.clone();
        count[20] = 4;
        assert_eq!(lno_problems(&count, &header).len(), 1);

        let mut columns = good.clone();
        columns.extend_from_slice(&[0; 12]);
        assert_eq!(lno_problems(&columns, &header).len(), 1);

        assert!(lno_problems(&good[..10], &header).len() >= 2);
        assert!(lno_problems(&[], &header).len() >= 2);
    }
}
