// SPDX-License-Identifier: GPL-3.0-or-later

//! A window for each local player after the first (L-86, ADR-0031): its own keys and mouse,
//! the move they make sent through its loopback connection, and the view its own client
//! reads, drawn by the same renderer as the first player's. The console and the menus are the
//! first player's; a key here runs its binding as this player's console line (NET-7.12).

use eframe::egui;
use qs_client::{Console, Event, KeyDest, Models, Random};
use qs_render::{MapMesh, Play};
use qs_sim::ClientCommand;
use qs_sim::input::{Input, MoveSettings};
use qs_sim::keys::Bindings;

use super::{Click, Pointer, Shell, layer_size};
use crate::AXES;
use crate::contents::contents_at;
use crate::game::Game;
use crate::keys::{keynum, modifier_keys, mouse_key, runs_binding, wheel_key};
use crate::map_bsp::MapBsp;
use crate::seats::Seat;
use crate::settings::HostSettings;

/// A player's window's size when it opens, in points: half the first window's.
const SEAT_SIZE: [f32; 2] = [640.0, 400.0];

/// Whether a player's window is still open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SeatWindow {
    /// It is open, and the player plays on.
    Open,
    /// The player closed it, and leaves the game.
    Closed,
}

/// What a player's window shares with the first player's: the game, the map and what draws
/// it, and the keys' bindings.
pub(super) struct Shared<'a> {
    pub(super) game: &'a mut Game,
    pub(super) play: &'a mut Play,
    pub(super) mesh: &'a mut MapMesh,
    pub(super) bsp: &'a MapBsp,
    pub(super) bindings: &'a Bindings,
    pub(super) shell: Shell,
}

/// A local player after the first, and its window.
pub(super) struct SeatView {
    seat: Seat,
    /// The buttons and the impulse its keys hold (INPUT-2).
    input: Input,
    /// The mouse's motion since its last move (INPUT-2.4).
    motion: [f32; 2],
    /// Whether its window holds the mouse.
    pointer: Pointer,
    /// The modifier keys as its last frame saw them (INPUT-3.1).
    modifiers: egui::Modifiers,
    /// Its notify lines, what the server printed to it (CON-2).
    console: Console,
    /// Where its eye was last frame, for the contents shift (VIEW-4.3).
    eye: [f32; 3],
}

impl SeatView {
    /// The window of `seat`, not yet holding the mouse, its notify lines laid out with the
    /// first player's console's text size (CON-5.6).
    pub(super) fn new(seat: Seat, text_size: usize) -> Self {
        Self {
            seat,
            input: Input::new(),
            motion: [0.0; 2],
            pointer: Pointer::Free,
            modifiers: egui::Modifiers::NONE,
            console: Console::with_text_size(text_size),
            eye: [0.0; AXES],
        }
    }

    /// The player's seat on the next level, a new client for its new signon; the buttons it
    /// holds are kept as the first player's are.
    pub(super) fn rejoined(&mut self, seat: Seat) {
        self.seat = seat;
    }

    /// Its seat.
    pub(super) const fn seat(&self) -> &Seat {
        &self.seat
    }

    /// Its client's frame of `frametime` seconds, what the server printed to it put on its
    /// notify lines at `realtime`.
    pub(super) fn read(
        &mut self,
        game: &mut Game,
        frametime: f32,
        readers: (&mut dyn Models, &mut dyn Random),
        realtime: f64,
    ) {
        match self.seat.read(game.world_mut(), frametime, readers) {
            Ok(events) => {
                for event in events {
                    if let Event::Print(text) = event {
                        self.console.print(&text, realtime);
                    }
                }
            }
            Err(error) => tracing::warn!(%error, player = self.seat.player().number, "client"),
        }
    }

