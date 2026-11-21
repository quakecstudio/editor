// SPDX-License-Identifier: GPL-3.0-or-later

//! The showcase screenshots and clips (P7-14): the app rendered offscreen through wgpu, as the
//! snapshot harness renders it, on LibreQuake's maps compiled with full light and their own
//! textures and on its QuakeC, each scene a PNG, or a folder of a clip's frames, for the website
//! and the user guide.
//!
//! LibreQuake is not in this repository (corpus/manifest.toml), so the scenes are rendered from a
//! checkout of it whose `texture-wads/` holds its WADs built from their PNGs, as LibreQuake's
//! `compile_wads.py` builds them, at the commit [`LIBREQUAKE`]. A map scene compiles its map
//! with the Showcase profile, written to `lq1/compile-profiles.toml`, unless its BSP is newer
//! than it; delete the BSP to compile it again:
//!
//! ```text
//! cargo run --release -p quakec-studio --example showcase -- --librequake <dir> --out <dir> \
//!     [--scale 2] [--only <name>]
//! ```
//!
//! `--scale 2` draws the 1440 by 900 window at two pixels a point, 2880 by 1800, as a Retina
//! display does. A clip scene writes `<out>/<scene>/frame-0000.png` and on, 30 a second of the
//! clip; its frames are paced in real time, as the game in the Game tab measures its frames by
//! the clock, and the showcase prints how many frames took longer than a thirtieth of a second
//! to draw.
//!
//! The scenes that play LibreQuake's QuakeC need a game folder holding LibreQuake's lite data in
//! `id1`, through `QS_GAME_DIR` (`cargo xtask assets fetch` puts one in the test-assets cache).
//! The debugger still plays it in the preview host, so it also needs the host built
//! (`cargo build --release -p quakec-engine`) and found through `QS_PREVIEW_HOST`; the clips play
//! it in the window's Game tab. The agent scenes run the agent on a scripted model, the replies
//! below, so that what the panel shows is the agent's real loop: its brief, its tool calls run
//! through the IDE's commands, its staged hunks and its own check that the project compiles and
//! its scenario passes.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use clap::Parser;
use eframe::egui::{self, Key, Modifiers, TexturesDelta, accesskit::Role};
use eframe::egui_wgpu::{self, ScreenDescriptor};
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, TestRenderer};
use image::ImageEncoder as _;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use qs_agent::{Message, Model, ModelError, Reply, ToolCall, ToolSpec};
use qs_models::{ProviderConfig, ProviderKind};
use qs_render::{Camera, Degrees, Offscreen};
use qs_workbench::engine::ENGINE_SCREENSHOT;
use qs_workbench::{BREAKPOINT, PLAY, PlayState, STEP_BY, STOP, WATCH};
use quakec_studio::App;
use quakec_studio::agent::{AgentPanel, ThreadIndex};
use quakec_studio::map_view::DEPTH_FORMAT;
use quakec_studio::strings::Text;
use quakec_studio::theme::{Mode, Theme};
use serde_json::{Value, json};

/// Where the pool room of lq_e1m1 is seen from: its south-east corner, looking north-west over
/// the slime and lava pools to the consoles.
const ROOM: [f32; 3] = [380.0, -850.0, 170.0];
/// The camera's yaw and pitch in the pool room, in degrees.
const ROOM_VIEW: (f32, f32) = (160.0, 12.0);
/// A green light over the slime: entity 1327 of lq_e1m1, `_color 143 240 164`.
const GREEN_LIGHT: usize = 1327;

/// LibreQuake's QuakeC is FTEQCC's dialect, with `#ifdef` for QuakeWorld.
const MANIFEST: &str =
    "[project]\ndialect = \"fteqcc\"\n\n[[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n";

/// The compile profile the showcase compiles each map with.
const PROFILE: &str = "Showcase";

/// [`PROFILE`] as `compile-profiles.toml` writes it.
const PROFILES: &str = "[[profile]]\nname = \"Showcase\"\ntasks = [{ task = \"bsp\" }, \
    { task = \"vis\" }, { task = \"light\" }]\n";

/// The window's size in points, the snapshot harness's.
const SIZE: [f32; 2] = [1440.0, 900.0];
/// The map the debugger scenes play: one of LibreQuake's lite game data, with monsters that
/// stand until they see the player.
const DEBUGGER_MAP: &str = "lq_e0m1";
/// The map the hot reload clip plays: LibreQuake's start map, lit by light style 0.
const START_MAP: &str = "start";
/// The LibreQuake commit the scenes are framed on.
const LIBREQUAKE: &str = "681890f1216d339a989b55d7c6a6cfd95db0b33e";
/// How long a map scene waits for its map's full compile.
const COMPILE_WAIT: Duration = Duration::from_secs(600);
/// How long the debugger scene waits for the game to reach its breakpoint.
const STOP_WAIT: Duration = Duration::from_secs(60);
/// How long the debugger scene waits between frames while the game starts.
const PREVIEW_FRAME: Duration = Duration::from_millis(50);
/// The frames run after each step of a still, so what it did is drawn.
const SETTLE_FRAMES: usize = 4;

/// A clip's frame: a thirtieth of a second.
const CLIP_FRAME: Duration = Duration::from_nanos(33_333_333);
/// [`CLIP_FRAME`] in seconds, the step of a clip's harness.
const CLIP_STEP: f32 = 1.0 / 30.0;
/// The frames a clip's harness may run to settle, ten seconds of them.
const CLIP_MAX_STEPS: u64 = 300;
/// The longest a clip waits on one condition.
const CLIP_WAIT: Duration = Duration::from_secs(120);
/// The threads that write a clip's frames as PNGs while it records.
const WRITERS: usize = 6;
/// The frames waiting for a writer before the clip waits for one.
const WRITE_QUEUE: usize = 24;
/// The frames each typed character is shown for.
const TYPE_FRAMES: usize = 2;
/// How long the game plays before a clip records it, while it loads its map.
const GAME_START: Duration = Duration::from_secs(4);

