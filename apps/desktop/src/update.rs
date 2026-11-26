// SPDX-License-Identifier: GPL-3.0-or-later

//! The update notice (P4-19, QA-11, PRIN-9): the status bar says when a later release exists,
//! and a window says how to get it with what installed this one. The app downloads and
//! installs nothing: a package manager's command is shown to copy, and a download is the
//! release's page, opened in the browser.
//!
//! A check is `qs-update`'s one request for the release feed. It is made when Help, Check for
//! Updates is chosen (`help.check_for_updates`), and as the app starts only once
//! `updates.check` is `daily`, at most once a day; with the setting left unset nothing is
//! asked of the network (`docs/privacy.md`). It runs off the frame, so the window is never
//! held by it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::SystemTime;

use eframe::egui::{self, RichText};
use qs_settings::{Scope, Setting, SettingsFile, SettingsStore, UpdateCheck};
use qs_update::{Channel, CheckError, Download, Feed, Fetch, Found, Install, State, Version};

use crate::strings::{Text, up_to_date, update_available, update_failed, update_item};
use crate::theme::{Theme, Token};

/// Who asked for a check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// A person, with Help, Check for Updates: the window says what was found, whatever it is.
    Person,
    /// The app, as it started with `updates.check = "daily"`: only a later release is said,
    /// in the status bar, and a failure is said nowhere but the log.
    Background,
}

/// What the update window shows.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Shown {
    /// Nothing: the window is closed.
    Hidden,
    /// A check a person asked for has not answered yet.
    Checking,
    /// A later release, and how to get it.
    Available(Feed),
    /// The one running is the latest.
    UpToDate,
    /// The check a person asked for failed, and why.
    Failed(String),
}

/// What a click in the update window asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Click {
    /// Copy the channel's command.
    Copy(&'static str),
    /// Open the release's page: its notes, and its downloads.
    Page,
    /// Do not announce this release again.
    Skip,
    /// Close the window.
    Close,
}

/// The update check's state in the window.
pub(crate) struct UpdateNotice {
    fetch: Arc<dyn Fetch>,
    /// The version running; none when the build's is not a release's, which no check can
    /// compare.
    current: Result<Version, String>,
    channel: Channel,
    clock: Box<dyn Fn() -> SystemTime>,
    /// Where the background check keeps when it last asked (`qs_update::state_path`).
    state_file: Option<PathBuf>,
    /// The user's settings file, which keeps the release skipped.
    settings_file: Option<PathBuf>,
    /// `updates.check`, from the user's settings.
    mode: UpdateCheck,
    /// `updates.skip_version`: the release the status bar does not announce.
    skipped: Option<Version>,
    /// The later release a check found.
    newer: Option<Feed>,
    /// The check that has not answered yet, and who asked for it.
    pending: Option<(Asked, Receiver<Result<Found, CheckError>>)>,
    shown: Shown,
}

impl UpdateNotice {
    /// The notice of the app as it runs: the feed over HTTPS, this build's version, this
    /// machine's install and the system's clock.
    pub(crate) fn new() -> Self {
        Self::with(
            default_fetch(),
            env!("CARGO_PKG_VERSION"),
            Install::current().map_or(Channel::Unpackaged, |install| install.channel()),
            Box::new(SystemTime::now),
        )
    }

    /// A notice reading the feed through `fetch`, for an install of `channel` running
    /// `version`, telling the time by `clock`.
    pub(crate) fn with(
        fetch: Arc<dyn Fetch>,
        version: &str,
        channel: Channel,
        clock: Box<dyn Fn() -> SystemTime>,
    ) -> Self {
        Self {
            fetch,
            current: version
                .parse()
                .map_err(|error: qs_update::NotAVersion| error.to_string()),
            channel,
            clock,
            state_file: None,
            settings_file: None,
            mode: UpdateCheck::default(),
            skipped: None,
            newer: None,
            pending: None,
            shown: Shown::Hidden,
        }
    }

    /// Takes `updates.check` and `updates.skip_version` from the user's `settings`, read from
    /// `file`, where Skip This Version is kept.
    pub(crate) fn read_settings(&mut self, settings: Option<&SettingsFile>, file: &Path) {
        let updates = settings.map(|settings| &settings.updates);
        self.mode = updates
            .and_then(|updates| updates.check)
            .unwrap_or_default();
        self.skipped = updates
            .and_then(|updates| updates.skip_version.as_deref())
            .and_then(|version| version.parse().ok());
        self.settings_file = Some(file.to_path_buf());
    }

