// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or a panic is a failed test, and the times and counts \
              are small"
)]

use std::io::{BufRead as _, BufReader, ErrorKind, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{
    AppImage, CHECK_INTERVAL, Channel, CheckError, Download, FEED_URL, Feed, Fetch, Found, Https,
    Install, Os, RpmTool, State, USER_AGENT, Version, state_path,
};

/// The feed the release workflow fills in and publishes (`.github/workflows/release.yml`).
const TEMPLATE: &str = include_str!("../../../packaging/latest.json");

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory for test `name`, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("qs-update-{}-{name}-{n}", std::process::id()));
        let _ignored = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

fn version(text: &str) -> Version {
    text.parse().unwrap()
}

/// The feed text naming `version`, as the release workflow writes it.
fn feed_text(version: &str) -> String {
    TEMPLATE.replace("@VERSION@", version)
}

/// A fetch answering with `text`, counting how often it is asked.
struct Fake {
    text: String,
    asked: AtomicUsize,
}

impl Fake {
    fn naming(version: &str) -> Self {
        Self {
            text: feed_text(version),
            asked: AtomicUsize::new(0),
        }
    }
}

impl Fetch for Fake {
    fn latest(&self) -> Result<String, CheckError> {
        self.asked.fetch_add(1, Ordering::Relaxed);
        Ok(self.text.clone())
    }
}

// No spec statement: P4-19: a version is three numbers, YYYY.MM.N, ordered as the releases
// were made, the month and the release as numbers and not as text
#[test]
fn versions_order_as_their_releases() {
    assert!(version("2026.12.1") > version("2026.11.3"));
    assert!(version("2027.1.1") > version("2026.12.9"));
    assert!(version("2026.11.10") > version("2026.11.9"));
    assert_eq!(version("2026.01.1"), version("2026.1.1"));
    assert_eq!(version("2026.11.3").to_string(), "2026.11.3");
    assert_eq!(
        serde_json::to_string(&version("2026.11.3")).unwrap(),
        "\"2026.11.3\""
    );
}

// No spec statement: P4-19: what is not three plain numbers is no version: a pre-release
// suffix, a sign, a space, a missing or an extra part, a number too large
#[test]
fn what_is_not_three_numbers_is_no_version() {
    for text in [
        "",
        "2026",
        "2026.11",
        "2026.11.1.4",
        "2026.11.1-rc1",
        "v2026.11.1",
        "2026.+11.1",
        "2026. 11.1",
        "2026..1",
        "2026.11.",
        "2026.11.99999999999",
    ] {
        let error = text.parse::<Version>().unwrap_err();
        assert!(error.to_string().contains("is not a version"), "{text}");
    }
}

// No spec statement: P4-19: the feed the release workflow writes from packaging/latest.json
// parses, naming the release's version and its page
#[test]
fn the_feed_the_release_writes_parses() {
    let feed = Feed::parse(&feed_text("2026.12.1")).unwrap();
    assert_eq!(feed.version(), version("2026.12.1"));
    assert_eq!(
        feed.page(),
        "https://github.com/quakecstudio/editor/releases/tag/v2026.12.1"
    );
    // A later feed may say more.
    let more = r#"{"version":"2026.12.1","url":"https://github.com/quakecstudio/editor/releases/tag/v2026.12.1","notes":"x"}"#;
    assert_eq!(Feed::parse(more).unwrap(), feed);
}

// No spec statement: P4-19: a newer version is found, and the same and an older one are not
#[test]
fn only_a_later_release_is_newer() {
    let current = version("2026.11.3");
    let newer = Fake::naming("2026.12.1");
    let found = Found::check(&newer, current).unwrap();
    assert_eq!(
        found,
        Found::Newer(Feed::parse(&feed_text("2026.12.1")).unwrap())
    );
    for same_or_older in ["2026.11.3", "2026.11.2", "2025.12.9"] {
        let fake = Fake::naming(same_or_older);
        assert_eq!(Found::check(&fake, current).unwrap(), Found::UpToDate);
        assert_eq!(fake.asked.load(Ordering::Relaxed), 1, "one read a check");
    }
}

