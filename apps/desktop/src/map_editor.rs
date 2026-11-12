// SPDX-License-Identifier: GPL-3.0-or-later

//! The map editor (P5-06, MAP-4, MAP-20, P5-13, MAP-27): the open map in 3D as lines, its
//! entities as boxes and model wireframes, picked with the mouse and edited with TrenchBroom's
//! keys, with the links between them and a filter by skill and game; the entity browser below
//! it, dragged from to place; and the key editor in the Properties panel, with fields chosen
//! from the QuakeC, an eyedropper to link without typing, and editing of several entities at
//! once. Every edit is a [`MapAction`] the shell runs as a `map.*` command, so each has undo.
//! `docs/map-editor.md` maps TrenchBroom's bindings to these.

use std::collections::{BTreeSet, HashMap};

use eframe::egui::{
    self, Color32, Key, Modifiers, Pos2, Rect, RichText, Sense, Stroke, Ui, WidgetInfo, WidgetType,
};
use qs_layout::MapPanes;
use qs_render::Camera;
use qs_workbench::entity_classes::ClassInfo;
use qs_workbench::map_edit::{
    EntityIndex, EntityKeys, FlagChange, GameMode, Keep, LayerInfo, MapDocument, MapRevision,
    OutlineRow, Skill,
};
use qs_workbench::map_textures::TextureSources;

use crate::map_browser::{ClassDrag, EntityBrowser};
use crate::map_fields::{Field, spawnflags};
use crate::map_groups::{GroupAction, layers_ui, overview_keys, selection_ui};
use crate::map_outline::{OutlineAction, OutlineView};
use crate::map_panes::{Pane, Plan, PlanCamera, Projection, panes};
use crate::map_scene::{ClassLook, Line, LinkView, SPAWNFLAGS, Scene, load_models, start_camera};
use crate::map_tools::{BrushAction, BrushTools, MapTool};
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
pub(crate) const GRID: f64 = 16.0;
/// TrenchBroom turns a selection by a right angle (`half_pi`).
const QUARTER_TURN: f64 = 90.0;
/// The key editor's columns: the key, its value and its buttons.
const KEY_COLUMNS: usize = 3;
/// The height of the entity browser below the view, in points (the mockup's texture
/// browser), at most half the space.
const BROWSER_HEIGHT: f32 = 188.0;
/// How far in front of the camera a class dropped into the view is placed, in map units.
const PLACE_DISTANCE: f32 = 256.0;
/// The radius of the dot at the end of a link that is fired, in points.
const LINK_END: f32 = 3.0;
/// The width of the new key's and new value's fields, in points.
const NEW_PAIR_WIDTH: f32 = 90.0;
/// The map units the camera stands back from a framed box beyond that, so a small entity is not
/// right against the eye.
const FRAME_MARGIN: f32 = 64.0;
/// The map's origin, where the camera stands before a map gives it a place.
const MAP_ORIGIN: [f32; 3] = [0.0; 3];
/// How far a 2D view's name is from its corner, in points.
const PANE_LABEL_INSET: f32 = 6.0;
/// The size of a 2D view's name, in points.
const PANE_LABEL_SIZE: f32 = 12.0;

/// An edit or a selection the map editor asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum MapAction {
    /// Select an entity, or clear the selection.
    Select(Option<EntityIndex>),
    /// Move an entity by map units.
    Move(EntityIndex, [f64; 3]),
    /// Turn an entity by degrees about the vertical.
    Rotate(EntityIndex, f64),
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
    /// Split the map view into other panes (P6-09).
    Panes(MapPanes),
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
    /// The document's path and edits the scene was built from.
    built: Option<(std::path::PathBuf, MapRevision)>,
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
    /// The entity to frame in the view on its next frame, chosen in the outline.
    frame: Option<EntityIndex>,
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
}

