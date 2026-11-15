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

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use eframe::egui::{self, TexturesDelta};
use eframe::egui_wgpu::{self, ScreenDescriptor};
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, TestRenderer};
use qs_render::{Camera, Degrees, Offscreen};
use quakec_studio::App;
use quakec_studio::map_view::DEPTH_FORMAT;
use quakec_studio::strings::Text;
use serde_json::{Value, json};

/// Where the hall of lq_e1m1 is seen from: east of its pillar, looking west.
const HALL: [f32; 3] = [200.0, 900.0, 400.0];
/// A red light of the hall: entity 185 of lq_e1m1, `_color 237 51 59`.
const RED_LIGHT: usize = 185;

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
    let hall = || Step::Look(HALL, 180.0, 10.0);
    let clear = || Step::HidePoints(None);
    vec![
        Scene {
            name: "showcase-code",
            opens: Opens::Code("weapons.qc", "void() W_FireRocket ="),
            steps: Vec::new(),
        },
        Scene {
            name: "showcase-lit",
            opens: Opens::Map("e1/lq_e1m1.map"),
            steps: vec![lit(), hall(), clear()],
        },
        Scene {
            name: "showcase-lit-wide",
            opens: Opens::Map("e1/lq_e1m1.map"),
            steps: [vec![lit(), hall(), clear()], wide().into()].concat(),
        },
        Scene {
            name: "showcase-textured",
            opens: Opens::Map("e1/lq_e1m1.map"),
            steps: [vec![textured(), clear()], wide().into()].concat(),
        },
        Scene {
            name: "showcase-light-reach",
            opens: Opens::Map("e1/lq_e1m1.map"),
            steps: [
                vec![
                    lit(),
                    hall(),
                    Step::HidePoints(Some(RED_LIGHT)),
                    Step::Run("map.select", json!({ "entity": RED_LIGHT })),
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
                    hall(),
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
            harness.run();
        }
        let image = harness.render()?;
        let path = args.out.join(format!("{}.png", scene.name));
        image.save(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
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
