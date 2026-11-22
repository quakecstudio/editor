// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading a file a person, a command or a project's own text names: a regular file only, so a
//! device such as `/dev/zero`, a FIFO or a directory named by a source's `#include`, a model to
//! import or a map's sidecar is refused at once rather than read without end (PRJ-4.15).

use std::fs::{self, File, Metadata};
use std::io::{self, Read as _};
use std::path::Path;

/// The bytes of the regular file at `path`, links followed to what they name.
///
/// # Errors
///
/// [`io::ErrorKind::IsADirectory`] for a directory, [`io::ErrorKind::InvalidInput`] for
/// anything else that is not a regular file, and the file system's own error when the file
/// cannot be found, opened or read.
pub fn read_regular_file(path: &Path) -> io::Result<Vec<u8>> {
    // The path's own kind first, since opening a FIFO that nothing writes to blocks.
    refuse_irregular(&fs::metadata(path)?)?;
    let mut file = File::open(path)?;
    // Then the file opened, which a path changed in between could make something else.
    refuse_irregular(&file.metadata()?)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// An error for what `metadata` describes, unless it is a regular file.
fn refuse_irregular(metadata: &Metadata) -> io::Result<()> {
    if metadata.is_file() {
        Ok(())
    } else if metadata.is_dir() {
        Err(io::Error::new(
            io::ErrorKind::IsADirectory,
            "is a directory",
        ))
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ))
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use std::path::{Path, PathBuf};

    use super::read_regular_file;

    /// A fresh directory for the test `name`, emptied of an earlier run's files first.
    fn fresh(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qs-paths-read-{name}-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // spec PRJ-4.15: a regular file is read, a directory is refused as a directory, and a
    // missing file is the file system's own error
    #[test]
    fn a_regular_file_is_read_and_a_directory_refused() {
        let dir = fresh("regular");
        std::fs::write(dir.join("a.qc"), "a").unwrap();
        assert_eq!(read_regular_file(&dir.join("a.qc")).unwrap(), b"a");
        let folder = read_regular_file(&dir).unwrap_err();
        assert_eq!(folder.kind(), std::io::ErrorKind::IsADirectory);
        let missing = read_regular_file(&dir.join("nope.qc")).unwrap_err();
        assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // spec PRJ-4.15: a device and a FIFO are refused at once, and a link is judged by what it
    // names: to a regular file it is read, to a device refused, and a loop is an error
    #[cfg(unix)]
    #[test]
    fn devices_fifos_and_links_to_them_are_refused() {
        for device in ["/dev/zero", "/dev/null", "/dev/random"] {
            let error = read_regular_file(Path::new(device)).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{device}");
            assert_eq!(error.to_string(), "not a regular file", "{device}");
        }
        let dir = fresh("links");
        let fifo = dir.join("pipe.qc");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success());
        // Nothing writes to the FIFO: a read would block for ever, a refusal is at once.
        let error = read_regular_file(&fifo).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        std::fs::write(dir.join("b.qc"), "b").unwrap();
        std::os::unix::fs::symlink(dir.join("b.qc"), dir.join("linked.qc")).unwrap();
        std::os::unix::fs::symlink("/dev/zero", dir.join("zero.qc")).unwrap();
        std::os::unix::fs::symlink(dir.join("loop.qc"), dir.join("loop.qc")).unwrap();
        assert_eq!(read_regular_file(&dir.join("linked.qc")).unwrap(), b"b");
        let zero = read_regular_file(&dir.join("zero.qc")).unwrap_err();
        assert_eq!(zero.kind(), std::io::ErrorKind::InvalidInput);
        assert!(read_regular_file(&dir.join("loop.qc")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // spec PRJ-4.15: Windows' NUL device is never read without end: it is refused, or gives
    // nothing at once
    #[cfg(windows)]
    #[test]
    fn the_nul_device_ends() {
        assert!(read_regular_file(Path::new("NUL")).map_or(true, |bytes| bytes.is_empty()));
    }
}