/// The line the hot reload clip types into `PlayerPreThink`: light style 0, which lights most
/// of a level, flickering as id's fluorescent style 10 does, set on every frame.
const FLICKER_LINE: &str = "lightstyle (0, \"mmamammmmammamamaaamammma\");";
/// The line of `PlayerPreThink` the clip types its line below.
const PRETHINK_LINE: &str = "makevectors (self.v_angle);\t\t// is this still used";

/// What the agent scenes ask of the agent.
const AGENT_TASK: &str = "Make the player spawn with 150 health instead of 100.";
/// The scenario that says the task is done, put in the QuakeC's `tests/` as the modder's.
const SCENARIO: &str = "# The player spawns with 150 health.\nmap start\nseed 1\nframes 4\n\
    expect field player classname == \"player\"\nexpect field player health == 150\n";
/// Where [`SCENARIO`] is kept, under the QuakeC.
const SCENARIO_FILE: &str = "tests/spawn-health.scenario";
/// The line of `SetNewParms` the agent changes, and what it becomes.
const PARM_LINE: (&str, &str) = ("\tparm2 = 100;", "\tparm2 = 150;");
/// The pause between the words of a scripted reply, as a model writes them.
const WORD_PAUSE: Duration = Duration::from_millis(40);

/// The showcase's options.
#[derive(Parser)]
#[command(about = "Render the showcase screenshots and clips from a LibreQuake checkout")]
struct Args {
    /// The LibreQuake checkout: its `lq1` game folder, `qcsrc` and built `texture-wads`.
    #[arg(long)]
    librequake: PathBuf,
    /// Where the PNGs are written.
    #[arg(long)]
    out: PathBuf,
    /// Look from here instead, as `x,y,z,yaw,pitch`, to find a scene's view.
    #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
    look: Option<Vec<f32>>,
    /// Render only the scenes whose names contain this.
    #[arg(long)]
    only: Option<String>,
    /// Pixels a point: 2 draws the window at 2880 by 1800.
    #[arg(long, default_value_t = 1.0)]
    scale: f32,
}

/// egui_kittest's renderer as `qs_render::Offscreen`, with the depth buffer the window has.
struct DepthRenderer {
    offscreen: Offscreen,
}

impl TestRenderer for DepthRenderer {
    fn handle_delta(&mut self, delta: &mut TexturesDelta) {
        self.offscreen.update_textures(delta);
    }

    #[expect(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the window's size in pixels: a few thousand, rounded and held at 0"
    )]
    fn render(
        &mut self,
        ctx: &egui::Context,
        output: &egui::FullOutput,
    ) -> Result<image::RgbaImage, String> {
        let size = ctx.content_rect().size() * ctx.pixels_per_point();
        let pixels = [size.x.round(), size.y.round()].map(|v| v.max(0.0) as u32);
        let screen = ScreenDescriptor {
            pixels_per_point: ctx.pixels_per_point(),
            size_in_pixels: pixels,
        };
        let tessellated = ctx.tessellate(output.shapes.clone(), ctx.pixels_per_point());
        let rendered = self
            .offscreen
            .render(&tessellated, &screen)
            .map_err(|error| error.to_string())?;
        image::RgbaImage::from_raw(rendered.width, rendered.height, rendered.rgba)
            .ok_or_else(|| "no image".to_owned())
    }
}

/// A camera that circles a point, a step of its circle each frame.
#[derive(Clone, Copy)]
struct Orbit {
    /// The point it circles.
    centre: [f32; 3],
    /// How far from the point it is, across the floor.
    radius: f32,
    /// How far above the point it is.
    height: f32,
    /// Where on the circle it starts, in degrees from east.
    from: f32,
    /// How far round it goes each frame, in degrees.
    step: f32,
    /// How far down it looks, in degrees.
    pitch: f32,
}

impl Orbit {
    /// The camera at frame `frame`: on the circle, looking at its centre.
    fn camera(&self, frame: usize) -> Camera {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a clip's frame, a few hundred at most"
        )]
        let angle = self.from + self.step * frame as f32;
        let radians = angle.to_radians();
        let [x, y, z] = self.centre;
        let origin = [
            x + self.radius * radians.cos(),
            y + self.radius * radians.sin(),
            z + self.height,
        ];
        let mut camera = Camera::new(origin, Degrees::new(angle + 180.0));
        camera.pitch = Degrees::new(self.pitch);
        camera
    }
}

/// What a clip waits for.
#[derive(Clone, Copy)]
enum Until {
    /// This much time.
    Time(Duration),
    /// The game stopped at a breakpoint.
    Stopped,
    /// The open map's BSP is newer than it.
    Compiled,
    /// The agent's model asked for its reply this many times.
    Asked(usize),
    /// The agent's first thread has its report.
    AgentDone,
}

/// Builds a reply of the scripted model from the QuakeC folder.
type MakeReply = fn(&Path) -> Result<Reply, String>;

