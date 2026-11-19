// SPDX-License-Identifier: GPL-3.0-or-later

//! The paths a command's JSON arguments name: every string under a key that names a path
//! ([`is_path_key`]), alone or in an array, at any depth, which the agent's permissions and the
//! MCP server's roots check and make absolute before a command runs. Every command is an agent's
//! tool, so a parameter naming a file a command reads or writes is under such a key. Two are
//! not, on purpose: a `game` folder, which is the Quake install outside every project and is
//! only read, and a breakpoint's `file`, which names a source file and opens nothing.

use std::path::Path;

use serde_json::Value;

use crate::lexical;

/// Every string under a key that names a path, alone or in an array, anywhere in `value`.
pub fn collect_paths(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, inner) in object {
                if is_path_key(key) {
                    let listed = inner.as_array().into_iter().flatten();
                    out.extend(
                        std::iter::once(inner)
                            .chain(listed)
                            .filter_map(Value::as_str)
                            .map(str::to_owned),
                    );
                }
                collect_paths(inner, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect_paths(item, out)),
        _ => {}
    }
}

/// The parameters that hold a path under a name of their own: an FGD file to write and an
/// export's output.
const NAMED_PATH_KEYS: [&str; 2] = ["fgd", "out"];

/// Whether a parameter named `key` holds a path, or paths: `path`, `paths`, a name ending in
/// `_path` or `_paths`, or one of the few with a name of its own.
#[must_use]
pub fn is_path_key(key: &str) -> bool {
    key == "path"
        || key == "paths"
        || key.ends_with("_path")
        || key.ends_with("_paths")
        || NAMED_PATH_KEYS.contains(&key)
}

/// Makes every path `arguments` names absolute, a relative one taken from `project`, with `.`
/// and `..` worked out, so the commands, which name files by their full paths, find them.
pub fn absolute_paths(arguments: &mut Value, project: &Path) {
    match arguments {
        Value::Object(object) => {
            let full = |path: &str| {
                Value::String(lexical(&project.join(path)).to_string_lossy().into_owned())
            };
            for (key, inner) in object.iter_mut() {
                match (is_path_key(key), &mut *inner) {
                    (true, Value::String(path)) => *inner = full(path),
                    (true, Value::Array(items)) => {
                        for item in items {
                            match item {
                                Value::String(path) => *item = full(path),
                                other => absolute_paths(other, project),
                            }
                        }
                    }
                    _ => absolute_paths(inner, project),
                }
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| absolute_paths(item, project)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::indexing_slicing,
        reason = "test code: a missing key is a failed test"
    )]

    use std::path::{Path, PathBuf};

    use serde_json::json;

    use super::{absolute_paths, collect_paths};
    use crate::{lexical, lies_under};

    // No spec statement: a path's `.` and `..` are worked out by its text, `..` at the root
    // staying there
    #[test]
    fn a_path_is_worked_out_by_its_text() {
        assert_eq!(lexical(Path::new("/p/./a/../b/c")), PathBuf::from("/p/b/c"));
        assert_eq!(lexical(Path::new("/../x")), PathBuf::from("/x"));
        // A relative path keeps the `..` that leaves it, so it never seems to lie under `.`.
        assert_eq!(lexical(Path::new("./../../x")), PathBuf::from("../../x"));
        assert_eq!(lexical(Path::new("a/../../x")), PathBuf::from("../x"));
    }

    // No spec statement: a path lies under a root only when it stays inside it, a relative
    // root included, whose `.` works out to no parts
    #[test]
    fn a_path_lies_under_a_root_only_inside_it() {
        assert!(lies_under(Path::new("/p/a/../b.qc"), Path::new("/p")));
        assert!(!lies_under(Path::new("/p/../q/b.qc"), Path::new("/p")));
        assert!(lies_under(Path::new("./a.qc"), Path::new(".")));
        assert!(!lies_under(Path::new("./../../etc/x"), Path::new(".")));
        assert!(!lies_under(Path::new("/q"), Path::new("/p")));
    }

    // No spec statement: every string under `path` or a key ending in `_path` is found, at any
    // depth and in arrays, and other keys are not
    #[test]
    fn the_paths_of_arguments_are_found_and_made_absolute() {
        let mut arguments = json!({
            "path": "a.qc",
            "nested": { "map_path": "maps/../maps/e1m1.map", "pathname": "no" },
            "files": [{ "path": "/abs/b.qc" }, { "path": "c.qc" }],
            "path_count": 3,
        });
        let mut found = Vec::new();
        collect_paths(&arguments, &mut found);
        found.sort();
        assert_eq!(
            found,
            ["/abs/b.qc", "a.qc", "c.qc", "maps/../maps/e1m1.map"]
        );
        absolute_paths(&mut arguments, Path::new("/p"));
        // The path as this OS writes it, `\` between its parts on Windows.
        let native = |path: &str| json!(lexical(Path::new(path)).to_string_lossy());
        assert_eq!(arguments["path"], native("/p/a.qc"));
        assert_eq!(arguments["nested"]["map_path"], native("/p/maps/e1m1.map"));
        assert_eq!(arguments["nested"]["pathname"], json!("no"));
        assert_eq!(arguments["files"][0]["path"], native("/abs/b.qc"));
        assert_eq!(arguments["files"][1]["path"], native("/p/c.qc"));
        assert_eq!(arguments["path_count"], json!(3));
    }

    // No spec statement: a list of paths under `paths` or a key ending in `_paths`, and the
    // paths named `fgd` and `out`, are found and made absolute; a game folder and a breakpoint's
    // file are not paths a command opens under the project
    #[test]
    fn listed_and_named_paths_are_found_and_made_absolute() {
        let mut arguments = json!({
            "paths": ["a.qc", "/abs/b.qc", 7],
            "open_paths": ["../c.qc"],
            "fgd": "defs/mod.fgd",
            "out": "/tmp/mod.zip",
            "game": "quake",
            "file": "doors.qc",
        });
        let mut found = Vec::new();
        collect_paths(&arguments, &mut found);
        found.sort();
        assert_eq!(
            found,
            [
                "../c.qc",
                "/abs/b.qc",
                "/tmp/mod.zip",
                "a.qc",
                "defs/mod.fgd"
            ]
        );
        absolute_paths(&mut arguments, Path::new("/p/q"));
        let native = |path: &str| json!(lexical(Path::new(path)).to_string_lossy());
        assert_eq!(
            arguments["paths"],
            json!([native("/p/q/a.qc"), native("/abs/b.qc"), 7])
        );
        assert_eq!(arguments["open_paths"], json!([native("/p/c.qc")]));
        assert_eq!(arguments["fgd"], native("/p/q/defs/mod.fgd"));
        assert_eq!(arguments["out"], native("/tmp/mod.zip"));
        assert_eq!(arguments["game"], json!("quake"));
        assert_eq!(arguments["file"], json!("doors.qc"));
    }
}
