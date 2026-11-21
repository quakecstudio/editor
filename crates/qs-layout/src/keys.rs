// SPDX-License-Identifier: GPL-3.0-or-later

//! The default keys (UI-21, MAP-20, P7-17): a shortcut for every command a person runs from
//! the window with no value to give, TrenchBroom's where it binds the same action, and for
//! every other command why it has none. The window's keymap, `docs/keymap.md` and the test
//! that no command is left out all read these two tables, so they cannot disagree.
//!
//! TrenchBroom's actions are named as `lib/TbUiLib/src/ActionManager.cpp` names them at
//! TrenchBroom/TrenchBroom@8f329f11e65e3d21b355a42bcae199cb3e4e0812; its `Ctrl` is the
//! platform's modifier, `secondary` here (Cmd on macOS).

/// Where a default key is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyContext {
    /// Wherever the keyboard is; but while a pane of the map view has it, a key the view
    /// takes itself ([`VIEW_KEYS`]) is the view's: Ctrl+G groups there and goes to a line
    /// elsewhere, where Ctrl is the platform's modifier.
    Window,
    /// While the code editor does not have the keyboard: it takes these keys itself, with
    /// its own undo history.
    Shell,
    /// While the Map tab is shown and the code editor does not have the keyboard.
    Map,
    /// While a pane of the map view has the keyboard: keys with no platform modifier, which
    /// a text field elsewhere in the tab would otherwise lose.
    MapView,
}

/// What a default key follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Follows {
    /// TrenchBroom's default for the action of this name.
    TrenchBroom(&'static str),
    /// Our own choice, and why.
    Own(&'static str),
}

/// A command's default key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefaultKey {
    /// The command's id.
    pub command: &'static str,
    /// The keys in Zed's spelling: modifiers and one key joined by `-`, `secondary` the
    /// platform's modifier.
    pub keys: &'static str,
    /// Where the keys are taken.
    pub context: KeyContext,
    /// What the key follows.
    pub follows: Follows,
}

/// Why a command has no key of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unkeyed {
    /// It answers a question: a panel shows its answer, and the agent and MCP ask it.
    Query,
    /// The window runs it itself as it draws: a poll, or what it takes from another part.
    Window,
    /// It needs a value a key cannot give, a path, a name, a place: it runs from the command
    /// line, which asks for each, or from the control in the window that has the value.
    Typed,
    /// It is run from a menu or the palette, too seldom for a key to be spent on it.
    Menu,
}