/// One step of a scene.
#[derive(Clone)]
enum Step {
    /// Run a command with its parameters.
    Run(&'static str, Value),
    /// Look from here, yaw and pitch in degrees.
    Look([f32; 3], f32, f32),
    /// Hide every point entity in the editor but this one, so the map shows.
    HidePoints(Option<usize>),
    /// Set a breakpoint in this file of the project, on the line holding this text.
    Breakpoint(&'static str, &'static str),
    /// Play this map and wait until the game stops at a breakpoint, then write the game's frame
    /// beside the scene's own image.
    PlayToStop(&'static str),
    /// Click the button with this label, such as a panel's tab.
    Click(Text),
    /// Press a key with modifiers.
    Key(Modifiers, Key),
    /// Type text.
    Type(String),
    /// Run frames, writing every `every`-th one as the clip's next frame.
    Record { frames: usize, every: usize },
    /// Run frames, writing each, until the condition holds; at most `most` of them.
    RecordUntil(Until, usize),
    /// Run frames, writing none, until the condition holds.
    Wait(Until),
    /// Move the camera round this circle on each frame from now on.
    Orbit(Orbit),
    /// Write the window as it is now as a still of this name.
    Snap(&'static str),
    /// Compile the open map with this profile, as its Compile button does.
    Compile(&'static str),
    /// Make a random level with this seed, as `maps/showcase-<seed>.map`, and open it.
    Generate(u64),
    /// Show the QuakeC file in the editor, its cursor at the end of this text, and give the
    /// editor the keyboard.
    OpenAfter(&'static str, &'static str),
    /// Play the game in the window's Game tab, in this process.
    GameInTab,
    /// Start the agent on [`AGENT_TASK`], on the scripted model.
    StartAgent,
    /// Give the scripted model its next reply.
    Reply(MakeReply),
}

/// What a scene opens: a map of the game folder, compiled, or a QuakeC file of `qcsrc` at the
/// start of a line holding some text.
#[derive(Clone, Copy)]
enum Opens {
    /// The map at this path under `lq1/maps/src`.
    Map(&'static str),
    /// A copy of the map at the first path, at the second, not compiled: the scene compiles it.
    MapCopy(&'static str, &'static str),
    /// The game folder as a project, with no map open.
    Game,
    /// The QuakeC file, and the text whose line the editor shows.
    Code(&'static str, &'static str),
    /// The QuakeC file, and the text at whose end the editor's cursor is put.
    CodeAfter(&'static str, &'static str),
}

/// A scene: its name, what it opens, its look and its steps after.
struct Scene {
    name: &'static str,
    opens: Opens,
    theme: egui::Theme,
    steps: Vec<Step>,
}

impl Scene {
    /// A scene in the dark look.
    const fn dark(name: &'static str, opens: Opens, steps: Vec<Step>) -> Self {
        Self {
            name,
            opens,
            theme: egui::Theme::Dark,
            steps,
        }
    }

    /// The scene's kind: a clip when a step records frames.
    fn kind(&self) -> Kind {
        if self
            .steps
            .iter()
            .any(|step| matches!(step, Step::Record { .. } | Step::RecordUntil(..)))
        {
            Kind::Clip
        } else {
            Kind::Still
        }
    }
}

/// Whether a scene is one image or a clip's frames.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// One image, drawn once the window settles.
    Still,
    /// Frames paced in real time.
    Clip,
}

/// Steps that type `text` a character a frame, each shown for [`TYPE_FRAMES`].
fn typing(text: &str) -> Vec<Step> {
    text.chars()
        .flat_map(|c| {
            [
                Step::Type(c.to_string()),
                Step::Record {
                    frames: TYPE_FRAMES,
                    every: 1,
                },
            ]
        })
        .collect()
}

/// The scenes, each from LibreQuake.
#[expect(
    clippy::too_many_lines,
    reason = "one list of every scene, read from top to bottom as the site shows them"
)]
fn scenes() -> Vec<Scene> {
    let lit = || Step::Run("view.map_shading", json!({ "shading": "lit" }));
    let textured = || Step::Run("view.map_shading", json!({ "shading": "textured" }));
    let wide = || {
        [
            Step::Run("view.toggle_panel", json!({ "panel": "left" })),
            Step::Run("view.toggle_panel", json!({ "panel": "bottom" })),
        ]
    };
    let room = || Step::Look(ROOM, ROOM_VIEW.0, ROOM_VIEW.1);
    let clear = || Step::HidePoints(None);
    let watch = |expression: &str| Step::Run(WATCH, json!({ "expression": expression }));
    let record = |frames| Step::Record { frames, every: 1 };
    let over = || Step::Run(STEP_BY, json!({ "kind": "over" }));
    let show = |panel: &str| Step::Run("view.show_panel", json!({ "panel": panel }));
    vec![
        Scene::dark(
            "showcase-code",
            Opens::Code("weapons.qc", "void() W_FireRocket ="),
            Vec::new(),
        ),
        Scene::dark(
            "showcase-debugger",
            Opens::Code("ai.qc", "void() ai_stand ="),
            vec![
                Step::Breakpoint("ai.qc", "if (time > self.pausetime)"),
                watch("self.classname"),
                watch("self.health"),
                watch("self.enemy.classname"),
                Step::PlayToStop(DEBUGGER_MAP),
                show("debugger"),
                Step::Run("view.toggle_panel", json!({ "panel": "right" })),
            ],
        ),
        Scene::dark(
            "showcase-lit-wide",
            Opens::Map("e1/lq_e1m1.map"),
            [
                vec![lit(), room(), clear()],
                wide().into(),
                vec![Step::Run("view.toggle_panel", json!({ "panel": "right" }))],
            ]
            .concat(),
        ),
        Scene::dark(
            "showcase-textured",
            Opens::Map("e1/lq_e1m1.map"),
            [vec![textured(), room(), clear()], wide().into()].concat(),
        ),
        Scene::dark(
            "showcase-light-reach",
            Opens::Map("e1/lq_e1m1.map"),
            [
                vec![
                    lit(),
                    room(),
                    Step::HidePoints(Some(GREEN_LIGHT)),
                    Step::Run("map.select", json!({ "entity": GREEN_LIGHT })),
                ],
                wide().into(),
            ]
            .concat(),
        ),
        Scene::dark(
            "showcase-quad",
            Opens::Map("e1/lq_e1m1.map"),
            [
                vec![
                    lit(),
                    room(),
                    clear(),
                    Step::Run("view.map_panes", json!({ "panes": "quad" })),
                ],
                wide().into(),
            ]
            .concat(),
        ),
        Scene::dark(
            "showcase-palette",
            Opens::Code("weapons.qc", "void() W_FireRocket ="),
            vec![
                Step::Key(Modifiers::COMMAND | Modifiers::SHIFT, Key::P),
                Step::Type("play".to_owned()),
            ],
        ),
        Scene::dark(
            "showcase-generated",
            Opens::Game,
            [
                vec![
                    Step::Generate(7),
                    Step::Wait(Until::Compiled),
                    lit(),
                    clear(),
                ],
                wide().into(),
            ]
            .concat(),
        ),
        Scene {
            name: "showcase-light-code",
            opens: Opens::Code("weapons.qc", "void() W_FireRocket ="),
            theme: egui::Theme::Light,
            steps: Vec::new(),
        },
        Scene {
            name: "showcase-light-map",
            opens: Opens::Map("e1/lq_e1m1.map"),
            theme: egui::Theme::Light,
            steps: [vec![lit(), room(), clear()], wide().into()].concat(),
        },
        // Edit a line, save, and the game in the Game tab takes the new program as it plays:
        // the level's light flickers. The Console shows the reload.
        Scene::dark(
            "clip-hot-reload",
            Opens::CodeAfter("client.qc", PRETHINK_LINE),
            [
                vec![
                    show("console"),
                    Step::GameInTab,
                    Step::Run(PLAY, json!({ "map": START_MAP })),
                    Step::Wait(Until::Time(GAME_START)),
                    record(45),
                    Step::Snap("showcase-game"),
                    Step::OpenAfter("client.qc", PRETHINK_LINE),
                    record(15),
                    Step::Key(Modifiers::NONE, Key::Enter),
                ],
                typing(FLICKER_LINE),
                vec![
                    record(15),
                    Step::Key(Modifiers::COMMAND, Key::S),
                    record(30),
                    Step::Snap("showcase-hot-reload"),
                    Step::Click(Text::Game),
                    record(75),
                ],
            ]
            .concat(),
        ),
        // The game runs in the Game tab, a breakpoint is set, the game stops at it and the
        // editor shows the line; then two steps over it.
        Scene::dark(
            "clip-debugger",
            Opens::Code("ai.qc", "void() ai_stand ="),
            vec![
                watch("self.classname"),
                watch("self.health"),
                Step::GameInTab,
                Step::Run(PLAY, json!({ "map": DEBUGGER_MAP })),
                Step::Wait(Until::Time(GAME_START)),
                record(45),
                Step::Breakpoint("ai.qc", "if (time > self.pausetime)"),
                Step::RecordUntil(Until::Stopped, 150),
                record(30),
                show("debugger"),
                record(20),
                over(),
                record(30),
                over(),
                record(45),
            ],
        ),
        // The agent reads, stages its change, compiles and runs the tests; its own check
        // passes the scenario; the modder accepts its hunks.
        Scene::dark(
            "clip-agent",
            Opens::Code("client.qc", PARM_LINE.0),
            vec![
                show("agent"),
                Step::StartAgent,
                Step::RecordUntil(Until::Asked(1), 300),
                Step::Reply(read_parms),
                Step::RecordUntil(Until::Asked(2), 300),
                record(15),
                Step::Reply(stage_health),
                Step::RecordUntil(Until::Asked(3), 300),
                record(30),
                Step::Reply(compile_and_test),
                Step::RecordUntil(Until::Asked(4), 300),
                record(30),
                Step::Snap("showcase-agent"),
                Step::Reply(finish),
                Step::RecordUntil(Until::AgentDone, 300),
                record(45),
                Step::Snap("showcase-agent-done"),
                Step::Click(Text::AgentAcceptAll),
                record(45),
            ],
        ),
        // The pool room textured while the camera circles it, compiled, then lit. The window
        // waits on the compile, which the clip leaves out; the Properties panel says how long
        // it took.
        Scene::dark(
            "clip-orbit",
            Opens::MapCopy("e1/lq_e1m1.map", "e1/lq_e1m1_orbit.map"),
            [
                vec![
                    textured(),
                    Step::Orbit(Orbit {
                        centre: [-100.0, -700.0, 60.0],
                        radius: 503.0,
                        height: 110.0,
                        from: -17.4,
                        step: 0.08,
                        pitch: 12.0,
                    }),
                    clear(),
                ],
                wide().into(),
                vec![
                    record(60),
                    Step::Compile(PROFILE),
                    Step::Wait(Until::Compiled),
                    lit(),
                    record(90),
                ],
            ]
            .concat(),
        ),
    ]
}

/// The scripted model's first reply: it reads the file the player's parms are set in.
fn read_parms(_: &Path) -> Result<Reply, String> {
    Ok(calls(
        "A new player's health is `parm2`, set in `SetNewParms`. Reading `client.qc` first.",
        vec![("file.read", json!({ "path": "client.qc" }))],
    ))
}

/// The second: the change, staged as hunks.
fn stage_health(code: &Path) -> Result<Reply, String> {
    let path = code.join("client.qc");
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let (old, new) = PARM_LINE;
    if text.matches(old).count() != 1 {
        return Err(format!("{old:?} is not once in {}", path.display()));
    }
    Ok(calls(
        "`SetNewParms` gives a new player 100 health. Staging the change to 150.",
        vec![(
            "edit.stage_change",
            json!({ "files": [{ "path": "client.qc", "text": text.replace(old, new) }] }),
        )],
    ))
}

/// The third: the project compiled and its tests run.
fn compile_and_test(_: &Path) -> Result<Reply, String> {
    Ok(calls(
        "Compiling, then running the tests, `tests/spawn-health.scenario` among them.",
        vec![("project.compile", json!({})), ("project.test", json!({}))],
    ))
}

/// The last: done.
fn finish(_: &Path) -> Result<Reply, String> {
    Ok(Reply::Finish {
        summary: "New players spawn with 150 health: `parm2 = 150` in `SetNewParms`, in \
            `client.qc`. The project compiles clean and `spawn-health.scenario` passes."
            .to_owned(),
    })
}

/// A reply that says `text` and makes `calls`.
fn calls(text: &str, calls: Vec<(&str, Value)>) -> Reply {
    Reply::Calls {
        text: text.to_owned(),
        calls: calls
            .into_iter()
            .enumerate()
            .map(|(n, (name, arguments))| ToolCall {
                id: format!("c{n}"),
                name: name.to_owned(),
                arguments,
            })
            .collect(),
    }
}

/// The agent's model in the showcase: each reply the scene gives it, in order, written a word
/// at a time as a model streams it; and a count of the replies it was asked for.
struct Scripted {
    replies: Receiver<Reply>,
    asked: Arc<AtomicUsize>,
}

impl Model for Scripted {
    fn reply(&mut self, _: &str, _: &[ToolSpec], _: &[Message]) -> Result<Reply, ModelError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.replies
            .recv()
            .map_err(|_| ModelError::Stopped("the showcase ended".to_owned()))
    }

    fn reply_streaming(
        &mut self,
        system: &str,
        tools: &[ToolSpec],
        transcript: &[Message],
        on_text: &mut dyn FnMut(&str),
    ) -> Result<Reply, ModelError> {
        let reply = self.reply(system, tools, transcript)?;
        let (Reply::Calls { text, .. } | Reply::Finish { summary: text }) = &reply;
        for word in text.split_inclusive(' ') {
            on_text(word);
            std::thread::sleep(WORD_PAUSE);
        }
        Ok(reply)
    }
}

/// The scripted model's side the scene holds: where its replies go, and how often it asked.
struct Script {
    replies: Sender<Reply>,
    asked: Arc<AtomicUsize>,
}

/// Writes a clip's frames as PNGs on threads of their own, so the clip keeps its pace.
struct Writer {
    frames: Option<SyncSender<(PathBuf, image::RgbaImage)>>,
    threads: Vec<JoinHandle<Result<(), String>>>,
}

impl Writer {
    /// [`WRITERS`] threads, waiting for frames.
    fn new() -> Self {
        let (frames, taken) = mpsc::sync_channel::<(PathBuf, image::RgbaImage)>(WRITE_QUEUE);
        let taken = Arc::new(Mutex::new(taken));
        let threads = (0..WRITERS)
            .map(|_| {
                let taken = Arc::clone(&taken);
                std::thread::spawn(move || {
                    loop {
                        let next = taken.lock().map_err(|e| e.to_string())?.recv();
                        let Ok((path, image)) = next else {
                            return Ok(());
                        };
                        write_png(&path, &image)?;
                    }
                })
            })
            .collect();
        Self {
            frames: Some(frames),
            threads,
        }
    }

