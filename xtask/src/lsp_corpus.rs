// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask lsp-corpus [--write] <dir>`: the language server driven against a corpus
//! codebase (P2-14).
//!
//! It runs `quakec-lsp` in process over the base protocol's framing, as an editor would: it
//! opens the codebase as the workspace under the FTEQCC profile with the rerelease as the target
//! engine (`initializationOptions`, autoprototyping for a codebase FTEQCC builds with
//! `-Fautoproto`), and for each probe, a name at a place in a file, asks for its definition,
//! its references, its hover, whether it can be renamed and the rename; then completion at two
//! places, the inlay hints of a file, the semantic tokens of every file, and it collects every
//! message shown and every published diagnostic. The answers, rendered one per line with every position as
//! `file:line:column` (1-based, UTF-16 columns), are compared with the golden file
//! `docs/spec/vectors/lsp/<dir name>.txt`; `--write` rewrites it instead.
//!
//! The codebase comes from `cargo xtask corpus fetch rerelease`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use qc_project::FsLoader;
use quakec_lsp::lines::{Encoding, LineIndex, Position};
use quakec_lsp::transport::read_message;
use quakec_lsp::uri;
use serde_json::{Value, json};

use crate::oracles::rerelease::autoprototypes;

const USAGE: &str = "usage: cargo xtask lsp-corpus [--write] <dir>";

/// Where the golden files are, under the repository root.
const GOLDEN: &str = "docs/spec/vectors/lsp";

/// A name to ask about: the file, the text the name starts, and which occurrence of that text.
struct Probe {
    file: &'static str,
    at: &'static str,
    occurrence: usize,
    /// The name, the start of `at`.
    name: &'static str,
}

/// The probes of the rerelease: a function used in another file, a function at its definition,
/// a local, a parameter, a system field and a system global (which rename refuses), a field and
/// a global of the mod's own, a constant, and two builtins, by number and by engine name.
const PROBES: [Probe; 11] = [
    Probe {
        file: "weapons.qc",
        at: "T_Damage (trace_ent",
        occurrence: 0,
        name: "T_Damage",
    },
    Probe {
        file: "subs.qc",
        at: "SUB_Remove",
        occurrence: 0,
        name: "SUB_Remove",
    },
    Probe {
        file: "combat.qc",
        at: "save = ceil",
        occurrence: 0,
        name: "save",
    },
    Probe {
        file: "combat.qc",
        at: "damage < 9999",
        occurrence: 0,
        name: "damage",
    },
    Probe {
        file: "combat.qc",
        at: "takedamage",
        occurrence: 0,
        name: "takedamage",
    },
    Probe {
        file: "combat.qc",
        at: "coop &&",
        occurrence: 0,
        name: "coop",
    },
    Probe {
        file: "doors.qc",
        at: "wait >= 0",
        occurrence: 0,
        name: "wait",
    },
    Probe {
        file: "combat.qc",
        at: "damage_attacker = attacker",
        occurrence: 0,
        name: "damage_attacker",
    },
    Probe {
        file: "defs.qc",
        at: "FL_FLY",
        occurrence: 0,
        name: "FL_FLY",
    },
    Probe {
        file: "weapons.qc",
        at: "setorigin",
        occurrence: 0,
        name: "setorigin",
    },
    Probe {
        file: "client.qc",
        at: "bprint(\"$qc_exited\"",
        occurrence: 0,
        name: "bprint",
    },
];

/// The places completion is asked at: in a body before a local's use, and after a `.`.
const COMPLETIONS: [(&str, &str); 2] = [("combat.qc", "save = ceil"), ("combat.qc", "takedamage")];

/// The file whose inlay hints are asked for.
const HINTS: &str = "combat.qc";

