// SPDX-License-Identifier: GPL-3.0-or-later

//! The embedded game view (ENG-10, ADR-0028): the game run in this process by `qs-game` and
//! drawn in the Game tab, where the preview host's own window shows it otherwise. The preview
//! starts and stops it through [`EmbeddedHost`] and speaks the bridge to it over two pipes; the
//! game and the bridge's client share this thread, so the client lets the game answer while it
//! waits.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use eframe::egui::{Context, Ui};
use eframe::egui_wgpu::RenderState;
use qs_engine::{Client, EmbeddedHost, LaunchError, LaunchSpec};
use qs_game::{Args, GameView, Running};
use qs_layout::GamePlace;

use crate::shell::CentreTab;

/// The game that runs in this window, where it is drawn and what it is drawn with: shared by
/// the shell, which draws it and says where the layout puts the game, and the preview's
/// [`EmbeddedHost`], which starts and stops it.
#[derive(Clone, Default)]
pub(crate) struct GameSlot {
    game: Rc<RefCell<Option<GameView>>>,
    state: Rc<RefCell<Option<RenderState>>>,
    /// The window's context, from the first frame, through which the mouse is given back.
    ctx: Rc<RefCell<Option<Context>>>,
    place: Rc<Cell<GamePlace>>,
}

impl core::fmt::Debug for GameSlot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GameSlot")
            .field("place", &self.place.get())
            .finish_non_exhaustive()
    }
}

impl GameSlot {
    /// The window's device, which the game is drawn on; none in a window with no device.
    pub(crate) fn set_render_state(&self, state: Option<RenderState>) {
        *self.state.borrow_mut() = state;
    }

    /// Where the layout puts the game, as the next Play reads it.
    pub(crate) fn set_place(&self, place: GamePlace) {
        self.place.set(place);
    }

    /// The host the preview starts the game through.
    pub(crate) fn host(&self) -> Box<dyn EmbeddedHost> {
        Box::new(Embedded(self.clone()))
    }

    /// Runs a frame of the game and draws it over `ui`; whether a game runs here. A game that
    /// stopped is dropped, which closes its bridge, so the preview's next poll stops.
    pub(crate) fn ui(&self, ui: &mut Ui) -> bool {
        let Ok(mut slot) = self.game.try_borrow_mut() else {
            return false;
        };
        let Some(game) = slot.as_mut() else {
            return false;
        };
        if game.ui(ui) == Running::Stopped {
            game.close(ui.ctx());
            *slot = None;
        }
        true
    }

    /// Follows the window: its context is kept, and while `tab`, the centre's, is not the Game
    /// tab a game that holds the mouse gives it back, since nothing there would free it.
    pub(crate) fn follow(&self, ctx: &Context, tab: CentreTab) {
        if self.ctx.borrow().is_none() {
            *self.ctx.borrow_mut() = Some(ctx.clone());
        }
        if tab != CentreTab::Game
            && let Ok(mut slot) = self.game.try_borrow_mut()
            && let Some(game) = slot.as_mut()
        {
            game.release_pointer(ctx);
        }
    }

    /// Whether the window shows the game now: the layout puts it in the Game tab, and the
    /// window has a device to draw it on.
    pub(crate) fn shows(&self) -> bool {
        self.place.get() == GamePlace::Tab && self.state.borrow().is_some()
    }

    /// Runs a frame of the game while its tab is not the one shown, drawn nowhere and given no
    /// keys or clicks, over the centre's area so a `screenshot` is the size it is seen at: the
    /// game goes on as it does in the preview host's window when that is covered.
    pub(crate) fn run_hidden(&self, ui: &mut Ui) {
        let mut hidden = ui.new_child(
            eframe::egui::UiBuilder::new()
                .max_rect(ui.max_rect())
                .invisible()
                .disabled(),
        );
        self.ui(&mut hidden);
    }

    /// Lets the game answer its bridge between two frames.
    fn serve(&self) {
        if let Ok(mut slot) = self.game.try_borrow_mut()
            && let Some(game) = slot.as_mut()
        {
            game.serve();
        }
    }

    /// Stops the game that runs here.
    fn stop(&self) {
        if let Ok(mut slot) = self.game.try_borrow_mut()
            && let Some(mut game) = slot.take()
        {
            match &*self.ctx.borrow() {
                Some(ctx) => game.close(ctx),
                // No frame was drawn, so the game never held the mouse.
                None => game.close(&Context::default()),
            }
        }
    }
}

/// The preview's way to this window's game.
struct Embedded(GameSlot);

impl EmbeddedHost for Embedded {
    fn shows(&self) -> bool {
        self.0.shows()
    }

    fn start(&mut self, spec: &LaunchSpec, client_name: &str) -> Result<Client, LaunchError> {
        self.0.stop();
        let Some(state) = self.0.state.borrow().clone() else {
            return Err(LaunchError::Embedded("the window has no device".to_owned()));
        };
        let words = spec
            .game_words()
            .into_iter()
            .map(|word| word.to_string_lossy().into_owned())
            .chain(spec.extra.iter().cloned())
            .chain(spec.map.clone());
        let args =
            Args::parse_words(words).map_err(|error| LaunchError::Embedded(error.to_string()))?;
        // The bridge, as over the host's socket: one pipe each way.
        let (game_reads, client_writes) = std::io::pipe().map_err(LaunchError::Spawn)?;
        let (client_reads, game_writes) = std::io::pipe().map_err(LaunchError::Spawn)?;
        let game = GameView::open(
            &args,
            &state,
            Some((Box::new(game_reads), Box::new(game_writes))),
        )
        .map_err(|error| LaunchError::Embedded(error.to_string()))?;
        *self.0.game.borrow_mut() = Some(game);
        let slot = self.0.clone();
        let mut client =
            Client::over(client_reads, client_writes).pumped(Box::new(move || slot.serve()));
        match client.initialize(client_name) {
            Ok(_) => Ok(client),
            Err(error) => {
                self.0.stop();
                Err(LaunchError::Bridge(error))
            }
        }
    }

    fn stop(&mut self) {
        self.0.stop();
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use std::path::PathBuf;

    use qs_engine::{LaunchError, LaunchSpec, RunRoot};
    use qs_layout::GamePlace;

    use super::GameSlot;

    // No spec statement: ENG-10, ADR-0028: the window shows the game only when the layout puts
    // it in the Game tab and the window has a device to draw on; without one a start is
    // refused, and the preview host's own window is used
    #[test]
    fn the_window_shows_the_game_only_in_the_tab_and_on_a_device() {
        let slot = GameSlot::default();
        let mut host = slot.host();
        assert!(!host.shows(), "the preview host's window at first");
        slot.set_place(GamePlace::Tab);
        assert!(!host.shows(), "no device to draw on");
        let spec = LaunchSpec {
            host: PathBuf::new(),
            run_root: RunRoot {
                path: PathBuf::from("/nowhere"),
                game: None,
                kpf: None,
            },
            map: None,
            socket: PathBuf::new(),
            extra: Vec::new(),
        };
        let error = host.start(&spec, "tests").err().unwrap();
        assert!(
            matches!(&error, LaunchError::Embedded(why) if why.contains("no device")),
            "{error}"
        );
        // Nothing runs, so there is nothing to draw and nothing to stop.
        host.stop();
        slot.set_place(GamePlace::Window);
        assert!(!host.shows());
    }
}
