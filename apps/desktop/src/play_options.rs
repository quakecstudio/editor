// SPDX-License-Identifier: GPL-3.0-or-later

//! The toolbar's play options beside Play (L-86, `docs/design/mockups/07-play-options.png`):
//! single player, coop or deathmatch, the players on this computer and the bots, which
//! `preview.play` and `map.play_from_here` are given. Each choice is picked from a list
//! (`docs/design/usability.md`), and the button names the game it plays.

use eframe::egui::{self, RichText, Ui};
use qs_workbench::engine::BotSkillParam;
use qs_workbench::{GameMode, GameOptions, MAX_LOCAL_PLAYERS, MAX_SCOREBOARD};

use crate::strings::{Text, game_mode, play_plan, play_summary};
use crate::theme::{Theme, Token};

/// The space between the options' names and their choices, a step of the theme's scale.
const COLUMN_GAP: usize = 3;
/// The space between two options, a step of the theme's scale.
const ROW_GAP: usize = 2;

/// The bot skills the list offers, after the game's own.
const SKILLS: [(BotSkillParam, Text); 4] = [
    (BotSkillParam::Easy, Text::MapSkillEasy),
    (BotSkillParam::Normal, Text::MapSkillNormal),
    (BotSkillParam::Hard, Text::MapSkillHard),
    (BotSkillParam::Nightmare, Text::MapSkillNightmare),
];

/// The play options' button and, when it is open, the options; whether one was changed.
pub(crate) fn button(ui: &mut Ui, theme: &Theme, options: &mut GameOptions) -> bool {
    let before = *options;
    // The options stay open while they are chosen; a click outside closes them.
    let config = egui::containers::menu::MenuConfig::new()
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let (response, _) =
        egui::containers::menu::MenuButton::new(RichText::new(play_summary(*options)).small())
            .config(config)
            .ui(ui, |ui| choices(ui, theme, options));
    response
        .on_hover_text(Text::PlayOptionsHint.get())
        .accessible_name(Text::PlayOptions.get());
    *options != before
}

/// The options themselves: the game, then for coop and deathmatch the players and the bots,
/// and what Play will start.
fn choices(ui: &mut Ui, theme: &Theme, options: &mut GameOptions) {
    let muted = |text: &str| {
        RichText::new(text)
            .small()
            .color(theme.color(Token::TextMuted))
    };
    ui.strong(Text::PlayOptions.get());
    egui::Grid::new(Text::PlayOptions)
        .num_columns(2)
        .spacing([theme.space(COLUMN_GAP), theme.space(ROW_GAP)])
        .show(ui, |ui| {
            ui.label(Text::Game.get());
            ui.horizontal(|ui| {
                for mode in GameMode::ALL {
                    ui.selectable_value(&mut options.mode, mode, game_mode(mode).get());
                }
            });
            ui.end_row();
            if options.mode.is_multiplayer() {
                ui.label(Text::LocalPlayers.get());
                let players = egui::ComboBox::from_id_salt(Text::LocalPlayers)
                    .selected_text(options.players.to_string())
                    .show_ui(ui, |ui| {
                        for n in 1..=MAX_LOCAL_PLAYERS {
                            ui.selectable_value(&mut options.players, n, n.to_string());
                        }
                    });
                players.response.accessible_name(Text::LocalPlayers.get());
                ui.end_row();
                ui.allocate_space(egui::Vec2::ZERO);
                ui.label(muted(Text::LocalPlayersNote.get()));
                ui.end_row();
                ui.label(Text::Bots.get());
                let room = u8::try_from(MAX_SCOREBOARD)
                    .unwrap_or(u8::MAX)
                    .saturating_sub(options.players);
                options.bots = options.bots.min(room);
                let bots = egui::ComboBox::from_id_salt(Text::Bots)
                    .selected_text(bot_name(options.bots))
                    .show_ui(ui, |ui| {
                        for n in 0..=room {
                            ui.selectable_value(&mut options.bots, n, bot_name(n));
                        }
                    });
                bots.response.accessible_name(Text::Bots.get());
                ui.end_row();
                ui.allocate_space(egui::Vec2::ZERO);
                ui.label(muted(Text::BotsNote.get()));
                ui.end_row();
                if options.bots > 0 {
                    ui.label(Text::BotSkill.get());
                    let shown = SKILLS
                        .iter()
                        .find(|(skill, _)| Some(*skill) == options.bot_skill)
                        .map_or(Text::BotSkillGame, |(_, text)| *text);
                    let skill = egui::ComboBox::from_id_salt(Text::BotSkill)
                        .selected_text(shown.get())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut options.bot_skill,
                                None,
                                Text::BotSkillGame.get(),
                            );
                            for (value, text) in SKILLS {
                                ui.selectable_value(
                                    &mut options.bot_skill,
                                    Some(value),
                                    text.get(),
                                );
                            }
                        });
                    skill.response.accessible_name(Text::BotSkill.get());
                    ui.end_row();
                }
            }
        });
    // Single player is one player alone: the counts go back to it.
    if !options.mode.is_multiplayer() {
        options.players = 1;
        options.bots = 0;
    }
    ui.separator();
    ui.label(muted(&play_plan(*options)));
}

