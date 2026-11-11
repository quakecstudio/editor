// SPDX-License-Identifier: GPL-3.0-or-later

//! The workbench layout (UI-2): which of the side and bottom panels are shown and how big each
//! is, read from and written to a file so it survives a restart, and the commands that change
//! it, so the toolbar, a menu, a keybinding, the CLI and the agent toggle a panel the same way
//! (PRIN-3).
//!
//! A layout change is not an edit: it is made outside the undo history
//! ([`qs_commands::Outcome::outside_history`]), as Zed and VS Code leave a hidden panel out of
//! undo.

use std::convert::Infallible;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use qs_commands::{Command, NoParams, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

mod editor;
#[cfg(test)]
mod tests;

pub use editor::{
    CLOSE_SPLIT, EditorRequest, FOLD, FOLD_ALL, GO_TO_LINE, GO_TO_SYMBOL, GoToLineParams, Outline,
    PROJECT_SEARCH_VIEW, SPLIT_DOWN, SPLIT_RIGHT, SoftWrap, Split, TAB_SWITCHER, TOGGLE_OUTLINE,
    TOGGLE_SOFT_WRAP, UNFOLD, UNFOLD_ALL,
};

/// The layout file's name in the app's data directory (`qs_paths::AppDirs::data`).
pub(crate) const LAYOUT_FILE: &str = "layout.toml";

/// The layout file in the data directory `data_dir`.
#[must_use]
pub fn layout_path(data_dir: &Path) -> PathBuf {
    data_dir.join(LAYOUT_FILE)
}

/// One of the panels around the centre (UI-2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Panel {
    /// Project and Entities.
    Left,
    /// Properties and the agent.
    Right,
    /// Problems, Console and the debugger.
    Bottom,
}

impl Panel {
    /// Every panel, left, right, then bottom.
    pub const ALL: [Self; 3] = [Self::Left, Self::Right, Self::Bottom];
}

/// The area that has the keyboard (QA-9): the editor, or a panel, which a screen reader
/// then reads from its heading.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Focus {
    /// The centre: the editor, or the centre tab shown when no file is open.
    #[default]
    Editor,
    /// A panel around the centre.
    Panel(Panel),
}

/// One panel's state.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelState {
    /// Whether it is shown.
    pub visible: bool,
    /// Its width, or height for the bottom panel, in points.
    pub size: f32,
}

/// The workbench layout, as the layout file holds it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Layout {
    /// Project and Entities.
    pub left: PanelState,
    /// Properties and the agent.
    pub right: PanelState,
    /// Problems, Console and the debugger.
    pub bottom: PanelState,
    /// The area the last focus command gave the keyboard. Not written to the file: the
    /// keyboard starts in the editor.
    #[serde(skip)]
    pub focus: Focus,
    /// How many focus commands have run: each one is applied, including one naming the area
    /// the keyboard was moved away from by a click, so a window compares this count rather
    /// than the area. Not written to the file.
    #[serde(skip)]
    pub focus_requests: u64,
    /// The editor's panes (P4-32).
    pub split: Split,
    /// Whether long lines wrap in the editor.
    pub soft_wrap: SoftWrap,
    /// Whether the outline panel is shown beside the editor.
    pub outline: Outline,
    /// The last one-off editor action a command asked for, which the window carries out once.
    /// Not written to the file.
    #[serde(skip)]
    pub editor_request: Option<EditorRequest>,
    /// How many editor actions have been asked for, so a window carries out each once. Not
    /// written to the file.
    #[serde(skip)]
    pub editor_requests: u64,
}

impl Default for Layout {
    /// Every panel shown, at the sizes of the approved play-loop mockup
    /// (`docs/design/mockups/01-play-loop.png`).
    fn default() -> Self {
        Self {
            left: PanelState {
                visible: true,
                size: 240.0,
            },
            right: PanelState {
                visible: true,
                size: 320.0,
            },
            bottom: PanelState {
                visible: true,
                size: 230.0,
            },
            focus: Focus::Editor,
            focus_requests: 0,
            split: Split::None,
            soft_wrap: SoftWrap::Off,
            outline: Outline::Hidden,
            editor_request: None,
            editor_requests: 0,
        }
    }
}

impl Layout {
    /// The state of `panel`.
    #[must_use]
    pub const fn panel(&self, panel: Panel) -> &PanelState {
        match panel {
            Panel::Left => &self.left,
            Panel::Right => &self.right,
            Panel::Bottom => &self.bottom,
        }
    }

    /// This layout as the file holds it: without the focus and the count of focus commands.
    #[must_use]
    pub const fn as_written(self) -> Self {
        Self {
            focus: Focus::Editor,
            focus_requests: 0,
            editor_request: None,
            editor_requests: 0,
            ..self
        }
    }

    /// The state of `panel`, to change.
    pub const fn panel_mut(&mut self, panel: Panel) -> &mut PanelState {
        match panel {
            Panel::Left => &mut self.left,
            Panel::Right => &mut self.right,
            Panel::Bottom => &mut self.bottom,
        }
    }

