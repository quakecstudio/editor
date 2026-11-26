// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap is a failed test, and the times are small"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use qs_update::{CHECK_INTERVAL, Channel, CheckError, Fetch, RpmTool, State, state_path};

use super::UpdateNotice;
use crate::App;
use crate::strings::{Text, up_to_date, update_available, update_failed, update_item};

/// The feed the release workflow fills in and publishes.
const TEMPLATE: &str = include_str!("../../../../packaging/latest.json");
/// The version the tests' app runs.
const RUNNING: &str = "2026.11.3";
/// A release later than it.
const LATER: &str = "2026.12.1";
/// When the tests begin, in seconds since the Unix epoch.
const START: u64 = 1_800_000_000;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "quakec-studio-update-{}-{name}-{n}",
            std::process::id()
        ));
        let _ignored = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

/// What the feed answers.
#[derive(Clone, Copy)]
enum Answer {
    /// The release of this version.
    Release(&'static str),
    /// Text that is not a feed.
    Malformed,
}

/// A feed that counts how often it is asked.
struct Feed {
    answer: Answer,
    asked: AtomicUsize,
}

impl Feed {
    fn new(answer: Answer) -> Arc<Self> {
        Arc::new(Self {
            answer,
            asked: AtomicUsize::new(0),
        })
    }

    fn asked(&self) -> usize {
        self.asked.load(Ordering::Relaxed)
    }
}

impl Fetch for Feed {
    fn latest(&self) -> Result<String, CheckError> {
        self.asked.fetch_add(1, Ordering::Relaxed);
        Ok(match self.answer {
            Answer::Release(version) => TEMPLATE.replace("@VERSION@", version),
            Answer::Malformed => "<html>not found</html>".to_owned(),
        })
    }
}

/// A clock a test sets, in seconds since the Unix epoch.
#[derive(Clone)]
struct Clock(Arc<AtomicU64>);

impl Clock {
    fn at(seconds: u64) -> Self {
        Self(Arc::new(AtomicU64::new(seconds)))
    }

    fn set(&self, seconds: u64) {
        self.0.store(seconds, Ordering::Relaxed);
    }

    fn reader(&self) -> Box<dyn Fn() -> SystemTime> {
        let seconds = Arc::clone(&self.0);
        Box::new(move || UNIX_EPOCH + Duration::from_secs(seconds.load(Ordering::Relaxed)))
    }
}

/// What a test's window is: the ordinary case is a Homebrew install of [`RUNNING`] whose feed
/// names [`LATER`], with no settings file and so no background check.
struct Spec {
    answer: Answer,
    channel: Channel,
    /// The user's settings file's text, when there is one.
    settings: Option<&'static str>,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            answer: Answer::Release(LATER),
            channel: Channel::Homebrew,
            settings: None,
        }
    }
}

type Window = Harness<'static, Option<App>>;

/// A started window of `spec` whose files are in `dir`, its feed and its clock. Starting it
/// is what the app's `main` does: the settings are read, then the update check is started.
fn window(spec: &Spec, dir: &Path, feed: &Arc<Feed>, clock: &Clock) -> Window {
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .build_ui_state(
            |ui, app: &mut Option<App>| {
                if let Some(app) = app {
                    app.frame(ui);
                }
            },
            None,
        );
    let mut app = App::checked_on_the_frame(&harness.ctx, None).unwrap();
    let fetch: Arc<dyn Fetch> = Arc::<Feed>::clone(feed);
    // Each check runs at once, where the app runs it on a thread: no test waits on one.
    app.shell_mut().update =
        UpdateNotice::with(fetch, RUNNING, spec.channel, clock.reader(), |check| {
            check()
        });
    let settings = dir.join("settings.toml");
    if let Some(text) = spec.settings
        && !settings.exists()
    {
        fs::write(&settings, text).unwrap();
    }
    if settings.exists() {
        app.shell_mut().set_settings_file(settings);
    }
    app.shell_mut()
        .start_update_check(state_path(dir), &harness.ctx);
    *harness.state_mut() = Some(app);
    harness.run();
    harness
}

