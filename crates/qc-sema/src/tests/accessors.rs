// SPDX-License-Identifier: GPL-3.0-or-later

//! The index's accessors give what the resolver recorded, which dependents read through them.

use super::fte_index_of as index;
use crate::resolve::ModuleIndex;
use crate::symbol::DefId;

/// The definition named `name`, found through [`ModuleIndex::definitions`].
fn def(index: &ModuleIndex, name: &str) -> DefId {
    index
        .definitions()
        .find(|(_, d)| d.name == name)
        .map(|(id, _)| id)
        .unwrap()
}

const SOURCE: &str = "float a[3];\nconst float c = 2;\nvar float v = 1;\nfloat e = c + 1;\n\
                      void() f;\nvoid() f = { local float x; x = c; };\nfloat bad = nothing;\n";

// No spec statement: the accessors read the index's own tables, so a dependent sees what the
// resolver recorded.
#[test]
fn the_index_is_read_through_its_accessors() {
    let i = index(SOURCE);
    assert!(!i.defs().is_empty());
    assert_eq!(i.defs(), &i.defs[..]);
    let pairs: Vec<_> = i
        .definitions()
        .map(|(id, d)| (id.index(), d.clone()))
        .collect();
    let expected: Vec<_> = i.defs.iter().cloned().enumerate().collect();
    assert_eq!(pairs, expected);
    assert!(!i.refs().is_empty());
    assert_eq!(i.refs(), &i.refs[..]);
    assert!(!i.redeclarations().is_empty());
    assert_eq!(i.redeclarations(), &i.redeclarations[..]);
    assert!(!i.diagnostics().is_empty());
    assert_eq!(i.diagnostics(), &i.diagnostics[..]);

    let f = def(&i, "f");
    assert!(i.body(f).is_some());
    assert_eq!(i.body(f), i.bodies.get(&f));
    assert_eq!(i.body(def(&i, "c")), None);
    let e = def(&i, "e");
    assert!(i.initial_value(e).is_some());
    assert_eq!(i.initial_value(e), i.initial_values.get(&e));
    assert_eq!(i.array_len(def(&i, "a")), Some(3));
    assert!(i.is_constant(def(&i, "c")));
    assert!(!i.is_constant(def(&i, "v")));
}