    /// The layout the file at `path` holds; the default when there is no file.
    ///
    /// # Errors
    ///
    /// [`LayoutError::Read`] when the file exists but cannot be read, and
    /// [`LayoutError::Parse`] when it is not a layout.
    pub fn read(path: &Path) -> Result<Self, LayoutError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => {
                return Err(LayoutError::Read {
                    path: path.to_path_buf(),
                    error,
                });
            }
        };
        toml::from_str(&text).map_err(|error| LayoutError::Parse {
            path: path.to_path_buf(),
            error,
        })
    }

    /// Writes the layout to the file at `path`, making its directory.
    ///
    /// # Errors
    ///
    /// [`LayoutError::Write`] when the directory or the file cannot be written.
    pub fn write(&self, path: &Path) -> Result<(), LayoutError> {
        let write = |error| LayoutError::Write {
            path: path.to_path_buf(),
            error,
        };
        let text = toml::to_string(self).map_err(|error| LayoutError::Encode { error })?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(write)?;
        }
        fs::write(path, text).map_err(write)
    }
}

/// Why a layout could not be read or written.
#[derive(Debug)]
pub enum LayoutError {
    /// The layout file exists but cannot be read.
    Read {
        /// The file.
        path: PathBuf,
        /// Why.
        error: io::Error,
    },
    /// The layout file is not a layout.
    Parse {
        /// The file.
        path: PathBuf,
        /// What TOML's parser says.
        error: toml::de::Error,
    },
    /// The layout could not be written as TOML.
    Encode {
        /// What TOML's writer says.
        error: toml::ser::Error,
    },
    /// The layout file, or its directory, cannot be written.
    Write {
        /// The file.
        path: PathBuf,
        /// Why.
        error: io::Error,
    },
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, error } => write!(f, "cannot read {}: {error}", path.display()),
            Self::Parse { path, error } => {
                write!(f, "{} is not a layout file: {error}", path.display())
            }
            Self::Encode { error } => write!(f, "cannot write the layout as TOML: {error}"),
            Self::Write { path, error } => write!(f, "cannot write {}: {error}", path.display()),
        }
    }
}

impl Error for LayoutError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { error, .. } | Self::Write { error, .. } => Some(error),
            Self::Parse { error, .. } => Some(error),
            Self::Encode { error } => Some(error),
        }
    }
}

/// A context the layout commands work on: one that has a [`Layout`].
pub trait HasLayout {
    /// The layout.
    fn layout_mut(&mut self) -> &mut Layout;
}

impl HasLayout for Layout {
    fn layout_mut(&mut self) -> &mut Layout {
        self
    }
}

/// The id of the command that shows or hides a panel.
pub const TOGGLE_PANEL: &str = "view.toggle_panel";

/// The id of the command that sets a panel's size.
pub const RESIZE_PANEL: &str = "view.resize_panel";

/// The id of the command that gives the editor the keyboard.
pub const FOCUS_EDITOR: &str = "view.focus_editor";

/// The id of the command that gives the left panel the keyboard.
pub const FOCUS_LEFT_PANEL: &str = "view.focus_left_panel";

/// The id of the command that gives the right panel the keyboard.
pub const FOCUS_RIGHT_PANEL: &str = "view.focus_right_panel";

/// The id of the command that gives the bottom panel the keyboard.
pub const FOCUS_BOTTOM_PANEL: &str = "view.focus_bottom_panel";

/// The smallest a panel may be made, in points, so a panel is never dragged out of reach.
pub const MIN_PANEL_SIZE: f32 = 120.0;

/// Registers the layout commands.
///
/// # Errors
///
/// [`RegisterError`] when one of their ids is taken.
pub fn register<C: HasLayout + 'static>(registry: &mut Registry<C>) -> Result<(), RegisterError> {
    registry.register(TogglePanel)?;
    registry.register(ResizePanel)?;
    registry.register(FocusArea::<Editor>::new())?;
    registry.register(FocusArea::<LeftPanel>::new())?;
    registry.register(FocusArea::<RightPanel>::new())?;
    registry.register(FocusArea::<BottomPanel>::new())?;
    editor::register(registry)
}

/// What a focus command returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Focused {
    /// The area that has the keyboard now.
    pub focus: Focus,
}

/// An area a focus command gives the keyboard: the command's id and summary, and the area.
trait FocusTarget: Send + Sync + 'static {
    const ID: &'static str;
    const SUMMARY: &'static str;
    const FOCUS: Focus;
}

/// `view.focus_editor`.
struct Editor;

impl FocusTarget for Editor {
    const ID: &'static str = FOCUS_EDITOR;
    const SUMMARY: &'static str = "Gives the keyboard to the editor in the centre.";
    const FOCUS: Focus = Focus::Editor;
}

/// `view.focus_left_panel`.
struct LeftPanel;

