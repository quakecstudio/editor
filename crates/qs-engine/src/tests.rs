// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::fs;
use std::io::{BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use interprocess::local_socket::traits::Listener as _;
use interprocess::local_socket::{GenericFilePath, ListenerOptions, ToFsName as _};
use qs_bridge::{
    Answer, Capabilities, ErrorCode, Host, HostInfo, Id, Message, Session, event, read_frame,
    write_frame,
};
use qs_install::GameBase;
use serde_json::{Value, json};

use crate::{
    Client, ClientError, Flavour, InspectWhat, LaunchError, LaunchSpec, RunRoot, RunRootError,
    StepKind, host_command, locate_host, prepare_run_root,
};

/// A folder removed when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("qs-engine-{name}-{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        Self(dir.canonicalize().unwrap())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _gone = fs::remove_dir_all(&self.0);
    }
}

/// A stand-in install under `dir`: `<dir>/install/<base>/id1/pak0.pak`, and for the rerelease
/// a `QuakeEX.kpf`.
fn install(dir: &Path, base: &str, kpf: bool) -> GameBase {
    let path = dir.join("install").join(base);
    let id1 = path.join("id1");
    fs::create_dir_all(&id1).unwrap();
    fs::write(id1.join("pak0.pak"), b"PACK").unwrap();
    let kpf = kpf.then(|| {
        let file = path.join("QuakeEX.kpf");
        fs::write(&file, b"PK").unwrap();
        file
    });
    GameBase {
        path,
        id1: id1.clone(),
        pak0: id1.join("pak0.pak"),
        pak1: None,
        game_dirs: Vec::new(),
        kpf,
    }
}

fn resolves_to(link: &Path, target: &Path) -> bool {
    fs::canonicalize(link).is_ok_and(|resolved| resolved == fs::canonicalize(target).unwrap())
}

// ADR-0008: a run root links id1 to the install's and the mod to its build output, which it
// makes, and writes nothing inside the install
#[test]
fn a_run_root_links_the_install_and_the_mod() {
    let dir = TempDir::new("root");
    let base = install(&dir.0, "quake", false);
    let project = dir.0.join("project");
    let root = prepare_run_root(&project, Flavour::Original, &base, "mymod").unwrap();
    assert_eq!(root.path, project.join("build/run/original"));
    assert_eq!(root.game.as_deref(), Some("mymod"));
    assert_eq!(root.kpf, None);
    assert!(resolves_to(&root.path.join("id1"), &base.id1));
    assert!(resolves_to(
        &root.path.join("mymod"),
        &project.join("build/mymod")
    ));
    let in_install: Vec<_> = fs::read_dir(&base.id1).unwrap().collect();
    assert_eq!(in_install.len(), 1, "only pak0.pak, as it was");
    assert_eq!(Flavour::Rerelease.dir_name(), "rerelease");
}

// ADR-0008: the rerelease's run root links QuakeEX.kpf on macOS and Linux, and passes it with
// -kpf on Windows, where a junction cannot link a file
#[test]
fn the_rerelease_root_has_the_kpf() {
    let dir = TempDir::new("kpf");
    let base = install(&dir.0, "rerelease", true);
    let project = dir.0.join("project");
    let root = prepare_run_root(&project, Flavour::Rerelease, &base, "mymod").unwrap();
    if cfg!(windows) {
        assert_eq!(root.kpf, base.kpf);
    } else {
        assert_eq!(root.kpf, None);
        assert!(resolves_to(
            &root.path.join("QuakeEX.kpf"),
            base.kpf.as_ref().unwrap()
        ));
    }
    // The original game has no kpf to link.
    let original = prepare_run_root(&project, Flavour::Original, &base, "mymod").unwrap();
    assert!(!original.path.join("QuakeEX.kpf").exists());
}

// P3-06: a broken or wrongly targeted link is repaired before the next launch; a real folder
// in a link's place is left alone and refused, as is a mod name that is a path
#[test]
fn run_root_links_are_repaired() {
    let dir = TempDir::new("repair");
    let base = install(&dir.0, "quake", false);
    let other = install(&dir.0, "other", false);
    let project = dir.0.join("project");
    prepare_run_root(&project, Flavour::Original, &other, "mymod").unwrap();
    let root = prepare_run_root(&project, Flavour::Original, &base, "mymod").unwrap();
    assert!(
        resolves_to(&root.path.join("id1"), &base.id1),
        "wrongly targeted, repaired"
    );
    fs::remove_dir_all(&base.path).unwrap();
    let again = install(&dir.0, "quake", false);
    assert!(prepare_run_root(&project, Flavour::Original, &again, "mymod").is_ok());
    assert!(
        resolves_to(&root.path.join("id1"), &again.id1),
        "broken, repaired"
    );

    let real = project.join("build/run/original/id1");
    fs::remove_file(&real)
        .or_else(|_| fs::remove_dir(&real))
        .unwrap();
    fs::create_dir_all(real.join("kept")).unwrap();
    assert!(matches!(
        prepare_run_root(&project, Flavour::Original, &again, "mymod"),
        Err(RunRootError::Link(_))
    ));
    assert!(real.join("kept").is_dir(), "a real folder is never removed");
    for bad in ["", ".", "..", "a/b", "c:d", "id1", "ID1"] {
        assert!(
            matches!(
                prepare_run_root(&project, Flavour::Original, &again, bad),
                Err(RunRootError::BadModName(_))
            ),
            "{bad:?}"
        );
    }
}

