// SPDX-License-Identifier: GPL-3.0-or-later

use super::{Files, Mode, file_extension, fix_file_name, handle_index};

// spec BUILTIN-18.36: a refused name, an unsupported mode or a file to read that no write made
// opens nothing; writing makes the file under data/ at once, and handles count from 1, a
// closed one reused
#[test]
fn fopen_opens_written_files_only() {
    let mut files = Files::default();
    for name in [b"".as_slice(), b"a:b", b"a\\b", b"/abs", b"x/../y", b".."] {
        assert_eq!(files.open(name, 2), -1.0, "{name:?}");
    }
    assert_eq!(
        files.open(b"scores.txt", 0),
        -1.0,
        "the directory starts empty"
    );
    assert_eq!(files.open(b"scores.txt", 3), -1.0, "an unsupported mode");
    assert_eq!(files.open(b"scores.txt", -1), -1.0);
    assert_eq!(files.open(b"scores.txt", 2), 1.0);
    assert_eq!(
        files.which_pack(b"data/scores.txt"),
        Some(b"id1".as_slice())
    );
    assert_eq!(files.which_pack(b"scores.txt"), None, "written under data/");
    assert_eq!(files.open(b"scores.txt", 0), 2.0, "made empty at once");
    assert_eq!(files.gets(2.0), None, "an empty file has no line");
    files.close(1.0);
    assert_eq!(
        files.open(b"other.txt", 1),
        1.0,
        "the closed handle is reused"
    );
    assert_eq!(Mode::from_number(1), Some(Mode::Append));
}

// spec BUILTIN-18.37: a write is in the file once it is closed; fgets reads each line without
// its line feed or a carriage return before one, an empty line as an empty string, and 0 at
// the end; append empties the file as `w+b` does
#[test]
fn written_lines_read_back() {
    let mut files = Files::default();
    let out = files.open(b"log.txt", 2);
    files.puts(out, b"first\r\n");
    files.puts(out, b"\nlast");
    assert_eq!(files.open(b"log.txt", 0), 2.0);
    assert_eq!(files.gets(2.0), None, "nothing closed yet");
    files.close(out);
    let read = files.open(b"log.txt", 0);
    assert_eq!(files.gets(read), Some(b"first".to_vec()));
    assert_eq!(files.gets(read), Some(Vec::new()));
    assert_eq!(files.gets(read), Some(b"last".to_vec()));
    assert_eq!(files.gets(read), None);
    files.puts(read, b"ignored");
    let append = files.open(b"log.txt", 1);
    files.close(append);
    let again = files.open(b"log.txt", 0);
    assert_eq!(files.gets(again), None, "append emptied it");
}

// spec BUILTIN-18.37: a line over 1023 bytes keeps its first 1023, and the carriage return
// check reads the last of those
#[test]
fn long_lines_keep_their_start() {
    let mut files = Files::default();
    let out = files.open(b"long.txt", 2);
    let mut line = vec![b'a'; 1022];
    line.extend_from_slice(b"\rbcd\nnext");
    files.puts(out, &line);
    files.close(out);
    let read = files.open(b"long.txt", 0);
    assert_eq!(
        files.gets(read),
        Some(vec![b'a'; 1022]),
        "the \\r at 1022 is dropped"
    );
    assert_eq!(files.gets(read), Some(b"next".to_vec()));
    let out = files.open(b"long.txt", 2);
    files.puts(out, &[b'z'; 3000]);
    files.close(out);
    let read = files.open(b"long.txt", 0);
    assert_eq!(files.gets(read).map(|l| l.len()), Some(1023));
}

// spec BUILTIN-18.37: a handle that is not an open file of the right mode reads and writes
// nothing; a handle from just above 0 to 1 is the first, as C truncates the float
#[test]
fn bad_handles_do_nothing() {
    let mut files = Files::default();
    let out = files.open(b"a.txt", 2);
    assert_eq!(files.gets(out), None, "open to write");
    assert_eq!(files.gets(0.0), None);
    assert_eq!(files.gets(f32::NAN), None);
    assert_eq!(files.gets(-5.0), None);
    assert_eq!(files.gets(9.0), None);
    files.puts(0.5, b"x");
    files.close(1.9);
    let read = files.open(b"a.txt", 0);
    assert_eq!(
        files.gets(read),
        Some(b"x".to_vec()),
        "0.5 and 1.9 are handle 1"
    );
    files.close(7.0);
    assert_eq!(handle_index(0.0, 3), None);
    assert_eq!(handle_index(0.01, 3), Some(0));
    assert_eq!(handle_index(3.0, 3), Some(2));
    assert_eq!(handle_index(4.0, 3), None);
}

// spec BUILTIN-18.36: QC_FixFileName reads a name under data/, and also the name itself
// unless it is a config outside configs/, particles/, huds/ and models/; the extension is
// after the last dot of the last part
#[test]
fn file_names_and_fallbacks() {
    assert_eq!(
        fix_file_name(b"a.txt"),
        Some((b"data/a.txt".to_vec(), Some(b"a.txt".to_vec())))
    );
    assert_eq!(fix_file_name(b"server.cfg").map(|f| f.1), Some(None));
    assert_eq!(fix_file_name(b"x/server.CFG").map(|f| f.1), Some(None));
    for allowed in [
        b"configs/a.cfg".as_slice(),
        b"particles/a.cfg",
        b"huds/a.cfg",
        b"models/a.cfg",
    ] {
        assert_eq!(
            fix_file_name(allowed).map(|f| f.1),
            Some(Some(allowed.to_vec())),
            "{allowed:?}"
        );
    }
    assert_eq!(file_extension(b"a.cfg"), b"cfg");
    assert_eq!(file_extension(b"a.b/c"), b"");
    assert_eq!(file_extension(b"x."), b"");
    assert_eq!(file_extension(b"a"), b"");
    assert_eq!(file_extension(b".cfg"), b"cfg");
}