/// Runs Help, Check for Updates, as the menu, the palette or a key does.
fn check_for_updates(harness: &mut Window) {
    let app = harness.state_mut().as_mut().unwrap();
    app.shell_mut()
        .execute(qs_layout::CHECK_FOR_UPDATES, serde_json::json!({}))
        .unwrap();
    harness.run();
    harness.run();
}

/// The frames a click is given to be carried out: the press, the release and the redraw.
const CLICK_STEPS: usize = 3;

/// Clicks the button `label` and gives what the window then asked of the system: text to
/// copy, a page to open.
fn click(harness: &mut Window, label: &str) -> Vec<egui::OutputCommand> {
    harness.get_by_label(label).click();
    let mut asked = Vec::new();
    for _ in 0..CLICK_STEPS {
        harness.step();
        asked.extend(harness.output().platform_output.commands.iter().cloned());
    }
    harness.run();
    asked
}

fn item() -> String {
    update_item(LATER)
}

// No spec statement: P4-19, PRIN-9: with `updates.check` unset the app asks nothing of the
// feed as it starts and keeps no state; Help, Check for Updates asks once, and says the later
// release in its window and in the status bar
#[test]
fn nothing_is_asked_until_a_person_asks() {
    let dir = TempDir::new("unasked");
    let feed = Feed::new(Answer::Release(LATER));
    let clock = Clock::at(START);
    let mut harness = window(&Spec::default(), &dir.0, &feed, &clock);
    for _ in 0..3 {
        harness.run();
    }
    assert_eq!(feed.asked(), 0);
    assert!(harness.query_by_label(&item()).is_none());
    assert!(!state_path(&dir.0).exists());

    check_for_updates(&mut harness);
    assert_eq!(feed.asked(), 1);
    harness.get_by_label(&update_available(LATER, RUNNING));
    harness.get_by_label(&item());
    assert!(
        !state_path(&dir.0).exists(),
        "no background check, no state"
    );

    // `off` is the same as unset.
    let dir = TempDir::new("off");
    let spec = Spec {
        settings: Some("[updates]\ncheck = \"off\"\n"),
        ..Spec::default()
    };
    let harness = window(&spec, &dir.0, &feed, &clock);
    assert_eq!(feed.asked(), 1);
    assert!(harness.query_by_label(&item()).is_none());
}

// No spec statement: P4-19: with `updates.check = "daily"`, two starts within a day by the
// clock make one request, the second still announcing what the first found, and a start a
// day later asks again
#[test]
fn a_daily_check_asks_once_a_day() {
    let dir = TempDir::new("daily");
    let spec = Spec {
        settings: Some("[updates]\ncheck = \"daily\"\n"),
        ..Spec::default()
    };
    let feed = Feed::new(Answer::Release(LATER));
    let clock = Clock::at(START);
    let first = window(&spec, &dir.0, &feed, &clock);
    assert_eq!(feed.asked(), 1);
    first.get_by_label(&item());
    // The background check opens no window.
    assert!(
        first
            .query_by_label(&update_available(LATER, RUNNING))
            .is_none()
    );
    drop(first);

    clock.set(START + CHECK_INTERVAL.as_secs() - 1);
    let second = window(&spec, &dir.0, &feed, &clock);
    assert_eq!(feed.asked(), 1, "one request within the day");
    second.get_by_label(&item());
    drop(second);

    clock.set(START + CHECK_INTERVAL.as_secs());
    let third = window(&spec, &dir.0, &feed, &clock);
    assert_eq!(feed.asked(), 2, "a day later it asks again");
    third.get_by_label(&item());
    let state = State::load(&state_path(&dir.0));
    assert!(!state.due(UNIX_EPOCH + Duration::from_secs(START + CHECK_INTERVAL.as_secs())));
}

