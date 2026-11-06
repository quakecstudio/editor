// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::expect_used,
    reason = "test code: a failed expect is a failed test"
)]

use super::{
    ClientCommand, ClientInfo, Clients, Decoded, SERVER_INFO_STRING_SIZE, atoi, info_get_key,
    info_remove_key, info_set_key,
};

// spec BUILTIN-18.27: Info_GetKey finds a key's value, at most size - 1 bytes of it, and
// nothing for a missing key or a malformed string
#[test]
fn info_get_key_reads_a_value() {
    let info = b"\\name\\player\\topcolor\\4";
    assert_eq!(info_get_key(info, b"name", 64), b"player");
    assert_eq!(info_get_key(info, b"topcolor", 64), b"4");
    assert_eq!(
        info_get_key(info, b"top", 64),
        b"",
        "a prefix is not the key"
    );
    assert_eq!(info_get_key(info, b"bottomcolor", 64), b"");
    assert_eq!(
        info_get_key(info, b"name", 4),
        b"pla",
        "three bytes and the zero"
    );
    assert_eq!(
        info_get_key(b"name\\player", b"name", 64),
        b"",
        "no leading backslash"
    );
    assert_eq!(
        info_get_key(b"\\name", b"x", 64),
        b"",
        "a key with no value ends it"
    );
    assert_eq!(info_get_key(b"", b"name", 64), b"");
}

// spec BUILTIN-18.27: Info_RemoveKey takes out the first pair of the key, and Info_SetKey
// appends `\key\value`, sets nothing for an empty value, an empty key or a backslash, and
// leaves the string as it was when the pair would not fit
#[test]
fn info_set_key_replaces_or_refuses() {
    let mut info = b"\\a\\1\\b\\2".to_vec();
    info_remove_key(&mut info, b"a");
    assert_eq!(info, b"\\b\\2");
    info_remove_key(&mut info, b"z");
    assert_eq!(info, b"\\b\\2", "a missing key changes nothing");
    info_set_key(&mut info, 64, b"b", b"3");
    assert_eq!(info, b"\\b\\3");
    info_set_key(&mut info, 64, b"c", b"4");
    assert_eq!(info, b"\\b\\3\\c\\4");
    info_set_key(&mut info, 64, b"c", b"");
    assert_eq!(info, b"\\b\\3", "an empty value removes the key");
    for (key, value) in [
        (b"".as_slice(), b"1".as_slice()),
        (b"k\\", b"1"),
        (b"k", b"1\\"),
    ] {
        let mut info = b"\\b\\3".to_vec();
        info_set_key(&mut info, 64, key, value);
        assert_eq!(info, b"\\b\\3", "{key:?} {value:?}");
    }
    // `\b\3` is four bytes; `\c\` and a value of n bytes need 3 + n more, and the total must
    // be below size - 1.
    let mut fits = b"\\b\\3".to_vec();
    info_set_key(&mut fits, 11, b"c", b"12");
    assert_eq!(fits, b"\\b\\3\\c\\12", "8 + 1 of 11");
    let mut full = b"\\b\\3".to_vec();
    info_set_key(&mut full, 10, b"c", b"12");
    assert_eq!(full, b"\\b\\3", "8 + 1 is not below 10 - 1");
    let mut info = b"\\b\\3".to_vec();
    info_set_key(&mut info, SERVER_INFO_STRING_SIZE, b"b", b"4");
    assert_eq!(info, b"\\b\\4");
}

