// SPDX-License-Identifier: GPL-3.0-or-later

use super::{ClientSlot, Clients, MaxPlayers};

// spec NET-7.4: a new client takes the first slot not active
// spec BUILTIN-18.45: spawnclient takes the last slot not active
#[test]
fn free_slots_are_found_from_each_end() {
    let mut clients = Clients::new(MaxPlayers::new(3));
    assert_eq!(clients.free().map(super::ClientId::slot), Some(0));
    assert_eq!(clients.free_from_last().map(super::ClientId::slot), Some(2));
    for slot in clients.iter_mut() {
        slot.active = true;
    }
    assert_eq!(clients.free(), None);
    assert_eq!(clients.free_from_last(), None);
    assert_eq!(clients.active_count(), 3);
}

// spec NET-7.6: ping is the mean of the last 16 ping times in milliseconds, truncated
#[test]
fn the_mean_ping_is_of_the_last_sixteen() {
    let mut slot = ClientSlot::default();
    assert_eq!(slot.mean_ping_ms(), 0);
    slot.last_time = 1.0;
    slot.record_ping(1.05);
    // One of 16 is 0.05: 50 / 16 ms, truncated.
    assert_eq!(slot.mean_ping_ms(), 3);
    for _ in 0..20 {
        slot.record_ping(1.1);
    }
    // The 21st replaced the first; every one is 0.1 s, up to float rounding.
    assert!(
        (99..=100).contains(&slot.mean_ping_ms()),
        "{}",
        slot.mean_ping_ms()
    );
}