pub(crate) fn main(root: &Path, args: &[String]) -> Result<bool, String> {
    let mut write = false;
    let mut dirs = Vec::new();
    for arg in args {
        if arg == "--write" {
            write = true;
        } else if arg.starts_with("--") {
            return Err(format!("unknown flag {arg}\n{USAGE}"));
        } else {
            dirs.push(PathBuf::from(arg));
        }
    }
    let [dir] = dirs.as_slice() else {
        return Err(USAGE.to_owned());
    };
    let dir = std::path::absolute(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{}: no directory name", dir.display()))?
        .to_owned();
    let text = drive(&dir, autoprototypes(&name))?;
    let golden = root.join(GOLDEN).join(format!("{name}.txt"));
    if write {
        fs::create_dir_all(root.join(GOLDEN)).map_err(|e| format!("{GOLDEN}: {e}"))?;
        fs::write(&golden, &text).map_err(|e| format!("{}: {e}", golden.display()))?;
        println!("lsp-corpus: {name}: wrote {}", golden.display());
        return Ok(true);
    }
    let want = fs::read_to_string(&golden).map_err(|e| format!("{}: {e}", golden.display()))?;
    if want == text {
        println!("lsp-corpus: {name}: every answer matches {GOLDEN}/{name}.txt");
        return Ok(true);
    }
    println!("lsp-corpus: FAIL {name}: the answers differ from {GOLDEN}/{name}.txt");
    for (line, (a, b)) in want.lines().zip(text.lines()).enumerate() {
        if a != b {
            println!(
                "      line {}: golden {a:?}, server {b:?}",
                line.saturating_add(1)
            );
        }
    }
    let (a, b) = (want.lines().count(), text.lines().count());
    if a != b {
        println!("      golden has {a} lines, the server's answers {b}");
    }
    Ok(false)
}

/// A session's requests: each request's id and how to render its answer.
struct Script {
    messages: Vec<Value>,
    /// For each request id, a label and what it was.
    labels: BTreeMap<i64, (String, Ask)>,
    next: i64,
}

/// What a request asked, for rendering its answer.
#[derive(Clone, Copy)]
enum Ask {
    Definition,
    References,
    Hover,
    PrepareRename,
    Rename,
    Completion,
    Hints,
    Tokens,
}

impl Script {
    fn request(&mut self, label: String, ask: Ask, method: &str, params: Value) {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        self.messages
            .push(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        self.labels.insert(id, (label, ask));
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.messages
            .push(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }
}

/// Runs the session against the codebase in `dir` and renders every answer.
fn drive(dir: &Path, autoproto: bool) -> Result<String, String> {
    let module = qc_project::open(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let files: Vec<PathBuf> = module
        .modules
        .iter()
        .flat_map(|m| m.graph.files.iter().map(|f| qc_project::normalize(&f.path)))
        .collect();
    let mut script = Script {
        messages: Vec::new(),
        labels: BTreeMap::new(),
        next: 1,
    };
    script
        .messages
        .push(initialize(dir, "rerelease", autoproto));
    script.notify("initialized", json!({}));
    for probe in &PROBES {
        let path = dir.join(probe.file);
        let at = place(&path, probe.at, probe.occurrence)?;
        let label = format!("{} {}", probe.name, show(dir, &path, at));
        let document = json!({ "uri": uri::from_path(&path) });
        let position = json!({ "line": at.line, "character": at.character });
        let params = json!({ "textDocument": document, "position": position });
        script.request(
            label.clone(),
            Ask::Definition,
            "textDocument/definition",
            params.clone(),
        );
        let references = json!({
            "textDocument": document, "position": position,
            "context": { "includeDeclaration": true },
        });
        script.request(
            label.clone(),
            Ask::References,
            "textDocument/references",
            references,
        );
        script.request(
            label.clone(),
            Ask::Hover,
            "textDocument/hover",
            params.clone(),
        );
        script.request(
            label.clone(),
            Ask::PrepareRename,
            "textDocument/prepareRename",
            params.clone(),
        );
        let rename = json!({
            "textDocument": document, "position": position,
            "newName": format!("{}Renamed", probe.name),
        });
        script.request(label, Ask::Rename, "textDocument/rename", rename);
    }
    for (file, at) in COMPLETIONS {
        let path = dir.join(file);
        let position = place(&path, at, 0)?;
        script.request(
            format!("at {}", show(dir, &path, position)),
            Ask::Completion,
            "textDocument/completion",
            json!({ "textDocument": { "uri": uri::from_path(&path) },
                    "position": { "line": position.line, "character": position.character } }),
        );
    }
    let hints = dir.join(HINTS);
    script.request(
        HINTS.to_owned(),
        Ask::Hints,
        "textDocument/inlayHint",
        json!({ "textDocument": { "uri": uri::from_path(&hints) },
                "range": { "start": { "line": 0, "character": 0 },
                           "end": { "line": u32::MAX, "character": 0 } } }),
    );
    for path in &files {
        script.request(
            relative(dir, path),
            Ask::Tokens,
            "textDocument/semanticTokens/full",
            json!({ "textDocument": { "uri": uri::from_path(path) } }),
        );
    }
    let answers = serve(std::mem::take(&mut script.messages))?;
    let mut text = render(dir, &script, &answers)?;
    // The same codebase targeting id's engine, which lacks the rerelease's `#0:ex_` builtins
    // (ENGT-4.1).
    text.push_str("# the same, target quake\n");
    let vanilla = serve(vec![
        initialize(dir, "quake", autoproto),
        json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
    ])?;
    text.push_str(&diagnostics(dir, &vanilla));
    Ok(text)
}

/// The `initialize` request for the codebase in `dir`, targeting `target`.
fn initialize(dir: &Path, target: &str, autoproto: bool) -> Value {
    json!({
        "jsonrpc": "2.0", "id": 0, "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": uri::from_path(dir),
            "capabilities": {
                "textDocument": {
                    "hover": { "contentFormat": ["markdown"] },
                    "rename": { "prepareSupport": true, "honorsChangeAnnotations": true },
                },
                "workspace": { "workspaceEdit": {
                    "documentChanges": true, "changeAnnotationSupport": {},
                } },
            },
            "initializationOptions": {
                "dialect": "fteqcc", "target": target, "autoproto": autoproto,
            },
        },
    })
}

/// Runs a session of `messages`, then `shutdown` and `exit`, and returns what the server wrote.
fn serve(mut messages: Vec<Value>) -> Result<Vec<Value>, String> {
    messages.push(json!({ "jsonrpc": "2.0", "id": -1, "method": "shutdown" }));
    messages.push(json!({ "jsonrpc": "2.0", "method": "exit" }));
    let input = framed(&messages)?;
    let mut output = Vec::new();
    let exit = quakec_lsp::run(FsLoader, input.as_slice(), &mut output)
        .map_err(|e| format!("the server failed: {e}"))?;
    if exit != quakec_lsp::Exit::Success {
        return Err("the server did not exit cleanly".to_owned());
    }
    unframed(&output)
}

/// The UTF-16 position of the `occurrence`-th `needle` in the file at `path`.
fn place(path: &Path, needle: &str, occurrence: usize) -> Result<Position, String> {
    let text = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let offset = text
        .windows(needle.len())
        .enumerate()
        .filter(|(_, w)| *w == needle.as_bytes())
        .nth(occurrence)
        .map(|(at, _)| at)
        .ok_or_else(|| format!("{}: no {needle:?}", path.display()))?;
    Ok(LineIndex::new(&text, Encoding::Utf16).position(offset))
}

/// A path under `dir`, with `/` separators.
fn relative(dir: &Path, path: &Path) -> String {
    path.strip_prefix(dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// `file:line:column`, 1-based.
fn show(dir: &Path, path: &Path, at: Position) -> String {
    format!(
        "{}:{}:{}",
        relative(dir, path),
        at.line.saturating_add(1),
        at.character.saturating_add(1)
    )
}

/// A protocol location as `file:line:column`.
fn show_location(dir: &Path, location: &Value) -> String {
    let path = location["uri"]
        .as_str()
        .and_then(uri::to_path)
        .unwrap_or_default();
    show(dir, &path, start_of(&location["range"]))
}

/// The start of a protocol range.
fn start_of(range: &Value) -> Position {
    position_of(at(range, "/start"))
}

/// A protocol position.
fn position_of(position: &Value) -> Position {
    let number = |path: &str| {
        at(position, path)
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0)
    };
    Position {
        line: number("/line"),
        character: number("/character"),
    }
}

/// The value at JSON pointer `path` of `value`, `null` when there is none.
fn at<'a>(value: &'a Value, path: &str) -> &'a Value {
    value.pointer(path).unwrap_or(&Value::Null)
}

/// Renders every answer and published diagnostic of the session.
fn render(dir: &Path, script: &Script, messages: &[Value]) -> Result<String, String> {
    let mut out = format!(
        "# cargo xtask lsp-corpus: {}, the FTEQCC profile, target rerelease\n",
        relative(dir.parent().unwrap_or(dir), dir)
    );
    let answers: BTreeMap<i64, &Value> = messages
        .iter()
        .filter(|m| m.get("method").is_none())
        .filter_map(|m| Some((m.get("id")?.as_i64()?, m)))
        .collect();
    for (id, (label, ask)) in &script.labels {
        let answer = answers
            .get(id)
            .ok_or_else(|| format!("no answer to {label}"))?;
        if let Some(error) = answer.get("error") {
            out.push_str(&format!(
                "{} {label}: error {}\n",
                ask_name(*ask),
                error["message"].as_str().unwrap_or_default()
            ));
            continue;
        }
        let result = &answer["result"];
        out.push_str(&format!(
            "{} {label}: {}\n",
            ask_name(*ask),
            summary(dir, *ask, result)
        ));
    }
    for message in messages {
        if message.get("method") == Some(&json!("window/showMessage")) {
            out.push_str(&format!(
                "message type {}: {}\n",
                at(message, "/params/type"),
                at(message, "/params/message").as_str().unwrap_or_default()
            ));
        }
    }
    out.push_str(&diagnostics(dir, messages));
    Ok(out)
}

/// The diagnostics last published for each file, by code, one line per file that has any.
fn diagnostics(dir: &Path, messages: &[Value]) -> String {
    let mut files: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for message in messages {
        if message.get("method") != Some(&json!("textDocument/publishDiagnostics")) {
            continue;
        }
        let params = at(message, "/params");
        let path = at(params, "/uri")
            .as_str()
            .and_then(uri::to_path)
            .unwrap_or_default();
        let codes = files.entry(relative(dir, &path)).or_default();
        codes.clear();
        for d in at(params, "/diagnostics").as_array().into_iter().flatten() {
            let code = at(d, "/code").as_str().unwrap_or_default().to_owned();
            let count = codes.entry(code).or_default();
            *count = count.saturating_add(1);
        }
    }
    files.retain(|_, codes| !codes.is_empty());
    if files.is_empty() {
        return "diagnostics: none\n".to_owned();
    }
    let mut out = String::new();
    for (file, codes) in files {
        let list: Vec<String> = codes.iter().map(|(c, n)| format!("{c} x{n}")).collect();
        out.push_str(&format!("diagnostics {file}: {}\n", list.join(", ")));
    }
    out
}

const fn ask_name(ask: Ask) -> &'static str {
    match ask {
        Ask::Definition => "definition",
        Ask::References => "references",
        Ask::Hover => "hover",
        Ask::PrepareRename => "prepare-rename",
        Ask::Rename => "rename",
        Ask::Completion => "completion",
        Ask::Hints => "inlay-hints",
        Ask::Tokens => "semantic-tokens",
    }
}

/// One answer, on one line.
fn summary(dir: &Path, ask: Ask, result: &Value) -> String {
    if result.is_null() {
        return "null".to_owned();
    }
    match ask {
        Ask::Definition => show_location(dir, result),
        Ask::References => {
            let list = result.as_array().map(Vec::as_slice).unwrap_or_default();
            let mut files: BTreeMap<String, usize> = BTreeMap::new();
            for location in list {
                let path = location["uri"]
                    .as_str()
                    .and_then(uri::to_path)
                    .unwrap_or_default();
                let count = files.entry(relative(dir, &path)).or_default();
                *count = count.saturating_add(1);
            }
            let by_file: Vec<String> = files.iter().map(|(f, n)| format!("{f} x{n}")).collect();
            format!("{} ({})", list.len(), by_file.join(", "))
        }
        Ask::Hover => at(result, "/contents/value")
            .as_str()
            .unwrap_or_default()
            .replace('\n', "\\n"),
        Ask::PrepareRename => {
            let start = start_of(result);
            format!(
                "{}:{}",
                start.line.saturating_add(1),
                start.character.saturating_add(1)
            )
        }
        Ask::Rename => {
            // Plain `changes`, or `documentChanges` of annotated edits that need confirmation
            // (ADR-0019).
            let mut edits = 0_usize;
            let mut files = Vec::new();
            let mut count_file = |uri: &str, list: &Value| {
                let count = list.as_array().map_or(0, Vec::len);
                edits = edits.saturating_add(count);
                let path = uri::to_path(uri).unwrap_or_default();
                files.push(format!("{} x{count}", relative(dir, &path)));
            };
            for (uri, list) in result["changes"].as_object().into_iter().flatten() {
                count_file(uri, list);
            }
            for change in result["documentChanges"].as_array().into_iter().flatten() {
                let uri = at(change, "/textDocument/uri").as_str().unwrap_or_default();
                count_file(uri, at(change, "/edits"));
            }
            let confirm =
                at(result, "/changeAnnotations/map-name/needsConfirmation") == &json!(true);
            format!(
                "{edits} edits ({}){}",
                files.join(", "),
                if confirm { ", each to confirm" } else { "" }
            )
        }
        Ask::Completion => {
            let list = result.as_array().map(Vec::as_slice).unwrap_or_default();
            let first: Vec<&str> = list
                .iter()
                .take(5)
                .filter_map(|i| i["label"].as_str())
                .collect();
            let last: Vec<&str> = list
                .iter()
                .rev()
                .take(3)
                .filter_map(|i| i["label"].as_str())
                .collect();
            format!(
                "{} items, first {}, last {}",
                list.len(),
                first.join(" "),
                last.join(" ")
            )
        }
        Ask::Hints => {
            let list = result.as_array().map(Vec::as_slice).unwrap_or_default();
            let first: Vec<String> = list
                .iter()
                .take(5)
                .map(|h| {
                    let place = position_of(at(h, "/position"));
                    format!(
                        "{}:{} {}",
                        place.line.saturating_add(1),
                        place.character.saturating_add(1),
                        at(h, "/label").as_str().unwrap_or_default()
                    )
                })
                .collect();
            format!("{} hints, first {}", list.len(), first.join(", "))
        }
        Ask::Tokens => {
            let count = result["data"].as_array().map_or(0, Vec::len);
            format!("{} tokens", count.checked_div(5).unwrap_or(0))
        }
    }
}

/// The bytes of `messages`, framed.
fn framed(messages: &[Value]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for message in messages {
        let content = serde_json::to_vec(message).map_err(|e| e.to_string())?;
        out.extend_from_slice(format!("Content-Length: {}\r\n\r\n", content.len()).as_bytes());
        out.extend_from_slice(&content);
    }
    Ok(out)
}

/// The messages of `bytes`, unframed.
fn unframed(bytes: &[u8]) -> Result<Vec<Value>, String> {
    let mut input = bytes;
    let mut out = Vec::new();
    while let Some(content) = read_message(&mut input).map_err(|e| e.to_string())? {
        out.push(serde_json::from_slice(&content).map_err(|e| e.to_string())?);
    }
    Ok(out)
}
