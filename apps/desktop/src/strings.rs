// SPDX-License-Identifier: GPL-3.0-or-later

//! Every string the app shows (CONV-16): the UI code names a [`Text`], never a literal, so the
//! wording is reviewed in one place and can be translated later. A test fails on a string
//! literal in the UI modules.

/// One string the app shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    /// Where the game's window is when its host cannot place it (Wayland).
    PreviewOwnWindow,
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
    /// The label over an inline edit's request (P4-10).
    InlineEditPrompt,
    /// The agent panel's button that opens the provider form.
    AgentSettings,
    /// The hint in the agent's task box.
    AgentTaskHint,
    /// The button that starts a thread on the task.
    AgentRun,
    /// Starts an agent thread about an entity, its state attached (UI-9).
    AskAboutThis,
    /// The Anthropic provider in the agent's form.
    ProviderAnthropic,
    /// An OpenAI-compatible endpoint in the agent's form.
    ProviderOpenAi,
    /// A local Ollama in the agent's form.
    ProviderOllama,
    /// The model field's label.
    AgentModel,
    /// The API address field's label.
    AgentBaseUrl,
    /// The API key field's label.
    AgentKey,
    /// Under the key field: where the key goes.
    AgentKeyKept,
    /// The provider form's button.
    AgentSave,
    /// A thread waiting on its model.
    AgentWorking,
    /// A run that finished with its work checked.
    AgentDone,
    /// A run that ended with its work not passing.
    AgentNotDone,
    /// The button that restores a run's checkpoint.
    AgentRestore,
    /// The heading over the staged hunks.
    AgentChanges,
    /// Accept one hunk.
    AgentAccept,
    /// Reject one hunk.
    AgentReject,
    /// Accept every hunk.
    AgentAcceptAll,
    /// Reject every hunk.
    AgentRejectAll,
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
    /// The status bar while the game runs.
    PreviewRunning,
    /// The status bar while the game is paused.
    PreviewPaused,
    /// The Play button while the game runs.
    Playing,
    /// The Play button's hint while the game runs.
    StopHint,
    /// The Play button while the game is paused.
    Resume,
    /// The Pause button while the game is paused.
    Paused,
    /// The toolbar after a reload failed.
    ReloadFailed,
    /// The first-run guide's heading (P3-21).
    GameGuide,
    /// The guide when the original registered game is found.
    GameFound,
    /// The guide when only the shareware episode is found.
    GameSharewareOnly,
    /// The guide when only the 2021 rerelease is found.
    GameRereleaseOnly,
    /// The guide when no game data is found.
    GameMissing,
    /// Before the files the chosen folder lacks.
    GameLacks,
    /// How to get the files on a Mac.
    GameHowMac,
    /// How to get the files on Windows and Linux.
    GameHowDesktop,
    /// Which stores sell only the rerelease.
    GameStores,
    /// That the files are not copied.
    GameInPlace,
    /// The guide's folder picker button.
    ChooseGameFolder,
    /// The guide's folder picker's title.
    ChooseGameTitle,
    /// The guide's button that closes it.
    NotNow,
    /// File, Quake Files: the menu item that opens the guide.
    GameFilesMenu,
    /// The notice before a reload that would reset values (P3-10), before their list.
    WouldReset,
    /// The notice's button that reloads all the same.
    ReloadAnyway,
    /// The Entities filter's hint.
    FilterEntities,
    /// An outliner row whose edict has no classname.
    NoClassname,
    /// After the outliner's count of edicts in use.
    Live,
    /// The Properties panel with nothing selected.
    NothingSelected,
    /// The Properties panel while the game runs.
    ReadOnlyWhilePlaying,
    /// The Properties panel's link to the selected entity's spawn function.
    OpenSpawnFunction,
    /// The toolbar after a selection or a field's edit failed.
    EditFailed,
    /// After the fields edited while paused.
    SentOnResume,
    /// The Debugger tab's call stack column.
    CallStack,
    /// The Debugger tab's variables and watches column.
    Watches,
    /// The Debugger tab's breakpoints column.
    Breakpoints,
    /// The hint in the box that adds a watch.
    AddWatch,
    /// Steps into the next call.
    StepIn,
    /// Steps to the next statement of the function.
    StepOver,
    /// Steps out to the caller.
    StepOut,
    /// A call stack frame of the engine's, not QuakeC's.
    EngineFrame,
    /// The hint in the console's command box.
    ConsoleCommand,
    /// The breakpoints column with none set.
    NoBreakpoints,
    /// The Debugger tab while the game runs.
    NotStopped,
    /// A watch with no value read yet.
    NotRead,
    /// The button that takes a watch away or closes a tab.
    Remove,
    /// The hint in the console's filter box.
    FilterConsole,
    /// The console's prompt before its command box.
    ConsolePrompt,
    /// After a tab's name when its file is unsaved.
    UnsavedMark,
    /// What that button does.
    CloseTabHint,
    /// Beside an unsaved file in the Project tree.
    ModifiedMark,
    /// Between the steps of the breadcrumb.
    PathSeparator,
    /// The file finder's name for a screen reader.
    FileFinder,
    /// The hint in the file finder's field.
    SearchFiles,
    /// The file finder with no file matching.
    NoFileMatch,
    /// Its Open Folder item.
    OpenFolderMenu,
    /// The system dialog's title.
    OpenFolderTitle,
    /// The Project section's button with no project open.
    OpenFolderButton,
    /// File, New id1 Project: a project from id's QuakeC 1.01 (P4-17).
    NewId1Menu,
    /// File, New Rerelease Project: a project from the rerelease's QuakeC (P4-17).
    NewRereleaseMenu,
    /// The system dialog's title when naming a new project's folder.
    NewProjectTitle,
    /// The heading over the recent projects.
    RecentProjects,
    /// Before the path of a folder that is not there.
    FolderGone,
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
    /// The palette's hint for the arrow keys.
    PaletteMove,
    /// The palette's hint for Enter.
    PaletteRun,
    /// The parameter prompt's Run button.
    PaletteRunButton,
    /// The palette's hint for Escape.
    PaletteClose,
    /// The parameter prompt's Cancel button.
    PaletteCancel,
    /// The palette when no command matches.
    PaletteNoMatch,
    /// A required parameter's mark.
    PaletteRequired,
    /// A choice parameter with nothing picked.
    PaletteChoose,
    /// The Escape key, in the palette's hints.
    KeyEscape,
    /// The up and down arrow keys, in the palette's hints.
    KeyArrows,
    /// The Enter key, in the palette's hints.
    KeyEnter,
    /// The Symbols section's heading.
    Symbols,
    /// The File menu, and the Symbols section's switch to the open file's symbols.
    File,
    /// The Symbols section's switch to every file's symbols.
    SymbolsAll,
    /// The Symbols section with nothing to list.
    NoSymbols,
    /// A function, in the Symbols section.
    SymbolFunction,
    /// A field, in the Symbols section.
    SymbolField,
    /// A constant, in the Symbols section.
    SymbolConstant,
    /// A global variable, in the Symbols section.
    SymbolGlobal,
    /// The references popup's heading.
    References,
    /// The rename popup's label.
    RenameTo,
    /// A rename the project refused.
    RenameFailed,
    /// Go to definition with no name, or one with no definition.
    NoDefinition,
    /// Find references with no name.
    NoReferences,
    /// Completion with nothing to offer.
    NoCompletions,
    /// The find bar's query field.
    Find,
    /// The find bar's replacement field and its button.
    Replace,
    /// The find bar's replace-all button.
    ReplaceAll,
    /// The find bar's next-match button.
    FindNext,
    /// The find bar's previous-match button.
    FindPrevious,
    /// The find bar's case switch.
    MatchCase,
    /// The map view's tab in the centre.
    Map,
    /// The map view with no map shown.
    NoMap,
    /// The toolbar's name to a screen reader.
    Toolbar,
    /// The status bar's name to a screen reader.
    StatusBar,
    /// The left panel's name to a screen reader.
    LeftPanel,
    /// The right panel's name to a screen reader.
    RightPanel,
    /// The bottom panel's name to a screen reader.
    BottomPanel,
    /// The centre's name to a screen reader.
    Centre,
    /// The map view's name to a screen reader.
    MapView,
    /// The command palette's name to a screen reader.
    CommandPalette,
}

