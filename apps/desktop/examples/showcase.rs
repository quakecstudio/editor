// SPDX-License-Identifier: GPL-3.0-or-later

//! The showcase screenshots (P7-14): the app rendered offscreen through wgpu, as the snapshot
//! harness renders it, on LibreQuake's maps compiled with full light and their own textures,
//! each scene a PNG for the website and the user guide.
//!
//! LibreQuake is not in this repository (corpus/manifest.toml), so the scenes are rendered from a
//! checkout of it whose `texture-wads/` holds its WADs built from their PNGs, as LibreQuake's
//! `compile_wads.py` builds them:
//!
//! ```text
//! cargo run --release -p quakec-studio --example showcase -- --librequake <dir> --out <dir>
//! ```
//!
//! The debugger scene plays LibreQuake's QuakeC in the preview host, so it needs the host built
//! (`cargo build --release -p quakec-engine`) and found through `QS_PREVIEW_HOST`, and a game
//! folder holding LibreQuake's lite data in `id1`, through `QS_GAME_DIR`
//! (`cargo xtask assets fetch` puts one in the test-assets cache).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::Parser;
use eframe::egui::{self, TexturesDelta, accesskit::Role};
use eframe::egui_wgpu::{self, ScreenDescriptor};
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, TestRenderer};
use qs_render::{Camera, Degrees, Offscreen};
use qs_workbench::engine::ENGINE_SCREENSHOT;
use qs_workbench::{BREAKPOINT, PLAY, PlayState, STOP, WATCH};
use quakec_studio::App;
use quakec_studio::map_view::DEPTH_FORMAT;
use quakec_studio::strings::Text;
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
/// The map the debugger scene plays: one of LibreQuake's lite game data, with monsters that
/// stand until they see the player.
const DEBUGGER_MAP: &str = "lq_e0m1";
/// How long the debugger scene waits for the game to reach its breakpoint.
const STOP_WAIT: Duration = Duration::from_secs(60);
/// How long the debugger scene waits between frames while the game starts.
const PREVIEW_FRAME: Duration = Duration::from_millis(50);
/// The frames run after each step, so what it did is drawn.
const SETTLE_FRAMES: usize = 4;

/// The showcase's options.
#[derive(Parser)]
#[command(about = "Render the showcase screenshots from a LibreQuake checkout")]
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
}

/// What a scene opens: a map of the game folder, compiled, or a QuakeC file of `qcsrc` at the
/// start of a line holding some text.
#[derive(Clone, Copy)]
enum Opens {
    /// The map at this path under `lq1/maps/src`.
    Map(&'static str),
    /// The QuakeC file, and the text whose line the editor shows.
    Code(&'static str, &'static str),
}

/// A scene: its file name, what it opens and its steps after.
struct Scene {
    name: &'static str,
    opens: Opens,
    steps: Vec<Step>,
}

/// The scenes, each from a LibreQuake map.
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
    vec![
        Scene {
            name: "showcase-code",
            opens: Opens::Code("weapons.qc", "void() W_FireRocket ="),
            steps: Vec::new(),
        },
        Scene {
            name: "showcase-debugger",
            opens: Opens::Code("ai.qc", "void() ai_stand ="),
            steps: vec![
                Step::Breakpoint("ai.qc", "if (time > self.pausetime)"),
                watch("self.classname"),
                watch("self.health"),
                watch("self.enemy.classname"),
                Step::PlayToStop(DEBUGGER_MAP),
                Step::Click(Text::Debugger),
                Step::Run("view.toggle_panel", json!({ "panel": "right" })),
            ],
        },
        Scene {
            name: "showcase-lit-wide",
            opens: Opens::Map("e1/lq_e1m1.map"),
            steps: [
                vec![lit(), room(), clear()],
                wide().into(),
                vec![Step::Run("view.toggle_panel", json!({ "panel": "right" }))],
            ]
            .concat(),
        },
        Scene {
            name: "showcase-textured",
            opens: Opens::Map("e1/lq_e1m1.map"),
            steps: [vec![textured(), room(), clear()], wide().into()].concat(),
        },
        Scene {
            name: "showcase-light-reach",
            opens: Opens::Map("e1/lq_e1m1.map"),
            steps: [
                vec![
                    lit(),
                    room(),
                    Step::HidePoints(Some(GREEN_LIGHT)),
                    Step::Run("map.select", json!({ "entity": GREEN_LIGHT })),
                ],
                wide().into(),
            ]
            .concat(),
        },
        Scene {
            name: "showcase-quad",
            opens: Opens::Map("e1/lq_e1m1.map"),
            steps: [
                vec![
                    lit(),
                    room(),
                    clear(),
                    Step::Run("view.map_panes", json!({ "panes": "quad" })),
                ],
                wide().into(),
            ]
            .concat(),
        },
    ]
}

