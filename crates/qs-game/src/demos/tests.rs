// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap, index or panic is a failed test"
)]

use std::path::PathBuf;

use qs_client::{Client, DemoFrameError, Event, ModelInfo, Models, Random};

use super::{Connection, DemoHost, Demos, Level, Showing, Started, with_extension};
use crate::args::Args;
use crate::game::tests::game_with;
use crate::game::{Game, HostAction};
use crate::window::{FRAME, frametime_f32};

/// A game directory removed when the test ends.
struct Root(PathBuf);

impl Root {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("quakec-engine-demos-{name}-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(dir.join("id1")).unwrap();
        Self(dir)
    }

    /// The file `name` of the game directory, named as the host names it: the directory, a
    /// `/` and the name, on every platform (`sprintf (name, "%s/%s", com_gamedir, ...)`).
    fn file(&self, name: &str) -> PathBuf {
        let mut path = self.0.join("id1").into_os_string();
        path.push("/");
        path.push(name);
        PathBuf::from(path)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _gone = std::fs::remove_dir_all(&self.0);
    }
}

/// Every model a brush's: the test map has only the world and the player.
struct Brushes;

impl Models for Brushes {
    fn describe(&mut self, _name: &str) -> Option<ModelInfo> {
        Some(ModelInfo::brush())
    }
}

struct NoRand;

impl Random for NoRand {
    fn rand(&mut self) -> i32 {
        0
    }
}

/// The host a demo test drives: the test game on its floor, the client that read its
/// connection, and the demos.
struct Host {
    root: Root,
    args: Args,
    game: Game,
    client: Client,
    demos: Demos,
    /// Whether each demo command of the last line said started a demo.
    started: Vec<Started>,
}

impl Host {
    /// The host with its client connected to the test game.
    fn connected(name: &str) -> Self {
        let mut host = Self::new(name);
        host.frame(0.0).unwrap();
        host.printed();
        host
    }

    /// The host with a client that has read nothing.
    fn new(name: &str) -> Self {
        let root = Root::new(name);
        let args =
            Args::parse_words(["--basedir".to_owned(), root.0.display().to_string()]).unwrap();
        let (game, _session) = game_with("");
        Self {
            root,
            args,
            game,
            client: Client::new(qs_client::Game::Standard),
            demos: Demos::new(),
            started: Vec::new(),
        }
    }

    /// The client's frame at the host's time `realtime`, over the server's packets.
    fn frame(&mut self, realtime: f64) -> Result<Vec<Event>, DemoFrameError> {
        let packets = self
            .game
            .world_mut()
            .take_messages(qs_sim::ClientId::FIRST)
            .packets;
        self.demos.frame(
            &mut self.client,
            (frametime_f32(FRAME), realtime, &packets),
            (&mut Brushes, &mut NoRand),
            &mut self.game,
        )
    }

    /// `line` typed at the console and run by the next server frame, its demo commands
    /// given to the demos; what the console then shows.
    fn say(&mut self, line: &str) -> Vec<String> {
        self.game.world_mut().console(&format!("{line}\n"));
        self.game.frame(FRAME);
        let dir = self.root.0.join("id1");
        self.started.clear();
        for command in self.game.world_mut().take_client_commands() {
            let mut host = DemoHost {
                args: &self.args,
                game: &mut self.game,
                client: &mut self.client,
                read: &|name| std::fs::read(dir.join(name)).ok(),
            };
            let started = self.demos.command(&command, &mut host);
            self.started.push(started);
        }
        self.printed()
    }

    /// The lines the console has been given since the last look.
    fn printed(&mut self) -> Vec<String> {
        self.game
            .take_printed()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect()
    }

    /// The map the host was asked to start, and how, once the console's `map` has run.
    fn asked(&mut self) -> Option<(String, Connection)> {
        self.game.frame(FRAME);
        match self.game.host_action()? {
            HostAction::Level(level) => Some((level.map, level.connection)),
            HostAction::Spawned(_)
            | HostAction::Save(_)
            | HostAction::Load(_)
            | HostAction::Quit => panic!("not a level"),
        }
    }