/// The browsers of the tab below the map view, as the mockup has them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BottomTab {
    /// The texture browser (P6-10).
    #[default]
    Textures,
    /// The entity browser (P5-13).
    Entities,
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
                    self.frame = Some(entity);
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

    /// Takes the open map's texture sources, read again when the map opens or a WAD changes.
    pub fn set_texture_sources(&mut self, sources: TextureSources) {
        self.textures.set_sources(sources);
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

    /// Forgets the camera, so the next map opened starts at its player start.
    pub fn reset_camera(&mut self) {
        self.camera = None;
    }

    /// Draws the view's toolbar, `document` in the view and the entity browser below it, in
    /// all the space `ui` has left; what they asked for.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        document: &MapDocument,
        layout: MapPanes,
    ) -> Vec<MapAction> {
        let mut actions = self.toolbar(ui, layout);
        let full = ui.available_rect_before_wrap();
        let height = BROWSER_HEIGHT.min(full.height() / 2.0);
        let (view, below) = full.split_top_bottom_at_y(full.bottom() - height);
        let mut view_ui = ui.new_child(egui::UiBuilder::new().max_rect(view));
        actions.extend(self.view(&mut view_ui, theme, document, layout));
        let mut browser_ui = ui.new_child(egui::UiBuilder::new().max_rect(below));
        browser_ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.bottom,
                BottomTab::Textures,
                Text::MapTextures.get(),
            );
            ui.selectable_value(&mut self.bottom, BottomTab::Entities, Text::Entities.get());
            ui.separator();
            match self.bottom {
                BottomTab::Textures => self.textures.header(ui, theme, document),
                BottomTab::Entities => self.browser.header(ui, theme, &self.names),
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
        }
        ui.advance_cursor_after_rect(full);
        actions
    }

    /// The links shown and the filter by skill and game; the filter chosen.
    fn toolbar(&mut self, ui: &mut Ui, layout: MapPanes) -> Vec<MapAction> {
        let mut actions = Vec::new();
        ui.horizontal(|ui| {
            self.tools.toolbar(ui);
            ui.separator();
            egui::ComboBox::from_id_salt(ui.id().with(core::any::TypeId::of::<MapPanes>()))
                .selected_text(panes_text(layout).get())
                .show_ui(ui, |ui| {
                    for choice in MapPanes::ALL {
                        if ui
                            .selectable_label(layout == choice, panes_text(choice).get())
                            .clicked()
                        {
                            actions.push(MapAction::Panes(choice));
                        }
                    }
                });
            ui.separator();
            ui.label(Text::MapLinks.get());
            ui.selectable_value(
                &mut self.links,
                LinkView::Selection,
                Text::MapLinksSelected.get(),
            );
            ui.selectable_value(&mut self.links, LinkView::All, Text::MapLinksAll.get());
            ui.separator();
            ui.label(Text::MapShow.get());
            egui::ComboBox::from_id_salt(ui.id().with(core::any::TypeId::of::<ShowFilter>()))
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
                });
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
        layout: MapPanes,
    ) -> Vec<MapAction> {
        let built = (document.path().to_path_buf(), document.edits());
        if self.built.as_ref() != Some(&built) {
            self.scene = Scene::build(document, &self.classes, &self.targeting);
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
        let camera = self.camera.get_or_insert_with(|| start_camera(document));
        let rect = ui.available_rect_before_wrap();
        if let Some(entity) = self.frame.take()
            && let Some(shape) = self.scene.entities.iter().find(|s| s.index == entity)
        {
            let aspect = rect.width() / rect.height().max(1.0);
            camera.frame(shape.bounds, aspect, FRAME_MARGIN);
        }
        let selected = document.selected();
        if selected.is_none() {
            self.also.clear();
        }
        let mut actions = Vec::new();
        for (pane, area) in panes(layout, rect) {
            actions.extend(self.pane(ui, theme, pane, area, &hidden, selected));
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
        hidden: &BTreeSet<EntityIndex>,
        selected: Option<EntityIndex>,
    ) -> Vec<MapAction> {
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
        let start = self.camera.unwrap_or_else(|| Camera::new(MAP_ORIGIN, 0.0));
        let projection = match pane {
            Pane::Perspective => {
                let camera = self.camera.get_or_insert(start);
                if response.dragged_by(egui::PointerButton::Secondary) {
                    let drag = response.drag_delta();
                    camera.turn(-drag.x * TURN, -drag.y * TURN);
                }
                if response.dragged_by(egui::PointerButton::Middle) {
                    let drag = response.drag_delta();
                    camera.travel(0.0, -drag.x * PAN, drag.y * PAN);
                }
                camera.travel(scroll * TRAVEL, 0.0, 0.0);
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
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                self.picking = None;
            }
            // The arrows move the selection, as TrenchBroom's do, not the keyboard focus.
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        ..egui::EventFilter::default()
                    },
                );
            });
            if pane == Pane::Perspective
                && let Some(camera) = self.camera.as_mut()
            {
                fly(ui, camera);
                if let Some(entity) = selected {
                    actions.extend(selection_keys(ui, camera, entity));
                }
            }
            let chosen: Vec<EntityIndex> = selection.iter().copied().collect();
            actions.extend(overview_keys(ui, &chosen).into_iter().map(MapAction::Group));
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
                .input(ui, &response, &self.scene, hidden, ray, &projection);
            actions.extend(found.into_iter().map(MapAction::Brush));
        }
        if selecting
            && response.clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
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
        let alt = ui.input(|i| i.modifiers.alt);
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
        if let Some(ClassDrag(classname)) = response.dnd_release_payload::<ClassDrag>().as_deref()
            && let Some(at) = pointer
        {
            let (start, direction) = ray(at);
            let snap = |c: f32| (f64::from(c) / GRID).round() * GRID;
            let distance = match pane {
                Pane::Perspective => PLACE_DISTANCE,
                // On the ground in the top view; on the line through the map's middle in the
                // others.
                Pane::Plan(_) => start.iter().map(|c| c.abs()).fold(0.0, f32::max),
            };
            let origin = [0, 1, 2].map(|axis| {
                let s = start.get(axis).copied().unwrap_or_default();
                let d = direction.get(axis).copied().unwrap_or_default();
                snap(s + d * distance)
            });
            actions.push(MapAction::Place(classname.clone(), origin));
        }
        let painter = ui.painter_at(area);
        painter.rect_filled(area, 0.0, theme.color(Token::SurfaceViewport));
        let world = Stroke::new(1.0, theme.color(Token::TextFaint));
        for (owner, line) in &self.scene.lines {
            if !hidden.contains(owner) {
                projection.line(&painter, *line, world);
            }
        }
        let link = Stroke::new(1.0, theme.color(Token::Focus));
        let mut centres = Vec::new();
        for line in self.scene.links(self.links, &selection, hidden) {
            projection.line(&painter, line, link);
            if let Some(end) = projection.point(line[1]) {
                painter.circle_filled(end, LINK_END, link.color);
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
        self.tools
            .draw(&painter, &projection, theme, &self.scene, pointer.map(ray));
        if let Pane::Plan(_) = pane {
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

    /// The key editor of the selected entity, in the Properties panel; what it asked for.
    #[expect(
        clippy::too_many_lines,
        reason = "one panel, top to bottom: the notices, the keys, the spawnflags and a new key"
    )]
    pub fn properties(&mut self, ui: &mut Ui, theme: &Theme) -> Vec<MapAction> {
        let mut actions = Vec::new();
        if let Some(refused) = &self.refused {
            ui.label(RichText::new(refused).color(theme.color(Token::StatusError)));
        }
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
        actions.extend(self.tools.properties(ui).into_iter().map(MapAction::Brush));
        let mut chosen: Vec<EntityIndex> =
            self.keys.as_ref().map(|(e, _)| *e).into_iter().collect();
        chosen.extend(self.also.iter().copied());
        actions.extend(selection_ui(ui, &chosen).into_iter().map(MapAction::Group));
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
                        if let Some(set) = field.ui(ui, typed, value) {
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
            ui.add(
                egui::TextEdit::singleline(&mut self.adding.0)
                    .hint_text(Text::MapKey.get())
                    .desired_width(NEW_PAIR_WIDTH),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.adding.1)
                    .hint_text(Text::MapValue.get())
                    .desired_width(NEW_PAIR_WIDTH),
            );
            let targets = !keys.targetnames.is_empty() && self.targeting.contains(&self.adding.0);
            if targets {
                egui::ComboBox::from_id_salt(ui.id().with((entity, 1)))
                    .selected_text(Text::MapTargets.get())
                    .show_ui(ui, |ui| {
                        for name in &keys.targetnames {
                            if ui.selectable_label(false, name).clicked() {
                                self.adding.1.clone_from(name);
                            }
                        }
                    });
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
fn fly(ui: &Ui, camera: &mut Camera) {
    let (dt, keys) = ui.input(|i| {
        let held = |key| if i.key_down(key) { 1.0 } else { 0.0 };
        let forward = held(Key::W) - held(Key::S);
        let right = held(Key::D) - held(Key::A);
        let up = held(Key::Q) - held(Key::X);
        (i.stable_dt, [forward, right, up])
    });
    if keys.iter().any(|k: &f32| *k != 0.0) {
        let step = FLY_SPEED * dt;
        camera.travel(keys[0] * step, keys[1] * step, keys[2] * step);
        ui.ctx().request_repaint();
    }
}

/// TrenchBroom's keys on the selection: the arrows and Page Up and Down move it by the grid,
/// along the map axis nearest the camera's; Alt with Left or Right turns it; Delete deletes it;
/// Ctrl+D (Cmd+D) duplicates it.
fn selection_keys(ui: &Ui, camera: &Camera, entity: EntityIndex) -> Vec<MapAction> {
    let [forward, right, _] = camera.axes();
    let snap = |v: [f32; 3]| -> [f64; 3] {
        if v[0].abs() >= v[1].abs() {
            [f64::from(v[0].signum()) * GRID, 0.0, 0.0]
        } else {
            [0.0, f64::from(v[1].signum()) * GRID, 0.0]
        }
    };
    let negate = |v: [f64; 3]| v.map(|c| -c);
    let mut actions = Vec::new();
    ui.input_mut(|i| {
        if i.consume_key(Modifiers::ALT, Key::ArrowLeft) {
            actions.push(MapAction::Rotate(entity, -QUARTER_TURN));
        }
        if i.consume_key(Modifiers::ALT, Key::ArrowRight) {
            actions.push(MapAction::Rotate(entity, QUARTER_TURN));
        }
        let moves = [
            (Key::ArrowUp, snap(forward)),
            (Key::ArrowDown, negate(snap(forward))),
            (Key::ArrowRight, snap(right)),
            (Key::ArrowLeft, negate(snap(right))),
            (Key::PageUp, [0.0, 0.0, GRID]),
            (Key::PageDown, [0.0, 0.0, -GRID]),
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
