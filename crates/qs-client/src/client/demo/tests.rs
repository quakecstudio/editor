// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    reason = "test code: a failed unwrap or index is a failed test; the numbers are small"
)]

use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;

use qs_sim::protocol::Protocol;

use super::{
    DEMO_MAX_MESSAGE, DEMO_NO_TRACK, Demo, DemoError, DemoFrameError, DemoPace, DemoRecorder,
    DemoState, DemoTrack, scan_int,
};
use crate::fitz_tests::{
    NoRandom, THING, TestModels, datagram, msg, serverinfo, thing_baseline, thing_update,
};
use crate::{CLIENT_SIGNONS, Client, ClientLimits, Event, Game};

/// The three protocols a demo is recorded and played in (ADR-0026).
const PROTOCOLS: [Protocol; 3] = [Protocol::NETQUAKE, Protocol::FITZQUAKE, Protocol::RMQ];
/// A server frame an exact number of seconds long, so times compare exactly.
const FRAME: f32 = 0.0625;
/// `U_ORIGIN1`.
const U_ORIGIN1: i32 = 1 << 1;

/// A file in memory that a recorder writes and the test reads.
#[derive(Clone, Default)]
struct File(Rc<RefCell<Vec<u8>>>);

impl Write for File {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A file that cannot be written.
struct Full;

impl Write for Full {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("disk full"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn client() -> Client {
    Client::with_limits(Game::Standard, ClientLimits::default())
}

/// A message of one command byte `command` and the bytes `rest` after it.
fn command(protocol: Protocol, command: i32, rest: &[u8]) -> Vec<u8> {
    let mut m = msg(protocol);
    m.write_byte(command);
    m.write_bytes(rest);
    m.take()
}

/// The packets of a short game in `protocol`, a frame at a time: the signon in three
/// stages, then `frames` datagrams a [`FRAME`] apart in which the thing moves along x, a
/// reliable message after the datagram in every third frame.
fn game(protocol: Protocol, frames: usize) -> Vec<Vec<Vec<u8>>> {
    let mut first = serverinfo(protocol);
    first.extend(command(protocol, 25, &[1]));
    let mut second = thing_baseline(protocol);
    // Light style 0, client 0's name, and the second signon stage.
    second.extend(command(protocol, 12, b"\0m\0"));
    second.extend(command(protocol, 13, b"\0player\0"));
    second.extend(command(protocol, 25, &[2]));
    let mut out = vec![vec![first], vec![second], vec![command(protocol, 25, &[3])]];
    for frame in 0..frames {
        let mut x = msg(protocol);
        x.write_coord(8.0 + frame as f32);
        let moved = thing_update(protocol, U_ORIGIN1, &x.take());
        let time = 1.0 + FRAME * frame as f32;
        let mut packets = vec![datagram(protocol, time, &[moved])];
        if frame % 3 == 1 {
            // `svc_updatestat` of the monsters killed, and a print.
            let mut stat = msg(protocol);
            stat.write_byte(3);
            stat.write_byte(14);
            stat.write_long(i32::try_from(frame).unwrap());
            stat.write_byte(8);
            stat.write_string(b"reliable\n");
            packets.push(stat.take());
        }
        out.push(packets);
    }
    out
}

/// What a client has heard from its server: what its messages set, and nothing a frame's
/// interpolation makes of it.
#[derive(Clone, Debug, PartialEq)]
struct Heard {
    signon: i32,
    message_time: f32,
    stats: Vec<i32>,
    styles: Vec<Vec<u8>>,
    names: Vec<Vec<u8>>,
    viewentity: usize,
    thing: ([f32; 3], [f32; 3], f32, usize),
}

fn heard(client: &Client) -> Heard {
    let thing = client.entities().get(THING).copied().unwrap_or_default();
    Heard {
        signon: client.signon(),
        message_time: client.message_time(),
        stats: client.stats().to_vec(),
        styles: client.lightstyles().to_vec(),
        names: client
            .scores()
            .iter()
            .map(|score| score.name.clone())
            .collect(),
        viewentity: client.viewentity(),
        thing: (
            thing.msg_origins[0],
            thing.msg_origins[1],
            thing.msgtime,
            thing.model,
        ),
    }
}

/// The view angles the recording client has in its frame `frame`.
fn looking(frame: usize) -> [f32; 3] {
    [frame as f32, 350.0 + 3.0 * frame as f32, 0.0]
}

/// Plays `frames` through a client recording into `recorder`, a packet at a time: what the
/// client had heard, and its view angles, after each packet.
fn live(
    frames: &[Vec<Vec<u8>>],
    recorder: &mut DemoRecorder,
    client: &mut Client,
    from: usize,
) -> Vec<(Heard, [f32; 3])> {
    let mut after = Vec::new();
    for (frame, packets) in frames.iter().enumerate().skip(from) {
        client.set_viewangles(looking(frame));
        for (place, packet) in packets.iter().enumerate() {
            let frametime = if place == 0 && frame >= 3 { FRAME } else { 0.0 };
            client
                .frame_recorded(
                    frametime,
                    core::slice::from_ref(packet),
                    recorder,
                    &mut TestModels,
                    &mut NoRandom,
                )
                .unwrap();
            after.push((heard(client), looking(frame)));
        }
    }
    after
}

// spec CLIENT-10.1, CLIENT-10.2, CLIENT-10.4, CLIENT-10.5, CLIENT-10.6: a demo recorded under
// each of protocols 15, 666 and 999 plays back to the same client state: after each frame of
// the playing client it has heard just what the recording client had when it had read as
// many messages, with the recording's view angles, to the disconnect message that ends it
#[test]
fn a_demo_plays_back_what_was_recorded_in_each_protocol() {
    for protocol in PROTOCOLS {
        let frames = game(protocol, 12);
        let file = File::default();
        let mut recorder = DemoRecorder::new();
        recorder
            .record(Box::new(file.clone()), DemoTrack::None, None)
            .unwrap();
        assert!(recorder.recording());
        let mut recording = client();
        let recorded = live(&frames, &mut recorder, &mut recording, 0);
        assert!(recorder.stop(recording.viewangles()).unwrap());
        assert!(!recorder.recording());
        let bytes = file.0.borrow().clone();
        assert!(bytes.starts_with(b"-1\n"));

        let mut demo = Demo::open(bytes).unwrap();
        assert_eq!(demo.forcetrack(), DEMO_NO_TRACK);
        let mut playing = client();
        let mut read = 0;
        let mut frames_played = 0;
        loop {
            let frame = playing
                .demo_frame(
                    FRAME,
                    &mut demo,
                    DemoPace::Timed,
                    &mut TestModels,
                    &mut NoRandom,
                )
                .unwrap();
            read += frame.read;
            frames_played += 1;
            if frame.state == DemoState::Ended {
                assert!(frame.events.contains(&Event::Disconnect));
                assert!(!playing.demo_playing());
                break;
            }
            assert!(playing.demo_playing());
            let (was, angles) = &recorded[read - 1];
            assert_eq!(&heard(&playing), was, "{protocol:?} after {read} messages");
            assert_eq!(
                playing.playback.unwrap().mviewangles[0],
                *angles,
                "the angles of message {read}"
            );
            assert!(frames_played < 100, "the demo does not end");
        }
        // Every recorded message and the disconnect were read.
        assert_eq!(read, recorded.len() + 1, "{protocol:?}");
        assert_eq!(heard(&playing), recorded.last().unwrap().0);
        assert_eq!(playing.signon(), CLIENT_SIGNONS);
    }
}

// spec CLIENT-10.6: until the client is fully connected every message is read in one frame;
// then, timed, a frame reads while the client's time is past the last message's, so a frame
// as long as the server's reads a datagram, and one half as long reads one every second
// frame; every frame, as timedemo plays, a frame reads one message whatever its length
#[test]
fn a_playing_demo_takes_its_messages_by_its_pace() {
    let protocol = Protocol::NETQUAKE;
    let frames = game(protocol, 8);
    let file = File::default();
    let mut recorder = DemoRecorder::new();
    recorder
        .record(Box::new(file.clone()), DemoTrack::None, None)
        .unwrap();
    live(&frames, &mut recorder, &mut client(), 0);
    let bytes = file.0.borrow().clone();
    let play = |pace: DemoPace, frametime: f32, count: usize| -> Vec<usize> {
        let mut demo = Demo::open(bytes.clone()).unwrap();
        let mut playing = client();
        (0..count)
            .map(|_| {
                playing
                    .demo_frame(frametime, &mut demo, pace, &mut TestModels, &mut NoRandom)
                    .unwrap()
                    .read
            })
            .collect()
    };
    // The three signon messages and the first datagram, which completes the signon. The
    // clock is then a tenth of a second before that datagram, as at any demo's start
    // (CLIENT-5.2), so the next frame reads nothing; then a datagram a frame, with the
    // reliable message that followed the second datagram read before the third.
    assert_eq!(play(DemoPace::Timed, FRAME, 5), [4, 0, 1, 2, 1]);
    assert_eq!(play(DemoPace::Timed, FRAME / 2.0, 6), [4, 0, 0, 0, 1, 0]);
    assert_eq!(play(DemoPace::EveryFrame, FRAME, 5), [4, 1, 1, 1, 1]);
    assert_eq!(play(DemoPace::EveryFrame, 0.0, 5), [4, 1, 1, 1, 1]);
}

// spec CLIENT-10.10: `pause` while a demo plays holds its messages and pauses the client,
// and again lets them go; with no demo playing it changes nothing
#[test]
fn pause_holds_a_playing_demos_messages() {
    let frames = game(Protocol::NETQUAKE, 8);
    let file = File::default();
    let mut recorder = DemoRecorder::new();
    recorder
        .record(Box::new(file.clone()), DemoTrack::None, None)
        .unwrap();
    live(&frames, &mut recorder, &mut client(), 0);
    let mut demo = Demo::open(file.0.borrow().clone()).unwrap();
    let mut playing = client();
    playing.pause_demo();
    assert!(!playing.paused());
    let mut frame = |playing: &mut Client| {
        playing
            .demo_frame(
                FRAME,
                &mut demo,
                DemoPace::EveryFrame,
                &mut TestModels,
                &mut NoRandom,
            )
            .unwrap()
    };
    assert_eq!(frame(&mut playing).read, 4);
    playing.pause_demo();
    assert!(playing.paused());
    for _ in 0..3 {
        let held = frame(&mut playing);
        assert_eq!((held.read, held.state), (0, DemoState::Playing));
    }
    playing.pause_demo();
    assert!(!playing.paused());
    assert_eq!(frame(&mut playing).read, 1);
}

// spec CLIENT-10.7: a playing demo's view turns between the angles of its last two messages
// by the frame's fraction, the short way round; in a timedemo it is at the last message's
#[test]
fn a_playing_demos_view_turns_between_its_messages() {
    let protocol = Protocol::NETQUAKE;
    let frames = game(protocol, 4);
    let file = File::default();
    let mut recorder = DemoRecorder::new();
    recorder
        .record(Box::new(file.clone()), DemoTrack::None, None)
        .unwrap();
    live(&frames, &mut recorder, &mut client(), 0);
    let bytes = file.0.borrow().clone();
    let mut demo = Demo::open(bytes).unwrap();
    let mut playing = client();
    let mut frame = |frametime: f32, pace: DemoPace| {
        playing
            .demo_frame(frametime, &mut demo, pace, &mut TestModels, &mut NoRandom)
            .unwrap();
        playing.viewangles()
    };
    let near = |at: [f32; 3], want: [f32; 3]| {
        at.iter()
            .zip(want)
            .all(|(at, want)| (at - want).abs() < 1e-3)
    };
    // The signon and the first datagram, at the demo's start: the clock is a tenth before
    // the datagram, and the view at the message before it.
    assert_eq!(frame(FRAME, DemoPace::Timed), looking(2));
    // No message this frame: 0.0625 of the tenth on, the view is five eighths of the way
    // from the angles of frame 2 to those of frame 3.
    let turned = frame(FRAME, DemoPace::Timed);
    assert!(near(turned, [2.625, 357.875, 0.0]), "{turned:?}");
    // The next datagram, a sixteenth after the first: two fifths of the way to its angles,
    // yaw 362, from the first's 359.
    let turned = frame(FRAME, DemoPace::Timed);
    assert!(near(turned, [3.4, 360.2, 0.0]), "{turned:?}");
    // Yaw 350 after 10 is 20 back, not 340 on: half way is 0.
    playing.playback.as_mut().unwrap().mviewangles = [[0.0, 350.0, 0.0], [0.0, 10.0, 0.0]];
    playing.mtime = [2.0, 1.9];
    playing.time = 1.95;
    playing.relink_entities(&mut NoRandom);
    assert!(
        near(playing.viewangles(), [0.0, 0.0, 0.0]),
        "{:?}",
        playing.viewangles()
    );
    // And 10 after 350 is 20 on: a quarter of the way is 355.
    playing.playback.as_mut().unwrap().mviewangles = [[0.0, 10.0, 0.0], [0.0, 350.0, 0.0]];
    playing.time = 1.925;
    playing.relink_entities(&mut NoRandom);
    assert!(
        near(playing.viewangles(), [0.0, 355.0, 0.0]),
        "{:?}",
        playing.viewangles()
    );
    // A timedemo is always at the last message.
    playing.playback.as_mut().unwrap().pace = DemoPace::EveryFrame;
    playing.time = 1.925;
    playing.relink_entities(&mut NoRandom);
    // 20 on from 350, which is 10 a turn round: the angle is not brought back under 360.
    assert_eq!(playing.viewangles(), [0.0, 370.0, 0.0]);
    assert!(near([playing.time(), 0.0, 0.0], [2.0, 0.0, 0.0]));
}

// spec CLIENT-10.3: a recording started once the client is connected holds the messages
// read before it was, then the client's state, and plays to what the client has heard;
// one started with a track writes the track
#[test]
fn a_recording_started_while_connected_plays() {
    for protocol in PROTOCOLS {
        let frames = game(protocol, 9);
        let mut recorder = DemoRecorder::new();
        let mut recording = client();
        // Six frames with nothing recorded: the signon and three datagrams.
        live(&frames[..6], &mut recorder, &mut recording, 0);
        assert_eq!(recording.signon(), CLIENT_SIGNONS);
        let file = File::default();
        recorder
            .record(
                Box::new(file.clone()),
                DemoTrack::Forced(7),
                Some(&recording),
            )
            .unwrap();
        let rest = live(&frames, &mut recorder, &mut recording, 6);
        recorder.stop(recording.viewangles()).unwrap();
        let bytes = file.0.borrow().clone();
        let mut demo = Demo::open(bytes).unwrap();
        assert_eq!(demo.forcetrack(), 7);
        let mut playing = client();
        let mut read = 0;
        while playing
            .demo_frame(
                FRAME,
                &mut demo,
                DemoPace::Timed,
                &mut TestModels,
                &mut NoRandom,
            )
            .map(|frame| {
                read += frame.read;
                frame.state
            })
            .unwrap()
            == DemoState::Playing
        {}
        // The two kept messages, the state, the three frames' four messages, the disconnect.
        assert_eq!(read, 2 + 1 + rest.len() + 1, "{protocol:?}");
        let (mut was, mut is) = (rest.last().unwrap().0.clone(), heard(&playing));
        // The state names the counts and the styles; the monsters killed came by a message
        // the recording did not hold, and the state carries it.
        assert_eq!(is.stats[14], was.stats[14]);
        // What the datagrams before the recording said of the health is not in it.
        was.stats[0] = 0;
        is.stats[0] = 0;
        assert_eq!(is, was, "{protocol:?}");
    }
    // Forgetting the kept messages, as a disconnect does, leaves a later recording none.
    let frames = game(Protocol::NETQUAKE, 2);
    let mut recorder = DemoRecorder::new();
    let mut recording = client();
    live(&frames, &mut recorder, &mut recording, 0);
    recorder.clear_signons();
    let file = File::default();
    recorder
        .record(Box::new(file.clone()), DemoTrack::None, Some(&recording))
        .unwrap();
    let bytes = file.0.borrow().clone();
    let mut demo = Demo::open(bytes).unwrap();
    assert_eq!(
        demo.next_message().unwrap().unwrap().data,
        recording.demo_state()
    );
    assert_eq!(demo.next_message(), Err(DemoError::Read));
}

// spec CLIENT-10.2, CLIENT-10.5: a keepalive is neither read nor recorded; stop with nothing
// recording says so; a file that cannot be written ends the recording and is said
#[test]
fn what_is_not_recorded() {
    let file = File::default();
    let mut recorder = DemoRecorder::new();
    assert!(!recorder.stop([0.0; 3]).unwrap());
    recorder
        .record(Box::new(file.clone()), DemoTrack::None, None)
        .unwrap();
    let mut recording = client();
    let events = recording
        .frame_recorded(
            0.0,
            &[vec![1]],
            &mut recorder,
            &mut TestModels,
            &mut NoRandom,
        )
        .unwrap();
    assert_eq!(
        events,
        [Event::Print(b"<-- server to client keepalive\n".to_vec())]
    );
    assert_eq!(file.0.borrow().as_slice(), b"-1\n");
    // Two nops are a message like any other.
    recording
        .frame_recorded(
            0.0,
            &[vec![1, 1]],
            &mut recorder,
            &mut TestModels,
            &mut NoRandom,
        )
        .unwrap();
    assert_eq!(file.0.borrow().len(), 3 + 4 + 12 + 2);

    let mut recorder = DemoRecorder::new();
    assert!(
        recorder
            .record(Box::new(Full), DemoTrack::None, None)
            .is_err()
    );
    assert!(!recorder.recording());
    // A file that fills later ends the recording at the message it could not take.
    struct Later(usize);
    impl Write for Later {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or_else(|| io::Error::other("full"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    recorder
        .record(Box::new(Later(10)), DemoTrack::None, None)
        .unwrap();
    let events = recording
        .frame_recorded(
            0.0,
            &[vec![1, 1]],
            &mut recorder,
            &mut TestModels,
            &mut NoRandom,
        )
        .unwrap();
    assert!(
        matches!(events.as_slice(), [Event::Print(said)] if said.starts_with(b"ERROR: the demo")),
        "{events:?}"
    );
    assert!(!recorder.recording());
}

// spec CLIENT-10.1: the first line is read as fscanf's %i reads a number, and a newline
// must follow it at once
#[test]
fn a_demos_first_line_is_a_number_and_a_newline() {
    assert_eq!(scan_int(b"-1\n"), Some((-1, 2)));
    assert_eq!(scan_int(b" \t+12x"), Some((12, 5)));
    assert_eq!(scan_int(b"0x1F\n"), Some((31, 4)));
    assert_eq!(scan_int(b"017 "), Some((15, 3)));
    assert_eq!(scan_int(b"08"), Some((0, 1)), "8 is no octal digit");
    assert_eq!(scan_int(b"0x"), Some((0, 1)), "no digit after the x");
    assert_eq!(scan_int(b"4294967298"), Some((2, 10)), "wrapped to 32 bits");
    for none in [&b""[..], b"-", b"x", b"\n"] {
        assert_eq!(scan_int(none), None, "{none:?}");
    }
    assert_eq!(Demo::open(b"3\nrest".to_vec()).unwrap().forcetrack(), 3);
    for invalid in [&b""[..], b"3", b"3 \n", b"3\r\n", b"three\n"] {
        assert_eq!(
            Demo::open(invalid.to_vec()),
            Err(DemoError::Invalid),
            "{invalid:?}"
        );
    }
    assert_eq!(DemoError::Invalid.to_string(), "the demo is invalid");
}

// spec CLIENT-10.4: a file that ends where a length should be is a read error; a length
// past MAX_MSGLEN is refused; a file that ends within a message, a message of no bytes
// and a length below 0 end the demo
#[test]
fn a_damaged_demo_ends_or_is_refused() {
    let message = |length: i32, bytes: &[u8]| {
        let mut file = b"-1\n".to_vec();
        file.extend(length.to_le_bytes());
        file.extend([0; 12]);
        file.extend(bytes);
        file
    };
    let whole = message(2, &[1, 1]);
    let mut demo = Demo::open(whole.clone()).unwrap();
    assert_eq!(demo.next_message().unwrap().unwrap().data, [1, 1]);
    assert_eq!(demo.next_message(), Err(DemoError::Read));
    assert_eq!(DemoError::Read.to_string(), "Demo read error");
    for cut in [3 + 2, 3 + 4 + 5, 3 + 4 + 12 + 1] {
        let mut demo = Demo::open(whole[..cut].to_vec()).unwrap();
        let ends = demo.next_message();
        if cut < 3 + 4 {
            assert_eq!(ends, Err(DemoError::Read), "{cut}");
        } else {
            assert_eq!(ends, Ok(None), "{cut}");
            assert_eq!(demo.next_message(), Err(DemoError::Read));
        }
    }
    for length in [0, -1, i32::MIN] {
        let file = message(length, &[1, 1]);
        assert_eq!(
            Demo::open(file).unwrap().next_message(),
            Ok(None),
            "{length}"
        );
    }
    let most = i32::try_from(DEMO_MAX_MESSAGE).unwrap();
    let long = message(most + 1, &[]);
    assert_eq!(
        Demo::open(long).unwrap().next_message(),
        Err(DemoError::TooLong)
    );
    assert_eq!(DemoError::TooLong.to_string(), "Demo message > MAX_MSGLEN");
    let fits = message(most, &vec![1; DEMO_MAX_MESSAGE]);
    let mut demo = Demo::open(fits).unwrap();
    assert_eq!(
        demo.next_message().unwrap().unwrap().data.len(),
        DEMO_MAX_MESSAGE
    );

    // A playing client stops at the end of the file, and says a read error past it.
    let mut playing = client();
    let mut demo = Demo::open(whole[..3 + 4 + 12 + 1].to_vec()).unwrap();
    let frame = playing
        .demo_frame(
            0.0,
            &mut demo,
            DemoPace::Timed,
            &mut TestModels,
            &mut NoRandom,
        )
        .unwrap();
    assert_eq!((frame.read, frame.state), (0, DemoState::Ended));
    assert!(!playing.demo_playing());
    let error = playing
        .demo_frame(
            0.0,
            &mut demo,
            DemoPace::Timed,
            &mut TestModels,
            &mut NoRandom,
        )
        .unwrap_err();
    assert_eq!(error, DemoFrameError::Demo(DemoError::Read));
    assert_eq!(error.to_string(), "Demo read error");
    // A message that does not read is its parse error.
    let bad = message(1, &[200]);
    let error = client()
        .demo_frame(
            0.0,
            &mut Demo::open(bad).unwrap(),
            DemoPace::Timed,
            &mut TestModels,
            &mut NoRandom,
        )
        .unwrap_err();
    assert!(matches!(error, DemoFrameError::Parse(_)), "{error}");
}
