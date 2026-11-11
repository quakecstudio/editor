// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's layout and navigation commands (P4-32, UI-16): split panes, soft wrap and the
//! outline panel, which the layout keeps, and folding, go to line, go to symbol and the tab
//! switcher, which a window carries out once each is asked for; and the project search over
//! the multibuffer (P4-33, UI-17), `project_search::ToggleFocus` in Zed's keymap.
//!
//! The model is Zed's (`docs/configuring-zed.md` and the default keymap at
//! zed-industries/zed@415d310): `pane::SplitRight` and `pane::SplitDown`, `editor::Fold`,
//! `editor::UnfoldLines`, `editor::FoldAll`, `editor::UnfoldAll`, `editor::ToggleSoftWrap`,
//! `go_to_line::Toggle`, `outline::Toggle`, `outline_panel::ToggleFocus` and
//! `tab_switcher::Toggle`, each a command the palette lists and a key runs.

use std::convert::Infallible;

use qs_commands::{Command, NoParams, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::HasLayout;

/// The id of the command that folds the innermost region at the caret.
pub const FOLD: &str = "editor.fold";
/// The id of the command that unfolds the innermost folded region at the caret.
pub const UNFOLD: &str = "editor.unfold";
/// The id of the command that folds every region.
pub const FOLD_ALL: &str = "editor.fold_all";
/// The id of the command that unfolds every region.
pub const UNFOLD_ALL: &str = "editor.unfold_all";
/// The id of the command that turns soft wrap on or off.
pub const TOGGLE_SOFT_WRAP: &str = "editor.toggle_soft_wrap";
/// The id of the command that moves the caret to a line.
pub const GO_TO_LINE: &str = "editor.go_to_line";
/// The id of the command that opens the picker of the file's symbols.
pub const GO_TO_SYMBOL: &str = "editor.go_to_symbol";
/// The id of the command that shows or hides the outline panel.
pub const TOGGLE_OUTLINE: &str = "view.toggle_outline";
/// The id of the command that opens the tab switcher.
pub const TAB_SWITCHER: &str = "view.tab_switcher";
/// The id of the command that opens the project search over the multibuffer (P4-33).
pub const PROJECT_SEARCH_VIEW: &str = "view.project_search";
/// The id of the command that splits the editor into two panes side by side.
pub const SPLIT_RIGHT: &str = "view.split_right";
/// The id of the command that splits the editor into two panes, one above the other.
pub const SPLIT_DOWN: &str = "view.split_down";
/// The id of the command that closes the second pane.
pub const CLOSE_SPLIT: &str = "view.close_split";

/// How the editor area is split.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Split {
    /// One pane.
    #[default]
    None,
    /// Two panes side by side.
    Right,
    /// Two panes, one above the other.
    Down,
}

/// Whether long lines wrap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SoftWrap {
    /// Each line is one row.
    #[default]
    Off,
    /// Long lines wrap at the editor's width.
    Width,
}

/// Whether the outline panel is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Outline {
    /// Hidden.
    #[default]
    Hidden,
    /// Shown at the editor's right.
    Shown,
}

/// A one-off action on the editor, carried out by the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EditorRequest {
    /// Fold the innermost region at the caret.
    Fold,
    /// Unfold the innermost folded region at the caret.
    Unfold,
    /// Fold every region.
    FoldAll,
    /// Unfold every region.
    UnfoldAll,
    /// Move the caret to this line, from 1.
    GoToLine(u32),
    /// Ask for the line to go to.
    AskLine,
    /// Open the picker of the file's symbols.
    GoToSymbol,
    /// Open the tab switcher.
    TabSwitcher,
    /// Open the project search over the multibuffer (P4-33).
    ProjectSearch,
}

/// Asks the window for `request`.
fn ask<C: HasLayout>(context: &mut C, request: EditorRequest) {
    let layout = context.layout_mut();
    layout.editor_request = Some(request);
    layout.editor_requests = layout.editor_requests.wrapping_add(1);
}

/// A command that asks the window for one editor action and takes no parameters.
trait Ask: Send + Sync + 'static {
    const ID: &'static str;
    const SUMMARY: &'static str;
    const REQUEST: EditorRequest;
}

/// The command of an [`Ask`].
struct Asking<A>(std::marker::PhantomData<A>);

impl<A: Ask, C: HasLayout + 'static> Command<C> for Asking<A> {
    const ID: &'static str = A::ID;
    const SUMMARY: &'static str = A::SUMMARY;
    type Params = NoParams;
    type Output = ();
    type Error = Infallible;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<(), C>, Infallible> {
        ask(context, A::REQUEST);
        Ok(Outcome::outside_history(()))
    }
}

/// Declares an [`Ask`] command.
macro_rules! asking {
    ($name:ident, $id:expr, $request:expr, $summary:expr) => {
        #[doc = concat!("`", stringify!($name), "`.")]
        struct $name;

        impl Ask for $name {
            const ID: &'static str = $id;
            const SUMMARY: &'static str = $summary;
            const REQUEST: EditorRequest = $request;
        }
    };
}

