// SPDX-License-Identifier: GPL-3.0-or-later

//! The map's feature (P6-41): the map view, the map editor and what it last showed, the open
//! map's backups and disk watch, and the Entities outliner and Properties panel, which the
//! shell used to hold.

use std::path::{Path, PathBuf};

use eframe::egui::{self};
use qs_layout::{
    MAP_CAMERA, MAP_GRID, MAP_PANES, MAP_SHADING, MapGridParams, MapPanesParams, MapShadingParams,
};
use qs_map_edit::EntityIndex;
use qs_map_edit::LAYER_KEY;
use qs_map_edit::backup::{Backup, MAP_BACKUP, MAP_BACKUPS, MAP_RESTORE_BACKUP, RestoreParams};
use qs_map_edit::texture_import::{AddWadParams, MAP_ADD_WAD};
use qs_map_edit::texture_import::{TEXTURE_IMPORT, TextureImportParams};
use qs_map_edit::textures::TextureSources;
use qs_map_edit::{
    AddLayerParams, ContainerParams, ExtractLinkedParams, GroupParams, MAP_ADD_LAYER,
    MAP_EXTRACT_LINKED, MAP_GROUP, MAP_LINK_DUPLICATE, MAP_MAKE_BRUSH_ENTITY, MAP_MAKE_STRUCTURAL,
    MAP_SEPARATE, MAP_UNGROUP, MakeBrushEntityParams, MakeStructuralParams,
};
use qs_map_edit::{
    BrushChamferParams, BrushClipParams, BrushCreateParams, BrushExtrudeParams, BrushHollowParams,
    BrushIntersectParams, BrushMergeParams, BrushMoveParams, BrushParams, BrushScaleParams,
    BrushShapeParams, BrushShearParams, BrushSplitEdgeParams, BrushSplitFaceParams,
    BrushSweepParams, BrushTextureParams, BrushVertexParams, BrushVerticesParams,
    MAP_BRUSH_CHAMFER, MAP_BRUSH_CLIP, MAP_BRUSH_CREATE, MAP_BRUSH_DELETE, MAP_BRUSH_EXTRUDE,
    MAP_BRUSH_HOLLOW, MAP_BRUSH_INTERSECT, MAP_BRUSH_MERGE, MAP_BRUSH_MOVE, MAP_BRUSH_SCALE,
    MAP_BRUSH_SHAPE, MAP_BRUSH_SHEAR, MAP_BRUSH_SPLIT_EDGE, MAP_BRUSH_SPLIT_FACE,
    MAP_BRUSH_SUBTRACT, MAP_BRUSH_SWEEP, MAP_BRUSH_TEXTURE, MAP_BRUSH_VERTEX, MAP_BRUSH_VERTICES,
    NewVertex, VertexMove,
};
use qs_map_edit::{BrushHullParams, MAP_BRUSH_HULL};
use qs_map_edit::{
    BspSave, EntityKeys, EntityParams, MAP_COMPARE, MAP_DELETE, MAP_DUPLICATE, MAP_ENTITY,
    MAP_MOVE, MAP_OPEN, MAP_REMOVE_KEY, MAP_RESOLVE, MAP_SAVE, MAP_SELECT, MAP_SET_KEY,
    MapDifference, MapDocument, MapOnDisk, MapOpenParams, MapSaved, MoveParams, RemoveKeyParams,
    ResolveParams, SaveParams, SelectParams as MapSelectParams, SetKeyParams,
};
use qs_map_edit::{
    Copied, CopyParams, MAP_COPY, MAP_PASTE, MAP_PLACE_PREFAB, MAP_PREFABS, MAP_SAVE_PREFAB,
    PasteParams, PlacePrefabParams, Prefabs, SavePrefabParams,
};
use qs_map_edit::{
    EntityListParams, LayerInfo, MAP_HIDE, MAP_ISOLATE, MAP_LAYERS, MAP_LOCK, MAP_SET_LAYER,
    MAP_SHOW_ALL, MAP_UNLOCK_ALL, SetLayerParams,
};
use qs_map_edit::{
    FlipParams, Lock, MAP_FLIP, MAP_REPLACE_TEXTURE, MAP_TURN, ReplaceTextureParams, Selection,
    TurnParams,
};
use qs_map_edit::{LinkParams, MAP_LINK, MAP_PLACE, MAP_SET_FLAG, PlaceParams, SetFlagParams};
use qs_map_edit::{MAP_ACCEPT, MAP_REJECT, MAP_STAGED, MapHunk, MapHunksParams};
use qs_map_edit::{
    MAP_FIX, MAP_ISSUES, MAP_SEARCH, MapFixParams, MapFound, MapIssues, MapRevision,
};
use qs_map_edit::{MAP_MOVE_TO, MAP_OUTLINE, MoveToParams, OutlineParams, OutlineRow};
use qs_workbench::entity_classes::{EntitiesFound, EntitiesParams, PROJECT_ENTITIES};
use qs_workbench::files_changed::{FILES_CHANGED, FilesChanged, FilesChangedParams};
use qs_workbench::map_compile::{
    MAP_LEAKS, MAP_PLAY_FROM_HERE, MAP_PORTALS, MapPortals, PlayFromHereParams,
};
use qs_workbench::map_filter::{FilterParams, MAP_FILTER, MapFiltered};
use qs_workbench::map_profile::{
    MAP_CHOOSE_PROFILE, MAP_PROFILES, MAP_RUN_PROFILE, Profile, ProfileRun, RunProfileParams,
};

