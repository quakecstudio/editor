// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask commands [--check]`: writes the command reference, `docs/commands.md`, from
//! the `qs-commands` registry (P2-12, PRIN-3); `--check` writes nothing and fails when the
//! checked-in file differs, which is the check `cargo xtask ci` runs.

use std::fs;
use std::path::Path;

use qs_commands::Registry;

/// The reference, relative to the workspace root.
pub(crate) const REFERENCE: &str = "docs/commands.md";

const USAGE: &str = "usage: cargo xtask commands [--check]";

/// Runs `cargo xtask commands` with the arguments `args`.
pub(crate) fn main(root: &Path, args: &[String]) -> Result<bool, String> {
    let check = match args {
        [] => false,
        [flag] if flag == "--check" => true,
        _ => return Err(USAGE.to_owned()),
    };
    run(root, check)
}

fn run(root: &Path, check: bool) -> Result<bool, String> {
    let reference = Registry::<()>::new().markdown();
    let path = root.join(REFERENCE);
    if check {
        let current = fs::read_to_string(&path).map_err(|e| format!("{REFERENCE}: {e}"))?;
        if current == reference {
            eprintln!("commands: {REFERENCE} matches the registry");
            return Ok(true);
        }
        eprintln!("commands: {REFERENCE} differs from the registry; run cargo xtask commands");
        return Ok(false);
    }
    fs::write(&path, reference).map_err(|e| format!("{REFERENCE}: {e}"))?;
    eprintln!("commands: wrote {REFERENCE}");
    Ok(true)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use super::*;
    use crate::test_dirs;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|&a| a.to_owned()).collect()
    }

    #[test]
    fn the_checked_in_reference_matches_the_registry() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        assert_eq!(main(root, &args(&["--check"])), Ok(true));
    }

    #[test]
    fn writing_then_checking_passes_and_an_edit_fails_the_check() {
        let root = test_dirs::fresh("commands-reference");
        fs::create_dir_all(root.join("docs")).unwrap();

        assert!(
            main(&root, &args(&["--check"]))
                .unwrap_err()
                .contains(REFERENCE)
        );
        assert_eq!(main(&root, &[]), Ok(true));
        assert_eq!(main(&root, &args(&["--check"])), Ok(true));

        let path = root.join(REFERENCE);
        let edited = fs::read_to_string(&path)
            .unwrap()
            .replace("edit.undo", "edit.undone");
        fs::write(&path, edited).unwrap();
        assert_eq!(main(&root, &args(&["--check"])), Ok(false));
    }

    #[test]
    fn other_arguments_are_a_usage_error() {
        let root = Path::new(".");
        for bad in [&["--write"][..], &["--check", "--check"]] {
            assert_eq!(main(root, &args(bad)), Err(USAGE.to_owned()));
        }
    }
}