    /// Queues `image` to be written to `path`.
    fn write(&self, path: PathBuf, image: image::RgbaImage) -> Result<(), String> {
        self.frames
            .as_ref()
            .ok_or("the writer has finished")?
            .send((path, image))
            .map_err(|e| e.to_string())
    }

    /// Waits for every frame to be written.
    fn finish(mut self) -> Result<(), String> {
        self.frames = None;
        for thread in self.threads.drain(..) {
            thread.join().map_err(|_| "a writer failed".to_owned())??;
        }
        Ok(())
    }
}

/// Writes `image` to `path` as a PNG, compressed fast: a clip's frames are encoded again later.
fn write_png(path: &Path, image: &image::RgbaImage) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    PngEncoder::new_with_quality(
        std::io::BufWriter::new(file),
        CompressionType::Fast,
        FilterType::Sub,
    )
    .write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|e| format!("{}: {e}", path.display()))
}

fn main() -> ExitCode {
    let args = Args::parse();
    if let Err(error) = run(&args) {
        // The one line the showcase prints on failure: why it stopped.
        #[expect(clippy::print_stderr, reason = "an example's command-line error")]
        {
            eprintln!("showcase: {error}");
        }
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Renders every scene into `args.out`.
fn run(args: &Args) -> Result<(), String> {
    std::fs::create_dir_all(&args.out).map_err(|e| e.to_string())?;
    let game = args.librequake.join("lq1");
    // A profile of the BSP, full vis and full light, with no launch: the showcase has no game
    // to start.
    let profiles = game.join("compile-profiles.toml");
    if profiles.exists() {
        let held = std::fs::read_to_string(&profiles).map_err(|e| e.to_string())?;
        if !held.contains(PROFILES) {
            return Err(format!(
                "{} lacks the Showcase profile; delete it to write it",
                profiles.display()
            ));
        }
    } else {
        std::fs::write(&profiles, PROFILES).map_err(|e| e.to_string())?;
    }
    pinned(&game.join("maps/src/e1/lq_e1m1.map"))?;
    let code = args.librequake.join("qcsrc");
    for (file, text) in [("quakec.toml", MANIFEST), (SCENARIO_FILE, SCENARIO)] {
        let path = code.join(file);
        if !path.exists() {
            if let Some(folder) = path.parent() {
                std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, text).map_err(|e| e.to_string())?;
        }
    }
    for scene in scenes() {
        if args
            .only
            .as_ref()
            .is_some_and(|only| !scene.name.contains(only.as_str()))
        {
            continue;
        }
        Shoot::new(args, &scene)?.scene(&scene)?;
    }
    Ok(())
}

/// One scene being shot: its window, where it writes, and a clip's state.
struct Shoot<'a> {
    args: &'a Args,
    harness: Harness<'static, Option<App>>,
    kind: Kind,
    /// The open map or QuakeC file.
    path: PathBuf,
    /// The QuakeC folder.
    code: PathBuf,
    /// A clip's frames' folder.
    frames: PathBuf,
    /// The clip's next frame.
    frame: usize,
    /// The frames that took longer than [`CLIP_FRAME`].
    late: usize,
    orbit: Option<(Orbit, usize)>,
    writer: Option<Writer>,
    script: Option<Script>,
    /// The QuakeC file the scene opened and its text, written back when the scene has saved it.
    restore: Option<(PathBuf, String)>,
}

impl<'a> Shoot<'a> {
    /// The window for `scene`, with what it opens open.
    fn new(args: &'a Args, scene: &Scene) -> Result<Self, String> {
        let kind = scene.kind();
        let mut harness = harness(args.scale, scene.theme, kind)?;
        let game = args.librequake.join("lq1");
        let code = args.librequake.join("qcsrc");
        let path = match scene.opens {
            Opens::Map(path) => {
                let map = game.join("maps/src").join(path);
                open_map(&mut harness, &game, &map)?;
                compiled(&mut harness, &map)?;
                map
            }
            Opens::MapCopy(from, to) => {
                let map = game.join("maps/src").join(to);
                std::fs::copy(game.join("maps/src").join(from), &map).map_err(|e| e.to_string())?;
                let bsp = map.with_extension("bsp");
                if bsp.exists() {
                    std::fs::remove_file(&bsp).map_err(|e| e.to_string())?;
                }
                open_map(&mut harness, &game, &map)?;
                map
            }
            Opens::Game => {
                execute(&mut harness, "project.open", json!({ "path": game }))?;
                harness.run();
                game.clone()
            }
            Opens::Code(file, text) | Opens::CodeAfter(file, text) => {
                let path = code.join(file);
                let after = offset_after(&path, text)?;
                let offset = if matches!(scene.opens, Opens::CodeAfter(..)) {
                    after
                } else {
                    after.saturating_sub(text.len())
                };
                execute(&mut harness, "project.open", json!({ "path": code }))?;
                execute(
                    &mut harness,
                    "file.open",
                    json!({ "path": path, "offset": offset }),
                )?;
                harness.run();
                path
            }
        };
        let restore = match scene.opens {
            Opens::Code(..) | Opens::CodeAfter(..) => {
                let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                Some((path.clone(), text))
            }
            Opens::Map(_) | Opens::MapCopy(..) | Opens::Game => None,
        };
        let frames = args.out.join(scene.name);
        let writer = if kind == Kind::Clip {
            if frames.exists() {
                std::fs::remove_dir_all(&frames).map_err(|e| e.to_string())?;
            }
            std::fs::create_dir_all(&frames).map_err(|e| e.to_string())?;
            Some(Writer::new())
        } else {
            None
        };
        Ok(Self {
            args,
            harness,
            kind,
            path,
            code,
            frames,
            frame: 0,
            late: 0,
            orbit: None,
            writer,
            script: None,
            restore,
        })
    }

    /// Runs the scene's steps and writes its image or frames.
    fn scene(mut self, scene: &Scene) -> Result<(), String> {
        for step in &scene.steps {
            self.step(scene, step)?;
            // A clip's steps before its first frame set it up, drawn as a still's are.
            if self.kind == Kind::Still || self.frame == 0 {
                self.harness.run_steps(SETTLE_FRAMES);
            }
        }
        if self.kind == Kind::Still {
            self.snap(scene.name)?;
        }
        let app = self.app()?;
        if app.shell_mut().session().context().preview.state() != PlayState::Stopped {
            app.shell_mut()
                .execute(STOP, json!({}))
                .map_err(|e| format!("{STOP}: {e}"))?;
        }
        // The scripted model's thread ends when its replies' sender is dropped.
        self.script = None;
        if let Some((path, text)) = self.restore.take()
            && std::fs::read_to_string(&path).map_err(|e| e.to_string())? != text
        {
            std::fs::write(&path, text).map_err(|e| e.to_string())?;
        }
        if let Some(writer) = self.writer.take() {
            writer.finish()?;
            #[expect(
                clippy::print_stdout,
                reason = "the showcase's report of a clip's pace"
            )]
            {
                println!(
                    "{}: {} frames, {} of them longer than a thirtieth of a second",
                    scene.name, self.frame, self.late
                );
            }
        }
        Ok(())
    }

    /// The app.
    fn app(&mut self) -> Result<&mut App, String> {
        self.harness
            .state_mut()
            .as_mut()
            .ok_or_else(|| "no app".to_owned())
    }

    /// Writes the window as it is now to `<out>/<name>.png`.
    fn snap(&mut self, name: &str) -> Result<(), String> {
        let image = self.harness.render()?;
        let path = self.args.out.join(format!("{name}.png"));
        image.save(&path).map_err(|e| e.to_string())
    }

    /// Runs one frame of a clip in its thirtieth of a second, the orbit's camera put first,
    /// and writes it as the clip's next frame when `write` says so.
    fn frame(&mut self, write: Write) -> Result<(), String> {
        let started = Instant::now();
        if let Some((orbit, at)) = &mut self.orbit {
            let camera = orbit.camera(*at);
            *at = at.saturating_add(1);
            self.app()?.shell_mut().set_map_camera(camera);
        }
        self.harness.step();
        if write == Write::Yes {
            let image = self.harness.render()?;
            let path = self.frames.join(format!("frame-{:04}.png", self.frame));
            self.writer
                .as_ref()
                .ok_or("a still records no frames")?
                .write(path, image)?;
            self.frame = self.frame.saturating_add(1);
        }
        match CLIP_FRAME.checked_sub(started.elapsed()) {
            Some(left) => std::thread::sleep(left),
            None => self.late = self.late.saturating_add(1),
        }
        Ok(())
    }

    /// Whether `until` holds, or why waiting for it failed.
    fn holds(&mut self, until: Until, since: Instant) -> Result<bool, String> {
        let asked = self
            .script
            .as_ref()
            .map_or(0, |script| script.asked.load(Ordering::SeqCst));
        let map = self.path.clone();
        let app = self.app()?;
        let preview = &app.shell_mut().session().context().preview;
        if let Some(error) = preview
            .status()
            .filter(|_| preview.state() == PlayState::Stopped)
            .filter(|_| matches!(until, Until::Stopped))
        {
            return Err(format!("the game stopped: {error}"));
        }
        Ok(match until {
            Until::Time(time) => since.elapsed() >= time,
            Until::Stopped => preview.debugger().stopped().is_some(),
            Until::Compiled => newer(&map.with_extension("bsp"), &map),
            Until::Asked(times) => asked >= times,
            Until::AgentDone => app.shell().agent_report(ThreadIndex::new(0)).is_some(),
        })
    }

    /// Runs frames until `until` holds, writing them when `write` says so, at most `most`.
    fn until(&mut self, until: Until, most: usize, write: Write) -> Result<(), String> {
        let since = Instant::now();
        for _ in 0..most {
            if self.holds(until, since)? {
                return Ok(());
            }
            if since.elapsed() > CLIP_WAIT {
                break;
            }
            match self.kind {
                Kind::Clip => self.frame(write)?,
                Kind::Still => {
                    self.harness.step();
                    std::thread::sleep(PREVIEW_FRAME);
                }
            }
        }
        if self.holds(until, since)? {
            Ok(())
        } else {
            Err("a scene waited too long".to_owned())
        }
    }

    /// Runs `step` of `scene`.
    fn step(&mut self, scene: &Scene, step: &Step) -> Result<(), String> {
        match step {
            Step::Run(id, params) => {
                execute(&mut self.harness, id, params.clone())?;
            }
            Step::HidePoints(keep) => {
                let hidden = point_entities(&self.path, *keep)?;
                execute(&mut self.harness, "map.hide", json!({ "entities": hidden }))?;
            }
            Step::Breakpoint(file, text) => {
                let source =
                    std::fs::read_to_string(self.code.join(file)).map_err(|e| e.to_string())?;
                let line = source
                    .lines()
                    .position(|line| line.contains(text))
                    .and_then(|index| u32::try_from(index).ok())
                    .ok_or_else(|| format!("{text} not in {file}"))?;
                execute(
                    &mut self.harness,
                    BREAKPOINT,
                    json!({ "file": file, "line": line.saturating_add(1) }),
                )?;
            }
            Step::PlayToStop(on) => {
                execute(&mut self.harness, PLAY, json!({ "map": on }))?;
                stopped(&mut self.harness)?;
                let game = self.args.out.join(format!("{}-game.png", scene.name));
                execute(
                    &mut self.harness,
                    ENGINE_SCREENSHOT,
                    json!({ "path": game }),
                )?;
            }
            Step::Click(label) => {
                self.harness
                    .get_by_role_and_label(Role::Button, label.get())
                    .click();
                self.harness.step();
                // The pointer leaves the window, so the shot shows no cursor on the button.
                self.harness.event(egui::Event::PointerGone);
            }
            Step::Key(modifiers, key) => self.harness.key_press_modifiers(*modifiers, *key),
            Step::Type(text) => self.harness.event(egui::Event::Text(text.clone())),
            Step::Look(origin, yaw, pitch) => {
                let (origin, yaw, pitch) = match self.args.look.as_deref() {
                    Some(&[x, y, z, yaw, pitch]) => ([x, y, z], yaw, pitch),
                    _ => (*origin, *yaw, *pitch),
                };
                let mut camera = Camera::new(origin, Degrees::new(yaw));
                camera.pitch = Degrees::new(pitch);
                self.app()?.shell_mut().set_map_camera(camera);
            }
            Step::Record { frames, every } => {
                for at in 0..*frames {
                    let write = if at.is_multiple_of((*every).max(1)) {
                        Write::Yes
                    } else {
                        Write::No
                    };
                    self.frame(write)?;
                }
            }
            Step::RecordUntil(until, most) => self.until(*until, *most, Write::Yes)?,
            Step::Wait(until) => self.until(*until, usize::MAX, Write::No)?,
            Step::Orbit(orbit) => {
                self.orbit = Some((*orbit, 0));
                self.app()?.shell_mut().set_map_camera(orbit.camera(0));
            }
            Step::Snap(name) => self.snap(name)?,
            Step::Compile(profile) => self.compile(profile)?,
            Step::Generate(seed) => {
                let map = self
                    .path
                    .join("maps/src")
                    .join(format!("showcase-{seed}.map"));
                for old in [map.clone(), map.with_extension("bsp")] {
                    if old.exists() {
                        std::fs::remove_file(&old).map_err(|e| e.to_string())?;
                    }
                }
                execute(
                    &mut self.harness,
                    "map.generate",
                    json!({ "path": map, "seed": seed, "size": "medium" }),
                )?;
                self.harness.run();
                self.path = map;
                self.compile(PROFILE)?;
            }
            Step::OpenAfter(file, text) => {
                let path = self.code.join(file);
                let offset = offset_after(&path, text)?;
                let ctx = self.harness.ctx.clone();
                // As the system hands the window a file: the editor's tab is shown.
                self.app()?.shell_mut().open_paths([path.clone()]);
                self.harness.step();
                execute(
                    &mut self.harness,
                    "file.open",
                    json!({ "path": path, "offset": offset }),
                )?;
                self.app()?.shell().focus_editor(&ctx);
            }
            Step::GameInTab => self.game_in_tab()?,
            Step::StartAgent => self.start_agent()?,
            Step::Reply(make) => {
                let reply = make(&self.code)?;
                self.script
                    .as_ref()
                    .ok_or("no agent started")?
                    .replies
                    .send(reply)
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    /// Compiles the open map with `profile`, as its Compile button does.
    fn compile(&mut self, profile: &str) -> Result<(), String> {
        execute(
            &mut self.harness,
            "map.choose_profile",
            json!({ "profile": profile }),
        )?;
        self.harness.step();
        self.harness
            .get_by_label(Text::MapCompileProfile.get())
            .click();
        self.harness.step();
        self.harness.event(egui::Event::PointerGone);
        Ok(())
    }

    /// Puts the game in the window's Game tab, in this process.
    fn game_in_tab(&mut self) -> Result<(), String> {
        let tab = json!("tab");
        let toggle = |harness: &mut Harness<'static, Option<App>>| {
            execute(harness, qs_layout::TOGGLE_EMBEDDED_GAME, json!({}))
        };
        if toggle(&mut self.harness)? != tab && toggle(&mut self.harness)? != tab {
            return Err("the game's place is not the Game tab".to_owned());
        }
        // The window gives the preview the new place on its next frame.
        self.harness.step();
        Ok(())
    }

    /// Starts the agent on [`AGENT_TASK`], on a scripted model the scene gives its replies.
    fn start_agent(&mut self) -> Result<(), String> {
        let (replies, taken) = mpsc::channel();
        let asked = Arc::new(AtomicUsize::new(0));
        let model = Mutex::new(Some(Scripted {
            replies: taken,
            asked: Arc::clone(&asked),
        }));
        self.script = Some(Script { replies, asked });
        let ctx = self.harness.ctx.clone();
        let shell = self.app()?.shell_mut();
        *shell.agent_mut() = AgentPanel::new(Box::new(move |_config| {
            let scripted = model
                .lock()
                .map_err(|e| e.to_string())?
                .take()
                .ok_or("the showcase scripts one thread")?;
            let model: Box<dyn Model + Send> = Box::new(scripted);
            Ok(model)
        }));
        shell
            .agent_mut()
            .set_config(ProviderConfig::new(ProviderKind::Ollama, "scripted"));
        shell.start_agent(AGENT_TASK, &ctx)
    }
}

/// Whether a clip's frame is written.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Write {
    /// Written as the clip's next frame.
    Yes,
    /// Run only.
    No,
}

