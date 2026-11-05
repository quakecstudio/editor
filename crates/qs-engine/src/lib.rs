// SPDX-License-Identifier: GPL-3.0-or-later

//! The IDE's client for the preview host, the only engine it runs (ADR-0005, P3-06): the run
//! roots ADR-0008 launches every engine in, finding and starting the host, and the bridge
//! protocol (`docs/spec/bridge-protocol.md`) spoken to it.

mod client;
mod launch;
mod run_root;

pub use client::{BridgeEvent, Client, ClientError, InspectWhat, StepKind};
pub use launch::{LaunchError, LaunchSpec, Running, host_command, launch, locate_host};
pub use run_root::{Flavour, RunRoot, RunRootError, prepare_run_root};

#[cfg(test)]
mod tests;