impl Text {
    /// Every string, in the order above.
    pub const ALL: [Self; 157] = [
        Self::AppName,
        Self::Play,
        Self::Pause,
        Self::Step,
        Self::RestartMap,
        Self::PreviewHost,
        Self::PreviewDocked,
        Self::PreviewOwnWindow,
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
        Self::InlineEditPrompt,
        Self::AgentSettings,
        Self::AgentTaskHint,
        Self::AgentRun,
        Self::AskAboutThis,
        Self::ProviderAnthropic,
        Self::ProviderOpenAi,
        Self::ProviderOllama,
        Self::AgentModel,
        Self::AgentBaseUrl,
        Self::AgentKey,
        Self::AgentKeyKept,
        Self::AgentSave,
        Self::AgentWorking,
        Self::AgentDone,
        Self::AgentNotDone,
        Self::AgentRestore,
        Self::AgentChanges,
        Self::AgentAccept,
        Self::AgentReject,
        Self::AgentAcceptAll,
        Self::AgentRejectAll,
        Self::NoProblems,
        Self::ConsoleIdle,
        Self::DebuggerIdle,
        Self::NoFileOpen,
        Self::GameStopped,
        Self::PreviewStopped,
        Self::PreviewRunning,
        Self::PreviewPaused,
        Self::Playing,
        Self::StopHint,
        Self::Resume,
        Self::Paused,
        Self::ReloadFailed,
        Self::GameGuide,
        Self::GameFound,
        Self::GameSharewareOnly,
        Self::GameRereleaseOnly,
        Self::GameMissing,
        Self::GameLacks,
        Self::GameHowMac,
        Self::GameHowDesktop,
        Self::GameStores,
        Self::GameInPlace,
        Self::ChooseGameFolder,
        Self::ChooseGameTitle,
        Self::NotNow,
        Self::GameFilesMenu,
        Self::WouldReset,
        Self::ReloadAnyway,
        Self::FilterEntities,
        Self::NoClassname,
        Self::Live,
        Self::NothingSelected,
        Self::ReadOnlyWhilePlaying,
        Self::OpenSpawnFunction,
        Self::EditFailed,
        Self::SentOnResume,
        Self::CallStack,
        Self::Watches,
        Self::Breakpoints,
        Self::AddWatch,
        Self::StepIn,
        Self::StepOver,
        Self::StepOut,
        Self::EngineFrame,
        Self::ConsoleCommand,
        Self::NoBreakpoints,
        Self::NotStopped,
        Self::NotRead,
        Self::Remove,
        Self::FilterConsole,
        Self::ConsolePrompt,
        Self::UnsavedMark,
        Self::CloseTabHint,
        Self::ModifiedMark,
        Self::PathSeparator,
        Self::FileFinder,
        Self::SearchFiles,
        Self::NoFileMatch,
        Self::OpenFolderMenu,
        Self::OpenFolderTitle,
        Self::OpenFolderButton,
        Self::NewId1Menu,
        Self::NewRereleaseMenu,
        Self::NewProjectTitle,
        Self::RecentProjects,
        Self::FolderGone,
        Self::NotYetAvailable,
        Self::ViewMenu,
        Self::ToggleLeftPanel,
        Self::ToggleRightPanel,
        Self::ToggleBottomPanel,
        Self::LoggingOff,
        Self::PaletteMove,
        Self::PaletteRun,
        Self::PaletteRunButton,
        Self::PaletteClose,
        Self::PaletteCancel,
        Self::PaletteNoMatch,
        Self::PaletteRequired,
        Self::PaletteChoose,
        Self::KeyEscape,
        Self::KeyArrows,
        Self::KeyEnter,
        Self::Symbols,
        Self::File,
        Self::SymbolsAll,
        Self::NoSymbols,
        Self::SymbolFunction,
        Self::SymbolField,
        Self::SymbolConstant,
        Self::SymbolGlobal,
        Self::References,
        Self::RenameTo,
        Self::RenameFailed,
        Self::NoDefinition,
        Self::NoReferences,
        Self::NoCompletions,
        Self::Find,
        Self::Replace,
        Self::ReplaceAll,
        Self::FindNext,
        Self::FindPrevious,
        Self::MatchCase,
        Self::Map,
        Self::NoMap,
        Self::Toolbar,
        Self::StatusBar,
        Self::LeftPanel,
        Self::RightPanel,
        Self::BottomPanel,
        Self::Centre,
        Self::MapView,
        Self::CommandPalette,
    ];