/// Every default key, a command once.
pub const DEFAULT_KEYS: [DefaultKey; 75] = [
    DefaultKey {
        command: "edit.undo",
        keys: "secondary-z",
        context: KeyContext::Shell,
        follows: Follows::TrenchBroom("Menu/Edit/Undo"),
    },
    DefaultKey {
        command: "edit.redo",
        keys: "secondary-shift-z",
        context: KeyContext::Shell,
        follows: Follows::TrenchBroom("Menu/Edit/Redo"),
    },
    DefaultKey {
        command: "edit.repeat",
        keys: "secondary-r",
        context: KeyContext::Shell,
        follows: Follows::TrenchBroom("Menu/Edit/Repeat"),
    },
    DefaultKey {
        command: "map.run_profile",
        keys: "secondary-b",
        context: KeyContext::Shell,
        follows: Follows::Own(
            "the build key of Xcode and Sublime Text; TrenchBroom's Compile Map has no key",
        ),
    },
    DefaultKey {
        command: "view.focus_editor",
        keys: "secondary-1",
        context: KeyContext::Window,
        follows: Follows::Own("VS Code's focus on the first editor group"),
    },
    DefaultKey {
        command: "view.focus_left_panel",
        keys: "secondary-2",
        context: KeyContext::Window,
        follows: Follows::Own("after the editor's, in the window's order"),
    },
    DefaultKey {
        command: "view.focus_right_panel",
        keys: "secondary-3",
        context: KeyContext::Window,
        follows: Follows::Own("after the editor's, in the window's order"),
    },
    DefaultKey {
        command: "view.focus_bottom_panel",
        keys: "secondary-4",
        context: KeyContext::Window,
        follows: Follows::Own("after the editor's, in the window's order"),
    },
    DefaultKey {
        command: "view.focus_command_line",
        keys: "secondary-;",
        context: KeyContext::Window,
        follows: Follows::Own("beside Vim's `:`, which the editor types"),
    },
    DefaultKey {
        command: "editor.fold",
        keys: "secondary-alt-[",
        context: KeyContext::Window,
        follows: Follows::Own("Zed's editor::Fold"),
    },
    DefaultKey {
        command: "editor.unfold",
        keys: "secondary-alt-]",
        context: KeyContext::Window,
        follows: Follows::Own("Zed's editor::UnfoldLines"),
    },
    DefaultKey {
        command: "editor.fold_all",
        keys: "secondary-alt-shift-[",
        context: KeyContext::Window,
        follows: Follows::Own("the fold key with Shift, for every block"),
    },
    DefaultKey {
        command: "editor.unfold_all",
        keys: "secondary-alt-shift-]",
        context: KeyContext::Window,
        follows: Follows::Own("the unfold key with Shift, for every block"),
    },
    DefaultKey {
        command: "editor.go_to_line",
        keys: "ctrl-g",
        context: KeyContext::Window,
        follows: Follows::Own("Zed's go_to_line::Toggle"),
    },
    DefaultKey {
        command: "editor.go_to_symbol",
        keys: "secondary-shift-o",
        context: KeyContext::Window,
        follows: Follows::Own("Zed's outline::Toggle"),
    },
    DefaultKey {
        command: "editor.toggle_soft_wrap",
        keys: "ctrl-alt-w",
        context: KeyContext::Window,
        follows: Follows::Own("Zed has no key; VS Code's Alt+Z types a character on macOS"),
    },
    DefaultKey {
        command: "view.project_search",
        keys: "secondary-shift-f",
        context: KeyContext::Window,
        follows: Follows::Own("Zed's and VS Code's search in the project"),
    },
    DefaultKey {
        command: "view.tab_switcher",
        keys: "ctrl-tab",
        context: KeyContext::Window,
        follows: Follows::Own("Zed's tab_switcher::Toggle"),
    },
    DefaultKey {
        command: "view.toggle_outline",
        keys: "secondary-shift-b",
        context: KeyContext::Window,
        follows: Follows::Own("Zed's outline_panel::ToggleFocus"),
    },
    DefaultKey {
        command: "view.split_right",
        keys: "secondary-\\",
        context: KeyContext::Window,
        follows: Follows::Own("VS Code's split editor"),
    },
    DefaultKey {
        command: "view.split_down",
        keys: "secondary-alt-\\",
        context: KeyContext::Window,
        follows: Follows::Own("the split key with Alt"),
    },
    DefaultKey {
        command: "view.close_split",
        keys: "secondary-shift-\\",
        context: KeyContext::Window,
        follows: Follows::Own("the split key with Shift"),
    },
    DefaultKey {
        command: "view.toggle_embedded_game",
        keys: "secondary-alt-g",
        context: KeyContext::Window,
        follows: Follows::Own("G for the game"),
    },
    DefaultKey {
        command: "preview.play",
        keys: "f5",
        context: KeyContext::Window,
        follows: Follows::Own("VS Code's Start Debugging"),
    },
    DefaultKey {
        command: "preview.stop",
        keys: "shift-f5",
        context: KeyContext::Window,
        follows: Follows::Own("VS Code's Stop"),
    },
    DefaultKey {
        command: "preview.restart",
        keys: "secondary-shift-f5",
        context: KeyContext::Window,
        follows: Follows::Own("VS Code's Restart"),
    },
    DefaultKey {
        command: "preview.pause",
        keys: "f6",
        context: KeyContext::Window,
        follows: Follows::Own("VS Code's Pause"),
    },
    DefaultKey {
        command: "preview.resume",
        keys: "f8",
        context: KeyContext::Window,
        follows: Follows::Own("the resume key of browsers' debuggers; F5 plays"),
    },
    DefaultKey {
        command: "preview.step",
        keys: "f10",
        context: KeyContext::Window,
        follows: Follows::Own("VS Code's Step Over"),
    },
    DefaultKey {
        command: "preview.reload",
        keys: "secondary-shift-r",
        context: KeyContext::Window,
        follows: Follows::Own("a browser's reload"),
    },
    DefaultKey {
        command: "project.build",
        keys: "f7",
        context: KeyContext::Window,
        follows: Follows::Own("Visual Studio's Build"),
    },
    DefaultKey {
        command: "project.check",
        keys: "shift-f7",
        context: KeyContext::Window,
        follows: Follows::Own("the build key with Shift"),
    },
    DefaultKey {
        command: "project.compile",
        keys: "secondary-f7",
        context: KeyContext::Window,
        follows: Follows::Own("the build key with the platform's modifier"),
    },
    DefaultKey {
        command: "project.test",
        keys: "secondary-shift-t",
        context: KeyContext::Window,
        follows: Follows::Own("T for test"),
    },
    DefaultKey {
        command: "project.export",
        keys: "secondary-alt-e",
        context: KeyContext::Window,
        follows: Follows::Own("E for export"),
    },
    DefaultKey {
        command: "checkpoint.create",
        keys: "secondary-alt-s",
        context: KeyContext::Window,
        follows: Follows::Own("a save beside the file's"),
    },
    DefaultKey {
        command: "terminal.new",
        keys: "ctrl-shift-`",
        context: KeyContext::Window,
        follows: Follows::Own("VS Code's Create New Terminal"),
    },
    DefaultKey {
        command: "settings.open",
        keys: "secondary-,",
        context: KeyContext::Window,
        follows: Follows::Own("the settings key of every Mac app, and Zed's"),
    },
    DefaultKey {
        command: "map.save",
        keys: "secondary-s",
        context: KeyContext::Map,
        follows: Follows::TrenchBroom("Menu/File/Save"),
    },
    DefaultKey {
        command: "map.copy",
        keys: "secondary-c",
        context: KeyContext::Map,
        follows: Follows::TrenchBroom("Menu/Edit/Copy"),
    },
    DefaultKey {
        command: "map.select",
        keys: "secondary-shift-a",
        context: KeyContext::Map,
        follows: Follows::TrenchBroom("Menu/Edit/Deselect All"),
    },
    DefaultKey {
        command: "map.show_all",
        keys: "secondary-shift-i",
        context: KeyContext::Map,
        follows: Follows::TrenchBroom("Menu/View/Show All"),
    },
    DefaultKey {
        command: "map.unlock_all",
        keys: "secondary-shift-l",
        context: KeyContext::Map,
        follows: Follows::Own("TrenchBroom has no key; L for lock, as Show All is I"),
    },
    DefaultKey {
        command: "map.find_leaks",
        keys: "secondary-shift-k",
        context: KeyContext::Map,
        follows: Follows::Own("TrenchBroom loads a point file from a menu with no key"),
    },
    DefaultKey {
        command: "map.accept_hunks",
        keys: "secondary-alt-y",
        context: KeyContext::Map,
        follows: Follows::Own("Y for yes, to every change in review"),
    },
    DefaultKey {
        command: "map.reject_hunks",
        keys: "secondary-alt-n",
        context: KeyContext::Map,
        follows: Follows::Own("N for no, to every change in review"),
    },
    DefaultKey {
        command: "map.stage",
        keys: "secondary-alt-r",
        context: KeyContext::Map,
        follows: Follows::Own("R for review"),
    },
    DefaultKey {
        command: "map.unstage",
        keys: "secondary-alt-shift-r",
        context: KeyContext::Map,
        follows: Follows::Own("the review key with Shift"),
    },
    DefaultKey {
        command: "map.backup",
        keys: "secondary-alt-b",
        context: KeyContext::Map,
        follows: Follows::Own("B for backup"),
    },
    DefaultKey {
        command: "map.choose_profile",
        keys: "secondary-alt-m",
        context: KeyContext::Map,
        follows: Follows::Own("back to the Full profile"),
    },
    DefaultKey {
        command: "view.map_top",
        keys: "alt-1",
        context: KeyContext::MapView,
        follows: Follows::Own(
            "TrenchBroom has no standard views; Alt and a digit, in AutoCAD's order of its preset views",
        ),
    },
    DefaultKey {
        command: "view.map_bottom",
        keys: "alt-2",
        context: KeyContext::MapView,
        follows: Follows::Own(
            "TrenchBroom has no standard views; Alt and a digit, in AutoCAD's order of its preset views",
        ),
    },
    DefaultKey {
        command: "view.map_front",
        keys: "alt-3",
        context: KeyContext::MapView,
        follows: Follows::Own(
            "TrenchBroom has no standard views; Alt and a digit, in AutoCAD's order of its preset views",
        ),
    },
    DefaultKey {
        command: "view.map_back",
        keys: "alt-4",
        context: KeyContext::MapView,
        follows: Follows::Own(
            "TrenchBroom has no standard views; Alt and a digit, in AutoCAD's order of its preset views",
        ),
    },
    DefaultKey {
        command: "view.map_left",
        keys: "alt-5",
        context: KeyContext::MapView,
        follows: Follows::Own(
            "TrenchBroom has no standard views; Alt and a digit, in AutoCAD's order of its preset views",
        ),
    },
    DefaultKey {
        command: "view.map_right",
        keys: "alt-6",
        context: KeyContext::MapView,
        follows: Follows::Own(
            "TrenchBroom has no standard views; Alt and a digit, in AutoCAD's order of its preset views",
        ),
    },
    DefaultKey {
        command: "view.map_isometric",
        keys: "alt-7",
        context: KeyContext::MapView,
        follows: Follows::Own(
            "TrenchBroom has no standard views; Alt and a digit, in AutoCAD's order of its preset views",
        ),
    },
    DefaultKey {
        command: "view.map_orbit_left",
        keys: "shift-alt-left",
        context: KeyContext::MapView,
        follows: Follows::Own(
            "TrenchBroom orbits with the mouse only; Shift and Alt, which it leaves free, with an arrow",
        ),
    },
    DefaultKey {
        command: "view.map_orbit_right",
        keys: "shift-alt-right",
        context: KeyContext::MapView,
        follows: Follows::Own(
            "TrenchBroom orbits with the mouse only; Shift and Alt, which it leaves free, with an arrow",
        ),
    },
    DefaultKey {
        command: "view.map_orbit_up",
        keys: "shift-alt-up",
        context: KeyContext::MapView,
        follows: Follows::Own(
            "TrenchBroom orbits with the mouse only; Shift and Alt, which it leaves free, with an arrow",
        ),
    },
    DefaultKey {
        command: "view.map_orbit_down",
        keys: "shift-alt-down",
        context: KeyContext::MapView,
        follows: Follows::Own(
            "TrenchBroom orbits with the mouse only; Shift and Alt, which it leaves free, with an arrow",
        ),
    },
    DefaultKey {
        command: "view.map_pan_left",
        keys: "secondary-shift-left",
        context: KeyContext::MapView,
        follows: Follows::Own(
            "TrenchBroom pans with the mouse only; the platform's modifier and Shift, which it leaves free, with an arrow",
        ),
    },
    DefaultKey {
        command: "view.map_pan_right",
        keys: "secondary-shift-right",
        context: KeyContext::MapView,
        follows: Follows::Own(
            "TrenchBroom pans with the mouse only; the platform's modifier and Shift, which it leaves free, with an arrow",
        ),
    },
    DefaultKey {
        command: "view.map_pan_up",
        keys: "secondary-shift-up",
        context: KeyContext::MapView,
        follows: Follows::Own(
            "TrenchBroom pans with the mouse only; the platform's modifier and Shift, which it leaves free, with an arrow",
        ),
    },
    DefaultKey {
        command: "view.map_pan_down",
        keys: "secondary-shift-down",
        context: KeyContext::MapView,
        follows: Follows::Own(
            "TrenchBroom pans with the mouse only; the platform's modifier and Shift, which it leaves free, with an arrow",
        ),
    },
    DefaultKey {
        command: "view.map_zoom_in",
        keys: "shift-alt-pageup",
        context: KeyContext::MapView,
        follows: Follows::Own(
            "TrenchBroom zooms with the wheel only; the orbit keys' Shift and Alt with Page Up",
        ),
    },
    DefaultKey {
        command: "view.map_zoom_out",
        keys: "shift-alt-pagedown",
        context: KeyContext::MapView,
        follows: Follows::Own(
            "TrenchBroom zooms with the wheel only; the orbit keys' Shift and Alt with Page Down",
        ),
    },
    DefaultKey {
        command: "view.map_view_1",
        keys: "f1",
        context: KeyContext::MapView,
        follows: Follows::Own("TrenchBroom has no saved views; a function key each"),
    },
    DefaultKey {
        command: "view.map_view_2",
        keys: "f2",
        context: KeyContext::MapView,
        follows: Follows::Own("TrenchBroom has no saved views; a function key each"),
    },
    DefaultKey {
        command: "view.map_view_3",
        keys: "f3",
        context: KeyContext::MapView,
        follows: Follows::Own("TrenchBroom has no saved views; a function key each"),
    },
    DefaultKey {
        command: "view.map_view_4",
        keys: "f4",
        context: KeyContext::MapView,
        follows: Follows::Own("TrenchBroom has no saved views; a function key each"),
    },
    DefaultKey {
        command: "view.map_save_view_1",
        keys: "shift-f1",
        context: KeyContext::MapView,
        follows: Follows::Own("the saved view's key with Shift"),
    },
    DefaultKey {
        command: "view.map_save_view_2",
        keys: "shift-f2",
        context: KeyContext::MapView,
        follows: Follows::Own("the saved view's key with Shift"),
    },
    DefaultKey {
        command: "view.map_save_view_3",
        keys: "shift-f3",
        context: KeyContext::MapView,
        follows: Follows::Own("the saved view's key with Shift"),
    },
    DefaultKey {
        command: "view.map_save_view_4",
        keys: "shift-f4",
        context: KeyContext::MapView,
        follows: Follows::Own("the saved view's key with Shift"),
    },
];

