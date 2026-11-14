// SPDX-License-Identifier: GPL-3.0-or-later

//! The commands each surface offers (P6-40): the command line registers the workbench's
//! commands, and the MCP server and the window those and the preview's. Moving a command
//! between crates must not move it between the two, which `docs/commands.md` cannot show: it
//! is written from a registry that has both.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use qc_project::MemoryLoader;
use qs_commands::Registry;
use qs_workbench::{HasPreview, HasWorkbench, Preview, Workbench, register, register_preview};

/// A workbench and a preview: what the MCP server and the window run commands on.
struct Context {
    workbench: Workbench<MemoryLoader>,
    preview: Preview,
}

impl HasWorkbench for Context {
    type Loader = MemoryLoader;

    fn workbench(&mut self) -> &mut Workbench<MemoryLoader> {
        &mut self.workbench
    }

    fn preview(&mut self) -> Option<&mut Preview> {
        Some(&mut self.preview)
    }
}

impl HasPreview for Context {
    fn workbench_and_preview(&mut self) -> (&mut Workbench<MemoryLoader>, &mut Preview) {
        (&mut self.workbench, &mut self.preview)
    }
}

/// The ids of `registry`'s commands, one to a line, in id order.
fn ids<C: 'static>(registry: &Registry<C>) -> String {
    registry
        .commands()
        .map(|info| format!("{}\n", info.id().as_str()))
        .collect()
}

// No spec statement: P6-40: the command line offers exactly the workbench's commands
#[test]
fn the_command_line_offers_the_workbench_commands() {
    let mut registry = Registry::<Workbench<MemoryLoader>>::new();
    register(&mut registry).unwrap();
    assert_eq!(
        ids(&registry),
        include_str!("command_surfaces/workbench.txt")
    );
}

// No spec statement: P6-40: MCP and the window offer the workbench's and the preview's commands
#[test]
fn mcp_and_the_window_offer_the_preview_commands_too() {
    let mut registry = Registry::<Context>::new();
    register(&mut registry).unwrap();
    register_preview(&mut registry).unwrap();
    assert_eq!(
        ids(&registry),
        include_str!("command_surfaces/with_preview.txt")
    );
}
