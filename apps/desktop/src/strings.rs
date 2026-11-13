// SPDX-License-Identifier: GPL-3.0-or-later

//! Every string the app shows (CONV-16): the UI code names a [`Text`], never a literal, so the
//! wording is reviewed in one place and can be translated later. A test fails on a string
//! literal in the UI modules.

use qs_workbench::map_textures::{WadFound, WadState};

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
    /// Why a thread could not be run again: it is gone.
    AgentNoThread,
    /// The agent panel's toggle that opens each file the agent works on (P4-31).
    AgentFollow,
    /// The agent panel's button that lists the threads kept.
    AgentHistory,
    /// The hint in the thread history's search box.
    AgentHistorySearch,
    /// The thread history's button back to the threads.
    AgentHistoryBack,
    /// The thread history with no thread kept.
    AgentNoHistory,
    /// Before a reopened thread's checkpoint number.
    AgentCheckpoint,
    /// The heading over a thread's compaction summaries.
    AgentSummaries,
    /// The button that edits an earlier message of a thread.
    AgentEdit,
    /// The button that runs a thread again from an edited message.
    AgentRunAgain,
    /// The button that queues a message for a running thread's next turn.
    AgentQueue,
    /// The hint in a running thread's message box.
    AgentQueueHint,
    /// The button that gives the running game's frame as context.
    AgentScreenshot,
    /// A thread's line for an image given as context.
    AgentImage,
    /// The bottom panel's Git tab (P4-35).
    Git,
    /// The Git tab's label before the branch.
    Branch,
    /// The hint in the Git tab's new branch box.
    NewBranchName,
    /// The Git tab's button that creates a branch and checks it out.
    CreateBranch,
    /// The Git tab's button that reads the repository again.
    Refresh,
    /// The Git tab when the project is not in a git repository.
    NotARepository,
    /// The Git tab's heading over the changed files.
    Changes,
    /// The Git tab with nothing changed.
    NoChanges,
    /// The Git tab's commit heading and button.
    Commit,
    /// The hint in the Git tab's commit message box.
    CommitMessage,
    /// The Git tab's button that stages a whole file.
    Stage,
    /// The Git tab's button that unstages a whole file.
    Unstage,
    /// The Git tab's button that stages one hunk.
    StageHunk,
    /// The Git tab's button that unstages one hunk.
    UnstageHunk,
    /// The Git tab's button that resolves a conflict to our side.
    KeepOurs,
    /// The Git tab's button that resolves a conflict to their side.
    KeepTheirs,
    /// The Git tab's button that keeps both sides of a conflict.
    KeepBoth,
    /// The Git tab's heading over the open file's commits.
    History,
    /// The Git tab's button that shows the open file's blame.
    ShowBlame,
    /// The Git tab's button that hides the open file's blame.
    HideBlame,
    /// The go to symbol picker's name and its search box's hint.
    GoToSymbol,
    /// The tab switcher's name and its search box's hint.
    TabSwitcher,
    /// The outline panel's heading.
    Outline,
    /// The outline panel when the file has no symbols.
    NoOutline,
    /// The editor's second pane's accessible name.
    SecondPane,
    /// The editor's list of quick fixes at the caret (P4-34).
    QuickFixes,
    /// The editor's note when the caret has no quick fix.
    NoQuickFix,
    /// The bottom panel's Terminal tab, and the terminal output's name.
    Terminal,
    /// The Terminal tab's button that opens a shell.
    NewTerminal,
    /// The Terminal tab's button that closes the shown tab.
    CloseTerminal,
    /// The hint in a shell tab's input box.
    TerminalCommand,
    /// A terminal tab's status while its process runs.
    TerminalRunning,
    /// A terminal tab's status when a signal ended its process.
    TerminalKilled,
    /// The Terminal tab with no tab open.
    NoTerminals,
    /// A task's row in the command palette, before its name.
    RunTask,
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
    /// The agent panel's profile picker (P4-30).
    AgentProfile,
    /// The Ask profile: reading tools only.
    AgentProfileAsk,
    /// The Write profile: reads and edits run, the rest asks.
    AgentProfileWrite,
    /// The Full profile: every tool, unasked.
    AgentProfileFull,
    /// Before the tool a call waiting for permission names.
    AgentAsks,
    /// Runs the call that asks.
    AgentAllowOnce,
    /// Runs it and every later call of the tool on what it names.
    AgentAlwaysAllow,
    /// Refuses the call that asks.
    AgentDeny,

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
    /// File, Export Mod: the open project as a zip players install (P4-28).
    ExportModMenu,
    /// Before the path of the zip an export wrote.
    ExportedTo,
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
    /// The centre's Search tab: the multibuffer of the project search (P4-33).
    ProjectSearch,
    /// The project search box's hint.
    SearchProject,
    /// The project search's Whole word toggle.
    WholeWord,
    /// The project search's regular expression toggle.
    UseRegex,
    /// The multibuffer with nothing to show.
    NoSearchMatch,
    /// An excerpt's button that opens its file at the caret.
    OpenExcerpt,
    /// The Problems tab's button that shows every problem as excerpts.
    OpenAsMultibuffer,
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
    /// The map editor's view, as a screen reader names it.
    MapEditorView,
    /// The map view's layout of the 3D view alone.
    MapLayoutSingle,
    /// The map view's layout of the 3D view with a top view in its corner.
    MapLayoutInset,
    /// The map view's layout of the 3D view and three 2D views.
    MapLayoutQuad,
    /// The top view, as a screen reader names it.
    MapViewTop,
    /// The front view, as a screen reader names it.
    MapViewFront,
    /// The side view, as a screen reader names it.
    MapViewSide,
    /// The key editor's line when no entity of the open map is selected.
    MapNoSelection,
    /// The notice when another program changed the open map while it had unsaved edits.
    MapChangedOnDisk,
    /// The notice's button that reads the map from disk, dropping the edits.
    MapReloadFromDisk,
    /// The notice's button that keeps the edits, to save over the file.
    MapKeepMine,
    /// The notice's button that lists how the file differs.
    MapCompare,
    /// The key editor's button that removes a key.
    MapRemoveKey,
    /// The key editor's heading over the keys the entity's class reads that it does not set.
    MapClassKeys,
    /// The key editor's hint in the new key's field.
    MapKey,
    /// The key editor's hint in the new value's field.
    MapValue,
    /// The key editor's list of the map's targetnames for a target key.
    MapTargets,
    /// The key editor's button that adds the key typed.
    MapAddKey,
    /// The entity browser's filter hint.
    MapFilterEntities,
    /// The texture browser's tab (P6-10).
    MapTextures,
    /// The texture browser's filter field.
    MapFilterTextures,
    /// A texture the map's faces name that no WAD or BSP holds.
    MapTextureMissing,
    /// Why no texture is shown in its colours: no palette in the project or the game.
    MapNoPalette,
    /// What dragging a class from the entity browser does.
    MapPlaceHint,
    /// Why a brush class in the entity browser cannot be dragged.
    MapBrushClass,
    /// The map view's choice of the links it draws.
    MapLinks,
    /// Draw the links of the selection.
    MapLinksSelected,
    /// Draw every link.
    MapLinksAll,
    /// The Select tool: pick, move and link entities.
    MapToolSelect,
    /// The Brush tool: draw a box brush.
    MapToolBrush,
    /// The Clip tool: cut a brush by a plane.
    MapToolClip,
    /// The Vertex tool: move a brush's corners.
    MapToolVertex,
    /// The Shape tool (P6-18).
    MapToolShape,
    /// What to do with the Shape tool.
    MapShapeHint,
    /// A shape: a cylinder.
    ShapeCylinder,
    /// A shape: a cone.
    ShapeCone,
    /// A shape: an arch.
    ShapeArch,
    /// A shape: a spheroid of bands and slices.
    ShapeUvSpheroid,
    /// A shape: a spheroid of an icosahedron's triangles.
    ShapeIcospheroid,
    /// A shape: a rock.
    ShapeRock,
    /// A shape: a curve on the grid at every size.
    ShapeCurve,
    /// A shape: straight stairs.
    ShapeStairs,
    /// A shape: spiral stairs.
    ShapeSpiralStairs,
    /// The sides of a shape.
    MapShapeSides,
    /// A round shape's corners on its circle.
    MapShapeCorners,
    /// A round shape's edges touching its circle.
    MapShapeEdges,
    /// A solid cylinder.
    MapShapeSolid,
    /// A hollow cylinder.
    MapShapeHollow,
    /// A hollow shape's wall thickness.
    MapShapeWall,
    /// Stairs climbing toward +x.
    MapClimbEast,
    /// Stairs climbing toward -x.
    MapClimbWest,
    /// Stairs climbing toward +y.
    MapClimbNorth,
    /// Stairs climbing toward -y.
    MapClimbSouth,
    /// How to draw a brush with the Brush tool.
    MapBrushHint,
    /// How to cut a brush with the Clip tool.
    MapClipHint,
    /// How to move a corner with the Vertex tool.
    MapVertexHint,
    /// The Move tool, which drags the selected brushes (P6-09).
    MapToolMove,
    /// The Measure tool, which measures between two points (P6-09).
    MapToolMeasure,
    /// What to do with the Move tool.
    MapMoveHint,
    /// What to do with the Measure tool.
    MapMeasureHint,
    /// The texture lock toggle: on, a moved brush's textures stay fixed in the world.
    MapTextureLock,
    /// The button that sets the selected brushes' shift, rotation and scale.
    MapAlignTexture,
    /// The label of a texture's shift.
    MapTextureShift,
    /// The label of a texture's rotation.
    MapTextureRotate,
    /// The label of a texture's scale.
    MapTextureScale,
    /// Keep the part of a clipped brush right of the cut line.
    MapKeepRight,
    /// Keep the part of a clipped brush left of the cut line.
    MapKeepLeft,
    /// The heading over the selected brushes' buttons.
    MapBrushes,
    /// Subtract the selected brush from the others of its entity.
    MapSubtract,
    /// Merge the selected brushes into one.
    MapMerge,
    /// Delete the selected brushes.
    MapDeleteBrush,
    /// The map view's filter by skill and game.
    MapShow,
    /// Show every entity.
    MapShowEvery,
    /// Show what the game spawns on easy.
    MapSkillEasy,
    /// Show what the game spawns on normal.
    MapSkillNormal,
    /// Show what the game spawns on hard.
    MapSkillHard,
    /// Show what the game spawns on nightmare.
    MapSkillNightmare,
    /// Show what the game spawns in coop.
    MapCoop,
    /// Show what the game spawns in deathmatch.
    MapDeathmatch,
    /// The key editor's spawnflag checkboxes.
    MapSpawnflags,
    /// The key editor's eyedropper beside a targeting key.
    MapPick,
    /// What the eyedropper does.
    MapPickHint,
    /// The angle editor's up button, angle -1.
    MapAngleUp,
    /// The angle editor's down button, angle -2.
    MapAngleDown,
    /// The key editor's list of the values the map uses for a key.
    MapChoices,
    /// The map outline's filter hint (P5-16).
    MapFilterOutline,
    /// The map outline's row for what no layer names: TrenchBroom's default layer.
    MapDefaultLayer,
    /// The map editor's list of the map's layers (P5-14).
    MapLayers,
    /// A layer's checkbox that hides it in the editor.
    MapLayerHidden,
    /// A layer's checkbox that locks it against edits.
    MapLayerLocked,
    /// A layer's checkbox that leaves it out of exports.
    MapLayerOmit,
    /// Hide the selection in the editor (TrenchBroom's Hide Selection).
    MapHide,
    /// Hide everything but the selection (TrenchBroom's Isolate Selection).
    MapIsolate,
    /// Lock the selection against edits.
    MapLock,
    /// Show every entity hidden in the editor (TrenchBroom's Show All).
    MapShowAll,
    /// Unlock every entity locked in the editor.
    MapUnlockAll,
    /// The command palette's name to a screen reader.
    CommandPalette,
}