/// When a key of the map view's own is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKeyWhen {
    /// Whenever a pane of the map view has the keyboard.
    Always,
    /// While faces are picked for the UV editor: the arrows then move their textures, not
    /// the selection, as TrenchBroom's do.
    FacesPicked,
}

/// A key the map view takes itself rather than through a command: it acts on what the view
/// holds, the selection it draws, its tool, its camera, where a command would have to be
/// given each as a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKey {
    /// Gives up the tool's work in hand, then the selection.
    Cancel,
    /// Frames the selection in the 3D view.
    FocusSelection,
    /// Selects everything shown.
    SelectAll,
    /// Selects what is not selected, and not what is.
    InvertSelection,
    /// Selects what the selected brush covers in a 2D view, however deep.
    SelectTall,
    /// Selects what is inside the selected brush.
    SelectInside,
    /// Selects what touches the selected brush.
    SelectTouching,
    /// Groups the selection.
    Group,
    /// Ungroups the selected group.
    Ungroup,
    /// Duplicates the selected group as a linked group.
    LinkedDuplicate,
    /// Hides the selection.
    Hide,
    /// Hides everything but the selection.
    Isolate,
    /// Duplicates the selection.
    Duplicate,
    /// Deletes the selection.
    Delete,
    /// Flips the selection across the view.
    FlipHorizontal,
    /// Flips the selection top to bottom.
    FlipVertical,
    /// Turns the selection a quarter turn clockwise, seen from above.
    YawClockwise,
    /// Turns the selection a quarter turn counter-clockwise, seen from above.
    YawCounterClockwise,
    /// Moves the selection a grid step forward, or up in a 2D view.
    MoveForward,
    /// Moves the selection a grid step backward, or down in a 2D view.
    MoveBackward,
    /// Moves the selection a grid step left.
    MoveLeft,
    /// Moves the selection a grid step right.
    MoveRight,
    /// Moves the selection a grid step up.
    MoveUp,
    /// Moves the selection a grid step down.
    MoveDown,
    /// Doubles the grid.
    GridCoarser,
    /// Halves the grid.
    GridFiner,
    /// Sets the grid to 1 unit.
    Grid1,
    /// Sets the grid to 2 units.
    Grid2,
    /// Sets the grid to 4 units.
    Grid4,
    /// Sets the grid to 8 units.
    Grid8,
    /// Sets the grid to 16 units.
    Grid16,
    /// Sets the grid to 32 units.
    Grid32,
    /// Sets the grid to 64 units.
    Grid64,
    /// Sets the grid to 128 units.
    Grid128,
    /// Sets the grid to 256 units.
    Grid256,
    /// Flies the 3D view's camera forward while held.
    FlyForward,
    /// Flies the 3D view's camera backward while held.
    FlyBackward,
    /// Flies the 3D view's camera left while held.
    FlyLeft,
    /// Flies the 3D view's camera right while held.
    FlyRight,
    /// Flies the 3D view's camera up while held.
    FlyUp,
    /// Flies the 3D view's camera down while held.
    FlyDown,
    /// Takes the brush tool.
    BrushTool,
    /// Takes the clip tool.
    ClipTool,
    /// Takes the vertex tool.
    VertexTool,
    /// Takes the move tool.
    MoveTool,
    /// Takes the measure tool.
    MeasureTool,
    /// Takes the shape tool.
    ShapeTool,
    /// Takes the hull tool.
    HullTool,
    /// Makes the brush drawn, or cuts with the clip plane placed.
    Confirm,
    /// Moves the picked faces' textures up: by the grid, coarsely with Shift, finely with the platform's modifier.
    TextureUp,
    /// Moves the picked faces' textures down: by the grid, coarsely with Shift, finely with the platform's modifier.
    TextureDown,
    /// Moves the picked faces' textures left: by the grid, coarsely with Shift, finely with the platform's modifier.
    TextureLeft,
    /// Moves the picked faces' textures right: by the grid, coarsely with Shift, finely with the platform's modifier.
    TextureRight,
    /// Turns the picked faces' textures clockwise: by the grid, coarsely with Shift, finely with the platform's modifier.
    TextureClockwise,
    /// Turns the picked faces' textures counter-clockwise: by the grid, coarsely with Shift, finely with the platform's modifier.
    TextureCounterClockwise,
}

