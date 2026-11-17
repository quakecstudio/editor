// SPDX-License-Identifier: GPL-3.0-or-later

//! The default keys (UI-21, MAP-20, P7-17): a shortcut for every command a person runs from
//! the window with no value to give, TrenchBroom's where it binds the same action, and for
//! every other command why it has none. The window's keymap, `docs/keymap.md` and the test
//! that no command is left out all read these two tables, so they cannot disagree.
//!
//! TrenchBroom's actions are named as `lib/TbUiLib/src/ActionManager.cpp` names them at
//! TrenchBroom/TrenchBroom@8f329f11e65e3d21b355a42bcae199cb3e4e0812; its `Ctrl` is the
//! platform's modifier, `secondary` here (Cmd on macOS).

/// Where a default key is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyContext {
    /// Wherever the keyboard is.
    Window,
    /// While the code editor does not have the keyboard: it takes these keys itself, with
    /// its own undo history.
    Shell,
    /// While the map view has the keyboard.
    Map,
}

/// What a default key follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Follows {
    /// TrenchBroom's default for the action of this name.
    TrenchBroom(&'static str),
    /// Our own choice, and why.
    Own(&'static str),
}

/// A command's default key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefaultKey {
    /// The command's id.
    pub command: &'static str,
    /// The keys in Zed's spelling: modifiers and one key joined by `-`, `secondary` the
    /// platform's modifier.
    pub keys: &'static str,
    /// Where the keys are taken.
    pub context: KeyContext,
    /// What the key follows.
    pub follows: Follows,
}

/// Why a command has no key of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unkeyed {
    /// It answers a question: a panel shows its answer, and the agent and MCP ask it.
    Query,
    /// The window runs it itself as it draws: a poll, or what it takes from another part.
    Window,
    /// It needs a value a key cannot give, a path, a name, a place: it runs from the command
    /// line, which asks for each, or from the control in the window that has the value.
    Typed,
}