use crate::entities::{Outliner, Properties};
use crate::features::{Commands, ShellRequest};
use crate::map_editor::{
    Arrange, ClipboardAction, DiskNotice, MapAction, MapEditor, MenuAction, ShowFilter, ViewSetting,
};
use crate::map_groups::GroupAction;
use crate::map_profiles::ProfileAction;
use crate::map_review::ReviewAction;
use crate::map_tools::BrushAction;
use crate::map_view::MapView;
use crate::shell::{CentreTab, MAP_BACKUP_INTERVAL, ShellError, Workspace};
use crate::strings::{group_name, layer_name};

use qs_map_edit::uv::{FaceAlignParams, FaceUvParams, MAP_FACE_ALIGN, MAP_FACE_UV};

use crate::disk_watch::DiskWatch;

/// The map's state.
#[derive(Default)]
pub(crate) struct MapFeature {
    /// The map view, once a map is shown.
    pub(crate) map: Option<MapView>,
    /// The map editor's view and key editor (P5-06).
    pub(crate) map_editor: MapEditor,
    /// The map the map editor last showed, to notice one newly opened.
    pub(crate) map_opened: Option<PathBuf>,
    /// When the open map was last backed up, in egui seconds, and at which of its edits
    /// (P6-20).
    pub(crate) backed_up: (f64, MapRevision),
    /// The modified times of the open map and the project's QuakeC, to notice another
    /// program's writes (P5-17).
    pub(crate) disk_watch: DiskWatch,
    /// The Entities outliner's filter.
    pub(crate) outliner: Outliner,
    /// The Properties panel's edits.
    pub(crate) properties: Properties,
    /// What the map's glue asked of the window, carried out when the call returns.
    requests: Vec<ShellRequest>,
    /// The map text the last copy wrote, which the window puts on the clipboard (P7-08).
    copied: Option<String>,
}

impl MapFeature {
    /// What the map's glue asked of the window since the last call.
    pub(crate) fn take_requests(&mut self) -> Vec<ShellRequest> {
        std::mem::take(&mut self.requests)
    }

    /// Opens the map at `path` in the map editor; [`Shell::sync_map`] shows it.
    pub(crate) fn open_map(&mut self, commands: &mut Commands<'_>, path: PathBuf) {
        let opened: Result<serde_json::Value, ShellError> =
            commands.attempt(MAP_OPEN, &MapOpenParams { path });
        if let Err(error) = opened {
            tracing::warn!(%error, "map not opened");
        }
    }

    /// When a map was opened since the last frame, by `map.open` from anywhere: gives the map
    /// editor the project's entity classes and their models, starts its camera at the player
    /// start and shows the Map tab.
    pub(crate) fn sync_map(&mut self, commands: &mut Commands<'_>) {
        let path = commands
            .session()
            .context()
            .workbench
            .map()
            .map(|document| document.path().to_path_buf());
        // The profile chosen from anywhere, the palette and the agent too, is the one shown.
        let chosen = commands
            .session()
            .context()
            .workbench
            .chosen_profile()
            .to_owned();
        self.map_editor.profiles_mut().set_chosen(chosen);
        if path == self.map_opened {
            // The world named other WADs, or another game was chosen (P6-10).
            let context = commands.session().context();
            let chosen = context.preview.game_folder();
            if context
                .workbench
                .map()
                .is_some_and(|document| self.map_editor.textures_stale(document, chosen))
            {
                self.load_map_textures(commands);
            }
            return;
        }
        self.map_opened.clone_from(&path);
        if path.is_none() {
            return;
        }
        self.load_map_classes(commands);
        self.load_map_textures(commands);
        // Another map's portals and last run are not this one's.
        self.map_editor.profiles_mut().set_portals(None);
        self.map_editor.profiles_mut().clear_run();
        self.load_profiles(commands);
        self.backed_up.1 = commands.session().context().map_edits();
        self.map_editor.set_disk(DiskNotice::None);
        self.map_editor.reset_camera();
        self.map_editor.set_keys(None, None);
        self.requests.push(ShellRequest::ShowCentre(CentreTab::Map));
    }

    /// Gives the map editor the open map's textures: the WADs its world names, found in the
    /// project, beside the map or in the detected game, and the game's palette (P6-10).
    pub(crate) fn load_map_textures(&mut self, commands: &mut Commands<'_>) {
        let context = commands.session().context();
        let Some(document) = context.workbench.map() else {
            return;
        };
        let game = context.preview.game_base_path();
        let sources = TextureSources::load(
            document,
            context.workbench.root(),
            game.as_deref(),
            context.preview.game_folder(),
        );
        self.map_editor.set_texture_sources(sources);
        let root = commands
            .session()
            .context()
            .workbench
            .root()
            .map(Path::to_path_buf);
        self.map_editor.set_project_root(root);
    }

    /// Gives the map editor the project's entity classes and their models.
    pub(crate) fn load_map_classes(&mut self, commands: &mut Commands<'_>) {
        let classes: Result<EntitiesFound, ShellError> =
            commands.attempt(PROJECT_ENTITIES, &EntitiesParams::default());
        let context = commands.session().context();
        let project = context.workbench.root().map(Path::to_path_buf);
        let game = context.preview.game_base_path();
        match classes {
            Ok(found) => {
                self.map_editor.set_classes(
                    found.classes.iter(),
                    project.as_deref(),
                    game.as_deref(),
                );
                self.map_editor.set_targeting(found.targeting);
            }
            Err(error) => tracing::warn!(%error, "no entity classes for the map"),
        }
    }