/// How far a texture key moves or turns the picked faces' textures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureStep {
    /// By the grid, or the grid's angle.
    Plain,
    /// Coarsely, with Shift.
    Coarse,
    /// Finely, with the platform's modifier.
    Fine,
}

/// A [`ViewKey`]'s keys and what they follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewKeyRow {
    /// The key.
    pub key: ViewKey,
    /// Its keys, in the default keys' spelling: any one of them, or for a texture key the
    /// plain, the coarse and the fine one in that order.
    pub keys: &'static [&'static str],
    /// What it does.
    pub does: &'static str,
    /// When it is taken.
    pub when: ViewKeyWhen,
    /// What it follows.
    pub follows: Follows,
}

impl ViewKey {
    /// The key's keys: any one of them, or for a texture key the plain, the coarse and the
    /// fine one in that order.
    #[must_use]
    pub fn keys(self) -> &'static [&'static str] {
        VIEW_KEYS
            .iter()
            .find(|row| row.key == self)
            .map_or(&[], |row| row.keys)
    }

    /// A texture key's keys for `step`; `None` for a key that is not a texture's.
    #[must_use]
    pub fn keys_for(self, step: TextureStep) -> Option<&'static str> {
        match (self.keys(), step) {
            ([plain, _, _], TextureStep::Plain) => Some(plain),
            ([_, coarse, _], TextureStep::Coarse) => Some(coarse),
            ([_, _, fine], TextureStep::Fine) => Some(fine),
            _ => None,
        }
    }
}

