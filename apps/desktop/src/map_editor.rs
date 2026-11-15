// SPDX-License-Identifier: GPL-3.0-or-later

//! The map editor (P5-06, MAP-4, MAP-20, P5-13, MAP-27): the open map in 3D as lines, its
//! entities as boxes and model wireframes, picked with the mouse and edited with TrenchBroom's
//! keys, with the links between them and a filter by skill and game; the entity browser below
//! it, dragged from to place; and the key editor in the Properties panel, with fields chosen
//! from the QuakeC, an eyedropper to link without typing, and editing of several entities at
//! once. Every edit is a [`MapAction`] the shell runs as a `map.*` command, so each has undo.
//! `docs/map-editor.md` maps TrenchBroom's bindings to these.

use qs_render::Degrees;
use std::collections::{BTreeSet, HashMap};

use eframe::egui::{
    self, Color32, Key, Modifiers, Pos2, Rect, RichText, Sense, Stroke, Ui, WidgetInfo, WidgetType,
};
use eframe::egui_wgpu::RenderState;
use qs_layout::{
    CameraSpeed, Direction, Layout, MapCamera, MapCameraParams, MapFov, MapGrid, MapPanes,
    MapShading,
};
use qs_map_edit::MapReport;
use qs_map_edit::texture_import::DitherChoice;
use qs_map_edit::textures::TextureSources;
#[cfg(test)]
use qs_map_edit::uv::FaceRef;
use qs_map_edit::{Axis, BrushIndex, ClassInfo};
use qs_map_edit::{
    EntityIndex, EntityKeys, FlagChange, Keep, LayerInfo, MapDocument, MapFound, MapIssue,
    MapRevision, MapSearchParams, OutlineRow, Prefab, QuickFix,
};
use qs_render::Camera;
use qs_workbench::map_filter::{GameMode, Skill};

use crate::help::{self, Topic};
use crate::map_browser::{ClassDrag, EntityBrowser};
use crate::map_fields::{Field, spawnflags};
use crate::map_groups::{GroupAction, Grouping, layers_ui, overview_keys, selection_ui};
use crate::map_issues::IssueBrowser;
use crate::map_leaks::LeakView;
use crate::map_outline::{OutlineAction, OutlineView};
use crate::map_panes::{Pane, Plan, PlanCamera, Projection, panes};
use crate::map_prefabs::{PrefabBrowser, PrefabDrag};
use crate::map_profiles::{ProfileAction, ProfileView};
use crate::map_review::{ReviewAction, ReviewView};
use crate::map_scene::{ClassLook, Line, LinkView, SPAWNFLAGS, Scene, load_models, start_camera};
use crate::map_select::{Picked, Region};
use crate::map_shading::{ShadedView, TRIGGER, TexturedFrom};
use crate::map_tools::{BrushAction, BrushTools, MapTool, PickMode};
use crate::shell::name_field;
use crate::strings::{Text, also_selected, panes_text};
use crate::texture_browser::TextureBrowser;
use crate::theme::{Theme, Token};

/// Degrees turned per point the pointer drags with the right button (TrenchBroom: look).
const TURN: f32 = 0.25;
/// Map units moved per point the wheel scrolls (TrenchBroom: move forward and back).
const TRAVEL: f32 = 2.0;
/// Map units the middle button pans per point dragged (TrenchBroom: pan).
const PAN: f32 = 1.0;
/// Map units a second the fly keys move the camera (TrenchBroom: W, S, A, D, Q, X).
const FLY_SPEED: f32 = 512.0;
/// TrenchBroom's grid, `Grid(4)`: 2^4 units, the step the arrow keys move a selection.
/// TrenchBroom turns a selection by a right angle (`half_pi`).
const QUARTER_TURN: f64 = 90.0;
/// The key editor's columns: the key, its value and its buttons.
const KEY_COLUMNS: usize = 3;
/// The height of the entity browser below the view, in points (the mockup's texture
/// browser), at most half the space.
const BROWSER_HEIGHT: f32 = 188.0;
/// How far in from the 3D pane's corner the Lit view's note is written, in points.
const NOTE_MARGIN: f32 = 8.0;
/// The least height of the map view, in points, however short the centre.
pub(crate) const MIN_VIEW_HEIGHT: f32 = 160.0;
/// The least height of the browser below the view: its tabs and a row of its contents.
const MIN_BROWSER_HEIGHT: f32 = 120.0;
/// How far in front of the camera a class dropped into the view is placed, in map units.
const PLACE_DISTANCE: f32 = 256.0;
/// The radius of the dot at the end of a link that is fired, in points.
const LINK_END: f32 = 3.0;
/// How strongly a light's preview is drawn, so it does not hide the brushes behind it.
const LIGHT_ALPHA: f32 = 0.6;
/// The brightest a light's every channel may be and still be drawn in the muted colour, so a
/// black light's reach is seen.
const VISIBLE_COLOUR: f32 = 0.05;
/// The width of the new key's and new value's fields, in points.
const NEW_PAIR_WIDTH: f32 = 90.0;
/// The map units the camera stands back from a framed box beyond that, so a small entity is not
/// right against the eye.
const FRAME_MARGIN: f32 = 64.0;
/// How far a 2D view's name is from its corner, in points.
const PANE_LABEL_INSET: f32 = 6.0;
/// The size of a 2D view's name, in points.
const PANE_LABEL_SIZE: f32 = 12.0;

/// What every pane of one frame of the view reads.
#[derive(Clone, Copy)]
struct Frame<'a> {
    /// The entities not drawn.
    hidden: &'a BTreeSet<EntityIndex>,
    /// The entity selected.
    selected: Option<EntityIndex>,
    /// The map's last check, while it is current (P6-11).
    report: Option<&'a MapReport>,
}

/// An edit or a selection the map editor asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum MapAction {
    /// Select an entity, or clear the selection.
    Select(Option<EntityIndex>),
    /// Move an entity by map units.
    Move(EntityIndex, [f64; 3]),
    /// Flip or turn the selection (P7-08).
    Arrange(Arrange),
    /// Delete an entity.
    Delete(EntityIndex),
    /// Duplicate an entity.
    Duplicate(EntityIndex),
    /// Set a key of an entity and of the others listed.
    SetKey(EntityIndex, Vec<EntityIndex>, String, String),
    /// Set or clear a spawnflag of an entity and of the others listed, each keeping its other
    /// flags.
    SetFlag(EntityIndex, Vec<EntityIndex>, u32, FlagChange),
    /// Place a point entity of a class at an origin.
    Place(String, [f64; 3]),
    /// Point a key of an entity, one the code fires entities by, at another entity.
    Link(EntityIndex, String, EntityIndex),
    /// Show only the entities the game spawns at a skill and in a kind of game.
    Filter(ShowFilter),
    /// Hide, isolate or lock entities, or change a layer (P5-14).
    Group(GroupAction),
    /// Move an entity or group into a layer or group, the world for the default layer (P5-16).
    MoveTo(EntityIndex, EntityIndex),
    /// Remove a key of an entity.
    RemoveKey(EntityIndex, String),
    /// Save the map.
    Save,
    /// After another program changed the map, keep the file or the edits.
    Resolve(Keep),
    /// List how the changed file differs.
    Compare,
    /// Create, cut, reshape, subtract, merge or delete brushes (P6-08).
    Brush(BrushAction),
    /// Change how the map view looks: its panes, shading, grid or camera.
    View(ViewSetting),
    /// Import the PNG at the path as a texture of the name (P6-16).
    ImportTexture(std::path::PathBuf, String, DitherChoice),
    /// Compile the map fast and play it with the player at the 3D view's camera (P6-12).
    PlayFromHere,
    /// Check the map for leaks and missing textures (P6-11).
    CheckLeaks,
    /// List the project's prefabs (P6-23).
    ListPrefabs,
    /// Copy the prefab at the path into the map at an origin (P6-23).
    PlacePrefab(std::path::PathBuf, [f64; 3]),
    /// Copy the selection to the clipboard, or paste map text (P7-08).
    Clipboard(ClipboardAction),
    /// List the map's issues (P6-19).
    CheckIssues,
    /// Apply an issue's quick fix (P6-19).
    Fix(QuickFix),
    /// Search the map's entities and brushes (P6-19).
    Find(MapSearchParams),
    /// Choose or run a compile profile, show the portals or restore a backup (P6-20).
    Profile(ProfileAction),
    /// Accept or reject map edits waiting for review (P6-21).
    Review(ReviewAction),
}

/// The external maps the view draws (MAP-14): the project's folder they are looked for in after
/// the map's own, every file an instance may name, watched so one written or made is drawn, the
/// files read, kept until one changes, and how many times one changed on disk.
#[derive(Debug, Default)]
struct Externals {
    root: Option<std::path::PathBuf>,
    files: Vec<std::path::PathBuf>,
    read: std::collections::HashMap<std::path::PathBuf, Option<Vec<u8>>>,
    changes: u64,
}

impl Externals {
    /// The bytes of the external map `name` of the map `document`, from the first of its
    /// folders that has it, read once until one changes; every place it may be is watched.
    fn read(&mut self, document: &MapDocument, name: &str) -> Option<Vec<u8>> {
        let mut found = None;
        for dir in document.external_folders(self.root.as_deref()) {
            let path = dir.join(name);
            if !self.files.contains(&path) {
                self.files.push(path.clone());
            }
            if found.is_none() {
                found = self
                    .read
                    .entry(path)
                    .or_insert_with_key(|path| std::fs::read(path).ok())
                    .clone();
            }
        }
        found
    }

    /// Forgets what was read and watched, one of the files having changed or the scene being
    /// built again.
    fn forget(&mut self) {
        self.files.clear();
    }
}

/// The entities the map view shows: every one, or those the game spawns at a skill or in a
/// kind of game (P5-13, `map.filter`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShowFilter {
    /// Every entity.
    #[default]
    Every,
    /// Single player on easy.
    Easy,
    /// Single player on normal.
    Normal,
    /// Single player on hard.
    Hard,
    /// Single player on nightmare.
    Nightmare,
    /// Coop on normal.
    Coop,
    /// Deathmatch.
    Deathmatch,
}