    /// Starts the background check, keeping its state in `state_file`: with `updates.check`
    /// `daily`, the release the last check found is announced again, and the feed is asked
    /// when the last answer is a day old. With the setting `off`, or unset, this does nothing
    /// and reads nothing.
    pub(crate) fn start(&mut self, state_file: PathBuf, ctx: &egui::Context) {
        if self.mode == UpdateCheck::Off {
            return;
        }
        let state = State::load(&state_file);
        self.state_file = Some(state_file);
        if let Ok(current) = self.current {
            self.newer = state.newer_than(current).cloned();
        }
        if state.due((self.clock)()) {
            self.check(Asked::Background, ctx);
        }
    }

    /// Asks the feed for the latest release on a thread of its own, as the agent's inline edit
    /// asks its model, so the frame never waits; [`UpdateNotice::poll`] takes the answer. A person's check opens the window at once. While a check waits no second one
    /// is made: a person who asks during the background's is answered by it.
    pub(crate) fn check(&mut self, asked: Asked, ctx: &egui::Context) {
        if asked == Asked::Person {
            self.shown = Shown::Checking;
        }
        let current = match &self.current {
            Ok(current) => *current,
            Err(why) => {
                if asked == Asked::Person {
                    self.shown = Shown::Failed(why.clone());
                }
                return;
            }
        };
        if let Some((waiting, _)) = &mut self.pending {
            if asked == Asked::Person {
                *waiting = Asked::Person;
            }
            return;
        }
        let (answer, answered) = channel();
        let fetch = Arc::clone(&self.fetch);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            // The window may have closed since: then nobody takes the answer.
            let _unsent = answer.send(Found::check(fetch.as_ref(), current));
            ctx.request_repaint();
        });
        self.pending = Some((asked, answered));
    }

    /// Takes the answer of a check, once it has one.
    pub(crate) fn poll(&mut self) {
        let Some((_, answered)) = &self.pending else {
            return;
        };
        match answered.try_recv() {
            Ok(result) => self.answered(result.map_err(|error| error.to_string())),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.answered(Err(Text::UpdateNoAnswer.get().to_owned()));
            }
        }
    }

    /// Waits for the check that is out, so a test sees its answer without waiting on a clock.
    #[cfg(test)]
    pub(crate) fn wait(&mut self) {
        if let Some((_, answered)) = &self.pending {
            let result = answered
                .recv()
                .map_err(|error| error.to_string())
                .and_then(|result| result.map_err(|error| error.to_string()));
            self.answered(result);
        }
    }

    /// Does what a check's answer calls for: a later release is announced and kept for the
    /// next start, and a person who asked is told whatever was found.
    fn answered(&mut self, result: Result<Found, String>) {
        let Some((asked, _)) = self.pending.take() else {
            return;
        };
        match result {
            Ok(found) => {
                self.record(&found);
                let (newer, shown) = match found {
                    Found::Newer(feed) => (Some(feed.clone()), Shown::Available(feed)),
                    Found::UpToDate => (None, Shown::UpToDate),
                };
                self.newer = newer;
                if asked == Asked::Person {
                    self.shown = shown;
                }
            }
            Err(why) => match asked {
                Asked::Person => self.shown = Shown::Failed(why),
                Asked::Background => tracing::info!(%why, "the update check had no answer"),
            },
        }
    }

    /// Keeps what a check found, and when, for the background check's next start.
    fn record(&self, found: &Found) {
        let Some(file) = &self.state_file else {
            return;
        };
        let mut state = State::load(file);
        state.record((self.clock)(), found);
        if let Err(error) = state.save(file) {
            tracing::warn!(%error, "the update check's state was not kept");
        }
    }

    /// The later release the status bar announces: the one found, unless it was skipped.
    pub(crate) fn announced(&self) -> Option<Version> {
        self.newer
            .as_ref()
            .map(Feed::version)
            .filter(|version| Some(*version) != self.skipped)
    }

    /// The status bar's item while a later release is announced; whether it was clicked.
    pub(crate) fn status_item(&self, ui: &mut egui::Ui, theme: &Theme) -> bool {
        let Some(version) = self.announced() else {
            return false;
        };
        let text = RichText::new(update_item(&version.to_string()))
            .small()
            .color(theme.color(Token::Accent));
        ui.add(egui::Button::new(text).frame(false))
            .on_hover_text(Text::UpdateItemHint.get())
            .clicked()
    }

    /// Opens the window on the release found, as a click on the status bar's item does.
    pub(crate) fn open(&mut self) {
        if let Some(feed) = &self.newer {
            self.shown = Shown::Available(feed.clone());
        }
    }

    /// Does not announce `version` again, here or after a restart: it is kept as
    /// `updates.skip_version` in the user's settings. A later release is announced.
    fn skip(&mut self, version: Version) {
        self.skipped = Some(version);
        let Some(file) = &self.settings_file else {
            return;
        };
        let value = Some(serde_json::Value::from(version.to_string()));
        let kept = SettingsStore::open(file.clone(), None)
            .and_then(|mut store| store.put(Scope::User, Setting::SkipVersion, value));
        if let Err(error) = kept {
            tracing::warn!(%error, "the skipped release was not kept");
        }
    }

    /// Draws the window over the app while it shows, and does what a click in it asked.
    pub(crate) fn show(&mut self, ctx: &egui::Context, theme: &Theme) {
        if self.shown == Shown::Hidden {
            return;
        }
        let mut click = ctx
            .input(|input| input.key_pressed(egui::Key::Escape))
            .then_some(Click::Close);
        egui::Window::new(Text::UpdateTitle.get())
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                let clicked = match &self.shown {
                    Shown::Available(feed) => self.available(ui, theme, feed),
                    Shown::Checking => said(ui, Text::UpdateChecking.get()),
                    Shown::UpToDate => said(ui, &up_to_date(&self.running())),
                    Shown::Failed(why) => said(ui, &update_failed(why)),
                    Shown::Hidden => None,
                };
                click = click.or(clicked);
            });
        let version = match &self.shown {
            Shown::Available(feed) => Some((feed.version(), feed.page().to_owned())),
            Shown::Hidden | Shown::Checking | Shown::UpToDate | Shown::Failed(_) => None,
        };
        match (click, version) {
            (Some(Click::Copy(command)), _) => ctx.copy_text(command.to_owned()),
            (Some(Click::Page), Some((_, page))) => ctx.open_url(egui::OpenUrl::new_tab(page)),
            (Some(Click::Skip), Some((version, _))) => {
                self.skip(version);
                self.shown = Shown::Hidden;
            }
            (Some(Click::Close), _) => self.shown = Shown::Hidden,
            (Some(Click::Page | Click::Skip), None) | (None, _) => {}
        }
    }

    /// The version running, as the window names it.
    fn running(&self) -> String {
        match &self.current {
            Ok(version) => version.to_string(),
            Err(_) => env!("CARGO_PKG_VERSION").to_owned(),
        }
    }

    /// The window's content for a later release: both versions, how this install updates,
    /// and its buttons; what was clicked.
    fn available(&self, ui: &mut egui::Ui, theme: &Theme, feed: &Feed) -> Option<Click> {
        let mut click = None;
        ui.label(update_available(
            &feed.version().to_string(),
            &self.running(),
        ));
        ui.label(RichText::new(how(self.channel).get()).color(theme.color(Token::TextMuted)));
        if let Some(command) = self.channel.command() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(command).monospace());
                if ui.button(Text::UpdateCopy.get()).clicked() {
                    click = Some(Click::Copy(command));
                }
            });
        }
        ui.horizontal(|ui| {
            if self.channel.download() == Download::Offered
                && ui.button(Text::UpdateDownload.get()).clicked()
            {
                click = Some(Click::Page);
            }
            if ui.button(Text::UpdateReleaseNotes.get()).clicked() {
                click = Some(Click::Page);
            }
            if ui.button(Text::UpdateSkip.get()).clicked() {
                click = Some(Click::Skip);
            }
            if ui.button(Text::UpdateLater.get()).clicked() {
                click = Some(Click::Close);
            }
        });
        click
    }
}

