// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use qs_map_edit::{EntityIndex, MapRevision};
use qs_workbench::map_logic::{
    CallbackKind, Edict, FireEnd, Fired, FiredEvent, LogicIssue, LogicIssues, LogicKind, MapFired,
};

use super::LogicPane;
use crate::map_editor::MapAction;
use crate::strings::{Text, fire_summary, fired_line, logic_issue_count};
use crate::theme::{Mode, Theme};

fn of_map(edict: usize, entity: usize) -> Fired {
    Fired {
        edict: Edict::new(edict),
        entity: Some(EntityIndex::new(entity)),
    }
}

/// A button, entity 1, fired: it looks its target up and the door, entity 2, is used.
fn trace() -> MapFired {
    MapFired {
        fired_by: CallbackKind::Use,
        events: vec![
            FiredEvent::Ran {
                time: 1.5,
                callback: CallbackKind::Use,
                entity: of_map(2, 1),
                classname: "func_button".to_owned(),
                function: "button_use".to_owned(),
            },
            FiredEvent::Looked {
                time: 1.5,
                by: of_map(2, 1),
                name: "d".to_owned(),
                from_start: true,
                found: Some(of_map(3, 2)),
            },
            FiredEvent::Ran {
                time: 1.5,
                callback: CallbackKind::Use,
                entity: of_map(3, 2),
                classname: "func_door".to_owned(),
                function: "door_use".to_owned(),
            },
        ],
        end: FireEnd::Quiet,
        seconds: 0.5,
    }
}

fn issue() -> LogicIssue {
    LogicIssue {
        entity: EntityIndex::new(4),
        kind: LogicKind::NothingToUse,
        fired: EntityIndex::new(1),
        message: "entity 4 (info_null) is fired and has no use function".to_owned(),
    }
}

