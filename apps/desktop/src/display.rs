// SPDX-License-Identifier: GPL-3.0-or-later

//! The system's display settings the app follows (P7-12): its light or dark appearance, and on
//! macOS the Accessibility > Display settings Increase contrast and Reduce motion, read from
//! `NSWorkspace` as Apple's Human Interface Guidelines, Accessibility, ask an app to follow
//! them. The appearance is followed each frame, as egui reports it; the accessibility settings
//! are read when the app starts and each time its window gains focus, as a person changes them
//! in System Settings and comes back.

use eframe::egui;

use crate::theme::{Contrast, Mode, Motion};

/// What the system asks the app's look to be.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Display {
    /// Light or dark.
    pub mode: Mode,
    /// The standard palette or the one with more contrast.
    pub contrast: Contrast,
    /// Whether animations run.
    pub motion: Motion,
}

impl Display {
    /// The system's settings now; dark, standard contrast and full motion where the system says
    /// nothing.
    #[must_use]
    pub fn read(ctx: &egui::Context) -> Self {
        let (contrast, motion) = accessibility();
        Self {
            mode: Self::mode(ctx),
            contrast,
            motion,
        }
    }

    /// The system's appearance, as egui reports it each frame; dark when it reports none.
    #[must_use]
    pub fn mode(ctx: &egui::Context) -> Mode {
        match ctx.system_theme() {
            Some(egui::Theme::Light) => Mode::Light,
            Some(egui::Theme::Dark) | None => Mode::Dark,
        }
    }
}

/// The Increase contrast and Reduce motion settings.
#[cfg(target_os = "macos")]
fn accessibility() -> (Contrast, Motion) {
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    let contrast = if workspace.accessibilityDisplayShouldIncreaseContrast() {
        Contrast::Increased
    } else {
        Contrast::Standard
    };
    let motion = if workspace.accessibilityDisplayShouldReduceMotion() {
        Motion::Reduced
    } else {
        Motion::Full
    };
    (contrast, motion)
}

/// Off macOS the app reads neither setting yet (L-16, the Windows and Linux checks).
#[cfg(not(target_os = "macos"))]
const fn accessibility() -> (Contrast, Motion) {
    (Contrast::Standard, Motion::Full)
}