    /// Records `frames` frames of the game as `name` and stops.
    fn recorded(&mut self, name: &str, frames: usize) {
        self.say(&format!("record {name}"));
        for _ in 0..frames {
            self.game.frame(FRAME);
            self.frame(0.0).unwrap();
        }
        self.say("stop");
    }
}

// spec CLIENT-10.3: a demo's name is given `.dem` unless that is its extension already.
#[test]
fn a_demo_name_is_given_its_extension() {
    assert_eq!(with_extension("run"), "run.dem");
    assert_eq!(with_extension("run.dem"), "run.dem");
    assert_eq!(with_extension("run.txt"), "run.txt.dem");
    assert_eq!(with_extension("my.demos/run"), "my.demos/run.dem");
    assert_eq!(with_extension("dem"), "dem.dem");
}

// spec CLIENT-10.5: `stop` with nothing recording says so.
#[test]
fn stop_with_no_recording_says_so() {
    let mut host = Host::connected("stop");
    assert_eq!(host.say("stop"), ["Not recording a demo.\n"]);
}

// spec CLIENT-10.3: `record` refuses a wrong count of words, a name with `..`, and a client
// not yet connected that names no map.
#[test]
fn record_refuses_what_it_cannot_record() {
    let mut host = Host::connected("refuse");
    assert_eq!(
        host.say("record"),
        ["record <demoname> [<map> [cd track]]\n"]
    );
    assert_eq!(
        host.say("record a b 2 c"),
        ["record <demoname> [<map> [cd track]]\n"]
    );
    assert_eq!(
        host.say("record ../a"),
        ["Relative pathnames are not allowed.\n"]
    );
    assert!(!host.demos.recorder.recording());

    let mut host = Host::new("unconnected");
    assert_eq!(
        host.say("record a"),
        ["Can't record - try again when connected\n"]
    );
    assert!(!host.root.file("a.dem").exists());
}

// spec CLIENT-10.3: `record` while connected opens `<name>.dem` in the game directory and
// writes the header, then the messages that made the connection.
// spec CLIENT-10.5: `stop` ends the file and says so.
#[test]
fn record_writes_a_file_and_stop_ends_it() {
    let mut host = Host::connected("record");
    let path = host.root.file("run.dem");
    assert_eq!(
        host.say("record run"),
        [format!("recording to {}.\n", path.display())]
    );
    assert!(host.demos.recorder.recording());
    host.game.frame(FRAME);
    host.frame(0.0).unwrap();
    assert_eq!(host.say("stop"), ["Completed demo\n"]);
    assert!(!host.demos.recorder.recording());
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"-1\n"));
    let demo = qs_client::Demo::open(bytes).unwrap();
    assert_eq!(demo.map().as_deref(), Some("test"));
}

// spec CLIENT-10.3: a second `record` stops the first, and a file that cannot be made is
// an error on the console, nothing recording.
#[test]
fn record_stops_the_last_and_reports_a_file_it_cannot_make() {
    let mut host = Host::connected("again");
    host.say("record one");
    let two = host.root.file("two.dem");
    assert_eq!(
        host.say("record two"),
        [
            "Completed demo\n".to_owned(),
            format!("recording to {}.\n", two.display())
        ]
    );
    let missing = host.root.file("none/three.dem");
    assert_eq!(
        host.say("record none/three"),
        [
            "Completed demo\n".to_owned(),
            format!("recording to {}.\n", missing.display()),
            format!("ERROR: couldn't create {}\n", missing.display()),
        ]
    );
    assert!(!host.demos.recorder.recording());
}