/// The pane with `selected` the one entity selected, after `act` on its harness; the pane and
/// what it asked for.
fn after(
    pane: LogicPane,
    selected: Option<EntityIndex>,
    act: impl FnOnce(&mut Harness<'_, (LogicPane, Vec<MapAction>)>),
) -> (LogicPane, Vec<MapAction>) {
    let theme = Theme::load(Mode::Dark).unwrap();
    let mut harness = theme.harness().build_ui_state(
        |ui, (pane, actions): &mut (LogicPane, Vec<MapAction>)| {
            actions.extend(pane.header(ui, &theme, selected));
            actions.extend(pane.body(ui, &theme));
        },
        (pane, Vec::new()),
    );
    harness.run();
    harness.state_mut().1.clear();
    act(&mut harness);
    harness.run();
    let actions = harness.state().1.clone();
    let pane = core::mem::take(&mut harness.state_mut().0);
    (pane, actions)
}

// No spec statement: L-76: Fire asks to fire the one entity selected, and Check logic asks for
// the logic issues
#[test]
fn fire_and_check_ask_for_their_commands() {
    let (_, fired) = after(LogicPane::default(), Some(EntityIndex::new(3)), |harness| {
        harness.get_by_label(Text::MapFire.get()).click();
    });
    assert_eq!(fired, vec![MapAction::Fire(EntityIndex::new(3))]);
    let (_, nothing) = after(LogicPane::default(), None, |harness| {
        harness.get_by_label(Text::MapFire.get()).click();
    });
    assert_eq!(nothing, vec![], "nothing is selected to fire");
    let (_, checked) = after(LogicPane::default(), None, |harness| {
        harness.get_by_label(Text::MapCheckLogic.get()).click();
    });
    assert_eq!(checked, vec![MapAction::CheckLogic]);
}

// No spec statement: L-76: a trace is listed step by step with what it came to, and a step
// selects its entity
#[test]
fn a_step_of_the_trace_selects_its_entity() {
    let mut pane = LogicPane::default();
    pane.set_fired(EntityIndex::new(1), trace(), MapRevision::default());
    let door = fired_line(&trace().events[2]).0;
    assert_eq!(door, "1.50  use  entity 2 (func_door)  door_use");
    let (_, chosen) = after(pane, None, |harness| {
        assert!(
            harness
                .query_by_label(&fire_summary(EntityIndex::new(1), &trace()))
                .is_some()
        );
        harness.get_by_label(&door).click();
    });
    assert_eq!(chosen, vec![MapAction::Select(Some(EntityIndex::new(2)))]);
}

// No spec statement: L-76: the links lit are those the trace has fired up to the scrubber's
// step, in order
#[test]
fn the_links_lit_follow_the_scrubber() {
    let mut pane = LogicPane::default();
    assert_eq!(pane.lit(), vec![]);
    pane.set_fired(EntityIndex::new(1), trace(), MapRevision::default());
    let link = (EntityIndex::new(1), EntityIndex::new(2));
    assert_eq!(pane.lit(), vec![link], "the whole trace at first");
    pane.fired.as_mut().unwrap().upto = 1;
    assert_eq!(pane.lit(), vec![], "before the lookup");
    pane.fired.as_mut().unwrap().upto = 2;
    assert_eq!(pane.lit(), vec![link]);
}

// No spec statement: L-76: a logic issue selects the entity it is about, and the count is
// said
#[test]
fn an_issue_selects_its_entity() {
    let mut pane = LogicPane::default();
    pane.set_issues(
        LogicIssues {
            issues: vec![issue()],
            fired: 3,
        },
        MapRevision::default(),
    );
    let (_, chosen) = after(pane, None, |harness| {
        assert!(harness.query_by_label(&logic_issue_count(1)).is_some());
        harness.get_by_label(&issue().message).click();
    });
    assert_eq!(chosen, vec![MapAction::Select(Some(EntityIndex::new(4)))]);
    assert_eq!(logic_issue_count(0), "No logic issues");
    assert_eq!(logic_issue_count(2), "2 logic issues");
}

// No spec statement: L-76: a trace and issues found at one edit of the map are dropped once
// the map is at another
#[test]
fn an_edit_drops_the_trace_and_the_issues() {
    let mut pane = LogicPane::default();
    let at = MapRevision::default();
    pane.set_fired(EntityIndex::new(1), trace(), at);
    pane.set_issues(
        LogicIssues {
            issues: vec![issue()],
            fired: 1,
        },
        at,
    );
    pane.follow(at);
    assert!(pane.fired.is_some() && pane.issues.is_some());
    pane.follow(at.next());
    assert!(pane.fired.is_none() && pane.issues.is_none() && pane.seen.is_none());
}

// No spec statement: L-76: each kind of event has its line, and an entity the game spawned is
// named by its edict
#[test]
fn every_event_has_its_line() {
    let spawned = Fired {
        edict: Edict::new(40),
        entity: None,
    };
    let lines: Vec<(String, Option<usize>)> = [
        FiredEvent::Looked {
            time: 2.0,
            by: of_map(2, 1),
            name: "d".to_owned(),
            from_start: false,
            found: None,
        },
        FiredEvent::Unanswered {
            time: 2.0,
            by: of_map(2, 1),
            entity: of_map(5, 4),
            classname: "info_null".to_owned(),
        },
        FiredEvent::Removed {
            time: 2.0,
            entity: spawned,
            classname: "DelayedUse".to_owned(),
            by: Some(spawned),
        },
        FiredEvent::Removed {
            time: 2.0,
            entity: of_map(5, 4),
            classname: "info_null".to_owned(),
            by: Some(of_map(2, 1)),
        },
        FiredEvent::Spawned {
            time: 2.0,
            edict: Edict::new(40),
            classname: "DelayedUse".to_owned(),
        },
        FiredEvent::Print {
            time: 2.0,
            text: "open".to_owned(),
        },
        FiredEvent::Sound {
            time: 2.0,
            entity: of_map(3, 2),
            sample: "doors/open.wav".to_owned(),
        },
    ]
    .iter()
    .map(|event| {
        let (text, entity) = fired_line(event);
        (text, entity.map(EntityIndex::get))
    })
    .collect();
    assert_eq!(
        lines,
        [
            (
                "2.00  entity 1 looks for \"d\": nothing more".to_owned(),
                Some(1)
            ),
            (
                "2.00  entity 4 (info_null) has no use function: nothing happens".to_owned(),
                Some(4)
            ),
            ("2.00  edict 40 (DelayedUse) is removed".to_owned(), None),
            (
                "2.00  entity 4 (info_null) is removed by entity 1".to_owned(),
                Some(4)
            ),
            ("2.00  edict 40 is spawned (DelayedUse)".to_owned(), None),
            ("2.00  prints \"open\"".to_owned(), None),
            ("2.00  entity 2 plays doors/open.wav".to_owned(), Some(2)),
        ]
    );
    let mut stopped = trace();
    stopped.end = FireEnd::Error {
        message: "NULL function".to_owned(),
    };
    assert_eq!(
        fire_summary(EntityIndex::new(1), &stopped),
        "entity 1 fired by its use: the game stops: NULL function"
    );
}