asking!(
    FoldCommand,
    FOLD,
    EditorRequest::Fold,
    "Folds the innermost block or block comment at the caret."
);
asking!(
    UnfoldCommand,
    UNFOLD,
    EditorRequest::Unfold,
    "Unfolds the innermost folded block or comment at the caret."
);
asking!(
    FoldAllCommand,
    FOLD_ALL,
    EditorRequest::FoldAll,
    "Folds every block and block comment of the open file, each down to its first and last \
     lines."
);
asking!(
    UnfoldAllCommand,
    UNFOLD_ALL,
    EditorRequest::UnfoldAll,
    "Unfolds every folded block and block comment of the open file, showing all of its \
     lines."
);
asking!(
    GoToSymbolCommand,
    GO_TO_SYMBOL,
    EditorRequest::GoToSymbol,
    "Opens a picker of the open file's symbols, to move the caret to one."
);
asking!(
    TabSwitcherCommand,
    TAB_SWITCHER,
    EditorRequest::TabSwitcher,
    "Opens the tab switcher: the open files, the one used most recently first."
);

asking!(
    ProjectSearchCommand,
    PROJECT_SEARCH_VIEW,
    EditorRequest::ProjectSearch,
    "Opens the project search: every match in the project's files as a multibuffer of \
     editable excerpts, to edit or replace in place."
);

/// The parameters of `editor.go_to_line`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoToLineParams {
    /// The line, from 1; without it the window asks for one.
    #[serde(default)]
    pub line: Option<u32>,
}

/// `editor.go_to_line`.
struct GoToLine;

impl<C: HasLayout + 'static> Command<C> for GoToLine {
    const ID: &'static str = GO_TO_LINE;
    const SUMMARY: &'static str = "Moves the caret to a line of the open file, from 1; \
        without a line it asks for one.";
    type Params = GoToLineParams;
    type Output = ();
    type Error = Infallible;

    fn run(&self, context: &mut C, params: GoToLineParams) -> Result<Outcome<(), C>, Infallible> {
        let request = params
            .line
            .map_or(EditorRequest::AskLine, EditorRequest::GoToLine);
        ask(context, request);
        Ok(Outcome::outside_history(()))
    }
}

/// `editor.toggle_soft_wrap`.
struct ToggleSoftWrap;

impl<C: HasLayout + 'static> Command<C> for ToggleSoftWrap {
    const ID: &'static str = TOGGLE_SOFT_WRAP;
    const SUMMARY: &'static str =
        "Turns soft wrap on or off: long lines wrap at the editor's width.";
    type Params = NoParams;
    type Output = SoftWrap;
    type Error = Infallible;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<SoftWrap, C>, Infallible> {
        let layout = context.layout_mut();
        layout.soft_wrap = match layout.soft_wrap {
            SoftWrap::Off => SoftWrap::Width,
            SoftWrap::Width => SoftWrap::Off,
        };
        Ok(Outcome::outside_history(layout.soft_wrap))
    }
}

/// `view.toggle_outline`.
struct ToggleOutline;

impl<C: HasLayout + 'static> Command<C> for ToggleOutline {
    const ID: &'static str = TOGGLE_OUTLINE;
    const SUMMARY: &'static str = "Shows or hides the outline panel beside the editor: the \
        open file's symbols, the one the caret is in marked.";
    type Params = NoParams;
    type Output = Outline;
    type Error = Infallible;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<Outline, C>, Infallible> {
        let layout = context.layout_mut();
        layout.outline = match layout.outline {
            Outline::Hidden => Outline::Shown,
            Outline::Shown => Outline::Hidden,
        };
        Ok(Outcome::outside_history(layout.outline))
    }
}

/// A command that sets how the editor area is split.
trait SplitTo: Send + Sync + 'static {
    const ID: &'static str;
    const SUMMARY: &'static str;
    const SPLIT: Split;
}

/// The command of a [`SplitTo`].
struct Splitting<S>(std::marker::PhantomData<S>);

impl<S: SplitTo, C: HasLayout + 'static> Command<C> for Splitting<S> {
    const ID: &'static str = S::ID;
    const SUMMARY: &'static str = S::SUMMARY;
    type Params = NoParams;
    type Output = Split;
    type Error = Infallible;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<Split, C>, Infallible> {
        context.layout_mut().split = S::SPLIT;
        Ok(Outcome::outside_history(S::SPLIT))
    }
}

/// `view.split_right`.
struct Right;

impl SplitTo for Right {
    const ID: &'static str = SPLIT_RIGHT;
    const SUMMARY: &'static str =
        "Splits the editor into two panes side by side, the second showing the open file.";
    const SPLIT: Split = Split::Right;
}

/// `view.split_down`.
struct Down;

