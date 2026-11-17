// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::sync::OnceLock;

use super::{Flow, Leaves, Progress, View};
use crate::portals::PortalFile;
use crate::{Level, VisError};

// spec VIS-7.5: the portals a step before could see are all of them for the first step
#[test]
fn a_full_set_holds_each_of_its_count_and_no_more() {
    for count in [0, 1, 63, 64, 65, 130] {
        let all = Leaves::all(count);
        assert_eq!(all.count(), count);
        assert!((0..count).all(|at| all.has(at)));
        assert!(!all.has(count));
    }
}

// spec VIS-7.7: the flows after a long one do not wait for it, and wait for one that is not;
// a flow that failed stops those that wait
#[test]
fn flows_wait_for_those_before_them_but_not_for_long_ones() {
    let progress = Progress::new(8);
    progress.finish(0).unwrap();
    progress.finish(2).unwrap();
    // Place 1 is not flowed: nothing past it is settled.
    assert!(progress.wait_for(1, 1).is_ok());
    progress.declare_long(1);
    assert!(progress.is_long(1) && !progress.is_long(0));
    // Places 0 to 2 are flowed or long; only place 0 is flowed with every place before it.
    assert!(progress.wait_for(3, 1).is_ok());
    progress.finish(1).unwrap();
    assert!(progress.wait_for(3, 3).is_ok());
    progress.fail(&VisError::LeafRecursion);
    assert_eq!(progress.wait_for(3, 3), Err(VisError::LeafRecursion));
    assert_eq!(progress.wait_for(8, 8), Err(VisError::LeafRecursion));
}

/// Four leaves in a row along x, each joined to the next: portals 0 and 1 between leaves 0
/// and 1, there and back, 2 and 3 between leaves 1 and 2, 4 and 5 between leaves 2 and 3.
const ROW: &str = "PRT2\n4\n4\n3\n\
    4 0 1 (64 0 0 ) (64 64 0 ) (64 64 64 ) (64 0 64 ) \n\
    4 1 2 (128 0 0 ) (128 64 0 ) (128 64 64 ) (128 0 64 ) \n\
    4 2 3 (192 0 0 ) (192 64 0 ) (192 64 64 ) (192 0 64 ) \n\
    0 -1\n1 -1\n2 -1\n3 -1\n";

// spec VIS-7.7: a flow sees as flowed a portal before its limit, a long one only before its
// limit for long ones
#[test]
fn a_flow_sees_a_long_flow_only_from_farther_on() {
    let progress = Progress::new(4);
    progress.declare_long(1);
    let done: Vec<OnceLock<Leaves>> = (0..4).map(|_| OnceLock::from(Leaves::none(4))).collect();
    let rank = [0, 1, 2, 3];
    let view = |limit: usize, long_limit: usize| View {
        done: &done,
        rank: &rank,
        limit,
        own: 3,
        long: Some((&progress, long_limit)),
    };
    assert!(view(3, 0).sees(0) && view(3, 0).sees(2));
    assert!(!view(3, 0).sees(1), "long, and not yet far enough before");
    assert!(view(3, 2).sees(1), "long, and far enough before");
    assert!(!view(1, 2).sees(1), "not before the limit");
    assert!(view(3, 0).exact(0).is_some() && view(3, 0).exact(1).is_none());
}

// spec VIS-7.8: a leaf that a flowed portal of a leaf might have seen and did not, and that no
// other portal of that leaf sees, cannot see that leaf: the portals leaving it are noted not to
// see it, and a flow that sees the noting flow as flowed starts without it
#[test]
fn a_leaf_unseen_from_a_leaf_is_noted_not_to_see_it() {
    let file = PortalFile::read(ROW).unwrap();
    let flow = Flow::new(&file, Level::Two);
    let progress = Progress::new(6);
    // Portal 2 looks from leaf 1 into leaf 2 and might see leaves 2 and 3; say its flow saw
    // only leaf 2, and that portal 1, back into leaf 0, saw that.
    let seen = |leaf: usize| {
        let mut seen = Leaves::none(4);
        seen.add(leaf);
        OnceLock::from(seen)
    };
    let done = [
        OnceLock::new(),
        seen(0),
        seen(2),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
    ];
    let rank = [2, 0, 1, 3, 4, 5];
    let view = |limit: usize, own: usize| View {
        done: &done,
        rank: &rank,
        limit,
        own,
        long: Some((&progress, 0)),
    };
    assert!(flow.might[2].has(3));
    // Portal 0's flow, at place 2, ends: it looks into leaf 1, whose portals are 1 and 2.
    flow.portal_completed(0, view(2, 2), &progress).unwrap();
    // Leaf 3 cannot see leaf 1: portal 5, which leaves leaf 3, is noted not to see it.
    assert_eq!(*progress.unseen[5].lock().unwrap(), [(2, 1)]);
    for (other, list) in progress.unseen.iter().enumerate().take(5) {
        assert!(list.lock().unwrap().is_empty(), "{other}");
    }
    // A flow of portal 5 that sees place 2 as flowed starts without leaf 1 in its might-see
    // set, so it stops in leaf 2; one that does not see it goes on to leaves 1 and 0.
    progress.finish(2).unwrap();
    let seeing = flow.portal_flow(5, view(3, 5)).unwrap();
    let unseeing = flow.portal_flow(5, view(2, 5)).unwrap();
    assert!(seeing.seen.has(2) && !seeing.seen.has(1) && !seeing.seen.has(0));
    assert!(unseeing.seen.has(2) && unseeing.seen.has(1) && unseeing.seen.has(0));
}
