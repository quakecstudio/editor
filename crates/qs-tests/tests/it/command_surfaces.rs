// SPDX-License-Identifier: GPL-3.0-or-later

//! The commands of the workbench's two registration functions (P6-40): the command line calls
//! `register`, and the MCP server and the window call `register_preview` as well. Moving a
//! command between crates must not move it between the two, which `docs/commands.md` cannot
//! show: it is written from a registry that has both. What a surface adds of its own (the
//! templates, the layout) is not pinned here.

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

impl qs_map_edit::HasMap for Context {
    type Project = Workbench<MemoryLoader>;
    type Live = Preview;

    fn map_store(&mut self) -> &mut qs_map_edit::MapStore {
        self.workbench.maps_mut()
    }

    fn map_project(&mut self) -> &mut Workbench<MemoryLoader> {
        &mut self.workbench
    }

    fn live_map(&mut self) -> Option<&mut Preview> {
        Some(&mut self.preview)
    }
}

impl HasWorkbench for Context {
    type Loader = MemoryLoader;

    fn workbench(&mut self) -> &mut Workbench<MemoryLoader> {
        &mut self.workbench
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

// No spec statement: P6-40: register offers the same commands as before, to the command line
// among others
#[test]
fn register_offers_the_workbench_commands() {
    let mut registry = Registry::<Workbench<MemoryLoader>>::new();
    register(&mut registry).unwrap();
    assert_eq!(
        ids(&registry),
        include_str!("command_surfaces/workbench.txt")
    );
}

// No spec statement: P6-40: register and register_preview together offer the same commands as
// before, to MCP and the window
#[test]
fn register_preview_adds_the_preview_commands() {
    let mut registry = Registry::<Context>::new();
    register(&mut registry).unwrap();
    register_preview(&mut registry).unwrap();
    assert_eq!(
        ids(&registry),
        include_str!("command_surfaces/with_preview.txt")
    );
}
