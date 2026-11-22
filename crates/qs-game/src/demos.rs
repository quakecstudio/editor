// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-2001 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2007-2008 Kristian Duske
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/cl_demo.c, CL_StopPlayback, CL_Stop_f, CL_Record_f, CL_PlayDemo_f, CL_FinishTimeDemo, CL_TimeDemo_f
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/host_cmd.c, Host_Pause_f, Host_Restart_f, Host_Changelevel_f, Host_Startdemos_f, Host_Demos_f, Host_Stopdemo_f
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/cl_main.c, CL_NextDemo
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/cmd.c, Cmd_ForwardToServer

//! The host's demo commands (CLIENT-10): `record`, `stop`, `playdemo` and `timedemo`, over
//! `qs-client`'s recorder and playing client. The host has no state without a map, so a
//! demo plays over the map it is of, loaded with its server standing still, and the map
//! played before it is started again when it ends.

use std::path::{Path, PathBuf};

use qs_client::{
    Client, Demo, DemoError, DemoFrameError, DemoPace, DemoRecorder, DemoState, DemoTrack, Event,
    Models, Random,
};
use qs_sim::ClientCommand;

use crate::args::Args;
use crate::game::Game;

/// The stage of the signon from which a recording may start while connected,
/// "`if (cls.signon < 2)`" it may not (CLIENT-10.3).
const RECORD_FROM_SIGNON: i32 = 2;
/// The words of `record`: the command and a name, with a map, with a map and a track.
const RECORD_WORDS: core::ops::RangeInclusive<usize> = 2..=4;
/// Where `record`'s words have the map and the CD track.
const RECORD_MAP: usize = 2;
const RECORD_TRACK: usize = 3;
/// The words of `playdemo` and `timedemo`: the command and a name.
const PLAY_WORDS: usize = 2;
/// `MAX_DEMOS` of `client.h`: the demos a loop holds (CLIENT-10.11).
const MAX_DEMOS: usize = 8;
/// The bytes of a loop's demo name kept: `MAX_DEMONAME` of `client.h`, 16, less the NUL.
const MAX_DEMO_NAME: usize = 15;
/// Where `demos` starts a loop that is not running: "`if (cls.demonum == -1) cls.demonum =
/// 1;`".
const LOOP_START: usize = 1;
/// The command that holds a playing demo.
const PAUSE: &[u8] = b"pause";
/// What a recording that waits for its map is written as, after its name.
const UNLOADED: &str = ".loading";
/// The extension a demo's name is given when it has another or none.
const DEMO_EXTENSION: &str = "dem";

/// Whether a level is a new game's, as `map` starts one, the client disconnected first, or
/// the next of the one being played, as `changelevel` and `restart` go on to it, the
/// client reconnecting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Connection {
    /// `Host_Map_f`: "`CL_Disconnect ();`" first.
    New,
    /// `Host_Changelevel_f`.
    Changed,
    /// `Host_Restart_f`.
    Restarted,
}

/// A recording that starts with the map `record` named. It is written beside its name
/// from the map's first message and takes the name once the map has loaded, so a map that
/// does not load costs no demo already there.
#[derive(Debug)]
struct Waiting {
    path: PathBuf,
    track: DemoTrack,
    /// Whether the file beside the name is open and written.
    started: bool,
}

/// Whose world the host has loaded: a game's, or the map a demo plays over, which a demo
/// that stops must leave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Showing {
    /// A game's map, with the game's client.
    Game,
    /// A demo's map, with the demo's client.
    Demo,
}

/// How a level asked for came out, with the client it left the host.
pub(crate) enum Level<'a> {
    /// It loaded: the new level's client.
    Loaded(&'a mut Client),
    /// It did not: the client there was.
    Stayed(&'a mut Client),
}

/// Whether a demo command started a demo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Started {
    /// It did: "get rid of the menu and/or console".
    Demo,
    /// It did not.
    Nothing,
}

/// Where a demo being played is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Its map is being loaded.
    Loading,
    /// Its messages are being read.
    Playing,
}

/// A demo being played.
#[derive(Debug)]
struct Playing {
    demo: Demo,
    pace: DemoPace,
    stage: Stage,
    /// The map played before it, started again when it ends.
    back: String,
    /// The frames it has played, and the host's time when its second began: `timedemo`'s
    /// count and clock.
    frames: u32,
    second: Option<f64>,
}