    /// Looks at the open map's file and the project's QuakeC for another program's writes
    /// (P5-17): a map with no unsaved edits is read again, one with unsaved edits says so, and
    /// changed QuakeC gives the map editor its classes again.
    pub(crate) fn watch_disk(&mut self, commands: &mut Commands<'_>, ctx: &egui::Context) {
        ctx.request_repaint_after(DiskWatch::every());
        let now = std::time::Instant::now();
        if !self.disk_watch.due(now) {
            return;
        }
        let workbench = &commands.session().context().workbench;
        let mut paths = workbench.project_files();
        paths.extend(workbench.map().map_or_default(MapDocument::watched));
        paths.extend(self.map_editor.texture_watched());
        paths.extend(self.map_editor.external_watched());
        let changed = self.disk_watch.changed(now, paths);
        if !changed.is_empty() {
            self.files_changed(commands, changed);
        }
    }

    /// Asks whether the open map's files changed, whatever their modified times say.
    pub(crate) fn check_map_on_disk(&mut self, commands: &mut Commands<'_>) {
        let watched = commands
            .session()
            .context()
            .workbench
            .map()
            .map_or_default(MapDocument::watched);
        if !watched.is_empty() {
            self.files_changed(commands, watched);
        }
    }

    /// Gives `changed` to `files.changed` and shows what it did.
    pub(crate) fn files_changed(&mut self, commands: &mut Commands<'_>, changed: Vec<PathBuf>) {
        let map = commands.session().context().workbench.map().is_some();
        let wads = self.map_editor.texture_watched();
        let wad_changed = changed.iter().any(|path| wads.contains(path));
        let externals = self.map_editor.external_watched();
        // An external map written by another program is drawn again (MAP-14).
        if changed.iter().any(|path| externals.contains(path)) {
            self.map_editor.externals_changed();
        }
        let found: Result<FilesChanged, ShellError> =
            commands.attempt(FILES_CHANGED, &FilesChangedParams { paths: changed });
        match found {
            Ok(found) => {
                if found.quakec && map {
                    self.load_map_classes(commands);
                }
                match found.map {
                    MapOnDisk::Same => {}
                    MapOnDisk::Reloaded => self.map_editor.set_disk(DiskNotice::None),
                    MapOnDisk::Conflict => self.map_editor.set_disk(DiskNotice::Changed),
                }
                // A WAD written by another program, or a map read again whose world may name
                // other WADs, gives the browser its textures again (MAP-32).
                if map && (wad_changed || found.map == MapOnDisk::Reloaded) {
                    self.load_map_textures(commands);
                }
            }
            Err(error) => tracing::warn!(%error, "the changed files were not read"),
        }
    }

