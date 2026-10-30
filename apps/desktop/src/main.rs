// SPDX-License-Identifier: GPL-3.0-or-later

//! `quakec-studio`: the desktop app.

use std::path::PathBuf;
use std::process::ExitCode;

use eframe::egui;
use quakec_studio::App;
use quakec_studio::strings::Text;

#[expect(
    clippy::print_stderr,
    reason = "a binary's crate root reports why the window could not open, or why logging is \
              off, to the person who ran it"
)]
fn main() -> ExitCode {
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
    let layout_file = dirs.ok().map(|dirs| qs_layout::layout_path(dirs.data()));
    // The one argument, if any, is a project directory to open.
    let project = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(Text::AppName.get())
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0]),
        // The map view tests depth inside egui's render pass (`map_view::DEPTH_FORMAT`).
        depth_buffer: 32,
        ..Default::default()
    };
    let result = eframe::run_native(
        Text::AppName.get(),
        options,
        Box::new(move |cc| {
            let mut app = App::new(&cc.egui_ctx, layout_file)?;
            app.shell_mut()
                .set_render_state(cc.wgpu_render_state.clone());
            if let Some(project) = &project
                && let Err(error) = app.shell_mut().open_project(project)
            {
                tracing::warn!(%error, "project not opened");
            }
            #[cfg(target_os = "macos")]
            app.install_menu();
            Ok(Box::new(app))
        }),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}: {error}", Text::AppName.get());
            ExitCode::FAILURE
        }
    }
}
