// SPDX-License-Identifier: GPL-3.0-or-later

//! How this copy of the app was installed, found from files and the environment alone, and so
//! how it is updated: no process is started and nothing is asked of a package manager.

use std::ffi::OsString;
use std::path::PathBuf;

/// The system the app runs on, as far as installing goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    /// macOS.
    Mac,
    /// Windows.
    Windows,
    /// Linux, and any other Unix.
    Linux,
}

impl Os {
    /// The system this build runs on.
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// Whether the AppImage runtime started the app: it sets `APPIMAGE` to the image's path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppImage {
    /// `APPIMAGE` is set.
    Running,
    /// It is not.
    NotRunning,
}

/// What of a machine says how the app was installed on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    /// The system.
    pub os: Os,
    /// The running executable.
    pub exe: PathBuf,
    /// The root of the file system, `/`; a test gives a directory of its own.
    pub root: PathBuf,
    /// The folders installers put programs in for every user, Windows' `ProgramFiles` and
    /// `ProgramW6432`.
    pub program_files: Vec<PathBuf>,
    /// Whether the AppImage runtime started the app.
    pub appimage: AppImage,
}

/// Homebrew's record of the cask, below the root, on Apple silicon, the one Mac the app is
/// built for (`packaging/homebrew/quakec-studio.rb`).
const HOMEBREW_CASK: &str = "opt/homebrew/Caskroom/quakec-studio";
/// dpkg's list of the deb's files, below the root (`packaging/linux/deb/control`).
const DPKG_LIST: &str = "var/lib/dpkg/info/quakec-studio.list";
/// Where an rpm database is kept, below the root: the old place and the one Fedora 36 moved
/// it to.
const RPM_DATABASES: [&str; 2] = ["var/lib/rpm", "usr/lib/sysimage/rpm"];
/// Where a package manager installs, below the root; the deb and the rpm put the app there.
const SYSTEM_PREFIX: &str = "usr";
/// dnf, below the root; a system with an rpm database and no dnf has yum.
const DNF: &str = "usr/bin/dnf";
/// The file Flatpak puts at the root of every sandbox.
const FLATPAK_INFO: &str = ".flatpak-info";
/// The variable the AppImage runtime sets.
const APPIMAGE_VARIABLE: &str = "APPIMAGE";
/// The variables Windows names its program folders in.
const PROGRAM_FILES_VARIABLES: [&str; 2] = ["ProgramFiles", "ProgramW6432"];

impl Install {
    /// This machine's, for the running executable; none when the system cannot say which
    /// file that is.
    #[must_use]
    pub fn current() -> Option<Self> {
        let exe = std::env::current_exe()
            .map_err(|error| tracing::warn!(%error, "the running executable is not known"))
            .ok()?;
        Some(Self::of(exe, |name| std::env::var_os(name)))
    }

    /// This system's, for `exe` and the environment `env` reads.
    pub(crate) fn of(exe: PathBuf, env: impl Fn(&str) -> Option<OsString>) -> Self {
        Self {
            os: Os::current(),
            exe,
            root: PathBuf::from("/"),
            program_files: PROGRAM_FILES_VARIABLES
                .iter()
                .filter_map(|name| env(name))
                .map(PathBuf::from)
                .collect(),
            appimage: if env(APPIMAGE_VARIABLE).is_some() {
                AppImage::Running
            } else {
                AppImage::NotRunning
            },
        }
    }

    /// Whether `path`, below the root, exists.
    fn has(&self, path: &str) -> bool {
        self.root.join(path).exists()
    }

    /// How the app was installed.
    #[must_use]
    pub fn channel(&self) -> Channel {
        match self.os {
            Os::Mac if self.has(HOMEBREW_CASK) => Channel::Homebrew,
            Os::Mac => Channel::MacPackage,
            Os::Windows
                if self
                    .program_files
                    .iter()
                    .any(|dir| self.exe.starts_with(dir)) =>
            {
                Channel::Msi
            }
            Os::Windows => Channel::PortableZip,
            Os::Linux => self.linux_channel(),
        }
    }

    /// [`Install::channel`] on Linux. The sandbox and the image are asked first: inside them
    /// the host's package lists may show too.
    fn linux_channel(&self) -> Channel {
        if self.has(FLATPAK_INFO) {
            return Channel::Flatpak;
        }
        if self.appimage == AppImage::Running {
            return Channel::AppImage;
        }
        if !self.exe.starts_with(self.root.join(SYSTEM_PREFIX)) {
            return Channel::Unpackaged;
        }
        if self.has(DPKG_LIST) {
            Channel::Deb
        } else if RPM_DATABASES.iter().any(|database| self.has(database)) {
            Channel::Rpm(if self.has(DNF) {
                RpmTool::Dnf
            } else {
                RpmTool::Yum
            })
        } else {
            Channel::Unpackaged
        }
    }
}

/// The program that updates an rpm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RpmTool {
    /// dnf, on Fedora and RHEL 8 and later.
    Dnf,
    /// yum, where there is no dnf.
    Yum,
}

/// Whether a channel's update is a download from the release page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Download {
    /// The release page has the new package to download.
    Offered,
    /// The channel's command brings the update; nothing is downloaded by hand.
    NotOffered,
}

/// How the app was installed, and so how it is updated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// Homebrew's cask, `quakecstudio/tap/quakec-studio`.
    Homebrew,
    /// The macOS installer package, downloaded.
    MacPackage,
    /// The Windows MSI, downloaded or installed by winget, which updates either.
    Msi,
    /// The Windows portable zip.
    PortableZip,
    /// The deb, from the APT repository or downloaded.
    Deb,
    /// The rpm, from the YUM/DNF repository or downloaded.
    Rpm(RpmTool),
    /// The AppImage.
    AppImage,
    /// The Flatpak bundle.
    Flatpak,
    /// None of them: a build run from where it was made.
    Unpackaged,
}

impl Channel {
    /// The command that updates an install of this channel; none where the update is a
    /// download.
    #[must_use]
    pub const fn command(self) -> Option<&'static str> {
        match self {
            Self::Homebrew => Some("brew upgrade --cask quakec-studio"),
            Self::Msi => Some("winget upgrade SmokeTurner.QuakeCStudio"),
            Self::Deb => Some("sudo apt update && sudo apt install --only-upgrade quakec-studio"),
            Self::Rpm(RpmTool::Dnf) => Some("sudo dnf upgrade quakec-studio"),
            Self::Rpm(RpmTool::Yum) => Some("sudo yum upgrade quakec-studio"),
            Self::MacPackage
            | Self::PortableZip
            | Self::AppImage
            | Self::Flatpak
            | Self::Unpackaged => None,
        }
    }

    /// Whether the update is offered as a download: where there is no command, and beside
    /// winget's, since Microsoft takes a release into winget in its own time.
    #[must_use]
    pub const fn download(self) -> Download {
        match self {
            Self::Homebrew | Self::Deb | Self::Rpm(_) => Download::NotOffered,
            Self::Msi
            | Self::MacPackage
            | Self::PortableZip
            | Self::AppImage
            | Self::Flatpak
            | Self::Unpackaged => Download::Offered,
        }
    }
}