/// Runs the command `id` with `params` in the app.
fn execute(
    harness: &mut Harness<'static, Option<App>>,
    id: &str,
    params: Value,
) -> Result<Value, String> {
    harness
        .state_mut()
        .as_mut()
        .ok_or("no app")?
        .shell_mut()
        .execute(id, params)
        .map_err(|e| format!("{id}: {e}"))
}

/// Opens the game folder `game` as the project and the map at `map` in it.
fn open_map(
    harness: &mut Harness<'static, Option<App>>,
    game: &Path,
    map: &Path,
) -> Result<(), String> {
    execute(harness, "project.open", json!({ "path": game }))?;
    execute(harness, "map.open", json!({ "path": map }))?;
    harness.run();
    Ok(())
}

/// The offset in the file at `path` just after the first `text` in it.
fn offset_after(path: &Path, text: &str) -> Result<usize, String> {
    let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    source
        .find(text)
        .map(|start| start.saturating_add(text.len()))
        .ok_or_else(|| format!("{text} not in {}", path.display()))
}

/// Whether the file at `path` was written after the one at `than`.
fn newer(path: &Path, than: &Path) -> bool {
    let modified = |path: &Path| std::fs::metadata(path).and_then(|m| m.modified()).ok();
    modified(than).is_some() && modified(path) >= modified(than)
}