// No spec statement: P4-19: a feed that is not the object, whose version is not one, or
// whose page is not one of the app's release pages is refused, each saying why
#[test]
fn a_malformed_feed_is_refused() {
    for text in [
        "",
        "not json",
        "[]",
        r#"{"version":"2026.12.1"}"#,
        r#"{"url":"https://github.com/quakecstudio/editor/releases/tag/v1"}"#,
        r#"{"version":"2026.12.1-rc1","url":"https://github.com/quakecstudio/editor/releases/tag/v1"}"#,
        r#"{"version":20261201,"url":"https://github.com/quakecstudio/editor/releases/tag/v1"}"#,
    ] {
        let error = Feed::parse(text).unwrap_err();
        assert!(matches!(error, CheckError::Malformed(_)), "{text}");
        assert!(error.to_string().contains("is not a release feed"));
    }
    for page in [
        "https://example.com/quakec-studio.pkg",
        "http://github.com/quakecstudio/editor/releases/tag/v2026.12.1",
        "https://github.com/quakecstudio/editor.evil/releases/",
        "file:///etc/passwd",
    ] {
        let text = format!(r#"{{"version":"2026.12.1","url":"{page}"}}"#);
        let error = Feed::parse(&text).unwrap_err();
        assert!(matches!(&error, CheckError::NotAReleasePage(url) if url == page));
        assert!(error.to_string().contains("is not a release page"));
    }
    let fake = Fake {
        text: "not json".to_owned(),
        asked: AtomicUsize::new(0),
    };
    assert!(Found::check(&fake, version("2026.11.3")).is_err());
}

/// Serves `body` once on a local port; gives the feed's address there and, once joined, the
/// request's lines up to the blank one, whatever followed them, and how many more
/// connections were waiting.
fn serve_once(
    status: &'static str,
    body: String,
) -> (String, std::thread::JoinHandle<(Vec<String>, usize)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/latest.json", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let line = line.trim_end().to_owned();
            if line.is_empty() {
                break;
            }
            lines.push(line);
        }
        let answer = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        reader.get_mut().write_all(answer.as_bytes()).unwrap();
        drop(reader);
        // Any other request the client made would be waiting here.
        listener.set_nonblocking(true).unwrap();
        let mut more = 0;
        loop {
            match listener.accept() {
                Ok(_) => more += 1,
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) => panic!("{error}"),
            }
        }
        (lines, more)
    });
    (url, server)
}

// No spec statement: P4-19, PRIN-9: a check is one GET of the feed and nothing else, with no
// query, no body and the fixed User-Agent, which names neither the version nor the install
#[test]
fn a_check_is_one_get_that_says_only_the_apps_name() {
    let (url, server) = serve_once("200 OK", feed_text("2026.12.1"));
    let found = Found::check(&Https::at(url), version("2026.11.3")).unwrap();
    assert!(matches!(found, Found::Newer(feed) if feed.version() == version("2026.12.1")));
    let (lines, more) = server.join().unwrap();
    assert_eq!(more, 0, "one request");
    assert_eq!(
        lines.first().map(String::as_str),
        Some("GET /latest.json HTTP/1.1")
    );
    let headers: Vec<(String, &str)> = lines
        .iter()
        .skip(1)
        .map(|line| {
            let (name, value) = line.split_once(':').unwrap();
            (name.to_ascii_lowercase(), value.trim())
        })
        .collect();
    assert_eq!(USER_AGENT, "quakec-studio");
    let agents: Vec<&str> = headers
        .iter()
        .filter(|(name, _)| name == "user-agent")
        .map(|(_, value)| *value)
        .collect();
    assert_eq!(agents, [USER_AGENT]);
    // Nothing is sent with it: no body, no cookie, no credential, nothing naming the install.
    for (name, value) in &headers {
        assert!(
            ["host", "user-agent", "accept", "accept-encoding"].contains(&name.as_str()),
            "{name}: {value}"
        );
        assert!(
            !value.contains(env!("CARGO_PKG_VERSION")),
            "{name}: {value}"
        );
    }
    assert!(!FEED_URL.contains('?'));
    assert_eq!(FEED_URL, "https://packages.quakecstudio.com/latest.json");
}

// No spec statement: P4-19: a feed that cannot be reached, or whose server answers with an
// error, is an error that says so, not an answer
#[test]
fn an_unreachable_feed_is_an_error() {
    // A port nothing listens on: bound, then given up.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/latest.json", listener.local_addr().unwrap());
    drop(listener);
    let error = Found::check(&Https::at(url), version("2026.11.3")).unwrap_err();
    assert!(matches!(error, CheckError::Request(_)));
    assert!(error.to_string().starts_with(FEED_URL));

    let (url, server) = serve_once("404 Not Found", String::new());
    let error = Found::check(&Https::at(url), version("2026.11.3")).unwrap_err();
    assert!(matches!(error, CheckError::Request(_)));
    assert_eq!(server.join().unwrap().1, 0, "no second try");
}