    /// Runs what the map editor asked for as `map.*` commands, then reads the selected
    /// entity's keys again.
    pub(crate) fn map_actions(&mut self, commands: &mut Commands<'_>, actions: Vec<MapAction>) {
        if actions.is_empty() {
            return;
        }
        for action in actions {
            let result: Result<serde_json::Value, ShellError> = match action {
                MapAction::Select(entity) => {
                    commands.attempt(MAP_SELECT, &MapSelectParams { entity })
                }
                MapAction::Move(entity, by) => {
                    commands.attempt(MAP_MOVE, &MoveParams { entity, by })
                }
                MapAction::Arrange(arrange) => Self::arrange(commands, arrange),
                MapAction::Menu(menu) => self.menu_action(commands, menu),
                MapAction::AddWad(path) => commands.attempt(MAP_ADD_WAD, &AddWadParams { path }),
                MapAction::Delete(entity) => commands.attempt(MAP_DELETE, &EntityParams { entity }),
                MapAction::Duplicate(entity) => {
                    commands.attempt(MAP_DUPLICATE, &EntityParams { entity })
                }
                MapAction::SetKey(entity, also, key, value) => commands.attempt(
                    MAP_SET_KEY,
                    &SetKeyParams {
                        entity,
                        key,
                        value,
                        also,
                    },
                ),
                MapAction::Place(classname, origin) => self.place(commands, classname, origin),
                MapAction::SetFlag(entity, also, flag, change) => commands.attempt(
                    MAP_SET_FLAG,
                    &SetFlagParams {
                        entity,
                        also,
                        flag,
                        change,
                    },
                ),
                MapAction::Link(entity, key, target) => commands.attempt(
                    MAP_LINK,
                    &LinkParams {
                        entity,
                        key,
                        target,
                    },
                ),
                MapAction::Filter(show) => self
                    .filter_map(commands, show)
                    .map(|()| serde_json::Value::Null),
                MapAction::Group(action) => self.group_action(commands, action),
                MapAction::MoveTo(entity, into) => {
                    commands.attempt(MAP_MOVE_TO, &MoveToParams { entity, into })
                }
                MapAction::View(setting) => Self::view_setting(commands, setting),
                MapAction::ImportTexture(path, name, dither) => {
                    let imported = commands.attempt(
                        TEXTURE_IMPORT,
                        &TextureImportParams {
                            path,
                            name: Some(name),
                            dither,
                        },
                    );
                    // The browser and the open map read the WAD again, the new texture in it.
                    self.load_map_textures(commands);
                    imported
                }
                MapAction::CheckLeaks => commands
                    .execute(MAP_LEAKS, serde_json::json!({}))
                    .map_err(ShellError::from),
                MapAction::PlayFromHere => self.play_from_here(commands),
                MapAction::ListPrefabs => self.list_prefabs(commands),
                MapAction::PlacePrefab(path, origin) => commands.attempt(
                    MAP_PLACE_PREFAB,
                    &PlacePrefabParams {
                        path,
                        origin,
                        grid: Some(commands.session().context().layout.map_grid.units()),
                    },
                ),
                MapAction::Clipboard(action) => self.clipboard_action(commands, action),
                MapAction::CheckIssues => self.check_map_issues(commands),
                MapAction::Fix(fix) => {
                    let fixed =
                        commands.attempt::<_, serde_json::Value>(MAP_FIX, &MapFixParams { fix });
                    // The list shows the map as it now is, the issue fixed gone.
                    fixed.and_then(|_| self.check_map_issues(commands))
                }
                MapAction::Profile(action) => self.profile_action(commands, action),
                MapAction::Review(action) => self.review_action(commands, action),
                MapAction::Find(search) => {
                    let found: Result<MapFound, ShellError> = commands.attempt(MAP_SEARCH, &search);
                    found.and_then(|found| {
                        let at = commands.session().context().map_edits();
                        let entity = self.map_editor.select_found(&found);
                        self.map_editor.set_found(found, at);
                        commands.attempt(MAP_SELECT, &MapSelectParams { entity })
                    })
                }
                MapAction::SelectFound(found) => {
                    let entity = self.map_editor.select_found(&found);
                    commands.attempt(MAP_SELECT, &MapSelectParams { entity })
                }
                MapAction::RemoveKey(entity, key) => {
                    commands.attempt(MAP_REMOVE_KEY, &RemoveKeyParams { entity, key })
                }
                MapAction::Save => {
                    let saved: Result<MapSaved, ShellError> =
                        commands.attempt(MAP_SAVE, &SaveParams { bsp: BspSave::Ent });
                    match saved {
                        Ok(saved) => {
                            self.disk_watch.wrote(saved.path);
                            Ok(serde_json::Value::Null)
                        }
                        Err(error) => {
                            // A refusal may be a change the watcher missed: look now, so the
                            // notice offers a way out.
                            self.check_map_on_disk(commands);
                            Err(error)
                        }
                    }
                }
                MapAction::Resolve(keep) => {
                    let resolved: Result<serde_json::Value, ShellError> =
                        commands.attempt(MAP_RESOLVE, &ResolveParams { keep });
                    if resolved.is_ok() {
                        self.map_editor.set_disk(DiskNotice::None);
                    }
                    resolved
                }
                MapAction::Brush(action) => self.brush_action(commands, action),
                MapAction::Compare => {
                    let compared: Result<Vec<MapDifference>, ShellError> =
                        commands.attempt(MAP_COMPARE, &serde_json::json!({}));
                    compared.map(|differences| {
                        let lines = differences
                            .into_iter()
                            .map(|d| format!("{}: {}", d.entity, d.text))
                            .collect();
                        self.map_editor.set_disk(DiskNotice::Compared(lines));
                        serde_json::Value::Null
                    })
                }
            };
            let refused = result.err().map(|error| {
                tracing::warn!(%error, "map edit refused");
                error.to_string()
            });
            self.map_editor.set_refused(refused);
        }
    }

    /// Lists the open map's issues in the map editor's issue browser (P6-19).
    pub(crate) fn check_map_issues(
        &mut self,
        commands: &mut Commands<'_>,
    ) -> Result<serde_json::Value, ShellError> {
        let found: MapIssues = commands.attempt(MAP_ISSUES, &serde_json::json!({}))?;
        let at = commands.session().context().map_edits();
        self.map_editor.set_issues(found.issues, at);
        Ok(serde_json::Value::Null)
    }

    /// Accepts or rejects map edits waiting for review, and shows the hunks left (P6-21).
    pub(crate) fn review_action(
        &mut self,
        commands: &mut Commands<'_>,
        action: ReviewAction,
    ) -> Result<serde_json::Value, ShellError> {
        let (id, hunks) = match action {
            ReviewAction::Accept(hunks) => (MAP_ACCEPT, hunks),
            ReviewAction::Reject(hunks) => (MAP_REJECT, hunks),
        };
        let left: Vec<MapHunk> = commands.attempt(id, &MapHunksParams { hunks })?;
        let revision = commands
            .session()
            .context()
            .workbench
            .map()
            .map(MapDocument::revision);
        self.map_editor.review_mut().set_hunks(left, revision);
        Ok(serde_json::Value::Null)
    }

    /// Reads the map edits waiting for review again when the map changed, its review included,
    /// from anywhere: the agent, MCP and the palette too (P6-21); whether it did.
    pub(crate) fn refresh_map_review(&mut self, commands: &mut Commands<'_>) -> bool {
        let Some(revision) = commands
            .session()
            .context()
            .workbench
            .map()
            .map(MapDocument::revision)
        else {
            return false;
        };
        if !self.map_editor.review().stale(revision) {
            return false;
        }
        let read: Result<Vec<MapHunk>, ShellError> =
            commands.attempt(MAP_STAGED, &serde_json::json!({}));
        self.map_editor
            .review_mut()
            .set_hunks(read.unwrap_or_default(), Some(revision));
        true
    }