impl ShowFilter {
    /// Every choice, in the order offered.
    const ALL: [Self; 7] = [
        Self::Every,
        Self::Easy,
        Self::Normal,
        Self::Hard,
        Self::Nightmare,
        Self::Coop,
        Self::Deathmatch,
    ];

    /// The skill and kind of game it spawns the map at, none for every entity.
    #[must_use]
    pub const fn game(self) -> Option<(Skill, GameMode)> {
        match self {
            Self::Every => None,
            Self::Easy => Some((Skill::Easy, GameMode::Single)),
            Self::Normal => Some((Skill::Normal, GameMode::Single)),
            Self::Hard => Some((Skill::Hard, GameMode::Single)),
            Self::Nightmare => Some((Skill::Nightmare, GameMode::Single)),
            Self::Coop => Some((Skill::Normal, GameMode::Coop)),
            Self::Deathmatch => Some((Skill::Normal, GameMode::Deathmatch)),
        }
    }

    /// Its name in the menu.
    const fn text(self) -> Text {
        match self {
            Self::Every => Text::MapShowEvery,
            Self::Easy => Text::MapSkillEasy,
            Self::Normal => Text::MapSkillNormal,
            Self::Hard => Text::MapSkillHard,
            Self::Nightmare => Text::MapSkillNightmare,
            Self::Coop => Text::MapCoop,
            Self::Deathmatch => Text::MapDeathmatch,
        }
    }
}

/// What the map editor says about the map's file on disk (P5-17).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DiskNotice {
    /// Nothing: the file is the one read or saved.
    #[default]
    None,
    /// Another program changed it while the map had unsaved edits.
    Changed,
    /// The same, with how the file differs, one line each.
    Compared(Vec<String>),
}

/// The map editor's view state.
#[derive(Debug, Default)]
pub struct MapEditor {
    camera: Option<Camera>,
    scene: Scene,
    /// The document's path and edits, and the external maps' changes, the scene was built
    /// from.
    built: Option<(std::path::PathBuf, MapRevision, u64)>,
    /// The external maps the scene drew (MAP-14), each as read, and how many times one of them
    /// changed on disk.
    externals: Externals,
    classes: HashMap<String, ClassLook>,
    /// The classnames, sorted, as the browser lists them.
    names: Vec<String>,
    models: HashMap<String, Vec<Line>>,
    browser: EntityBrowser,
    /// The selected entity's keys, as `map.entity` gave them.
    keys: Option<(EntityIndex, EntityKeys)>,
    /// The document revision the keys were read at.
    keys_read: Option<MapRevision>,
    /// The values being typed, by key.
    typing: HashMap<String, String>,
    /// The new key and value being typed.
    adding: (String, String),
    /// Why the last map command asked for was refused, until one runs.
    refused: Option<String>,
    /// The notice about the map's file on disk.
    disk: DiskNotice,
    /// The keys the code fires entities by (MAP-8.8).
    targeting: Vec<String>,
    /// The entities selected with Shift besides the selected one, which key edits also set.
    also: BTreeSet<EntityIndex>,
    /// Which links the view draws.
    links: LinkView,
    /// Which lights' previews the map view draws (P7-09, LIGHT-7.4).
    lights: LinkView,
    /// Which entities the view shows.
    show: ShowFilter,
    /// The entities the filter hides, and the map's edits it was found at.
    hidden: BTreeSet<EntityIndex>,
    hidden_read: Option<MapRevision>,
    /// The key the eyedropper sets on the next entity clicked.
    picking: Option<String>,
    /// The key the eyedropper last set, which an Alt-drag sets too.
    link_key: Option<String>,
    /// The entity an Alt-drag links from.
    link_from: Option<EntityIndex>,
    /// The entities the document hides, by `map.hide`, `map.isolate` or a hidden layer
    /// (P5-14), and the revision they were read at.
    kept_out: (BTreeSet<EntityIndex>, Option<MapRevision>),
    /// The map's layers, for the Properties panel.
    layers: Vec<LayerInfo>,
    /// The map outline in the left panel (P5-16).
    outline: OutlineView,
    /// What to frame in the view on its next frame: an entity chosen in the outline, or the
    /// selection (Cmd+U).
    frame: Option<Framing>,
    /// The point an Alt+right-drag orbits the camera about, while it lasts (P7-08).
    orbit: Option<[f32; 3]>,
    /// The camera settings, the layout's (P7-08).
    camera_settings: MapCamera,
    /// How far Shift and the wheel have zoomed the 3D view in (P7-08).
    zoom: ViewZoom,
    /// Where each entity's centre was drawn in the last frame, where a link dragged from it
    /// starts.
    centres: Vec<(EntityIndex, Pos2)>,
    /// The brush tools and the brushes selected (P6-08).
    tools: BrushTools,
    /// Which browser the tab below the view shows.
    bottom: BottomTab,
    /// The texture browser (P6-10).
    textures: TextureBrowser,
    /// Where each 2D view looks, from when it first opens.
    plans: HashMap<Plan, PlanCamera>,
    /// The map's last leak check (P6-11).
    leaks: LeakView,
    /// The prefab browser (P6-23).
    prefabs: PrefabBrowser,
    /// The issue browser and search (P6-19).
    issues: IssueBrowser,
    /// The compile profiles, the last run and the portals (P6-20).
    profiles: ProfileView,
    /// The map edits waiting for review (P6-21).
    review: ReviewView,
    /// How the 3D view draws the map (P7-13).
    shaded: ShadedView,
    /// How many times the scene was built, which the textured mesh follows.
    scene_builds: u64,
    /// The entities with brushes the textured and lit views draw as faces; the lines of the
    /// rest, such as an external map's instance, are still drawn over them.
    faced: BTreeSet<EntityIndex>,
    /// The trigger entities, which the textured view leaves out.
    triggers: BTreeSet<EntityIndex>,
    /// The scene's brushes with a face the textured view leaves out, by their place in it: a
    /// trigger's, or one with a clip, skip or hint face. Their edges stay drawn over the faces,
    /// so they can be seen and picked.
    outlined: Vec<usize>,
}

/// A hash of `set`, which changes when the set does.
fn hash_of(set: &BTreeSet<EntityIndex>) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    set.hash(&mut hasher);
    hasher.finish()
}

/// The browsers of the tab below the map view, as the mockup has them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BottomTab {
    /// The texture browser (P6-10).
    #[default]
    Textures,
    /// The entity browser (P5-13).
    Entities,
    /// The prefab browser (P6-23).
    Prefabs,
    /// The issue browser and search (P6-19).
    Issues,
}

impl MapEditor {
    /// How the 3D view draws the map, for tests.
    #[cfg(test)]
    pub(crate) const fn shaded(&self) -> &ShadedView {
        &self.shaded
    }

    /// The textured mesh of the scene as drawn now, for tests.
    #[cfg(test)]
    #[expect(
        clippy::absolute_paths,
        reason = "test code: one call into the shading module"
    )]
    pub(crate) fn textured_mesh(&self) -> qs_render::MapMesh {
        let sources = self.textures.sources();
        self.scene.textured_mesh(
            &mut |name| crate::map_shading::decoded(sources, name),
            &BTreeSet::new(),
            qs_render::Look::QuakeSpasm,
        )
    }
}

