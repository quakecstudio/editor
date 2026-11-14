// SPDX-License-Identifier: GPL-3.0-or-later

//! The editors' feature (P6-41): the editor and the second pane, the editors of the tabs not
//! shown, the go to symbol picker and the tab switcher, the multibuffer, and where the editor
//! was last taken, which the shell used to hold.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Id};
use qs_editor_view::Position;
use qs_workbench::debug::Location;
use qs_workbench::git::{GIT_HEAD_TEXT, GitPathParams};
use qs_workbench::tools::{PROJECT_SEARCH, ProjectSearchParams, SearchFound};
use qs_workbench::{
    FileText, OnLoss, OpenFile, PlayState, PreviewStatus, READ_FILE, RELOAD, ReloadParams,
    SaveFileParams, UPDATE_FILE, UpdateFileParams,
};
use qs_workbench::{INLAY_HINTS, InlayHint};

use crate::editor::Editor;
use crate::fallback::{SystemFonts, families};
use crate::features::{Commands, ShellRequest};
use crate::multibuffer::Source as MultiSource;
use crate::project::Jump;
use crate::strings::{Text, Truncation};
use crate::theme::Theme;

use crate::multibuffer::Multibuffer;
use crate::picker::Picker;
use crate::shell::{CentreTab, EDITOR_AREA, inlay_text, line_index};
use std::collections::BTreeMap;

/// The editors' state.
#[derive(Default)]
pub(crate) struct EditorsFeature {
    /// The editor of the open file.
    pub(crate) editor: Option<Editor>,
    /// The editor of the second pane while the editor is split, showing a file not being
    /// edited, or the same one.
    pub(crate) second: Option<Editor>,
    /// The editors of the tabs not shown, with their carets and undo histories.
    pub(crate) parked: BTreeMap<PathBuf, Editor>,
    /// The editor's file and text revision whose characters have fonts.
    pub(crate) covered: Option<(PathBuf, u64)>,
    /// The line, from 1, of the file to show once it is open in the editor.
    pub(crate) pending_line: Option<(PathBuf, u32)>,
    /// The count of editor actions the layout had asked for when last carried out.
    pub(crate) editor_seen: u64,
    /// Where the game last stopped that the editor was taken to.
    pub(crate) shown_stop: Option<Location>,
    /// The go to symbol picker and the tab switcher, and the place each row stands for (P4-32).
    pub(crate) picker: (Picker, Vec<Jump>),
    /// The multibuffer the Search tab shows (P4-33).
    pub(crate) multibuffer: Multibuffer,
    /// What the editors' glue asked of the window, carried out when the call returns.
    requests: Vec<ShellRequest>,
}

impl EditorsFeature {
    /// What the editors' glue asked of the window since the last call.
    pub(crate) fn take_requests(&mut self) -> Vec<ShellRequest> {
        std::mem::take(&mut self.requests)
    }

    /// Reads the editor's inlay hints again when its file's analysis changed (P4-34): each
    /// parameter's name before the argument passed as it.
    pub(crate) fn refresh_inlays(&mut self, commands: &mut Commands<'_>) {
        let Some(revision) = commands
            .session()
            .context()
            .workbench
            .open()
            .map(OpenFile::revision)
        else {
            return;
        };
        let Some(path) = self
            .editor
            .as_ref()
            .filter(|editor| editor.hints_at != Some(revision))
            .map(|editor| editor.path.clone())
        else {
            return;
        };
        let hints: Vec<InlayHint> = commands
            .run(INLAY_HINTS, &SaveFileParams { path })
            .unwrap_or_default();
        if let Some(editor) = self.editor.as_mut() {
            let buffer = editor.view.state().buffer();
            let placed: Vec<(Position, String)> = hints
                .into_iter()
                .map(|hint| (buffer.position(hint.offset), inlay_text(&hint.name)))
                .collect();
            editor.view.set_inlays(placed);
            editor.hints_at = Some(revision);
        }
    }

    /// Marks in the editor's gutter its file's changes against the text at `HEAD` (P4-35);
    /// none outside a repository or for a file `HEAD` does not have.
    pub(crate) fn mark_changes(&mut self, commands: &mut Commands<'_>) {
        let Some(path) = self.editor.as_ref().map(|editor| editor.path.clone()) else {
            return;
        };
        let base = commands
            .session_mut()
            .execute(
                GIT_HEAD_TEXT,
                serde_json::to_value(GitPathParams { path }).unwrap_or_default(),
            )
            .ok()
            .and_then(|value| serde_json::from_value::<Option<String>>(value).ok())
            .flatten();
        if let Some(editor) = self.editor.as_mut() {
            editor.view.set_gutter_base(base);
        }
    }

    /// Brings the editor up to date with the open file: a new view for another file, the caret
    /// moved for another `file.open` of the same one, none for no file.
    pub(crate) fn sync_editor(&mut self, commands: &mut Commands<'_>, theme: &Theme) {
        // Another file's editor sends its edits before it goes: the workbench keeps them, so
        // the file shows them when it is opened again.
        let open_path = commands
            .session()
            .context()
            .workbench
            .open()
            .map(OpenFile::path);
        if let Some(editor) = self.editor.as_mut()
            && open_path != Some(editor.path.as_path())
            && let Some(text) = editor.unsent()
        {
            let path = editor.path.clone();
            let _: Option<()> = commands.run(UPDATE_FILE, &UpdateFileParams { path, text });
        }
        let workbench = &commands.session().context().workbench;
        let tabs = workbench.tabs();
        self.parked.retain(|path, _| tabs.contains(path));
        let open = workbench.open();
        match (open, &mut self.editor) {
            (None, _) => self.editor = None,
            (Some(open), Some(editor)) if editor.path == open.path() => {
                editor.sync(open, theme);
            }
            (Some(open), _) => {
                if let Some(old) = self.editor.take()
                    && tabs.contains(&old.path)
                {
                    self.parked.insert(old.path.clone(), old);
                }
                let editor = match self.parked.remove(open.path()) {
                    Some(mut editor) => {
                        editor.sync(open, theme);
                        editor
                    }
                    None => Editor::new(Id::new(EDITOR_AREA), open, theme),
                };
                self.editor = Some(editor);
                self.mark_changes(commands);
            }
        }
        if let Some(editor) = self.editor.as_mut()
            && let Some((_, line)) = self.pending_line.take_if(|(path, _)| *path == editor.path)
        {
            editor.go_to_line(line_index(line));
        }
    }

