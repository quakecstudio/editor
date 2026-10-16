// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::PathBuf;

use qs_pak::PakError;
use qs_wad::WadError;

use crate::fixtures::{MemFs, pak, pak_with_extra, pop_lmp, unmodified_pak, wad};
use crate::{
    Element, FS_GAMENAME, FS_POP_PATH, FS_POP_SIZE, FS_POP_TABLE, FileRef, Location, Vfs, VfsError,
};

/// No command-line arguments.
const NO_ARGS: &[&str] = &[];

/// The tree of the spec's FS-1 vectors under `base`, plus a loose `gfx/pop.lmp` so the game
/// is registered and the vectors' archives may be modified.
fn spec_tree(base: &str) -> MemFs {
    let at = |path: &str| format!("{base}/{path}");
    MemFs::default()
        .file(
            &at("id1/pak0.pak"),
            &pak(&[
                ("a.txt", "id1-pak0"),
                ("progs.dat", "pak0-progs"),
                ("f.txt", "pak0-f"),
            ]),
        )
        .file(&at("id1/pak1.pak"), &pak(&[("a.txt", "id1-pak1")]))
        .file(&at("id1/pak3.pak"), &pak(&[("d.txt", "id1-pak3")]))
        .file(&at("id1/a.txt"), b"id1-loose")
        .file(&at("id1/b.txt"), b"id1-loose-b")
        .file(&at("id1/sub/c.txt"), b"id1-loose-c")
        .file(&at("id1/gfx/pop.lmp"), &pop_lmp(&[]))
        .file(&at("mod/pak0.pak"), &pak(&[("e.txt", "mod-pak0")]))
        .file(&at("mod/a.txt"), b"mod-loose")
        .file(&at("mod/progs.dat"), b"mod-progs")
        .file(&at("rogue/pak0.pak"), &pak(&[("f.txt", "rogue-f")]))
        .file(&at("hipnotic/pak0.pak"), &pak(&[("f.txt", "hipnotic-f")]))
}

fn spec_vfs(args: &[&str]) -> Vfs<MemFs> {
    Vfs::new(spec_tree("."), args, ".").unwrap()
}

/// The names of the search path's elements, head first.
fn path_names<H: crate::HostFs>(vfs: &Vfs<H>) -> Vec<&str> {
    vfs.elements().iter().map(Element::name).collect()
}

/// The contents of `name` as text, or `None` when it is not found.
fn lookup<H: crate::HostFs>(vfs: &Vfs<H>, name: &str) -> Option<String> {
    vfs.load(name.as_bytes())
        .unwrap()
        .map(|file| String::from_utf8(file.into_bytes()).unwrap())
}

// spec FS-1.4: a game directory's archives come before it, highest number first.
// spec FS-3.1: the first element that has a name wins.
// spec FS-3.2: an archive element has a name when an entry matches it.
#[test]
fn no_options() {
    let vfs = spec_vfs(&[]);
    assert_eq!(
        path_names(&vfs),
        ["./id1/pak1.pak", "./id1/pak0.pak", "./id1"]
    );
    assert_eq!(lookup(&vfs, "a.txt").as_deref(), Some("id1-pak1"));
    assert_eq!(lookup(&vfs, "f.txt").as_deref(), Some("pak0-f"));
    assert_eq!(lookup(&vfs, "b.txt").as_deref(), Some("id1-loose-b"));
    assert_eq!(FS_GAMENAME, "id1");
}

// spec FS-1.5: the archive scan stops at the first missing number; pak3 is never loaded.
#[test]
fn archive_scan_stops_at_a_gap() {
    let vfs = spec_vfs(&[]);
    assert_eq!(lookup(&vfs, "d.txt"), None);
    assert!(!path_names(&vfs).contains(&"./id1/pak3.pak"));
}

// spec FS-1.5: with no pak0.pak a game directory has only its loose files, and a missing
// game directory is not an error.
#[test]
fn missing_archives_and_directories_are_not_errors() {
    let vfs = spec_vfs(&["-game", "nothere"]);
    assert_eq!(
        path_names(&vfs),
        ["./nothere", "./id1/pak1.pak", "./id1/pak0.pak", "./id1"]
    );
    assert_eq!(lookup(&vfs, "a.txt").as_deref(), Some("id1-pak1"));
    let loose_only = MemFs::default().file("q/id1/x.txt", b"x");
    let vfs = Vfs::new(loose_only, NO_ARGS, "q").unwrap();
    assert_eq!(path_names(&vfs), ["q/id1"]);
    assert_eq!(lookup(&vfs, "x.txt").as_deref(), Some("x"));
}

