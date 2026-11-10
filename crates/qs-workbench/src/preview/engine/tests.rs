// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::io::{BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use interprocess::local_socket::traits::Listener as _;
use interprocess::local_socket::{GenericFilePath, ListenerOptions, ToFsName as _};
use qc_project::MemoryLoader;
use qs_bridge::{
    Answer, Capabilities, Host, HostInfo, Id, Message, Session, read_frame, write_frame,
};
use qs_commands::{Registry, Session as Commands};
use qs_engine::Client;
use serde_json::{Value, json};

use super::{
    ENGINE_CONSOLE, ENGINE_INSPECT, ENGINE_SCREENSHOT, EngineInspectParams, Inspected, ShotSize,
    console_output, inspect, next_screenshot_path, register_engine, screenshot_to,
};
use crate::preview::{HasPreview, Preview, PreviewConfig, PreviewError};
use crate::{Config, HasWorkbench, Workbench};

/// The eight bytes a PNG starts with, in base64.
const PNG_BASE64: &str = "iVBORw0KGgo=";

/// A host that answers `inspect` with its parameters and `screenshot` with a PNG's signature,
/// and after answering `console` prints two lines, or none for `quiet`.
struct Fake;

impl Host for Fake {
    fn info(&self) -> HostInfo {
        HostInfo {
            name: "fake".to_owned(),
            version: "0".to_owned(),
            engine: "test".to_owned(),
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::of(["inspect", "console", "screenshot"])
    }

    fn call(&mut self, _id: &Id, method: &str, params: &Value) -> Answer {
        Answer::Now(Ok(match method {
            "screenshot" => json!({
                "png": PNG_BASE64,
                "width": params.get("width").cloned().unwrap_or(json!(1280)),
                "height": params.get("height").cloned().unwrap_or(json!(800)),
            }),
            _ => params.clone(),
        }))
    }

    fn shutdown(&mut self) {}
}

static SOCKETS: AtomicUsize = AtomicUsize::new(0);

/// A client of the fake host, initialized.
fn client() -> Client {
    let name = format!(
        "qs-workbench-engine-{}-{}",
        std::process::id(),
        SOCKETS.fetch_add(1, Ordering::Relaxed)
    );
    let path = if cfg!(windows) {
        PathBuf::from(format!(r"\\.\pipe\{name}"))
    } else {
        std::env::temp_dir().join(format!("{name}.sock"))
    };
    let listener = ListenerOptions::new()
        .name(path.as_os_str().to_fs_name::<GenericFilePath>().unwrap())
        .try_overwrite(true)
        .create_sync()
        .unwrap();
    std::thread::spawn(move || {
        let stream = listener.accept().unwrap();
        let mut reader = BufReader::new(&stream);
        let mut session = Session::new();
        let mut host = Fake;
        let line = |text: &str| {
            qs_bridge::event("console/line", json!({ "text": text, "level": "print" })).to_bytes()
        };
        while let Ok(Some(content)) = read_frame(&mut reader) {
            let Ok(message) = Message::parse(&content) else {
                return;
            };
            let console = match &message {
                Message::Request { method, params, .. } if method == "console" => params
                    .get("command")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                _ => None,
            };
            let reply = session.handle(&mut host, message);
            let mut out = &stream;
            if let Some(response) = reply.response {
                write_frame(&mut out, &response.to_bytes()).unwrap();
            }
            if let Some(command) = console.filter(|c| c != "quiet") {
                write_frame(&mut out, &line(&format!("] {command}"))).unwrap();
                write_frame(&mut out, &line("Unknown command")).unwrap();
            }
            out.flush().unwrap();
        }
    });
    let mut client = Client::connect(&path, 100, Duration::from_millis(10)).unwrap();
    client.initialize("tests").unwrap();
    client
}

// No spec statement: P4-03, engine.inspect reads entities, globals, the stack and one edict as
// the bridge asks for them; an edict to read is needed for edict
#[test]
fn inspect_asks_the_bridge() {
    let mut client = client();
    let ask = |what, edict| EngineInspectParams { what, edict };
    let mut answer = |what, edict| inspect(&mut client, &ask(what, edict)).unwrap();
    assert_eq!(
        answer(Inspected::Entities, None),
        json!({ "what": "entities" })
    );
    assert_eq!(
        answer(Inspected::Globals, Some(4)),
        json!({ "what": "globals" })
    );
    assert_eq!(answer(Inspected::Stack, None), json!({ "what": "stack" }));
    assert_eq!(
        answer(Inspected::Edict, Some(7)),
        json!({ "what": "edict", "edict": 7 })
    );
    assert!(matches!(
        inspect(&mut client, &ask(Inspected::Edict, None)),
        Err(PreviewError::BadValue { field, .. }) if field == "edict"
    ));
}

// No spec statement: P4-03, PRIN-2, engine.console answers the lines a command printed, which
// come after its answer; a command that prints nothing answers no lines
#[test]
fn console_answers_what_the_command_printed() {
    let mut client = client();
    let output = console_output(
        &mut client,
        "frobnicate",
        Duration::from_secs(10),
        Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(output.lines, ["] frobnicate", "Unknown command"]);
    assert_eq!(
        output.events.len(),
        2,
        "every event is kept for the preview"
    );
    let quiet = console_output(
        &mut client,
        "quiet",
        Duration::from_millis(50),
        Duration::from_millis(50),
    )
    .unwrap();
    assert!(quiet.lines.is_empty(), "{:?}", quiet.lines);
}

// No spec statement: P4-03, engine.screenshot writes the PNG the bridge answers, at the size
// asked, making its folder; the default path is the first free one under build/screenshots
#[test]
fn screenshots_are_written() {
    let mut client = client();
    let dir =
        std::env::temp_dir().join(format!("qs-workbench-engine-shots-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    let first = next_screenshot_path(&dir);
    assert_eq!(first, dir.join("build/screenshots/screenshot-1.png"));
    let size = ShotSize {
        width: 320,
        height: 200,
    };
    let shot = screenshot_to(&mut client, &first, Some(size)).unwrap();
    assert_eq!((shot.width, shot.height), (320, 200));
    let bytes = std::fs::read(&first).unwrap();
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "{bytes:?}");
    assert_eq!(
        next_screenshot_path(&dir),
        dir.join("build/screenshots/screenshot-2.png")
    );
    let own = screenshot_to(&mut client, &next_screenshot_path(&dir), None).unwrap();
    assert_eq!((own.width, own.height), (1280, 800));
}

/// A workbench and a preview with no game running.
struct Context {
    workbench: Workbench<MemoryLoader>,
    preview: Preview,
}

impl HasWorkbench for Context {
    type Loader = MemoryLoader;

    fn workbench(&mut self) -> &mut Workbench<MemoryLoader> {
        &mut self.workbench
    }
}

impl HasPreview for Context {
    fn workbench_and_preview(&mut self) -> (&mut Workbench<MemoryLoader>, &mut Preview) {
        (&mut self.workbench, &mut self.preview)
    }
}

// No spec statement: P4-03, the engine commands run through the registry and need a running
// game; a screenshot with no path needs a project to put it in
#[test]
fn the_engine_commands_need_a_running_game() {
    let mut registry = Registry::new();
    register_engine(&mut registry).unwrap();
    let context = Context {
        workbench: Workbench::new(MemoryLoader::new(), Config::default()),
        preview: Preview::new(PreviewConfig {
            host: None,
            app: Path::new("/nowhere/app").to_path_buf(),
            game: None,
            extra: Vec::new(),
        }),
    };
    let mut session = Commands::new(context, registry);
    for (id, params) in [
        (ENGINE_INSPECT, json!({ "what": "entities" })),
        (ENGINE_CONSOLE, json!({ "command": "god" })),
        (ENGINE_SCREENSHOT, json!({ "path": "/tmp/x.png" })),
    ] {
        let error = session.execute(id, params).unwrap_err();
        assert!(error.to_string().contains("not running"), "{id}: {error}");
    }
    let error = session.execute(ENGINE_SCREENSHOT, json!({})).unwrap_err();
    assert!(error.to_string().contains("no project is open"), "{error}");
    assert!(
        session
            .execute(ENGINE_INSPECT, json!({ "what": "nothing" }))
            .is_err()
    );
}
