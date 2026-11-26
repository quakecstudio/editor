// SPDX-License-Identifier: GPL-3.0-or-later

use super::{Offer, Suggest};

/// The offers `gate`, `Gate2` and `light1`, each naming its class.
fn offers() -> Vec<Offer> {
    [
        ("gate", "func_door"),
        ("Gate2", "func_door"),
        ("light1", "light"),
    ]
    .into_iter()
    .map(|(name, detail)| Offer {
        name: name.to_owned(),
        detail: detail.to_owned(),
    })
    .collect()
}

/// The names `typed` matches among [`offers`].
fn matching(typed: &str) -> Vec<String> {
    let offers = offers();
    let field = Suggest {
        name: "target",
        offers: &offers,
        width: 0.0,
    };
    field
        .matching(typed)
        .into_iter()
        .map(|offer| offer.name.clone())
        .collect()
}

// No spec statement: UI-24: an empty field, or one holding an offer exactly, offers every
// value, so a value set already shows what else it may be; text typed narrows the list to
// the offers containing it in any case, and text no offer contains offers none
#[test]
fn typing_narrows_the_offers() {
    assert_eq!(matching(""), ["gate", "Gate2", "light1"]);
    assert_eq!(matching(" gate "), ["gate", "Gate2", "light1"]);
    assert_eq!(matching("GAT"), ["gate", "Gate2"]);
    assert_eq!(matching("t1"), ["light1"]);
    assert!(matching("door").is_empty());
}
