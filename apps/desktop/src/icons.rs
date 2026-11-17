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

/// Whether a row is the one chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Selected {
    /// Chosen: drawn framed.
    Yes,
    /// Not chosen.
    No,
}

/// The class an icon button's node carries for a screen reader's tree and the panels' test.
pub(crate) const ICON_BUTTON: &str = "icon-button";
/// The class a list row's node carries.
pub(crate) const ROW: &str = "row";

/// Names `response`'s node `name` and gives it `class`.
pub(crate) fn mark(ui: &Ui, response: &Response, name: &str, class: &str) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_label(name);
        node.set_class_name(class);
    });
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
    /// `folder-open`: an open folder of the project tree.
    FolderOpen,
    /// `file`: a file of no kind the tree knows.
    File,
    /// `file-code`: a QuakeC file.
    FileCode,
    /// `file-text`: a text or settings file.
    FileText,
    /// `file-box`: a game file: a BSP, a model, a WAD, a sound.
    FileBox,
    /// `square-function`: a function.
    Function,
    /// `variable`: a global.
    Variable,
    /// `tag`: an entity field.
    Field,
    /// `pi`: a constant.
    Constant,
    /// `skull`: a monster.
    Monster,
    /// `package`: an item, a weapon or ammunition.
    Item,
    /// `lightbulb`: a light.
    Light,
    /// `zap`: a trigger.
    Trigger,
    /// `door-open`: a brush entity: a door, a button, a platform.
    Door,
    /// `user`: a player or a player start.
    Player,
    /// `layers`: a layer.
    Layers,
    /// `group`: a group; grouping the selection.
    Group,
    /// `ungroup`: ungrouping.
    Ungroup,
    /// `eye-off`: hiding.
    Hide,
    /// `lock`: locking.
    Lock,
    /// `lock-open`: unlocking.
    Unlock,
    /// `focus`: showing the selection alone.
    Isolate,
    /// `plus`: adding one.
    Add,
    /// `trash-2`: deleting.
    Delete,
    /// `copy`: copying.
    Copy,
    /// `clipboard-paste`: pasting.
    Paste,
    /// `save`: saving.
    Save,
    /// `refresh-cw`: reading again.
    Refresh,
    /// `check-check`: accepting every proposed change.
    AcceptAll,
    /// `x`: rejecting.
    Reject,
    /// `droplets`: checking a map for leaks.
    Leak,
    /// `plane`: flying along a leak.
    Fly,
    /// `search-x`: putting the zoom back.
    ResetZoom,
    /// `file-diff`: comparing two versions.
    Compare,
    /// `user-check`: keeping this window's version.
    KeepMine,
    /// `hard-drive-download`: reading the file on disk again.
    FromDisk,
    /// `squares-subtract`: subtracting brushes.
    Subtract,
    /// `squares-unite`: merging brushes.
    Merge,
    /// `rotate-cw`: turning.
    Turn,
    /// `mouse-pointer-click`: selecting the others like it.
    Siblings,
    /// `align-center`: aligning a texture.
    Align,
    /// `hammer`: compiling the map.
    Compile,
    /// `archive-restore`: restoring a backup.
    Restore,
    /// `unlink`: separating a linked group.
    Separate,
    /// `copy-plus`: duplicating as a linked group.
    LinkDuplicate,
    /// `brick-wall`: making brushes structural.
    Structural,
    /// `split`: extracting from a linked group.
    Extract,
    /// `file-x`: leaving out of an export.
    Omit,
    /// `code-xml`: opening the code behind it.
    OpenCode,
}