// spec CLIENT-10.3: `record <name> <map> <track>` says the track forced before, starts the
// map as a new game and records it from its first message with the track in the header,
// beside its name until the map has loaded; a map that does not load records nothing and
// leaves a demo of that name as it was.
#[test]
fn record_with_a_map_starts_the_map_and_records_it() {
    let mut host = Host::connected("map");
    assert_eq!(host.say("record run test 3"), ["Forcing CD track to -1\n"]);
    assert!(!host.demos.recorder.recording());
    assert_eq!(host.asked(), Some(("test".to_owned(), Connection::New)));
    let path = host.root.file("run.dem");
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    assert_eq!(
        host.printed(),
        [format!("recording to {}.\n", path.display())]
    );
    assert!(host.demos.recorder.recording());
    let beside = host.root.file("run.dem.loading");
    assert!(beside.exists() && !path.exists());
    host.demos
        .after_level(Level::Loaded(&mut host.client), &mut host.game);
    assert!(host.demos.recorder.recording());
    assert!(path.exists() && !beside.exists());
    host.say("stop");
    assert!(std::fs::read(&path).unwrap().starts_with(b"3\n"));
    // The track forced is the one the next `record` names as the one before.
    assert_eq!(host.say("record run test 4"), ["Forcing CD track to 3\n"]);
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    host.demos
        .after_level(Level::Stayed(&mut host.client), &mut host.game);
    assert!(!host.demos.recorder.recording());
    assert!(!beside.exists());
    assert!(
        std::fs::read(&path).unwrap().starts_with(b"3\n"),
        "the demo there was is as it was"
    );
    assert!(host.demos.waiting.is_none());
}

// spec CLIENT-10.3: a new game ends a demo being recorded, and the next level of the game
// being played goes on being recorded.
#[test]
fn a_new_game_ends_a_recording_and_the_next_level_keeps_it() {
    let mut host = Host::connected("levels");
    host.say("record run");
    host.demos
        .before_level(Connection::Changed, &mut host.game, &host.client);
    host.demos
        .after_level(Level::Loaded(&mut host.client), &mut host.game);
    assert!(host.demos.recorder.recording());
    assert_eq!(host.printed(), Vec::<String>::new());
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    assert!(!host.demos.recorder.recording());
    assert_eq!(host.printed(), ["Completed demo\n"]);
}

// spec CLIENT-10.8: `playdemo` and `timedemo` say their use, and a demo that is not there,
// does not read or names no map is an error on the console, nothing played.
#[test]
fn a_demo_that_cannot_be_played_is_reported() {
    let mut host = Host::connected("bad");
    assert_eq!(
        host.say("playdemo"),
        ["playdemo <demoname> : plays a demo\n"]
    );
    assert_eq!(
        host.say("timedemo a b"),
        ["timedemo <demoname> : gets demo speeds\n"]
    );
    assert_eq!(
        host.say("playdemo none"),
        [
            "Playing demo from none.dem.\n",
            "ERROR: couldn't open none.dem\n"
        ]
    );
    std::fs::write(host.root.file("bad.dem"), b"x\n").unwrap();
    assert_eq!(
        host.say("playdemo bad"),
        [
            "Playing demo from bad.dem.\n",
            "ERROR: demo \"bad.dem\" is invalid\n"
        ]
    );
    std::fs::write(host.root.file("empty.dem"), b"-1\n").unwrap();
    assert_eq!(
        host.say("playdemo empty"),
        [
            "Playing demo from empty.dem.\n",
            "ERROR: demo \"empty.dem\" names no map\n"
        ]
    );
    assert!(!host.demos.playing());
    assert_eq!(host.asked(), None);
}

// spec CLIENT-10.3: nothing is recorded while a demo plays.
// spec CLIENT-10.8: `playdemo` ends a recording, starts the demo's map as a new game and
// plays the demo over it; when the demo ends the map played before starts again.
#[test]
fn a_demo_plays_over_its_map_and_the_last_map_comes_back() {
    let mut host = Host::connected("play");
    host.recorded("run", 3);
    host.say("record other");
    assert_eq!(
        host.say("playdemo run"),
        ["Completed demo\n", "Playing demo from run.dem.\n"]
    );
    // The map is loading: the server's packets are still the client's.
    assert!(!host.demos.playing());
    assert_eq!(host.asked(), Some(("test".to_owned(), Connection::New)));
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    host.demos
        .after_level(Level::Loaded(&mut host.client), &mut host.game);
    assert!(host.demos.playing());
    assert_eq!(host.client.signon(), 0);
    assert_eq!(
        host.say("record more"),
        ["Can't record during demo playback\n"]
    );
    let mut frames = 0;
    while host.demos.playing() {
        host.frame(0.0).unwrap();
        frames += 1;
        assert!(frames < 100, "the demo does not end");
    }
    assert!(!host.client.demo_playing());
    // `playdemo` says no speed.
    assert_eq!(host.printed(), Vec::<String>::new());
    assert_eq!(host.asked(), Some(("test".to_owned(), Connection::New)));
}