    /// Runs a compile control's action (P6-20).
    pub(crate) fn profile_action(
        &mut self,
        commands: &mut Commands<'_>,
        action: ProfileAction,
    ) -> Result<serde_json::Value, ShellError> {
        match action {
            ProfileAction::Choose(name) => {
                let chosen = commands.attempt::<_, String>(
                    MAP_CHOOSE_PROFILE,
                    &RunProfileParams {
                        profile: Some(name),
                    },
                );
                self.load_profiles(commands);
                chosen.map(|_| serde_json::Value::Null)
            }
            ProfileAction::Run => self.run_profile(commands),
            ProfileAction::ShowPortals => {
                let read: Result<MapPortals, ShellError> =
                    commands.attempt(MAP_PORTALS, &serde_json::json!({}));
                read.map(|read| {
                    self.map_editor
                        .profiles_mut()
                        .set_portals(Some(read.portals));
                    serde_json::Value::Null
                })
            }
            ProfileAction::HidePortals => {
                self.map_editor.profiles_mut().set_portals(None);
                Ok(serde_json::Value::Null)
            }
            ProfileAction::RestoreBackup => self.restore_backup(commands),
        }
    }

    /// Gives the map editor the compile profiles and the one chosen (P6-20).
    pub(crate) fn load_profiles(&mut self, commands: &mut Commands<'_>) {
        let listed: Result<Vec<Profile>, ShellError> =
            commands.attempt(MAP_PROFILES, &serde_json::json!({}));
        let names = listed
            .map_or_default(|profiles| profiles.into_iter().map(|profile| profile.name).collect());
        let chosen = commands
            .session()
            .context()
            .workbench
            .chosen_profile()
            .to_owned();
        self.map_editor.profiles_mut().set_profiles(names, chosen);
    }

    /// Runs the chosen compile profile and shows how it ended; the portals shown are read
    /// again, from the compile just made (P6-20).
    pub(crate) fn run_profile(
        &mut self,
        commands: &mut Commands<'_>,
    ) -> Result<serde_json::Value, ShellError> {
        let ran: Result<ProfileRun, ShellError> =
            commands.attempt(MAP_RUN_PROFILE, &RunProfileParams::default());
        let run = ran?;
        self.map_editor.profiles_mut().set_run(run);
        if self.map_editor.profiles().shows_portals() {
            let read: Result<MapPortals, ShellError> =
                commands.attempt(MAP_PORTALS, &serde_json::json!({}));
            if let Ok(read) = read {
                self.map_editor
                    .profiles_mut()
                    .set_portals(Some(read.portals));
            }
        }
        Ok(serde_json::Value::Null)
    }

    /// Puts the open map's newest backup in the editor (P6-20).
    pub(crate) fn restore_backup(
        &mut self,
        commands: &mut Commands<'_>,
    ) -> Result<serde_json::Value, ShellError> {
        let listed: Vec<Backup> = commands.attempt(MAP_BACKUPS, &serde_json::json!({}))?;
        let Some(newest) = listed.into_iter().next() else {
            return Ok(serde_json::Value::Null);
        };
        commands.attempt(MAP_RESTORE_BACKUP, &RestoreParams { path: newest.path })
    }

    /// Backs the open map up when it has edits not saved or backed up and the last backup
    /// is [`MAP_BACKUP_INTERVAL`] seconds old (P6-20).
    pub(crate) fn back_up_map(&mut self, commands: &mut Commands<'_>, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let edits = commands.session().context().map_edits();
        let unsaved = commands
            .session()
            .context()
            .workbench
            .map()
            .is_some_and(MapDocument::is_unsaved);
        let (at, backed) = self.backed_up;
        if !unsaved || edits == backed || now - at < MAP_BACKUP_INTERVAL {
            return;
        }
        self.backed_up = (now, edits);
        let taken: Result<Backup, ShellError> =
            commands.attempt(MAP_BACKUP, &serde_json::json!({}));
        if let Err(error) = taken {
            tracing::warn!(%error, "the map could not be backed up");
        }
    }

    /// Lists the project's prefabs in the map editor's prefab browser; none when no project
    /// is open, so the browser does not ask again (P6-23).
    pub(crate) fn list_prefabs(
        &mut self,
        commands: &mut Commands<'_>,
    ) -> Result<serde_json::Value, ShellError> {
        let found: Result<Prefabs, ShellError> =
            commands.attempt(MAP_PREFABS, &serde_json::json!({}));
        let prefabs = found.as_ref().map_or_default(|found| found.prefabs.clone());
        self.map_editor.set_prefabs(prefabs);
        found.map(|_| serde_json::Value::Null)
    }

    /// Compiles the open map fast and plays it with the player's eye at the 3D view's camera,
    /// facing its way (P6-12, MAP-13).
    pub(crate) fn play_from_here(
        &mut self,
        commands: &mut Commands<'_>,
    ) -> Result<serde_json::Value, ShellError> {
        let Some(camera) = self.map_editor.camera().copied() else {
            return Ok(serde_json::Value::Null);
        };
        let played = commands.attempt(
            MAP_PLAY_FROM_HERE,
            &PlayFromHereParams {
                eye: camera.origin.map(f64::from),
                angles: Some([
                    f64::from(camera.pitch.get()),
                    f64::from(camera.yaw.get()),
                    0.0,
                ]),
                region: None,
            },
        );
        // As Play from the toolbar: the error on the toolbar, and the guide when no game was
        // found (P3-21).
        commands.set_notice(played.as_ref().err().map(ToString::to_string));
        if played.is_err() {
            self.requests.push(ShellRequest::OpenGameGuide);
        }
        played
    }

