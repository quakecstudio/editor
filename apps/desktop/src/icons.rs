// SPDX-License-Identifier: GPL-3.0-or-later

//! The Lucide icons the app draws (ADR-0010), as characters of the bundled `lucide.ttf`,
//! release 1.57.0, whose `codepoints.json` gives each one.

use eframe::egui::{Button, Response, Ui};

use crate::shell::name_widget;
use crate::strings::{Text, icon_hint};

/// Whether an icon button is the one chosen, as Zed's `IconButton::toggle_state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Toggled {
    /// Chosen: drawn framed.
    On,
    /// Not chosen: bare until hovered.
    Off,
}

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
    /// `mouse-pointer-2`: the map editor's Select tool.
    Select,
    /// `box`: the Brush tool.
    Brush,
    /// `scissors`: the Clip tool.
    Clip,
    /// `circle-dot`: the Vertex tool.
    Vertex,
    /// `move`: the Move tool.
    Move,
    /// `ruler`: the Measure tool.
    Measure,
    /// `shapes`: the Shape tool.
    Shape,
    /// `hexagon`: the Hull tool.
    Hull,
    /// `square-dashed`: the 3D view drawn as lines.
    Lines,
    /// `image`: the 3D view drawn textured.
    Textured,
    /// `sun`: the 3D view drawn lit.
    Lit,
    /// `link`: every entity's links drawn.
    Link,
    /// `hash`: the grid.
    Grid,
    /// `eye`: what the view shows.
    Show,
    /// `folder`: the Project panel.
    Project,
    /// `list-tree`: the Symbols panel.
    Symbols,
    /// `boxes`: the Entities panel.
    Entities,
    /// `map`: the Map panel.
    MapOutline,
    /// `sliders-horizontal`: the Properties panel.
    Properties,
    /// `sparkles`: the Agent panel.
    Agent,
    /// `circle-alert`: the Problems panel.
    Problems,
    /// `scroll-text`: the Console panel.
    Console,
    /// `bug`: the Debugger panel.
    Debugger,
    /// `terminal`: the Terminal panel.
    Terminal,
    /// `git-branch`: the Git panel.
    Git,
}

impl Icon {
    /// Every icon.
    pub const ALL: [Self; 35] = [
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
        Self::Select,
        Self::Brush,
        Self::Clip,
        Self::Vertex,
        Self::Move,
        Self::Measure,
        Self::Shape,
        Self::Hull,
        Self::Lines,
        Self::Textured,
        Self::Lit,
        Self::Link,
        Self::Grid,
        Self::Show,
        Self::Project,
        Self::Symbols,
        Self::Entities,
        Self::MapOutline,
        Self::Properties,
        Self::Agent,
        Self::Problems,
        Self::Console,
        Self::Debugger,
        Self::Terminal,
        Self::Git,
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
            Self::Select => '\u{e1c3}',
            Self::Brush => '\u{e061}',
            Self::Clip => '\u{e14e}',
            Self::Vertex => '\u{e345}',
            Self::Move => '\u{e121}',
            Self::Measure => '\u{e14b}',
            Self::Shape => '\u{e4b3}',
            Self::Hull => '\u{e0f3}',
            Self::Lines => '\u{e1cb}',
            Self::Textured => '\u{e0f6}',
            Self::Lit => '\u{e178}',
            Self::Link => '\u{e102}',
            Self::Grid => '\u{e0ef}',
            Self::Show => '\u{e0ba}',
            Self::Project => '\u{e0d7}',
            Self::Symbols => '\u{e408}',
            Self::Entities => '\u{e2d0}',
            Self::MapOutline => '\u{e110}',
            Self::Properties => '\u{e29a}',
            Self::Agent => '\u{e412}',
            Self::Problems => '\u{e077}',
            Self::Console => '\u{e45f}',
            Self::Debugger => '\u{e20c}',
            Self::Terminal => '\u{e181}',
            Self::Git => '\u{e0e2}',
        }
    }

    /// A button of this icon alone, framed when `state` is on, as Zed's `IconButton` with its
    /// toggle state: named `name` for VoiceOver, and on hover its name and `detail`, what it
    /// does (P7-15).
    pub(crate) fn toggle(self, ui: &mut Ui, state: Toggled, name: Text, detail: Text) -> Response {
        self.toggle_named(ui, state, name.get(), detail.get())
    }

    /// As [`Icon::toggle`], for a name made of others, such as a panel's button's.
    pub(crate) fn toggle_named(
        self,
        ui: &mut Ui,
        state: Toggled,
        name: &str,
        detail: &str,
    ) -> Response {
        // egui's selectable button: framed when selected or hovered, bare otherwise.
        let button = Button::selectable(state == Toggled::On, self.char().to_string());
        let response = ui.add(button).on_hover_text(icon_hint(name, detail));
        name_widget(ui, &response, name);
        response
    }
}