// No spec statement: P4-19: the same and an older release show no item, and a person who
// asked is told the one running is the latest
#[test]
fn the_same_and_an_older_release_announce_nothing() {
    for latest in [RUNNING, "2026.10.2"] {
        let dir = TempDir::new("current");
        let spec = Spec {
            answer: Answer::Release(latest),
            settings: Some("[updates]\ncheck = \"daily\"\n"),
            ..Spec::default()
        };
        let feed = Feed::new(spec.answer);
        let mut harness = window(&spec, &dir.0, &feed, &Clock::at(START));
        assert_eq!(feed.asked(), 1);
        assert!(harness.query_by_label(&update_item(latest)).is_none());
        assert!(harness.query_by_label(&up_to_date(RUNNING)).is_none());
        check_for_updates(&mut harness);
        assert_eq!(feed.asked(), 2);
        harness.get_by_label(&up_to_date(RUNNING));
        assert!(harness.query_by_label(&update_item(latest)).is_none());
        harness.get_by_label(Text::AboutClose.get()).click();
        harness.run();
        assert!(harness.query_by_label(&up_to_date(RUNNING)).is_none());
    }
}

// No spec statement: P4-19: a background check that fails shows nothing and is tried again
// at the next start, and Help, Check for Updates that fails says it could not check, and why
#[test]
fn a_failed_check_is_said_only_to_a_person_who_asked() {
    let dir = TempDir::new("failed");
    let spec = Spec {
        answer: Answer::Malformed,
        settings: Some("[updates]\ncheck = \"daily\"\n"),
        ..Spec::default()
    };
    let feed = Feed::new(spec.answer);
    let clock = Clock::at(START);
    let failed = |harness: &Window| {
        harness
            .query_by_label_contains(&update_failed(""))
            .is_some()
    };
    let mut harness = window(&spec, &dir.0, &feed, &clock);
    assert_eq!(feed.asked(), 1);
    assert!(!failed(&harness));
    assert!(harness.query_by_label(Text::UpdateTitle.get()).is_none());
    assert!(!state_path(&dir.0).exists(), "no answer is not recorded");

    check_for_updates(&mut harness);
    assert_eq!(feed.asked(), 2);
    assert!(failed(&harness));
    let why = CheckError::Malformed(serde_json::from_str::<()>("<").unwrap_err());
    assert!(why.to_string().contains("is not a release feed"));
    drop(harness);

    // The next start, a second later, asks again: the failure was not counted as a check.
    clock.set(START + 1);
    let again = window(&spec, &dir.0, &feed, &clock);
    assert_eq!(feed.asked(), 3);
    assert!(!failed(&again));
}

// No spec statement: P4-19: the window names both versions and shows the install's own
// command with a Copy button, or the Download button, or both for the Windows installer
#[test]
fn the_window_shows_each_installs_way_to_update() {
    let command = |channel: Channel| channel.command().unwrap();
    for (channel, how, shows_command, shows_download) in [
        (Channel::Homebrew, Text::UpdateViaHomebrew, true, false),
        (Channel::MacPackage, Text::UpdateViaMacPackage, false, true),
        (Channel::Msi, Text::UpdateViaMsi, true, true),
        (Channel::PortableZip, Text::UpdateViaZip, false, true),
        (Channel::Deb, Text::UpdateViaDeb, true, false),
        (Channel::Rpm(RpmTool::Dnf), Text::UpdateViaRpm, true, false),
        (Channel::Rpm(RpmTool::Yum), Text::UpdateViaRpm, true, false),
        (Channel::AppImage, Text::UpdateViaAppImage, false, true),
        (Channel::Flatpak, Text::UpdateViaFlatpak, false, true),
        (Channel::Unpackaged, Text::UpdateViaSource, false, true),
    ] {
        let dir = TempDir::new("channel");
        let spec = Spec {
            channel,
            ..Spec::default()
        };
        let feed = Feed::new(spec.answer);
        let mut harness = window(&spec, &dir.0, &feed, &Clock::at(START));
        check_for_updates(&mut harness);
        harness.get_by_label(&update_available(LATER, RUNNING));
        harness.get_by_label(how.get());
        assert_eq!(channel.command().is_some(), shows_command, "{channel:?}");
        if shows_command {
            harness.get_by_label(command(channel));
            let copied = click(&mut harness, Text::UpdateCopy.get()).iter().any(
                |c| matches!(c, egui::OutputCommand::CopyText(text) if text == command(channel)),
            );
            assert!(copied, "{channel:?}: the command is copied");
        } else {
            assert!(harness.query_by_label(Text::UpdateCopy.get()).is_none());
        }
        assert_eq!(
            harness.query_by_label(Text::UpdateDownload.get()).is_some(),
            shows_download,
            "{channel:?}"
        );
        // Download and Release Notes open the release's page in the browser; the app
        // fetches nothing itself.
        let buttons: &[Text] = if shows_download {
            &[Text::UpdateDownload, Text::UpdateReleaseNotes]
        } else {
            &[Text::UpdateReleaseNotes]
        };
        for button in buttons {
            let opened = click(&mut harness, button.get()).iter().any(|c| {
                matches!(c, egui::OutputCommand::OpenUrl(open)
                    if open.url == format!("{}tag/v{LATER}", qs_update::RELEASES))
            });
            assert!(opened, "{channel:?}: {button:?} opens the release's page");
        }
        assert_eq!(
            feed.asked(),
            1,
            "{channel:?}: one request, whatever is clicked"
        );
        harness.get_by_label(Text::UpdateLater.get()).click();
        harness.run();
        assert!(harness.query_by_label(how.get()).is_none());
        // Not Now leaves the notice, which opens the window again.
        harness.get_by_label(&item()).click();
        harness.run();
        harness.get_by_label(how.get());
    }
}