// spec PAK-1.4: an archive that cannot be opened is not an error; the scan stops there.
#[test]
fn unopenable_archive_ends_the_scan() {
    let tree = spec_tree(".").unreadable("./id1/pak1.pak");
    let vfs = Vfs::new(tree, NO_ARGS, ".").unwrap();
    assert_eq!(path_names(&vfs), ["./id1/pak0.pak", "./id1"]);
    assert_eq!(lookup(&vfs, "a.txt").as_deref(), Some("id1-pak0"));
}

// spec FS-1.6: an archive that opens but fails to load is fatal.
#[test]
fn bad_archive_is_fatal() {
    let tree = spec_tree(".").file("./mod/pak0.pak", b"pack0000000000000");
    let error = Vfs::new(tree, &["-game", "mod"], ".").unwrap_err();
    assert_eq!(
        error,
        VfsError::Archive {
            path: "./mod/pak0.pak".to_owned(),
            error: PakError::NotAPackfile
        }
    );
    assert_eq!(error.to_string(), "./mod/pak0.pak is not a packfile");
}

// spec FS-1.7: -game adds its directory in front of id1.
// spec FS-1.9: the game directory is the last one added.
#[test]
fn game_option() {
    let vfs = spec_vfs(&["-game", "mod"]);
    assert_eq!(
        path_names(&vfs),
        [
            "./mod/pak0.pak",
            "./mod",
            "./id1/pak1.pak",
            "./id1/pak0.pak",
            "./id1"
        ]
    );
    assert_eq!(vfs.game_dir(), "./mod");
    assert_eq!(lookup(&vfs, "a.txt").as_deref(), Some("mod-loose"));
    assert_eq!(lookup(&vfs, "e.txt").as_deref(), Some("mod-pak0"));
    assert_eq!(lookup(&vfs, "progs.dat").as_deref(), Some("mod-progs"));
    assert_eq!(spec_vfs(&[]).game_dir(), "./id1");
}

// spec FS-1.7: rogue, then hipnotic, then -game, whatever the order on the command line.
#[test]
fn mission_pack_order() {
    for args in [
        ["-hipnotic", "-rogue", "-game", "mod"],
        ["-game", "mod", "-rogue", "-hipnotic"],
    ] {
        let vfs = spec_vfs(&args);
        assert_eq!(
            path_names(&vfs),
            [
                "./mod/pak0.pak",
                "./mod",
                "./hipnotic/pak0.pak",
                "./hipnotic",
                "./rogue/pak0.pak",
                "./rogue",
                "./id1/pak1.pak",
                "./id1/pak0.pak",
                "./id1"
            ]
        );
        assert_eq!(lookup(&vfs, "f.txt").as_deref(), Some("hipnotic-f"));
        assert_eq!(vfs.game_dir(), "./mod");
    }
    let vfs = spec_vfs(&["-rogue"]);
    assert_eq!(lookup(&vfs, "f.txt").as_deref(), Some("rogue-f"));
    assert_eq!(vfs.game_dir(), "./rogue");
}

// spec FS-1.1: options match exactly and case-sensitively, the first occurrence wins, and an
// option with a value is ignored when it is the last argument.
#[test]
fn option_matching() {
    let vfs = spec_vfs(&["-game"]);
    assert_eq!(path_names(&vfs).len(), 3);
    assert_eq!(lookup(&vfs, "a.txt").as_deref(), Some("id1-pak1"));
    let vfs = spec_vfs(&["-GAME", "mod", "-Rogue", "--game", "mod"]);
    assert_eq!(path_names(&vfs).len(), 3);
    let vfs = spec_vfs(&["-game", "mod", "-game", "rogue"]);
    assert_eq!(vfs.game_dir(), "./mod");
    let vfs = spec_vfs(&["-basedir"]);
    assert_eq!(path_names(&vfs)[2], "./id1");
}

// spec FS-1.2: the base directory is -basedir's value, else the host's default.
#[test]
fn base_directory() {
    let tree = spec_tree("quake");
    let vfs = Vfs::new(tree, NO_ARGS, "quake").unwrap();
    assert_eq!(path_names(&vfs)[2], "quake/id1");
    let tree = spec_tree("other");
    let vfs = Vfs::new(tree, &["-basedir", "other"], "quake").unwrap();
    assert_eq!(path_names(&vfs)[2], "other/id1");
    assert_eq!(lookup(&vfs, "a.txt").as_deref(), Some("id1-pak1"));
}

