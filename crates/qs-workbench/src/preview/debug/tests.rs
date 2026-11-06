// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::Path;

use serde_json::json;

use super::{Debugger, Location, StopReason};

// No spec statement: a breakpoint toggles on and off at its file and line, and the host's id is
// forgotten when the game stops
#[test]
fn breakpoints_toggle_and_are_kept() {
    let mut debugger = Debugger::new();
    let file = Path::new("src/weapons.qc");
    assert!(debugger.toggle(file, 33).is_none(), "added");
    assert!(debugger.toggle(Path::new("src/items.qc"), 33).is_none());
    assert_eq!(debugger.lines_of(file).collect::<Vec<_>>(), [33]);
    debugger.breakpoint_mut(file, 33).unwrap().id = Some(3);
    debugger.unset_all();
    assert_eq!(debugger.breakpoints()[0].id, None);
    let removed = debugger.toggle(file, 33).unwrap();
    assert_eq!(removed.line, 33);
    assert_eq!(debugger.breakpoints().len(), 1);
    assert!(debugger.breakpoint_mut(file, 33).is_none());
}

// spec BRIDGE-3.3, BRIDGE-3.4: a stop names its function, and its file and line when the
// program has a .lno; an engine frame its function alone; resuming forgets the stop and stack
#[test]
fn stops_and_stacks_are_read() {
    let mut debugger = Debugger::new();
    let stopped = Location::from_value(&json!({
        "function": "Ember_Touch", "file": "src/weapons.qc", "line": 33, "statement": 1177
    }))
    .unwrap();
    assert_eq!(stopped.line, Some(33));
    assert!(Location::from_value(&json!({ "file": "x.qc" })).is_none());
    debugger.stop(StopReason::Breakpoint, stopped.clone());
    debugger.set_stack(&json!({ "stack": [
        { "index": 0, "function": "Ember_Touch", "file": "src/weapons.qc", "line": 33 },
        { "index": 1, "function": "SV_Impact", "engine": true },
        { "index": 2 }
    ] }));
    assert_eq!(debugger.stack().len(), 2);
    assert!(debugger.stack()[1].engine);
    assert_eq!(debugger.stack()[1].file, None);
    assert_eq!(debugger.stopped().unwrap().1, stopped);
    debugger.failed("division by zero");
    assert_eq!(debugger.error(), Some("division by zero"));
    debugger.resumed();
    assert!(debugger.stopped().is_none());
    assert!(debugger.stack().is_empty());
    assert_eq!(StopReason::of("step"), Some(StopReason::Step));
    assert_eq!(StopReason::of("error"), Some(StopReason::Error));
    assert_eq!(StopReason::of("pause"), None);
}

// No spec statement: watches are added once each, never empty, and taken away by place
#[test]
fn watches_are_added_and_taken_away() {
    let mut debugger = Debugger::new();
    debugger.watch(" self.health ");
    debugger.watch("self.health");
    debugger.watch("   ");
    debugger.watch("time");
    let names: Vec<_> = debugger
        .watches()
        .iter()
        .map(|w| w.expression.as_str())
        .collect();
    assert_eq!(names, ["self.health", "time"]);
    debugger.watches_mut()[0].value = Some("100".to_owned());
    debugger.unwatch(5);
    debugger.unwatch(0);
    assert_eq!(debugger.watches().len(), 1);
    assert_eq!(debugger.watches()[0].expression, "time");
}