// ADR-0008: the host is launched with -basedir, -game when there is a mod, -kpf when passed,
// then -bridge, the extra words and the map
#[test]
fn the_host_command_line() {
    let spec = LaunchSpec {
        host: PathBuf::from("/app/preview-host"),
        run_root: RunRoot {
            path: PathBuf::from("/p/build/run/rerelease"),
            game: Some("mymod".to_owned()),
            kpf: Some(PathBuf::from("/q/QuakeEX.kpf")),
        },
        map: Some("e1m1".to_owned()),
        socket: PathBuf::from("/tmp/b.sock"),
        extra: vec!["--nosound".to_owned()],
    };
    let command = host_command(&spec);
    let args: Vec<String> = command
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(command.get_program(), "/app/preview-host");
    assert_eq!(
        args,
        [
            "-basedir",
            "/p/build/run/rerelease",
            "-game",
            "mymod",
            "-kpf",
            "/q/QuakeEX.kpf",
            "-bridge",
            "/tmp/b.sock",
            "--nosound",
            "e1m1"
        ]
    );
    let plain = LaunchSpec {
        run_root: RunRoot {
            kpf: None,
            ..spec.run_root.clone()
        },
        map: None,
        extra: Vec::new(),
        ..spec
    };
    let args: Vec<String> = host_command(&plain)
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert!(!args.iter().any(|a| a == "-kpf"));
    assert_eq!(args.last().map(String::as_str), Some("/tmp/b.sock"));
    let id1 = LaunchSpec {
        run_root: RunRoot {
            game: None,
            ..plain.run_root.clone()
        },
        ..plain
    };
    let args: Vec<String> = host_command(&id1)
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "-basedir",
            "/p/build/run/rerelease",
            "-bridge",
            "/tmp/b.sock"
        ]
    );
}

// P3-06: the host comes from the path the user set, else from beside the app; a missing one
// is an error, never a download
#[test]
fn the_host_is_found_beside_the_app() {
    let dir = TempDir::new("locate");
    let app = dir.0.join("QuakeC Studio");
    let host = dir
        .0
        .join(format!("preview-host{}", std::env::consts::EXE_SUFFIX));
    assert!(matches!(
        locate_host(None, &app),
        Err(LaunchError::NotFound(path)) if path == host
    ));
    fs::write(&host, b"").unwrap();
    assert_eq!(locate_host(None, &app).unwrap(), host);
    let configured = dir.0.join("elsewhere");
    assert!(locate_host(Some(&configured), &app).is_err());
    fs::write(&configured, b"").unwrap();
    assert_eq!(locate_host(Some(&configured), &app).unwrap(), configured);
}

/// A host that offers `pause`, `step`, `breakpoint/clear` and `evaluate` and sends an event
/// before each answer.
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
        Capabilities::of(["pause", "step", "breakpoint/clear", "evaluate"])
    }

    fn call(&mut self, _id: &Id, method: &str, params: &Value) -> Answer {
        Answer::Now(Ok(json!({ "method": method, "params": params })))
    }

    fn shutdown(&mut self) {}
}

static SOCKETS: AtomicUsize = AtomicUsize::new(0);

fn socket_path() -> PathBuf {
    let name = format!(
        "qs-engine-{}-{}",
        std::process::id(),
        SOCKETS.fetch_add(1, Ordering::Relaxed)
    );
    if cfg!(windows) {
        PathBuf::from(format!(r"\\.\pipe\{name}"))
    } else {
        std::env::temp_dir().join(format!("{name}.sock"))
    }
}

/// Serves the fake host at `path` to one client, writing a `console/line` event before each
/// response.
fn serve(path: &Path) {
    let name = path.as_os_str().to_fs_name::<GenericFilePath>().unwrap();
    let listener = ListenerOptions::new()
        .name(name)
        .try_overwrite(true)
        .create_sync()
        .unwrap();
    std::thread::spawn(move || {
        let stream = listener.accept().unwrap();
        let mut reader = BufReader::new(&stream);
        let mut session = Session::new();
        let mut host = Fake;
        while let Ok(Some(content)) = read_frame(&mut reader) {
            let Ok(message) = Message::parse(&content) else {
                return;
            };
            let event = qs_bridge::event("console/line", json!({ "text": "hi", "level": "print" }));
            let reply = session.handle(&mut host, message);
            let mut out = &stream;
            if session.is_initialized() {
                write_frame(&mut out, &event.to_bytes()).unwrap();
            }
            if let Some(response) = reply.response {
                write_frame(&mut out, &response.to_bytes()).unwrap();
            }
            out.flush().unwrap();
        }
    });
}