// spec CLIENT-10.8: a demo whose map does not load is over, and says so.
#[test]
fn a_demo_whose_map_does_not_load_is_over() {
    let mut host = Host::connected("nomap");
    host.recorded("run", 1);
    host.say("playdemo run");
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    host.demos
        .after_level(Level::Stayed(&mut host.client), &mut host.game);
    assert!(!host.demos.playing());
    assert_eq!(host.printed(), ["ERROR: the demo's map did not load\n"]);
}

// spec CLIENT-10.9: `timedemo` plays a message a frame and says the frames after the first,
// the seconds from the second frame by the host's clock, and their rate.
#[test]
fn timedemo_says_its_speed() {
    let mut host = Host::connected("time");
    host.recorded("run", 3);
    host.say("timedemo run");
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    host.demos
        .after_level(Level::Loaded(&mut host.client), &mut host.game);
    let mut realtime = 10.0;
    let mut frames = 0;
    while host.demos.playing() {
        host.frame(realtime).unwrap();
        realtime += 0.5;
        frames += 1;
        assert!(frames < 100, "the demo does not end");
    }
    assert_eq!(frames, 5);
    assert_eq!(host.printed(), ["4 frames   1.5 seconds   2.7 fps\n"]);
}

// spec CLIENT-10.9: a demo timed in no time is said to have taken a second.
#[test]
fn timedemo_in_no_time_took_a_second() {
    let mut host = Host::connected("instant");
    host.recorded("run", 3);
    host.say("timedemo run");
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    host.demos
        .after_level(Level::Loaded(&mut host.client), &mut host.game);
    while host.demos.playing() {
        host.frame(7.0).unwrap();
    }
    assert_eq!(host.printed(), ["4 frames   1.0 seconds   4.0 fps\n"]);
}

// spec CLIENT-10.4: a demo whose file ends inside a message stops playing there, with no
// error, and the map played before starts again.
#[test]
fn a_demo_cut_short_stops() {
    let mut host = Host::connected("cut");
    host.recorded("run", 3);
    let path = host.root.file("run.dem");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.truncate(bytes.len() - 5);
    std::fs::write(&path, bytes).unwrap();
    host.say("playdemo run");
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    host.demos
        .after_level(Level::Loaded(&mut host.client), &mut host.game);
    let mut frames = 0;
    while host.demos.playing() {
        host.frame(0.0).unwrap();
        frames += 1;
        assert!(frames < 100, "the demo does not end");
    }
    assert_eq!(host.asked(), Some(("test".to_owned(), Connection::New)));
}

// spec CLIENT-10.4: a message longer than a message may be stops the demo with the error,
// and the map played before starts again.
#[test]
fn a_demo_with_a_message_too_long_stops_with_the_error() {
    let mut host = Host::connected("long");
    host.recorded("run", 3);
    let path = host.root.file("run.dem");
    let mut bytes = std::fs::read(&path).unwrap();
    // The second message's length: after the header's line and the first message, which is
    // its length, three angles and its bytes.
    let first = usize::try_from(i32::from_le_bytes(bytes[3..7].try_into().unwrap())).unwrap();
    let second = 3 + 4 + 12 + first;
    bytes[second..second + 4].copy_from_slice(&i32::MAX.to_le_bytes());
    std::fs::write(&path, bytes).unwrap();
    host.say("playdemo run");
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    host.demos
        .after_level(Level::Loaded(&mut host.client), &mut host.game);
    let mut failed = None;
    for _ in 0..100 {
        if !host.demos.playing() {
            break;
        }
        failed = host.frame(0.0).err();
    }
    assert!(matches!(
        failed,
        Some(DemoFrameError::Demo(qs_client::DemoError::TooLong))
    ));
    assert!(!host.demos.playing());
    assert_eq!(host.asked(), Some(("test".to_owned(), Connection::New)));
}