impl FocusTarget for LeftPanel {
    const ID: &'static str = FOCUS_LEFT_PANEL;
    const SUMMARY: &'static str =
        "Shows the left panel, Project and Entities, if it is hidden and gives it the keyboard.";
    const FOCUS: Focus = Focus::Panel(Panel::Left);
}

/// `view.focus_right_panel`.
struct RightPanel;

impl FocusTarget for RightPanel {
    const ID: &'static str = FOCUS_RIGHT_PANEL;
    const SUMMARY: &'static str = "Shows the right panel, Properties and the agent, if it is \
        hidden and gives it the keyboard.";
    const FOCUS: Focus = Focus::Panel(Panel::Right);
}

/// `view.focus_bottom_panel`.
struct BottomPanel;

impl FocusTarget for BottomPanel {
    const ID: &'static str = FOCUS_BOTTOM_PANEL;
    const SUMMARY: &'static str = "Shows the bottom panel, Problems, Console and the debugger, \
        if it is hidden and gives it the keyboard.";
    const FOCUS: Focus = Focus::Panel(Panel::Bottom);
}

/// A focus command: gives one area the keyboard, showing it first when it is a hidden panel,
/// outside the history like the other layout commands.
struct FocusArea<T: FocusTarget>(PhantomData<T>);

impl<T: FocusTarget> FocusArea<T> {
    const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<C: HasLayout + 'static, T: FocusTarget> Command<C> for FocusArea<T> {
    const ID: &'static str = T::ID;
    const SUMMARY: &'static str = T::SUMMARY;
    type Params = NoParams;
    type Output = Focused;
    type Error = Infallible;

    fn run(
        &self,
        context: &mut C,
        NoParams {}: NoParams,
    ) -> Result<Outcome<Focused, C>, Infallible> {
        let layout = context.layout_mut();
        if let Focus::Panel(panel) = T::FOCUS {
            layout.panel_mut(panel).visible = true;
        }
        layout.focus = T::FOCUS;
        layout.focus_requests = layout.focus_requests.wrapping_add(1);
        Ok(Outcome::outside_history(Focused { focus: T::FOCUS }))
    }
}

/// The parameters of `view.toggle_panel`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TogglePanelParams {
    /// The panel: `left` (Project and Entities), `right` (Properties and the agent) or
    /// `bottom` (Problems, Console and the debugger).
    pub panel: Panel,
}

/// What `view.toggle_panel` returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct PanelShown {
    /// The panel.
    pub panel: Panel,
    /// Whether it is shown now.
    pub visible: bool,
}

/// `view.toggle_panel`: shows a hidden panel or hides a shown one.
struct TogglePanel;

impl<C: HasLayout + 'static> Command<C> for TogglePanel {
    const ID: &'static str = TOGGLE_PANEL;
    const SUMMARY: &'static str = "Shows or hides a panel: Project and Entities on the left, \
        Properties and the agent on the right, or Problems, Console and the debugger at the \
        bottom.";
    type Params = TogglePanelParams;
    type Output = PanelShown;
    type Error = Infallible;

    fn run(
        &self,
        context: &mut C,
        params: TogglePanelParams,
    ) -> Result<Outcome<PanelShown, C>, Infallible> {
        let state = context.layout_mut().panel_mut(params.panel);
        state.visible = !state.visible;
        Ok(Outcome::outside_history(PanelShown {
            panel: params.panel,
            visible: state.visible,
        }))
    }
}

/// The parameters of `view.resize_panel`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResizePanelParams {
    /// The panel: `left`, `right` or `bottom`.
    pub panel: Panel,
    /// Its new width, or height for the bottom panel, in points: at least 120.
    pub size: f32,
}

/// Why `view.resize_panel` refused a size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BadPanelSize {
    /// The size asked for.
    pub size: f32,
}

impl fmt::Display for BadPanelSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a panel's size must be a number of at least {MIN_PANEL_SIZE} points, not {}",
            self.size
        )
    }
}

impl Error for BadPanelSize {}

/// `view.resize_panel`: sets a panel's width, or the bottom panel's height.
struct ResizePanel;

impl<C: HasLayout + 'static> Command<C> for ResizePanel {
    const ID: &'static str = RESIZE_PANEL;
    const SUMMARY: &'static str = "Sets the width of the left or right panel, or the height \
        of the bottom panel, in points.";
    type Params = ResizePanelParams;
    type Output = ();
    type Error = BadPanelSize;

    fn run(
        &self,
        context: &mut C,
        params: ResizePanelParams,
    ) -> Result<Outcome<(), C>, BadPanelSize> {
        // `>=` is false for NaN, so a NaN is refused too.
        if !(params.size >= MIN_PANEL_SIZE && params.size.is_finite()) {
            return Err(BadPanelSize { size: params.size });
        }
        context.layout_mut().panel_mut(params.panel).size = params.size;
        Ok(Outcome::outside_history(()))
    }
}