impl MapEditor {
    /// Takes the project's entity classes, and the wireframes of their models from the
    /// project's folder or the game at `game`.
    pub fn set_classes<'a>(
        &mut self,
        classes: impl Iterator<Item = &'a ClassInfo>,
        project: Option<&std::path::Path>,
        game: Option<&std::path::Path>,
    ) {
        self.classes = classes
            .map(|class| (class.name.clone(), ClassLook::from(class)))
            .collect();
        self.names = self.classes.keys().cloned().collect();
        self.names.sort();
        self.models = load_models(&self.classes, project, game);
        self.built = None;
    }

    /// Whether the keys were read before the map's revision `revision`.
    #[must_use]
    pub fn keys_stale(&self, revision: MapRevision) -> bool {
        self.keys_read != Some(revision)
    }

    /// Takes the selected entity's keys, or none, read at the map's revision `revision`.
    pub fn set_keys(
        &mut self,
        keys: Option<(EntityIndex, EntityKeys)>,
        revision: Option<MapRevision>,
    ) {
        self.keys_read = revision;
        if self.keys.as_ref().map(|k| k.0) != keys.as_ref().map(|k| k.0) {
            self.adding = (String::new(), String::new());
        }
        self.typing.clear();
        self.keys = keys;
    }

    /// Why the last map command was refused, for tests.
    #[cfg(test)]
    pub(crate) fn refused_text(&self) -> Option<String> {
        self.refused.clone()
    }

    /// The leak check shown, for tests.
    #[cfg(test)]
    pub(crate) const fn leaks(&self) -> &LeakView {
        &self.leaks
    }

    /// The 3D view's camera: where Play from here puts the player (P6-12).
    pub(crate) const fn camera(&self) -> Option<&Camera> {
        self.camera.as_ref()
    }

    /// How many entities are selected besides the document's own selection, for tests.
    #[cfg(test)]
    pub(crate) fn also_count(&self) -> usize {
        self.also.len()
    }

    /// How many brushes are selected, for tests.
    #[cfg(test)]
    pub(crate) fn chosen_count(&self) -> usize {
        self.tools.chosen().len()
    }

    /// Puts the 3D view's camera at `camera`: where a script or a test looks from.
    pub(crate) const fn set_camera(&mut self, camera: Camera) {
        self.camera = Some(camera);
    }

    /// Takes why the last map command asked for was refused, or none when it ran.
    pub fn set_refused(&mut self, refused: Option<String>) {
        self.refused = refused;
    }

    /// The filter to read the outline with, when it was read for another revision or filter.
    #[must_use]
    pub fn outline_stale(&self, revision: MapRevision) -> Option<String> {
        self.outline.stale(revision)
    }

    /// Takes the outline's rows, read at `revision` with `filter`.
    pub fn set_outline(&mut self, rows: Vec<OutlineRow>, revision: MapRevision, filter: String) {
        self.outline.set_rows(rows, revision, filter);
    }

    /// Draws the map outline, `selected` marked; what it asked for. A row chosen is selected
    /// and framed in the view on its next frame.
    pub fn outline_ui(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        selected: Option<EntityIndex>,
    ) -> Vec<MapAction> {
        let mut actions = Vec::new();
        for action in self.outline.ui(ui, theme, selected) {
            match action {
                OutlineAction::Choose(entity) => {
                    self.frame = Some(Framing::Entity(entity));
                    self.also.clear();
                    actions.push(MapAction::Select(Some(entity)));
                }
                OutlineAction::MoveTo(entity, into) => {
                    actions.push(MapAction::MoveTo(entity, into))
                }
            }
        }
        actions
    }

    /// Takes the map's layers (P5-14).
    pub fn set_layers(&mut self, layers: Vec<LayerInfo>) {
        self.layers = layers;
    }

    /// Takes the keys the code fires entities by (MAP-8.8).
    pub fn set_targeting(&mut self, targeting: Vec<String>) {
        self.targeting = targeting;
        self.built = None;
    }

    /// The filter to find the hidden entities with again, when the map was edited since they
    /// were found at `edits`.
    #[must_use]
    pub fn filter_stale(&self, edits: MapRevision) -> Option<ShowFilter> {
        (self.show != ShowFilter::Every && self.hidden_read != Some(edits)).then_some(self.show)
    }

    /// Takes the filter chosen and the entities it hides, found at the map's edits `edits`;
    /// every entity shown when it could not be found.
    pub fn set_hidden(
        &mut self,
        show: ShowFilter,
        hidden: Option<Vec<EntityIndex>>,
        edits: MapRevision,
    ) {
        match hidden {
            Some(hidden) => {
                self.show = show;
                self.hidden = hidden.into_iter().collect();
            }
            None => {
                self.show = ShowFilter::Every;
                self.hidden.clear();
            }
        }
        self.hidden_read = Some(edits);
    }

    /// The key an Alt-drag sets: the eyedropper's last, else the shortest the code fires by,
    /// the plain one (`target` in id's QuakeC).
    fn link_key(&self) -> Option<String> {
        self.link_key.clone().or_else(|| {
            self.targeting
                .iter()
                .min_by_key(|key| (key.len(), (*key).clone()))
                .cloned()
        })
    }

    /// Takes the prefabs `map.prefabs` listed (P6-23).
    pub fn set_prefabs(&mut self, prefabs: Vec<Prefab>) {
        self.prefabs.set_prefabs(prefabs);
    }

    /// The compile profiles, the last run and the portals (P6-20).
    pub(crate) const fn profiles(&self) -> &ProfileView {
        &self.profiles
    }

    /// The map edits waiting for review (P6-21).
    pub(crate) const fn review(&self) -> &ReviewView {
        &self.review
    }

    /// The map edits waiting for review, to change.
    pub(crate) const fn review_mut(&mut self) -> &mut ReviewView {
        &mut self.review
    }

    /// The compile profiles, to change.
    pub(crate) const fn profiles_mut(&mut self) -> &mut ProfileView {
        &mut self.profiles
    }

    /// Takes the open map's texture sources, read again when the map opens or a WAD changes.
    pub fn set_texture_sources(&mut self, sources: TextureSources) {
        self.textures.set_sources(sources);
    }

    /// Takes the project's folder, where external maps are read when they are not beside the
    /// map (MAP-14).
    pub fn set_project_root(&mut self, root: Option<std::path::PathBuf>) {
        if self.externals.root != root {
            self.externals.root = root;
            self.externals_changed();
        }
    }

    /// The external map files the view draws, to watch for another program's writes.
    pub fn external_watched(&self) -> Vec<std::path::PathBuf> {
        self.externals.files.clone()
    }

    /// Draws the external maps again, one of them changed on disk.
    pub fn externals_changed(&mut self) {
        self.externals.changes = self.externals.changes.wrapping_add(1);
        self.externals.read.clear();
    }

    /// Whether the texture sources were made for other WADs or another chosen game than
    /// `document` and `chosen` now give.
    pub fn textures_stale(&self, document: &MapDocument, chosen: Option<&std::path::Path>) -> bool {
        self.textures.is_stale(document, chosen)
    }

    /// The WAD files the texture browser reads, to watch for another program's writes.
    pub fn texture_watched(&self) -> Vec<std::path::PathBuf> {
        self.textures.watched()
    }

    /// The scene the view draws, for tests.
    #[cfg(test)]
    pub(crate) const fn scene(&self) -> &Scene {
        &self.scene
    }

    /// The brush tools, for tests.
    #[cfg(test)]
    pub(crate) const fn tools(&self) -> &BrushTools {
        &self.tools
    }

    /// The texture browser, for tests.
    #[cfg(test)]
    pub(crate) const fn texture_browser(&self) -> &TextureBrowser {
        &self.textures
    }

    /// Takes what to say about the map's file on disk.
    pub fn set_disk(&mut self, disk: DiskNotice) {
        self.disk = disk;
    }

    /// Where `entity`'s centre was drawn in the last frame, when it was in view.
    #[must_use]
    pub fn drawn_at(&self, entity: EntityIndex) -> Option<Pos2> {
        self.centres
            .iter()
            .find(|(drawn, _)| *drawn == entity)
            .map(|(_, at)| *at)
    }

    /// Forgets the camera, so the next map opened starts at its player start, and the last
    /// map's issues and search.
    pub fn reset_camera(&mut self) {
        self.camera = None;
        self.plans.clear();
        self.issues.clear();
    }

    /// Takes the issues `map.issues` found in the map at `at` (P6-19).
    pub fn set_issues(&mut self, issues: Vec<MapIssue>, at: MapRevision) {
        self.issues.set_issues(issues, at);
    }

    /// Takes what `map.search` found in the map at `at` (P6-19).
    pub fn set_found(&mut self, found: MapFound, at: MapRevision) {
        self.issues.set_found(found, at);
    }

    /// Draws the view's toolbar, `document` in the view and the entity browser below it, in
    /// all the space `ui` has left; what they asked for.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        document: &MapDocument,
        (settings, state): (&Layout, Option<&RenderState>),
    ) -> Vec<MapAction> {
        let (layout, shading) = (settings.map_panes, settings.map_shading);
        self.tools.grid = settings.map_grid;
        self.camera_settings = settings.map_camera;
        // The tab's own width, which the toolbar wraps within and the view keeps to, so a long
        // toolbar never widens the view under the Properties panel.
        let bounds = ui.available_rect_before_wrap();
        let mut actions = self.toolbar(ui, (layout, shading), bounds.width());
        let mut full = ui.available_rect_before_wrap();
        full.max.x = full.max.x.min(bounds.max.x);
        // The view and the browser keep their least heights below however many rows the
        // toolbar took; a parent too short for them scrolls (P7-12, large text).
        full.max.y = full
            .max
            .y
            .max(full.min.y + MIN_VIEW_HEIGHT + MIN_BROWSER_HEIGHT);
        let height = BROWSER_HEIGHT
            .min(full.height() / 2.0)
            .max(MIN_BROWSER_HEIGHT)
            .min(full.height() - MIN_VIEW_HEIGHT);
        let (view, below) = full.split_top_bottom_at_y(full.bottom() - height);
        let mut view_ui = ui.new_child(egui::UiBuilder::new().max_rect(view));
        actions.extend(self.view(&mut view_ui, theme, document, (layout, shading, state)));
        let mut browser_ui = ui.new_child(egui::UiBuilder::new().max_rect(below));
        self.issues.follow(document.edits());
        browser_ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.bottom,
                BottomTab::Textures,
                Text::MapTextures.get(),
            );
            ui.selectable_value(&mut self.bottom, BottomTab::Entities, Text::Entities.get());
            ui.selectable_value(&mut self.bottom, BottomTab::Prefabs, Text::MapPrefabs.get());
            ui.selectable_value(&mut self.bottom, BottomTab::Issues, Text::MapIssues.get());
            let topic = match self.bottom {
                BottomTab::Textures => Topic::MapTextures,
                BottomTab::Entities => Topic::MapEntities,
                BottomTab::Prefabs => Topic::MapPrefabs,
                BottomTab::Issues => Topic::MapIssues,
            };
            help::button(ui, theme, topic);
            ui.separator();
            match self.bottom {
                BottomTab::Textures => self.textures.header(ui, theme, document),
                BottomTab::Entities => self.browser.header(ui, theme, &self.names),
                BottomTab::Prefabs => actions.extend(self.prefabs.header(ui, theme)),
                BottomTab::Issues => actions.extend(self.issues.header(ui, theme)),
            }
        });
        browser_ui.separator();
        match self.bottom {
            BottomTab::Textures => self.textures.body(&mut browser_ui, theme),
            BottomTab::Entities => self.browser.body(
                &mut browser_ui,
                theme,
                &self.names,
                &self.classes,
                &self.models,
            ),
            BottomTab::Prefabs => self.prefabs.body(&mut browser_ui, theme),
            BottomTab::Issues => actions.extend(self.issues.body(&mut browser_ui, theme)),
        }
        if let Some((path, name, dither)) = self.textures.import_window(ui.ctx(), theme) {
            actions.push(MapAction::ImportTexture(path, name, dither));
        }
        ui.advance_cursor_after_rect(full);
        actions
    }

    /// The links shown and the filter by skill and game; the filter chosen.
    fn toolbar(
        &mut self,
        ui: &mut Ui,
        (layout, shading): (MapPanes, MapShading),
        width: f32,
    ) -> Vec<MapAction> {
        let mut actions = Vec::new();
        // Wrapping, so the tools and their settings never widen the view past the centre.
        ui.horizontal_wrapped(|ui| {
            ui.set_max_width(width);
            self.tools.toolbar(ui);
            ui.separator();
            if let Some(shading) = ShadedView::toolbar(ui, shading) {
                actions.push(MapAction::View(ViewSetting::Shading(shading)));
            }
            ui.separator();
            // The pickers keep egui's combo width and cut a long choice short, so a row wraps
            // before one rather than letting it run past the centre (P7-12, large text).
            let combo = ui.spacing().combo_width;
            let panes =
                egui::ComboBox::from_id_salt(ui.id().with(core::any::TypeId::of::<MapPanes>()))
                    .width(combo)
                    .truncate()
                    .selected_text(panes_text(layout).get())
                    .show_ui(ui, |ui| {
                        for choice in MapPanes::ALL {
                            if ui
                                .selectable_label(layout == choice, panes_text(choice).get())
                                .clicked()
                            {
                                actions.push(MapAction::View(ViewSetting::Panes(choice)));
                            }
                        }
                    });
            name_field(ui, &panes.response, Text::MapPanesPicker);
            // A choice cut short is read whole on hover.
            panes.response.on_hover_text(panes_text(layout).get());
            ui.separator();
            ui.label(Text::MapGrid.get());
            let grid = self.tools.grid;
            let picked =
                egui::ComboBox::from_id_salt(ui.id().with(core::any::TypeId::of::<MapGrid>()))
                    .width(combo)
                    .selected_text(grid.to_string())
                    .show_ui(ui, |ui| {
                        for choice in MapGrid::all() {
                            if ui
                                .selectable_label(grid == choice, choice.to_string())
                                .clicked()
                            {
                                actions.push(MapAction::View(ViewSetting::Grid(choice)));
                            }
                        }
                    });
            name_field(ui, &picked.response, Text::MapGridPicker);
            picked.response.on_hover_text(Text::MapGridPicker.get());
            ui.separator();
            ui.label(Text::MapLinks.get());
            ui.selectable_value(
                &mut self.links,
                LinkView::Selection,
                Text::MapLinksSelected.get(),
            );
            ui.selectable_value(&mut self.links, LinkView::All, Text::MapLinksAll.get());
            ui.separator();
            let show_label = egui::WidgetText::from(Text::MapShow.get())
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Body,
                )
                .size()
                .x;
            let row_started = ui.available_size_before_wrap().x < ui.max_rect().width();
            let show_row = show_label + ui.spacing().item_spacing.x + combo;
            if row_started && ui.available_size_before_wrap().x < show_row {
                // The label is no use apart from its picker.
                ui.end_row();
            }
            ui.label(Text::MapShow.get());
            let show =
                egui::ComboBox::from_id_salt(ui.id().with(core::any::TypeId::of::<ShowFilter>()))
                    .width(combo)
                    .truncate()
                    .selected_text(self.show.text().get())
                    .show_ui(ui, |ui| {
                        for show in ShowFilter::ALL {
                            if ui
                                .selectable_label(self.show == show, show.text().get())
                                .clicked()
                            {
                                actions.push(MapAction::Filter(show));
                            }
                        }
                        ui.separator();
                        // The selected lights' reach is always drawn; this draws every light's.
                        let mut every = self.lights == LinkView::All;
                        if ui.checkbox(&mut every, Text::MapLightsAll.get()).changed() {
                            self.lights = if every {
                                LinkView::All
                            } else {
                                LinkView::Selection
                            };
                        }
                    });
            name_field(ui, &show.response, Text::MapShowPicker);
            show.response.on_hover_text(self.show.text().get());
        });
        actions
    }

    /// Draws `document` in all the space `ui` has left, in the panes of the layout chosen,
    /// and reads the mouse and keys; what they asked for.
    fn view(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        document: &MapDocument,
        (layout, shading, state): (MapPanes, MapShading, Option<&RenderState>),
    ) -> Vec<MapAction> {
        let built = (
            document.path().to_path_buf(),
            document.edits(),
            self.externals.changes,
        );
        if self.built.as_ref() != Some(&built) {
            self.externals.forget();
            let externals = &mut self.externals;
            let mut load = |name: &str| externals.read(document, name);
            self.scene = Scene::build(document, &self.classes, &self.targeting, &mut load);
            self.scene_builds = self.scene_builds.wrapping_add(1);
            self.faced = self.scene.brushes.iter().map(|b| b.entity).collect();
            // Triggers are left out of the textured view, as the game does not draw them.
            self.triggers = document
                .file()
                .entities
                .iter()
                .enumerate()
                .filter(|(_, e)| e.classname().is_some_and(|c| c.starts_with(TRIGGER)))
                .map(|(index, _)| EntityIndex::new(index))
                .collect();
            self.outlined = self.scene.outlined(&self.triggers);
            self.built = Some(built);
            let count = document.file().entities.len();
            self.also.retain(|entity| entity.get() < count);
            self.tools.keep_found(&self.scene);
        }
        if self.kept_out.1 != Some(document.revision()) {
            let kept_out = document.hidden_entities();
            self.kept_out = (kept_out, Some(document.revision()));
        }
        let hidden: BTreeSet<EntityIndex> = self.hidden.union(&self.kept_out.0).copied().collect();
        // P7-13: the textured or lit mesh, made from the scene just built, without the hidden.
        let unseen: BTreeSet<EntityIndex> = hidden.union(&self.triggers).copied().collect();
        let from = TexturedFrom {
            scene: self.scene_builds,
            hidden: hash_of(&unseen),
            sources: self.textures.loads(),
        };
        self.shaded.refresh(
            (shading, state),
            document,
            (&self.scene, from, &unseen),
            self.textures.sources(),
        );
        let camera = self
            .camera
            .get_or_insert_with(|| start_camera(document, &self.scene));
        if self
            .leaks
            .fly(camera, ui.input(|i| i.stable_dt), document.report())
        {
            ui.ctx().request_repaint();
        }
        let rect = ui.available_rect_before_wrap();
        let selected = document.selected();
        let framed = match self.frame.take() {
            Some(Framing::Entity(entity)) => self.scene.bounds_of([entity].into_iter(), None),
            Some(Framing::Selection) => self.scene.bounds_of(
                selected.into_iter().chain(self.also.iter().copied()),
                self.tools.bounds(&self.scene),
            ),
            None => None,
        };
        if let Some(bounds) = framed {
            let aspect = rect.width() / rect.height().max(1.0);
            camera.frame(bounds, aspect, FRAME_MARGIN);
        }
        if selected.is_none() {
            self.also.clear();
        }
        let mut actions = Vec::new();
        for (pane, area) in panes(layout, rect) {
            let frame = Frame {
                hidden: &hidden,
                selected,
                report: document.report(),
            };
            actions.extend(self.pane(ui, theme, pane, area, &frame));
        }
        actions
    }

    /// One pane of the view in `area`: its camera, picking, dropping, linking and drawing, in
    /// order; what it asked for.
    #[expect(
        clippy::too_many_lines,
        reason = "one frame of a pane: camera, picking, dropping, linking and drawing, in order"
    )]
    fn pane(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        pane: Pane,
        area: Rect,
        frame: &Frame<'_>,
    ) -> Vec<MapAction> {
        let Frame {
            hidden,
            selected,
            report,
        } = *frame;
        let link_key = self.link_key();
        let response = ui.interact(
            area,
            egui::Id::new(core::any::TypeId::of::<Self>()).with(pane),
            Sense::click_and_drag(),
        );
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, pane.text().get()));
        if response.clicked() || response.secondary_clicked() {
            response.request_focus();
        }
        let scroll = if response.hovered() {
            ui.input(|i| i.smooth_scroll_delta.y)
        } else {
            0.0
        };
        let Some(start) = self.camera else {
            return Vec::new();
        };
        let projection = match pane {
            Pane::Perspective => {
                let camera = self.camera.get_or_insert(start);
                // TrenchBroom's orbit: Alt with the right button turns about the point under
                // the pointer where the drag began, else 256 units ahead along its ray
                // (`Camera::DefaultPointDistance`).
                if response.drag_started_by(egui::PointerButton::Secondary) {
                    let at = ui.input(|i| i.pointer.press_origin());
                    self.orbit = at.filter(|_| ui.input(|i| i.modifiers.alt)).map(|at| {
                        let (from, way) = Projection::perspective(*camera, area).ray(at);
                        let t = self
                            .scene
                            .hit_distance(from, way, hidden)
                            .unwrap_or(ORBIT_DISTANCE);
                        [0, 1, 2].map(|i| {
                            from.get(i).copied().unwrap_or_default()
                                + way.get(i).copied().unwrap_or_default() * t
                        })
                    });
                }
                if response.dragged_by(egui::PointerButton::Secondary) {
                    let drag = response.drag_delta();
                    let look = TURN * self.camera_settings.look_speed.factor();
                    let tilt = look * self.camera_settings.look_vertical.sign();
                    let (yaw, pitch) = (Degrees::new(-drag.x * look), Degrees::new(-drag.y * tilt));
                    match self.orbit {
                        Some(centre) => camera.orbit(centre, yaw, pitch),
                        None => camera.turn(yaw, pitch),
                    }
                }
                if response.drag_stopped() {
                    self.orbit = None;
                }
                if response.dragged_by(egui::PointerButton::Middle) {
                    let drag = response.drag_delta();
                    let pan = PAN * self.camera_settings.pan_speed.factor();
                    camera.travel(0.0, -drag.x * pan, drag.y * pan);
                }
                // Shift and the wheel zoom the view for now, as TrenchBroom's; the wheel alone
                // travels.
                if ui.input(|i| i.modifiers.shift) {
                    // egui turns a vertical scroll with Shift held into a horizontal one.
                    let across = if response.hovered() {
                        ui.input(|i| i.smooth_scroll_delta.x)
                    } else {
                        0.0
                    };
                    self.zoom = self.zoom.scrolled(scroll + across);
                } else {
                    camera.travel(
                        scroll * TRAVEL * self.camera_settings.wheel.sign(),
                        0.0,
                        0.0,
                    );
                }
                camera.fov_x = self.zoom.fov(self.camera_settings.fov);
                Projection::perspective(*camera, area)
            }
            Pane::Plan(plan) => {
                let view = self
                    .plans
                    .entry(plan)
                    .or_insert_with(|| PlanCamera::new(plan, start.origin));
                if response.dragged_by(egui::PointerButton::Secondary)
                    || response.dragged_by(egui::PointerButton::Middle)
                {
                    view.pan(response.drag_delta());
                }
                view.zoom(scroll);
                Projection::plan(plan, *view, area)
            }
        };
        let mut actions = Vec::new();
        let selection: BTreeSet<EntityIndex> = selected
            .into_iter()
            .chain(self.also.iter().copied())
            .collect();
        if response.has_focus() {
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
                && self.picking.take().is_none()
                && !self.tools.cancel()
            {
                self.also.clear();
                actions.push(MapAction::Select(None));
            }
            // TrenchBroom's Focus on Selection, Ctrl+U (Cmd+U on macOS, as Qt maps Ctrl).
            if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::U)) {
                self.frame = Some(Framing::Selection);
                ui.ctx().request_repaint();
            }
            actions.extend(arrange_keys(
                ui,
                &self.camera,
                &selection,
                self.tools.chosen(),
            ));
            actions.extend(self.select_keys(ui, pane, &selection, hidden));
            if let Some(grid) = grid_key(ui, self.tools.grid) {
                actions.push(MapAction::View(ViewSetting::Grid(grid)));
            }
            // The arrows move the selection, as TrenchBroom's do, not the keyboard focus, and
            // Escape cancels.
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        // Escape cancels in the view rather than taking its keyboard away.
                        escape: true,
                        ..egui::EventFilter::default()
                    },
                );
            });
            // The overview and group keys first: Cmd+Shift+D is Create Linked Duplicate before
            // Cmd+D can take it as Duplicate.
            let chosen: Vec<EntityIndex> = selection.iter().copied().collect();
            let world = self.world_brushes();
            let grouping = self.grouping(&chosen, &world);
            actions.extend(
                overview_keys(ui, &chosen, grouping)
                    .into_iter()
                    .map(MapAction::Group),
            );
            if pane == Pane::Perspective
                && let Some(camera) = self.camera.as_mut()
            {
                fly(ui, camera, self.camera_settings.fly_speed.factor());
                if let Some(entity) = selected {
                    actions.extend(selection_keys(ui, camera, entity, self.tools.grid));
                }
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
                actions.push(MapAction::Save);
            }
        }
        let ray = |at: Pos2| projection.ray(at);
        let pick = |at: Pos2| {
            let (start, direction) = ray(at);
            self.scene.pick(start, direction, hidden)
        };
        let selecting = self.tools.tool() == MapTool::Select;
        if !selecting {
            let found = self
                .tools
                .input(ui, &response, &self.scene, hidden, &projection, pane);
            actions.extend(found.into_iter().map(MapAction::Brush));
        }
        let alt = ui.input(|i| i.modifiers.alt);
        if selecting
            && response.clicked()
            && alt
            && let Some(at) = response.interact_pointer_pos()
        {
            let mode = if ui.input(|i| i.modifiers.shift) {
                PickMode::Toggle
            } else {
                PickMode::Replace
            };
            self.tools.pick_face(ray(at), &self.scene, hidden, mode);
        } else if selecting
            && response.clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
            self.tools.clear_faces();
            let picked = pick(at);
            let shift = ui.input(|i| i.modifiers.shift);
            if let Some(key) = self.picking.take() {
                if let (Some(from), Some(to)) = (selected, picked)
                    && from != to
                {
                    self.link_key = Some(key.clone());
                    actions.push(MapAction::Link(from, key, to));
                }
            } else if shift && selected.is_some() && picked != selected {
                if let Some(entity) = picked
                    && !self.also.remove(&entity)
                {
                    self.also.insert(entity);
                }
            } else {
                self.also.clear();
                actions.push(MapAction::Select(picked));
            }
        }
        if selecting
            && response.drag_started_by(egui::PointerButton::Primary)
            && alt
            && let Some(at) = response.interact_pointer_pos()
        {
            self.link_from = pick(at).filter(|entity| selection.contains(entity));
        }
        let pointer = ui
            .ctx()
            .pointer_latest_pos()
            .filter(|at| area.contains(*at));
        if response.drag_stopped()
            && let Some(from) = self.link_from.take()
            && let Some(to) = pointer.and_then(pick)
            && to != from
            && let Some(key) = link_key
        {
            actions.push(MapAction::Link(from, key, to));
        }
        let dropped_at = |at: Pos2| {
            let (start, direction) = ray(at);
            let snap = |c: f32| self.tools.grid.snap(f64::from(c));
            let distance = match pane {
                Pane::Perspective => PLACE_DISTANCE,
                // On the ground in the top view; on the line through the map's middle in the
                // others.
                Pane::Plan(_) => start.iter().map(|c| c.abs()).fold(0.0, f32::max),
            };
            [0, 1, 2].map(|axis| {
                let s = start.get(axis).copied().unwrap_or_default();
                let d = direction.get(axis).copied().unwrap_or_default();
                snap(s + d * distance)
            })
        };
        // egui's release takes whatever is dragged before it checks its kind, so each kind is
        // looked for first.
        if response.dnd_hover_payload::<ClassDrag>().is_some()
            && let Some(ClassDrag(classname)) =
                response.dnd_release_payload::<ClassDrag>().as_deref()
            && let Some(at) = pointer
        {
            actions.push(MapAction::Place(classname.clone(), dropped_at(at)));
        }
        if response.dnd_hover_payload::<PrefabDrag>().is_some()
            && let Some(PrefabDrag(path)) = response.dnd_release_payload::<PrefabDrag>().as_deref()
            && let Some(at) = pointer
        {
            actions.push(MapAction::PlacePrefab(path.clone(), dropped_at(at)));
        }
        // TrenchBroom's Copy, Paste and Paste at Original Position (Cmd+C, Cmd+V, Cmd+Alt+V),
        // which egui hands over as its copy and paste events while the view has the keyboard.
        if response.has_focus() {
            let (copy, pasted, original) = ui.input(|i| {
                let copy = i.events.iter().any(|e| matches!(e, egui::Event::Copy));
                let pasted = i.events.iter().find_map(|e| match e {
                    egui::Event::Paste(text) => Some(text.clone()),
                    _ => None,
                });
                (copy, pasted, i.modifiers.alt)
            });
            if copy {
                actions.push(MapAction::Clipboard(ClipboardAction::Copy(
                    self.tools.chosen().to_vec(),
                    selection.iter().copied().collect(),
                )));
            }
            if let Some(text) = pasted {
                let at = (!original).then(|| dropped_at(pointer.unwrap_or(area.center())));
                actions.push(MapAction::Clipboard(ClipboardAction::Paste(text, at)));
            }
        }
        let painter = ui.painter_at(area);
        painter.rect_filled(area, 0.0, theme.color(Token::SurfaceViewport));
        // P7-13: the textured or lit faces under everything the view draws after, in place of
        // the world's edges.
        let faces = pane == Pane::Perspective && self.shaded.draws_faces();
        if faces && let Some(camera) = &self.camera {
            self.shaded.paint(ui, area, camera);
        }
        let world = Stroke::new(1.0, theme.color(Token::TextFaint));
        for (owner, line) in &self.scene.lines {
            if !(faces && self.faced.contains(owner)) && !hidden.contains(owner) {
                projection.line(&painter, *line, world);
            }
        }
        if faces {
            for line in self.scene.outlined_lines(&self.outlined, hidden) {
                projection.line(&painter, *line, world);
            }
        }
        if pane == Pane::Perspective
            && let Some(note) = self.shaded.note()
        {
            painter.text(
                area.shrink(NOTE_MARGIN).left_top(),
                egui::Align2::LEFT_TOP,
                note.get(),
                egui::FontId::proportional(PANE_LABEL_SIZE),
                theme.color(Token::Text),
            );
        }
        let link = Stroke::new(1.0, theme.color(Token::Focus));
        let mut centres = Vec::new();
        for line in self.scene.links(self.links, &selection, hidden) {
            projection.line(&painter, line, link);
            if let Some(end) = projection.point(line[1]) {
                painter.circle_filled(end, LINK_END, link.color);
            }
        }
        // LIGHT-7.4: each light's reach in its colour, and a spotlight's cone.
        for light in self.scene.lights(self.lights, &selection, hidden) {
            // `_color` is the modder's sRGB colour; a black light is drawn muted to be seen.
            let [r, g, b] = light
                .colour
                .map(|c| egui::ecolor::linear_from_gamma(c.clamp(0.0, 1.0)));
            let colour = if light.colour.iter().all(|&c| c < VISIBLE_COLOUR) {
                theme.color(Token::TextMuted)
            } else {
                egui::Color32::from(egui::Rgba::from_rgb(r, g, b))
            };
            let stroke = Stroke::new(1.0, colour.gamma_multiply(LIGHT_ALPHA));
            for line in &light.lines {
                projection.line(&painter, *line, stroke);
            }
        }
        for shape in &self.scene.entities {
            if hidden.contains(&shape.index) {
                continue;
            }
            if let Some(centre) = projection.point(shape.centre()) {
                centres.push((shape.index, centre));
            }
            let chosen = selection.contains(&shape.index);
            let color = if chosen {
                theme.color(Token::Accent)
            } else {
                shape.color.map_or(theme.color(Token::TextMuted), rgb)
            };
            let stroke = Stroke::new(if chosen { 2.0 } else { 1.0 }, color);
            if !shape.brush {
                for line in box_lines(shape.bounds) {
                    projection.line(&painter, line, stroke);
                }
            }
            if let Some(lines) = shape.model.as_ref().and_then(|m| self.models.get(m)) {
                let (sin, cos) = shape.yaw.to_radians().sin_cos();
                let place = |p: [f32; 3]| {
                    [
                        shape.origin[0] + p[0] * cos - p[1] * sin,
                        shape.origin[1] + p[0] * sin + p[1] * cos,
                        shape.origin[2] + p[2],
                    ]
                };
                for line in lines {
                    projection.line(&painter, [place(line[0]), place(line[1])], stroke);
                }
            }
        }
        if let (Some(from), Some(at)) = (self.link_from, pointer)
            && let Some((_, start)) = centres.iter().find(|(entity, _)| *entity == from)
        {
            painter.line_segment([*start, at], Stroke::new(2.0, theme.color(Token::Focus)));
        }
        self.leaks
            .draw(&painter, &projection, theme, &self.scene, report);
        self.profiles.draw(&painter, &projection, theme);
        self.tools
            .draw(&painter, &projection, theme, &self.scene, pointer.map(ray));
        // A compass in the pane's lower left corner, as TrenchBroom's: the map's x, y and z axes
        // as the pane sees them, each with its letter.
        let (cx, cy) = (area.left() + COMPASS_INSET, area.bottom() - COMPASS_INSET);
        let centre = egui::pos2(cx, cy);
        for (axis, token, letter) in COMPASS {
            let way = projection.compass(axis);
            let (dx, dy) = (way.x * COMPASS_LENGTH, way.y * COMPASS_LENGTH);
            let colour = theme.color(token);
            let end = egui::pos2(cx + dx, cy + dy);
            painter.line_segment([centre, end], Stroke::new(2.0, colour));
            if dx.hypot(dy) > COMPASS_LABEL_LEAST {
                painter.text(
                    egui::pos2(cx + dx * COMPASS_LABEL_OUT, cy + dy * COMPASS_LABEL_OUT),
                    egui::Align2::CENTER_CENTER,
                    letter.get(),
                    egui::FontId::proportional(PANE_LABEL_SIZE),
                    colour,
                );
            }
        }
        if let Pane::Plan(_) = pane {
            // The soft map bounds as a guide, in the 2D views as TrenchBroom draws them.
            let bounds = Stroke::new(1.0, theme.color(Token::StatusWarn));
            for line in box_lines(SOFT_BOUNDS) {
                projection.line(&painter, line, bounds);
            }
            painter.text(
                egui::pos2(
                    area.left() + PANE_LABEL_INSET,
                    area.top() + PANE_LABEL_INSET,
                ),
                egui::Align2::LEFT_TOP,
                pane.text().get(),
                egui::FontId::proportional(PANE_LABEL_SIZE),
                theme.color(Token::TextMuted),
            );
            painter.rect_stroke(
                area,
                0.0,
                Stroke::new(1.0, theme.color(Token::Border)),
                egui::StrokeKind::Inside,
            );
        }
        if pane == Pane::Perspective {
            self.centres = centres;
        }
        actions
    }

    /// Selects these faces for the UV editor, as Alt-clicks would.
    #[cfg(test)]
    pub(crate) fn select_faces(&mut self, faces: &[FaceRef]) {
        for &face in faces {
            self.tools.select_face(face, &self.scene);
        }
    }

    /// The key editor of the selected entity, in the Properties panel; what it asked for.
    #[expect(
        clippy::too_many_lines,
        reason = "one panel, top to bottom: the notices, the keys, the spawnflags and a new key"
    )]
    pub fn properties(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        report: Option<&MapReport>,
    ) -> Vec<MapAction> {
        let mut actions = Vec::new();
        if let Some(refused) = &self.refused {
            ui.label(RichText::new(refused).color(theme.color(Token::StatusError)));
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(Text::MapCheckLeaks.get()).clicked() {
                actions.push(MapAction::CheckLeaks);
            }
            if ui
                .add_enabled(
                    self.camera.is_some(),
                    egui::Button::new(Text::MapPlayFromHere.get()),
                )
                .clicked()
            {
                actions.push(MapAction::PlayFromHere);
            }
        });
        // P7-08: where the 3D camera stands, which a value typed moves it to, as TrenchBroom's
        // Move Camera to... but always shown.
        if let Some(camera) = &mut self.camera {
            ui.horizontal_wrapped(|ui| {
                ui.label(Text::MapCamera.get());
                for (axis, name) in camera.origin.iter_mut().zip(CAMERA_AXES) {
                    let field = ui.add(egui::DragValue::new(axis).speed(1.0).max_decimals(1));
                    name_field(ui, &field, name);
                    field.on_hover_text(Text::MapCameraHelp.get());
                }
                // TrenchBroom's Reset camera zoom is Ctrl+Alt+Z, which is undo here (Cmd+Z with
                // an extra Alt), so the zoom is put back from here.
                if self.zoom != ViewZoom::default() && ui.button(Text::MapResetZoom.get()).clicked()
                {
                    self.zoom = ViewZoom::default();
                }
            });
        }
        actions.extend(self.camera_settings_ui(ui));
        actions.extend(self.review.properties(ui, theme));
        actions.extend(self.profiles.controls(ui, theme));
        self.leaks.properties(ui, theme, report);
        if self.disk != DiskNotice::None {
            ui.label(
                RichText::new(Text::MapChangedOnDisk.get()).color(theme.color(Token::StatusWarn)),
            );
            ui.horizontal_wrapped(|ui| {
                if ui.button(Text::MapReloadFromDisk.get()).clicked() {
                    actions.push(MapAction::Resolve(Keep::Disk));
                }
                if ui.button(Text::MapKeepMine.get()).clicked() {
                    actions.push(MapAction::Resolve(Keep::Mine));
                }
                if ui.button(Text::MapCompare.get()).clicked() {
                    actions.push(MapAction::Compare);
                }
            });
            if let DiskNotice::Compared(lines) = &self.disk {
                for line in lines {
                    ui.label(RichText::new(line).color(theme.color(Token::TextMuted)));
                }
            }
            ui.separator();
        }
        let texture = self.textures.selected();
        let textures = &self.textures;
        let size_of = |name: &str| textures.size(name);
        let mut chosen: Vec<EntityIndex> =
            self.keys.as_ref().map(|(e, _)| *e).into_iter().collect();
        chosen.extend(self.also.iter().copied());
        actions.extend(
            self.tools
                .properties(ui, &self.scene, texture, &size_of, &chosen)
                .into_iter()
                .map(MapAction::Brush),
        );
        let world = self.world_brushes();
        let grouping = self.grouping(&chosen, &world);
        actions.extend(
            selection_ui(ui, &chosen, grouping)
                .into_iter()
                .map(MapAction::Group),
        );
        actions.extend(
            layers_ui(ui, theme, &self.layers)
                .into_iter()
                .map(MapAction::Group),
        );
        let Some((entity, keys)) = &self.keys else {
            ui.label(
                RichText::new(Text::MapNoSelection.get()).color(theme.color(Token::TextMuted)),
            );
            return actions;
        };
        let entity = *entity;
        let also: Vec<EntityIndex> = self.also.iter().copied().collect();
        ui.label(RichText::new(keys.classname.clone().unwrap_or_default()).strong());
        if !also.is_empty() {
            ui.label(RichText::new(also_selected(also.len())).color(theme.color(Token::TextMuted)));
        }
        egui::Grid::new(ui.id().with(entity))
            .num_columns(KEY_COLUMNS)
            .striped(true)
            .show(ui, |ui| {
                for (key, value) in &keys.pairs {
                    ui.label(RichText::new(key).color(theme.color(Token::TextMuted)));
                    let typed = self
                        .typing
                        .entry(key.clone())
                        .or_insert_with(|| value.clone());
                    let ty = keys
                        .keys
                        .iter()
                        .find(|k| k.name == *key)
                        .map(|k| k.ty.as_str());
                    let field = if self.targeting.contains(key) {
                        Field::Target
                    } else {
                        Field::of(key, ty, self.scene.values.get(key))
                    };
                    ui.horizontal(|ui| {
                        if let Some(set) = field.ui(ui, key, typed, value) {
                            let set = MapAction::SetKey(entity, also.clone(), key.clone(), set);
                            actions.push(set);
                        }
                        if field == Field::Target {
                            let picking = self.picking.as_ref() == Some(key);
                            if ui
                                .selectable_label(picking, Text::MapPick.get())
                                .on_hover_text(Text::MapPickHint.get())
                                .clicked()
                            {
                                self.picking = (!picking).then(|| key.clone());
                            }
                        }
                    });
                    if ui
                        .small_button(Text::MapRemoveKey.get())
                        .on_hover_text(Text::MapRemoveKey.get())
                        .clicked()
                    {
                        actions.push(MapAction::RemoveKey(entity, key.clone()));
                    }
                    ui.end_row();
                }
            });
        if !keys.spawnflags.is_empty() {
            let value = keys
                .pairs
                .iter()
                .rev()
                .find(|(k, _)| k == SPAWNFLAGS)
                .map(|(_, v)| v.as_str())
                .unwrap_or_default();
            if let Some((flag, change)) = spawnflags(ui, &keys.spawnflags, value) {
                actions.push(MapAction::SetFlag(entity, also.clone(), flag, change));
            }
        }
        let unset: Vec<_> = keys
            .keys
            .iter()
            .filter(|key| !keys.pairs.iter().any(|(k, _)| *k == key.name))
            .collect();
        if !unset.is_empty() {
            ui.label(RichText::new(Text::MapClassKeys.get()).color(theme.color(Token::TextMuted)));
            ui.horizontal_wrapped(|ui| {
                for key in unset {
                    let hint = format!("{} ({})", key.name, key.ty);
                    let button = ui.small_button(hint);
                    let button = match &key.default {
                        Some(default) => button.on_hover_text(default),
                        None => button,
                    };
                    if button.clicked() {
                        self.adding = (key.name.clone(), key.default.clone().unwrap_or_default());
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            let key = ui.add(
                egui::TextEdit::singleline(&mut self.adding.0)
                    .hint_text(Text::MapKey.get())
                    .desired_width(NEW_PAIR_WIDTH),
            );
            name_field(ui, &key, Text::MapKey);
            let value = ui.add(
                egui::TextEdit::singleline(&mut self.adding.1)
                    .hint_text(Text::MapValue.get())
                    .desired_width(NEW_PAIR_WIDTH),
            );
            name_field(ui, &value, Text::MapValue);
            let targets = !keys.targetnames.is_empty() && self.targeting.contains(&self.adding.0);
            if targets {
                let picker = egui::ComboBox::from_id_salt(ui.id().with((entity, 1)))
                    .selected_text(Text::MapTargets.get())
                    .show_ui(ui, |ui| {
                        for name in &keys.targetnames {
                            if ui.selectable_label(false, name).clicked() {
                                self.adding.1.clone_from(name);
                            }
                        }
                    });
                name_field(ui, &picker.response, Text::MapTargets);
            }
            let ready = !self.adding.0.trim().is_empty();
            if ui
                .add_enabled(ready, egui::Button::new(Text::MapAddKey.get()))
                .clicked()
            {
                let (key, value) = core::mem::take(&mut self.adding);
                actions.push(MapAction::SetKey(
                    entity,
                    also,
                    key.trim().to_owned(),
                    value,
                ));
            }
        });
        actions
    }
}

/// Moves the camera while a fly key is held (TrenchBroom's `CameraFly*`).
fn fly(ui: &Ui, camera: &mut Camera, speed: f32) {
    let (dt, keys) = ui.input(|i| {
        let held = |key| if i.key_down(key) { 1.0 } else { 0.0 };
        let forward = held(Key::W) - held(Key::S);
        let right = held(Key::D) - held(Key::A);
        let up = held(Key::Q) - held(Key::X);
        (i.stable_dt, [forward, right, up])
    });
    if keys.iter().any(|k: &f32| *k != 0.0) {
        let step = FLY_SPEED * speed * dt;
        camera.travel(keys[0] * step, keys[1] * step, keys[2] * step);
        ui.ctx().request_repaint();
    }
}

/// The soft map bounds drawn as a guide in the 2D views (P7-08): TrenchBroom's for Quake, its
/// `games/Quake/GameConfig.cfg`'s `"softMapBounds":"-4096 -4096 -4096 4096 4096 4096"`, where
/// players, items and other entities can be.
const SOFT_BOUNDS: [[f32; 3]; 2] = [[-4096.0; 3], [4096.0; 3]];

/// How far ahead an orbit's centre is when nothing is under the pointer: TrenchBroom's
/// `Camera::DefaultPointDistance`, 256 units.
const ORBIT_DISTANCE: f32 = 256.0;

/// The compass's axes: the map's x, y and z, in red, green and blue as TrenchBroom draws them,
/// from the theme's error, ok and focus colours, each with its letter.
const COMPASS: [([f32; 3], Token, Text); 3] = [
    ([1.0, 0.0, 0.0], Token::StatusError, Text::CompassX),
    ([0.0, 1.0, 0.0], Token::StatusOk, Text::CompassY),
    ([0.0, 0.0, 1.0], Token::Focus, Text::CompassZ),
];
/// How far the compass's centre is from the pane's lower left corner, in points.
const COMPASS_INSET: f32 = 36.0;
/// How long an axis lying across the pane is drawn, in points.
const COMPASS_LENGTH: f32 = 22.0;
/// How far past an axis's end its letter is, as a share of its length.
const COMPASS_LABEL_OUT: f32 = 1.35;
/// The shortest an axis is drawn, in points, and still has its letter: one pointing at the
/// viewer has none, so its letter does not cover the others.
const COMPASS_LABEL_LEAST: f32 = 4.0;

/// The camera position fields' names, x, y and z.
const CAMERA_AXES: [Text; 3] = [Text::MapCameraX, Text::MapCameraY, Text::MapCameraZ];

impl MapEditor {
    /// The camera settings under the camera's place, folded at first: the field of view, the
    /// speeds and the directions, each change a `view.map_camera` (P7-08).
    fn camera_settings_ui(&self, ui: &mut Ui) -> Option<MapAction> {
        let now = self.camera_settings;
        let mut asked = None;
        egui::CollapsingHeader::new(Text::MapCameraSettings.get())
            .id_salt(core::any::TypeId::of::<MapCamera>())
            .show(ui, |ui| {
                let mut fov = now.fov.degrees();
                let slider = egui::Slider::new(&mut fov, MapFov::MIN..=MapFov::MAX)
                    .text(Text::MapCameraFov.get());
                if ui.add(slider).changed()
                    && let Ok(fov) = MapFov::try_from(fov)
                {
                    asked = Some(MapCameraParams {
                        fov: Some(fov),
                        ..MapCameraParams::default()
                    });
                }
                for (speed, text, set) in [
                    (now.look_speed, Text::MapCameraLookSpeed, SpeedOf::Look),
                    (now.pan_speed, Text::MapCameraPanSpeed, SpeedOf::Pan),
                    (now.fly_speed, Text::MapCameraFlySpeed, SpeedOf::Fly),
                ] {
                    let mut factor = speed.factor();
                    let slider =
                        egui::Slider::new(&mut factor, CameraSpeed::MIN..=CameraSpeed::MAX)
                            .logarithmic(true)
                            .text(text.get());
                    if ui.add(slider).changed()
                        && let Ok(speed) = CameraSpeed::try_from(factor)
                    {
                        asked = Some(set.params(speed));
                    }
                }
                for (direction, text, wheel) in [
                    (now.look_vertical, Text::MapCameraInvertLook, false),
                    (now.wheel, Text::MapCameraInvertWheel, true),
                ] {
                    let mut inverted = direction == Direction::Inverted;
                    if ui.checkbox(&mut inverted, text.get()).changed() {
                        let direction = if inverted {
                            Direction::Inverted
                        } else {
                            Direction::Normal
                        };
                        asked = Some(if wheel {
                            MapCameraParams {
                                wheel: Some(direction),
                                ..MapCameraParams::default()
                            }
                        } else {
                            MapCameraParams {
                                look_vertical: Some(direction),
                                ..MapCameraParams::default()
                            }
                        });
                    }
                }
            });
        asked.map(|params| MapAction::View(ViewSetting::Camera(params)))
    }
}

/// A copy of the selection to the clipboard, or a paste of map text (P7-08).
#[derive(Clone, Debug, PartialEq)]
pub enum ClipboardAction {
    /// Copy these brushes and entities as map text.
    Copy(Vec<(EntityIndex, BrushIndex)>, Vec<EntityIndex>),
    /// Paste the text with the middle of its floor at the point, or where it was.
    Paste(String, Option<[f64; 3]>),
}

/// A change of how the map view looks, a `view.*` command of the layout.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewSetting {
    /// Split the map view into other panes (P6-09).
    Panes(MapPanes),
    /// Draw the 3D pane as lines, textured or lit (P7-13).
    Shading(MapShading),
    /// Snap to this grid (P7-08).
    Grid(MapGrid),
    /// Change the camera settings (P7-08).
    Camera(MapCameraParams),
}