/// Every key the map view takes itself.
pub const VIEW_KEYS: [ViewKeyRow; 55] = [
    ViewKeyRow {
        key: ViewKey::Cancel,
        keys: &["escape"],
        does: "Gives up the tool's work in hand, then the selection",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Controls/Map view/Cancel"),
    },
    ViewKeyRow {
        key: ViewKey::FocusSelection,
        keys: &["secondary-u"],
        does: "Frames the selection in the 3D view",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/View/Camera/Focus on Selection"),
    },
    ViewKeyRow {
        key: ViewKey::SelectAll,
        keys: &["secondary-a"],
        does: "Selects everything shown",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/Edit/Select All"),
    },
    ViewKeyRow {
        key: ViewKey::InvertSelection,
        keys: &["secondary-alt-a"],
        does: "Selects what is not selected, and not what is",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/Edit/Invert Selection"),
    },
    ViewKeyRow {
        key: ViewKey::SelectTall,
        keys: &["secondary-shift-e"],
        does: "Selects what the selected brush covers in a 2D view, however deep",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/Edit/Select Tall"),
    },
    ViewKeyRow {
        key: ViewKey::SelectInside,
        keys: &["secondary-e"],
        does: "Selects what is inside the selected brush",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/Edit/Select Inside"),
    },
    ViewKeyRow {
        key: ViewKey::SelectTouching,
        keys: &["secondary-t"],
        does: "Selects what touches the selected brush",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/Edit/Select Touching"),
    },
    ViewKeyRow {
        key: ViewKey::Group,
        keys: &["secondary-g"],
        does: "Groups the selection",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/Edit/Group"),
    },
    ViewKeyRow {
        key: ViewKey::Ungroup,
        keys: &["secondary-shift-g"],
        does: "Ungroups the selected group",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/Edit/Ungroup"),
    },
    ViewKeyRow {
        key: ViewKey::LinkedDuplicate,
        keys: &["secondary-shift-d"],
        does: "Duplicates the selected group as a linked group",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/Edit/Create Linked Duplicate"),
    },
    ViewKeyRow {
        key: ViewKey::Hide,
        keys: &["secondary-alt-i"],
        does: "Hides the selection",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/View/Hide"),
    },
    ViewKeyRow {
        key: ViewKey::Isolate,
        keys: &["secondary-i"],
        does: "Hides everything but the selection",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/View/Isolate"),
    },
    ViewKeyRow {
        key: ViewKey::Duplicate,
        keys: &["secondary-d"],
        does: "Duplicates the selection",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/Edit/Duplicate"),
    },
    ViewKeyRow {
        key: ViewKey::Delete,
        keys: &["delete", "backspace"],
        does: "Deletes the selection",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/Edit/Delete"),
    },
    ViewKeyRow {
        key: ViewKey::FlipHorizontal,
        keys: &["secondary-f"],
        does: "Flips the selection across the view",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Controls/Map view/Flip objects horizontally"),
    },
    ViewKeyRow {
        key: ViewKey::FlipVertical,
        keys: &["secondary-alt-f"],
        does: "Flips the selection top to bottom",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Controls/Map view/Flip objects vertically"),
    },
    ViewKeyRow {
        key: ViewKey::YawClockwise,
        keys: &["alt-left"],
        does: "Turns the selection a quarter turn clockwise, seen from above",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Controls/Map view/Yaw objects clockwise"),
    },
    ViewKeyRow {
        key: ViewKey::YawCounterClockwise,
        keys: &["alt-right"],
        does: "Turns the selection a quarter turn counter-clockwise, seen from above",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Controls/Map view/Yaw objects counter-clockwise"),
    },
    ViewKeyRow {
        key: ViewKey::MoveForward,
        keys: &["up"],
        does: "Moves the selection a grid step forward, or up in a 2D view",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Controls/Map view/Move objects up; Move objects forward"),
    },
    ViewKeyRow {
        key: ViewKey::MoveBackward,
        keys: &["down"],
        does: "Moves the selection a grid step backward, or down in a 2D view",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Controls/Map view/Move objects down; Move objects backward"),
    },
    ViewKeyRow {
        key: ViewKey::MoveLeft,
        keys: &["left"],
        does: "Moves the selection a grid step left",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Controls/Map view/Move objects left"),
    },
    ViewKeyRow {
        key: ViewKey::MoveRight,
        keys: &["right"],
        does: "Moves the selection a grid step right",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Controls/Map view/Move objects right"),
    },
    ViewKeyRow {
        key: ViewKey::MoveUp,
        keys: &["pageup"],
        does: "Moves the selection a grid step up",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Controls/Map view/Move objects backward; Move objects up"),
    },
    ViewKeyRow {
        key: ViewKey::MoveDown,
        keys: &["pagedown"],
        does: "Moves the selection a grid step down",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Controls/Map view/Move objects forward; Move objects down"),
    },
    ViewKeyRow {
        key: ViewKey::GridCoarser,
        keys: &["+", "="],
        does: "Doubles the grid",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/View/Grid/Increase Grid Size"),
    },
    ViewKeyRow {
        key: ViewKey::GridFiner,
        keys: &["-"],
        does: "Halves the grid",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/View/Grid/Decrease Grid Size"),
    },
    ViewKeyRow {
        key: ViewKey::Grid1,
        keys: &["1"],
        does: "Sets the grid to 1 unit",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/View/Grid/Set Grid Size 1"),
    },
    ViewKeyRow {
        key: ViewKey::Grid2,
        keys: &["2"],
        does: "Sets the grid to 2 units",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/View/Grid/Set Grid Size 2"),
    },
    ViewKeyRow {
        key: ViewKey::Grid4,
        keys: &["3"],
        does: "Sets the grid to 4 units",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/View/Grid/Set Grid Size 4"),
    },
    ViewKeyRow {
        key: ViewKey::Grid8,
        keys: &["4"],
        does: "Sets the grid to 8 units",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/View/Grid/Set Grid Size 8"),
    },
    ViewKeyRow {
        key: ViewKey::Grid16,
        keys: &["5"],
        does: "Sets the grid to 16 units",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/View/Grid/Set Grid Size 16"),
    },
    ViewKeyRow {
        key: ViewKey::Grid32,
        keys: &["6"],
        does: "Sets the grid to 32 units",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/View/Grid/Set Grid Size 32"),
    },
    ViewKeyRow {
        key: ViewKey::Grid64,
        keys: &["7"],
        does: "Sets the grid to 64 units",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/View/Grid/Set Grid Size 64"),
    },
    ViewKeyRow {
        key: ViewKey::Grid128,
        keys: &["8"],
        does: "Sets the grid to 128 units",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/View/Grid/Set Grid Size 128"),
    },
    ViewKeyRow {
        key: ViewKey::Grid256,
        keys: &["9"],
        does: "Sets the grid to 256 units",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/View/Grid/Set Grid Size 256"),
    },
    ViewKeyRow {
        key: ViewKey::FlyForward,
        keys: &["w"],
        does: "Flies the 3D view's camera forward while held",
        when: ViewKeyWhen::Always,
        follows: Follows::Own(
            "TrenchBroom's fly keys, which its preferences hold, not its actions",
        ),
    },
    ViewKeyRow {
        key: ViewKey::FlyBackward,
        keys: &["s"],
        does: "Flies the 3D view's camera backward while held",
        when: ViewKeyWhen::Always,
        follows: Follows::Own(
            "TrenchBroom's fly keys, which its preferences hold, not its actions",
        ),
    },
    ViewKeyRow {
        key: ViewKey::FlyLeft,
        keys: &["a"],
        does: "Flies the 3D view's camera left while held",
        when: ViewKeyWhen::Always,
        follows: Follows::Own(
            "TrenchBroom's fly keys, which its preferences hold, not its actions",
        ),
    },
    ViewKeyRow {
        key: ViewKey::FlyRight,
        keys: &["d"],
        does: "Flies the 3D view's camera right while held",
        when: ViewKeyWhen::Always,
        follows: Follows::Own(
            "TrenchBroom's fly keys, which its preferences hold, not its actions",
        ),
    },
    ViewKeyRow {
        key: ViewKey::FlyUp,
        keys: &["q"],
        does: "Flies the 3D view's camera up while held",
        when: ViewKeyWhen::Always,
        follows: Follows::Own(
            "TrenchBroom's fly keys, which its preferences hold, not its actions",
        ),
    },
    ViewKeyRow {
        key: ViewKey::FlyDown,
        keys: &["x"],
        does: "Flies the 3D view's camera down while held",
        when: ViewKeyWhen::Always,
        follows: Follows::Own(
            "TrenchBroom's fly keys, which its preferences hold, not its actions",
        ),
    },
    ViewKeyRow {
        key: ViewKey::BrushTool,
        keys: &["b"],
        does: "Takes the brush tool",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/Edit/Tools/Brush Tool"),
    },
    ViewKeyRow {
        key: ViewKey::ClipTool,
        keys: &["c"],
        does: "Takes the clip tool",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/Edit/Tools/Clip Tool"),
    },
    ViewKeyRow {
        key: ViewKey::VertexTool,
        keys: &["v"],
        does: "Takes the vertex tool",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Menu/Edit/Tools/Vertex Tool"),
    },
    ViewKeyRow {
        key: ViewKey::MoveTool,
        keys: &["m"],
        does: "Takes the move tool",
        when: ViewKeyWhen::Always,
        follows: Follows::Own("TrenchBroom moves with the select tool; M for move"),
    },
    ViewKeyRow {
        key: ViewKey::MeasureTool,
        keys: &["l"],
        does: "Takes the measure tool",
        when: ViewKeyWhen::Always,
        follows: Follows::Own("TrenchBroom has no measure tool; L for length"),
    },
    ViewKeyRow {
        key: ViewKey::ShapeTool,
        keys: &["p"],
        does: "Takes the shape tool",
        when: ViewKeyWhen::Always,
        follows: Follows::Own("TrenchBroom picks a shape in its brush tool; P for primitive"),
    },
    ViewKeyRow {
        key: ViewKey::HullTool,
        keys: &["h"],
        does: "Takes the hull tool",
        when: ViewKeyWhen::Always,
        follows: Follows::Own("TrenchBroom makes a hull in its brush tool; H for hull"),
    },
    ViewKeyRow {
        key: ViewKey::Confirm,
        keys: &["enter"],
        does: "Makes the brush drawn, or cuts with the clip plane placed",
        when: ViewKeyWhen::Always,
        follows: Follows::TrenchBroom("Controls/Map view/Create brush"),
    },
    ViewKeyRow {
        key: ViewKey::TextureUp,
        keys: &["up", "shift-up", "secondary-up"],
        does: "Moves the picked faces' textures up: by the grid, coarsely with Shift, finely with the platform's modifier",
        when: ViewKeyWhen::FacesPicked,
        follows: Follows::TrenchBroom("Controls/Map view/Move textures up"),
    },
    ViewKeyRow {
        key: ViewKey::TextureDown,
        keys: &["down", "shift-down", "secondary-down"],
        does: "Moves the picked faces' textures down: by the grid, coarsely with Shift, finely with the platform's modifier",
        when: ViewKeyWhen::FacesPicked,
        follows: Follows::TrenchBroom("Controls/Map view/Move textures down"),
    },
    ViewKeyRow {
        key: ViewKey::TextureLeft,
        keys: &["left", "shift-left", "secondary-left"],
        does: "Moves the picked faces' textures left: by the grid, coarsely with Shift, finely with the platform's modifier",
        when: ViewKeyWhen::FacesPicked,
        follows: Follows::TrenchBroom("Controls/Map view/Move textures left"),
    },
    ViewKeyRow {
        key: ViewKey::TextureRight,
        keys: &["right", "shift-right", "secondary-right"],
        does: "Moves the picked faces' textures right: by the grid, coarsely with Shift, finely with the platform's modifier",
        when: ViewKeyWhen::FacesPicked,
        follows: Follows::TrenchBroom("Controls/Map view/Move textures right"),
    },
    ViewKeyRow {
        key: ViewKey::TextureClockwise,
        keys: &["pageup", "shift-pageup", "secondary-pageup"],
        does: "Turns the picked faces' textures clockwise: by the grid, coarsely with Shift, finely with the platform's modifier",
        when: ViewKeyWhen::FacesPicked,
        follows: Follows::TrenchBroom("Controls/Map view/Rotate textures clockwise"),
    },
    ViewKeyRow {
        key: ViewKey::TextureCounterClockwise,
        keys: &["pagedown", "shift-pagedown", "secondary-pagedown"],
        does: "Turns the picked faces' textures counter-clockwise: by the grid, coarsely with Shift, finely with the platform's modifier",
        when: ViewKeyWhen::FacesPicked,
        follows: Follows::TrenchBroom("Controls/Map view/Rotate textures counter-clockwise"),
    },
];

