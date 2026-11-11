// SPDX-License-Identifier: GPL-3.0-or-later

//! The client's settings as the host reads them (CON-7.4).

use std::collections::HashMap;

use qs_client::ViewSettings;
use qs_render::Look;

use super::HostSettings;

/// id's defaults for the four settings the host reads (CON-7.1: `viewsize` 100, `gamma` 1,
/// `volume` 0.7, `bgmvolume` 1), with `changed` over them, read in `look`.
fn read(changed: &[(&str, f32)], look: Look) -> HostSettings {
    let mut values: HashMap<&str, f32> = [
        ("viewsize", 100.0),
        ("gamma", 1.0),
        ("volume", 0.7),
        ("bgmvolume", 1.0),
    ]
    .into_iter()
    .collect();
    values.extend(changed.iter().copied());
    HostSettings::read(
        |name| values.get(name).copied(),
        ViewSettings::default(),
        look,
    )
}

// spec CON-7.4: each setting the options menu changes and the host reads in the frame changes
// what the host does with it, and only that: `viewsize` the view (VIEW-3.6), `gamma` the whole
// frame in QuakeSpasm's look (REND-13.9), `volume` the mixer (SND-5.4), `bgmvolume` the music
// (SND-6.2)
#[test]
fn each_option_reaches_what_the_host_does() {
    let base = read(&[], Look::QuakeSpasm);
    assert_eq!(base.view.viewsize, 100.0);
    assert_eq!((base.frame_gamma, base.view.gamma), (1.0, 1.0));
    assert_eq!((base.volume, base.music), (0.7, 1.0));

    let smaller = read(&[("viewsize", 30.0)], Look::QuakeSpasm);
    assert_eq!(smaller.view.viewsize, 30.0);
    assert_eq!(
        HostSettings {
            view: base.view,
            ..smaller
        },
        base
    );

    let brighter = read(&[("gamma", 0.5)], Look::QuakeSpasm);
    assert_eq!(brighter.frame_gamma, 0.5);
    // QuakeSpasm's look corrects the frame, not the blend table.
    assert_eq!(brighter.view.gamma, 1.0);
    assert_eq!(
        HostSettings {
            frame_gamma: 1.0,
            ..brighter
        },
        base
    );

    let quiet = read(&[("volume", 0.0)], Look::QuakeSpasm);
    assert_eq!(quiet.volume, 0.0);
    assert_eq!(
        HostSettings {
            volume: 0.7,
            ..quiet
        },
        base
    );

    let no_music = read(&[("bgmvolume", 0.0)], Look::QuakeSpasm);
    assert_eq!(no_music.music, 0.0);
    assert_eq!(
        HostSettings {
            music: 1.0,
            ..no_music
        },
        base
    );
}

// spec CON-7.4: in GLQuake's look `gamma` builds the blend table (VIEW-4.7); the view keeps the
// client's other settings; a setting the world lacks reads as 0
#[test]
fn gamma_goes_to_the_blend_table_in_glquakes_look() {
    let brighter = read(&[("gamma", 0.6)], Look::GlQuake);
    assert_eq!((brighter.frame_gamma, brighter.view.gamma), (0.6, 0.6));
    let client = ViewSettings {
        idlescale: 2.5,
        offsets: [1.0, 2.0, 3.0],
        cshiftpercent: 50.0,
        ..ViewSettings::default()
    };
    let kept = HostSettings::read(|_| Some(0.8), client, Look::QuakeSpasm).view;
    assert_eq!(
        (kept.idlescale, kept.offsets, kept.cshiftpercent),
        (2.5, [1.0, 2.0, 3.0], 50.0)
    );
    let none = HostSettings::read(|_| None, client, Look::GlQuake);
    assert_eq!(
        (none.view.viewsize, none.view.gamma, none.frame_gamma),
        (0.0, 0.0, 0.0)
    );
    assert_eq!((none.volume, none.music), (0.0, 0.0));
}