// spec CLIENT-10.6: the console's commands run while a demo plays, the server standing still.
// spec CLIENT-10.10: `pause` holds a playing demo and lets it go, the client paused with
// it, and any other command for the server is dropped.
#[test]
fn pause_holds_a_playing_demo() {
    let mut host = Host::connected("pause");
    host.recorded("run", 6);
    host.say("playdemo run");
    // The host starts the demo's map.
    host.asked();
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    host.demos
        .after_level(Level::Loaded(&mut host.client), &mut host.game);
    host.frame(0.0).unwrap();
    let time = host.game.world().time();

    host.game.world_mut().console("pause; god\n");
    Demos::console_frame(&mut host.game, &mut host.client);
    assert!(host.client.paused());
    // The server did not run, and nothing waits for it.
    assert_eq!(host.game.world().time().to_bits(), time.to_bits());
    assert!(host.game.world_mut().take_client_commands_sent().is_empty());
    for _ in 0..20 {
        host.frame(0.0).unwrap();
    }
    assert!(host.demos.playing());

    host.game.world_mut().console("pause\n");
    Demos::console_frame(&mut host.game, &mut host.client);
    assert!(!host.client.paused());
    let mut frames = 0;
    while host.demos.playing() {
        host.frame(0.0).unwrap();
        frames += 1;
        assert!(frames < 100, "the demo does not end");
    }
}

// spec CLIENT-10.8: a command typed while a demo plays that starts a map stops the demo.
#[test]
fn a_map_started_while_a_demo_plays_stops_it() {
    let mut host = Host::connected("leave");
    host.recorded("run", 6);
    host.say("playdemo run");
    // The host starts the demo's map.
    host.asked();
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    host.demos
        .after_level(Level::Loaded(&mut host.client), &mut host.game);
    host.frame(0.0).unwrap();
    host.game.world_mut().console("map test\n");
    Demos::console_frame(&mut host.game, &mut host.client);
    assert!(matches!(
        host.game.host_action(),
        Some(HostAction::Level(_))
    ));
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    // A map that does not load leaves the demo playing, with its client.
    host.demos
        .after_level(Level::Stayed(&mut host.client), &mut host.game);
    assert!(host.demos.playing());
    assert!(host.client.demo_playing());
    assert_eq!(host.demos.showing, Showing::Demo);
    // One that loads is the game's: the demo is over.
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    host.demos
        .after_level(Level::Loaded(&mut host.client), &mut host.game);
    assert!(!host.demos.playing());
    assert_eq!(host.demos.showing, Showing::Game);
}

// spec CLIENT-10.6: while a demo plays `restart` does nothing and `changelevel` says only
// the server may, and a new game is not refused; with no demo playing none is refused
#[test]
fn no_level_follows_the_one_a_demo_plays_over() {
    let mut host = Host::connected("levels-refused");
    for connection in [Connection::New, Connection::Changed, Connection::Restarted] {
        assert!(!host.demos.refuses(connection, &mut host.game));
    }
    host.recorded("run", 3);
    host.say("playdemo run");
    host.asked();
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    host.demos
        .after_level(Level::Loaded(&mut host.client), &mut host.game);
    host.printed();
    assert!(host.demos.refuses(Connection::Restarted, &mut host.game));
    assert_eq!(host.printed(), Vec::<String>::new());
    assert!(host.demos.refuses(Connection::Changed, &mut host.game));
    assert_eq!(host.printed(), ["Only the server may changelevel\n"]);
    assert!(!host.demos.refuses(Connection::New, &mut host.game));
    assert!(host.demos.playing());
}

// spec CLIENT-10.3: with no map named, a client connected to signon stage 2 may record and
// one before it may not
#[test]
fn record_waits_for_the_second_signon_stage() {
    let mut host = Host::new("signon");
    let packets = host
        .game
        .world_mut()
        .take_messages(qs_sim::ClientId::FIRST)
        .packets;
    let mut stages = Vec::new();
    for packet in &packets {
        host.client
            .frame(
                0.0,
                core::slice::from_ref(packet),
                &mut Brushes,
                &mut NoRand,
            )
            .unwrap();
        stages.push(host.client.signon());
        if host.client.signon() == 1 {
            assert_eq!(
                host.say("record early"),
                ["Can't record - try again when connected\n"]
            );
        }
        if host.client.signon() == 2 {
            break;
        }
    }
    assert_eq!(host.client.signon(), 2, "{stages:?}");
    let path = host.root.file("ready.dem");
    assert_eq!(
        host.say("record ready"),
        [format!("recording to {}.\n", path.display())]
    );
}