/// The host's demos: one being recorded, one being played.
#[derive(Debug)]
pub(crate) struct Demos {
    recorder: DemoRecorder,
    /// `cls.forcetrack`.
    forcetrack: i32,
    waiting: Option<Waiting>,
    playing: Option<Playing>,
    /// `cls.demos`: the names `startdemos` gave, an empty one where there is none.
    listed: [String; MAX_DEMOS],
    /// `cls.demonum`: the demo of the loop to play next, none when no loop runs.
    next: Option<usize>,
    /// The map to go back to after a loop's demos, kept from one demo to the next.
    resume: Option<String>,
    showing: Showing,
}

/// What the demo commands work on.
pub(crate) struct DemoHost<'a> {
    pub(crate) args: &'a Args,
    pub(crate) game: &'a mut Game,
    pub(crate) client: &'a mut Client,
    /// A file of the game's search path by name.
    pub(crate) read: &'a dyn Fn(&str) -> Option<Vec<u8>>,
}

/// `name` with `.dem` after it unless that is its extension already, `COM_AddExtension`.
fn with_extension(name: &str) -> String {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let has = file
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extension == DEMO_EXTENSION);
    if has {
        name.to_owned()
    } else {
        format!("{name}.{DEMO_EXTENSION}")
    }
}

/// The file a recording that waits for the map of `path` is written as.
fn unloaded(path: &Path) -> PathBuf {
    let mut file = path.as_os_str().to_owned();
    file.push(UNLOADED);
    PathBuf::from(file)
}

/// The word `place` of a command, as text.
fn word(words: &[Vec<u8>], place: usize) -> String {
    words
        .get(place)
        .map_or_default(|word| String::from_utf8_lossy(word).into_owned())
}

impl Demos {
    /// No demo recorded or played.
    pub(crate) const fn new() -> Self {
        Self {
            recorder: DemoRecorder::new(),
            forcetrack: qs_client::DEMO_NO_TRACK,
            waiting: None,
            playing: None,
            listed: [const { String::new() }; MAX_DEMOS],
            next: None,
            resume: None,
            showing: Showing::Game,
        }
    }

    /// Whether a demo's messages are being read in the server's place, `cls.demoplayback`.
    pub(crate) fn playing(&self) -> bool {
        self.playing
            .as_ref()
            .is_some_and(|playing| playing.stage == Stage::Playing)
    }

    /// The host's frame while a demo plays, in the server's place: the console's buffer
    /// runs, and of what it forwards to the server `pause` holds the demo or lets it go and
    /// the rest is dropped, "`if (cls.demoplayback) return; // not really connected`".
    ///
    /// Implements CLIENT-10.10.
    pub(crate) fn console_frame(game: &mut Game, client: &mut Client) {
        for command in game.console_frame() {
            let name = command
                .split(|byte| byte.is_ascii_whitespace())
                .next()
                .unwrap_or_default();
            if name.eq_ignore_ascii_case(PAUSE) {
                client.pause_demo();
            }
        }
    }

    /// The recorder, which a map's loader gives the connection's first messages.
    pub(crate) const fn recorder(&mut self) -> &mut DemoRecorder {
        &mut self.recorder
    }

    /// Runs a demo command the console met, and says whether it started a demo; any other
    /// client command is not this module's.
    pub(crate) fn command(&mut self, command: &ClientCommand, host: &mut DemoHost<'_>) -> Started {
        match command {
            ClientCommand::Record(words) => self.record(words, host),
            ClientCommand::StopDemo => self.stop(host.game, host.client),
            ClientCommand::PlayDemo(words) => return self.play(words, DemoPace::Timed, host),
            ClientCommand::TimeDemo(words) => {
                return self.play(words, DemoPace::EveryFrame, host);
            }
            ClientCommand::StartDemos(words) => self.start_demos(words, host.game),
            ClientCommand::DemoLoop => return self.demo_loop(host),
            ClientCommand::StopPlayback => self.stop_playback(host),
            ClientCommand::ToggleConsole
            | ClientCommand::Clear
            | ClientCommand::ToggleMenu
            | ClientCommand::MenuMain
            | ClientCommand::MenuSinglePlayer
            | ClientCommand::MenuLoad
            | ClientCommand::MenuSave
            | ClientCommand::MenuOptions
            | ClientCommand::Help
            | ClientCommand::MenuQuit
            | ClientCommand::Bind(_)
            | ClientCommand::Unbind(_)
            | ClientCommand::UnbindAll
            | ClientCommand::Impulse(_)
            | ClientCommand::Press(..)
            | ClientCommand::Release(..) => {}
        }
        Started::Nothing
    }