    /// Runs what the view's context menu asked that no other action does (P7-08).
    fn menu_action(
        &mut self,
        commands: &mut Commands<'_>,
        action: MenuAction,
    ) -> Result<serde_json::Value, ShellError> {
        match action {
            MenuAction::MakeBrushEntity(classname, brushes) => commands.attempt(
                MAP_MAKE_BRUSH_ENTITY,
                &MakeBrushEntityParams { classname, brushes },
            ),
            MenuAction::MakeStructural(entity) => {
                commands.attempt(MAP_MAKE_STRUCTURAL, &MakeStructuralParams { entity })
            }
            MenuAction::ExtractLinked(entities) => {
                // The group the first names, its own entity.
                let group = commands
                    .session()
                    .context()
                    .workbench
                    .map()
                    .and_then(|map| {
                        let entities_now = &map.file().entities;
                        let id = entities_now.get(entities.first()?.get())?.group()?;
                        entities_now
                        .iter()
                        .position(|e| {
                            matches!(e.container(), Some((qs_map::RoomKind::Group, g)) if g == id)
                        })
                        .map(EntityIndex::new)
                    });
                let Some(group) = group else {
                    return Ok(serde_json::Value::Null);
                };
                commands.attempt(MAP_EXTRACT_LINKED, &ExtractLinkedParams { group, entities })
            }
            MenuAction::SelectAll(entities) => {
                let mut entities = entities.into_iter();
                let entity = entities.next();
                self.map_editor.select_also(entities.collect());
                commands.attempt(MAP_SELECT, &MapSelectParams { entity })
            }
        }
    }

    /// Places an entity of `classname` at `origin`, naming the current layer, as TrenchBroom
    /// puts a new entity there (P7-08).
    fn place(
        &self,
        commands: &mut Commands<'_>,
        classname: String,
        origin: [f64; 3],
    ) -> Result<serde_json::Value, ShellError> {
        let keys = self
            .map_editor
            .current_layer()
            .map(|(_, id)| (LAYER_KEY.to_owned(), id.text()))
            .into_iter()
            .collect();
        commands.attempt(
            MAP_PLACE,
            &PlaceParams {
                classname,
                origin,
                keys,
            },
        )
    }

    /// Flips or turns the selection (P7-08).
    fn arrange(
        commands: &mut Commands<'_>,
        arrange: Arrange,
    ) -> Result<serde_json::Value, ShellError> {
        match arrange {
            Arrange::Flip(entities, brushes, axis) => commands.attempt(
                MAP_FLIP,
                &FlipParams {
                    selection: Selection { entities, brushes },
                    axis,
                    lock: Lock::default(),
                },
            ),
            Arrange::Turn(entities, brushes, degrees) => commands.attempt(
                MAP_TURN,
                &TurnParams {
                    selection: Selection { entities, brushes },
                    degrees,
                    lock: Lock::default(),
                },
            ),
        }
    }

    /// Copies the selection as map text for the window to put on the clipboard, or pastes map
    /// text on the layout's grid (P7-08).
    fn clipboard_action(
        &mut self,
        commands: &mut Commands<'_>,
        action: ClipboardAction,
    ) -> Result<serde_json::Value, ShellError> {
        match action {
            ClipboardAction::Copy(brushes, entities) => {
                let copied: Copied =
                    commands.attempt(MAP_COPY, &CopyParams { brushes, entities })?;
                self.copied = Some(copied.text);
                Ok(serde_json::Value::Null)
            }
            ClipboardAction::Paste(text, at) => {
                let grid = Some(commands.session().context().layout.map_grid.units());
                commands.attempt(MAP_PASTE, &PasteParams { text, at, grid })
            }
        }
    }

    /// Takes the map text the last copy wrote, for the window's clipboard.
    pub(crate) const fn take_copied(&mut self) -> Option<String> {
        self.copied.take()
    }

    /// Runs a change of how the map view looks, a `view.*` command of the layout.
    fn view_setting(
        commands: &mut Commands<'_>,
        setting: ViewSetting,
    ) -> Result<serde_json::Value, ShellError> {
        match setting {
            ViewSetting::Panes(panes) => commands.attempt(MAP_PANES, &MapPanesParams { panes }),
            ViewSetting::Shading(shading) => {
                commands.attempt(MAP_SHADING, &MapShadingParams { shading })
            }
            ViewSetting::Grid(size) => commands.attempt(MAP_GRID, &MapGridParams { size }),
            ViewSetting::Camera(params) => commands.attempt(MAP_CAMERA, &params),
        }
    }