/// Every command with no key of its own, and why.
pub const UNKEYED: [(&str, Unkeyed); 179] = [
    ("checkpoint.list", Unkeyed::Query),
    ("checkpoint.restore", Unkeyed::Typed),
    ("code.actions", Unkeyed::Typed),
    ("code.fix", Unkeyed::Typed),
    ("commands.list", Unkeyed::Query),
    ("edit.accept", Unkeyed::Typed),
    ("edit.stage_change", Unkeyed::Typed),
    ("edit.reject", Unkeyed::Typed),
    ("edit.staged", Unkeyed::Query),
    ("engine.builtins", Unkeyed::Query),
    ("engine.console", Unkeyed::Typed),
    ("engine.inspect", Unkeyed::Typed),
    ("engine.screenshot", Unkeyed::Typed),
    ("file.close", Unkeyed::Typed),
    ("file.format", Unkeyed::Typed),
    ("file.inlay_hints", Unkeyed::Typed),
    ("file.open", Unkeyed::Typed),
    ("file.read", Unkeyed::Typed),
    ("file.replace_all", Unkeyed::Typed),
    ("file.replace_range", Unkeyed::Typed),
    ("file.save", Unkeyed::Typed),
    ("file.update", Unkeyed::Typed),
    ("files.changed", Unkeyed::Typed),
    ("git.blame", Unkeyed::Typed),
    ("git.branches", Unkeyed::Query),
    ("git.commit", Unkeyed::Typed),
    ("git.conflicts", Unkeyed::Typed),
    ("git.create_branch", Unkeyed::Typed),
    ("git.diff", Unkeyed::Typed),
    ("git.head_text", Unkeyed::Typed),
    ("git.log", Unkeyed::Typed),
    ("git.resolve", Unkeyed::Typed),
    ("git.stage", Unkeyed::Typed),
    ("git.status", Unkeyed::Query),
    ("git.switch", Unkeyed::Typed),
    ("git.unstage", Unkeyed::Typed),
    ("help.about", Unkeyed::Menu),
    ("help.report_issue", Unkeyed::Menu),
    ("help.user_guide", Unkeyed::Menu),
    ("map.add_layer", Unkeyed::Typed),
    ("map.add_wad", Unkeyed::Typed),
    ("map.backups", Unkeyed::Query),
    ("map.brush_chamfer", Unkeyed::Typed),
    ("map.brush_clip", Unkeyed::Typed),
    ("map.brush_create", Unkeyed::Typed),
    ("map.brush_delete", Unkeyed::Typed),
    ("map.brush_extrude", Unkeyed::Typed),
    ("map.brush_hollow", Unkeyed::Typed),
    ("map.brush_hull", Unkeyed::Typed),
    ("map.brush_intersect", Unkeyed::Typed),
    ("map.brush_merge", Unkeyed::Typed),
    ("map.brush_move", Unkeyed::Typed),
    ("map.brush_scale", Unkeyed::Typed),
    ("map.brush_shape", Unkeyed::Typed),
    ("map.brush_shear", Unkeyed::Typed),
    ("map.brush_split_edge", Unkeyed::Typed),
    ("map.brush_split_face", Unkeyed::Typed),
    ("map.brush_subtract", Unkeyed::Typed),
    ("map.brush_sweep", Unkeyed::Typed),
    ("map.brush_texture", Unkeyed::Typed),
    ("map.brush_vertex", Unkeyed::Typed),
    ("map.brush_vertices", Unkeyed::Typed),
    ("map.check", Unkeyed::Typed),
    ("map.compare_with_disk", Unkeyed::Query),
    ("map.compile", Unkeyed::Typed),
    ("map.delete_entity", Unkeyed::Typed),
    ("map.duplicate_entity", Unkeyed::Typed),
    ("map.read_entity", Unkeyed::Typed),
    ("map.export", Unkeyed::Typed),
    ("map.extract_linked", Unkeyed::Typed),
    ("map.face_align", Unkeyed::Typed),
    ("map.face_uv", Unkeyed::Typed),
    ("map.entities_removed_by_skill", Unkeyed::Query),
    ("map.apply_fix", Unkeyed::Typed),
    ("map.flip", Unkeyed::Typed),
    ("map.group", Unkeyed::Typed),
    ("map.hide", Unkeyed::Typed),
    ("map.list_entities", Unkeyed::Query),
    ("map.brushes", Unkeyed::Query),
    ("map.summary", Unkeyed::Query),
    ("map.isolate", Unkeyed::Typed),
    ("map.issues", Unkeyed::Query),
    ("map.layers", Unkeyed::Query),
    ("map.link_target", Unkeyed::Typed),
    ("map.link_duplicate", Unkeyed::Typed),
    ("map.lock", Unkeyed::Typed),
    ("map.make_brush_entity", Unkeyed::Typed),
    ("map.make_structural", Unkeyed::Typed),
    ("map.move_entity", Unkeyed::Typed),
    ("map.move_to_layer_or_group", Unkeyed::Typed),
    ("map.new", Unkeyed::Typed),
    ("map.generate", Unkeyed::Menu),
    ("map.open", Unkeyed::Typed),
    ("map.outline", Unkeyed::Query),
    ("map.paste", Unkeyed::Typed),
    ("map.place_entity", Unkeyed::Typed),
    ("map.place_prefab", Unkeyed::Typed),
    ("map.play_from_here", Unkeyed::Typed),
    ("map.portals", Unkeyed::Query),
    ("map.prefabs", Unkeyed::Query),
    ("map.profiles", Unkeyed::Query),
    ("map.remove_key", Unkeyed::Typed),
    ("map.rename_key", Unkeyed::Typed),
    ("map.replace_texture", Unkeyed::Typed),
    ("map.resolve_disk_change", Unkeyed::Typed),
    ("map.restore_backup", Unkeyed::Typed),
    ("map.rotate_entity", Unkeyed::Typed),
    ("map.save_prefab", Unkeyed::Typed),
    ("map.search", Unkeyed::Typed),
    ("map.unlink_group", Unkeyed::Typed),
    ("map.set_flag", Unkeyed::Typed),
    ("map.set_key", Unkeyed::Typed),
    ("map.set_layer", Unkeyed::Typed),
    ("map.staged_hunks", Unkeyed::Query),
    ("map.list_textures", Unkeyed::Query),
    ("map.rotate_objects", Unkeyed::Typed),
    ("map.ungroup", Unkeyed::Typed),
    ("paths.open", Unkeyed::Typed),
    ("paths.take_opened", Unkeyed::Window),
    ("preview.breakpoint", Unkeyed::Typed),
    ("preview.choose_game", Unkeyed::Typed),
    ("preview.console", Unkeyed::Typed),
    ("preview.dock", Unkeyed::Typed),
    ("preview.game_files", Unkeyed::Query),
    ("preview.pick", Unkeyed::Typed),
    ("preview.poll", Unkeyed::Window),
    ("preview.select", Unkeyed::Typed),
    ("preview.set_field", Unkeyed::Typed),
    ("preview.step_by", Unkeyed::Typed),
    ("preview.unwatch", Unkeyed::Typed),
    ("preview.watch", Unkeyed::Typed),
    ("model.add_frames", Unkeyed::Typed),
    ("model.copy_from_game", Unkeyed::Typed),
    ("model.export_skin", Unkeyed::Typed),
    ("model.import", Unkeyed::Typed),
    ("model.inspect", Unkeyed::Query),
    ("model.move", Unkeyed::Typed),
    ("model.recolor", Unkeyed::Typed),
    ("model.remove_frames", Unkeyed::Typed),
    ("model.rename_frames", Unkeyed::Typed),
    ("model.scale", Unkeyed::Typed),
    ("model.set_skin", Unkeyed::Typed),
    ("project.entities", Unkeyed::Query),
    ("project.files", Unkeyed::Query),
    ("project.info", Unkeyed::Query),
    ("project.reference", Unkeyed::Query),
    ("project.new", Unkeyed::Typed),
    ("project.open", Unkeyed::Typed),
    ("project.search", Unkeyed::Typed),
    ("project.symbols", Unkeyed::Query),
    ("project.take_checked", Unkeyed::Window),
    ("scenario.run", Unkeyed::Typed),
    ("settings.get", Unkeyed::Typed),
    ("settings.list", Unkeyed::Query),
    ("settings.reset", Unkeyed::Typed),
    ("settings.set", Unkeyed::Typed),
    ("symbol.complete", Unkeyed::Typed),
    ("symbol.definition", Unkeyed::Typed),
    ("symbol.hover", Unkeyed::Typed),
    ("symbol.references", Unkeyed::Typed),
    ("symbol.rename", Unkeyed::Typed),
    ("task.list", Unkeyed::Query),
    ("task.run", Unkeyed::Typed),
    ("terminal.close", Unkeyed::Typed),
    ("terminal.poll", Unkeyed::Window),
    ("terminal.send", Unkeyed::Typed),
    ("texture.import", Unkeyed::Typed),
    ("view.map_camera", Unkeyed::Query),
    ("view.map_grid", Unkeyed::Typed),
    ("view.map_grid_step", Unkeyed::Typed),
    ("view.map_keep_view", Unkeyed::Window),
    ("view.map_panes", Unkeyed::Typed),
    ("view.map_shading", Unkeyed::Typed),
    ("view.move_panel", Unkeyed::Typed),
    ("view.resize_panel", Unkeyed::Typed),
    ("view.show_panel", Unkeyed::Typed),
    ("view.toggle_panel", Unkeyed::Typed),
    ("view.zoom", Unkeyed::Typed),
    ("vm.run", Unkeyed::Typed),
];