    /// Gives egui a system font for each character of the editor's text that the bundled fonts
    /// lack: the whole text of a file just opened, then the caret's line as it is edited.
    pub(crate) fn cover_characters(
        &mut self,
        _commands: &mut Commands<'_>,
        fonts: &mut SystemFonts,
        ctx: &egui::Context,
    ) {
        let Some(editor) = &self.editor else {
            return;
        };
        let state = editor.view.state();
        let now = (editor.path.clone(), state.revision());
        if self.covered.as_ref() == Some(&now) {
            return;
        }
        let text = match &self.covered {
            Some((path, _)) if *path == now.0 => {
                state.buffer().line(state.primary().head.line).to_owned()
            }
            Some(_) | None => state.buffer().text(),
        };
        fonts.cover(ctx, &families(), &text);
        self.covered = Some(now);
    }

    /// Reloads the running game after a save, its error shown on the toolbar.
    pub(crate) fn reload_after_save(&mut self, commands: &mut Commands<'_>) {
        if commands.session().context().preview.state() == PlayState::Stopped {
            return;
        }
        // P3-10: a reload that would lose a value waits for the modder (`reset_notice`).
        let params = ReloadParams {
            on_loss: OnLoss::Ask,
        };
        let reloaded: Option<PreviewStatus> = commands.run(RELOAD, &params);
        if reloaded.is_none() {
            commands.set_notice(Some(Text::ReloadFailed.get().to_owned()));
        }
    }

    /// Shows `places`, each a file's byte range, as the multibuffer's excerpts from `source`,
    /// in the Search tab (P4-33). `truncation` says whether the places were cut short.
    pub(crate) fn show_places(
        &mut self,
        commands: &mut Commands<'_>,
        source: MultiSource,
        places: Vec<(PathBuf, usize, usize)>,
        truncation: Truncation,
    ) {
        let mut texts: Vec<(PathBuf, Vec<u8>)> = Vec::new();
        for (path, _, _) in &places {
            if texts.iter().any(|(p, _)| p == path) {
                continue;
            }
            let read: Option<FileText> =
                commands.run(READ_FILE, &SaveFileParams { path: path.clone() });
            if let Some(read) = read {
                texts.push((path.clone(), read.text.into_bytes()));
            }
        }
        self.multibuffer.show(source, &places, truncation, |path| {
            texts
                .iter()
                .find(|(p, _)| p == path)
                .map(|(_, text)| text.clone())
        });
        self.requests
            .push(ShellRequest::ShowCentre(CentreTab::Search));
    }

    /// Searches the project with the multibuffer's form and shows what it found.
    pub(crate) fn search_project(&mut self, commands: &mut Commands<'_>) {
        let form = self.multibuffer.form.clone();
        let params = ProjectSearchParams {
            text: form.text,
            case_sensitive: form.case_sensitive,
            whole_word: form.whole_word,
            regex: form.regex,
        };
        match commands.attempt::<_, SearchFound>(PROJECT_SEARCH, &params) {
            Ok(found) => {
                let places = found.matches.into_iter().map(|m| (m.path, m.start, m.end));
                let truncation = if found.truncated {
                    Truncation::Cut
                } else {
                    Truncation::All
                };
                self.show_places(commands, MultiSource::Search, places.collect(), truncation);
            }
            Err(error) => self.multibuffer.error = Some(error.to_string()),
        }
    }

    /// Shows every problem as the multibuffer's excerpts (P4-33).
    pub(crate) fn problems_as_excerpts(&mut self, commands: &mut Commands<'_>) {
        let places = commands
            .session()
            .context()
            .workbench
            .problems()
            .iter()
            .map(|p| {
                (
                    p.path.clone(),
                    p.diagnostic.span.start,
                    p.diagnostic.span.end,
                )
            })
            .collect();
        self.show_places(commands, MultiSource::Problems, places, Truncation::All);
    }

    /// Opens `file`, relative to the project's root, at `line`, from 1.
    pub(crate) fn show_line(&mut self, commands: &mut Commands<'_>, file: &Path, line: u32) {
        let Some(root) = commands.session().context().workbench.root() else {
            return;
        };
        let path = root.join(file);
        self.pending_line = Some((path.clone(), line));
        self.requests
            .push(ShellRequest::Jump(Jump { path, offset: 0 }));
    }

    /// Takes the editor to where the game stopped, once for each stop.
    pub(crate) fn follow_stop(&mut self, commands: &mut Commands<'_>) {
        let stop = commands
            .session()
            .context()
            .preview
            .debugger()
            .stopped()
            .map(|(_, at)| at.clone());
        if stop == self.shown_stop {
            return;
        }
        if let Some(Location {
            file: Some(file),
            line: Some(line),
            ..
        }) = &stop
        {
            self.show_line(commands, file, *line);
        }
        self.shown_stop = stop;
    }
}