// spec FS-1.3: exactly one trailing separator is removed; an empty base gives `/id1`.
#[test]
fn base_directory_trailing_separator() {
    let vfs = Vfs::new(spec_tree("../base"), &["-basedir", "..//base//"], ".").unwrap();
    assert_eq!(path_names(&vfs)[0], "..//base//id1/pak1.pak");
    assert_eq!(lookup(&vfs, "a.txt").as_deref(), Some("id1-pak1"));
    let vfs = Vfs::new(spec_tree("../base"), &["-basedir", "../base\\"], ".").unwrap();
    assert_eq!(path_names(&vfs)[2], "../base/id1");
    let vfs = Vfs::new(spec_tree("/"), &["-basedir", ""], ".").unwrap();
    assert_eq!(path_names(&vfs), ["/id1/pak1.pak", "/id1/pak0.pak", "/id1"]);
    assert_eq!(lookup(&vfs, "b.txt").as_deref(), Some("id1-loose-b"));
}

// spec FS-1.8: -game's value is used as given, may contain `/` and `..`, marks the game as
// modified, and `-game id1` changes no lookup.
#[test]
fn game_value_as_given() {
    let tree = spec_tree("q/base").file("q/elsewhere/z.txt", b"z");
    let vfs = Vfs::new(tree, &["-game", "../elsewhere"], "q/base").unwrap();
    assert_eq!(vfs.game_dir(), "q/base/../elsewhere");
    assert_eq!(lookup(&vfs, "z.txt").as_deref(), Some("z"));
    assert!(vfs.is_modified());
    let vfs = spec_vfs(&["-game", "mod/.."]);
    assert_eq!(vfs.game_dir(), "./mod/..");
    assert_eq!(lookup(&vfs, "id1/a.txt").as_deref(), Some("id1-loose"));
    let vfs = spec_vfs(&["-game", "id1"]);
    assert_eq!(path_names(&vfs).len(), 6);
    assert_eq!(lookup(&vfs, "a.txt").as_deref(), Some("id1-pak1"));
    assert_eq!(lookup(&vfs, "b.txt").as_deref(), Some("id1-loose-b"));
}

// spec FS-2.1: -path replaces the search path with its arguments, last first, up to the end or
// an argument starting with `+` or `-`; the game directory is unchanged and the game modified.
// spec FS-2.2: a `pak` argument is loaded as an archive; anything else is a directory.
#[test]
fn path_option() {
    let vfs = spec_vfs(&["-path", "id1", "mod/pak0.pak"]);
    assert_eq!(path_names(&vfs), ["mod/pak0.pak", "id1"]);
    assert_eq!(vfs.game_dir(), "./id1");
    assert!(vfs.is_modified());
    assert_eq!(lookup(&vfs, "a.txt").as_deref(), Some("id1-loose"));
    assert_eq!(lookup(&vfs, "e.txt").as_deref(), Some("mod-pak0"));
    assert_eq!(lookup(&vfs, "f.txt"), None);
    let vfs = spec_vfs(&["-path", "id1", "+map", "start", "mod"]);
    assert_eq!(path_names(&vfs), ["id1"]);
    assert_eq!(lookup(&vfs, "a.txt").as_deref(), Some("id1-loose"));
    let vfs = spec_vfs(&["-path", "id1", "-game", "mod"]);
    assert_eq!(path_names(&vfs), ["id1"]);
    assert_eq!(vfs.game_dir(), "./mod");
}

// spec FS-2.1: -path with no arguments leaves the search path empty.
#[test]
fn empty_path_option() {
    let tree = MemFs::default().file("id1/gfx/pop.lmp", &pop_lmp(&[]));
    let error = Vfs::new(tree, &["-path"], ".").unwrap_err();
    // Nothing is on the path, so pop.lmp is not found and the modified game is shareware.
    assert_eq!(error, VfsError::ModifiedShareware);
}

// spec FS-2.2: a `-path` archive that cannot be opened is `Couldn't load packfile: <arg>`;
// a directory is not checked.
#[test]
fn path_option_missing_archive() {
    let error = Vfs::new(spec_tree("."), &["-path", "nope.pak"], ".").unwrap_err();
    assert_eq!(
        error,
        VfsError::CouldntLoadPackfile {
            path: "nope.pak".to_owned()
        }
    );
    assert_eq!(error.to_string(), "Couldn't load packfile: nope.pak");
    let vfs = spec_vfs(&["-path", "id1", "nodir"]);
    assert_eq!(path_names(&vfs), ["nodir", "id1"]);
}