/// A count of bots as the list shows it: "None", else the number.
fn bot_name(bots: u8) -> String {
    if bots == 0 {
        Text::BotsNone.get().to_owned()
    } else {
        bots.to_string()
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use qs_game::Args;
    use qs_workbench::engine::BotSkillParam;
    use qs_workbench::{GameMode, GameOptions};

    /// The preview host's command line for `options`, read as the host reads its own.
    fn host(options: GameOptions) -> Args {
        Args::parse_words(options.host_words().unwrap()).unwrap()
    }

    fn setting(args: &Args, name: &str) -> Option<String> {
        args.new_game_settings(args.maxplayers())
            .into_iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, value)| value)
    }

    // spec NET-7.1: the default options give single player: one slot, no setting, no bots
    #[test]
    fn the_default_plays_single_player_with_no_bots() {
        let args = host(GameOptions::default());
        assert_eq!(args.maxplayers().get(), 1);
        assert_eq!(args.players, 1);
        assert!(args.new_game_settings(args.maxplayers()).is_empty());
        assert!(args.bot_lines().is_empty());
    }

    // spec NET-7.1: deathmatch reaches the world as deathmatch 1 with room for every client
    #[test]
    fn deathmatch_reaches_the_world() {
        let args = host(GameOptions {
            mode: GameMode::Deathmatch,
            ..GameOptions::default()
        });
        assert_eq!(setting(&args, "deathmatch").as_deref(), Some("1"));
        assert_eq!(setting(&args, "coop"), None);
        assert!(args.maxplayers().is_multiplayer());
    }

    // spec DIFF-3.2: coop reaches the world as coop 1 and deathmatch 0
    #[test]
    fn coop_reaches_the_world() {
        let args = host(GameOptions {
            mode: GameMode::Coop,
            ..GameOptions::default()
        });
        assert_eq!(setting(&args, "coop").as_deref(), Some("1"));
        assert_eq!(setting(&args, "deathmatch").as_deref(), Some("0"));
    }

    // spec NET-7.4: the local players reach the host, with a slot each
    #[test]
    fn the_local_players_reach_the_host() {
        let args = host(GameOptions {
            mode: GameMode::Coop,
            players: 3,
            ..GameOptions::default()
        });
        assert_eq!(args.players, 3);
        assert!(args.check_players(args.maxplayers()).is_ok());
    }

    // spec BOT-9.3, BOT-1.4: the bots and their skill reach the world's console as addbot
    #[test]
    fn the_bots_and_their_skill_reach_the_world() {
        let args = host(GameOptions {
            mode: GameMode::Deathmatch,
            players: 2,
            bots: 14,
            bot_skill: Some(BotSkillParam::Nightmare),
        });
        assert_eq!(args.bot_lines(), vec!["addbot 3\n"; 14]);
        assert_eq!(args.maxplayers().get(), 16, "every client a slot");
    }
}