impl DefaultKey {
    /// The default key of the command `id`, if it has one.
    #[must_use]
    pub fn of(id: &str) -> Option<&'static Self> {
        DEFAULT_KEYS.iter().find(|key| key.command == id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        DEFAULT_KEYS, DefaultKey, Follows, KeyContext, TextureStep, UNKEYED, VIEW_KEYS, ViewKey,
        ViewKeyWhen,
    };

    // No spec statement: UI-21, #971: no two default keys are the same, since a key held in one
    // context is held in the window's too; and no command is in both tables or twice in one
    #[test]
    fn no_two_default_keys_collide_and_no_command_is_listed_twice() {
        let keys: BTreeSet<&str> = DEFAULT_KEYS.iter().map(|key| key.keys).collect();
        assert_eq!(keys.len(), DEFAULT_KEYS.len(), "two commands share keys");
        let mut commands: Vec<&str> = DEFAULT_KEYS
            .iter()
            .map(|key| key.command)
            .chain(UNKEYED.iter().map(|(command, _)| *command))
            .collect();
        let listed = commands.len();
        commands.sort_unstable();
        commands.dedup();
        assert_eq!(commands.len(), listed, "a command is listed twice");
    }

    // No spec statement: MAP-20, #971: a key that follows TrenchBroom names the action, and a
    // key of our own says why; a command's key is found by its id
    #[test]
    fn every_default_key_says_what_it_follows() {
        for key in &DEFAULT_KEYS {
            let said = match key.follows {
                Follows::TrenchBroom(action) => action.starts_with("Menu/"),
                Follows::Own(why) => !why.is_empty(),
            };
            assert!(said, "{}", key.command);
        }
        assert_eq!(
            DefaultKey::of("edit.repeat").map(|key| key.keys),
            Some("secondary-r")
        );
        assert_eq!(DefaultKey::of("file.open"), None);
    }

    /// `keys` as one platform holds them: `secondary` as `platform`'s modifier, the modifiers
    /// in one order.
    fn held(keys: &str, platform: &str) -> String {
        // `-` itself is the key when the keys end with it.
        let (key, modifiers) = match keys.strip_suffix('-') {
            Some(modifiers) => ("-", modifiers),
            None => keys.rsplit_once('-').map_or((keys, ""), |(m, k)| (k, m)),
        };
        let mut modifiers: Vec<&str> = modifiers
            .split('-')
            .filter(|modifier| !modifier.is_empty())
            .map(|modifier| {
                if modifier == "secondary" {
                    platform
                } else {
                    modifier
                }
            })
            .collect();
        modifiers.sort_unstable();
        modifiers.push(key);
        modifiers.join("-")
    }