/// Which of the camera's speeds a slider sets.
#[derive(Clone, Copy, Debug)]
enum SpeedOf {
    /// A right-drag's turn.
    Look,
    /// A middle-drag's pan.
    Pan,
    /// The keys' flight.
    Fly,
}

impl SpeedOf {
    /// `view.map_camera`'s parameters setting this speed to `speed`.
    fn params(self, speed: CameraSpeed) -> MapCameraParams {
        let mut params = MapCameraParams::default();
        match self {
            Self::Look => params.look_speed = Some(speed),
            Self::Pan => params.pan_speed = Some(speed),
            Self::Fly => params.fly_speed = Some(speed),
        }
        params
    }
}

/// How far Shift and the wheel have zoomed the 3D view in: a factor its field of view is
/// divided by, 1 at first and again after Reset zoom (P7-08).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ViewZoom(f32);

impl ViewZoom {
    /// The farthest in, eight times.
    const MOST: f32 = 8.0;
    /// The farthest out, the field of view as set.
    const LEAST: f32 = 1.0;
    /// How much a point of scroll zooms, as a power of e per point.
    const RATE: f32 = 0.005;

    /// Zoomed by `scroll` points: in for a positive one, held within 1 to 8 times.
    fn scrolled(self, scroll: f32) -> Self {
        Self((self.0 * (scroll * Self::RATE).exp()).clamp(Self::LEAST, Self::MOST))
    }

