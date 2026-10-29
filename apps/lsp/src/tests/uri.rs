// SPDX-License-Identifier: GPL-3.0-or-later

//! `file` URIs and paths.

use std::path::{Path, PathBuf};

use crate::uri::{from_path, to_path};

// LSP 3.18, "URI": "some clients (such as VS Code) may encode colons in drive letters while
// others do not ... Clients and servers should not assume that each other are encoding the
// same way", so both forms name the one path.
#[test]
fn file_uris_to_paths() {
    assert_eq!(to_path("file:///p/a.qc"), Some(PathBuf::from("/p/a.qc")));
    assert_eq!(
        to_path("FILE://localhost/p/a.qc"),
        Some(PathBuf::from("/p/a.qc"))
    );
    assert_eq!(
        to_path("file:///p/my%20mod/a.qc?x#y"),
        Some(PathBuf::from("/p/my mod/a.qc"))
    );
    assert_eq!(
        to_path("file:///c:/q/a.qc"),
        Some(PathBuf::from("c:/q/a.qc"))
    );
    assert_eq!(
        to_path("file:///C%3A/q/a.qc"),
        Some(PathBuf::from("C:/q/a.qc"))
    );
    assert_eq!(to_path("file:///p/100%"), Some(PathBuf::from("/p/100%")));
    assert_eq!(to_path("file:///p/%zz"), Some(PathBuf::from("/p/%zz")));
}

// LSP 3.18, "URI": a `DocumentUri` "can be parsed as a valid URI"; one of another scheme or host
// names no file of this machine.
#[test]
fn uris_that_name_no_file() {
    assert_eq!(to_path("untitled:Untitled-1"), None);
    assert_eq!(to_path("https://example.com/a.qc"), None);
    assert_eq!(to_path("file://server/share/a.qc"), None);
    assert_eq!(to_path("file://"), None);
    assert_eq!(to_path("file:///p/%FF"), None);
}

// LSP 3.18, "URI": the server writes its URIs one way, and reads them back.
#[test]
fn paths_to_file_uris() {
    assert_eq!(from_path(Path::new("/p/a.qc")), "file:///p/a.qc");
    assert_eq!(
        from_path(Path::new("/p/my mod/#1.qc")),
        "file:///p/my%20mod/%231.qc"
    );
    assert_eq!(from_path(Path::new("c:\\q\\a.qc")), "file:///c:/q/a.qc");
    for path in ["/p/a b/\u{e9}.qc", "/p/x~y_z-1.qc", "c:/q/a.qc"] {
        assert_eq!(
            to_path(&from_path(Path::new(path))),
            Some(PathBuf::from(path))
        );
    }
}
