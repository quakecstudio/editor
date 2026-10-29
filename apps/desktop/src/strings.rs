// SPDX-License-Identifier: GPL-3.0-or-later

//! Every string the app shows (CONV-16): the UI code names a [`Text`], never a literal, so the
//! wording is reviewed in one place and can be translated later. A test fails on a string
//! literal in the UI modules.

/// One string the app shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Text {
    /// The app's name, in the title bar and the macOS app menu (ADR-0006).
    AppName,
    /// The toolbar's Play button.
    Play,
    /// The toolbar's Pause button.
    Pause,
    /// The toolbar's Step button.
    Step,
    /// The toolbar's Restart map button.
    RestartMap,
    /// The preview host's name in the toolbar.
    PreviewHost,
    /// Where the preview host's window is.
    PreviewDocked,
    /// The command search box's hint.
    SearchCommands,
    /// The Project panel's heading.
    Project,
    /// The Entities panel's heading.
    Entities,
    /// The Properties panel's heading.
    Properties,
    /// The agent panel's heading.
    Agent,
    /// The Problems tab.
    Problems,
    /// The Console tab.
    Console,
    /// The Debugger tab.
    Debugger,
    /// The editor tab in the centre.
    Editor,
    /// The game tab in the centre.
    Game,
    /// The Project panel with no project open.
    NoProject,
    /// The Entities and Properties panels while the game is not running.
    NotPlaying,
    /// The agent panel before an agent is set up.
    NoAgent,
    /// The Problems tab with nothing to report.
    NoProblems,
    /// The Console tab before the preview host has started.
    ConsoleIdle,
    /// The Debugger tab while nothing is paused.
    DebuggerIdle,
    /// The editor with no file open.
    NoFileOpen,
    /// The game view while the preview host is not running.
    GameStopped,
    /// The status bar while the preview host is stopped.
    PreviewStopped,
    /// A control that has no action yet.
    NotYetAvailable,
    /// The View menu.
    ViewMenu,
    /// The command that shows or hides the left panel.
    ToggleLeftPanel,
    /// The command that shows or hides the right panel.
    ToggleRightPanel,
    /// The command that shows or hides the bottom panel.
    ToggleBottomPanel,
    /// On standard error, when the log cannot be opened.
    LoggingOff,
}

impl Text {
    /// Every string, in the order above.
    pub const ALL: [Self; 32] = [
        Self::AppName,
        Self::Play,
        Self::Pause,
        Self::Step,
        Self::RestartMap,
        Self::PreviewHost,
        Self::PreviewDocked,
        Self::SearchCommands,
        Self::Project,
        Self::Entities,
        Self::Properties,
        Self::Agent,
        Self::Problems,
        Self::Console,
        Self::Debugger,
        Self::Editor,
        Self::Game,
        Self::NoProject,
        Self::NotPlaying,
        Self::NoAgent,
        Self::NoProblems,
        Self::ConsoleIdle,
        Self::DebuggerIdle,
        Self::NoFileOpen,
        Self::GameStopped,
        Self::PreviewStopped,
        Self::NotYetAvailable,
        Self::ViewMenu,
        Self::ToggleLeftPanel,
        Self::ToggleRightPanel,
        Self::ToggleBottomPanel,
        Self::LoggingOff,
    ];

    /// The string, in English.
    #[must_use]
    pub const fn get(self) -> &'static str {
        match self {
            Self::AppName => "QuakeC Studio",
            Self::Play => "Play",
            Self::Pause => "Pause",
            Self::Step => "Step",
            Self::RestartMap => "Restart map",
            Self::PreviewHost => "Preview host",
            Self::PreviewDocked => "docked right",
            Self::SearchCommands => "Search commands",
            Self::Project => "Project",
            Self::Entities => "Entities",
            Self::Properties => "Properties",
            Self::Agent => "Agent",
            Self::Problems => "Problems",
            Self::Console => "Console",
            Self::Debugger => "Debugger",
            Self::Editor => "Editor",
            Self::Game => "Game",
            Self::NoProject => "No project open.",
            Self::NotPlaying => "Entities appear here while the game runs.",
            Self::NoAgent => "The agent is not set up yet.",
            Self::NoProblems => "No problems.",
            Self::ConsoleIdle => "The preview host's console appears here once it starts.",
            Self::DebuggerIdle => "Nothing is paused.",
            Self::NoFileOpen => "Open a file from the Project panel.",
            Self::GameStopped => "The preview host is not running.",
            Self::PreviewStopped => "Preview host stopped",
            Self::NotYetAvailable => "Not available yet",
            Self::ViewMenu => "View",
            Self::ToggleLeftPanel => "Project and Entities",
            Self::ToggleRightPanel => "Properties and Agent",
            Self::ToggleBottomPanel => "Problems, Console and Debugger",
            Self::LoggingOff => "logging is off",
        }
    }
}