    /// The field of view across, `fov` narrowed by the zoom.
    fn fov(self, fov: MapFov) -> Degrees {
        Degrees::new(fov.degrees() / self.0)
    }
}

impl Default for ViewZoom {
    fn default() -> Self {
        Self(Self::LEAST)
    }
}

/// What the view frames on its next frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Framing {
    /// An entity chosen in the outline.
    Entity(EntityIndex),
    /// Every selected entity and brush (P7-08).
    Selection,
}

impl MapEditor {
    /// How many layers the map has.
    pub(crate) fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// The world's brushes selected, which a new group takes (P7-08).
    fn world_brushes(&self) -> Vec<BrushIndex> {
        self.tools
            .chosen()
            .iter()
            .filter(|(entity, _)| entity.is_world())
            .map(|&(_, brush)| brush)
            .collect()
    }

    /// What the selection can be grouped from, and the group it is when the one entity whose
    /// keys are shown is a group's own entity (MAP-9.1).
    fn grouping<'a>(&self, entities: &'a [EntityIndex], brushes: &'a [BrushIndex]) -> Grouping<'a> {
        let group = self
            .keys
            .as_ref()
            .and_then(|(entity, keys)| keys.is_group().then_some(*entity));
        Grouping {
            entities,
            brushes,
            group,
        }
    }

