// SPDX-License-Identifier: GPL-3.0-or-later

//! What a Play plays (L-86, ADR-0031): single player, coop or deathmatch, the players on this
//! machine and the bots, as `preview.play` and `map.play_from_here` take them and the preview
//! host's command line gives them. The model is Roblox Studio's local test, which starts a
//! server with a number of players and a window for each.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::engine::BotSkillParam;

pub use qs_sim::{MAX_LOCAL_PLAYERS, MAX_SCOREBOARD};

/// Which game a Play starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GameMode {
    /// One player and no bots, as the game has always played.
    #[default]
    Single,
    /// Coop: `coop` 1, the players on the same side against the map's monsters.
    Coop,
    /// Deathmatch: `deathmatch` 1, every player against every other.
    Deathmatch,
}

impl GameMode {
    /// Every game, single player first.
    pub const ALL: [Self; 3] = [Self::Single, Self::Coop, Self::Deathmatch];

    /// Whether it has room for other players and bots.
    #[must_use]
    pub const fn is_multiplayer(self) -> bool {
        match self {
            Self::Single => false,
            Self::Coop | Self::Deathmatch => true,
        }
    }
}

/// What a Play plays: the game, the players on this machine, the bots and how well they play.
/// The default is single player with no bots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GameOptions {
    /// Single player, coop or deathmatch; single player when absent.
    #[serde(default)]
    pub mode: GameMode,
    /// The players on this machine, 1 to 4; each after the first plays in a window of its own.
    /// One when absent.
    #[serde(default = "one_player")]
    pub players: u8,
    /// The bots brought in as the game starts, which play any mod through its own QuakeC
    /// (ADR-0031); none when absent. Players and bots are 16 at most.
    #[serde(default)]
    pub bots: u8,
    /// How well the bots play; the game's `skill` setting when absent.
    #[serde(default)]
    pub bot_skill: Option<BotSkillParam>,
}

const fn one_player() -> u8 {
    1
}

impl Default for GameOptions {
    fn default() -> Self {
        Self {
            mode: GameMode::Single,
            players: one_player(),
            bots: 0,
            bot_skill: None,
        }
    }
}

/// Why a Play's options do not make a game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameOptionsError {
    /// Single player has one player and no bots.
    SinglePlayerAlone,
    /// The players on this machine are 1 to [`MAX_LOCAL_PLAYERS`].
    Players(u8),
    /// Players and bots together are [`MAX_SCOREBOARD`] at most.
    Clients(usize),
}

impl core::fmt::Display for GameOptionsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::SinglePlayerAlone => f.write_str(
                "single player has one player and no bots: choose coop or deathmatch to play \
                 with others",
            ),
            Self::Players(n) => write!(
                f,
                "{n} players on this machine: a game has 1 to {MAX_LOCAL_PLAYERS}"
            ),
            Self::Clients(n) => write!(
                f,
                "{n} players and bots: a game has {MAX_SCOREBOARD} at most"
            ),
        }
    }
}

impl std::error::Error for GameOptionsError {}

impl GameOptions {
    /// The preview host's command line for these options: none for single player; for coop
    /// and deathmatch `-listen` with room for every player and bot and at least 8 slots, then
    /// `--coop`, `--players`, `--bots` and `--bot-skill` as they apply.
    ///
    /// # Errors
    ///
    /// [`GameOptionsError`] when the options do not make a game.
    pub fn host_words(self) -> Result<Vec<String>, GameOptionsError> {
        if !(1..=MAX_LOCAL_PLAYERS).contains(&self.players) {
            return Err(GameOptionsError::Players(self.players));
        }
        let clients = usize::from(self.players).saturating_add(usize::from(self.bots));
        if clients > MAX_SCOREBOARD {
            return Err(GameOptionsError::Clients(clients));
        }
        if !self.mode.is_multiplayer() {
            return if clients > 1 {
                Err(GameOptionsError::SinglePlayerAlone)
            } else {
                Ok(Vec::new())
            };
        }
        let slots = i32::try_from(clients)
            .unwrap_or(qs_sim::LISTEN_SLOTS)
            .max(qs_sim::LISTEN_SLOTS);
        let mut words = vec!["-listen".to_owned(), slots.to_string()];
        if self.mode == GameMode::Coop {
            words.extend(["--rules".to_owned(), "coop".to_owned()]);
        }
        if self.players > 1 {
            words.extend(["--players".to_owned(), self.players.to_string()]);
        }
        if self.bots > 0 {
            words.extend(["--bots".to_owned(), self.bots.to_string()]);
            if let Some(skill) = self.bot_skill {
                words.extend(["--bot-skill".to_owned(), skill.level().to_string()]);
            }
        }
        Ok(words)
    }
}

#[cfg(test)]
mod tests;
