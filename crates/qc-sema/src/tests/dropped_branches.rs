// SPDX-License-Identifier: GPL-3.0-or-later

//! What the body keeps of the statements a constant condition drops (FTE-3.6): the effects
//! they made, in statement order, and the `case` labels at or before the point dropped to;
//! and the values an emulation gives back unchanged (FTEF-27.12).

use qc_preproc::Op;

use super::fte_index_of;
use crate::ir::{Effect, Name};
use crate::{DefId, ModuleIndex, TypedBody};

/// The checked body of `name` in `index`.
fn body<'i>(index: &'i ModuleIndex, name: &str) -> &'i TypedBody {
    let id = index
        .defs
        .iter()
        .position(|d| d.name == name)
        .map(DefId::new)
        .unwrap();
    &index.bodies[&id]
}

// spec FTE-3.6: the effects of a dropped branch, a `for` loop's bounds among its statements'
// own, are all made before the statement after it, in the order of the statements they came
// before
#[test]
fn a_dropped_branchs_effects_keep_their_order() {
    let index = fte_index_of(
        "float arr[4]; float d, g;\n\
         void() t = { if (0) { for (d = 0; d < 3; d++) { g = g * 2 + d; } arr[d & 3] = d; } \
         g = 1; };\n",
    );
    let t = body(&index, "t");
    assert!(t.followed());
    let effects = t.effects();
    assert!(
        effects.windows(2).all(|w| w[0].0 <= w[1].0),
        "in position order: {effects:?}"
    );
    let at = |found: &dyn Fn(&Effect) -> bool| effects.iter().position(|(_, e)| found(e)).unwrap();
    let begin = at(&|e| *e == Effect::ForBegin);
    let end = at(&|e| *e == Effect::ForEnd);
    let temp = at(&|e| matches!(e, Effect::Temp(_)));
    let setter = at(&|e| matches!(e, Effect::Make(Name::ArraySet(_))));
    assert!(begin < temp && temp < end && end < setter, "{effects:?}");
    assert_eq!(
        t.generated().first().map(|s| s.op),
        Some(Op::StoreF),
        "the branch is dropped"
    );
}

// spec FTE-3.6: a `case` label at or before the statement a dropped `if` rewinds to is kept,
// whether its first statement is the `if` (DIFF-14.10) or a comparison before it
#[test]
fn a_dropped_ifs_case_keeps_its_label() {
    let index = fte_index_of(
        "float d, g; vector a, b;\n\
         void() t = { switch (d) { case 1: g = 1; break; case 2: if (0) { g = 3; } g = 2; \
         break; } };\n\
         void() u = { switch (d) { case 1: g = 1; break; case 2: if ((a == b) && 0) { g = 3; } \
         g = 2; break; } };\n",
    );
    for name in ["t", "u"] {
        let tests = body(&index, name)
            .generated()
            .iter()
            .filter(|s| s.op == Op::EqF)
            .count();
        assert_eq!(tests, 2, "{name}: a test for each case");
    }
}

// spec FTEF-34.16: an `IFNOT` of a `NOT` drops the `NOT`, whose temporary is still taken
#[test]
fn a_dropped_not_still_takes_its_temporary() {
    let index = fte_index_of("float d, g;\nvoid() t = { if (!d) g = 1; };\n");
    let t = body(&index, "t");
    assert_eq!(t.generated().first().map(|s| s.op), Some(Op::IfI));
    assert!(
        t.effects().contains(&(0, Effect::Temp(1))),
        "{:?}",
        t.effects()
    );
}

// spec FTEF-27.12: `1i * f` is emulated as the float multiply, which gives `f` back: an
// element the setter stores is stored from the `?:` value itself
#[test]
fn an_emulation_that_gives_its_operand_back_is_that_value() {
    let index = fte_index_of(
        "enum { EN_A };\nfloat arr[4]; float d, g;\n\
         void() t = { arr[d & 3] = ((495 || EN_A) * (g ? 3 : d)); };\n",
    );
    let t = body(&index, "t");
    assert!(t.followed());
    let generated = t.generated();
    let arm = generated
        .iter()
        .find(|s| s.op == Op::StoreF && matches!(s.b, Some(Name::Made(_))))
        .and_then(|s| s.b);
    let stored = generated
        .iter()
        .find(|s| s.b == Some(Name::Parameter(1)))
        .and_then(|s| s.a);
    assert!(arm.is_some());
    assert_eq!(stored, arm, "{generated:?}");
}
