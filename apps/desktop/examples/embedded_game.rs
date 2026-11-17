// SPDX-License-Identifier: GPL-3.0-or-later

//! Plays a project in the window's Game tab, the game run in this process (ENG-10, ADR-0028),
//! with no display: the app in the snapshot harness on a real wgpu device, the layout's game
//! place set to the tab, Play, a number of frames each drawn and read back, the last one's
//! picture, and the time each frame took.
//!
//! ```text
//! QS_GAME_DIR=<game root> cargo run --release -p quakec-studio --example embedded_game -- \
//!     --project <QuakeC project> --map <map> --frames 300 --out <png>
//! ```
//!
//! The game root is one holding `id1`, such as LibreQuake's lite data
//! (`cargo xtask assets fetch`), read through `QS_GAME_DIR` as the app reads it.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use clap::Parser;
use eframe::egui::{self, TexturesDelta};
use eframe::egui_wgpu::{self, ScreenDescriptor};
use egui_kittest::{Harness, TestRenderer};
use qs_render::Offscreen;
use qs_workbench::{PLAY, PlayState, STOP};
use quakec_studio::App;
use quakec_studio::map_view::DEPTH_FORMAT;
use quakec_studio::shell::{CentreTab, Shell};
use serde_json::json;

/// The window's size in points, the snapshot harness's.
const SIZE: [f32; 2] = [1440.0, 900.0];
/// The frames run before the timed ones, in which the game loads its first pictures.
const WARM_UP: usize = 30;

/// The example's options.
#[derive(Parser)]
#[command(about = "Play a project in the window's Game tab, with no display")]
struct Args {
    /// The QuakeC project to build and play.
    #[arg(long)]
    project: PathBuf,
    /// The map to start on.
    #[arg(long, default_value = "start")]
    map: String,
    /// How many frames to time.
    #[arg(long, default_value_t = 300)]
    frames: usize,
    /// Where the window's picture after the last frame is written.
    #[arg(long)]
    out: PathBuf,
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

fn main() -> ExitCode {
    let args = Args::parse();
    #[expect(
        clippy::print_stdout,
        clippy::print_stderr,
        reason = "an example's result and its command-line error"
    )]
    match run(&args) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("embedded_game: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The app's shell.
fn shell<'a>(harness: &'a mut Harness<'static, Option<App>>) -> Result<&'a mut Shell, String> {
    harness
        .state_mut()
        .as_mut()
        .map(App::shell_mut)
        .ok_or_else(|| "no app".to_owned())
}

/// Runs the command `id` with `params`.
fn command(
    harness: &mut Harness<'static, Option<App>>,
    id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    shell(harness)?
        .execute(id, params)
        .map_err(|error| format!("{id}: {error}"))
}

/// Plays `args.project` in the Game tab and reports the frames' work.
fn run(args: &Args) -> Result<String, String> {
    let state = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        egui_wgpu::RendererOptions {
            depth_stencil_format: Some(DEPTH_FORMAT),
            ..egui_wgpu::RendererOptions::PREDICTABLE
        },
    );
    let mut harness: Harness<'static, Option<App>> = Harness::builder()
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
    harness.step();
    command(
        &mut harness,
        "project.open",
        json!({ "path": args.project }),
    )?;
    let place = command(&mut harness, qs_layout::TOGGLE_EMBEDDED_GAME, json!({}))?;
    if place != json!("tab") {
        return Err(format!("the game's place is {place}, not the tab"));
    }
    harness.step();
    command(&mut harness, PLAY, json!({ "map": args.map }))?;
    for _ in 0..WARM_UP {
        harness.step();
    }
    {
        let shell = shell(&mut harness)?;
        let preview = &shell.session().context().preview;
        if preview.state() == PlayState::Stopped || !preview.is_embedded() {
            return Err("the game does not run in the window".to_owned());
        }
        if shell.centre_tab() != CentreTab::Game {
            return Err("the Game tab is not shown".to_owned());
        }
    }
    // Each frame is the whole window's: the app's interface, a frame of the game, and the
    // picture drawn on the device and read back, which a window on a display does not pay for.
    let mut longest = 0.0_f64;
    let started = Instant::now();
    let mut image = harness.render()?;
    for _ in 0..args.frames {
        let frame = Instant::now();
        harness.step();
        image = harness.render()?;
        longest = longest.max(frame.elapsed().as_secs_f64());
    }
    let total = started.elapsed().as_secs_f64();
    image.save(&args.out).map_err(|e| e.to_string())?;
    command(&mut harness, STOP, json!({}))?;
    harness.step();
    if shell(&mut harness)?.session().context().preview.state() != PlayState::Stopped {
        return Err("the game did not stop".to_owned());
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a count of frames, a few thousand at most"
    )]
    let average = total / args.frames.max(1) as f64;
    Ok(format!(
        "embedded: {} on {}: {} frames drawn and read back, {:.2} ms a frame on average and \
         {:.2} ms the longest; picture at {}",
        args.project.display(),
        args.map,
        args.frames,
        average * 1000.0,
        longest * 1000.0,
        args.out.display()
    ))
}