impl Text {
    /// Every string, in the order above.
    pub const ALL: [Self; 319] = [
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
        Self::AgentProfile,
        Self::AgentProfileAsk,
        Self::AgentProfileWrite,
        Self::AgentProfileFull,
        Self::AgentAsks,
        Self::AgentAllowOnce,
        Self::AgentAlwaysAllow,
        Self::AgentDeny,
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
        Self::ExportModMenu,
        Self::ExportedTo,
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
        Self::AgentNoThread,
        Self::AgentFollow,
        Self::AgentHistory,
        Self::AgentHistorySearch,
        Self::AgentHistoryBack,
        Self::AgentNoHistory,
        Self::AgentCheckpoint,
        Self::AgentSummaries,
        Self::AgentEdit,
        Self::AgentRunAgain,
        Self::AgentQueue,
        Self::AgentQueueHint,
        Self::AgentScreenshot,
        Self::AgentImage,
        Self::Git,
        Self::Branch,
        Self::NewBranchName,
        Self::CreateBranch,
        Self::Refresh,
        Self::NotARepository,
        Self::Changes,
        Self::NoChanges,
        Self::Commit,
        Self::CommitMessage,
        Self::Stage,
        Self::Unstage,
        Self::StageHunk,
        Self::UnstageHunk,
        Self::KeepOurs,
        Self::KeepTheirs,
        Self::KeepBoth,
        Self::History,
        Self::ShowBlame,
        Self::HideBlame,
        Self::GoToSymbol,
        Self::TabSwitcher,
        Self::Outline,
        Self::NoOutline,
        Self::SecondPane,
        Self::QuickFixes,
        Self::NoQuickFix,
        Self::Terminal,
        Self::NewTerminal,
        Self::CloseTerminal,
        Self::TerminalCommand,
        Self::TerminalRunning,
        Self::TerminalKilled,
        Self::NoTerminals,
        Self::RunTask,
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
        Self::ProjectSearch,
        Self::SearchProject,
        Self::WholeWord,
        Self::UseRegex,
        Self::NoSearchMatch,
        Self::OpenExcerpt,
        Self::OpenAsMultibuffer,
        Self::Map,
        Self::NoMap,
        Self::Toolbar,
        Self::StatusBar,
        Self::LeftPanel,
        Self::RightPanel,
        Self::BottomPanel,
        Self::Centre,
        Self::MapView,
        Self::MapEditorView,
        Self::MapLayoutSingle,
        Self::MapLayoutInset,
        Self::MapLayoutQuad,
        Self::MapViewTop,
        Self::MapViewFront,
        Self::MapViewSide,
        Self::MapNoSelection,
        Self::MapChangedOnDisk,
        Self::MapReloadFromDisk,
        Self::MapKeepMine,
        Self::MapCompare,
        Self::MapRemoveKey,
        Self::MapClassKeys,
        Self::MapKey,
        Self::MapValue,
        Self::MapTargets,
        Self::MapAddKey,
        Self::MapFilterEntities,
        Self::MapTextures,
        Self::MapFilterTextures,
        Self::MapTextureMissing,
        Self::MapNoPalette,
        Self::MapPlaceHint,
        Self::MapBrushClass,
        Self::MapLinks,
        Self::MapLinksSelected,
        Self::MapLinksAll,
        Self::MapToolSelect,
        Self::MapToolBrush,
        Self::MapToolClip,
        Self::MapToolVertex,
        Self::MapToolShape,
        Self::MapShapeHint,
        Self::ShapeCylinder,
        Self::ShapeCone,
        Self::ShapeArch,
        Self::ShapeUvSpheroid,
        Self::ShapeIcospheroid,
        Self::ShapeRock,
        Self::ShapeCurve,
        Self::ShapeStairs,
        Self::ShapeSpiralStairs,
        Self::MapShapeSides,
        Self::MapShapeCorners,
        Self::MapShapeEdges,
        Self::MapShapeSolid,
        Self::MapShapeHollow,
        Self::MapShapeWall,
        Self::MapClimbEast,
        Self::MapClimbWest,
        Self::MapClimbNorth,
        Self::MapClimbSouth,
        Self::MapBrushHint,
        Self::MapClipHint,
        Self::MapVertexHint,
        Self::MapToolMove,
        Self::MapToolMeasure,
        Self::MapMoveHint,
        Self::MapMeasureHint,
        Self::MapTextureLock,
        Self::MapAlignTexture,
        Self::MapTextureShift,
        Self::MapTextureRotate,
        Self::MapTextureScale,
        Self::MapKeepRight,
        Self::MapKeepLeft,
        Self::MapBrushes,
        Self::MapSubtract,
        Self::MapMerge,
        Self::MapDeleteBrush,
        Self::MapShow,
        Self::MapShowEvery,
        Self::MapSkillEasy,
        Self::MapSkillNormal,
        Self::MapSkillHard,
        Self::MapSkillNightmare,
        Self::MapCoop,
        Self::MapDeathmatch,
        Self::MapSpawnflags,
        Self::MapPick,
        Self::MapPickHint,
        Self::MapAngleUp,
        Self::MapAngleDown,
        Self::MapChoices,
        Self::MapFilterOutline,
        Self::MapDefaultLayer,
        Self::MapLayers,
        Self::MapLayerHidden,
        Self::MapLayerLocked,
        Self::MapLayerOmit,
        Self::MapHide,
        Self::MapIsolate,
        Self::MapLock,
        Self::MapShowAll,
        Self::MapUnlockAll,
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
            Self::AgentProfile => "Profile",
            Self::AgentProfileAsk => "Ask",
            Self::AgentProfileWrite => "Write",
            Self::AgentProfileFull => "Full",
            Self::AgentAsks => "Allow the agent to run",
            Self::AgentAllowOnce => "Allow once",
            Self::AgentAlwaysAllow => "Always allow",
            Self::AgentDeny => "Deny",

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
            Self::ExportModMenu => "Export Mod\u{2026}",
            Self::ExportedTo => "Exported for players to",
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
            Self::AgentNoThread => "That thread is gone.",
            Self::AgentFollow => "Follow the agent",
            Self::AgentHistory => "History",
            Self::AgentHistorySearch => "Search threads",
            Self::AgentHistoryBack => "Back",
            Self::AgentNoHistory => "No thread has ended in this project yet.",
            Self::AgentCheckpoint => "Checkpoint",
            Self::AgentSummaries => "Summaries of compacted turns",
            Self::AgentEdit => "Edit",
            Self::AgentRunAgain => "Run again from here",
            Self::AgentQueue => "Queue",
            Self::AgentQueueHint => "A message for its next turn",
            Self::AgentScreenshot => "Attach screenshot",
            Self::AgentImage => "An image, given as context",
            Self::Git => "Git",
            Self::Branch => "Branch",
            Self::NewBranchName => "New branch name",
            Self::CreateBranch => "Create branch",
            Self::Refresh => "Refresh",
            Self::NotARepository => "The project is not in a git repository.",
            Self::Changes => "Changes",
            Self::NoChanges => "No changes since the last commit.",
            Self::Commit => "Commit",
            Self::CommitMessage => "Commit message",
            Self::Stage => "Stage",
            Self::Unstage => "Unstage",
            Self::StageHunk => "Stage hunk",
            Self::UnstageHunk => "Unstage hunk",
            Self::KeepOurs => "Keep ours",
            Self::KeepTheirs => "Keep theirs",
            Self::KeepBoth => "Keep both",
            Self::History => "History of the open file",
            Self::ShowBlame => "Show blame",
            Self::HideBlame => "Hide blame",
            Self::GoToSymbol => "Go to a symbol in the file",
            Self::TabSwitcher => "Switch to an open file",
            Self::Outline => "Outline",
            Self::NoOutline => "No symbols in this file.",
            Self::SecondPane => "Second editor pane",
            Self::QuickFixes => "Quick fixes",
            Self::NoQuickFix => "No quick fix here.",
            Self::Terminal => "Terminal",
            Self::NewTerminal => "New terminal",
            Self::CloseTerminal => "Close",
            Self::TerminalCommand => "Type a command for the shell",
            Self::TerminalRunning => "Running",
            Self::TerminalKilled => "Ended by a signal",
            Self::NoTerminals => {
                "No terminal is open. Run a task from the command palette, or open a new terminal."
            }
            Self::RunTask => "Run task",
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
            Self::ProjectSearch => "Search",
            Self::SearchProject => "Search the project",
            Self::WholeWord => "Whole word",
            Self::UseRegex => "Regular expression",
            Self::NoSearchMatch => "Nothing found.",
            Self::OpenExcerpt => "Open",
            Self::OpenAsMultibuffer => "Open as excerpts",
            Self::Map => "Map",
            Self::NoMap => "No map open.",
            Self::Toolbar => "Toolbar",
            Self::StatusBar => "Status bar",
            Self::LeftPanel => "Left panel: Project and Entities",
            Self::RightPanel => "Right panel: Properties and Agent",
            Self::BottomPanel => "Bottom panel: Problems, Console and Debugger",
            Self::Centre => "Centre: Editor, Game and Map",
            Self::MapView => "Map view: drag to turn, scroll to move",
            Self::MapEditorView => {
                "Map editor: right-drag to look, middle-drag to pan, W A S D Q X to fly, click to select"
            }
            Self::MapLayoutSingle => "3D",
            Self::MapLayoutInset => "3D + top",
            Self::MapLayoutQuad => "4 views",
            Self::MapViewTop => "Top",
            Self::MapViewFront => "Front",
            Self::MapViewSide => "Side",
            Self::MapNoSelection => "Select an entity in the map to edit its keys.",
            Self::MapChangedOnDisk => {
                "Another program changed this map. Your unsaved edits are kept until you choose."
            }
            Self::MapReloadFromDisk => "Reload from disk",
            Self::MapKeepMine => "Keep mine",
            Self::MapCompare => "Compare",
            Self::MapRemoveKey => "Remove",
            Self::MapClassKeys => "Keys its class reads:",
            Self::MapKey => "key",
            Self::MapValue => "value",
            Self::MapTargets => "Targetnames",
            Self::MapAddKey => "Add key",
            Self::MapFilterEntities => "Filter entities",
            Self::MapTextures => "Textures",
            Self::MapFilterTextures => "Filter textures",
            Self::MapTextureMissing => "No WAD the map names holds this texture",
            Self::MapNoPalette => "No game palette",
            Self::MapPlaceHint => "Drag into the map to place",
            Self::MapBrushClass => "A brush entity: made from brushes",
            Self::MapLinks => "Links",
            Self::MapLinksSelected => "Selected",
            Self::MapLinksAll => "Every link",
            Self::MapToolSelect => "Select",
            Self::MapToolBrush => "Brush",
            Self::MapToolClip => "Clip",
            Self::MapToolVertex => "Vertex",
            Self::MapToolShape => "Shape",
            Self::MapShapeHint => "Drag a box to fill it with the shape",
            Self::ShapeCylinder => "Cylinder",
            Self::ShapeCone => "Cone",
            Self::ShapeArch => "Arch",
            Self::ShapeUvSpheroid => "UV sphere",
            Self::ShapeIcospheroid => "Icosphere",
            Self::ShapeRock => "Rock",
            Self::ShapeCurve => "Scalable curve",
            Self::ShapeStairs => "Stairs",
            Self::ShapeSpiralStairs => "Spiral stairs",
            Self::MapShapeSides => "Sides",
            Self::MapShapeCorners => "Corners on circle",
            Self::MapShapeEdges => "Edges on circle",
            Self::MapShapeSolid => "Solid",
            Self::MapShapeHollow => "Hollow",
            Self::MapShapeWall => "Wall",
            Self::MapClimbEast => "East",
            Self::MapClimbWest => "West",
            Self::MapClimbNorth => "North",
            Self::MapClimbSouth => "South",
            Self::MapBrushHint => "Drag on the ground to draw a box",
            Self::MapClipHint => "Select a brush, then drag the cut line",
            Self::MapVertexHint => "Select a brush, then drag a corner",
            Self::MapToolMove => "Move",
            Self::MapToolMeasure => "Measure",
            Self::MapMoveHint => "Select brushes, then drag to move them",
            Self::MapMeasureHint => "Drag from one point to another",
            Self::MapTextureLock => "Texture lock",
            Self::MapAlignTexture => "Align",
            Self::MapTextureShift => "Shift",
            Self::MapTextureRotate => "Rotate",
            Self::MapTextureScale => "Scale",
            Self::MapKeepRight => "Keep right",
            Self::MapKeepLeft => "Keep left",
            Self::MapBrushes => "Brushes",
            Self::MapSubtract => "Subtract",
            Self::MapMerge => "Merge",
            Self::MapDeleteBrush => "Delete brush",
            Self::MapShow => "Show",
            Self::MapShowEvery => "Every entity",
            Self::MapSkillEasy => "Easy",
            Self::MapSkillNormal => "Normal",
            Self::MapSkillHard => "Hard",
            Self::MapSkillNightmare => "Nightmare",
            Self::MapCoop => "Coop",
            Self::MapDeathmatch => "Deathmatch",
            Self::MapSpawnflags => "Spawn flags",
            Self::MapPick => "Pick",
            Self::MapPickHint => {
                "Click the entity in the map to fire; or Alt-drag from the selection onto it"
            }
            Self::MapAngleUp => "Up",
            Self::MapAngleDown => "Down",
            Self::MapChoices => "Values in the map",
            Self::MapFilterOutline => "Filter the map",
            Self::MapDefaultLayer => "Default layer",
            Self::MapLayers => "Layers",
            Self::MapLayerHidden => "Hidden",
            Self::MapLayerLocked => "Locked",
            Self::MapLayerOmit => "Not exported",
            Self::MapHide => "Hide",
            Self::MapIsolate => "Isolate",
            Self::MapLock => "Lock",
            Self::MapShowAll => "Show all",
            Self::MapUnlockAll => "Unlock all",
            Self::CommandPalette => "Command palette",
        }
    }
}