    /// TrenchBroom's selection keys (P7-08): Cmd+A selects everything, Cmd+Alt+A inverts the
    /// selection and Cmd+Shift+A clears it; Cmd+T, Cmd+E and Cmd+Shift+E select what the
    /// selected brushes touch, hold, or hold stretched along the view's axis, and delete them,
    /// as TrenchBroom's Select Touching, Inside and Tall do.
    fn select_keys(
        &mut self,
        ui: &Ui,
        pane: Pane,
        selection: &BTreeSet<EntityIndex>,
        hidden: &BTreeSet<EntityIndex>,
    ) -> Vec<MapAction> {
        let tall = match pane {
            Pane::Plan(plan) => plan.axis(),
            Pane::Perspective => Z_AXIS,
        };
        let asked = ui.input_mut(|i| {
            let mut key = |modifiers, key| i.consume_key(modifiers, key);
            if key(Modifiers::COMMAND | Modifiers::ALT, Key::A) {
                Some(SelectKey::Invert)
            } else if key(Modifiers::COMMAND | Modifiers::SHIFT, Key::A) {
                Some(SelectKey::Clear)
            } else if key(Modifiers::COMMAND, Key::A) {
                Some(SelectKey::All)
            } else if key(Modifiers::COMMAND | Modifiers::SHIFT, Key::E) {
                Some(SelectKey::By(Region::Tall(tall)))
            } else if key(Modifiers::COMMAND, Key::E) {
                Some(SelectKey::By(Region::Inside))
            } else if key(Modifiers::COMMAND, Key::T) {
                Some(SelectKey::By(Region::Touching))
            } else {
                None
            }
        });
        let Some(asked) = asked else {
            return Vec::new();
        };
        let now = Picked {
            entities: selection.iter().copied().collect(),
            brushes: self.tools.chosen().to_vec(),
        };
        let mut actions = Vec::new();
        let picked = match asked {
            SelectKey::All => self.scene.all(hidden),
            SelectKey::Invert => self.scene.inverted(&now, hidden),
            SelectKey::Clear => Picked::default(),
            SelectKey::By(region) => {
                let mut picked = self.scene.picked_by(&now.brushes, region, hidden);
                // The selecting brushes go, the last of each entity first so the rest keep
                // their places; the brushes picked after one in its entity move down a place.
                let mut gone = now.brushes.clone();
                gone.sort();
                for &(entity, brush) in gone.iter().rev() {
                    actions.push(MapAction::Brush(BrushAction::Delete(entity, brush)));
                }
                for (entity, brush) in &mut picked.brushes {
                    let before = gone
                        .iter()
                        .filter(|&&(e, b)| e == *entity && b < *brush)
                        .count();
                    *brush = BrushIndex::new(brush.get().saturating_sub(before));
                }
                picked
            }
        };
        let mut entities = picked.entities.into_iter();
        let first = entities.next();
        self.also = entities.collect();
        actions.insert(0, MapAction::Select(first));
        self.tools.choose(picked.brushes, &self.scene);
        actions
    }
}