// spec CLIENT-10.8: a demo that does not start while another plays over another map leaves
// the host on the map played before the first
#[test]
fn a_demo_that_does_not_start_goes_back_to_the_map_before() {
    let mut host = Host::connected("back");
    host.recorded("run", 3);
    host.say("playdemo run");
    host.asked();
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    host.demos
        .after_level(Level::Loaded(&mut host.client), &mut host.game);
    // As when the first demo was started from another map than its own.
    host.demos.playing.as_mut().unwrap().back = "elsewhere".to_owned();
    host.game.world_mut().console("playdemo none\n");
    Demos::console_frame(&mut host.game, &mut host.client);
    for command in host.game.world_mut().take_client_commands() {
        let mut demo_host = DemoHost {
            args: &host.args,
            game: &mut host.game,
            client: &mut host.client,
            read: &|_| None,
        };
        host.demos.command(&command, &mut demo_host);
    }
    assert!(!host.demos.playing());
    assert_eq!(
        host.printed(),
        [
            "Playing demo from none.dem.\n",
            "ERROR: couldn't open none.dem\n"
        ]
    );
    Demos::console_frame(&mut host.game, &mut host.client);
    match host.game.host_action() {
        Some(HostAction::Level(level)) => assert_eq!(level.map, "elsewhere"),
        other => panic!("{:?}", other.is_some()),
    }
}

impl Host {
    /// Loads the map a demo command asked for and lets the demo play over it.
    fn starts_playing(&mut self) {
        assert_eq!(self.asked(), Some(("test".to_owned(), Connection::New)));
        self.demos
            .before_level(Connection::New, &mut self.game, &self.client);
        self.demos
            .after_level(Level::Loaded(&mut self.client), &mut self.game);
        assert!(self.demos.playing());
    }

    /// Plays the demo to its end.
    fn plays_out(&mut self) {
        let mut frames = 0_u32;
        while self.demos.playing() {
            self.frame(0.0).unwrap();
            frames = frames.saturating_add(1);
            assert!(frames < 100, "the demo does not end");
        }
    }
}

// spec CLIENT-10.11: `startdemos` lists at most eight demos, each name kept to fifteen
// bytes, says how many, and starts no loop while the host's server runs
#[test]
fn startdemos_lists_the_loops_demos() {
    let mut host = Host::connected("list");
    assert_eq!(host.say("startdemos one two"), ["2 demo(s) in loop\n"]);
    assert_eq!(host.demos.listed[..3], ["one", "two", ""]);
    assert_eq!(host.demos.next, None);
    assert!(!host.demos.playing());
    assert_eq!(host.asked(), None);
    // A name past the words given keeps what it was.
    assert_eq!(
        host.say("startdemos a_name_of_twenty_byte"),
        ["1 demo(s) in loop\n"]
    );
    assert_eq!(host.demos.listed[..3], ["a_name_of_twent", "two", ""]);
    assert_eq!(
        host.say("startdemos 1 2 3 4 5 6 7 8 9"),
        ["Max 8 demos in demoloop\n", "8 demo(s) in loop\n"]
    );
    assert_eq!(host.demos.listed[7], "8");
    assert_eq!(host.say("startdemos"), ["0 demo(s) in loop\n"]);
    assert_eq!(host.demos.listed[0], "1");
}

// spec CLIENT-10.12: `demos` starts the loop at its second demo, each demo that ends is
// followed by the next, the list going round, and the map played before the first is kept
// to go back to; with no demo listed it says so
#[test]
fn demos_plays_the_loop_round() {
    let mut host = Host::connected("loop");
    assert_eq!(host.say("demos"), ["No demos listed with startdemos\n"]);
    assert_eq!(host.demos.next, None);
    host.recorded("one", 2);
    host.recorded("two", 2);
    host.say("startdemos one two");
    assert_eq!(host.say("demos"), ["Playing demo from two.dem.\n"]);
    host.starts_playing();
    host.demos.playing.as_mut().unwrap().back = "before".to_owned();
    host.plays_out();
    // The list's third name is empty: the loop goes round to the first.
    assert_eq!(host.say(""), ["Playing demo from one.dem.\n"]);
    host.starts_playing();
    assert_eq!(host.demos.playing.as_ref().unwrap().back, "before");
    host.plays_out();
    assert_eq!(host.say(""), ["Playing demo from two.dem.\n"]);
    host.starts_playing();
    // `demos` while one plays goes on to the next.
    assert_eq!(host.say("demos"), ["Playing demo from one.dem.\n"]);
    assert_eq!(host.demos.playing.as_ref().unwrap().back, "before");
}

