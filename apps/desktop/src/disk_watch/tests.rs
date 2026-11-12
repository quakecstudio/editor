// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::time::{Duration, Instant, SystemTime};

use super::DiskWatch;

// No spec statement: P5-17: a file whose modified time moved is reported once, a file seen for
// the first time or written by the editor is not, and the files are looked at once a second
#[test]
fn a_moved_time_is_a_change() {
    let dir = std::env::temp_dir().join(format!("qs-disk-watch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.map");
    std::fs::write(&file, "{}").unwrap();
    let set = |seconds: u64| {
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
            .unwrap();
    };
    set(1000);
    let start = Instant::now();
    let mut watch = DiskWatch::default();
    let at = |seconds: u64| start + Duration::from_secs(seconds);
    assert!(
        watch.changed(at(0), vec![file.clone()]).is_empty(),
        "first seen"
    );
    set(2000);
    assert!(
        watch.changed(at(0), vec![file.clone()]).is_empty(),
        "within the second"
    );
    assert_eq!(
        watch.changed(at(1), vec![file.clone()]),
        std::slice::from_ref(&file)
    );
    assert!(watch.changed(at(2), vec![file.clone()]).is_empty(), "once");
    set(3000);
    watch.wrote(file.clone());
    assert!(
        watch.changed(at(3), vec![file.clone()]).is_empty(),
        "the editor's own write"
    );
}
