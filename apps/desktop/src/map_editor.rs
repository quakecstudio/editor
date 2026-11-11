// SPDX-License-Identifier: GPL-3.0-or-later

//! The map editor (P5-06, MAP-4, MAP-20): the open map in 3D as lines, its entities as boxes
//! and model wireframes, picked with the mouse and edited with TrenchBroom's keys; and the key
//! editor in the Properties panel. Every edit is a [`MapAction`] the shell runs as a `map.*`
//! command, so each has undo. `docs/map-editor.md` maps TrenchBroom's bindings to these.

use std::collections::HashMap;

use eframe::egui::{
    self, Color32, Key, Modifiers, Painter, Pos2, Rect, RichText, Sense, Stroke, Ui, WidgetInfo,
    WidgetType,
};
use qs_render::Camera;
use qs_workbench::entity_classes::ClassInfo;
use qs_workbench::map_edit::{EntityIndex, EntityKeys, MapDocument, MapRevision};

use crate::map_scene::{ClassLook, Line, Scene, is_target_key, load_models, start_camera};
use crate::strings::Text;
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
const GRID: f64 = 16.0;
/// TrenchBroom turns a selection by a right angle (`half_pi`).
const QUARTER_TURN: f64 = 90.0;
/// The width of a value's field in the key editor, in points.
const VALUE_WIDTH: f32 = 140.0;
/// The key editor's columns: the key, its value and its remove button.
const KEY_COLUMNS: usize = 3;
/// The width of the new key's and new value's fields, in points.
const NEW_PAIR_WIDTH: f32 = 90.0;
/// The nearest a line is drawn, in map units, as the renderer's near plane.
const NEAR: f32 = 4.0;

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
    /// Set a key of an entity.
    SetKey(EntityIndex, String, String),
    /// Remove a key of an entity.
    RemoveKey(EntityIndex, String),
    /// Save the map.
    Save,
}

/// The map editor's view state.
#[derive(Debug, Default)]
pub struct MapEditor {
    camera: Option<Camera>,
    scene: Scene,
    /// The document's path and revision the scene was built from.
    built: Option<(std::path::PathBuf, MapRevision)>,
    classes: HashMap<String, ClassLook>,
    models: HashMap<String, Vec<Line>>,
    /// The selected entity's keys, as `map.entity` gave them.
    keys: Option<(EntityIndex, EntityKeys)>,
    /// The document revision the keys were read at.
    keys_read: Option<MapRevision>,
    /// The values being typed, by key.
    typing: HashMap<String, String>,
    /// The new key and value being typed.
    adding: (String, String),
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

    /// Forgets the camera, so the next map opened starts at its player start.
    pub fn reset_camera(&mut self) {
        self.camera = None;
    }

    /// Draws `document` in all the space `ui` has left and reads the mouse and keys; what
    /// they asked for.
    pub fn ui(&mut self, ui: &mut Ui, theme: &Theme, document: &MapDocument) -> Vec<MapAction> {
        let built = (document.path().to_path_buf(), document.revision());
        if self.built.as_ref() != Some(&built) {
            self.scene = Scene::build(document, &self.classes);
            self.built = Some(built);
        }
        let camera = self.camera.get_or_insert_with(|| start_camera(document));
        let rect = ui.available_rect_before_wrap();
        let response = ui.interact(
            rect,
            egui::Id::new(core::any::TypeId::of::<Self>()),
            Sense::click_and_drag(),
        );
        response.widget_info(|| {
            WidgetInfo::labeled(WidgetType::Other, true, Text::MapEditorView.get())
        });
        if response.clicked() || response.secondary_clicked() {
            response.request_focus();
        }
        if response.dragged_by(egui::PointerButton::Secondary) {
            let drag = response.drag_delta();
            camera.turn(-drag.x * TURN, -drag.y * TURN);
        }
        if response.dragged_by(egui::PointerButton::Middle) {
            let drag = response.drag_delta();
            camera.travel(0.0, -drag.x * PAN, drag.y * PAN);
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            camera.travel(scroll * TRAVEL, 0.0, 0.0);
        }
        let mut actions = Vec::new();
        let selected = document.selected();
        if response.has_focus() {
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
            fly(ui, camera);
            if let Some(entity) = selected {
                actions.extend(selection_keys(ui, camera, entity));
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
                actions.push(MapAction::Save);
            }
        }
        let aspect = rect.width() / rect.height().max(1.0);
        if response.clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
            let x = (at.x - rect.left()) / rect.width().max(1.0) * 2.0 - 1.0;
            let y = 1.0 - (at.y - rect.top()) / rect.height().max(1.0) * 2.0;
            let (start, direction) = camera.ray(aspect, [x, y]);
            actions.push(MapAction::Select(self.scene.pick(start, direction)));
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme.color(Token::SurfaceViewport));
        let projection = Projection::new(camera, rect);
        let world = Stroke::new(1.0, theme.color(Token::TextFaint));
        for line in &self.scene.lines {
            projection.line(&painter, *line, world);
        }
        for shape in &self.scene.entities {
            let chosen = selected == Some(shape.index);
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
        actions
    }