    // No spec statement: MAP-20, #971: the keys the map view takes itself are each one key's
    // while they are taken together, and none is a default key taken in the Map tab, on macOS
    // (Cmd) or elsewhere (Ctrl), which the window would take first; a key taken anywhere
    // gives way to the view's; each says what it follows
    #[test]
    fn the_map_views_own_keys_collide_with_nothing() {
        assert_eq!(held("secondary-alt-shift-[", "cmd"), "alt-cmd-shift-[");
        assert_eq!(held("secondary--", "ctrl"), "ctrl--");
        assert_eq!(held("-", "ctrl"), "-");
        for platform in ["cmd", "ctrl"] {
            let mut taken: BTreeSet<String> = DEFAULT_KEYS
                .iter()
                .filter(|key| key.context != KeyContext::Window)
                .map(|key| held(key.keys, platform))
                .collect();
            for row in VIEW_KEYS
                .iter()
                .filter(|row| row.when == ViewKeyWhen::Always)
            {
                for keys in row.keys {
                    assert!(
                        taken.insert(held(keys, platform)),
                        "{keys} of {:?} is taken on {platform}",
                        row.key
                    );
                }
            }
        }
        for row in &VIEW_KEYS {
            let said = match row.follows {
                Follows::TrenchBroom(action) => {
                    action.starts_with("Menu/") || action.starts_with("Controls/Map view/")
                }
                Follows::Own(why) => !why.is_empty(),
            };
            assert!(
                said && !row.does.is_empty() && !row.keys.is_empty(),
                "{:?}",
                row.key
            );
            assert_eq!(row.key.keys(), row.keys);
        }
        assert_eq!(ViewKey::GridFiner.keys(), ["-"]);
        assert_eq!(ViewKey::GridFiner.keys_for(TextureStep::Plain), None);
        let up = |step| ViewKey::TextureUp.keys_for(step);
        assert_eq!(
            [TextureStep::Plain, TextureStep::Coarse, TextureStep::Fine].map(up),
            [Some("up"), Some("shift-up"), Some("secondary-up")]
        );
        // A texture key has its plain, coarse and fine keys.
        let faces = VIEW_KEYS
            .iter()
            .filter(|row| row.when == ViewKeyWhen::FacesPicked);
        assert!(faces.clone().count() == 6 && faces.clone().all(|row| row.keys.len() == 3));
    }
    /// TrenchBroom's default key for each action a key here follows, as `ActionManager.cpp`
    /// writes it at the pinned commit: its `KeySequence`, or the `StandardShortcut` it asks
    /// the platform for.
    const TRENCHBROOM: [(&str, &str); 64] = [
        ("Controls/Map view/Cancel", "Esc"),
        ("Controls/Map view/Create brush", "Return"),
        ("Controls/Map view/Flip objects horizontally", "Ctrl+F"),
        ("Controls/Map view/Flip objects vertically", "Ctrl+Alt+F"),
        (
            "Controls/Map view/Move objects backward; Move objects up",
            "PgUp",
        ),
        (
            "Controls/Map view/Move objects down; Move objects backward",
            "Down",
        ),
        (
            "Controls/Map view/Move objects forward; Move objects down",
            "PgDown",
        ),
        ("Controls/Map view/Move objects left", "Left"),
        ("Controls/Map view/Move objects right", "Right"),
        (
            "Controls/Map view/Move objects up; Move objects forward",
            "Up",
        ),
        ("Controls/Map view/Move textures down", "Down"),
        (
            "Controls/Map view/Move textures down (coarse)",
            "Shift+Down",
        ),
        ("Controls/Map view/Move textures down (fine)", "Ctrl+Down"),
        ("Controls/Map view/Move textures left", "Left"),
        (
            "Controls/Map view/Move textures left (coarse)",
            "Shift+Left",
        ),
        ("Controls/Map view/Move textures left (fine)", "Ctrl+Left"),
        ("Controls/Map view/Move textures right", "Right"),
        (
            "Controls/Map view/Move textures right (coarse)",
            "Shift+Right",
        ),
        ("Controls/Map view/Move textures right (fine)", "Ctrl+Right"),
        ("Controls/Map view/Move textures up", "Up"),
        ("Controls/Map view/Move textures up (coarse)", "Shift+Up"),
        ("Controls/Map view/Move textures up (fine)", "Ctrl+Up"),
        ("Controls/Map view/Rotate textures clockwise", "PgUp"),
        (
            "Controls/Map view/Rotate textures clockwise (coarse)",
            "Shift+PgUp",
        ),
        (
            "Controls/Map view/Rotate textures clockwise (fine)",
            "Ctrl+PgUp",
        ),
        (
            "Controls/Map view/Rotate textures counter-clockwise",
            "PgDown",
        ),
        (
            "Controls/Map view/Rotate textures counter-clockwise (coarse)",
            "Shift+PgDown",
        ),
        (
            "Controls/Map view/Rotate textures counter-clockwise (fine)",
            "Ctrl+PgDown",
        ),
        ("Controls/Map view/Yaw objects clockwise", "Alt+Left"),
        (
            "Controls/Map view/Yaw objects counter-clockwise",
            "Alt+Right",
        ),
        ("Menu/Edit/Copy", "StandardShortcut::Copy"),
        ("Menu/Edit/Create Linked Duplicate", "Ctrl+Shift+D"),
        ("Menu/Edit/Delete", "StandardShortcut::Delete"),
        ("Menu/Edit/Deselect All", "Ctrl+Shift+A"),
        ("Menu/Edit/Duplicate", "Ctrl+D"),
        ("Menu/Edit/Group", "Ctrl+G"),
        ("Menu/Edit/Invert Selection", "Ctrl+Alt+A"),
        ("Menu/Edit/Redo", "StandardShortcut::Redo"),
        ("Menu/Edit/Repeat", "Ctrl+R"),
        ("Menu/Edit/Select All", "StandardShortcut::SelectAll"),
        ("Menu/Edit/Select Inside", "Ctrl+E"),
        ("Menu/Edit/Select Tall", "Ctrl+Shift+E"),
        ("Menu/Edit/Select Touching", "Ctrl+T"),
        ("Menu/Edit/Tools/Brush Tool", "B"),
        ("Menu/Edit/Tools/Clip Tool", "C"),
        ("Menu/Edit/Tools/Vertex Tool", "V"),
        ("Menu/Edit/Undo", "StandardShortcut::Undo"),
        ("Menu/Edit/Ungroup", "Ctrl+Shift+G"),
        ("Menu/File/Save", "StandardShortcut::Save"),
        ("Menu/View/Camera/Focus on Selection", "Ctrl+U"),
        ("Menu/View/Grid/Decrease Grid Size", "-"),
        ("Menu/View/Grid/Increase Grid Size", "+"),
        ("Menu/View/Grid/Set Grid Size 1", "1"),
        ("Menu/View/Grid/Set Grid Size 128", "8"),
        ("Menu/View/Grid/Set Grid Size 16", "5"),
        ("Menu/View/Grid/Set Grid Size 2", "2"),
        ("Menu/View/Grid/Set Grid Size 256", "9"),
        ("Menu/View/Grid/Set Grid Size 32", "6"),
        ("Menu/View/Grid/Set Grid Size 4", "3"),
        ("Menu/View/Grid/Set Grid Size 64", "7"),
        ("Menu/View/Grid/Set Grid Size 8", "4"),
        ("Menu/View/Hide", "Ctrl+Alt+I"),
        ("Menu/View/Isolate", "Ctrl+I"),
        ("Menu/View/Show All", "Ctrl+Shift+I"),
    ];

    /// TrenchBroom's `keys` in this table's spelling: Qt's names for ours, `Ctrl` the
    /// platform's modifier, and a standard shortcut as the platform's key for it.
    fn ours(keys: &str) -> String {
        let standard = match keys {
            "StandardShortcut::Undo" => Some("secondary-z"),
            "StandardShortcut::Redo" => Some("secondary-shift-z"),
            "StandardShortcut::Save" => Some("secondary-s"),
            "StandardShortcut::Copy" => Some("secondary-c"),
            "StandardShortcut::SelectAll" => Some("secondary-a"),
            "StandardShortcut::Delete" => Some("delete"),
            _ => None,
        };
        if let Some(standard) = standard {
            return standard.to_owned();
        }
        let parts: Vec<String> = keys
            .split('+')
            .map(|part| match part {
                "Ctrl" => "secondary".to_owned(),
                "Return" => "enter".to_owned(),
                "Esc" => "escape".to_owned(),
                "PgUp" => "pageup".to_owned(),
                "PgDown" => "pagedown".to_owned(),
                // `+` itself, which the split took as a separator.
                "" => "+".to_owned(),
                other => other.to_lowercase(),
            })
            .collect();
        let mut parts = parts;
        parts.dedup_by(|a, b| a == "+" && b == "+");
        parts.join("-")
    }

    // No spec statement: MAP-20, #971: every key that follows TrenchBroom is TrenchBroom's
    // default for the action it names, at the pinned commit; a texture key's coarse and fine
    // keys are those of TrenchBroom's coarse and fine actions
    #[test]
    fn every_key_that_follows_trenchbroom_is_trenchbrooms() {
        let default = |action: &str| {
            TRENCHBROOM
                .iter()
                .find(|(named, _)| *named == action)
                .map(|(_, keys)| ours(keys))
        };
        assert_eq!(ours("Ctrl+Alt+F"), "secondary-alt-f");
        assert_eq!(ours("+"), "+");
        assert_eq!(ours("PgUp"), "pageup");
        for key in &DEFAULT_KEYS {
            if let Follows::TrenchBroom(action) = key.follows {
                assert_eq!(default(action).as_deref(), Some(key.keys), "{action}");
            }
        }
        for row in &VIEW_KEYS {
            let Follows::TrenchBroom(action) = row.follows else {
                continue;
            };
            assert_eq!(
                default(action).as_deref(),
                row.keys.first().copied(),
                "{action}"
            );
            if row.when == ViewKeyWhen::FacesPicked {
                let coarse = default(&format!("{action} (coarse)"));
                let fine = default(&format!("{action} (fine)"));
                assert_eq!(coarse.as_deref(), row.key.keys_for(TextureStep::Coarse));
                assert_eq!(fine.as_deref(), row.key.keys_for(TextureStep::Fine));
            }
        }
    }
}