// No spec statement: P4-19, PRIN-9: a redirect is not followed, so a check asks the feed's
// host and no other; it is an error, as an unreachable feed is
#[test]
fn a_redirect_is_not_followed() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let elsewhere = format!("http://{}/elsewhere.json", listener.local_addr().unwrap());
    let status: &'static str =
        Box::leak(format!("302 Found\r\nLocation: {elsewhere}").into_boxed_str());
    let (url, server) = serve_once(status, String::new());
    let error = Found::check(&Https::at(url), version("2026.11.3")).unwrap_err();
    assert!(matches!(error, CheckError::Status(302)), "{error}");
    assert!(error.to_string().ends_with("answered with status 302"));
    assert_eq!(server.join().unwrap().1, 0);
    // Nothing asked the address the redirect named.
    listener.set_nonblocking(true).unwrap();
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        ErrorKind::WouldBlock,
        "the redirect was followed"
    );
}

/// What of a machine a test sets: the ordinary case is a Linux machine with nothing of the
/// app's on it, the app run from a build directory.
struct Machine {
    os: Os,
    /// The executable, below the root.
    exe: &'static str,
    /// The files and directories there are, below the root.
    present: &'static [&'static str],
    /// Windows' program folders, below the root.
    program_files: &'static [&'static str],
    appimage: AppImage,
}

impl Default for Machine {
    fn default() -> Self {
        Self {
            os: Os::Linux,
            exe: "home/dev/editor/target/release/quakec-studio",
            present: &[],
            program_files: &[],
            appimage: AppImage::NotRunning,
        }
    }
}

impl Machine {
    /// The machine as a file tree below a fresh root, and what it says of the install.
    fn install(&self, name: &str) -> (TempDir, Install) {
        let dir = TempDir::new(name);
        for path in self.present {
            std::fs::create_dir_all(dir.0.join(path)).unwrap();
        }
        let install = Install {
            os: self.os,
            exe: dir.0.join(self.exe),
            root: dir.0.clone(),
            program_files: self.program_files.iter().map(|p| dir.0.join(p)).collect(),
            appimage: self.appimage,
        };
        (dir, install)
    }

    fn channel(&self, name: &str) -> Channel {
        self.install(name).1.channel()
    }
}

// No spec statement: P4-19: Homebrew's cask is known by its Caskroom record, and its update is
// brew's
#[test]
fn homebrew_is_found_by_its_caskroom() {
    let machine = Machine {
        os: Os::Mac,
        exe: "Applications/QuakeC Studio.app/Contents/MacOS/quakec-studio",
        present: &["opt/homebrew/Caskroom/quakec-studio"],
        ..Machine::default()
    };
    let channel = machine.channel("homebrew");
    assert_eq!(channel, Channel::Homebrew);
    assert_eq!(channel.command(), Some("brew upgrade --cask quakec-studio"));
    assert_eq!(channel.download(), Download::NotOffered);
}

// No spec statement: P4-19: a Mac with no Caskroom record has the downloaded package, even
// with other casks installed, and its update is a download
#[test]
fn the_macos_package_is_a_mac_without_the_cask() {
    let machine = Machine {
        os: Os::Mac,
        exe: "Applications/QuakeC Studio.app/Contents/MacOS/quakec-studio",
        present: &["opt/homebrew/Caskroom/zed"],
        ..Machine::default()
    };
    let channel = machine.channel("pkg");
    assert_eq!(channel, Channel::MacPackage);
    assert_eq!(channel.command(), None);
    assert_eq!(channel.download(), Download::Offered);
}

// No spec statement: P4-19: on Windows a copy running from a program folder is the MSI's,
// updated by winget or by a download, since winget lags a release
#[test]
fn the_msi_runs_from_program_files() {
    for folder in ["Program Files", "Program Files (Arm)"] {
        let machine = Machine {
            os: Os::Windows,
            exe: if folder == "Program Files" {
                "Program Files/QuakeC Studio/quakec-studio.exe"
            } else {
                "Program Files (Arm)/QuakeC Studio/quakec-studio.exe"
            },
            program_files: &["Program Files (Arm)", "Program Files"],
            ..Machine::default()
        };
        let channel = machine.channel("msi");
        assert_eq!(channel, Channel::Msi, "{folder}");
        assert_eq!(
            channel.command(),
            Some("winget upgrade SmokeTurner.QuakeCStudio")
        );
        assert_eq!(channel.download(), Download::Offered);
    }
}