    /// `CL_Stop_f`.
    ///
    /// Implements CLIENT-10.5.
    fn stop(&mut self, game: &mut Game, client: &Client) {
        if !self.recorder.recording() {
            game.console_line("Not recording a demo.\n");
            return;
        }
        if let Err(error) = self.recorder.stop(client.viewangles()) {
            tracing::warn!(%error, "the demo's last message was not written");
        }
        self.waiting = None;
        game.console_line("Completed demo\n");
    }

    /// `CL_Record_f`.
    ///
    /// Implements CLIENT-10.3.
    fn record(&mut self, words: &[Vec<u8>], host: &mut DemoHost<'_>) {
        if self.playing() {
            host.game
                .console_line("Can't record during demo playback\n");
            return;
        }
        if self.recorder.recording() {
            self.stop(host.game, host.client);
        }
        if !RECORD_WORDS.contains(&words.len()) {
            host.game
                .console_line("record <demoname> [<map> [cd track]]\n");
            return;
        }
        let name = word(words, 1);
        if name.contains("..") {
            host.game
                .console_line("Relative pathnames are not allowed.\n");
            return;
        }
        let map = words.get(RECORD_MAP).map(|_| word(words, RECORD_MAP));
        if map.is_none() && host.client.signon() < RECORD_FROM_SIGNON {
            host.game
                .console_line("Can't record - try again when connected\n");
            return;
        }
        // "write the forced cd track number, or -1": the track said is the one before.
        let track = match words.get(RECORD_TRACK) {
            Some(track) => {
                let said = format!("Forcing CD track to {}\n", self.forcetrack);
                host.game.console_line(&said);
                DemoTrack::Forced(qs_sim::atoi(track))
            }
            None => DemoTrack::None,
        };
        let path = host.args.demo_path(&with_extension(&name));
        match map {
            // "start the map up": the file is opened when the map has loaded.
            Some(map) => {
                self.waiting = Some(Waiting {
                    path,
                    track,
                    started: false,
                });
                host.game.world_mut().console(&format!("map {map}\n"));
            }
            None => {
                self.open((&path, &path), track, Some(host.client), host.game);
            }
        }
    }

    /// Opens the demo file `file`, said as `path`, and starts the recording, from
    /// `connected`'s state when the client is; whether it started.
    fn open(
        &mut self,
        (path, file): (&Path, &Path),
        track: DemoTrack,
        connected: Option<&Client>,
        game: &mut Game,
    ) -> bool {
        game.console_line(&format!("recording to {}.\n", path.display()));
        let opened = std::fs::File::create(file)
            .and_then(|file| self.recorder.record(Box::new(file), track, connected));
        match opened {
            Ok(()) => {
                self.forcetrack = match track {
                    DemoTrack::None => qs_client::DEMO_NO_TRACK,
                    DemoTrack::Forced(track) => track,
                };
                true
            }
            Err(_) => {
                game.console_line(&format!("ERROR: couldn't create {}\n", path.display()));
                false
            }
        }
    }

