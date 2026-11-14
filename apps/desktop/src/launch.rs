// SPDX-License-Identifier: GPL-3.0-or-later

//! One window for every launch: a launch with paths hands them to the copy of the app already
//! running, over the app's socket, as Zed's launches do on Windows and Linux (a named pipe and a
//! Unix socket there). Explorer starts one launch per file it opens, so launches can start
//! together with no window yet: the one that takes the app's lock file opens the window, and
//! the others wait for its socket and hand it their paths, as Zed's instance mutex decides.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use qs_commands::remote::Client;
use qs_workbench::open_paths::{OPEN_PATHS, OpenPathsParams};

/// How long a launch waits for the running copy to answer. One busy in a native dialog answers
/// once it closes; past this the launch leaves the paths with it and exits.
const ANSWER_WAIT: Duration = Duration::from_secs(10);

/// How long a launch that finds another opening the window waits for that window's socket.
pub const STARTING_WAIT: Duration = Duration::from_secs(10);

/// How often a waiting launch tries the socket again.
const RETRY: Duration = Duration::from_millis(100);

/// What a launch does.
#[derive(Debug)]
pub enum Launch {
    /// The running copy took the paths: this launch exits.
    Forwarded,
    /// This launch opens the window, holding the app's lock file while it is open when it
    /// could take it.
    Window(Option<File>),
}

impl Launch {
    /// Hands `paths` to the running copy at `socket`, or claims the window through the lock
    /// file `lock`, waiting up to `patience` for another launch that holds it to start
    /// answering.
    #[must_use]
    pub fn claim(
        lock: Option<&Path>,
        socket: &Path,
        paths: &[PathBuf],
        patience: Duration,
    ) -> Self {
        if forward_paths(socket, paths) {
            return Self::Forwarded;
        }
        let Some(lock) = lock else {
            return Self::Window(None);
        };
        match take_lock(lock) {
            Ok(file) => Self::Window(Some(file)),
            Err(TryLockError::WouldBlock) => {
                let start = Instant::now();
                while !paths.is_empty() && start.elapsed() < patience {
                    std::thread::sleep(RETRY);
                    if forward_paths(socket, paths) {
                        return Self::Forwarded;
                    }
                }
                Self::Window(None)
            }
            Err(TryLockError::Error(error)) => {
                tracing::warn!(%error, lock = %lock.display(), "the app's lock file is not held");
                Self::Window(None)
            }
        }
    }
}

/// The lock on the file `lock`, made with its folder if missing.
fn take_lock(lock: &Path) -> Result<File, TryLockError> {
    if let Some(folder) = lock.parent() {
        std::fs::create_dir_all(folder).map_err(TryLockError::Error)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock)
        .map_err(TryLockError::Error)?;
    file.try_lock()?;
    Ok(file)
}

/// Hands `paths` to the copy of the app answering on `socket`, through `paths.open`: true when
/// it took them, or did not answer within [`ANSWER_WAIT`] and so has them waiting. With no
/// paths, no copy answering, or a copy that refused the request, false.
#[must_use]
pub fn forward_paths(socket: &Path, paths: &[PathBuf]) -> bool {
    if paths.is_empty() {
        return false;
    }
    let Ok(mut client) = Client::connect(socket) else {
        return false;
    };
    // The running copy has its own working directory.
    let paths = paths
        .iter()
        .map(|path| std::path::absolute(path).unwrap_or_else(|_| path.clone()))
        .collect();
    let params = match serde_json::to_value(OpenPathsParams { paths }) {
        Ok(params) => params,
        Err(error) => {
            tracing::warn!(%error, "the paths could not be sent");
            return false;
        }
    };
    let (answered, answer) = mpsc::channel();
    std::thread::spawn(move || {
        let _sent = answered.send(client.execute(OPEN_PATHS, params));
    });
    match answer.recv_timeout(ANSWER_WAIT) {
        Ok(Ok(Ok(_))) => true,
        Ok(Ok(Err(error))) => {
            tracing::warn!(%error, "the running app refused the paths; opening a window");
            false
        }
        Ok(Err(error)) => {
            tracing::warn!(%error, "the running app did not answer; opening a window");
            false
        }
        Err(_) => {
            tracing::warn!("the running app is busy; it opens the paths when it can");
            true
        }
    }
}