// No spec statement: P4-19: a Windows copy anywhere else is the portable zip's, even in a
// folder whose name only starts as a program folder's does; its update is a download
#[test]
fn the_portable_zip_runs_from_anywhere_else() {
    for exe in [
        "Users/dev/Downloads/quakec-studio/quakec-studio.exe",
        "Program Files Portable/quakec-studio.exe",
    ] {
        let machine = Machine {
            os: Os::Windows,
            exe,
            program_files: &["Program Files"],
            ..Machine::default()
        };
        let channel = machine.channel("zip");
        assert_eq!(channel, Channel::PortableZip, "{exe}");
        assert_eq!(channel.command(), None);
        assert_eq!(channel.download(), Download::Offered);
    }
    // A path started in another case is the same folder to Windows.
    let machine = Machine {
        os: Os::Windows,
        exe: "program files/quakec studio/QUAKEC-STUDIO.EXE",
        program_files: &["Program Files"],
        ..Machine::default()
    };
    assert_eq!(machine.channel("msi-case"), Channel::Msi);
    // With no program folder named in the environment, nothing is the MSI's.
    let machine = Machine {
        os: Os::Windows,
        exe: "Program Files/QuakeC Studio/quakec-studio.exe",
        ..Machine::default()
    };
    assert_eq!(machine.channel("zip-no-env"), Channel::PortableZip);
}

// No spec statement: P4-19: the deb is known by dpkg's list of its files, and its update is
// apt's
#[test]
fn the_deb_is_found_by_dpkgs_list() {
    let machine = Machine {
        exe: "usr/lib/quakec-studio/quakec-studio",
        // Debian has an rpm database too when rpm is installed; dpkg's list wins.
        present: &["var/lib/dpkg/info/quakec-studio.list", "var/lib/rpm"],
        ..Machine::default()
    };
    let channel = machine.channel("deb");
    assert_eq!(channel, Channel::Deb);
    assert_eq!(
        channel.command(),
        Some("sudo apt update && sudo apt install --only-upgrade quakec-studio")
    );
    assert_eq!(channel.download(), Download::NotOffered);
}

// No spec statement: P4-19: the rpm is a copy under /usr on a system with an rpm database,
// in either place one is kept, updated with dnf, or with yum where there is no dnf
#[test]
fn the_rpm_is_found_by_the_rpm_database() {
    for (present, tool, command) in [
        (
            &["usr/lib/sysimage/rpm", "usr/bin/dnf"][..],
            RpmTool::Dnf,
            "sudo dnf upgrade quakec-studio",
        ),
        (
            &["var/lib/rpm", "usr/bin/dnf"][..],
            RpmTool::Dnf,
            "sudo dnf upgrade quakec-studio",
        ),
        (
            &["var/lib/rpm"][..],
            RpmTool::Yum,
            "sudo yum upgrade quakec-studio",
        ),
    ] {
        let machine = Machine {
            exe: "usr/bin/quakec-studio",
            present,
            ..Machine::default()
        };
        let channel = machine.channel("rpm");
        assert_eq!(channel, Channel::Rpm(tool));
        assert_eq!(channel.command(), Some(command));
        assert_eq!(channel.download(), Download::NotOffered);
    }
}

// No spec statement: P4-19: the AppImage is known by the variable its runtime sets, whatever
// packages the host has; its update is a download
#[test]
fn the_appimage_is_found_by_its_variable() {
    let machine = Machine {
        exe: "tmp/.mount_quakec/usr/lib/quakec-studio/quakec-studio",
        present: &["var/lib/dpkg/info/quakec-studio.list"],
        appimage: AppImage::Running,
        ..Machine::default()
    };
    let channel = machine.channel("appimage");
    assert_eq!(channel, Channel::AppImage);
    assert_eq!(channel.command(), None);
    assert_eq!(channel.download(), Download::Offered);
}

// No spec statement: P4-19: the Flatpak is known by the file at its sandbox's root, before
// anything else is asked; its update is a download, the bundle having no remote
#[test]
fn the_flatpak_is_found_by_its_sandbox_file() {
    let machine = Machine {
        exe: "app/bin/quakec-studio",
        present: &[".flatpak-info", "var/lib/rpm"],
        appimage: AppImage::Running,
        ..Machine::default()
    };
    let channel = machine.channel("flatpak");
    assert_eq!(channel, Channel::Flatpak);
    assert_eq!(channel.command(), None);
    assert_eq!(channel.download(), Download::Offered);
}