    /// `CL_PlayDemo_f`, and `CL_TimeDemo_f` at the pace of every frame.
    ///
    /// Implements CLIENT-10.8 and CLIENT-10.9.
    fn play(&mut self, words: &[Vec<u8>], pace: DemoPace, host: &mut DemoHost<'_>) -> Started {
        if words.len() != PLAY_WORDS {
            host.game.console_line(match pace {
                DemoPace::Timed => "playdemo <demoname> : plays a demo\n",
                DemoPace::EveryFrame => "timedemo <demoname> : gets demo speeds\n",
            });
            return Started::Nothing;
        }
        // "disconnect from server": a demo being played stops, one being recorded ends.
        // The map to go back to is the one before the first of a loop's demos.
        let back = match (self.playing.take(), self.resume.take()) {
            (Some(playing), _) => playing.back,
            (None, Some(back)) => back,
            (None, None) => {
                if self.recorder.recording() {
                    self.stop(host.game, host.client);
                }
                host.game.world().map().to_owned()
            }
        };
        // The messages kept for a recording are forgotten when the demo's map loads
        // (`Args::load_map`): a demo that does not start leaves the game connected.
        let name = with_extension(&word(words, 1));
        host.game
            .console_line(&format!("Playing demo from {name}.\n"));
        let said = match (host.read)(&name).map(Demo::open) {
            None => Err(format!("ERROR: couldn't open {name}\n")),
            Some(Err(DemoError::Invalid | DemoError::Read | DemoError::TooLong)) => {
                Err(format!("ERROR: demo \"{name}\" is invalid\n"))
            }
            Some(Ok(demo)) => match demo.map() {
                Some(map) => Ok((demo, map)),
                None => Err(format!("ERROR: demo \"{name}\" names no map\n")),
            },
        };
        let (demo, map) = match said {
            Ok(found) => found,
            Err(error) => {
                host.game.console_line(&error);
                // "stop demo loop".
                self.next = None;
                self.leave(&back, host.client, host.game);
                return Started::Nothing;
            }
        };
        self.forcetrack = demo.forcetrack();
        self.playing = Some(Playing {
            demo,
            pace,
            stage: Stage::Loading,
            back,
            frames: 0,
            second: None,
        });
        host.game.world_mut().console(&format!("map {map}\n"));
        Started::Demo
    }

    /// Whether a level asked for is refused because a demo plays: `restart` does nothing,
    /// "`if (cls.demoplayback || !sv.active) return;`", and `changelevel` says why, "`if
    /// (!sv.active || cls.demoplayback)`". A new game is never refused: it stops the demo.
    ///
    /// Implements CLIENT-10.6.
    pub(crate) fn refuses(&self, connection: Connection, game: &mut Game) -> bool {
        if !self.playing() {
            return false;
        }
        match connection {
            Connection::New => false,
            Connection::Changed => {
                game.console_line("Only the server may changelevel\n");
                true
            }
            Connection::Restarted => true,
        }
    }

    /// Where every demo that stops leaves the host: when the world loaded is a demo's, its
    /// client stops playing and the map `back`, played before the demos, starts again; a
    /// game's world, which a demo that never started left alone, goes on.
    fn leave(&self, back: &str, client: &mut Client, game: &mut Game) {
        if self.showing == Showing::Demo {
            client.stop_playback();
            game.world_mut().console(&format!("map {back}\n"));
        }
    }

    /// What a level about to load does to the demos. A new game disconnects the client
    /// first, which ends a demo being recorded and stops the demo loop ("stop demo loop in
    /// case this fails"); the recording `record` asked for with its map starts with the
    /// map; the next level of a game goes on being recorded. A demo being played is left
    /// until the level is known to have loaded ([`Demos::after_level`]).
    pub(crate) fn before_level(
        &mut self,
        connection: Connection,
        game: &mut Game,
        client: &Client,
    ) {
        let loading = self
            .playing
            .as_ref()
            .is_some_and(|playing| playing.stage == Stage::Loading);
        if connection == Connection::New && !loading {
            self.next = None;
            if self.recorder.recording() {
                self.stop(game, client);
            }
        }
        if let Some(waiting) = self.waiting.take() {
            let file = unloaded(&waiting.path);
            let started = self.open((&waiting.path, &file), waiting.track, None, game);
            if started {
                self.waiting = Some(Waiting { started, ..waiting });
            }
        }
    }