impl Icon {
    /// Every icon.
    pub const ALL: [Self; 84] = [
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
        Self::FolderOpen,
        Self::File,
        Self::FileCode,
        Self::FileText,
        Self::FileBox,
        Self::Function,
        Self::Variable,
        Self::Field,
        Self::Constant,
        Self::Monster,
        Self::Item,
        Self::Light,
        Self::Trigger,
        Self::Door,
        Self::Player,
        Self::Layers,
        Self::Group,
        Self::Ungroup,
        Self::Hide,
        Self::Lock,
        Self::Unlock,
        Self::Isolate,
        Self::Add,
        Self::Delete,
        Self::Copy,
        Self::Paste,
        Self::Save,
        Self::Refresh,
        Self::AcceptAll,
        Self::Reject,
        Self::Leak,
        Self::Fly,
        Self::ResetZoom,
        Self::Compare,
        Self::KeepMine,
        Self::FromDisk,
        Self::Subtract,
        Self::Merge,
        Self::Turn,
        Self::Siblings,
        Self::Align,
        Self::Compile,
        Self::Restore,
        Self::Separate,
        Self::LinkDuplicate,
        Self::Structural,
        Self::Extract,
        Self::Omit,
        Self::OpenCode,
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
            Self::FolderOpen => '\u{e247}',
            Self::File => '\u{e0c0}',
            Self::FileCode => '\u{e0c3}',
            Self::FileText => '\u{e0cc}',
            Self::FileBox => '\u{e310}',
            Self::Function => '\u{e22d}',
            Self::Variable => '\u{e473}',
            Self::Field => '\u{e17f}',
            Self::Constant => '\u{e472}',
            Self::Monster => '\u{e221}',
            Self::Item => '\u{e129}',
            Self::Light => '\u{e1c2}',
            Self::Trigger => '\u{e1b4}',
            Self::Door => '\u{e3d6}',
            Self::Player => '\u{e19f}',
            Self::Layers => '\u{e529}',
            Self::Group => '\u{e464}',
            Self::Ungroup => '\u{e467}',
            Self::Hide => '\u{e0bb}',
            Self::Lock => '\u{e10b}',
            Self::Unlock => '\u{e10c}',
            Self::Isolate => '\u{e29e}',
            Self::Add => '\u{e13d}',
            Self::Delete => '\u{e18e}',
            Self::Copy => '\u{e09e}',
            Self::Paste => '\u{e3e8}',
            Self::Save => '\u{e14d}',
            Self::Refresh => '\u{e145}',
            Self::AcceptAll => '\u{e38e}',
            Self::Reject => '\u{e1b2}',
            Self::Leak => '\u{e0b5}',
            Self::Fly => '\u{e1de}',
            Self::ResetZoom => '\u{e4ad}',
            Self::Compare => '\u{e317}',
            Self::KeepMine => '\u{e1a0}',
            Self::FromDisk => '\u{e4e5}',
            Self::Subtract => '\u{e659}',
            Self::Merge => '\u{e65a}',
            Self::Turn => '\u{e149}',
            Self::Siblings => '\u{e120}',
            Self::Align => '\u{e182}',
            Self::Compile => '\u{e0ec}',
            Self::Restore => '\u{e2cd}',
            Self::Separate => '\u{e19c}',
            Self::LinkDuplicate => '\u{e3fd}',
            Self::Structural => '\u{e581}',
            Self::Extract => '\u{e440}',
            Self::Omit => '\u{e0cd}',
            Self::OpenCode => '\u{e206}',
        }
    }

    /// The icon of an entity class's group, by the classname's first word as id's QuakeC names
    /// them: `monster_`, `item_` and `weapon_`, `light`, `trigger_`, `func_`, the player and its
    /// starts, the world; a box for any other.
    #[must_use]
    pub(crate) fn of_class(classname: &str) -> Self {
        match classname.split('_').next().unwrap_or_default() {
            "monster" => Self::Monster,
            "item" | "weapon" | "ammo" => Self::Item,
            "light" => Self::Light,
            "trigger" => Self::Trigger,
            "func" | "door" | "plat" | "train" => Self::Door,
            "player" | "info" => Self::Player,
            "worldspawn" => Self::MapOutline,
            _ => Self::Brush,
        }
    }

    /// The icon of a file's kind, by its extension: QuakeC, a map, a game file, text, or none
    /// the tree knows.
    #[must_use]
    pub(crate) fn of_file(path: &std::path::Path) -> Self {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("qc" | "qh" | "src" | "inc" | "h") => Self::FileCode,
            Some("map") => Self::MapOutline,
            Some(
                "bsp" | "mdl" | "md5mesh" | "md5anim" | "spr" | "wad" | "lmp" | "wav" | "pak"
                | "lit",
            ) => Self::FileBox,
            Some("cfg" | "rc" | "toml" | "txt" | "md" | "json" | "def" | "fgd" | "ent") => {
                Self::FileText
            }
            _ => Self::File,
        }
    }

    /// A panel's action as this icon alone, bare until hovered (P7-16, as the hover toolbar of
    /// Zed's project and outline panels): named `name` for VoiceOver and on hover, and marked
    /// an icon button for the test that no side panel shows a text button.
    pub(crate) fn tool(self, ui: &mut Ui, name: Text) -> Response {
        self.tool_named(ui, name.get())
    }

    /// As [`Icon::tool`], for a name made of others.
    pub(crate) fn tool_named(self, ui: &mut Ui, name: &str) -> Response {
        let button = Button::new(self.char().to_string()).frame_when_inactive(false);
        let response = ui.add(button).on_hover_text(name);
        mark(ui, &response, name, ICON_BUTTON);
        response
    }

    /// A row of a panel's list: this icon, then `label`, selected or not; named `label`
    /// alone for VoiceOver, and marked a row.
    pub(crate) fn row(self, ui: &mut Ui, selected: Selected, label: &str) -> Response {
        let text = format!("{}  {label}", self.char());
        let button = Button::selectable(selected == Selected::Yes, text).frame_when_inactive(false);
        let response = ui.add(button);
        mark(ui, &response, label, ROW);
        response
    }

    /// `label` with this icon before it, for a header that opens and closes.
    #[must_use]
    pub(crate) fn before(self, label: &str) -> String {
        format!("{}  {label}", self.char())
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