// BRIDGE-2.1, BRIDGE-2.3: the client initializes, then sends a command the host offers and
// refuses, unsent, one it does not; a host's error comes back with its code
#[test]
fn the_client_speaks_the_bridge() {
    let path = socket_path();
    serve(&path);
    let mut client = Client::connect(&path, 100, Duration::from_millis(10)).unwrap();
    let answer = client.initialize("tests").unwrap();
    assert_eq!(answer["host"]["name"], "fake");
    assert!(client.offers("pause") && client.offers("inspect"));
    assert!(!client.offers("screenshot"));
    let paused = client.pause().unwrap();
    assert_eq!(paused["method"], "pause");
    let stepped = client.step(StepKind::Frame).unwrap();
    assert_eq!(stepped["params"]["kind"], "frame");
    let cleared = client.clear_breakpoint(4).unwrap();
    assert_eq!(cleared["params"], json!({ "id": 4 }));
    let evaluated = client.evaluate("self.health").unwrap();
    assert_eq!(evaluated["params"], json!({ "expression": "self.health" }));
    let inspected = client.inspect(InspectWhat::Edict(3)).unwrap();
    assert_eq!(inspected["params"], json!({ "what": "edict", "edict": 3 }));
    assert!(matches!(
        client.screenshot(None),
        Err(ClientError::Unsupported(method)) if method == "screenshot"
    ));
    // Each answer, initialize's too, came after an event, kept in order.
    let events = client.take_events();
    assert_eq!(events.len(), 6);
    assert!(events.iter().all(|e| e.method == "console/line"));
    // The host answers a step with bad parameters with -32602 (BRIDGE-4.2).
    let bad = client.request("step", json!({ "kind": "sideways" }));
    assert!(matches!(
        bad,
        Err(ClientError::Rpc {
            code: ErrorCode::InvalidParams,
            ..
        })
    ));
    let waited = client
        .wait_event("console/line", Duration::from_secs(5))
        .unwrap();
    assert_eq!(waited.params["text"], "hi");
}

// BRIDGE-1.1: a client cannot connect where no host listens, and a closed host is an error
#[test]
fn a_missing_or_closed_host_is_an_error() {
    let path = socket_path();
    assert!(matches!(
        Client::connect(&path, 2, Duration::from_millis(1)),
        Err(ClientError::Io(_))
    ));
    let (read, write) = (std::io::empty(), std::io::sink());
    let mut client = Client::over(read, write);
    assert!(matches!(
        client.initialize("tests"),
        Err(ClientError::Closed)
    ));
}

/// A host that sends `console/line` events without end and answers nothing.
struct Chatty {
    frame: Vec<u8>,
    at: usize,
}

impl Chatty {
    fn new() -> Self {
        let mut frame = Vec::new();
        let line = event(
            "console/line",
            json!({ "text": "x", "level": "print", "time": 0 }),
        );
        write_frame(&mut frame, &line.to_bytes()).unwrap();
        Self { frame, at: 0 }
    }
}

impl std::io::Read for Chatty {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        for byte in buf.iter_mut() {
            *byte = self.frame[self.at];
            self.at = self
                .at
                .wrapping_add(1)
                .checked_rem(self.frame.len())
                .unwrap();
        }
        Ok(buf.len())
    }
}

// spec BRIDGE-3.1: the host's events between two answers do not stretch a wait; an event that
// never comes times out however many others do
#[test]
fn a_wait_ends_on_time_while_other_events_come() {
    let mut client = Client::over(Chatty::new(), std::io::sink());
    let waited = client.wait_event("map/loaded", Duration::from_millis(50));
    assert!(matches!(waited, Err(ClientError::Timeout)), "{waited:?}");
    assert!(!client.take_events().is_empty());
}

// spec BRIDGE-3.1: polling takes the events already read without waiting for more, and a
// closed connection is an error once every event before it is taken
#[test]
fn polling_takes_the_events_without_waiting() {
    let mut chatty = Client::over(Chatty::new(), std::io::sink());
    let start = std::time::Instant::now();
    let mut seen = 0;
    while seen == 0 && start.elapsed() < Duration::from_secs(5) {
        seen = chatty.poll_events().unwrap().len();
    }
    assert!(seen > 0, "the reader thread's events arrive");
    let mut client = Client::over(std::io::empty(), std::io::sink());
    let mut result = client.poll_events();
    while matches!(&result, Ok(events) if events.is_empty()) {
        result = client.poll_events();
    }
    assert!(matches!(result, Err(ClientError::Closed)), "{result:?}");
}