    /// What a level that loaded, or did not, does to the demos.
    ///
    /// A recording that waited for the map takes its name when the map loaded, and with no
    /// map is not made: "`if (cls.state != ca_connected) return;`". A demo whose map loaded
    /// plays, with a client of its own in the place of the one the map's server connected;
    /// one whose map did not is over, and leaves. A demo that was playing when another
    /// level loaded is over, the level the game's; when that level did not load it plays
    /// on.
    pub(crate) fn after_level(&mut self, level: Level<'_>, game: &mut Game) {
        if let Some(waiting) = self.waiting.take()
            && waiting.started
        {
            let file = unloaded(&waiting.path);
            let kept = match &level {
                Level::Loaded(_) => std::fs::rename(&file, &waiting.path),
                Level::Stayed(_) => {
                    // No client read the map: the recording ends with no view to say.
                    if let Err(error) = self.recorder.stop(qs_client::DEMO_NO_ANGLES) {
                        tracing::debug!(%error, "the demo of a map that did not load");
                    }
                    std::fs::remove_file(&file)
                }
            };
            if let Err(error) = kept {
                tracing::warn!(%error, file = %file.display(), "the demo beside its name");
            }
        }
        let stage = self.playing.as_ref().map(|playing| playing.stage);
        match (stage, level) {
            (Some(Stage::Loading), Level::Loaded(client)) => {
                *client = Client::new(qs_client::Game::Standard);
                if let Some(playing) = self.playing.as_mut() {
                    playing.stage = Stage::Playing;
                }
                self.showing = Showing::Demo;
            }
            (Some(Stage::Loading), Level::Stayed(client)) => {
                game.console_line("ERROR: the demo's map did not load\n");
                self.next = None;
                if let Some(playing) = self.playing.take() {
                    self.leave(&playing.back, client, game);
                }
            }
            (Some(Stage::Playing) | None, Level::Loaded(_)) => {
                self.playing = None;
                self.resume = None;
                self.showing = Showing::Game;
            }
            (Some(Stage::Playing) | None, Level::Stayed(_)) => {}
        }
    }

    /// The client's frame: the playing demo's messages in the place of the server's
    /// `packets`, or the packets, each recorded as it is read. A demo that ends, or does
    /// not read, stops, `timedemo` says its speed by the host's clock `realtime`, and the
    /// map played before it starts again. While the game is paused a playing demo stands
    /// still. A demo that goes on to another level has that level's map loaded before the
    /// level's messages are read (CLIENT-10.8). The CD track the events name is the one a
    /// demo being recorded or played forces, when it forces one (CLIENT-10.1).
    ///
    /// # Errors
    ///
    /// [`DemoFrameError`]: a message of the server's or of the demo's that does not read,
    /// or the demo's file.
    pub(crate) fn frame(
        &mut self,
        client: &mut Client,
        (frametime, realtime, packets): (f32, f64, &[Vec<u8>]),
        (models, rand): (&mut dyn Models, &mut dyn Random),
        game: &mut Game,
    ) -> Result<Vec<Event>, DemoFrameError> {
        let mut events =
            self.read_frame(client, (frametime, realtime, packets), (models, rand), game)?;
        // "`if ( (cls.demoplayback || cls.demorecording) && (cls.forcetrack != -1) )
        // BGM_PlayCDtrack ((byte)cls.forcetrack, true);`".
        let forced = self.playing.is_some() || self.recorder.recording();
        if forced && self.forcetrack != qs_client::DEMO_NO_TRACK {
            for event in &mut events {
                if let Event::CdTrack { track, .. } = event {
                    *track = self.forcetrack;
                }
            }
        }
        Ok(events)
    }

    /// [`Demos::frame`]'s messages, read.
    fn read_frame(
        &mut self,
        client: &mut Client,
        (frametime, realtime, packets): (f32, f64, &[Vec<u8>]),
        (models, rand): (&mut dyn Models, &mut dyn Random),
        game: &mut Game,
    ) -> Result<Vec<Event>, DemoFrameError> {
        let Some(playing) = self
            .playing
            .as_mut()
            .filter(|playing| playing.stage == Stage::Playing)
        else {
            return client
                .frame_recorded(frametime, packets, &mut self.recorder, models, rand)
                .map_err(DemoFrameError::Parse);
        };
        // The game paused from the IDE, or at a breakpoint: no time passes and no message is
        // read, as when the server's own frame does not run.
        if game.paused() {
            return client
                .frame(0.0, &[], models, rand)
                .map_err(DemoFrameError::Parse);
        }
        // "if this is the second frame, grab the real td_starttime so the bogus time on
        // the first frame doesn't count".
        if playing.frames == 1 {
            playing.second = Some(realtime);
        }
        playing.frames = playing.frames.saturating_add(1);
        let played = client.demo_frame(frametime, &mut playing.demo, playing.pace, models, rand);
        let frame = match played {
            Ok(frame) => frame,
            Err(error) => {
                self.finish(client, game, realtime);
                return Err(error);
            }
        };
        // The demo went on to a level of another map: the map is loaded, and the level's
        // messages are read again from its serverinfo by the client the map is given.
        if let Some((map, start)) = &frame.level
            && map != game.world().map()
        {
            playing.demo.seek(*start);
            playing.stage = Stage::Loading;
            game.world_mut().console(&format!("map {map}\n"));
            return Ok(Vec::new());
        }
        if frame.state == DemoState::Ended {
            self.finish(client, game, realtime);
        }
        Ok(frame.events)
    }

