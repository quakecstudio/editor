// SPDX-License-Identifier: GPL-3.0-or-later

//! The Lucide icons the app draws (ADR-0010), as characters of the bundled `lucide.ttf`,
//! release 1.57.0, whose `codepoints.json` gives each one.

/// One icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// `play`.
    Play,
    /// `pause`.
    Pause,
    /// `step-forward`.
    Step,
    /// `rotate-ccw`.
    Restart,
    /// `search`.
    Search,
    /// `panel-left`.
    PanelLeft,
    /// `panel-right`.
    PanelRight,
    /// `panel-bottom`.
    PanelBottom,
    /// `circle-x`.
    Error,
    /// `triangle-alert`.
    Warning,
}

impl Icon {
    /// Every icon.
    pub const ALL: [Self; 10] = [
        Self::Play,
        Self::Pause,
        Self::Step,
        Self::Restart,
        Self::Search,
        Self::PanelLeft,
        Self::PanelRight,
        Self::PanelBottom,
        Self::Error,
        Self::Warning,
    ];

    /// The icon's character in `lucide.ttf`.
    #[must_use]
    pub const fn char(self) -> char {
        match self {
            Self::Play => '\u{e13c}',
            Self::Pause => '\u{e12e}',
            Self::Step => '\u{e3ea}',
            Self::Restart => '\u{e148}',
            Self::Search => '\u{e151}',
            Self::PanelLeft => '\u{e12a}',
            Self::PanelRight => '\u{e431}',
            Self::PanelBottom => '\u{e42c}',
            Self::Error => '\u{e084}',
            Self::Warning => '\u{e193}',
        }
    }
}
