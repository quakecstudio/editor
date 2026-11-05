// SPDX-License-Identifier: GPL-3.0-or-later

//! `--headless`: the game with no window and no sound, for a bridge client's tests (P3-06).
//! Each frame is one of `Host_FilterTime`'s shortest, 1/72 of a second, run when that much
//! time has passed; the bridge is served between two server frames (BRIDGE-4.3), the client
//! reads the server's messages so `pick` looks from the player's eye, and level changes load
//! the next map as the window does.

use std::time::{Duration, Instant};

use qs_client::Client;

use crate::args::Args;
use crate::bridge::{Bridge, serve};
use crate::game::{Eye, Game, Screen};
use crate::load::{Acted, Describer, Loaded, Rand, act};
use crate::window::{FRAME, frametime_f32};

/// The view's width per unit of height `pick` assumes: the window's first size, 1280 by 800.
const ASPECT: f32 = 1.6;

/// Runs the game of `loaded` until the bridge's client stops the host or goes away.
pub(crate) fn run(args: &Args, loaded: Loaded, mut bridge: Bridge) -> Result<(), String> {
    let mut world = Headless::new(loaded);
    let frame = Duration::from_secs_f64(FRAME);
    let mut next = Instant::now();
    loop {
        if !serve(&mut bridge, &mut world.game) {
            return Ok(());
        }
        let ran = world.game.frame(FRAME);
        if let Some(action) = world.game.host_action() {
            match act(args, &mut world.game, action) {
                Acted::Load(loaded) => {
                    let old = core::mem::replace(&mut world, Headless::new(*loaded));
                    world.game.carry_from(old.game);
                    continue;
                }
                Acted::Quit => return Ok(()),
                Acted::Stay => {}
            }
        }
        // The client's clock stands while the server's does, as the window's.
        world.read_messages(if ran { FRAME } else { 0.0 });
        next = next.checked_add(frame).unwrap_or(next);
        if let Some(wait) = next.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
}

/// What a headless host keeps of a loaded map.
struct Headless {
    game: Game,
    client: Client,
    describer: Describer,
    rand: Rand,
    fov_x: f32,
}

impl Headless {
    fn new(loaded: Loaded) -> Self {
        let Loaded {
            mut game,
            client,
            describer,
            rand,
            camera,
            ..
        } = loaded;
        game.set_screen(Screen::Headless);
        Self {
            game,
            client,
            describer,
            rand,
            fov_x: camera.fov_x,
        }
    }

    /// The client's frame: it reads what the server wrote, and the eye `pick` looks from is
    /// the client's view.
    fn read_messages(&mut self, frametime: f64) {
        let packets = self.game.world_mut().take_messages().packets;
        let seconds = frametime_f32(frametime);
        if let Err(error) =
            self.client
                .frame(seconds, &packets, &mut self.describer, &mut self.rand)
        {
            tracing::warn!(%error, "client");
        }
        let view = self.client.scene().view;
        self.game.set_eye(Eye {
            origin: view.origin,
            angles: view.angles,
            fov_x: self.fov_x,
            aspect: ASPECT,
        });
    }
}
