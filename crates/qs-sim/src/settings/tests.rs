// SPDX-License-Identifier: GPL-3.0-or-later

use super::{SERVER_SETTINGS, Setting};

// spec PHYS-12.1: every setting the server reads is one id registers, or one the engine has
#[test]
fn every_setting_read_is_registered() {
    let read = [
        Setting::MaxVelocity,
        Setting::Gravity,
        Setting::Friction,
        Setting::EdgeFriction,
        Setting::StopSpeed,
        Setting::MaxSpeed,
        Setting::Accelerate,
        Setting::IdealPitchScale,
        Setting::Aim,
        Setting::NoStep,
    ];
    for setting in read {
        assert!(
            SERVER_SETTINGS
                .iter()
                .any(|(name, _)| *name == setting.name()),
            "{setting:?}"
        );
    }
    assert_eq!(Setting::Teamplay.name(), "teamplay");
}
