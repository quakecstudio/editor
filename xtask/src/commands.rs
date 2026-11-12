// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask commands [--check]`: writes the command reference, `docs/commands.md`, from
//! the `qs-commands` registry with every crate's commands registered (P2-12, PRIN-3); `--check` writes nothing and fails when the
//! checked-in file differs. `cargo xtask ci` runs the same check in-process ([`check`]).

use std::fs;
use std::path::Path;

use qs_commands::Registry;
use qs_layout::{HasLayout, Layout};
use qs_settings::{HasSettings, SettingsStore};
use qs_workbench::{HasWorkbench, Workbench};

use crate::{Report, print_outcome};

/// The reference, relative to the workspace root.
pub(crate) const REFERENCE: &str = "docs/commands.md";

/// `cargo xtask commands [--check]`.
#[derive(clap::Parser, Debug)]
pub(crate) struct Args {
    /// Check the reference instead of writing it.
    #[arg(long)]
    check: bool,
}

/// Runs `cargo xtask commands` with the arguments `args`.
pub(crate) fn main(root: &Path, args: &Args) -> Result<bool, String> {
    if args.check {
        return Ok(print_outcome("commands", check(root)));
    }
    fs::write(root.join(REFERENCE), reference()?).map_err(|e| format!("{REFERENCE}: {e}"))?;
    eprintln!("commands: wrote {REFERENCE}");
    Ok(true)
}

/// Checks that `docs/commands.md` is what the registry renders.
pub(crate) fn check(root: &Path) -> Result<Report, String> {
    let current =
        fs::read_to_string(root.join(REFERENCE)).map_err(|e| format!("{REFERENCE}: {e}"))?;
    let mut report = Report::default();
    let matches = current == reference()?;
    report.record(
        matches,
        if matches {
            format!("{REFERENCE} matches the registry")
        } else {
            format!("{REFERENCE} differs from the registry; run cargo xtask commands")
        },
    );
    Ok(report)
}

/// The context the reference's registry is built over: one with every part a command reads.
struct Everything {
    settings: SettingsStore,
    layout: Layout,
    workbench: Workbench<qc_project::MemoryLoader>,
    preview: qs_workbench::Preview,
}

impl HasSettings for Everything {
    fn settings(&mut self) -> &mut SettingsStore {
        &mut self.settings
    }
}

impl HasLayout for Everything {
    fn layout_mut(&mut self) -> &mut Layout {
        &mut self.layout
    }
}

impl HasWorkbench for Everything {
    type Loader = qc_project::MemoryLoader;

    fn workbench(&mut self) -> &mut Workbench<qc_project::MemoryLoader> {
        &mut self.workbench
    }

    fn preview(&mut self) -> Option<&mut qs_workbench::Preview> {
        Some(&mut self.preview)
    }
}

impl qs_workbench::HasPreview for Everything {
    fn workbench_and_preview(
        &mut self,
    ) -> (
        &mut Workbench<qc_project::MemoryLoader>,
        &mut qs_workbench::Preview,
    ) {
        (&mut self.workbench, &mut self.preview)
    }
}

/// The reference the registry renders: the built-in commands, the settings commands (P2-22),
/// the layout commands (P2-17), the workbench commands (P2-19), the play loop's (P3-12) and `project.new` (P4-17).
fn reference() -> Result<String, String> {
    let mut registry = Registry::<Everything>::new();
    qs_settings::register(&mut registry).map_err(|e| e.to_string())?;
    qs_layout::register(&mut registry).map_err(|e| e.to_string())?;
    qs_workbench::register(&mut registry).map_err(|e| e.to_string())?;
    qs_workbench::register_preview(&mut registry).map_err(|e| e.to_string())?;
    qs_templates::register(
        &mut registry,
        std::sync::Arc::new(qs_templates::GitHub::new()),
    )
    .map_err(|e| e.to_string())?;
    Ok(registry.markdown())
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use super::*;
    use crate::test_dirs;

    #[test]
    fn writing_then_checking_passes_and_an_edit_fails_the_check() {
        let root = test_dirs::fresh("commands-reference");
        fs::create_dir_all(root.join("docs")).unwrap();

        assert!(check(&root).unwrap_err().contains(REFERENCE));
        assert_eq!(main(&root, &Args { check: false }), Ok(true));
        assert!(check(&root).unwrap().problems.is_empty());
        assert_eq!(main(&root, &Args { check: true }), Ok(true));

        let path = root.join(REFERENCE);
        let edited = fs::read_to_string(&path)
            .unwrap()
            .replace("edit.undo", "edit.undone");
        fs::write(&path, edited).unwrap();
        assert_eq!(check(&root).unwrap().problems.len(), 1);
        assert_eq!(main(&root, &Args { check: true }), Ok(false));
    }

    #[test]
    fn other_arguments_are_a_usage_error() {
        for bad in [&["--write"][..], &["--check", "--check"]] {
            assert!(crate::parse_cli::<Args, _>(bad).is_err(), "{bad:?}");
        }
    }
}