    /// `CL_StopPlayback`: the demo is over, `timedemo`'s speed is said (`CL_FinishTimeDemo`),
    /// and the map played before the demo starts again.
    fn finish(&mut self, client: &mut Client, game: &mut Game, realtime: f64) {
        client.stop_playback();
        let Some(playing) = self.playing.take() else {
            return;
        };
        if playing.pace == DemoPace::EveryFrame {
            // "the first frame didn't count".
            let frames = playing.frames.saturating_sub(1);
            // "if (!time) time = 1;".
            let seconds = playing.second.map_or(0.0, |second| realtime - second);
            let seconds = if seconds == 0.0 { 1.0 } else { seconds };
            let rate = f64::from(frames) / seconds;
            game.console_line(&format!(
                "{frames} frames {seconds:5.1} seconds {rate:5.1} fps\n"
            ));
        }
        // `Host_EndGame`: "`if (cls.demonum != -1 && !cls.timedemo) CL_NextDemo ();`".
        if playing.pace == DemoPace::Timed
            && let Some(name) = self.next_demo(game)
        {
            self.resume = Some(playing.back);
            game.world_mut().console(&format!("playdemo {name}\n"));
            return;
        }
        game.world_mut().console(&format!("map {}\n", playing.back));
    }

    /// `Host_Startdemos_f`: the loop's demos are the words after the command, at most
    /// [`MAX_DEMOS`], each kept to [`MAX_DEMO_NAME`] bytes; a name past the words given keeps
    /// what it was. The host's server is always running, so no loop starts: "`if (!sv.active
    /// && ...) ... else cls.demonum = -1;`".
    ///
    /// Implements CLIENT-10.11.
    fn start_demos(&mut self, words: &[Vec<u8>], game: &mut Game) {
        let given = words.len().saturating_sub(1);
        if given > MAX_DEMOS {
            game.console_line(&format!("Max {MAX_DEMOS} demos in demoloop\n"));
        }
        let count = given.min(MAX_DEMOS);
        game.console_line(&format!("{count} demo(s) in loop\n"));
        for (slot, name) in self.listed.iter_mut().zip(words.iter().skip(1)) {
            let kept = name.get(..MAX_DEMO_NAME).unwrap_or(name);
            *slot = String::from_utf8_lossy(kept).into_owned();
        }
        self.next = None;
    }

    /// `CL_NextDemo`: the name of the loop's next demo, the loop moved on past it; none
    /// when no loop runs, or when none is listed, which is said and ends the loop.
    ///
    /// Implements CLIENT-10.12.
    fn next_demo(&mut self, game: &mut Game) -> Option<String> {
        let mut next = self.next?;
        if self.listed.get(next).is_none_or(String::is_empty) {
            next = 0;
        }
        let Some(name) = self.listed.get(next).filter(|name| !name.is_empty()) else {
            game.console_line("No demos listed with startdemos\n");
            self.next = None;
            return None;
        };
        self.next = Some(next.saturating_add(1));
        Some(name.clone())
    }

    /// `Host_Demos_f`: the loop starts, at its second demo when it was not running, or goes
    /// on to its next demo, the one playing stopped.
    ///
    /// Implements CLIENT-10.12.
    fn demo_loop(&mut self, host: &mut DemoHost<'_>) -> Started {
        if self.next.is_none() {
            self.next = Some(LOOP_START);
        }
        match self.next_demo(host.game) {
            Some(name) => {
                let words = [b"playdemo".to_vec(), name.into_bytes()];
                self.play(&words, DemoPace::Timed, host)
            }
            // `CL_Disconnect`: a demo being played stops.
            None => {
                self.stop_playback(host);
                Started::Nothing
            }
        }
    }

    /// `Host_Stopdemo_f`: a demo being played stops, "`if (!cls.demoplayback) return;`", and
    /// the map played before it starts again.
    ///
    /// Implements CLIENT-10.13.
    fn stop_playback(&mut self, host: &mut DemoHost<'_>) {
        let Some(playing) = self.playing.take() else {
            return;
        };
        self.leave(&playing.back, host.client, host.game);
    }
}

#[cfg(test)]
mod tests;