/// A selection key's command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectKey {
    /// Select All.
    All,
    /// Invert Selection.
    Invert,
    /// Deselect All.
    Clear,
    /// Select what the selected brushes pick.
    By(Region),
}

/// The vertical, 2: what Select Tall stretches along in the 3D view, as the top view sees it.
const Z_AXIS: usize = 2;

/// A flip or turn of the selection: its whole entities and its brushes (P7-08).
#[derive(Clone, Debug, PartialEq)]
pub enum Arrange {
    /// Mirror along an axis (`map.flip`).
    Flip(Vec<EntityIndex>, Vec<(EntityIndex, BrushIndex)>, Axis),
    /// Turn by degrees about the vertical (`map.turn`).
    Turn(Vec<EntityIndex>, Vec<(EntityIndex, BrushIndex)>, f64),
}

/// TrenchBroom's keys that flip and turn the selection: Cmd+F (its Ctrl+F) flips it across the
/// view, along the map axis nearest the camera's right, Cmd+Alt+F flips it up and down, and
/// Alt with Left or Right turns it a right angle about the vertical.
fn arrange_keys(
    ui: &Ui,
    camera: &Option<Camera>,
    entities: &BTreeSet<EntityIndex>,
    brushes: &[(EntityIndex, BrushIndex)],
) -> Vec<MapAction> {
    if entities.is_empty() && brushes.is_empty() {
        return Vec::new();
    }
    let across = camera.map_or(Axis::X, |camera| {
        let [_, right, _] = camera.axes();
        if right[0].abs() >= right[1].abs() {
            Axis::X
        } else {
            Axis::Y
        }
    });
    let mut asked = Vec::new();
    ui.input_mut(|i| {
        if i.consume_key(Modifiers::COMMAND | Modifiers::ALT, Key::F) {
            asked.push(Arrange::Flip(Vec::new(), Vec::new(), Axis::Z));
        }
        if i.consume_key(Modifiers::COMMAND, Key::F) {
            asked.push(Arrange::Flip(Vec::new(), Vec::new(), across));
        }
        if i.consume_key(Modifiers::ALT, Key::ArrowLeft) {
            asked.push(Arrange::Turn(Vec::new(), Vec::new(), -QUARTER_TURN));
        }
        if i.consume_key(Modifiers::ALT, Key::ArrowRight) {
            asked.push(Arrange::Turn(Vec::new(), Vec::new(), QUARTER_TURN));
        }
    });
    let whole: Vec<EntityIndex> = entities.iter().copied().collect();
    asked
        .into_iter()
        .map(|arrange| {
            MapAction::Arrange(match arrange {
                Arrange::Flip(_, _, axis) => Arrange::Flip(whole.clone(), brushes.to_vec(), axis),
                Arrange::Turn(_, _, degrees) => {
                    Arrange::Turn(whole.clone(), brushes.to_vec(), degrees)
                }
            })
        })
        .collect()
}

