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
const HOST_NAME: &str = "preview-host";

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
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(path) => write!(f, "no preview host at {}", path.display()),
            Self::Spawn(error) => write!(f, "the preview host did not start: {error}"),
            Self::Exited(status) => write!(f, "the preview host stopped as it started: {status}"),
            Self::Bridge(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for LaunchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(error) => Some(error),
            Self::Bridge(error) => Some(error),
            Self::NotFound(_) | Self::Exited(_) => None,
        }
    }
}

/// The preview host's executable: `configured` when given, else the one beside `app`, the
/// running app's executable (inside the macOS bundle, `Contents/MacOS`, P4-18).
///
/// # Errors
///
/// [`LaunchError::NotFound`] when there is no file there.
pub fn locate_host(configured: Option<&Path>, app: &Path) -> Result<PathBuf, LaunchError> {
    let path = match configured {
        Some(path) => path.to_path_buf(),
        None => app
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

/// The command that starts the host for `spec` (ADR-0008): `-basedir`, `-game` when there is a
/// mod, `-kpf` when
/// the run root passes the file, `-bridge`, then the extra words and the map.
#[must_use]
pub fn host_command(spec: &LaunchSpec) -> Command {
    let mut command = Command::new(&spec.host);
    command.arg("-basedir").arg(&spec.run_root.path);
    if let Some(game) = &spec.run_root.game {
        command.arg("-game").arg(game);
    }
    if let Some(kpf) = &spec.run_root.kpf {
        command.arg("-kpf").arg(kpf);
    }
    command.arg("-bridge").arg(&spec.socket);
    command.args(&spec.extra);
    if let Some(map) = &spec.map {
        command.arg(map);
    }
    command
}

/// Starts the host for `spec` and connects to its bridge, initialized as `client_name`.
///
/// # Errors
///
/// [`LaunchError`] when the process does not start or the bridge does not connect; a host
/// that started is stopped again.
pub fn launch(spec: &LaunchSpec, client_name: &str) -> Result<Running, LaunchError> {
    let mut child = host_command(spec).spawn().map_err(LaunchError::Spawn)?;
    match connect(&mut child, &spec.socket, client_name) {
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
