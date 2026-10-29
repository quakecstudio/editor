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
use std::path::{Path, PathBuf};

use qs_commands::{Command, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

/// The layout file's name in the app's data directory (`qs_paths::AppDirs::data`).
pub const LAYOUT_FILE: &str = "layout.toml";

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

/// The smallest a panel may be made, in points, so a panel is never dragged out of reach.
pub const MIN_PANEL_SIZE: f32 = 120.0;

/// Registers the layout commands.
///
/// # Errors
///
/// [`RegisterError`] when one of their ids is taken.
pub fn register<C: HasLayout + 'static>(registry: &mut Registry<C>) -> Result<(), RegisterError> {
    registry.register(TogglePanel)?;
    registry.register(ResizePanel)
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
pub struct PanelShown {
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
pub struct BadPanelSize {
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