    /// The string, in English.
    #[must_use]
    #[expect(
        clippy::too_many_lines,
        reason = "the catalog: one arm per string, which the tests hold to Text::ALL"
    )]
    pub const fn get(self) -> &'static str {
        match self {
            Self::AppName => "QuakeC Studio",
            Self::Play => "Play",
            Self::Pause => "Pause",
            Self::Step => "Step",
            Self::RestartMap => "Restart map",
            Self::PreviewHost => "Preview host",
            Self::PreviewDocked => "docked right",
            Self::PreviewOwnWindow => "in its own window",
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
            Self::InlineEditPrompt => "Change the selection to\u{2026}",
            Self::AgentSettings => "Settings",
            Self::AgentTaskHint => "What should the agent do?",
            Self::AgentRun => "Start",
            Self::AskAboutThis => "Ask about this",
            Self::ProviderAnthropic => "Anthropic",
            Self::ProviderOpenAi => "OpenAI-compatible",
            Self::ProviderOllama => "Ollama (local)",
            Self::AgentModel => "Model",
            Self::AgentBaseUrl => "Address (optional)",
            Self::AgentKey => "API key",
            Self::AgentKeyKept => "Kept in the system keychain, never in a file.",
            Self::AgentSave => "Save",
            Self::AgentWorking => "Working\u{2026}",
            Self::AgentDone => "Done",
            Self::AgentNotDone => "Not done",
            Self::AgentRestore => "Restore checkpoint",
            Self::AgentChanges => "Proposed changes",
            Self::AgentAccept => "Accept",
            Self::AgentReject => "Reject",
            Self::AgentAcceptAll => "Accept all",
            Self::AgentRejectAll => "Reject all",
            Self::NoProblems => "No problems.",
            Self::ConsoleIdle => "The preview host's console appears here once it starts.",
            Self::DebuggerIdle => "Nothing is paused.",
            Self::NoFileOpen => "Open a file from the Project panel.",
            Self::GameStopped => "The preview host is not running.",
            Self::PreviewStopped => "Preview host stopped",
            Self::PreviewRunning => "Preview host running",
            Self::PreviewPaused => "Preview host paused",
            Self::Playing => "Playing",
            Self::StopHint => "Stop the game",
            Self::Resume => "Resume",
            Self::Paused => "Paused",
            Self::ReloadFailed => "The reload failed; see the Console",
            Self::GameGuide => "Your Quake files",
            Self::GameFound => "Found Quake. Play reads it where it is.",
            Self::GameSharewareOnly => {
                "Only the shareware episode is here. Play works on its maps; the rest of the game needs id1/pak1.pak."
            }
            Self::GameRereleaseOnly => {
                "Only the 2021 rerelease is here. Play needs the original game's id1 folder."
            }
            Self::GameMissing => "No Quake files were found.",
            Self::GameLacks => "Missing:",
            Self::GameHowMac => {
                "On a Mac: download GOG's Windows installer and extract it with innoextract, or download the game with steamcmd. Then choose the folder."
            }
            Self::GameHowDesktop => "Install Quake from Steam or GOG, then choose its folder.",
            Self::GameStores => "The Epic Games Store and Game Pass sell only the rerelease.",
            Self::GameInPlace => "The files are read where they are, never copied.",
            Self::ChooseGameFolder => "Choose folder",
            Self::ChooseGameTitle => "Choose your Quake folder",
            Self::NotNow => "Not now",
            Self::GameFilesMenu => "Quake Files\u{2026}",
            Self::WouldReset => "Reloading would reset",
            Self::ReloadAnyway => "Reload anyway",
            Self::FilterEntities => "Filter by classname or number",
            Self::NoClassname => "(no classname)",
            Self::Live => "live",
            Self::NothingSelected => "Select an entity in the Entities list.",
            Self::ReadOnlyWhilePlaying => "Read only while playing. Pause to edit.",
            Self::OpenSpawnFunction => "Open its spawn function",
            Self::EditFailed => "The game did not take the change; see the log",
            Self::SentOnResume => "edited, sent to the game on Resume",
            Self::CallStack => "Call stack",
            Self::Watches => "Variables and watches",
            Self::Breakpoints => "Breakpoints",
            Self::AddWatch => "Add a watch, for example self.owner.health",
            Self::StepIn => "Step in",
            Self::StepOver => "Step over",
            Self::StepOut => "Step out",
            Self::EngineFrame => "engine",
            Self::ConsoleCommand => "Console command, for example impulse 9",
            Self::NoBreakpoints => "Click a line number to set a breakpoint.",
            Self::NotStopped => {
                "The call stack and watches show when the game stops at a breakpoint or a step."
            }
            Self::NotRead => "not read",
            Self::Remove => "\u{d7}",
            Self::FilterConsole => "Filter the console",
            Self::ConsolePrompt => "]",
            Self::UnsavedMark => "\u{25cf}",
            Self::CloseTabHint => "Close the tab; its unsaved edits are kept",
            Self::ModifiedMark => "M",
            Self::PathSeparator => " / ",
            Self::FileFinder => "File finder",
            Self::SearchFiles => "Search files by name",
            Self::NoFileMatch => "No file matches.",
            Self::OpenFolderMenu => "Open Folder\u{2026}",
            Self::OpenFolderTitle => "Open a QuakeC project folder",
            Self::OpenFolderButton => "Open Folder",
            Self::NewId1Menu => "New id1 Project\u{2026}",
            Self::NewRereleaseMenu => "New Rerelease Project\u{2026}",
            Self::NewProjectTitle => "Name the new project's folder",
            Self::RecentProjects => "Recent projects",
            Self::FolderGone => "The folder is no longer there:",
            Self::NotYetAvailable => "Not available yet",
            Self::ViewMenu => "View",
            Self::ToggleLeftPanel => "Project and Entities",
            Self::ToggleRightPanel => "Properties and Agent",
            Self::ToggleBottomPanel => "Problems, Console and Debugger",
            Self::LoggingOff => "logging is off",
            Self::PaletteMove => "move",
            Self::PaletteRun => "run",
            Self::PaletteRunButton => "Run",
            Self::PaletteClose => "close",
            Self::PaletteCancel => "Cancel",
            Self::PaletteNoMatch => "No command matches.",
            Self::PaletteRequired => "required",
            Self::PaletteChoose => "Choose…",
            Self::KeyEscape => "esc",
            Self::KeyArrows => "\u{2191}\u{2193}",
            Self::KeyEnter => "enter",
            Self::Symbols => "Symbols",
            Self::File => "File",
            Self::SymbolsAll => "All",
            Self::NoSymbols => "No symbols.",
            Self::SymbolFunction => "function",
            Self::SymbolField => "field",
            Self::SymbolConstant => "constant",
            Self::SymbolGlobal => "global",
            Self::References => "References",
            Self::RenameTo => "Rename to",
            Self::RenameFailed => "This name cannot be renamed to that.",
            Self::NoDefinition => "No definition found.",
            Self::NoReferences => "No references found.",
            Self::NoCompletions => "No completions.",
            Self::Find => "Find",
            Self::Replace => "Replace",
            Self::ReplaceAll => "Replace all",
            Self::FindNext => "Next",
            Self::FindPrevious => "Previous",
            Self::MatchCase => "Match case",
            Self::Map => "Map",
            Self::NoMap => "No map open.",
            Self::Toolbar => "Toolbar",
            Self::StatusBar => "Status bar",
            Self::LeftPanel => "Left panel: Project and Entities",
            Self::RightPanel => "Right panel: Properties and Agent",
            Self::BottomPanel => "Bottom panel: Problems, Console and Debugger",
            Self::Centre => "Centre: Editor, Game and Map",
            Self::MapView => "Map view: drag to turn, scroll to move",
            Self::CommandPalette => "Command palette",
        }
    }
}

/// The palette's count of the commands it lists, out of all of them.
#[must_use]
pub fn command_count(shown: usize, total: usize) -> String {
    format!("{shown} of {total} commands")
}

/// The file finder's count of the files it lists, out of all of them.
#[must_use]
pub fn file_count(shown: usize, total: usize) -> String {
    format!("{shown} of {total} files")
}

/// The find bar's count of the matches.
#[must_use]
pub fn match_count(count: usize) -> String {
    match count {
        1 => "1 match".to_owned(),
        n => format!("{n} matches"),
    }
}