/// Every default key, a command once.
pub const DEFAULT_KEYS: [DefaultKey; 49] = [
    DefaultKey {
        command: "edit.undo",
        keys: "secondary-z",
        context: KeyContext::Shell,
        follows: Follows::TrenchBroom("Menu/Edit/Undo"),
    },
    DefaultKey {
        command: "edit.redo",
        keys: "secondary-shift-z",
        context: KeyContext::Shell,
        follows: Follows::TrenchBroom("Menu/Edit/Redo"),
    },
    DefaultKey {
        command: "edit.repeat",
        keys: "secondary-r",
        context: KeyContext::Shell,
        follows: Follows::TrenchBroom("Menu/Edit/Repeat"),
    },
    DefaultKey {
        command: "map.run_profile",
        keys: "secondary-b",
        context: KeyContext::Shell,
        follows: Follows::Own(
            "the build key of Xcode and Sublime Text; TrenchBroom's Compile Map has no key",
        ),
    },
    DefaultKey {
        command: "view.focus_editor",
        keys: "secondary-1",
        context: KeyContext::Window,
        follows: Follows::Own("VS Code's focus on the first editor group"),
    },
    DefaultKey {
        command: "view.focus_left_panel",
        keys: "secondary-2",
        context: KeyContext::Window,
        follows: Follows::Own("after the editor's, in the window's order"),
    },
    DefaultKey {
        command: "view.focus_right_panel",
        keys: "secondary-3",
        context: KeyContext::Window,
        follows: Follows::Own("after the editor's, in the window's order"),
    },
    DefaultKey {
        command: "view.focus_bottom_panel",
        keys: "secondary-4",
        context: KeyContext::Window,
        follows: Follows::Own("after the editor's, in the window's order"),
    },
    DefaultKey {
        command: "view.focus_command_line",
        keys: "secondary-;",
        context: KeyContext::Window,
        follows: Follows::Own("beside Vim's `:`, which the editor types"),
    },
    DefaultKey {
        command: "editor.fold",
        keys: "secondary-alt-[",
        context: KeyContext::Window,
        follows: Follows::Own("Zed's editor::Fold"),
    },
    DefaultKey {
        command: "editor.unfold",
        keys: "secondary-alt-]",
        context: KeyContext::Window,
        follows: Follows::Own("Zed's editor::UnfoldLines"),
    },
    DefaultKey {
        command: "editor.fold_all",
        keys: "secondary-alt-shift-[",
        context: KeyContext::Window,
        follows: Follows::Own("the fold key with Shift, for every block"),
    },
    DefaultKey {
        command: "editor.unfold_all",
        keys: "secondary-alt-shift-]",
        context: KeyContext::Window,
        follows: Follows::Own("the unfold key with Shift, for every block"),
    },
    DefaultKey {
        command: "editor.go_to_line",
        keys: "ctrl-g",
        context: KeyContext::Window,
        follows: Follows::Own("Zed's go_to_line::Toggle"),
    },
    DefaultKey {
        command: "editor.go_to_symbol",
        keys: "secondary-shift-o",
        context: KeyContext::Window,
        follows: Follows::Own("Zed's outline::Toggle"),
    },
    DefaultKey {
        command: "editor.toggle_soft_wrap",
        keys: "ctrl-alt-w",
        context: KeyContext::Window,
        follows: Follows::Own("Zed has no key; VS Code's Alt+Z types a character on macOS"),
    },
    DefaultKey {
        command: "view.project_search",
        keys: "secondary-shift-f",
        context: KeyContext::Window,
        follows: Follows::Own("Zed's and VS Code's search in the project"),
    },
    DefaultKey {
        command: "view.tab_switcher",
        keys: "ctrl-tab",
        context: KeyContext::Window,
        follows: Follows::Own("Zed's tab_switcher::Toggle"),
    },
    DefaultKey {
        command: "view.toggle_outline",
        keys: "secondary-shift-b",
        context: KeyContext::Window,
        follows: Follows::Own("Zed's outline_panel::ToggleFocus"),
    },
    DefaultKey {
        command: "view.split_right",
        keys: "secondary-\\",
        context: KeyContext::Window,
        follows: Follows::Own("VS Code's split editor"),
    },
    DefaultKey {
        command: "view.split_down",
        keys: "secondary-alt-\\",
        context: KeyContext::Window,
        follows: Follows::Own("the split key with Alt"),
    },
    DefaultKey {
        command: "view.close_split",
        keys: "secondary-shift-\\",
        context: KeyContext::Window,
        follows: Follows::Own("the split key with Shift"),
    },
    DefaultKey {
        command: "view.toggle_embedded_game",
        keys: "secondary-alt-g",
        context: KeyContext::Window,
        follows: Follows::Own("G for the game"),
    },
    DefaultKey {
        command: "preview.play",
        keys: "f5",
        context: KeyContext::Window,
        follows: Follows::Own("VS Code's Start Debugging"),
    },
    DefaultKey {
        command: "preview.stop",
        keys: "shift-f5",
        context: KeyContext::Window,
        follows: Follows::Own("VS Code's Stop"),
    },
    DefaultKey {
        command: "preview.restart",
        keys: "secondary-shift-f5",
        context: KeyContext::Window,
        follows: Follows::Own("VS Code's Restart"),
    },
    DefaultKey {
        command: "preview.pause",
        keys: "f6",
        context: KeyContext::Window,
        follows: Follows::Own("VS Code's Pause"),
    },
    DefaultKey {
        command: "preview.resume",
        keys: "f8",
        context: KeyContext::Window,
        follows: Follows::Own("the resume key of browsers' debuggers; F5 plays"),
    },
    DefaultKey {
        command: "preview.step",
        keys: "f10",
        context: KeyContext::Window,
        follows: Follows::Own("VS Code's Step Over"),
    },
    DefaultKey {
        command: "preview.reload",
        keys: "secondary-shift-r",
        context: KeyContext::Window,
        follows: Follows::Own("a browser's reload"),
    },
    DefaultKey {
        command: "project.build",
        keys: "f7",
        context: KeyContext::Window,
        follows: Follows::Own("Visual Studio's Build"),
    },
    DefaultKey {
        command: "project.check",
        keys: "shift-f7",
        context: KeyContext::Window,
        follows: Follows::Own("the build key with Shift"),
    },
    DefaultKey {
        command: "compile",
        keys: "secondary-f7",
        context: KeyContext::Window,
        follows: Follows::Own("the build key with the platform's modifier"),
    },
    DefaultKey {
        command: "project.test",
        keys: "secondary-shift-t",
        context: KeyContext::Window,
        follows: Follows::Own("T for test"),
    },
    DefaultKey {
        command: "project.export",
        keys: "secondary-alt-e",
        context: KeyContext::Window,
        follows: Follows::Own("E for export"),
    },
    DefaultKey {
        command: "checkpoint.create",
        keys: "secondary-alt-s",
        context: KeyContext::Window,
        follows: Follows::Own("a save beside the file's"),
    },
    DefaultKey {
        command: "terminal.new",
        keys: "ctrl-shift-`",
        context: KeyContext::Window,
        follows: Follows::Own("VS Code's Create New Terminal"),
    },
    DefaultKey {
        command: "map.save",
        keys: "secondary-s",
        context: KeyContext::Map,
        follows: Follows::TrenchBroom("Menu/File/Save"),
    },
    DefaultKey {
        command: "map.copy",
        keys: "secondary-c",
        context: KeyContext::Map,
        follows: Follows::TrenchBroom("Menu/Edit/Copy"),
    },
    DefaultKey {
        command: "map.select",
        keys: "secondary-shift-a",
        context: KeyContext::Map,
        follows: Follows::TrenchBroom("Menu/Edit/Deselect All"),
    },
    DefaultKey {
        command: "map.show_all",
        keys: "secondary-shift-i",
        context: KeyContext::Map,
        follows: Follows::TrenchBroom("Menu/View/Show All"),
    },
    DefaultKey {
        command: "map.unlock_all",
        keys: "secondary-shift-l",
        context: KeyContext::Map,
        follows: Follows::Own("TrenchBroom has no key; L for lock, as Show All is I"),
    },
    DefaultKey {
        command: "map.leaks",
        keys: "secondary-shift-k",
        context: KeyContext::Map,
        follows: Follows::Own("TrenchBroom loads a point file from a menu with no key"),
    },
    DefaultKey {
        command: "map.accept",
        keys: "secondary-alt-y",
        context: KeyContext::Map,
        follows: Follows::Own("Y for yes, to every change in review"),
    },
    DefaultKey {
        command: "map.reject",
        keys: "secondary-alt-n",
        context: KeyContext::Map,
        follows: Follows::Own("N for no, to every change in review"),
    },
    DefaultKey {
        command: "map.stage",
        keys: "secondary-alt-r",
        context: KeyContext::Map,
        follows: Follows::Own("R for review"),
    },
    DefaultKey {
        command: "map.unstage",
        keys: "secondary-alt-shift-r",
        context: KeyContext::Map,
        follows: Follows::Own("the review key with Shift"),
    },
    DefaultKey {
        command: "map.backup",
        keys: "secondary-alt-b",
        context: KeyContext::Map,
        follows: Follows::Own("B for backup"),
    },
    DefaultKey {
        command: "map.choose_profile",
        keys: "secondary-alt-m",
        context: KeyContext::Map,
        follows: Follows::Own("back to the Full profile"),
    },
];