    /// Runs a brush edit as its `map.brush_*` command, on the commands' grid.
    #[expect(
        clippy::too_many_lines,
        reason = "one arm per brush action, each its command's parameters: one match reads \
                  better whole than split"
    )]
    pub(crate) fn brush_action(
        &mut self,
        commands: &mut Commands<'_>,
        action: BrushAction,
    ) -> Result<serde_json::Value, ShellError> {
        let grid = Some(commands.session().context().layout.map_grid.units());
        // TrenchBroom's current material: the texture chosen in the browser, else the map's
        // last face's.
        let texture = self.map_editor.current_texture();
        // A new brush goes into the current layer's own entity, as TrenchBroom keeps a layer's
        // brushes; the world's for the default layer.
        let layer = self.map_editor.current_layer().map(|(entity, _)| entity);
        match action {
            BrushAction::Create(min, max) => commands.attempt(
                MAP_BRUSH_CREATE,
                &BrushCreateParams {
                    entity: layer,
                    min,
                    max,
                    texture,
                    grid,
                },
            ),
            BrushAction::Clip(entity, brush, points, keep) => commands.attempt(
                MAP_BRUSH_CLIP,
                &BrushClipParams {
                    entity,
                    brush,
                    points,
                    keep,
                },
            ),
            BrushAction::Vertex(entity, brush, from, to) => commands.attempt(
                MAP_BRUSH_VERTEX,
                &BrushVertexParams {
                    entity,
                    brush,
                    from,
                    to,
                    // The view has snapped the corner across its plane and keeps its height.
                    grid: Some(0.0),
                },
            ),
            BrushAction::Subtract(entity, brush) => {
                commands.attempt(MAP_BRUSH_SUBTRACT, &BrushParams { entity, brush })
            }
            BrushAction::Merge(entity, brushes) => {
                commands.attempt(MAP_BRUSH_MERGE, &BrushMergeParams { entity, brushes })
            }
            BrushAction::Delete(entity, brush) => {
                commands.attempt(MAP_BRUSH_DELETE, &BrushParams { entity, brush })
            }
            BrushAction::Move(entity, brushes, by, lock) => commands.attempt(
                MAP_BRUSH_MOVE,
                &BrushMoveParams {
                    entity,
                    brushes,
                    by,
                    lock,
                    // The view has snapped the move to the grid.
                    grid: Some(0.0),
                },
            ),
            BrushAction::Texture(entity, brushes, change) => commands.attempt(
                MAP_BRUSH_TEXTURE,
                &BrushTextureParams {
                    entity,
                    brushes,
                    texture: change.texture,
                    shift: change.shift,
                    rotate: change.rotate,
                    scale: change.scale,
                },
            ),
            BrushAction::Uv(faces, edit) => {
                commands.attempt(MAP_FACE_UV, &FaceUvParams { faces, edit })
            }
            BrushAction::AlignFaces(from, to) => {
                commands.attempt(MAP_FACE_ALIGN, &FaceAlignParams { from, to })
            }
            BrushAction::SavePrefab(name, brushes, entities) => {
                let saved = commands.attempt::<_, serde_json::Value>(
                    MAP_SAVE_PREFAB,
                    &SavePrefabParams {
                        name,
                        brushes,
                        entities,
                        grid,
                    },
                );
                // The browser lists the new prefab.
                saved.and_then(|_| self.list_prefabs(commands))
            }
            BrushAction::Shape(min, max, shape) => commands.attempt(
                MAP_BRUSH_SHAPE,
                &BrushShapeParams {
                    entity: layer,
                    min,
                    max,
                    shape,
                    texture,
                    grid,
                },
            ),
            BrushAction::Hollow(entity, brush, wall) => commands.attempt(
                MAP_BRUSH_HOLLOW,
                &BrushHollowParams {
                    entity,
                    brush,
                    wall,
                },
            ),
            BrushAction::Intersect(entity, brushes) => commands.attempt(
                MAP_BRUSH_INTERSECT,
                &BrushIntersectParams { entity, brushes },
            ),
            BrushAction::Scale(entity, brushes, factors, lock) => commands.attempt(
                MAP_BRUSH_SCALE,
                &BrushScaleParams {
                    entity,
                    brushes,
                    factors,
                    about: None,
                    lock,
                },
            ),
            BrushAction::Shear(entity, brushes, along, by, factor, lock) => commands.attempt(
                MAP_BRUSH_SHEAR,
                &BrushShearParams {
                    entity,
                    brushes,
                    along,
                    by,
                    factor,
                    about: None,
                    lock,
                },
            ),
            BrushAction::Sweep(entity, brush, path, lock) => commands.attempt(
                MAP_BRUSH_SWEEP,
                &BrushSweepParams {
                    entity,
                    brush,
                    path,
                    lock,
                    grid,
                },
            ),
            BrushAction::Extrude(face, by, lock, mode) => commands.attempt(
                MAP_BRUSH_EXTRUDE,
                &BrushExtrudeParams {
                    face,
                    by,
                    lock,
                    // The distance is the one typed in the panel, or snapped by the drag.
                    grid: Some(0.0),
                    mode,
                },
            ),
            BrushAction::Arrange(arrange) => Self::arrange(commands, arrange),
            BrushAction::Hull(points) => commands.attempt(
                MAP_BRUSH_HULL,
                &BrushHullParams {
                    entity: layer,
                    points,
                    texture,
                    grid,
                },
            ),
            BrushAction::ReplaceTexture(from, to, brushes) => commands.attempt(
                MAP_REPLACE_TEXTURE,
                &ReplaceTextureParams { from, to, brushes },
            ),
            BrushAction::SplitFace(face, by) => commands.attempt(
                MAP_BRUSH_SPLIT_FACE,
                &BrushSplitFaceParams {
                    face,
                    at: NewVertex::Out { by },
                },
            ),
            BrushAction::Vertices(entity, brush, moves) => commands.attempt(
                MAP_BRUSH_VERTICES,
                &BrushVerticesParams {
                    entity,
                    brush,
                    moves: moves
                        .into_iter()
                        .map(|(from, to)| VertexMove { from, to })
                        .collect(),
                    // The view has snapped the move to the grid.
                    grid: Some(0.0),
                },
            ),
            BrushAction::SplitEdge(entity, brush, edge, by) => commands.attempt(
                MAP_BRUSH_SPLIT_EDGE,
                &BrushSplitEdgeParams {
                    entity,
                    brush,
                    edge,
                    at: NewVertex::Out { by },
                },
            ),
            BrushAction::Chamfer(entity, brush, edge, distance, faces) => commands.attempt(
                MAP_BRUSH_CHAMFER,
                &BrushChamferParams {
                    entity,
                    brush,
                    edge,
                    distance,
                    faces,
                },
            ),
        }
    }

    /// Finds the entities the game does not spawn under `show` with `map.filter`, for the map
    /// editor to hide; every entity shown when it fails.
    pub(crate) fn filter_map(
        &mut self,
        commands: &mut Commands<'_>,
        show: ShowFilter,
    ) -> Result<(), ShellError> {
        let Some(edits) = commands
            .session()
            .context()
            .workbench
            .map()
            .map(MapDocument::edits)
        else {
            return Ok(());
        };
        let Some((skill, mode)) = show.game() else {
            self.map_editor.set_hidden(show, Some(Vec::new()), edits);
            return Ok(());
        };
        let filtered: Result<MapFiltered, ShellError> =
            commands.attempt(MAP_FILTER, &FilterParams { skill, mode });
        match filtered {
            Ok(filtered) => {
                self.map_editor
                    .set_hidden(show, Some(filtered.removed), edits);
                Ok(())
            }
            Err(error) => {
                self.map_editor.set_hidden(show, None, edits);
                Err(error)
            }
        }
    }

    /// Reads the open map's outline again when the map or the outline's filter changed since it
    /// was read (P5-16).
    pub(crate) fn refresh_map_outline(&mut self, commands: &mut Commands<'_>) {
        // The rows change with the map's edits, not with the selection.
        let Some(revision) = commands
            .session()
            .context()
            .workbench
            .map()
            .map(MapDocument::edits)
        else {
            return;
        };
        let Some(filter) = self.map_editor.outline_stale(revision) else {
            return;
        };
        let params = OutlineParams {
            filter: Some(filter.clone()),
        };
        let rows: Result<Vec<OutlineRow>, ShellError> = commands.attempt(MAP_OUTLINE, &params);
        self.map_editor
            .set_outline(rows.unwrap_or_default(), revision, filter);
    }

    /// Reads the keys of the open map's selected entity for the key editor.
    pub(crate) fn refresh_map_keys(&mut self, commands: &mut Commands<'_>) {
        let map = commands.session().context().workbench.map();
        let revision = map.map(MapDocument::revision);
        let selected = map.and_then(MapDocument::selected);
        let keys = selected.and_then(|entity| {
            let keys: Result<EntityKeys, ShellError> =
                commands.attempt(MAP_ENTITY, &EntityParams { entity });
            keys.ok().map(|keys| (entity, keys))
        });
        self.map_editor.set_keys(keys, revision);
        let layers: Result<Vec<LayerInfo>, ShellError> =
            commands.attempt(MAP_LAYERS, &serde_json::json!({}));
        self.map_editor.set_layers(layers.unwrap_or_default());
    }

    /// Runs a hide, isolate, lock or layer change from the map editor as its `map.*` command
    /// (P5-14).
    pub(crate) fn group_action(
        &mut self,
        commands: &mut Commands<'_>,
        action: GroupAction,
    ) -> Result<serde_json::Value, ShellError> {
        match action {
            GroupAction::Hide(entities) => {
                commands.attempt(MAP_HIDE, &EntityListParams { entities })
            }
            GroupAction::Isolate(entities) => {
                commands.attempt(MAP_ISOLATE, &EntityListParams { entities })
            }
            GroupAction::Lock(entities) => {
                commands.attempt(MAP_LOCK, &EntityListParams { entities })
            }
            GroupAction::ShowAll => commands.attempt(MAP_SHOW_ALL, &serde_json::json!({})),
            GroupAction::UnlockAll => commands.attempt(MAP_UNLOCK_ALL, &serde_json::json!({})),
            GroupAction::SetLayer(layer, setting, switch) => commands.attempt(
                MAP_SET_LAYER,
                &SetLayerParams {
                    layer,
                    setting,
                    switch,
                },
            ),
            GroupAction::Group(entities, brushes) => {
                let groups = commands
                    .session()
                    .context()
                    .workbench
                    .map()
                    .map_or(0, |map| {
                        let entities = &map.file().entities;
                        let group = |e: &&qs_map::Entity| {
                            matches!(e.container(), Some((qs_map::RoomKind::Group, _)))
                        };
                        entities.iter().filter(group).count()
                    });
                let name = group_name(groups.saturating_add(1));
                commands.attempt(
                    MAP_GROUP,
                    &GroupParams {
                        name,
                        entities,
                        brushes,
                    },
                )
            }
            GroupAction::Ungroup(group) => {
                commands.attempt(MAP_UNGROUP, &ContainerParams { group })
            }
            GroupAction::LinkDuplicate(group) => {
                commands.attempt(MAP_LINK_DUPLICATE, &ContainerParams { group })
            }
            GroupAction::Separate(group) => {
                commands.attempt(MAP_SEPARATE, &ContainerParams { group })
            }
            GroupAction::AddLayer => {
                let count = self.map_editor.layer_count();
                commands.attempt(
                    MAP_ADD_LAYER,
                    &AddLayerParams {
                        name: layer_name(count.saturating_add(1)),
                    },
                )
            }
        }
    }
}

impl Workspace {
    /// The open map's edits, which a list of its issues or matches is good for.
    pub(crate) fn map_edits(&self) -> MapRevision {
        self.workbench.map().map_or_default(MapDocument::edits)
    }
}