    /// The key editor of the selected entity, in the Properties panel; what it asked for.
    pub fn properties(&mut self, ui: &mut Ui, theme: &Theme) -> Vec<MapAction> {
        let mut actions = Vec::new();
        let Some((entity, keys)) = &self.keys else {
            ui.label(
                RichText::new(Text::MapNoSelection.get()).color(theme.color(Token::TextMuted)),
            );
            return actions;
        };
        let entity = *entity;
        ui.label(RichText::new(keys.classname.clone().unwrap_or_default()).strong());
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
                    let edit = ui.add_sized(
                        [VALUE_WIDTH, ui.spacing().interact_size.y],
                        egui::TextEdit::singleline(typed),
                    );
                    if edit.lost_focus() && typed != value {
                        actions.push(MapAction::SetKey(entity, key.clone(), typed.clone()));
                    }
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
            let targets = !keys.targetnames.is_empty() && is_target_key(&self.adding.0);
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
                actions.push(MapAction::SetKey(entity, key.trim().to_owned(), value));
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
fn box_lines([min, max]: [[f32; 3]; 2]) -> [Line; 12] {
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
fn rgb(color: [f32; 3]) -> Color32 {
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

/// Map space to the view's points, from the camera's axes and field of view.
struct Projection {
    origin: [f32; 3],
    axes: [[f32; 3]; 3],
    half: [f32; 2],
    rect: Rect,
}

impl Projection {
    fn new(camera: &Camera, rect: Rect) -> Self {
        let aspect = rect.width() / rect.height().max(1.0);
        let half_x = (camera.fov_x.to_radians() / 2.0).tan();
        Self {
            origin: camera.origin,
            axes: camera.axes(),
            half: [half_x, half_x / aspect.max(f32::EPSILON)],
            rect,
        }
    }

    /// `p` in view space: right, up and forward.
    fn view(&self, p: [f32; 3]) -> [f32; 3] {
        let v = [
            p[0] - self.origin[0],
            p[1] - self.origin[1],
            p[2] - self.origin[2],
        ];
        let dot = |a: [f32; 3]| a[0] * v[0] + a[1] * v[1] + a[2] * v[2];
        let [forward, right, up] = self.axes;
        [dot(right), dot(up), dot(forward)]
    }

    fn screen(&self, v: [f32; 3]) -> Pos2 {
        let x = v[0] / (v[2] * self.half[0]);
        let y = v[1] / (v[2] * self.half[1]);
        Pos2::new(
            self.rect.center().x + x * self.rect.width() / 2.0,
            self.rect.center().y - y * self.rect.height() / 2.0,
        )
    }

    /// Draws `line`, cut at the near plane.
    fn line(&self, painter: &Painter, [a, b]: Line, stroke: Stroke) {
        let (mut a, mut b) = (self.view(a), self.view(b));
        if a[2] < NEAR && b[2] < NEAR {
            return;
        }
        let cut = |front: [f32; 3], back: [f32; 3]| {
            let t = (NEAR - back[2]) / (front[2] - back[2]);
            [
                back[0] + (front[0] - back[0]) * t,
                back[1] + (front[1] - back[1]) * t,
                NEAR,
            ]
        };
        if a[2] < NEAR {
            a = cut(b, a);
        } else if b[2] < NEAR {
            b = cut(a, b);
        }
        painter.line_segment([self.screen(a), self.screen(b)], stroke);
    }
}