// spec CLIENT-10.12: a timed demo is followed by none, a demo that does not open ends the
// loop, and so does a new game
#[test]
fn what_ends_the_loop() {
    let mut host = Host::connected("ends");
    host.recorded("one", 2);
    host.say("startdemos one one");
    host.say("demos");
    host.starts_playing();
    host.say("timedemo one");
    host.starts_playing();
    host.plays_out();
    let said = host.printed();
    assert!(said[0].ends_with("fps\n"), "{said:?}");
    assert_eq!(host.say(""), Vec::<String>::new(), "no demo follows");

    let mut host = Host::connected("ends-open");
    host.say("startdemos none");
    assert_eq!(
        host.say("demos"),
        [
            "Playing demo from none.dem.\n",
            "ERROR: couldn't open none.dem\n"
        ]
    );
    assert_eq!(host.demos.next, None);

    let mut host = Host::connected("ends-map");
    host.recorded("one", 2);
    host.say("startdemos one one");
    host.say("demos");
    host.starts_playing();
    assert!(host.demos.next.is_some());
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    assert_eq!(host.demos.next, None);
    host.demos
        .after_level(Level::Loaded(&mut host.client), &mut host.game);
    assert!(!host.demos.playing());
}

// spec CLIENT-10.13: `stopdemo` stops a demo being played, and the map played before
// starts again; with none playing it does nothing
#[test]
fn stopdemo_stops_a_playing_demo() {
    let mut host = Host::connected("stopdemo");
    assert_eq!(host.say("stopdemo"), Vec::<String>::new());
    assert_eq!(host.asked(), None);
    host.recorded("one", 4);
    host.say("playdemo one");
    host.starts_playing();
    host.frame(0.0).unwrap();
    assert!(host.client.demo_playing());
    assert_eq!(host.say("stopdemo"), Vec::<String>::new());
    assert!(!host.demos.playing());
    assert!(!host.client.demo_playing());
    assert_eq!(host.asked(), Some(("test".to_owned(), Connection::New)));
}

// spec CLIENT-10.8: a demo that does not start in the place of one playing, or whose map
// does not load, leaves the demo's map: its client stops playing and the map played before
// starts again, though it has the demo's own name; one that fails over a game leaves the
// game alone
#[test]
fn a_demo_that_fails_over_a_demo_leaves_it() {
    let mut host = Host::connected("fails");
    // Over a game: nothing is asked of the host, and the game's connection is still kept
    // for a recording.
    host.say("playdemo none");
    assert_eq!(host.started, [Started::Nothing]);
    assert_eq!(host.asked(), None);

    host.recorded("run", 6);
    host.say("playdemo run");
    assert_eq!(host.started, [Started::Demo]);
    host.starts_playing();
    host.frame(0.0).unwrap();
    assert!(host.client.demo_playing());
    host.game.world_mut().console("playdemo none\n");
    Demos::console_frame(&mut host.game, &mut host.client);
    let commands = host.game.world_mut().take_client_commands();
    for command in commands {
        let mut demo_host = DemoHost {
            args: &host.args,
            game: &mut host.game,
            client: &mut host.client,
            read: &|_| None,
        };
        assert_eq!(
            host.demos.command(&command, &mut demo_host),
            Started::Nothing
        );
    }
    assert!(!host.demos.playing());
    assert!(!host.client.demo_playing());
    assert_eq!(host.asked(), Some(("test".to_owned(), Connection::New)));

    // The next demo's map does not load while the last demo's is shown.
    let mut host = Host::connected("fails-load");
    host.recorded("run", 2);
    host.say("playdemo run");
    host.starts_playing();
    host.frame(0.0).unwrap();
    host.say("playdemo run");
    host.asked();
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    host.demos
        .after_level(Level::Stayed(&mut host.client), &mut host.game);
    assert_eq!(host.printed(), ["ERROR: the demo's map did not load\n"]);
    assert!(!host.demos.playing());
    assert!(!host.client.demo_playing());
    assert_eq!(host.asked(), Some(("test".to_owned(), Connection::New)));
}