// No spec statement: P4-19: a build run from where it was made is no package's, on a system
// with dpkg's list or an rpm database as on one with neither, a copy under /usr with neither
// is none's too, and so is one under /usr/local with either; the release page is where it is
// sent
#[test]
fn a_build_run_in_place_is_unpackaged() {
    for machine in [
        Machine::default(),
        Machine {
            present: &["var/lib/dpkg/info/quakec-studio.list", "var/lib/rpm"],
            ..Machine::default()
        },
        Machine {
            exe: "usr/bin/quakec-studio",
            ..Machine::default()
        },
        // /usr/local is the machine's own, whatever packages the system has.
        Machine {
            exe: "usr/local/bin/quakec-studio",
            present: &["var/lib/rpm", "usr/bin/dnf"],
            ..Machine::default()
        },
        Machine {
            exe: "usr/local/bin/quakec-studio",
            present: &["var/lib/dpkg/info/quakec-studio.list"],
            ..Machine::default()
        },
    ] {
        let channel = machine.channel("unpackaged");
        assert_eq!(channel, Channel::Unpackaged);
        assert_eq!(channel.command(), None);
        assert_eq!(channel.download(), Download::Offered);
    }
}

// No spec statement: P4-19: this machine's install reads the executable, the program folders
// and the AppImage variable from the environment given, and nothing else
#[test]
fn the_current_install_reads_the_environment() {
    let exe = PathBuf::from("/usr/bin/quakec-studio");
    let install = Install::of(exe.clone(), |name| match name {
        "ProgramFiles" => Some("C:\\Program Files".into()),
        "APPIMAGE" => Some("/home/dev/quakec-studio.AppImage".into()),
        _ => None,
    });
    assert_eq!(install.os, Os::current());
    assert_eq!(install.exe, exe);
    assert_eq!(install.root, Path::new("/"));
    assert_eq!(install.program_files, [PathBuf::from("C:\\Program Files")]);
    assert_eq!(install.appimage, AppImage::Running);
    let bare = Install::of(exe, |_| None);
    assert!(bare.program_files.is_empty());
    assert_eq!(bare.appimage, AppImage::NotRunning);
    assert!(Install::current().is_some());
}

/// `seconds` after the Unix epoch.
fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

// No spec statement: P4-19: a background check is due when none had an answer, a day after
// the last, and when the last is later than now; not a second before the day is up
#[test]
fn a_check_is_due_once_a_day() {
    const START: u64 = 1_800_000_000;
    let day = CHECK_INTERVAL.as_secs();
    assert_eq!(day, 86_400);
    let mut state = State::default();
    assert!(state.due(at(START)));
    state.record(at(START), &Found::UpToDate);
    assert!(!state.due(at(START)));
    assert!(!state.due(at(START + day - 1)));
    assert!(state.due(at(START + day)));
    assert!(state.due(at(START + day + 1)));
    assert!(state.due(at(START - 1)), "the clock was set back");
}

// No spec statement: P4-19: the state is kept in update.json of the data directory and read
// back as written, with the release found; no file, and one that is not a state, read as
// nothing checked
#[test]
fn the_state_is_kept_between_runs() {
    const START: u64 = 1_800_000_000;
    let dir = TempDir::new("state");
    let path = state_path(&dir.0.join("data"));
    assert_eq!(path, dir.0.join("data").join("update.json"));
    assert_eq!(State::load(&path), State::default());
    let feed = Feed::parse(&feed_text("2026.12.1")).unwrap();
    let mut state = State::default();
    state.record(at(START), &Found::Newer(feed.clone()));
    state.save(&path).unwrap();
    let back = State::load(&path);
    assert_eq!(back, state);
    assert!(!back.due(at(START + 1)));
    assert_eq!(back.newer_than(version("2026.11.3")), Some(&feed));
    // An install updated since has that release already.
    assert_eq!(back.newer_than(version("2026.12.1")), None);
    assert_eq!(back.newer_than(version("2027.1.1")), None);
    // A later check that finds nothing newer forgets it.
    let mut later = back;
    later.record(at(START + CHECK_INTERVAL.as_secs()), &Found::UpToDate);
    assert_eq!(later.newer_than(version("2026.11.3")), None);

    std::fs::write(&path, "not a state").unwrap();
    assert_eq!(State::load(&path), State::default());
    // A state that cannot be written says so.
    let blocked = dir.0.join("file");
    std::fs::write(&blocked, "").unwrap();
    assert!(state.save(&blocked.join("update.json")).is_err());
}