// spec FS-2.3: the extension is the text after the first `.`, at most 7 bytes, compared
// case-sensitively.
#[test]
fn path_option_extension() {
    for arg in ["./id1/pak0.pak", "id1/PAK1.PAK"] {
        let vfs = spec_vfs(&["-path", "id1", arg]);
        assert!(matches!(vfs.elements()[0], Element::Directory(_)), "{arg}");
        assert_eq!(lookup(&vfs, "a.txt").as_deref(), Some("id1-loose"), "{arg}");
    }
    assert_eq!(crate::com_file_extension("a.pakpakpak"), b"pakpakp");
    assert_eq!(crate::com_file_extension("noext"), b"");
    assert_eq!(crate::com_file_extension("a.b.pak"), b"b.pak");
    let vfs = spec_vfs(&["-path", "id1", "id1/pak1.pak"]);
    assert!(matches!(vfs.elements()[0], Element::Archive(_)));
    assert_eq!(lookup(&vfs, "a.txt").as_deref(), Some("id1-pak1"));
}

// spec FS-2.4: with -proghack, `progs.dat` is looked up from the second element.
#[test]
fn proghack() {
    let vfs = spec_vfs(&["-game", "mod", "-proghack"]);
    assert_eq!(lookup(&vfs, "progs.dat").as_deref(), Some("mod-progs"));
    let tree = spec_tree(".").file("./mod/pak0.pak", &pak(&[("progs.dat", "mod-pak-progs")]));
    let plain = Vfs::new(tree, &["-game", "mod"], ".").unwrap();
    assert_eq!(
        lookup(&plain, "progs.dat").as_deref(),
        Some("mod-pak-progs")
    );
    let tree = spec_tree(".").file("./mod/pak0.pak", &pak(&[("progs.dat", "mod-pak-progs")]));
    let hacked = Vfs::new(tree, &["-game", "mod", "-proghack"], ".").unwrap();
    assert_eq!(lookup(&hacked, "progs.dat").as_deref(), Some("mod-progs"));
    assert_eq!(lookup(&hacked, "./progs.dat").as_deref(), Some("mod-progs"));
    let tree = spec_tree(".").file("./mod/pak0.pak", &pak(&[("x.dat", "pak-x")]));
    let hacked = Vfs::new(tree, &["-game", "mod", "-proghack"], ".").unwrap();
    assert_eq!(lookup(&hacked, "x.dat").as_deref(), Some("pak-x"));
}

// spec FS-2.5: -cachedir is not implemented and changes nothing.
#[test]
fn cachedir_is_ignored() {
    let plain = spec_vfs(&[]);
    let cached = spec_vfs(&["-cachedir", "/tmp/qcache"]);
    assert_eq!(path_names(&plain), path_names(&cached));
    assert_eq!(lookup(&cached, "b.txt").as_deref(), Some("id1-loose-b"));
}

// spec FS-3.3: a directory element has `<dir>/<name>` if it exists and can be opened.
// spec FS-3.6: the size is the entry's or the file's length; one NUL follows, not counted.
#[test]
fn loose_files_and_sizes() {
    let vfs = spec_vfs(&[]);
    let file = vfs.load(b"sub/c.txt").unwrap().unwrap();
    assert_eq!(file.bytes(), b"id1-loose-c");
    assert_eq!(file.size(), 11);
    assert_eq!(file.bytes_with_nul(), b"id1-loose-c\0");
    let file = vfs.load(b"b.txt").unwrap().unwrap();
    assert_eq!(file.size(), 11);
    let file = vfs.load(b"a.txt").unwrap().unwrap();
    assert_eq!(file.size(), 8);
    assert_eq!(file.bytes_with_nul().len(), 9);
    assert_eq!(lookup(&vfs, "sub"), None);
    assert_eq!(lookup(&vfs, "sub//c.txt").as_deref(), Some("id1-loose-c"));
    let tree = spec_tree(".").unreadable("./id1/b.txt");
    let vfs = Vfs::new(tree, NO_ARGS, ".").unwrap();
    let error = vfs.load(b"b.txt").unwrap_err();
    assert!(matches!(error, VfsError::Io { .. }), "{error}");
}

// spec FS-3.5: a name found nowhere is not an error.
#[test]
fn not_found_is_none() {
    let vfs = spec_vfs(&[]);
    assert!(vfs.find(b"e.txt").is_none());
    assert_eq!(vfs.load(b"e.txt").unwrap(), None);
    assert_eq!(vfs.load(b"").unwrap(), None);
    for name in [".", "sub/.", "./", "sub/"] {
        assert_eq!(vfs.load(name.as_bytes()).unwrap(), None, "{name}");
    }
}