/// TrenchBroom's grid keys (its "Increase Grid Size", "Decrease Grid Size" and "Set Grid Size"
/// actions): + doubles `grid` and - halves it; 1 to 9 set 1 to 256 units.
fn grid_key(ui: &Ui, grid: MapGrid) -> Option<MapGrid> {
    const DIGITS: [Key; 9] = [
        Key::Num1,
        Key::Num2,
        Key::Num3,
        Key::Num4,
        Key::Num5,
        Key::Num6,
        Key::Num7,
        Key::Num8,
        Key::Num9,
    ];
    ui.input_mut(|i| {
        if i.consume_key(Modifiers::NONE, Key::Plus) || i.consume_key(Modifiers::NONE, Key::Equals)
        {
            return Some(grid.coarser());
        }
        if i.consume_key(Modifiers::NONE, Key::Minus) {
            return Some(grid.finer());
        }
        let digit = DIGITS
            .iter()
            .position(|&key| i.consume_key(Modifiers::NONE, key))?;
        let power = u8::try_from(digit).ok()?;
        MapGrid::try_from(f64::from(power).exp2()).ok()
    })
}

/// TrenchBroom's keys on the selection: the arrows and Page Up and Down move it by the grid,
/// along the map axis nearest the camera's; Alt with Left or Right turns it; Delete deletes it;
/// Ctrl+D (Cmd+D) duplicates it.
fn selection_keys(ui: &Ui, camera: &Camera, entity: EntityIndex, grid: MapGrid) -> Vec<MapAction> {
    let step = grid.units();
    let [forward, right, _] = camera.axes();
    let snap = |v: [f32; 3]| -> [f64; 3] {
        if v[0].abs() >= v[1].abs() {
            [f64::from(v[0].signum()) * step, 0.0, 0.0]
        } else {
            [0.0, f64::from(v[1].signum()) * step, 0.0]
        }
    };
    let negate = |v: [f64; 3]| v.map(|c| -c);
    let mut actions = Vec::new();
    ui.input_mut(|i| {
        let moves = [
            (Key::ArrowUp, snap(forward)),
            (Key::ArrowDown, negate(snap(forward))),
            (Key::ArrowRight, snap(right)),
            (Key::ArrowLeft, negate(snap(right))),
            (Key::PageUp, [0.0, 0.0, step]),
            (Key::PageDown, [0.0, 0.0, -step]),
        ];
        for (key, by) in moves {
            if i.consume_key(Modifiers::NONE, key) {
                actions.push(MapAction::Move(entity, by));
            }
        }
        if i.consume_key(Modifiers::NONE, Key::Delete)
            || i.consume_key(Modifiers::NONE, Key::Backspace)
        {
            actions.push(MapAction::Delete(entity));
        }
        if i.consume_key(Modifiers::COMMAND, Key::D) {
            actions.push(MapAction::Duplicate(entity));
        }
    });
    actions
}

/// The twelve edges of a box.
pub(crate) fn box_lines([min, max]: [[f32; 3]; 2]) -> [Line; 12] {
    let corner = |x: bool, y: bool, z: bool| {
        [
            if x { max[0] } else { min[0] },
            if y { max[1] } else { min[1] },
            if z { max[2] } else { min[2] },
        ]
    };
    let edge = |a: (bool, bool, bool), b: (bool, bool, bool)| {
        [corner(a.0, a.1, a.2), corner(b.0, b.1, b.2)]
    };
    let (f, t) = (false, true);
    [
        edge((f, f, f), (t, f, f)),
        edge((f, t, f), (t, t, f)),
        edge((f, f, t), (t, f, t)),
        edge((f, t, t), (t, t, t)),
        edge((f, f, f), (f, t, f)),
        edge((t, f, f), (t, t, f)),
        edge((f, f, t), (f, t, t)),
        edge((t, f, t), (t, t, t)),
        edge((f, f, f), (f, f, t)),
        edge((t, f, f), (t, f, t)),
        edge((f, t, f), (f, t, t)),
        edge((t, t, f), (t, t, t)),
    ]
}

/// A class's colour as the painter takes it.
pub(crate) fn rgb(color: [f32; 3]) -> Color32 {
    let channel = |c: f32| {
        let scaled = (c.clamp(0.0, 1.0) * f32::from(u8::MAX)).round();
        u8::try_from(scaled_to_int(scaled)).unwrap_or(u8::MAX)
    };
    Color32::from_rgb(channel(color[0]), channel(color[1]), channel(color[2]))
}

/// A float already rounded into 0 to 255 as an integer.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the value is rounded and clamped to 0 to 255 first"
)]
const fn scaled_to_int(value: f32) -> i32 {
    value as i32
}
