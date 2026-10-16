// SPDX-License-Identifier: GPL-3.0-or-later

//! The directory links of ADR-0008's run roots: a symbolic link on macOS and Linux, a
//! junction on Windows, which needs no privileges.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// What [`link_dir`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkOutcome {
    /// There was nothing at the link's path, and the link was made.
    Created,
    /// A link to the target was already there.
    Unchanged,
    /// A link was there but led elsewhere or nowhere (a broken link), and it was replaced.
    Repaired,
}

/// Why [`link_dir`] failed.
#[derive(Debug)]
pub enum LinkError {
    /// The target is not an existing directory, such as an install that has been removed.
    MissingTarget(PathBuf),
    /// A real file or directory, not a link, is at the link's path. It is never removed.
    Occupied(PathBuf),
    /// The host filesystem refused an operation on the path.
    Io {
        /// The path being created, removed or read.
        path: PathBuf,
        /// The host's error.
        error: io::Error,
    },
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTarget(path) => {
                write!(f, "{} is not a directory that exists", path.display())
            }
            Self::Occupied(path) => write!(
                f,
                "{} is a file or folder, not a link, so it was left alone",
                path.display()
            ),
            Self::Io { path, error } => write!(f, "{}: {error}", path.display()),
        }
    }
}

impl std::error::Error for LinkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { error, .. } => Some(error),
            Self::MissingTarget(_) | Self::Occupied(_) => None,
        }
    }
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> LinkError + '_ {
    move |error| LinkError::Io {
        path: path.to_path_buf(),
        error,
    }
}

/// Makes `link` a directory link to `target`, creating `link`'s parent folders, or repairs
/// a link already there that leads elsewhere or is broken.
///
/// On macOS and Linux the link is a symbolic link; on Windows it is a directory junction,
/// which any user may create. Nothing is ever written inside `target`, and a real file or
/// folder at `link` is never removed.
///
/// # Errors
///
/// [`LinkError::MissingTarget`] when `target` is not an existing directory,
/// [`LinkError::Occupied`] when something other than a link is at `link`, and
/// [`LinkError::Io`] when the host refuses to create or remove the link.
pub fn link_dir(target: &Path, link: &Path) -> Result<LinkOutcome, LinkError> {
    let canonical_target = fs::canonicalize(target)
        .ok()
        .filter(|path| path.is_dir())
        .ok_or_else(|| LinkError::MissingTarget(target.to_path_buf()))?;
    let outcome = match fs::symlink_metadata(link) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => LinkOutcome::Created,
        Err(error) => return Err(io_error(link)(error)),
        Ok(metadata) => {
            if !is_link(link, &metadata) {
                return Err(LinkError::Occupied(link.to_path_buf()));
            }
            if fs::canonicalize(link).is_ok_and(|resolved| resolved == canonical_target) {
                return Ok(LinkOutcome::Unchanged);
            }
            remove_link(link).map_err(io_error(link))?;
            LinkOutcome::Repaired
        }
    };
    if let Some(parent) = link
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(io_error(parent))?;
    }
    create_link(&canonical_target, link).map_err(io_error(link))?;
    Ok(outcome)
}

#[cfg(not(windows))]
fn is_link(_link: &Path, metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(not(windows))]
fn remove_link(link: &Path) -> io::Result<()> {
    fs::remove_file(link)
}

#[cfg(not(windows))]
fn create_link(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn is_link(link: &Path, metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink() || junction::exists(link).unwrap_or(false)
}

#[cfg(windows)]
fn remove_link(link: &Path) -> io::Result<()> {
    // A junction or a directory symbolic link is a directory entry; removing it as a
    // directory removes the link, not what it leads to. A file symbolic link is a file.
    fs::remove_dir(link).or_else(|_| fs::remove_file(link))
}

#[cfg(windows)]
fn create_link(target: &Path, link: &Path) -> io::Result<()> {
    junction::create(target, link)
}
