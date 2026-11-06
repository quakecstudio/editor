// SPDX-License-Identifier: GPL-3.0-or-later

use super::StringBuffers;

// spec BUILTIN-18.39: buf_create takes the first free of 64 buffers, handles from 1, for the
// type `string` in any case only; buf_del frees one for reuse
#[test]
fn buffers_are_created_and_deleted() {
    let mut buffers = StringBuffers::default();
    assert_eq!(buffers.create(b"float"), -1.0);
    assert_eq!(buffers.create(b"String"), 1.0);
    assert_eq!(buffers.create(b"string"), 2.0);
    buffers.delete(0);
    assert_eq!(buffers.size(0), None);
    assert_eq!(buffers.create(b"string"), 1.0, "the freed slot is reused");
    for _ in 2..64 {
        assert!(buffers.create(b"string") > 0.0);
    }
    assert_eq!(buffers.create(b"string"), -1.0, "all 64 in use");
    buffers.delete(64);
    buffers.delete(u32::MAX);
}

// spec BUILTIN-18.39: bufstr_add puts a string at the end, or in the first hole when not
// ordered, and gives its index; bufstr_get reads it back cut to 1023 bytes, and nothing past
// the end; a buffer not in use has no size and takes nothing
#[test]
fn strings_are_added_and_read() {
    let mut buffers = StringBuffers::default();
    let handle = buffers.create(b"string");
    assert_eq!(handle, 1.0);
    assert_eq!(buffers.add(0, b"a", true), Some(0));
    assert_eq!(buffers.add(0, b"b", false), Some(1), "no hole: the end");
    assert_eq!(buffers.size(0), Some(2));
    assert_eq!(buffers.get(0, 1), Some(b"b".to_vec()));
    assert_eq!(buffers.get(0, 2), None);
    assert_eq!(buffers.get(5, 0), None);
    assert_eq!(buffers.add(5, b"x", true), None);
    assert_eq!(buffers.size(5), None);
    let long = [b'z'; 2000];
    assert_eq!(buffers.add(0, &long, true), Some(2));
    assert_eq!(buffers.get(0, 2).map(|t| t.len()), Some(1023));
}

// spec BUILTIN-18.39: an unordered add fills the first hole, a get of a hole is nothing, and
// an ordered add still goes on the end
#[test]
fn an_unordered_add_fills_a_hole() {
    let mut buffers = StringBuffers::default();
    assert_eq!(buffers.create(b"string"), 1.0);
    if let Some(slot) = buffers.buffers.get_mut(0) {
        *slot = Some(vec![Some(b"a".to_vec()), None, Some(b"c".to_vec())]);
    }
    assert_eq!(buffers.get(0, 1), None);
    assert_eq!(buffers.add(0, b"end", true), Some(3));
    assert_eq!(buffers.add(0, b"b", false), Some(1));
    assert_eq!(buffers.get(0, 1), Some(b"b".to_vec()));
}