/// Runs frames until the game stops at a breakpoint, or [`STOP_WAIT`] passes.
fn stopped(harness: &mut Harness<'static, Option<App>>) -> Result<(), String> {
    let started = Instant::now();
    loop {
        harness.step();
        let app = harness.state_mut().as_mut().ok_or("no app")?;
        let preview = &app.shell_mut().session().context().preview;
        if preview.debugger().stopped().is_some() {
            return Ok(());
        }
        if let Some(error) = preview
            .status()
            .filter(|_| preview.state() == PlayState::Stopped)
        {
            return Err(format!("the game stopped: {error}"));
        }
        if started.elapsed() > STOP_WAIT {
            return Err("the game never reached the breakpoint".to_owned());
        }
        std::thread::sleep(PREVIEW_FRAME);
    }
}

/// Every entity of the map at `path` with no brush, the world aside, but `keep`.
fn point_entities(path: &Path, keep: Option<usize>) -> Result<Vec<usize>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let file = qs_map::MapFile::parse(&bytes).map_err(|e| e.to_string())?;
    Ok(file
        .entities
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(index, entity)| Some(*index) != keep && entity.brushes().next().is_none())
        .map(|(index, _)| index)
        .collect())
}

/// Compiles the open map with [`PROFILE`], unless its BSP is newer than it.
fn compiled(harness: &mut Harness<'static, Option<App>>, map: &Path) -> Result<(), String> {
    let bsp = map.with_extension("bsp");
    if newer(&bsp, map) {
        return Ok(());
    }
    execute(harness, "map.choose_profile", json!({ "profile": PROFILE }))?;
    harness.run();
    harness.get_by_label(Text::MapCompileProfile.get()).click();
    let started = Instant::now();
    while !newer(&bsp, map) {
        if started.elapsed() > COMPILE_WAIT {
            return Err(format!("{} did not compile", map.display()));
        }
        harness.step();
        std::thread::sleep(PREVIEW_FRAME);
    }
    harness.event(egui::Event::PointerGone);
    harness.run_steps(SETTLE_FRAMES);
    Ok(())
}