impl SplitTo for Down {
    const ID: &'static str = SPLIT_DOWN;
    const SUMMARY: &'static str =
        "Splits the editor into two panes, one above the other, the second showing the open file.";
    const SPLIT: Split = Split::Down;
}

/// `view.close_split`.
struct Close;

impl SplitTo for Close {
    const ID: &'static str = CLOSE_SPLIT;
    const SUMMARY: &'static str = "Closes the editor's second pane.";
    const SPLIT: Split = Split::None;
}

/// Registers the editor's layout and navigation commands.
pub(crate) fn register<C: HasLayout + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(Asking::<FoldCommand>(std::marker::PhantomData))?;
    registry.register(Asking::<UnfoldCommand>(std::marker::PhantomData))?;
    registry.register(Asking::<FoldAllCommand>(std::marker::PhantomData))?;
    registry.register(Asking::<UnfoldAllCommand>(std::marker::PhantomData))?;
    registry.register(Asking::<GoToSymbolCommand>(std::marker::PhantomData))?;
    registry.register(Asking::<TabSwitcherCommand>(std::marker::PhantomData))?;
    registry.register(Asking::<ProjectSearchCommand>(std::marker::PhantomData))?;
    registry.register(GoToLine)?;
    registry.register(ToggleSoftWrap)?;
    registry.register(ToggleOutline)?;
    registry.register(Splitting::<Right>(std::marker::PhantomData))?;
    registry.register(Splitting::<Down>(std::marker::PhantomData))?;
    registry.register(Splitting::<Close>(std::marker::PhantomData))
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use qs_commands::{Registry, Session};
    use serde_json::json;

    use super::{EditorRequest, Outline, SoftWrap, Split};
    use crate::{Layout, register};

    fn session() -> Session<Layout> {
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        Session::new(Layout::default(), registry)
    }

    // No spec statement: P4-32: each one-off editor command asks the window once, the count
    // telling two asks for the same action apart
    #[test]
    fn the_editor_commands_ask_the_window() {
        let mut s = session();
        for (id, request) in [
            (super::FOLD, EditorRequest::Fold),
            (super::UNFOLD, EditorRequest::Unfold),
            (super::FOLD_ALL, EditorRequest::FoldAll),
            (super::UNFOLD_ALL, EditorRequest::UnfoldAll),
            (super::GO_TO_SYMBOL, EditorRequest::GoToSymbol),
            (super::TAB_SWITCHER, EditorRequest::TabSwitcher),
            (super::PROJECT_SEARCH_VIEW, EditorRequest::ProjectSearch),
        ] {
            let before = s.context().editor_requests;
            s.execute(id, json!({})).unwrap();
            assert_eq!(s.context().editor_request, Some(request), "{id}");
            assert_eq!(s.context().editor_requests, before.wrapping_add(1));
        }
        s.execute(super::GO_TO_LINE, json!({ "line": 12 })).unwrap();
        assert_eq!(
            s.context().editor_request,
            Some(EditorRequest::GoToLine(12))
        );
        s.execute(super::GO_TO_LINE, json!({})).unwrap();
        assert_eq!(s.context().editor_request, Some(EditorRequest::AskLine));
        assert!(s.execute(super::GO_TO_LINE, json!({ "line": -1 })).is_err());
        assert_eq!(s.undo_depth(), 0, "none is an edit");
    }

    // No spec statement: P4-32: soft wrap, the outline and the split are layout state, written
    // to the layout file; the asks are not
    #[test]
    fn wrap_outline_and_split_are_kept_in_the_layout() {
        let mut s = session();
        assert_eq!(
            s.execute(super::TOGGLE_SOFT_WRAP, json!({})).unwrap(),
            json!("width")
        );
        assert_eq!(s.context().soft_wrap, SoftWrap::Width);
        s.execute(super::TOGGLE_OUTLINE, json!({})).unwrap();
        assert_eq!(s.context().outline, Outline::Shown);
        s.execute(super::SPLIT_DOWN, json!({})).unwrap();
        assert_eq!(s.context().split, Split::Down);
        s.execute(super::SPLIT_RIGHT, json!({})).unwrap();
        assert_eq!(s.context().split, Split::Right);
        s.execute(super::FOLD, json!({})).unwrap();
        let written = s.context().as_written();
        assert_eq!(written.editor_request, None);
        let text = toml::to_string(&written).unwrap();
        let back: Layout = toml::from_str(&text).unwrap();
        assert_eq!(back, written);
        assert!(text.contains("split = \"right\""), "{text}");
        s.execute(super::CLOSE_SPLIT, json!({})).unwrap();
        assert_eq!(s.context().split, Split::None);
        s.execute(super::TOGGLE_SOFT_WRAP, json!({})).unwrap();
        s.execute(super::TOGGLE_OUTLINE, json!({})).unwrap();
        assert_eq!(s.context().soft_wrap, SoftWrap::Off);
        assert_eq!(s.context().outline, Outline::Hidden);
    }
}
