// SPDX-License-Identifier: GPL-3.0-or-later

//! `file` URIs and paths, by the rules of each host.

use std::path::{Path, PathBuf};

use crate::uri::{Host, from_path, from_path_on, to_path, to_path_on};

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
    assert_eq!(to_path("file:///p/100%"), Some(PathBuf::from("/p/100%")));
    assert_eq!(to_path("file:///p/%zz"), Some(PathBuf::from("/p/%zz")));
    let windows = |uri| to_path_on(uri, Host::Windows);
    assert_eq!(
        windows("file:///c:/q/a.qc"),
        Some(PathBuf::from("c:/q/a.qc"))
    );
    assert_eq!(
        windows("file:///C%3A/q/a.qc"),
        Some(PathBuf::from("C:/q/a.qc"))
    );
}

// P2-161: a drive letter is Windows's alone; on any other host `/c:/x` is the absolute path it
// spells, and `\` a byte of a file's name that the URI writes encoded and reads back.
#[test]
fn drive_letters_and_backslashes_are_windows_rules() {
    let unix = |uri| to_path_on(uri, Host::Unix);
    assert_eq!(unix("file:///c:/q/a.qc"), Some(PathBuf::from("/c:/q/a.qc")));
    assert_eq!(
        unix("file:///C%3A/q/a.qc"),
        Some(PathBuf::from("/C:/q/a.qc"))
    );
    assert_eq!(
        from_path_on(Path::new("/p/a\\b.qc"), Host::Unix),
        "file:///p/a%5Cb.qc"
    );
    assert_eq!(
        unix("file:///p/a%5Cb.qc"),
        Some(PathBuf::from("/p/a\\b.qc"))
    );
    assert_eq!(
        from_path_on(Path::new("/c:/q/a.qc"), Host::Unix),
        "file:///c%3A/q/a.qc"
    );
}

// LSP 3.18, "URI": a `DocumentUri` "can be parsed as a valid URI"; one of another scheme or host
// names no file of this machine.
#[test]
fn uris_that_name_no_file() {
    assert_eq!(to_path("untitled:Untitled-1"), None);
    assert_eq!(to_path("https://example.com/a.qc"), None);
    assert_eq!(to_path_on("file://server/share/a.qc", Host::Unix), None);
    assert_eq!(to_path("file://"), None);
    assert_eq!(to_path("file:///p/%FF"), None);
}

// LSP 3.18, "URI": the server writes its URIs one way, and reads them back, by its host's rules.
#[test]
fn paths_to_file_uris() {
    assert_eq!(from_path(Path::new("/p/a.qc")), "file:///p/a.qc");
    assert_eq!(
        from_path(Path::new("/p/my mod/#1.qc")),
        "file:///p/my%20mod/%231.qc"
    );
    assert_eq!(
        from_path_on(Path::new("c:\\q\\a.qc"), Host::Windows),
        "file:///c:/q/a.qc"
    );
    for path in ["/p/a b/\u{e9}.qc", "/p/x~y_z-1.qc"] {
        assert_eq!(
            to_path(&from_path(Path::new(path))),
            Some(PathBuf::from(path))
        );
    }
    let written = from_path_on(Path::new("c:/q/a.qc"), Host::Windows);
    assert_eq!(
        to_path_on(&written, Host::Windows),
        Some(PathBuf::from("c:/q/a.qc"))
    );
}

// P3-37: on a Windows host a `file` URI's host other than `localhost` is a UNC share, as
// vscode-uri's `uriToFsPath` reads one ("unc path: file://shares/c$/far/boo") and RFC 8089
// Appendix E.3.1 maps one; a UNC path is written with its host as the URI's, the one string the
// client opens the file by.
#[test]
fn a_uri_host_is_a_unc_share_on_windows() {
    let windows = |uri| to_path_on(uri, Host::Windows);
    assert_eq!(
        windows("file://server/share/x.qc"),
        Some(PathBuf::from("\\\\server\\share\\x.qc"))
    );
    assert_eq!(
        windows("file://shares/c%24/far/boo"),
        Some(PathBuf::from("\\\\shares\\c$\\far\\boo"))
    );
    assert_eq!(
        windows("file://LOCALHOST/c:/q/a.qc"),
        Some(PathBuf::from("c:/q/a.qc"))
    );
    assert_eq!(
        from_path_on(Path::new("\\\\server\\share\\x.qc"), Host::Windows),
        "file://server/share/x.qc"
    );
    assert_eq!(
        from_path_on(
            Path::new("//host.example.com/Share/path/to/file.txt"),
            Host::Windows
        ),
        "file://host.example.com/Share/path/to/file.txt"
    );
    assert_eq!(
        from_path_on(Path::new("\\\\shares\\c$\\far boo"), Host::Windows),
        "file://shares/c%24/far%20boo"
    );
    // A host with one root writes `//x` as the path it spells.
    assert_eq!(
        from_path_on(Path::new("//server/share"), Host::Unix),
        "file:////server/share"
    );
}

// P3-37: the edges of a share. vscode-uri writes the host in lower case (`authority =
// authority.toLowerCase();`) and reads a host as a share only `if (uri.authority &&
// uri.path.length > 1 && uri.scheme === 'file')`; a verbatim path names its share or its drive,
// and a device path, an empty host, a host that decodes to a separator, and a query or a
// fragment before the path name no share.
#[test]
fn the_edges_of_a_unc_share() {
    let windows = |uri| to_path_on(uri, Host::Windows);
    let write = |path| from_path_on(Path::new(path), Host::Windows);
    assert_eq!(write("\\\\FileSrv\\Mods\\x.qc"), "file://filesrv/Mods/x.qc");
    assert_eq!(windows("file://server"), None);
    assert_eq!(windows("file://server/"), None);
    assert_eq!(windows("file://server/?x"), None);
    assert_eq!(windows("file://a%2Fb/x.qc"), None);
    assert_eq!(windows("file://a%5Cb/x.qc"), None);
    assert_eq!(windows("file://srv#f/x.qc"), None);
    assert_eq!(windows("file://%FF/x.qc"), None);
    assert_eq!(
        windows("file://srv/s/x.qc?q#f"),
        Some(PathBuf::from("\\\\srv\\s\\x.qc"))
    );
    assert_eq!(
        write("\\\\?\\UNC\\srv\\share\\x.qc"),
        "file://srv/share/x.qc"
    );
    assert_eq!(write("\\\\?\\C:\\q\\a.qc"), "file:///C:/q/a.qc");
    assert_eq!(write("\\\\.\\pipe\\x"), "file:////./pipe/x");
    assert_eq!(write("\\\\"), "file:////");
    assert_eq!(write("//server"), "file://server/");
}