/// Checks the map at `path` is the one the scenes were framed on: [`GREEN_LIGHT`] is its green
/// light, as at LibreQuake's commit [`LIBREQUAKE`].
fn pinned(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let file = qs_map::MapFile::parse(&bytes).map_err(|e| e.to_string())?;
    let light = file.entities.get(GREEN_LIGHT);
    if light.and_then(|e| e.value(b"_color")) != Some(&b"143 240 164"[..]) {
        return Err(format!(
            "{} is not LibreQuake's at {LIBREQUAKE}: entity {GREEN_LIGHT} is not its green light",
            path.display()
        ));
    }
    Ok(())
}

/// A harness running the app in a window of [`SIZE`] points at `scale` pixels a point, in the
/// system look `theme`, on a wgpu device it can draw maps on; a clip's steps a thirtieth of a
/// second.
fn harness(
    scale: f32,
    theme: egui::Theme,
    kind: Kind,
) -> Result<Harness<'static, Option<App>>, String> {
    let state = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        egui_wgpu::RendererOptions {
            depth_stencil_format: Some(DEPTH_FORMAT),
            ..egui_wgpu::RendererOptions::PREDICTABLE
        },
    );
    let builder = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(scale)
        .with_theme(theme);
    let builder = match kind {
        Kind::Still => builder,
        Kind::Clip => builder
            .with_step_dt(CLIP_STEP)
            .with_max_steps(CLIP_MAX_STEPS),
    };
    let mut harness = builder
        .renderer(DepthRenderer {
            offscreen: Offscreen::from_state(state.clone(), DEPTH_FORMAT),
        })
        .build_ui_state(
            |ui, app: &mut Option<App>| {
                if let Some(app) = app {
                    app.frame(ui);
                }
            },
            None,
        );
    let mut app = App::checked_on_the_frame(&harness.ctx, None).map_err(|e| e.to_string())?;
    app.shell_mut().set_render_state(Some(state));
    if theme == egui::Theme::Light {
        // As the app shows the system's light appearance.
        let light = Theme::load(Mode::Light).map_err(|e| e.to_string())?;
        light.apply(&harness.ctx);
        app.shell_mut().set_theme(light);
    }
    *harness.state_mut() = Some(app);
    harness.run();
    Ok(harness)
}