// No spec statement: P4-19: Skip This Version hides the item for that version alone, here
// and after a restart, where it is kept as `updates.skip_version`; a later version shows the
// item again
#[test]
fn a_skipped_version_is_not_announced_again() {
    let dir = TempDir::new("skip");
    let spec = Spec {
        settings: Some("# mine\n[updates]\ncheck = \"daily\"\n"),
        ..Spec::default()
    };
    let feed = Feed::new(Answer::Release(LATER));
    let clock = Clock::at(START);
    let mut harness = window(&spec, &dir.0, &feed, &clock);
    harness.get_by_label(&item()).click();
    harness.run();
    harness.get_by_label(Text::UpdateSkip.get()).click();
    harness.run();
    harness.run();
    assert!(harness.query_by_label(&item()).is_none());
    assert!(harness.query_by_label(Text::UpdateSkip.get()).is_none());
    assert_eq!(
        fs::read_to_string(dir.0.join("settings.toml")).unwrap(),
        "# mine\n[updates]\ncheck = \"daily\"\nskip_version = \"2026.12.1\"\n"
    );
    // A person who asks is still told of it.
    check_for_updates(&mut harness);
    harness.get_by_label(&update_available(LATER, RUNNING));
    assert!(harness.query_by_label(&item()).is_none());
    drop(harness);

    // After a restart, within the day and a day later, the skipped release stays unsaid.
    for seconds in [START + 1, START + CHECK_INTERVAL.as_secs()] {
        clock.set(seconds);
        let restarted = window(&spec, &dir.0, &feed, &clock);
        assert!(restarted.query_by_label(&item()).is_none());
    }

    // A later release is announced again.
    let later = Feed::new(Answer::Release("2027.1.1"));
    clock.set(START + CHECK_INTERVAL.as_secs() * 2);
    let restarted = window(&spec, &dir.0, &later, &clock);
    assert_eq!(later.asked(), 1);
    restarted.get_by_label(&update_item("2027.1.1"));
}

// No spec statement: P4-19: the build's own version is a release's, three numbers, so every
// build can be compared with the feed
#[test]
fn this_builds_version_is_a_releases() {
    assert!(
        env!("CARGO_PKG_VERSION")
            .parse::<qs_update::Version>()
            .is_ok()
    );
    // A build whose version is not one says so to a person who asks, and asks nothing.
    let feed = Feed::new(Answer::Release(LATER));
    let fetch: Arc<dyn Fetch> = Arc::<Feed>::clone(&feed);
    let mut notice = UpdateNotice::with(
        fetch,
        "2026.11.3-dev",
        Channel::Unpackaged,
        Clock::at(START).reader(),
        |check| check(),
    );
    let ctx = egui::Context::default();
    notice.check(super::Asked::Background, &ctx);
    assert_eq!(notice.shown, super::Shown::Hidden);
    notice.check(super::Asked::Person, &ctx);
    assert!(matches!(&notice.shown, super::Shown::Failed(why) if why.contains("not a version")));
    assert_eq!(feed.asked(), 0);
}
