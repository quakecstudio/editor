// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-2001 Id Software, Inc.
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/cl_demo.c, CL_StopPlayback, CL_Stop_f, CL_Record_f, CL_PlayDemo_f, CL_FinishTimeDemo, CL_TimeDemo_f

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
/// The words of `playdemo` and `timedemo`: the command and a name.
const PLAY_WORDS: usize = 2;
/// The command that holds a playing demo.
const PAUSE: &[u8] = b"pause";
/// The extension a demo's name is given when it has another or none.
const DEMO_EXTENSION: &str = "dem";

/// Whether a level is a new game's, as `map` starts one, the client disconnected first, or
/// the next of the one being played, as `changelevel` and `restart` go on to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Connection {
    /// `Host_Map_f`: "`CL_Disconnect ();`" first.
    New,
    /// `Host_Changelevel_f` and `Host_Restart_f`: the client reconnects.
    Kept,
}

/// A recording that starts with the map `record` named, once the map has loaded.
#[derive(Debug)]
struct Waiting {
    path: PathBuf,
    track: DemoTrack,
    /// Whether the file is open and written: from the map's first message.
    started: bool,
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
}

/// What the demo commands work on.
pub(crate) struct DemoHost<'a> {
    pub(crate) args: &'a Args,
    pub(crate) game: &'a mut Game,
    pub(crate) client: &'a Client,
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

    /// Runs a demo command the console met; any other client command is not this module's.
    pub(crate) fn command(&mut self, command: &ClientCommand, host: &mut DemoHost<'_>) {
        match command {
            ClientCommand::Record(words) => self.record(words, host),
            ClientCommand::StopDemo => self.stop(host.game, host.client),
            ClientCommand::PlayDemo(words) => self.play(words, DemoPace::Timed, host),
            ClientCommand::TimeDemo(words) => self.play(words, DemoPace::EveryFrame, host),
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
        let map = words.get(2).map(|_| word(words, 2));
        if map.is_none() && host.client.signon() < RECORD_FROM_SIGNON {
            host.game
                .console_line("Can't record - try again when connected\n");
            return;
        }
        // "write the forced cd track number, or -1": the track said is the one before.
        let track = match words.get(3) {
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
                self.open(&path, track, Some(host.client), host.game);
            }
        }
    }

    /// Opens the demo file at `path` and starts the recording, from `connected`'s state
    /// when the client is; whether it started.
    fn open(
        &mut self,
        path: &Path,
        track: DemoTrack,
        connected: Option<&Client>,
        game: &mut Game,
    ) -> bool {
        game.console_line(&format!("recording to {}.\n", path.display()));
        let opened = std::fs::File::create(path)
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
    fn play(&mut self, words: &[Vec<u8>], pace: DemoPace, host: &mut DemoHost<'_>) {
        if words.len() != PLAY_WORDS {
            host.game.console_line(match pace {
                DemoPace::Timed => "playdemo <demoname> : plays a demo\n",
                DemoPace::EveryFrame => "timedemo <demoname> : gets demo speeds\n",
            });
            return;
        }
        // "disconnect from server": a demo being played stops, one being recorded ends.
        let back = match self.playing.take() {
            Some(playing) => playing.back,
            None => {
                if self.recorder.recording() {
                    self.stop(host.game, host.client);
                }
                host.game.world().map().to_owned()
            }
        };
        self.recorder.clear_signons();
        let name = with_extension(&word(words, 1));
        host.game
            .console_line(&format!("Playing demo from {name}.\n"));
        let Some(bytes) = (host.read)(&name) else {
            host.game
                .console_line(&format!("ERROR: couldn't open {name}\n"));
            self.back_to(&back, host.game);
            return;
        };
        let demo = match Demo::open(bytes) {
            Ok(demo) => demo,
            Err(DemoError::Invalid | DemoError::Read | DemoError::TooLong) => {
                host.game
                    .console_line(&format!("ERROR: demo \"{name}\" is invalid\n"));
                self.back_to(&back, host.game);
                return;
            }
        };
        let Some(map) = demo.map() else {
            host.game
                .console_line(&format!("ERROR: demo \"{name}\" names no map\n"));
            self.back_to(&back, host.game);
            return;
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
    }

    /// Starts the map `back` again when it is not the one being played: where a demo that
    /// stopped another, and then did not start, leaves the host.
    fn back_to(&self, back: &str, game: &mut Game) {
        if game.world().map() != back {
            game.world_mut().console(&format!("map {back}\n"));
        }
    }

    /// What a level about to load does to the demos: a new game disconnects the client
    /// first, which stops a demo being played and ends one being recorded, and the
    /// recording `record` asked for with its map starts with the map; the next level of a
    /// game goes on being recorded. The messages kept for a later recording are forgotten.
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
            self.playing = None;
            if self.recorder.recording() {
                self.stop(game, client);
            }
        }
        self.recorder.clear_signons();
        if let Some(waiting) = self.waiting.take() {
            let started = self.open(&waiting.path, waiting.track, None, game);
            if started {
                self.waiting = Some(Waiting { started, ..waiting });
            }
        }
    }

    /// What a level that loaded, or did not, does to the demos. A recording that waited
    /// for the map is recording it, or with no map is not, its file removed: "`if (cls.state
    /// != ca_connected) return;`". A demo whose map loaded plays, its client `fresh`, in the
    /// place of the one the map's own server connected; one whose map did not is over.
    pub(crate) fn after_level(&mut self, loaded: Option<&mut Client>, game: &mut Game) {
        if let Some(waiting) = self.waiting.take()
            && loaded.is_none()
            && waiting.started
        {
            if let Err(error) = self.recorder.stop([0.0; 3]) {
                tracing::debug!(%error, "the demo of a map that did not load");
            }
            if let Err(error) = std::fs::remove_file(&waiting.path) {
                tracing::warn!(%error, "the demo of a map that did not load was not removed");
            }
        }
        let Some(playing) = self.playing.as_mut() else {
            return;
        };
        if playing.stage != Stage::Loading {
            return;
        }
        match loaded {
            Some(client) => {
                *client = Client::new(qs_client::Game::Standard);
                playing.stage = Stage::Playing;
            }
            None => {
                game.console_line("ERROR: the demo's map did not load\n");
                self.playing = None;
            }
        }
    }

    /// The client's frame: the playing demo's messages in the place of the server's
    /// `packets`, or the packets, each recorded as it is read. A demo that ends, or does
    /// not read, stops, `timedemo` says its speed by the host's clock `realtime`, and the
    /// map played before it starts again.
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
        let Some(playing) = self
            .playing
            .as_mut()
            .filter(|playing| playing.stage == Stage::Playing)
        else {
            return client
                .frame_recorded(frametime, packets, &mut self.recorder, models, rand)
                .map_err(DemoFrameError::Parse);
        };
        // "if this is the second frame, grab the real td_starttime so the bogus time on
        // the first frame doesn't count".
        if playing.frames == 1 {
            playing.second = Some(realtime);
        }
        playing.frames = playing.frames.saturating_add(1);
        let played = client.demo_frame(frametime, &mut playing.demo, playing.pace, models, rand);
        let ended = match &played {
            Ok(frame) => frame.state == DemoState::Ended,
            Err(_) => true,
        };
        if ended {
            self.finish(client, game, realtime);
        }
        played.map(|frame| frame.events)
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
        game.world_mut().console(&format!("map {}\n", playing.back));
    }
}

#[cfg(test)]
mod tests;
