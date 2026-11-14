// SPDX-License-Identifier: GPL-3.0-or-later

//! `quakec-studio`: the desktop app.

use std::path::PathBuf;
use std::process::ExitCode;

use eframe::egui;
use qs_commands::remote::Host;
#[cfg(target_os = "macos")]
use quakec_studio::open_documents;
use quakec_studio::strings::Text;
use quakec_studio::{App, agent, keymap};
#[cfg(target_os = "macos")]
use winit::event_loop::EventLoop;

/// `quakec-studio [<path>...]`.
#[derive(clap::Parser, Debug)]
#[command(name = "quakec-studio", about = "The QuakeC Studio desktop app")]
struct Args {
    /// Project directories to open, or files to open in their projects; Windows and Linux
    /// pass the files opened with the app here.
    paths: Vec<PathBuf>,
}

#[expect(
    clippy::print_stderr,
    reason = "a binary's crate root reports why the window could not open, or why logging is \
              off, to the person who ran it"
)]
fn main() -> ExitCode {
    let Args { paths } = <Args as clap::Parser>::parse();
    let dirs = qs_paths::current();
    let logging = dirs.as_ref().map_err(ToString::to_string).and_then(|dirs| {
        qs_log::init(&qs_log::LogConfig::for_app(dirs)).map_err(|error| error.to_string())
    });
    if let Err(error) = logging {
        eprintln!(
            "{}: {}: {error}",
            Text::AppName.get(),
            Text::LoggingOff.get()
        );
    }
    let recent_file = dirs
        .as_ref()
        .ok()
        .map(|dirs| qs_workbench::recent_path(dirs.data()));
    let settings_file = dirs
        .as_ref()
        .ok()
        .map(|dirs| qs_settings::user_settings_path(dirs.config()));
    let app_socket = dirs
        .as_ref()
        .ok()
        .map(|dirs| dirs.app_socket().to_path_buf());
    let agent_file = dirs
        .as_ref()
        .ok()
        .map(|dirs| agent::config_path(dirs.data()));
    let keymap_file = dirs
        .as_ref()
        .ok()
        .map(|dirs| keymap::keymap_path(dirs.config()));
    let layout_file = dirs.ok().map(|dirs| qs_layout::layout_path(dirs.data()));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(Text::AppName.get())
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0]),
        // The map view tests depth inside egui's render pass (`map_view::DEPTH_FORMAT`).
        depth_buffer: 32,
        ..Default::default()
    };
    // Why the app could not start, which eframe's own event loop returns but one the app runs
    // itself does not (macOS, below).
    let failed = std::cell::OnceCell::new();
    let creator: eframe::AppCreator<'_> = Box::new(|cc| {
        let mut app = App::new(&cc.egui_ctx, layout_file).inspect_err(|error| {
            failed.get_or_init(|| error.to_string());
        })?;
        app.shell_mut()
            .set_render_state(cc.wgpu_render_state.clone());
        if let Some(file) = recent_file {
            app.shell_mut().set_recent_file(file);
        }
        // P3-44: the game folder chosen before, and on Windows and Linux the menu's items on
        // the toolbar, which macOS has in its menu bar.
        if let Some(file) = keymap_file {
            app.shell_mut().set_keymap_file(file);
        }
        if let Some(file) = settings_file {
            app.shell_mut().set_settings_file(file);
        }
        // P4-07: the agent's provider; its key is in the keychain.
        if let Some(file) = agent_file {
            app.shell_mut().set_agent_file(file);
        }
        #[cfg(not(target_os = "macos"))]
        app.shell_mut().set_menu_in_window();
        // AGT-7: `quakec-ide mcp --connect` runs the agent's tools in this window.
        if let Some(socket) = &app_socket {
            let ctx = cc.egui_ctx.clone();
            match Host::listen(socket, move || ctx.request_repaint()) {
                Ok(host) => app.shell_mut().set_host(host),
                Err(error) => tracing::warn!(%error, "the app's socket is not served"),
            }
        }
        app.shell_mut().open_paths(paths);
        #[cfg(target_os = "macos")]
        open_documents::set_context(&cc.egui_ctx);
        #[cfg(target_os = "macos")]
        app.install_menu();
        Ok(Box::new(app))
    });
    #[cfg(target_os = "macos")]
    let result = run_macos(options, creator);
    #[cfg(not(target_os = "macos"))]
    let result = eframe::run_native(Text::AppName.get(), options, creator);
    let error = match (result, failed.into_inner()) {
        (Ok(()), None) => return ExitCode::SUCCESS,
        (Err(error), _) => error.to_string(),
        (Ok(()), Some(error)) => error,
    };
    eprintln!("{}: {error}", Text::AppName.get());
    ExitCode::FAILURE
}

/// Runs the app on an event loop it builds itself, so the files Finder opens with it reach
/// it: the delegate method that receives them is added once winit has registered its
/// delegate and before the loop runs, when macOS sends the files of a launch.
#[cfg(target_os = "macos")]
fn run_macos(
    options: eframe::NativeOptions,
    creator: eframe::AppCreator<'_>,
) -> Result<(), eframe::Error> {
    let event_loop = EventLoop::<eframe::UserEvent>::with_user_event()
        .build()
        .map_err(eframe::Error::WinitEventLoop)?;
    if let Err(error) = open_documents::install() {
        tracing::warn!(%error, "files Finder opens with the app are not received");
    }
    let mut app = eframe::create_native(Text::AppName.get(), options, creator, &event_loop);
    event_loop
        .run_app(&mut app)
        .map_err(eframe::Error::WinitEventLoop)
}