/// The palette's count of the commands it lists, out of all of them.
#[must_use]
pub fn command_count(shown: usize, total: usize) -> String {
    format!("{shown} of {total} commands")
}

/// A thread's estimated token use (P4-31).
#[must_use]
pub fn tokens_used(tokens: usize) -> String {
    format!("About {tokens} tokens")
}

/// How many messages wait for a running thread's next turn (P4-31).
#[must_use]
pub fn queued_count(count: usize) -> String {
    format!("{count} queued for the next turn")
}

/// A merge conflict's link in the Git tab: where its markers start, a line from 1.
#[must_use]
pub fn conflict_at(line: usize) -> String {
    format!("Conflict at line {line}")
}

/// A terminal tab's status once its process exited with `code`.
#[must_use]
pub fn exit_status(code: i32) -> String {
    format!("Exited with {code}")
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

/// The multibuffer's count of the places it shows and their files; `more` when the search
/// found more than it shows (P4-33).
#[must_use]
pub fn search_count(places: usize, files: usize, more: bool) -> String {
    let places = match places {
        1 => "1 result".to_owned(),
        n => format!("{n} results"),
    };
    let files = match files {
        1 => "1 file".to_owned(),
        n => format!("{n} files"),
    };
    if more {
        format!("{places} in {files}, the first only")
    } else {
        format!("{places} in {files}")
    }
}

/// The entity browser's count of the classes it lists, out of all of them (P5-13).
#[must_use]
pub fn class_count(shown: usize, total: usize) -> String {
    format!("{shown} of {total} entities")
}

/// A WAD the map names, as the texture browser's summary gives it: its name, and that it was
/// not found, or is no WAD, when so.
#[must_use]
pub fn wad_label(wad: &WadFound) -> String {
    match wad.state {
        WadState::Read { .. } => wad.name.clone(),
        WadState::Missing => format!("{} not found", wad.name),
        WadState::Unreadable => format!("{} is no WAD", wad.name),
    }
}

/// The texture browser's summary: the WADs the map names, and how many textures are shown of
/// how many, as the mockup's "ember.wad, 46 textures".
#[must_use]
pub fn texture_count(wads: &[String], shown: usize, total: usize) -> String {
    let count = if shown == total {
        format!("{total} textures")
    } else {
        format!("{shown} of {total} textures")
    };
    if wads.is_empty() {
        format!("no WAD, {count}")
    } else {
        format!("{}, {count}", wads.join(", "))
    }
}

/// A point or offset in map units, its coordinates as the `.map` file writes them.
fn units([x, y, z]: [f64; 3]) -> String {
    format!("{x} {y} {z}")
}

/// What the Move tool shows: the move so far and where the pointer is (P6-09).
#[must_use]
pub fn move_readout(by: [f64; 3], at: [f64; 3]) -> String {
    format!("Move {}, pointer at {}", units(by), units(at))
}

/// What the Measure tool shows: the distance between two points and along each axis (P6-09).
#[must_use]
pub fn measure_readout(distance: f64, along: [f64; 3]) -> String {
    format!("Distance {distance:.2} ({})", units(along))
}

/// The size of the selected brushes' bounds (P6-09).
#[must_use]
pub fn selection_size([x, y, z]: [f64; 3]) -> String {
    format!("Size {x} x {y} x {z}")
}

/// The button that puts the texture chosen in the browser on the selected brushes (P6-09).
#[must_use]
pub fn apply_texture(name: &str) -> String {
    format!("Apply {name}")
}

/// How many entities besides the one shown a key edit also sets (P5-13).
#[must_use]
pub fn also_selected(count: usize) -> String {
    match count {
        1 => "1 more selected: key edits set it too".to_owned(),
        n => format!("{n} more selected: key edits set them too"),
    }
}

/// How many entities a layer holds (P5-14).
#[must_use]
pub fn layer_count(count: usize) -> String {
    match count {
        1 => "1 entity".to_owned(),
        n => format!("{n} entities"),
    }
}

/// How many brushes an entity has, in the map outline (P5-16).
#[must_use]
pub fn brush_count(count: usize) -> String {
    match count {
        1 => "1 brush".to_owned(),
        n => format!("{n} brushes"),
    }
}

/// The name of the map view's layout `panes` in its menu.
pub(crate) const fn panes_text(panes: qs_layout::MapPanes) -> Text {
    match panes {
        qs_layout::MapPanes::Single => Text::MapLayoutSingle,
        qs_layout::MapPanes::Inset => Text::MapLayoutInset,
        qs_layout::MapPanes::Quad => Text::MapLayoutQuad,
    }
}