/// Every command with no key of its own, and why.
pub const UNKEYED: [(&str, Unkeyed); 159] = [
    ("checkpoint.list", Unkeyed::Query),
    ("checkpoint.restore", Unkeyed::Typed),
    ("code.actions", Unkeyed::Typed),
    ("code.fix", Unkeyed::Typed),
    ("commands.list", Unkeyed::Query),
    ("edit.accept", Unkeyed::Typed),
    ("edit.apply", Unkeyed::Typed),
    ("edit.reject", Unkeyed::Typed),
    ("edit.staged", Unkeyed::Query),
    ("engine.builtins", Unkeyed::Query),
    ("engine.console", Unkeyed::Typed),
    ("engine.inspect", Unkeyed::Typed),
    ("engine.screenshot", Unkeyed::Typed),
    ("file.close", Unkeyed::Typed),
    ("file.format", Unkeyed::Typed),
    ("file.inlay_hints", Unkeyed::Typed),
    ("file.open", Unkeyed::Typed),
    ("file.read", Unkeyed::Typed),
    ("file.replace_all", Unkeyed::Typed),
    ("file.replace_range", Unkeyed::Typed),
    ("file.save", Unkeyed::Typed),
    ("file.update", Unkeyed::Typed),
    ("files.changed", Unkeyed::Typed),
    ("git.blame", Unkeyed::Typed),
    ("git.branches", Unkeyed::Query),
    ("git.commit", Unkeyed::Typed),
    ("git.conflicts", Unkeyed::Typed),
    ("git.create_branch", Unkeyed::Typed),
    ("git.diff", Unkeyed::Typed),
    ("git.head_text", Unkeyed::Typed),
    ("git.log", Unkeyed::Typed),
    ("git.resolve", Unkeyed::Typed),
    ("git.stage", Unkeyed::Typed),
    ("git.status", Unkeyed::Query),
    ("git.switch", Unkeyed::Typed),
    ("git.unstage", Unkeyed::Typed),
    ("map.add_layer", Unkeyed::Typed),
    ("map.add_wad", Unkeyed::Typed),
    ("map.backups", Unkeyed::Query),
    ("map.brush_chamfer", Unkeyed::Typed),
    ("map.brush_clip", Unkeyed::Typed),
    ("map.brush_create", Unkeyed::Typed),
    ("map.brush_delete", Unkeyed::Typed),
    ("map.brush_extrude", Unkeyed::Typed),
    ("map.brush_hollow", Unkeyed::Typed),
    ("map.brush_hull", Unkeyed::Typed),
    ("map.brush_intersect", Unkeyed::Typed),
    ("map.brush_merge", Unkeyed::Typed),
    ("map.brush_move", Unkeyed::Typed),
    ("map.brush_scale", Unkeyed::Typed),
    ("map.brush_shape", Unkeyed::Typed),
    ("map.brush_shear", Unkeyed::Typed),
    ("map.brush_split_edge", Unkeyed::Typed),
    ("map.brush_split_face", Unkeyed::Typed),
    ("map.brush_subtract", Unkeyed::Typed),
    ("map.brush_sweep", Unkeyed::Typed),
    ("map.brush_texture", Unkeyed::Typed),
    ("map.brush_vertex", Unkeyed::Typed),
    ("map.brush_vertices", Unkeyed::Typed),
    ("map.check", Unkeyed::Typed),
    ("map.compare", Unkeyed::Query),
    ("map.compile", Unkeyed::Typed),
    ("map.delete", Unkeyed::Typed),
    ("map.duplicate", Unkeyed::Typed),
    ("map.entity", Unkeyed::Typed),
    ("map.export", Unkeyed::Typed),
    ("map.extract_linked", Unkeyed::Typed),
    ("map.face_align", Unkeyed::Typed),
    ("map.face_uv", Unkeyed::Typed),
    ("map.filter", Unkeyed::Query),
    ("map.fix", Unkeyed::Typed),
    ("map.flip", Unkeyed::Typed),
    ("map.group", Unkeyed::Typed),
    ("map.hide", Unkeyed::Typed),
    ("map.inspect", Unkeyed::Query),
    ("map.isolate", Unkeyed::Typed),
    ("map.issues", Unkeyed::Query),
    ("map.layers", Unkeyed::Query),
    ("map.link", Unkeyed::Typed),
    ("map.link_duplicate", Unkeyed::Typed),
    ("map.lock", Unkeyed::Typed),
    ("map.make_brush_entity", Unkeyed::Typed),
    ("map.make_structural", Unkeyed::Typed),
    ("map.move", Unkeyed::Typed),
    ("map.move_to", Unkeyed::Typed),
    ("map.open", Unkeyed::Typed),
    ("map.outline", Unkeyed::Query),
    ("map.paste", Unkeyed::Typed),
    ("map.place", Unkeyed::Typed),
    ("map.place_prefab", Unkeyed::Typed),
    ("map.play_from_here", Unkeyed::Typed),
    ("map.portals", Unkeyed::Query),
    ("map.prefabs", Unkeyed::Query),
    ("map.profiles", Unkeyed::Query),
    ("map.remove_key", Unkeyed::Typed),
    ("map.rename_key", Unkeyed::Typed),
    ("map.replace_texture", Unkeyed::Typed),
    ("map.resolve", Unkeyed::Typed),
    ("map.restore_backup", Unkeyed::Typed),
    ("map.rotate", Unkeyed::Typed),
    ("map.save_prefab", Unkeyed::Typed),
    ("map.search", Unkeyed::Typed),
    ("map.separate", Unkeyed::Typed),
    ("map.set_flag", Unkeyed::Typed),
    ("map.set_key", Unkeyed::Typed),
    ("map.set_layer", Unkeyed::Typed),
    ("map.staged", Unkeyed::Query),
    ("map.textures", Unkeyed::Query),
    ("map.turn", Unkeyed::Typed),
    ("map.ungroup", Unkeyed::Typed),
    ("paths.open", Unkeyed::Typed),
    ("paths.take_opened", Unkeyed::Window),
    ("preview.breakpoint", Unkeyed::Typed),
    ("preview.choose_game", Unkeyed::Typed),
    ("preview.console", Unkeyed::Typed),
    ("preview.dock", Unkeyed::Typed),
    ("preview.game", Unkeyed::Query),
    ("preview.pick", Unkeyed::Typed),
    ("preview.poll", Unkeyed::Window),
    ("preview.select", Unkeyed::Typed),
    ("preview.set_field", Unkeyed::Typed),
    ("preview.step_by", Unkeyed::Typed),
    ("preview.unwatch", Unkeyed::Typed),
    ("preview.watch", Unkeyed::Typed),
    ("project.entities", Unkeyed::Query),
    ("project.files", Unkeyed::Query),
    ("project.info", Unkeyed::Query),
    ("project.new", Unkeyed::Typed),
    ("project.open", Unkeyed::Typed),
    ("project.search", Unkeyed::Typed),
    ("project.symbols", Unkeyed::Query),
    ("project.take_checked", Unkeyed::Window),
    ("scenario.run", Unkeyed::Typed),
    ("settings.get", Unkeyed::Typed),
    ("settings.list", Unkeyed::Query),
    ("settings.reset", Unkeyed::Typed),
    ("settings.set", Unkeyed::Typed),
    ("symbol.complete", Unkeyed::Typed),
    ("symbol.definition", Unkeyed::Typed),
    ("symbol.hover", Unkeyed::Typed),
    ("symbol.references", Unkeyed::Typed),
    ("symbol.rename", Unkeyed::Typed),
    ("task.list", Unkeyed::Query),
    ("task.run", Unkeyed::Typed),
    ("terminal.close", Unkeyed::Typed),
    ("terminal.poll", Unkeyed::Window),
    ("terminal.send", Unkeyed::Typed),
    ("texture.import", Unkeyed::Typed),
    ("view.map_camera", Unkeyed::Query),
    ("view.map_grid", Unkeyed::Typed),
    ("view.map_grid_step", Unkeyed::Typed),
    ("view.map_panes", Unkeyed::Typed),
    ("view.map_shading", Unkeyed::Typed),
    ("view.move_panel", Unkeyed::Typed),
    ("view.resize_panel", Unkeyed::Typed),
    ("view.show_panel", Unkeyed::Typed),
    ("view.toggle_panel", Unkeyed::Typed),
    ("view.zoom", Unkeyed::Typed),
    ("vm.run", Unkeyed::Typed),
];