// spec FS-3.7: directory, archive and game directory names match ASCII case-insensitively;
// archive entries do not.
#[test]
fn case_insensitive_host_names() {
    let tree = MemFs::default()
        .file("Q/ID1/PAK0.PAK", &pak(&[("Maps/E1M1.bsp", "map")]))
        .file("Q/ID1/GFX/Pop.Lmp", &pop_lmp(&[]))
        .file("Q/ID1/Progs/Player.MDL", b"mdl")
        .file("Q/Mod/progs.dat", b"mod")
        .file("Q/Mod/PROGS.DAT", b"upper");
    let vfs = Vfs::new(tree, &["-game", "mod"], "Q").unwrap();
    assert_eq!(path_names(&vfs), ["Q/mod", "Q/id1/pak0.pak", "Q/id1"]);
    assert!(vfs.is_registered());
    assert_eq!(lookup(&vfs, "progs/player.mdl").as_deref(), Some("mdl"));
    assert_eq!(lookup(&vfs, "PROGS/PLAYER.MDL").as_deref(), Some("mdl"));
    assert_eq!(lookup(&vfs, "progs.dat").as_deref(), Some("mod"));
    assert_eq!(lookup(&vfs, "PROGS.DAT").as_deref(), Some("upper"));
    assert_eq!(lookup(&vfs, "Progs.Dat").as_deref(), Some("upper"));
    assert_eq!(lookup(&vfs, "Maps/E1M1.bsp").as_deref(), Some("map"));
    assert_eq!(lookup(&vfs, "maps/e1m1.bsp"), None);
}

// spec FS-3.8: an absolute name or one with a `..` component never matches a directory
// element, but still matches an archive entry of exactly that name.
#[test]
fn names_are_confined_to_the_install() {
    let tree = spec_tree("q/base")
        .file("q/secret.txt", b"secret")
        .file("/etc/passwd", b"root")
        .file("q/base/id1/etc/passwd", b"inside")
        .file("q/base/id1/pak0.pak", &pak(&[("../x.txt", "in-pak")]));
    let vfs = Vfs::new(tree, NO_ARGS, "q/base").unwrap();
    for name in [
        "../../secret.txt",
        "sub/../../../secret.txt",
        "..\\..\\secret.txt",
        "/etc/passwd",
        "\\etc\\passwd",
        "C:/etc/passwd",
    ] {
        assert_eq!(lookup(&vfs, name), None, "{name}");
    }
    assert_eq!(lookup(&vfs, "../x.txt").as_deref(), Some("in-pak"));
    assert_eq!(lookup(&vfs, "etc/passwd").as_deref(), Some("inside"));
    assert_eq!(lookup(&vfs, "sub/./c.txt").as_deref(), Some("id1-loose-c"));
    assert_eq!(lookup(&vfs, "..x/c.txt"), None);
}

// spec FS-3.9: no limit on path lengths.
#[test]
fn long_paths() {
    let long = "d".repeat(300);
    let name = format!("{long}/{long}.txt");
    let tree = spec_tree(&long).file(&format!("{long}/id1/{name}"), b"deep");
    let vfs = Vfs::new(tree, NO_ARGS, &long).unwrap();
    assert!(vfs.elements()[0].name().len() > 128);
    assert_eq!(lookup(&vfs, &name).as_deref(), Some("deep"));
    assert_eq!(lookup(&vfs, "a.txt").as_deref(), Some("id1-pak1"));
}

// spec FS-3.10: no limit on the number of archives.
#[test]
fn many_archives() {
    let mut tree = MemFs::default().file("id1/gfx/pop.lmp", &pop_lmp(&[]));
    for n in 0..20 {
        let data = format!("pak{n}");
        tree = tree.file(&format!("id1/pak{n}.pak"), &pak(&[("n.txt", &data)]));
    }
    let vfs = Vfs::new(tree, NO_ARGS, ".").unwrap();
    assert_eq!(vfs.elements().len(), 21);
    assert_eq!(vfs.elements()[0].name(), "./id1/pak19.pak");
    assert_eq!(lookup(&vfs, "n.txt").as_deref(), Some("pak19"));
}

// spec FS-4.1: the registration check finds gfx/pop.lmp as a registered game would, so a
// loose id1/gfx/pop.lmp counts.
// spec FS-4.2: a pop.lmp whose first 256 bytes hold id's table makes the game registered.
#[test]
fn registered_by_loose_pop() {
    assert_eq!(FS_POP_PATH, "gfx/pop.lmp");
    assert_eq!(FS_POP_SIZE, 256);
    assert_eq!(FS_POP_TABLE.len() * 2, FS_POP_SIZE);
    assert_eq!(FS_POP_TABLE[10], 0x6600);
    assert_eq!(FS_POP_TABLE[127], 0x0000);
    assert_eq!(FS_POP_TABLE[124], 0x6400);
    let vfs = spec_vfs(&[]);
    assert!(vfs.is_registered());
    let tree = MemFs::default().file("id1/gfx/pop.lmp", &pop_lmp(b"trailing bytes"));
    let vfs = Vfs::new(tree, NO_ARGS, ".").unwrap();
    assert!(vfs.is_registered());
    assert!(!vfs.is_modified());
}

