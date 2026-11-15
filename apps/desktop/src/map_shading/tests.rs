// SPDX-License-Identifier: GPL-3.0-or-later

use qs_layout::MapShading;

use super::{LitState, ShadedView, shade_of};
use crate::strings::Text;

// No spec statement: P7-13: floors and ceilings are drawn brightest, walls facing x darker and
// walls facing y darker again, so neighbouring faces read apart without a light
#[test]
fn faces_are_shaded_by_the_way_they_face() {
    let up = shade_of([0.0, 0.0, 1.0]).get();
    let down = shade_of([0.0, 0.0, -1.0]).get();
    let x = shade_of([1.0, 0.0, 0.0]).get();
    let y = shade_of([0.0, -1.0, 0.0]).get();
    assert_eq!((up, down), (255, 255));
    assert!(up > x && x > y, "{up} {x} {y}");
    assert_eq!(y, 209);
    assert_eq!(x, 232);
}

// No spec statement: P7-13: Lit says when there is no compile yet or the map changed since,
// and nothing in the other shadings; no faces are drawn before a mesh is made
#[test]
fn lit_says_when_its_compile_is_missing_or_old() {
    let mut view = ShadedView::default();
    assert_eq!(view.shading, MapShading::Lines);
    view.lit = Some(LitState::NoCompile);
    assert_eq!(view.note(), None);
    view.shading = MapShading::Lit;
    assert_eq!(view.note(), Some(Text::MapLitNone));
    view.lit = Some(LitState::Stale);
    assert_eq!(view.note(), Some(Text::MapLitStale));
    view.lit = Some(LitState::Current);
    assert_eq!(view.note(), None);
    assert!(!view.draws_faces());
    view.shading = MapShading::Textured;
    assert!(!view.draws_faces());
}