/// One line and the button that closes the window; whether it was clicked.
fn said(ui: &mut egui::Ui, line: &str) -> Option<Click> {
    ui.label(line);
    ui.button(Text::AboutClose.get())
        .clicked()
        .then_some(Click::Close)
}

/// What the window says of an install of `channel`, above its command or its Download.
const fn how(channel: Channel) -> Text {
    match channel {
        Channel::Homebrew => Text::UpdateViaHomebrew,
        Channel::MacPackage => Text::UpdateViaMacPackage,
        Channel::Msi => Text::UpdateViaMsi,
        Channel::PortableZip => Text::UpdateViaZip,
        Channel::Deb => Text::UpdateViaDeb,
        Channel::Rpm(_) => Text::UpdateViaRpm,
        Channel::AppImage => Text::UpdateViaAppImage,
        Channel::Flatpak => Text::UpdateViaFlatpak,
        Channel::Unpackaged => Text::UpdateViaSource,
    }
}

/// The feed the app reads: over HTTPS.
#[cfg(not(test))]
fn default_fetch() -> Arc<dyn Fetch> {
    Arc::new(qs_update::Https::new())
}

/// The feed a test's window reads unless the test gives one: none, so no test asks the
/// network for one.
#[cfg(test)]
fn default_fetch() -> Arc<dyn Fetch> {
    /// A feed that is not there.
    struct NoFeed;

    impl Fetch for NoFeed {
        fn latest(&self) -> Result<String, CheckError> {
            Ok(String::new())
        }
    }

    Arc::new(NoFeed)
}

#[cfg(test)]
mod tests;