// spec FS-4.2: no pop.lmp is shareware; a wrong or short one is `Corrupted data file.`.
#[test]
fn pop_check() {
    let tree = MemFs::default().file("id1/x.txt", b"x");
    let vfs = Vfs::new(tree, NO_ARGS, ".").unwrap();
    assert!(!vfs.is_registered());
    let mut wrong = pop_lmp(&[]);
    wrong[21] ^= 1;
    for bad in [wrong, pop_lmp(&[])[..255].to_vec(), Vec::new()] {
        let tree = MemFs::default().file("id1/gfx/pop.lmp", &bad);
        let error = Vfs::new(tree, NO_ARGS, ".").unwrap_err();
        assert_eq!(error, VfsError::CorruptedDataFile);
        assert_eq!(error.to_string(), "Corrupted data file.");
    }
    let tree = MemFs::default().file("id1/pak0.pak", &pak(&[("gfx/pop.lmp", "short")]));
    assert_eq!(
        Vfs::new(tree, NO_ARGS, ".").unwrap_err(),
        VfsError::CorruptedDataFile
    );
}

// spec FS-4.3: a shareware game marked as modified, by an archive, -game or -path, is fatal.
#[test]
fn modified_shareware_is_fatal() {
    let message = "You must have the registered version to use modified games";
    let by_archive = MemFs::default().file("id1/pak0.pak", &pak(&[("x", "y")]));
    let error = Vfs::new(by_archive, NO_ARGS, ".").unwrap_err();
    assert_eq!(error, VfsError::ModifiedShareware);
    assert_eq!(error.to_string(), message);
    let loose = || MemFs::default().file("id1/x.txt", b"x").dir("mod");
    assert_eq!(
        Vfs::new(loose(), &["-game", "mod"], ".").unwrap_err(),
        VfsError::ModifiedShareware
    );
    assert_eq!(
        Vfs::new(loose(), &["-path", "id1"], ".").unwrap_err(),
        VfsError::ModifiedShareware
    );
    let unmodified = MemFs::default().file("id1/pak0.pak", &unmodified_pak(&[("x", "y")]));
    let vfs = Vfs::new(unmodified, NO_ARGS, ".").unwrap();
    assert!(!vfs.is_modified());
    assert!(!vfs.is_registered());
}

// spec FS-3.4: in shareware, directory elements are skipped for names with `/` or `\`;
// archives are not.
// spec FS-4.4: the restriction applies to every lookup after the check in a shareware game.
#[test]
fn shareware_restricts_loose_subdirectories() {
    let tree = MemFs::default()
        .file(
            "id1/pak0.pak",
            &unmodified_pak(&[("maps/start.bsp", "pak-start")]),
        )
        .file("id1/progs.dat", b"loose-progs")
        .file("id1/maps/e1m1.bsp", b"loose-e1m1")
        .file("id1/maps/start.bsp", b"loose-start")
        .file("id1/back\\slash.txt", b"bs");
    let vfs = Vfs::new(tree, NO_ARGS, ".").unwrap();
    assert!(!vfs.is_registered());
    assert_eq!(lookup(&vfs, "progs.dat").as_deref(), Some("loose-progs"));
    assert_eq!(lookup(&vfs, "maps/e1m1.bsp"), None);
    assert_eq!(lookup(&vfs, "maps\\e1m1.bsp"), None);
    assert_eq!(lookup(&vfs, "back\\slash.txt"), None);
    assert_eq!(lookup(&vfs, "maps/start.bsp").as_deref(), Some("pak-start"));
    let tree = spec_tree(".").file("./id1/back\\slash.txt", b"bs");
    let registered = Vfs::new(tree, NO_ARGS, ".").unwrap();
    assert_eq!(
        lookup(&registered, "sub/c.txt").as_deref(),
        Some("id1-loose-c")
    );
    assert_eq!(
        lookup(&registered, "back\\slash.txt").as_deref(),
        Some("bs")
    );
}

/// A rerelease install as FS-5 describes it, under `rerelease`.
fn rerelease_tree() -> MemFs {
    MemFs::default()
        .file(
            "rerelease/id1/pak0.pak",
            &pak(&[("maps/start.bsp", "rr-start")]),
        )
        .file("rerelease/id1/gfx/pop.lmp", &pop_lmp(&[]))
        .file("rerelease/mg1/pak0.pak", &pak(&[("maps/mg1.bsp", "mg1")]))
        .file(
            "rerelease/hipnotic/pak0.pak",
            &pak(&[("maps/hip1m1.bsp", "hip")]),
        )
        .file(
            "rerelease/QuakeEX.kpf",
            b"PK\x03\x04localization/loc_english.txt",
        )
        .file("id1/pak0.pak", &pak(&[("maps/start.bsp", "original")]))
}