impl DefaultKey {
    /// The default key of the command `id`, if it has one.
    #[must_use]
    pub fn of(id: &str) -> Option<&'static Self> {
        DEFAULT_KEYS.iter().find(|key| key.command == id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{DEFAULT_KEYS, DefaultKey, Follows, UNKEYED};

    // No spec statement: UI-21, #971: no two default keys are the same, since a key held in one
    // context is held in the window's too; and no command is in both tables or twice in one
    #[test]
    fn no_two_default_keys_collide_and_no_command_is_listed_twice() {
        let keys: BTreeSet<&str> = DEFAULT_KEYS.iter().map(|key| key.keys).collect();
        assert_eq!(keys.len(), DEFAULT_KEYS.len(), "two commands share keys");
        let mut commands: Vec<&str> = DEFAULT_KEYS
            .iter()
            .map(|key| key.command)
            .chain(UNKEYED.iter().map(|(command, _)| *command))
            .collect();
        let listed = commands.len();
        commands.sort_unstable();
        commands.dedup();
        assert_eq!(commands.len(), listed, "a command is listed twice");
    }

    // No spec statement: MAP-20, #971: a key that follows TrenchBroom names the action, and a
    // key of our own says why; a command's key is found by its id
    #[test]
    fn every_default_key_says_what_it_follows() {
        for key in &DEFAULT_KEYS {
            let said = match key.follows {
                Follows::TrenchBroom(action) => action.starts_with("Menu/"),
                Follows::Own(why) => !why.is_empty(),
            };
            assert!(said, "{}", key.command);
        }
        assert_eq!(
            DefaultKey::of("edit.repeat").map(|key| key.keys),
            Some("secondary-r")
        );
        assert_eq!(DefaultKey::of("file.open"), None);
    }
}