// spec CLIENT-10.8: a demo that goes on to a level of another map has that map loaded, and
// reads the level again from its serverinfo with the client the map is given
#[test]
fn a_demo_that_changes_level_has_the_next_map_loaded() {
    let mut host = Host::connected("levels-in");
    host.recorded("run", 3);
    // The demo's level is of another map than the one the host can load here.
    let path = host.root.file("run.dem");
    let bytes = std::fs::read(&path).unwrap();
    let at = bytes
        .windows(13)
        .position(|window| window == b"maps/test.bsp")
        .unwrap();
    let mut other = bytes;
    other[at..at + 13].copy_from_slice(b"maps/tesu.bsp");
    std::fs::write(&path, other).unwrap();
    host.say("playdemo run");
    assert_eq!(host.asked(), Some(("tesu".to_owned(), Connection::New)));
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    // The host's world is still `test`, as when a level of the demo names another map.
    host.demos
        .after_level(Level::Loaded(&mut host.client), &mut host.game);
    assert!(host.demos.playing());
    assert_eq!(host.frame(0.0).unwrap(), Vec::new());
    assert!(!host.demos.playing(), "the map is loading");
    assert_eq!(host.asked(), Some(("tesu".to_owned(), Connection::New)));
    // The level is read again from its serverinfo.
    let playing = host.demos.playing.as_ref().unwrap();
    assert_eq!(playing.demo.map().as_deref(), Some("tesu"));
    assert_eq!(playing.demo.position(), 3);
}

// spec CLIENT-10.6: while the game is paused a playing demo stands still
#[test]
fn a_paused_game_holds_a_playing_demo() {
    let mut host = Host::connected("paused");
    host.recorded("run", 3);
    host.say("timedemo run");
    host.starts_playing();
    host.frame(0.0).unwrap();
    host.game.start_paused();
    for _ in 0..40 {
        assert!(host.frame(0.0).unwrap().is_empty());
    }
    assert!(host.demos.playing());
}

// spec CLIENT-10.1: the CD track a demo being recorded or played forces is the one played,
// in the place of the level's own
#[test]
fn a_forced_track_is_the_one_played() {
    let track = |events: &[Event]| -> Vec<i32> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::CdTrack { track, .. } => Some(*track),
                _ => None,
            })
            .collect()
    };
    let mut host = Host::new("track");
    host.say("record run test 5");
    host.asked();
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    let events = host.frame(0.0).unwrap();
    assert_eq!(track(&events), [5], "recording");
    host.demos
        .after_level(Level::Loaded(&mut host.client), &mut host.game);
    host.game.frame(FRAME);
    host.frame(0.0).unwrap();
    host.say("stop");
    // Played, the demo's header forces it again.
    host.say("playdemo run");
    host.starts_playing();
    let mut played = Vec::new();
    while host.demos.playing() {
        played.extend(track(&host.frame(0.0).unwrap()));
        assert!(played.len() < 100, "the demo does not end");
    }
    assert_eq!(played, [5], "playing");

    // With none forced the level's own is played.
    let mut host = Host::new("no-track");
    let events = host.frame(0.0).unwrap();
    let own = track(&events);
    assert_eq!(own.len(), 1);
    assert_ne!(own, [5]);
    // So it is while a recording forces none, and when a track was forced by a demo that
    // is over.
    let mut host = Host::new("record-no-track");
    host.say("record run test");
    host.asked();
    host.demos
        .before_level(Connection::New, &mut host.game, &host.client);
    assert_eq!(track(&host.frame(0.0).unwrap()), own);
    let mut host = Host::new("track-over");
    host.demos.forcetrack = 5;
    assert_eq!(track(&host.frame(0.0).unwrap()), own);
}