// spec FS-5.1: a rerelease install is its own base directory with only pak0.pak in id1.
// spec FS-5.2: its game directories are searched by FS-1 and FS-3 with -game.
#[test]
fn rerelease_layout() {
    let vfs = Vfs::new(rerelease_tree(), &["-game", "mg1"], "rerelease").unwrap();
    assert_eq!(
        path_names(&vfs),
        [
            "rerelease/mg1/pak0.pak",
            "rerelease/mg1",
            "rerelease/id1/pak0.pak",
            "rerelease/id1"
        ]
    );
    assert_eq!(lookup(&vfs, "maps/mg1.bsp").as_deref(), Some("mg1"));
    assert_eq!(lookup(&vfs, "maps/start.bsp").as_deref(), Some("rr-start"));
    let vfs = Vfs::new(rerelease_tree(), &["-hipnotic"], "rerelease").unwrap();
    assert_eq!(lookup(&vfs, "maps/hip1m1.bsp").as_deref(), Some("hip"));
}

// spec FS-5.3: QuakeEX.kpf is not on the search path.
// spec FS-5.4: the search path is the engine's, with no per-user game directories.
#[test]
fn rerelease_kpf_is_not_searched() {
    let vfs = Vfs::new(rerelease_tree(), NO_ARGS, "rerelease").unwrap();
    assert_eq!(lookup(&vfs, "QuakeEX.kpf"), None);
    assert_eq!(lookup(&vfs, "localization/loc_english.txt"), None);
    assert_eq!(lookup(&vfs, "../QuakeEX.kpf"), None);
    assert_eq!(
        path_names(&vfs),
        ["rerelease/id1/pak0.pak", "rerelease/id1"]
    );
}

// spec WAD-1.4: gfx.wad is found through the search path, so a mod can replace it; missing,
// it is `W_LoadWadFile: couldn't load gfx.wad`.
#[test]
fn gfx_wad_through_the_search_path() {
    let tree = spec_tree(".")
        .file("./id1/gfx.wad", &wad(b"CONCHARS"))
        .file("./mod/gfx.wad", &wad(b"MODCHARS"))
        .file("./bad/gfx.wad", b"WAD3");
    let tree_vfs = Vfs::new(tree, &["-game", "mod"], ".").unwrap();
    let bytes = tree_vfs.load_wad("gfx.wad").unwrap();
    let loaded = qs_wad::Wad::read(&bytes).unwrap();
    assert!(loaded.find(b"modchars").is_ok());
    let error = spec_vfs(&[]).load_wad("gfx.wad").unwrap_err();
    assert_eq!(error.to_string(), "W_LoadWadFile: couldn't load gfx.wad");
    let tree = spec_tree(".").file("./bad/gfx.wad", b"WAD3");
    let bad = Vfs::new(tree, &["-game", "bad"], ".").unwrap();
    let error = bad.load_wad("gfx.wad").unwrap_err();
    assert_eq!(
        error,
        VfsError::Wad {
            name: "gfx.wad".to_owned(),
            error: WadError::WrongId
        }
    );
    assert_eq!(error.to_string(), "Wad file gfx.wad doesn't have WAD2 id");
}

#[test]
fn files_lists_every_element() {
    let vfs = spec_vfs(&["-game", "mod"]);
    let files = vfs.files();
    let names: Vec<(usize, String)> = files
        .iter()
        .map(|file| (file.element, String::from_utf8(file.name.clone()).unwrap()))
        .collect();
    let expect = [
        (0, "e.txt"),
        (1, "a.txt"),
        (1, "pak0.pak"),
        (1, "progs.dat"),
        (2, "a.txt"),
        (3, "a.txt"),
        (3, "progs.dat"),
        (3, "f.txt"),
        (4, "a.txt"),
        (4, "b.txt"),
        (4, "gfx/pop.lmp"),
        (4, "pak0.pak"),
        (4, "pak1.pak"),
        (4, "pak3.pak"),
        (4, "sub/c.txt"),
    ];
    let expect: Vec<(usize, String)> = expect.iter().map(|&(e, n)| (e, n.to_owned())).collect();
    assert_eq!(names, expect);
    for file in &files {
        vfs.read(file).unwrap();
    }
    assert_eq!(vfs.read(&files[1]).unwrap(), b"mod-loose");
    assert_eq!(vfs.read(&files[7]).unwrap(), b"pak0-f");
}

