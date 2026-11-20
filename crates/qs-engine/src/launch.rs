// SPDX-License-Identifier: GPL-3.0-or-later

//! Finding the preview host and starting it (ADR-0005, ADR-0008): the binary shipped beside
//! the app, or a path the user set, never downloaded; launched with `-basedir <run root>
//! -game <mod>`, `-kpf` where the run root cannot link the file, and `-bridge <socket>`.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::Duration;

use crate::client::{Client, ClientError};
use crate::run_root::RunRoot;

/// The preview host's executable name.
const HOST_NAME: &str = "quakec-engine";

/// How often, and how far apart, the client tries the host's socket while it starts: for a
/// minute, since the host loads the whole map before it listens.
const CONNECT_ATTEMPTS: u32 = 1200;
const CONNECT_PAUSE: Duration = Duration::from_millis(50);

/// What to launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchSpec {
    /// The preview host's executable ([`locate_host`]).
    pub host: PathBuf,
    /// The run root to play in.
    pub run_root: RunRoot,
    /// The map to start on, the host's own default when `None`.
    pub map: Option<String>,
    /// Where the host listens for the bridge (BRIDGE-1.1).
    pub socket: PathBuf,
    /// Further words of the host's command line, such as `--nosound`.
    pub extra: Vec<String>,
}

/// A host that is running, and the client connected to it. The caller owns the process: it
/// sends `shutdown` and waits for it, or kills it.
pub struct Running {
    /// The host's process.
    pub child: Child,
    /// The bridge, initialized.
    pub client: Client,
}

/// A host the app runs in its own process, in its own window (ADR-0028), in place of the
/// preview host's process: the embedded game view. The app's top layer implements it; the
/// layers below start and stop the game through it and speak the same bridge to it.
pub trait EmbeddedHost {
    /// Whether the app shows the game in its own window now: it has a device to draw on, and
    /// its layout puts the game there.
    fn shows(&self) -> bool;

    /// Starts the game `spec` describes, its host and socket aside, and gives the bridge's
    /// client, initialized as `client_name`.
    ///
    /// # Errors
    ///
    /// [`LaunchError::Embedded`] when the game does not load, [`LaunchError::Bridge`] when its
    /// bridge does not initialize.
    fn start(&mut self, spec: &LaunchSpec, client_name: &str) -> Result<Client, LaunchError>;

    /// Stops the game the last [`EmbeddedHost::start`] made, if it still runs.
    fn stop(&mut self);
}

/// Why the host did not start or connect.
#[derive(Debug)]
pub enum LaunchError {
    /// No preview host at the configured path, or beside the app.
    NotFound(PathBuf),
    /// The process did not start.
    Spawn(std::io::Error),
    /// The host stopped before its bridge connected, with this status.
    Exited(ExitStatus),
    /// The bridge did not connect or initialize.
    Bridge(ClientError),
    /// The game did not start in the app's own process (ADR-0028), with why.
    Embedded(String),
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(path) => write!(f, "no preview host at {}", path.display()),
            Self::Spawn(error) => write!(f, "the preview host did not start: {error}"),
            Self::Exited(status) => write!(f, "the preview host stopped as it started: {status}"),
            Self::Bridge(error) => write!(f, "{error}"),
            Self::Embedded(why) => write!(f, "the game did not start: {why}"),
        }
    }
}

impl std::error::Error for LaunchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(error) => Some(error),
            Self::Bridge(error) => Some(error),
            Self::NotFound(_) | Self::Exited(_) | Self::Embedded(_) => None,
        }
    }
}

/// The preview host's executable: `configured` when given, else the one beside `app`, the
/// running app's executable (inside the macOS bundle, `Contents/MacOS`, P4-18). A link to the
/// app, as the macOS package puts in `/usr/local/bin`, is followed first, so the host is the
/// one beside the real executable.
///
/// # Errors
///
/// [`LaunchError::NotFound`] when there is no file there.
pub fn locate_host(configured: Option<&Path>, app: &Path) -> Result<PathBuf, LaunchError> {
    let path = match configured {
        Some(path) => path.to_path_buf(),
        None => std::fs::canonicalize(app)
            .unwrap_or_else(|_| app.to_path_buf())
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{HOST_NAME}{}", std::env::consts::EXE_SUFFIX)),
    };
    if path.is_file() {
        Ok(path)
    } else {
        Err(LaunchError::NotFound(path))
    }
}

impl LaunchSpec {
    /// The extra word that runs the host with no window and no sound, its frames drawn
    /// offscreen for a screenshot.
    pub const HEADLESS: &'static str = "--headless";

    /// The command that starts the host (ADR-0008): `-basedir`, `-game` when there is a mod,
    /// `-kpf` when the run root passes the file, `-bridge`, then the extra words and the map.
    #[must_use]
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.host);
        command.args(self.game_words());
        command.arg("-bridge").arg(&self.socket);
        command.args(&self.extra);
        if let Some(map) = &self.map {
            command.arg(map);
        }
        command
    }

    /// The words that name the game to play (ADR-0008): `-basedir`, `-game` when there is a
    /// mod, and `-kpf` when the run root passes the file.
    #[must_use]
    pub fn game_words(&self) -> Vec<std::ffi::OsString> {
        let mut words: Vec<std::ffi::OsString> =
            vec!["-basedir".into(), self.run_root.path.clone().into()];
        if let Some(game) = &self.run_root.game {
            words.extend(["-game".into(), game.into()]);
        }
        if let Some(kpf) = &self.run_root.kpf {
            words.extend(["-kpf".into(), kpf.clone().into()]);
        }
        words
    }

    /// Starts the host and connects to its bridge, initialized as `client_name`.
    ///
    /// # Errors
    ///
    /// [`LaunchError`] when the process does not start or the bridge does not connect; a host
    /// that started is stopped again.
    pub fn launch(&self, client_name: &str) -> Result<Running, LaunchError> {
        let mut child = self.command().spawn().map_err(LaunchError::Spawn)?;
        match connect(&mut child, &self.socket, client_name) {
            Ok(client) => Ok(Running { child, client }),
            Err(error) => {
                // The host is of no use without its bridge; it is waited for, so it leaves no
                // process behind.
                if let Err(kill) = child.kill().and_then(|()| child.wait().map(drop)) {
                    tracing::warn!(%kill, "the preview host could not be stopped");
                }
                Err(error)
            }
        }
    }
}

/// Connects to the host `child` starts at `socket` and initializes the bridge, trying while
/// the host runs; a host that stops first is [`LaunchError::Exited`].
fn connect(child: &mut Child, socket: &Path, client_name: &str) -> Result<Client, LaunchError> {
    let mut last = ClientError::Timeout;
    for _ in 0..CONNECT_ATTEMPTS {
        if let Some(status) = child.try_wait().map_err(LaunchError::Spawn)? {
            return Err(LaunchError::Exited(status));
        }
        match Client::connect(socket, 1, Duration::ZERO) {
            Ok(mut client) => {
                client
                    .initialize(client_name)
                    .map_err(LaunchError::Bridge)?;
                return Ok(client);
            }
            Err(error) => last = error,
        }
        std::thread::sleep(CONNECT_PAUSE);
    }
    Err(LaunchError::Bridge(last))
}
