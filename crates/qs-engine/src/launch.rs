// SPDX-License-Identifier: GPL-3.0-or-later

//! Finding the preview host and starting it (ADR-0005, ADR-0008): the binary shipped beside
//! the app, or a path the user set, never downloaded; launched with `-basedir <run root>
//! -game <mod>`, `-kpf` where the run root cannot link the file, and `-bridge <socket>`.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

use crate::client::{Client, ClientError};
use crate::run_root::RunRoot;

/// The preview host's executable name.
const HOST_NAME: &str = "preview-host";

/// How often, and how far apart, the client tries the host's socket while it starts.
const CONNECT_ATTEMPTS: u32 = 200;
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

/// A host that is running, and the client connected to it.
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
    /// The bridge did not connect or initialize.
    Bridge(ClientError),
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(path) => write!(f, "no preview host at {}", path.display()),
            Self::Spawn(error) => write!(f, "the preview host did not start: {error}"),
            Self::Bridge(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for LaunchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(error) => Some(error),
            Self::Bridge(error) => Some(error),
            Self::NotFound(_) => None,
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

/// The command that starts the host for `spec` (ADR-0008): `-basedir`, `-game`, `-kpf` when
/// the run root passes the file, `-bridge`, then the extra words and the map.
#[must_use]
pub fn host_command(spec: &LaunchSpec) -> Command {
    let mut command = Command::new(&spec.host);
    command
        .arg("-basedir")
        .arg(&spec.run_root.path)
        .arg("-game")
        .arg(&spec.run_root.game);
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
    let connected = Client::connect(&spec.socket, CONNECT_ATTEMPTS, CONNECT_PAUSE)
        .and_then(|mut client| client.initialize(client_name).map(|_| client));
    match connected {
        Ok(client) => Ok(Running { child, client }),
        Err(error) => {
            // The host is of no use without its bridge.
            if let Err(kill) = child.kill() {
                tracing::warn!(%kill, "the preview host could not be stopped");
            }
            Err(LaunchError::Bridge(error))
        }
    }
}