#[test]
fn read_errors() {
    let vfs = spec_vfs(&[]);
    let bogus = FileRef {
        element: 2,
        name: b"x".to_vec(),
        location: Location::Entry(0),
    };
    assert!(matches!(vfs.read(&bogus), Err(VfsError::Io { .. })));
    let bogus = FileRef {
        element: 0,
        name: b"x".to_vec(),
        location: Location::Entry(99),
    };
    assert!(matches!(vfs.read(&bogus), Err(VfsError::Archive { .. })));
    let loose = FileRef {
        element: 2,
        name: b"x".to_vec(),
        location: Location::Loose(PathBuf::from("nowhere")),
    };
    let error = vfs.read(&loose).unwrap_err();
    assert_eq!(
        error.to_string(),
        "nowhere could not be read: entity not found"
    );
    let bad_range = pak_with_extra(&[(b"a", b"xy")], &[]);
    let mut bad_range = bad_range;
    let at = bad_range.len() - 4;
    bad_range[at..].copy_from_slice(&100_i32.to_le_bytes());
    let tree = MemFs::default()
        .file("id1/gfx/pop.lmp", &pop_lmp(&[]))
        .file("id1/pak0.pak", &bad_range);
    let vfs = Vfs::new(tree, NO_ARGS, ".").unwrap();
    let error = vfs.load(b"a").unwrap_err();
    assert_eq!(
        error,
        VfsError::Archive {
            path: "./id1/pak0.pak".to_owned(),
            error: PakError::EntryOutOfBounds {
                filepos: 12,
                filelen: 100
            }
        }
    );
}

#[test]
fn reads_reopen_archives() {
    let vfs = spec_vfs(&[]);
    let before = vfs.host().opens.get();
    lookup(&vfs, "a.txt");
    assert_eq!(vfs.host().opens.get(), before + 1);
}

/// A directory under the system's temporary directory, removed when dropped.
struct TempTree(PathBuf);

impl TempTree {
    fn new(test: &str) -> Self {
        let root = std::env::temp_dir().join(format!("qs-vfs-{test}-{}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn file(&self, path: &str, bytes: &[u8]) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

// spec FS-3.7: on the host's own filesystem, game directory, archive and loose names match
// ASCII case-insensitively.
#[test]
fn host_filesystem() {
    let tree = TempTree::new("host");
    tree.file("ID1/PAK0.PAK", &pak(&[("a.txt", "pak")]));
    tree.file("ID1/Gfx/POP.lmp", &pop_lmp(&[]));
    tree.file("ID1/Maps/Start.BSP", b"start");
    tree.file("ID1/sub/deeper/x.txt", b"x");
    let base = tree.0.to_str().unwrap();
    let vfs = Vfs::new(crate::StdFs, NO_ARGS, base).unwrap();
    assert!(vfs.is_registered());
    assert_eq!(vfs.elements().len(), 2);
    assert_eq!(lookup(&vfs, "a.txt").as_deref(), Some("pak"));
    assert_eq!(lookup(&vfs, "maps/start.bsp").as_deref(), Some("start"));
    assert_eq!(lookup(&vfs, "sub/deeper/x.txt").as_deref(), Some("x"));
    assert_eq!(lookup(&vfs, "sub/deeper"), None);
    let names: Vec<Vec<u8>> = vfs.files().into_iter().map(|file| file.name).collect();
    assert!(names.contains(&b"Maps/Start.BSP".to_vec()));
    assert!(names.contains(&b"sub/deeper/x.txt".to_vec()));
}

// spec FS-3.3: a symbolic link to a file is a file; a link to a directory is not walked, so
// a link loop cannot hang the listing.
#[cfg(unix)]
#[test]
fn host_filesystem_links() {
    let tree = TempTree::new("links");
    tree.file("id1/gfx/pop.lmp", &pop_lmp(&[]));
    tree.file("id1/real.txt", b"real");
    std::os::unix::fs::symlink(tree.0.join("id1/real.txt"), tree.0.join("id1/link.txt")).unwrap();
    std::os::unix::fs::symlink(tree.0.join("id1"), tree.0.join("id1/loop")).unwrap();
    let base = tree.0.to_str().unwrap();
    let vfs = Vfs::new(crate::StdFs, NO_ARGS, base).unwrap();
    assert_eq!(lookup(&vfs, "link.txt").as_deref(), Some("real"));
    let names: Vec<Vec<u8>> = vfs.files().into_iter().map(|file| file.name).collect();
    assert_eq!(
        names,
        [
            b"gfx/pop.lmp".to_vec(),
            b"link.txt".to_vec(),
            b"real.txt".to_vec()
        ]
    );
}
