// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(clippy::unwrap_used, reason = "test code: a failed unwrap is a failed test")]

use super::{
    Bindings, K_AUX1, K_ENTER, K_F1, K_JOY1, K_MOUSE1, K_PAUSE, K_SHIFT, key_from_name, key_name,
};

fn words(line: &[&str]) -> Vec<Vec<u8>> {
    line.iter().map(|w| w.as_bytes().to_vec()).collect()
}

// spec INPUT-1.1: a character is its own number, a name any case from keynames, the runs of F,
// JOY and AUX keys by number; printable keys name themselves, the rest by keynames
#[test]
fn keys_have_names_and_numbers() {
    assert_eq!(key_from_name(b"w"), Some(b'w'));
    assert_eq!(key_from_name(b"enter"), Some(K_ENTER));
    assert_eq!(key_from_name(b"SHIFT"), Some(K_SHIFT));
    assert_eq!(key_from_name(b"SEMICOLON"), Some(b';'));
    assert_eq!(key_from_name(b"f12"), Some(K_F1 + 11));
    assert_eq!(key_from_name(b"F13"), None);
    assert_eq!(key_from_name(b"F01"), None);
    assert_eq!(key_from_name(b"JOY4"), Some(K_JOY1 + 3));
    assert_eq!(key_from_name(b"AUX32"), Some(K_AUX1 + 31));
    assert_eq!(key_from_name(b"MOUSE2"), Some(K_MOUSE1 + 1));
    assert_eq!(key_from_name(b""), None);
    assert_eq!(key_from_name(b"nokey"), None);
    assert_eq!(key_name(b'w'), "w");
    assert_eq!(key_name(b' '), "SPACE", "32 is not printed as itself");
    assert_eq!(key_name(K_F1 + 4), "F5");
    assert_eq!(key_name(K_AUX1), "AUX1");
    assert_eq!(key_name(K_PAUSE), "PAUSE");
    assert_eq!(key_name(1), "<UNKNOWN KEYNUM>");
}

// spec INPUT-1.2: bind sets, shows and refuses; unbind and unbindall leave empty bindings
#[test]
fn bind_unbind_and_unbindall() {
    let mut keys = Bindings::new();
    assert_eq!(
        keys.bind(&words(&["bind", "w"])).unwrap(),
        "\"w\" is not bound\n"
    );
    assert_eq!(keys.bind(&words(&["bind", "w", "+forward"])), None);
    assert_eq!(keys.binding(b'w'), Some(b"+forward".as_slice()));
    assert_eq!(
        keys.bind(&words(&["bind", "w"])).unwrap(),
        "\"w\" = \"+forward\"\n"
    );
    assert_eq!(
        keys.bind(&words(&["bind"])).unwrap(),
        "bind <key> [command] : attach a command to a key\n"
    );
    assert_eq!(
        keys.bind(&words(&["bind", "a", "b", "c"])).unwrap(),
        "bind <key> [command] : attach a command to a key\n"
    );
    assert_eq!(
        keys.bind(&words(&["bind", "nokey", "x"])).unwrap(),
        "\"nokey\" isn't a valid key\n"
    );
    assert_eq!(keys.unbind(&words(&["unbind", "w"])), None);
    assert_eq!(
        keys.binding(b'w'),
        Some(b"".as_slice()),
        "an empty binding stays"
    );
    assert_eq!(
        keys.unbind(&words(&["unbind"])).unwrap(),
        "unbind <key> : remove commands from a key\n"
    );
    assert_eq!(
        keys.unbind(&words(&["unbind", "nokey"])).unwrap(),
        "\"nokey\" isn't a valid key\n"
    );
    keys.bind(&words(&["bind", "SPACE", "+jump"]));
    keys.unbind_all();
    assert_eq!(keys.binding(b' '), Some(b"".as_slice()));
    assert_eq!(keys.binding(b'q'), None, "never bound stays unbound");
}

// spec INPUT-1.3: a + binding goes down with its key and up as -; another binding only down;
// an unbound key from 200 says so
#[test]
fn keys_go_down_and_up() {
    let mut keys = Bindings::new();
    keys.set(b'w', b"+forward");
    keys.set(b'1', b"impulse 1");
    keys.set(b'e', b"");
    assert_eq!(keys.down(b'w').unwrap(), b"+forward 119\n");
    assert_eq!(keys.up(b'w').unwrap(), b"-forward 119\n");
    assert_eq!(keys.down(b'1').unwrap(), b"impulse 1\n");
    assert_eq!(keys.up(b'1'), None);
    assert_eq!(keys.down(b'e'), None);
    assert_eq!(keys.down(b'q'), None);
    assert_eq!(
        keys.unbound_notice(K_MOUSE1 + 2).unwrap(),
        "MOUSE3 is unbound, hit F4 to set.\n"
    );
    assert_eq!(keys.unbound_notice(b'q'), None, "below 200");
    keys.set(K_MOUSE1, b"");
    assert_eq!(
        keys.unbound_notice(K_MOUSE1),
        None,
        "an empty binding is a binding"
    );
}

// spec INPUT-1.4: the first two keys, in number order, whose binding starts with the command
#[test]
fn keys_for_a_command() {
    let mut keys = Bindings::new();
    keys.set(b'w', b"+forward");
    keys.set(K_F1, b"+forward");
    keys.set(b'a', b"+forwardx");
    keys.set(b'z', b"+forward");
    assert_eq!(keys.keys_for(b"+forward"), [Some(b'a'), Some(b'w')]);
    assert_eq!(keys.keys_for(b"+jump"), [None, None]);
}