    /// Shows its window for a frame of `seconds`: its keys and mouse, its move for the next
    /// server frame, and its view; whether the player closed it.
    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        shared: &mut Shared<'_>,
        seconds: f32,
        realtime: f64,
    ) -> SeatWindow {
        let number = self.seat.player().number;
        let id = egui::ViewportId::from_hash_of(("player", number));
        let builder = egui::ViewportBuilder::default()
            .with_title(format!("QuakeC Studio preview: player {number}"))
            .with_inner_size(SEAT_SIZE);
        ctx.show_viewport_immediate(id, builder, |ui, _class| {
            if ui.input(|input| input.viewport().close_requested()) {
                return SeatWindow::Closed;
            }
            let rect = ui.max_rect();
            let response = ui.allocate_rect(rect, egui::Sense::click());
            let click = if response.clicked() {
                Click::Capture
            } else {
                Click::None
            };
            self.hold_pointer(ui, click);
            self.bind_keys(ui, shared);
            self.steer(shared.game, seconds);
            self.draw(ui, rect, shared, realtime);
            SeatWindow::Open
        })
    }

    /// Captures the mouse on a click in its window and frees it on Escape or when the window
    /// loses focus; while held, its motion turns the view (`IN_MouseMove`).
    fn hold_pointer(&mut self, ui: &egui::Ui, click: Click) {
        let (escape, focused, motion) = ui.input(|input| {
            let mut motion = [0.0_f32; 2];
            for event in &input.events {
                if let egui::Event::MouseMoved(delta) = event {
                    motion[0] += delta.x;
                    motion[1] += delta.y;
                }
            }
            (input.key_pressed(egui::Key::Escape), input.focused, motion)
        });
        let next = match self.pointer {
            Pointer::Free if click == Click::Capture => Pointer::Captured,
            Pointer::Captured if escape || !focused => Pointer::Free,
            same => same,
        };
        if next == self.pointer {
            if self.pointer == Pointer::Captured {
                self.motion[0] += motion[0];
                self.motion[1] += motion[1];
            }
            return;
        }
        let (grab, visible) = match next {
            Pointer::Captured => (egui::CursorGrab::Locked, false),
            Pointer::Free => (egui::CursorGrab::None, true),
        };
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::CursorGrab(grab));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::CursorVisible(visible));
        self.pointer = next;
    }

    /// `Key_Event` for its keys (INPUT-3.2), as the first player's window runs them in the
    /// game: each key going down runs its binding, each going up its `+` binding's `-`
    /// command, as this player's console line (NET-7.12); the buttons and the impulse it holds
    /// are this player's.
    fn bind_keys(&mut self, ui: &egui::Ui, shared: &mut Shared<'_>) {
        let (events, modifiers) = ui.input(|input| (input.events.clone(), input.modifiers));
        let mut keys: Vec<(u8, bool)> = Vec::new();
        let before = modifier_keys(self.modifiers);
        for (now, was) in modifier_keys(modifiers).into_iter().zip(before) {
            if now.1 != was.1 {
                keys.push(now);
            }
        }
        self.modifiers = modifiers;
        let held = self.pointer == Pointer::Captured;
        for event in events {
            match event {
                egui::Event::Key {
                    key,
                    pressed,
                    repeat: false,
                    ..
                } => {
                    if let Some(k) = keynum(key) {
                        keys.push((k, pressed));
                    }
                }
                egui::Event::PointerButton {
                    button, pressed, ..
                } if held => {
                    if let Some(k) = mouse_key(button) {
                        keys.push((k, pressed));
                    }
                }
                egui::Event::MouseWheel { delta, .. } if held => {
                    if let Some(k) = wheel_key(delta.y) {
                        keys.push((k, true));
                        keys.push((k, false));
                    }
                }
                _ => {}
            }
        }
        for (key, down) in keys {
            let text = if down {
                if !runs_binding(key, KeyDest::Game) {
                    continue;
                }
                shared.bindings.down(key)
            } else {
                shared.bindings.up(key)
            };
            let Some(text) = text else {
                continue;
            };
            let line = text.strip_suffix(b"\n").unwrap_or(&text);
            shared
                .game
                .type_console_as(self.seat.player().loopback, line);
        }
        // NET-7.12: what its console met, these keys' lines and what a `wait` left of them.
        let loopback = self.seat.player().loopback;
        for command in shared.game.take_client_commands_of(loopback) {
            self.client_command(shared.game, &command);
        }
    }

    /// A client's command its line met: the buttons and the impulse are this player's
    /// (INPUT-2.1); the console's, the menus', the bindings' and the demos' are the first
    /// player's, whose window has them.
    fn client_command(&mut self, game: &Game, command: &ClientCommand) {
        match command {
            ClientCommand::Impulse(n) => self.input.set_impulse(*n),
            ClientCommand::Press(button, key) => {
                let _printed = self.input.press(*button, *key);
            }
            ClientCommand::Release(button, key) => {
                let lookspring = game.world().setting("lookspring").unwrap_or(0.0);
                if self.input.release(*button, *key, lookspring).start {
                    self.seat.client_mut().start_pitch_drift();
                }
            }
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
            | ClientCommand::Record(_)
            | ClientCommand::StopDemo
            | ClientCommand::StartDemos(_)
            | ClientCommand::StopPlayback
            | ClientCommand::PlayDemo(_)
            | ClientCommand::TimeDemo(_)
            | ClientCommand::DemoLoop => {}
        }
    }

    /// Its move for a frame of `seconds` (INPUT-2), as `CL_AdjustAngles`, `CL_BaseMove` and
    /// `IN_MouseMove` make it, sent through its connection.
    fn steer(&mut self, game: &mut Game, seconds: f32) {
        let world = game.world();
        let settings = MoveSettings::read(&|name| world.setting(name).unwrap_or(0.0));
        let client = self.seat.client_mut();
        let [pitch, yaw, _] = client.viewangles();
        let mut angles = [pitch, yaw, 0.0];
        let motion = core::mem::take(&mut self.motion);
        let (cmd, drift) = self.input.frame(&settings, seconds, &mut angles, motion);
        client.set_viewangles(angles);
        if drift.stop {
            client.stop_pitch_drift();
        }
        if drift.start {
            client.start_pitch_drift();
        }
        self.seat.player().loopback.send(game.world_mut(), cmd);
    }

    /// Its client's view in `rect`, with its status bar and its notify lines at `realtime`.
    fn draw(&mut self, ui: &egui::Ui, rect: egui::Rect, shared: &mut Shared<'_>, realtime: f64) {
        let aspect = rect.width() / rect.height().max(1.0);
        let world = shared.game.world();
        let client = self.seat.client_mut();
        let read = HostSettings::read(
            |name| world.setting(name),
            *client.view_settings(),
            shared.mesh.look,
        );
        client.set_view_settings(read.view);
        // `V_SetContentsColor`: the leaf the eye was in last frame tints this one (VIEW-4.3).
        client.set_contents(contents_at(shared.bsp.bsp(), self.eye));
        let scene = client.scene();
        self.eye = scene.view.origin;
        let screen = layer_size(shared.mesh.look, rect);
        // The 2D layer is this window's while it draws, and the first player's again after.
        let first_screen = core::mem::replace(&mut shared.play.screen, screen);
        client.set_screen(screen[0], screen[1]);
        let mut hud = client.hud();
        self.console.resize(screen[0]);
        hud.console = self.console.notify(realtime);
        let callback = shared
            .play
            .frame(&scene, &hud, shared.bsp.bsp(), shared.mesh, aspect);
        shared.play.screen = first_screen;
        ui.painter().add(shared.shell.paint(rect, callback));
    }
}