// spec BUILTIN-18.27: a client enters named `player` with colours 0
// spec BUILTIN-18.27: SV_DecodeUserInfo: each colour is atoi of its text masked to 15 and at
// most 13, team is the bottom colour plus 1, and the name is set only when it changed
#[test]
fn decoding_sets_team_colours_and_name() {
    let mut client = ClientInfo::default();
    assert_eq!(client.name(), b"player");
    assert_eq!(client.colors(), 0);
    assert_eq!(client.update(b"bottomcolor", b"0"), None, "unchanged");
    assert_eq!(
        client.update(b"bottomcolor", b"4"),
        Some(Decoded {
            team: 5.0,
            netname: None,
        })
    );
    assert_eq!(client.colors(), 4);
    assert_eq!(client.update(b"topcolor", b"30").map(|d| d.team), Some(5.0));
    assert_eq!(client.colors(), (13 << 4) | 4, "30 & 15 is 14, then 13");
    assert_eq!(
        client.update(b"bottomcolor", b"-1").map(|d| d.team),
        Some(14.0)
    );
    assert_eq!(client.colors() & 15, 13, "-1 & 15 is 15, then 13");
    assert_eq!(
        client.update(b"name", b"other"),
        Some(Decoded {
            team: 14.0,
            netname: Some(b"other".to_vec()),
        })
    );
    assert_eq!(client.name(), b"other");
    assert!(client.userinfo().ends_with(b"\\name\\other"));
}

// spec BUILTIN-18.27: an empty name is `unnamed`, and a name is cut to 31 bytes, so a longer
// one differs from the kept name at every decode and is set again
#[test]
fn names_are_unnamed_or_cut() {
    let mut client = ClientInfo::default();
    let decoded = client.update(b"name", b"").expect("a change");
    assert_eq!(decoded.netname, Some(b"unnamed".to_vec()));
    let long = [b'n'; 40];
    let decoded = client.update(b"name", &long).expect("a change");
    assert_eq!(decoded.netname, Some(vec![b'n'; 31]));
    let again = client.update(b"topcolor", b"1").expect("a change");
    assert_eq!(
        again.netname,
        Some(vec![b'n'; 31]),
        "40 bytes never match 31"
    );
}

// spec BUILTIN-18.27: each client slot has its own info, the default until it changes
#[test]
fn clients_keep_their_own_info() {
    let mut clients = Clients::default();
    assert_eq!(clients.get(3), ClientInfo::default());
    let _ = clients.get_mut(1).update(b"bottomcolor", b"2");
    assert_eq!(clients.get(1).colors(), 2);
    assert_eq!(clients.get(0).colors(), 0);
}

// spec BUILTIN-18.28: from a client only setinfo with two arguments and a key not starting
// with `*` changes anything; the command is matched without case, its words read up to a line
// feed; every other command, `color` among them, does nothing
#[test]
fn client_commands_run_only_setinfo() {
    let setinfo = |key: &[u8], value: &[u8]| ClientCommand::Setinfo {
        key: key.to_vec(),
        value: value.to_vec(),
    };
    assert_eq!(
        ClientCommand::parse(b"setinfo bottomcolor 4"),
        setinfo(b"bottomcolor", b"4")
    );
    assert_eq!(
        ClientCommand::parse(b"  SetInfo \"name\" \"a b\""),
        setinfo(b"name", b"a b")
    );
    assert_eq!(
        ClientCommand::parse(b"setinfo a 1\nsetinfo b 2"),
        setinfo(b"a", b"1"),
        "a line feed ends the command"
    );
    for text in [
        b"".as_slice(),
        b"   ",
        b"\nsetinfo a 1",
        b"setinfo a",
        b"setinfo a 1 2",
        b"setinfo *ip 1",
        b"color 4",
        b"serverinfo",
        b"say hi",
        b"setinfos a 1",
    ] {
        assert_eq!(
            ClientCommand::parse(text),
            ClientCommand::Nothing,
            "{text:?}"
        );
    }
}

// spec BUILTIN-18.27: atoi skips white space and reads a sign and digits, and a value past a
// long is cut to an int's low 32 bits
#[test]
fn atoi_reads_as_glibc() {
    assert_eq!(atoi(b"  12x"), 12);
    assert_eq!(atoi(b"-7"), -7);
    assert_eq!(atoi(b"+3"), 3);
    assert_eq!(atoi(b"x1"), 0);
    assert_eq!(atoi(b""), 0);
    assert_eq!(atoi(b"4294967297"), 1, "2^32 + 1 keeps its low bits");
    assert_eq!(atoi(b"99999999999999999999"), -1, "LONG_MAX's low 32 bits");
    assert_eq!(atoi(b"-99999999999999999999"), 0, "LONG_MIN's low 32 bits");
}
