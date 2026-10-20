// SPDX-License-Identifier: GPL-3.0-or-later

//! The directories xtask's tests write, each owned by one test.
//!
//! `cargo test` runs the tests of one binary on parallel threads, so two tests
//! that write the same directory read each other's half-written files: two
//! readiness tests sharing one cargo-mutants output directory once failed
//! `make ci` with "mutants outcomes: EOF while parsing a value" (#468). Every
//! directory a test writes comes from [`fresh`], or is [`claim`]ed if its path
//! is fixed, and a test that claims a directory overlapping another test's
//! fails at once instead of racing.
//!
//! libtest names each test's thread after the test, which is how a claim knows
//! its owner; one test may claim the same directory again.

#![expect(
    clippy::panic,
    reason = "test code: a directory two tests share, or one that cannot be made, fails the test"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// Every directory claimed in this run, and the test that claimed it.
static CLAIMS: Mutex<BTreeMap<PathBuf, String>> = Mutex::new(BTreeMap::new());

/// The repository.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Claims `dir` for the running test. Panics if another test of this run
/// claimed `dir`, a directory inside it or one that contains it.
pub(crate) fn claim(dir: &Path) {
    let test = std::thread::current()
        .name()
        .unwrap_or("an unnamed thread")
        .to_owned();
    claim_as(dir, &test);
}

fn claim_as(dir: &Path, test: &str) {
    let mut claims = CLAIMS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((other, owner)) = claims
        .iter()
        .find(|(other, owner)| *owner != test && (dir.starts_with(other) || other.starts_with(dir)))
    {
        panic!(
            "{test} writes {}, which overlaps {} of {owner}: give each test its own directory",
            dir.display(),
            other.display()
        );
    }
    claims.insert(dir.to_path_buf(), test.to_owned());
}

/// A new, empty `target/xtask-tests/<name>`, claimed for the running test.
/// `name` may have several components, `readiness/mutation-reads`.
pub(crate) fn fresh(name: &str) -> PathBuf {
    let dir = root().join("target").join("xtask-tests").join(name);
    claim(&dir);
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    }
    fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    dir
}

mod tests {
    use super::*;

    fn claims_fail(dir: &str, test: &str) -> bool {
        let dir = Path::new("/claims").join(dir);
        std::panic::catch_unwind(|| claim_as(&dir, test)).is_err()
    }

    #[test]
    fn a_directory_overlapping_another_tests_is_refused() {
        assert!(!claims_fail("a/b", "first"));
        assert!(
            !claims_fail("a/b", "first"),
            "a test may claim its own again"
        );
        assert!(claims_fail("a/b", "second"), "the same directory");
        assert!(claims_fail("a/b/c", "second"), "a directory inside it");
        assert!(claims_fail("a", "second"), "a directory containing it");
        assert!(
            !claims_fail("a/bc", "second"),
            "a sibling with a longer name"
        );
        assert!(!claims_fail("a/d", "second"));
    }

    #[test]
    fn a_fresh_directory_is_empty_and_belongs_to_its_test() {
        let dir = fresh("test-dirs/fresh");
        fs::write(dir.join("left-over"), "x").unwrap_or_else(|e| panic!("{e}"));
        let again = fresh("test-dirs/fresh");
        assert_eq!(dir, again);
        assert!(again.is_dir());
        assert_eq!(
            fs::read_dir(&again)
                .unwrap_or_else(|e| panic!("{e}"))
                .count(),
            0
        );
        assert!(
            std::thread::spawn(move || claim(&again)).join().is_err(),
            "another thread, as another test, may not claim it"
        );
    }
}