fn main() -> ExitCode {
    let args = Args::parse();
    if let Err(error) = run(&args) {
        // The one line the showcase prints: why it stopped.
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
    if !profiles.exists() {
        std::fs::write(&profiles, PROFILES).map_err(|e| e.to_string())?;
    }
    for scene in scenes() {
        if args
            .only
            .as_ref()
            .is_some_and(|only| !scene.name.contains(only.as_str()))
        {
            continue;
        }
        let mut harness = harness()?;
        let map = match scene.opens {
            Opens::Map(path) => {
                let map = game.join("maps/src").join(path);
                let app = harness.state_mut().as_mut().ok_or("no app")?;
                let shell = app.shell_mut();
                shell
                    .execute("project.open", json!({ "path": game }))
                    .map_err(|e| e.to_string())?;
                shell
                    .execute("map.open", json!({ "path": map }))
                    .map_err(|e| e.to_string())?;
                harness.run();
                compiled(&mut harness, &map)?;
                map
            }
            Opens::Code(file, text) => {
                let code = args.librequake.join("qcsrc");
                let manifest = code.join("quakec.toml");
                if !manifest.exists() {
                    std::fs::write(&manifest, MANIFEST).map_err(|e| e.to_string())?;
                }
                let path = code.join(file);
                let source = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                let offset = source
                    .find(text)
                    .ok_or_else(|| format!("{text} not in {file}"))?;
                let app = harness.state_mut().as_mut().ok_or("no app")?;
                let shell = app.shell_mut();
                shell
                    .execute("project.open", json!({ "path": code }))
                    .map_err(|e| e.to_string())?;
                shell
                    .execute("file.open", json!({ "path": path, "offset": offset }))
                    .map_err(|e| e.to_string())?;
                harness.run();
                path
            }
        };
        for step in &scene.steps {
            let app = harness.state_mut().as_mut().ok_or("no app")?;
            match step {
                Step::Run(id, params) => {
                    app.shell_mut()
                        .execute(id, params.clone())
                        .map_err(|e| format!("{id}: {e}"))?;
                }
                Step::HidePoints(keep) => {
                    let hidden = point_entities(&map, *keep)?;
                    app.shell_mut()
                        .execute("map.hide", json!({ "entities": hidden }))
                        .map_err(|e| format!("map.hide: {e}"))?;
                }
                Step::Breakpoint(file, text) => {
                    let source = std::fs::read_to_string(map.with_file_name(file))
                        .map_err(|e| e.to_string())?;
                    let line = source
                        .lines()
                        .position(|line| line.contains(text))
                        .and_then(|index| u32::try_from(index).ok())
                        .ok_or_else(|| format!("{text} not in {file}"))?;
                    app.shell_mut()
                        .execute(
                            BREAKPOINT,
                            json!({ "file": file, "line": line.saturating_add(1) }),
                        )
                        .map_err(|e| format!("{BREAKPOINT}: {e}"))?;
                }
                Step::PlayToStop(on) => {
                    app.shell_mut()
                        .execute(PLAY, json!({ "map": on }))
                        .map_err(|e| format!("{PLAY}: {e}"))?;
                    stopped(&mut harness)?;
                    let game = args.out.join(format!("{}-game.png", scene.name));
                    let app = harness.state_mut().as_mut().ok_or("no app")?;
                    app.shell_mut()
                        .execute(ENGINE_SCREENSHOT, json!({ "path": game }))
                        .map_err(|e| format!("{ENGINE_SCREENSHOT}: {e}"))?;
                }
                Step::Click(label) => {
                    harness
                        .get_by_role_and_label(Role::Button, label.get())
                        .click();
                    harness.run_steps(1);
                    // The pointer leaves the window, so the shot shows no cursor on the tab.
                    harness.event(egui::Event::PointerGone);
                }
                Step::Look(origin, yaw, pitch) => {
                    let (origin, yaw, pitch) = match args.look.as_deref() {
                        Some(&[x, y, z, yaw, pitch]) => ([x, y, z], yaw, pitch),
                        _ => (*origin, *yaw, *pitch),
                    };
                    let mut camera = Camera::new(origin, Degrees::new(yaw));
                    camera.pitch = Degrees::new(pitch);
                    app.shell_mut().set_map_camera(camera);
                }
            }
            harness.run_steps(SETTLE_FRAMES);
        }
        let image = harness.render()?;
        let path = args.out.join(format!("{}.png", scene.name));
        image.save(&path).map_err(|e| e.to_string())?;
        let app = harness.state_mut().as_mut().ok_or("no app")?;
        if app.shell_mut().session().context().preview.state() != PlayState::Stopped {
            app.shell_mut()
                .execute(STOP, json!({}))
                .map_err(|e| format!("{STOP}: {e}"))?;
        }
    }
    Ok(())
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
    let modified = |path: &Path| std::fs::metadata(path).and_then(|m| m.modified()).ok();
    if modified(&map.with_extension("bsp")) >= modified(map) && modified(map).is_some() {
        return Ok(());
    }
    let app = harness.state_mut().as_mut().ok_or("no app")?;
    app.shell_mut()
        .execute("map.choose_profile", json!({ "profile": PROFILE }))
        .map_err(|e| e.to_string())?;
    harness.run();
    harness.get_by_label(Text::MapCompileProfile.get()).click();
    harness.run();
    harness.run();
    Ok(())
}

/// A harness running the app in a window of [`SIZE`] on a wgpu device it can draw maps on.
fn harness() -> Result<Harness<'static, Option<App>>, String> {
    let state = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        egui_wgpu::RendererOptions {
            depth_stencil_format: Some(DEPTH_FORMAT),
            ..egui_wgpu::RendererOptions::PREDICTABLE
        },
    );
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(1.0)
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
    *harness.state_mut() = Some(app);
    harness.run();
    Ok(harness)
}
