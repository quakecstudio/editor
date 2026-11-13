// SPDX-License-Identifier: GPL-3.0-or-later

//! The map editor's brush tools (P6-08, MAP-2): TrenchBroom's Brush, Clip and Vertex tools
//! beside Select. Brush draws a box on the ground with a drag; Clip cuts the selected brush by
//! the vertical plane through a line dragged on the ground, keeping either side or both; Vertex
//! drags a corner of the selected brush across the plane it lies in; Shape fills a box dragged
//! on the ground with the shape chosen (P6-18, MAP-10). Move drags the selected brushes, with
//! texture lock or without, and Measure the distance between two points, each showing its
//! numbers in the view (P6-09). Clicking with any of them selects a brush, Shift+click adds
//! one, and the Properties panel shows the selection's size, aligns its textures, and
//! subtracts, merges and deletes the brushes selected; it also hollows, intersects, scales,
//! shears and sweeps them, extrudes or splits a face picked with Alt+click, and splits or
//! chamfers the edge between two corners the Vertex tool picked with Ctrl+click, several of
//! which a drag moves together (P6-18, MAP-15). Each edit is a [`BrushAction`] the shell runs
//! as a `map.brush_*` command, so each has undo.

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui::{self, Painter, Pos2, Response, Stroke, Ui};
use qs_map::shapes::{
    SHAPE_MAX_RINGS, SHAPE_MAX_ROCK_POINTS, SHAPE_MAX_SIDES, SHAPE_MAX_SPHEROID_FACES,
    SHAPE_MIN_RINGS, SHAPE_MIN_ROCK_POINTS, SHAPE_MIN_SIDES, SHAPE_STEP_DEPTH, SHAPE_STEP_HEIGHT,
};
use qs_workbench::map_edit::{
    ArchWall, Axis, BrushIndex, Circle, Climb, EntityIndex, KeepPart, Lock, ShapeSpec,
};
use qs_workbench::map_uv::{FaceRef, UvChoice};

use crate::map_editor::{GRID, box_lines};
use crate::map_panes::{Pane, Projection};
use crate::map_scene::{Scene, single};
pub(crate) use crate::map_uv::PickMode;
use crate::map_uv::UvEditor;
use crate::strings::{Text, apply_texture, measure_readout, move_readout, selection_size};
use crate::theme::{Theme, Token};
use tool_fields::ToolFields;

mod tool_fields;

/// The width of the field the prefab's name is typed in, in points.
const PREFAB_NAME_WIDTH: f32 = 140.0;
/// The height of a box the Brush tool draws, in map units: four grid steps.
const BRUSH_HEIGHT: f32 = 64.0;
/// The height of a box the Shape tool fills, in map units: eight grid steps.
const SHAPE_HEIGHT: f32 = 128.0;
/// The sides a round shape may have (MAP-10.3).
const SIDES: core::ops::RangeInclusive<u32> = SHAPE_MIN_SIDES..=SHAPE_MAX_SIDES;
/// The sides of a scalable curve the Shape tool offers (MAP-10.9).
const CURVE_SIDES: [u32; 4] = [12, 24, 48, 96];
/// The sides a shape starts with.
const DEFAULT_SIDES: u32 = 16;
/// A scalable curve's sides before others are chosen.
const DEFAULT_CURVE_SIDES: u32 = 24;
/// The wall a hollow shape starts with, in map units.
const DEFAULT_WALL: f64 = 16.0;
/// How far one drag step moves the wall, in map units.
const WALL_STEP: f64 = 1.0;
/// An arch's curve: half a turn from +x.
const ARCH_ARC: f64 = 180.0;
/// The times each triangle of an icospheroid is cut.
const ICOSPHERE_SUBDIVISIONS: u32 = 2;
/// The height of the clip plane drawn while a cut line is dragged, in map units.
const CLIP_HEIGHT: f32 = 256.0;
/// How near the pointer must be to a corner, in points, for the Vertex tool to take it.
const VERTEX_REACH: f32 = 8.0;
/// The radius of a corner's handle, in points.
const VERTEX_DOT: f32 = 3.0;
/// The smallest ray component counted as crossing a plane rather than running along it.
const ALONG_PLANE: f32 = 1e-6;
/// How far a readout is from the view's lower left corner, in points.
const READOUT_INSET: f32 = 8.0;
/// The size of a readout's text, in points.
const READOUT_SIZE: f32 = 13.0;
/// The step a texture field moves by per point dragged.
const ALIGN_STEP: f64 = 1.0;
/// The step a texture scale field moves by per point dragged.
const SCALE_STEP: f64 = 0.05;
/// A point with every coordinate 0.
const ORIGIN: [f32; 3] = [0.0; 3];
/// How far ahead along the pointer's ray a measure's first point is, in map units: on the
/// plane facing the view there, in front of the camera.
const MEASURE_AHEAD: f32 = 256.0;

/// A brush: the entity holding it and its place among the entity's brushes.
pub(crate) type BrushRef = (EntityIndex, BrushIndex);

/// The tool the map view's left button uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MapTool {
    /// Select, move and link entities.
    #[default]
    Select,
    /// Draw a box brush.
    Brush,
    /// Cut a brush by a plane.
    Clip,
    /// Move a brush's corners.
    Vertex,
    /// Move the selected brushes (P6-09).
    Move,
    /// Measure between two points (P6-09).
    Measure,
    /// Fill a box with a shape of brushes.
    Shape,
}

impl MapTool {
    /// Every tool, in the toolbar's order.
    const ALL: [Self; 7] = [
        Self::Select,
        Self::Brush,
        Self::Clip,
        Self::Vertex,
        Self::Move,
        Self::Measure,
        Self::Shape,
    ];

    const fn text(self) -> Text {
        match self {
            Self::Select => Text::MapToolSelect,
            Self::Brush => Text::MapToolBrush,
            Self::Clip => Text::MapToolClip,
            Self::Vertex => Text::MapToolVertex,
            Self::Move => Text::MapToolMove,
            Self::Measure => Text::MapToolMeasure,
            Self::Shape => Text::MapToolShape,
        }
    }

    /// What to do with the tool, shown beside the toolbar.
    const fn hint(self) -> Option<Text> {
        match self {
            Self::Select => None,
            Self::Brush => Some(Text::MapBrushHint),
            Self::Clip => Some(Text::MapClipHint),
            Self::Vertex => Some(Text::MapVertexHint),
            Self::Move => Some(Text::MapMoveHint),
            Self::Measure => Some(Text::MapMeasureHint),
            Self::Shape => Some(Text::MapShapeHint),
        }
    }
}

/// A brush edit the map editor asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum BrushAction {
    /// Create a box brush in the world from one corner to the other.
    Create([f64; 3], [f64; 3]),
    /// Cut a brush by the plane through three points, keeping the parts chosen.
    Clip(EntityIndex, BrushIndex, [[f64; 3]; 3], KeepPart),
    /// Move a corner of a brush from one point to another.
    Vertex(EntityIndex, BrushIndex, [f64; 3], [f64; 3]),
    /// Subtract a brush from the others of its entity.
    Subtract(EntityIndex, BrushIndex),
    /// Merge brushes of an entity into their hull.
    Merge(EntityIndex, Vec<BrushIndex>),
    /// Delete a brush.
    Delete(EntityIndex, BrushIndex),
    /// Move brushes of an entity by an offset, their textures fixed in the world or not.
    Move(EntityIndex, Vec<BrushIndex>, [f64; 3], Lock),
    /// Set the texture or alignment of every face of brushes of an entity.
    Texture(EntityIndex, Vec<BrushIndex>, TextureChange),
    /// Fill the box from one corner to the other with a shape, in the world.
    Shape([f64; 3], [f64; 3], ShapeSpec),
    /// Fit, justify, flip, reset, snap or adjust the texture of faces (MAP-13).
    Uv(Vec<FaceRef>, UvChoice),
    /// Carry one face's texture alignment onto others (MAP-13.8).
    AlignFaces(FaceRef, Vec<FaceRef>),
    /// Save brushes, each by its entity, and whole entities as the prefab of the name (P6-23).
    SavePrefab(String, Vec<BrushRef>, Vec<EntityIndex>),
    /// Hollow a brush into walls of the thickness (MAP-15.5).
    Hollow(EntityIndex, BrushIndex, f64),
    /// Replace brushes of an entity with the solid all hold (MAP-15.6).
    Intersect(EntityIndex, Vec<BrushIndex>),
    /// Scale brushes of an entity about their middle by factors along x, y and z (MAP-15.2).
    Scale(EntityIndex, Vec<BrushIndex>, [f64; 3], Lock),
    /// Shear brushes of an entity about their middle along an axis by another (MAP-15.2).
    Shear(EntityIndex, Vec<BrushIndex>, Axis, Axis, f64, Lock),
    /// Sweep a brush along a path from the origin (MAP-15.4).
    Sweep(EntityIndex, BrushIndex, Vec<[f64; 3]>, Lock),
    /// Move a face along its normal (MAP-15.3).
    Extrude(FaceRef, f64, Lock),
    /// Split a face by a new vertex the distance out from its middle (MAP-15.8).
    SplitFace(FaceRef, f64),
    /// Move several corners of a brush at once (MAP-15.7).
    Vertices(EntityIndex, BrushIndex, Vec<([f64; 3], [f64; 3])>),
    /// Split an edge by a new vertex the distance out from its middle (MAP-15.8).
    SplitEdge(EntityIndex, BrushIndex, [[f64; 3]; 2], f64),
    /// Chamfer an edge into bevel faces from the distance along its faces (MAP-15.9).
    Chamfer(EntityIndex, BrushIndex, [[f64; 3]; 2], f64, u32),
}

/// What `map.brush_texture` sets; the parts not given are kept (MAP-12).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextureChange {
    /// The texture's name.
    pub texture: Option<String>,
    /// The s and t shift.
    pub shift: Option<[f64; 2]>,
    /// The rotation in degrees.
    pub rotate: Option<f64>,
    /// The s and t scale.
    pub scale: Option<[f64; 2]>,
}

/// A part of a texture's alignment, as the Properties panel edits it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum AlignPart {
    Shift,
    Rotate,
    Scale,
}

/// What the Move or Measure tool shows, while it drags and after.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Readout {
    /// The selection moved by `by`, the pointer at `at`.
    Move {
        /// The move so far, on the grid.
        by: [f64; 3],
        /// Where the pointer is, on the grid.
        at: [f64; 3],
    },
    /// From one point to another, both on the grid.
    Measure {
        /// Where it starts.
        from: [f64; 3],
        /// Where it ends.
        to: [f64; 3],
    },
}

impl Readout {
    /// Its text, in Quake units.
    pub(crate) fn text(self) -> String {
        match self {
            Self::Move { by, at } => move_readout(by, at),
            Self::Measure { from, to } => {
                let along = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
                let distance = along.iter().map(|c| c * c).sum::<f64>().sqrt();
                measure_readout(distance, along)
            }
        }
    }
}

/// The texture alignment fields of the Properties panel, and the parts edited since the last
/// Align, which alone it sets.
#[derive(Clone, Debug, PartialEq)]
struct AlignFields {
    shift: [f64; 2],
    rotate: f64,
    scale: [f64; 2],
    edited: BTreeSet<AlignPart>,
}

impl Default for AlignFields {
    fn default() -> Self {
        Self {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
            edited: BTreeSet::new(),
        }
    }
}

impl AlignFields {
    /// The change setting the parts edited.
    fn change(&self) -> TextureChange {
        let edited = |part| self.edited.contains(&part);
        TextureChange {
            texture: None,
            shift: edited(AlignPart::Shift).then_some(self.shift),
            rotate: edited(AlignPart::Rotate).then_some(self.rotate),
            scale: edited(AlignPart::Scale).then_some(self.scale),
        }
    }
}

/// The shapes the Shape tool makes, in its menu's order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ShapeKind {
    /// A cylinder, solid or hollow.
    #[default]
    Cylinder,
    /// A cone.
    Cone,
    /// An arch.
    Arch,
    /// A UV spheroid.
    UvSpheroid,
    /// An icospheroid.
    Icospheroid,
    /// A rock.
    Rock,
    /// A scalable curve.
    Curve,
    /// Straight stairs.
    Stairs,
    /// Spiral stairs.
    SpiralStairs,
}

impl ShapeKind {
    /// Every shape, in the menu's order.
    const ALL: [Self; 9] = [
        Self::Cylinder,
        Self::Cone,
        Self::Arch,
        Self::UvSpheroid,
        Self::Icospheroid,
        Self::Rock,
        Self::Curve,
        Self::Stairs,
        Self::SpiralStairs,
    ];

    const fn text(self) -> Text {
        match self {
            Self::Cylinder => Text::ShapeCylinder,
            Self::Cone => Text::ShapeCone,
            Self::Arch => Text::ShapeArch,
            Self::UvSpheroid => Text::ShapeUvSpheroid,
            Self::Icospheroid => Text::ShapeIcospheroid,
            Self::Rock => Text::ShapeRock,
            Self::Curve => Text::ShapeCurve,
            Self::Stairs => Text::ShapeStairs,
            Self::SpiralStairs => Text::ShapeSpiralStairs,
        }
    }

    /// Whether the shape is round, so its corners lie on or around the box's ellipse.
    const fn takes_circle(self) -> bool {
        match self {
            Self::Cylinder | Self::Cone => true,
            Self::Arch
            | Self::UvSpheroid
            | Self::Icospheroid
            | Self::Rock
            | Self::Curve
            | Self::Stairs
            | Self::SpiralStairs => false,
        }
    }
}

/// Whether a cylinder is solid or a wall around a hole.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Fill {
    /// Solid.
    #[default]
    Solid,
    /// A wall around a hole.
    Hollow,
}

/// What the Shape tool makes: the shape and the settings its toolbar shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ShapeOptions {
    kind: ShapeKind,
    sides: u32,
    curve_sides: u32,
    circle: Circle,
    fill: Fill,
    wall: f64,
    climb: Climb,
}

impl Default for ShapeOptions {
    fn default() -> Self {
        Self {
            kind: ShapeKind::default(),
            sides: DEFAULT_SIDES,
            curve_sides: DEFAULT_CURVE_SIDES,
            circle: Circle::default(),
            fill: Fill::default(),
            wall: DEFAULT_WALL,
            climb: Climb::default(),
        }
    }
}

impl ShapeOptions {
    /// The shape with these settings, the rest their defaults (MAP-10).
    fn spec(&self) -> ShapeSpec {
        let (sides, circle) = (self.sides, self.circle);
        match self.kind {
            ShapeKind::Cylinder => ShapeSpec::Cylinder {
                sides,
                circle,
                hollow: match self.fill {
                    Fill::Solid => None,
                    Fill::Hollow => Some(self.wall),
                },
            },
            ShapeKind::Cone => ShapeSpec::Cone { sides, circle },
            ShapeKind::Arch => ShapeSpec::Arch {
                sides,
                arc: ARCH_ARC,
                start: 0.0,
                thickness: self.wall,
                wall: ArchWall::Open,
            },
            ShapeKind::UvSpheroid => ShapeSpec::UvSpheroid {
                sides,
                rings: sides
                    .div_ceil(2)
                    .min(
                        SHAPE_MAX_SPHEROID_FACES
                            .checked_div(sides)
                            .unwrap_or(SHAPE_MAX_RINGS),
                    )
                    .clamp(SHAPE_MIN_RINGS, SHAPE_MAX_RINGS),
            },
            ShapeKind::Icospheroid => ShapeSpec::Icospheroid {
                subdivisions: ICOSPHERE_SUBDIVISIONS,
            },
            ShapeKind::Rock => ShapeSpec::Rock {
                points: sides.clamp(SHAPE_MIN_ROCK_POINTS, SHAPE_MAX_ROCK_POINTS),
                seed: 0,
            },
            ShapeKind::Curve => ShapeSpec::Curve {
                sides: self.curve_sides,
                hollow: None,
            },
            ShapeKind::Stairs => ShapeSpec::Stairs {
                height: SHAPE_STEP_HEIGHT,
                depth: SHAPE_STEP_DEPTH,
                climb: self.climb,
            },
            ShapeKind::SpiralStairs => ShapeSpec::SpiralStairs {
                sides,
                height: SHAPE_STEP_HEIGHT,
                core: 0.0,
            },
        }
    }

    /// The shape's settings in the toolbar.
    fn toolbar(&mut self, ui: &mut Ui) {
        egui::ComboBox::from_id_salt(ui.id().with(core::any::TypeId::of::<ShapeKind>()))
            .selected_text(self.kind.text().get())
            .show_ui(ui, |ui| {
                for kind in ShapeKind::ALL {
                    ui.selectable_value(&mut self.kind, kind, kind.text().get());
                }
            });
        match self.kind {
            ShapeKind::Curve => {
                ui.label(Text::MapShapeSides.get());
                for sides in CURVE_SIDES {
                    ui.selectable_value(&mut self.curve_sides, sides, sides.to_string());
                }
            }
            ShapeKind::Icospheroid | ShapeKind::Stairs => {}
            ShapeKind::Cylinder
            | ShapeKind::Cone
            | ShapeKind::Arch
            | ShapeKind::UvSpheroid
            | ShapeKind::Rock
            | ShapeKind::SpiralStairs => {
                ui.label(Text::MapShapeSides.get());
                ui.add(egui::DragValue::new(&mut self.sides).range(SIDES));
            }
        }
        if self.kind.takes_circle() {
            ui.selectable_value(
                &mut self.circle,
                Circle::Vertices,
                Text::MapShapeCorners.get(),
            );
            ui.selectable_value(&mut self.circle, Circle::Edges, Text::MapShapeEdges.get());
        }
        if self.kind == ShapeKind::Cylinder {
            ui.selectable_value(&mut self.fill, Fill::Solid, Text::MapShapeSolid.get());
            ui.selectable_value(&mut self.fill, Fill::Hollow, Text::MapShapeHollow.get());
        }
        if (self.kind == ShapeKind::Cylinder && self.fill == Fill::Hollow)
            || self.kind == ShapeKind::Arch
        {
            ui.label(Text::MapShapeWall.get());
            ui.add(egui::DragValue::new(&mut self.wall).speed(WALL_STEP));
        }
        if self.kind == ShapeKind::Stairs {
            for (climb, text) in [
                (Climb::East, Text::MapClimbEast),
                (Climb::West, Text::MapClimbWest),
                (Climb::North, Text::MapClimbNorth),
                (Climb::South, Text::MapClimbSouth),
            ] {
                ui.selectable_value(&mut self.climb, climb, text.get());
            }
        }
    }
}

/// What the left button is dragging.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// A box or a cut line, from this point on the ground.
    Ground([f32; 3]),
    /// A corner of a brush, from where it is.
    Corner(BrushRef, [f64; 3]),
    /// The selected brushes, from this point, on the plane across this axis through it.
    Selection([f32; 3], MapAxis),
    /// A measure, from this point, on the plane across this axis through it.
    Measure([f32; 3], MapAxis),
}

/// The brush tools' state.
#[derive(Debug, Default)]
pub(crate) struct BrushTools {
    tool: MapTool,
    /// Which parts of a clipped brush stay.
    keep: KeepPart,
    /// The brushes selected, the first chosen first.
    chosen: Vec<BrushRef>,
    /// The corners each selected brush had when it was last seen: an edit or undo that moves
    /// brushes along leaves a place naming another brush, whose corners differ.
    seen: BTreeMap<BrushRef, Vec<[f64; 3]>>,
    /// The brush the last clip or vertex move asked for reshapes, which stays selected.
    reshaped: Option<BrushRef>,
    drag: Option<Drag>,
    /// The pane the drag started in, the only one that reads it.
    dragging_in: Option<Pane>,
    /// Whether moved brushes keep their textures fixed in the world (MAP-11.2).
    lock: Lock,
    /// What the Move or Measure tool last showed.
    shown: Option<Readout>,
    /// The texture alignment fields.
    align: AlignFields,
    /// What the Shape tool makes.
    shape: ShapeOptions,
    /// The faces selected and the UV editor.
    uv: UvEditor,
    /// The name typed for a prefab of the selection.
    prefab_name: String,
    /// The brush tools' fields in the Properties panel (P6-18).
    fields: ToolFields,
    /// The corners of the first selected brush the Vertex tool picked with Ctrl+click.
    corners: Vec<[f64; 3]>,
}

/// A ray in map space: where it starts and its direction.
pub(crate) type Ray = ([f32; 3], [f32; 3]);

/// Where `ray` meets the horizontal plane at height `z`, ahead of its start.
fn at_height((start, direction): Ray, z: f32) -> Option<[f32; 3]> {
    if direction[2].abs() < ALONG_PLANE {
        return None;
    }
    let t = (z - start[2]) / direction[2];
    (t > 0.0).then(|| [start[0] + direction[0] * t, start[1] + direction[1] * t, z])
}

/// One of the map's axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MapAxis {
    X,
    Y,
    Z,
}

impl MapAxis {
    /// Its place in a point's coordinates.
    const fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }

    /// The axis `ray` runs most along: the one the plane facing it most squarely is across.
    /// From above that is z, the ground; in a 2D view it is the view's own axis.
    fn facing((_, direction): Ray) -> Self {
        let [x, y, z] = direction.map(f32::abs);
        if z >= x && z >= y {
            Self::Z
        } else if y >= x {
            Self::Y
        } else {
            Self::X
        }
    }

    /// Where `ray` meets the plane across this axis through `through`, ahead of its start.
    fn meets(self, (start, direction): Ray, through: [f32; 3]) -> Option<[f32; 3]> {
        let i = self.index();
        let d = direction.get(i).copied()?;
        if d.abs() < ALONG_PLANE {
            return None;
        }
        let t = (through.get(i).copied()? - start.get(i).copied()?) / d;
        (t > 0.0).then(|| {
            [0, 1, 2].map(|i| {
                let (s, d) = (start.get(i), direction.get(i));
                s.copied().unwrap_or_default() + d.copied().unwrap_or_default() * t
            })
        })
    }
}

/// `point` on the grid, as the commands snap it.
fn snapped(point: [f32; 3]) -> [f64; 3] {
    point.map(|c| (f64::from(c) / GRID).round() * GRID)
}

/// `point` on the grid, for drawing.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a grid point of a map is well inside f32's range"
)]
fn snapped_f32(point: [f32; 3]) -> [f32; 3] {
    snapped(point).map(|c| c as f32)
}

/// The box from `a` to `b` on the ground, `height` tall.
fn ground_box(a: [f32; 3], b: [f32; 3], height: f32) -> [[f32; 3]; 2] {
    [
        [a[0].min(b[0]), a[1].min(b[1]), a[2]],
        [a[0].max(b[0]), a[1].max(b[1]), a[2] + height],
    ]
}

impl BrushTools {
    /// The tool the left button uses.
    pub(crate) const fn tool(&self) -> MapTool {
        self.tool
    }

    /// Picks the face `ray` meets first into the face selection as `mode` says, as an
    /// Alt-click or Alt-Shift-click does; the brushes chosen are let go.
    pub(crate) fn pick_face(
        &mut self,
        (start, direction): Ray,
        scene: &Scene,
        hidden: &BTreeSet<EntityIndex>,
        mode: PickMode,
    ) {
        self.uv
            .pick(scene.pick_face(start, direction, hidden), scene, mode);
        self.chosen.clear();
    }

    /// Lets go of the faces selected.
    pub(crate) fn clear_faces(&mut self) {
        self.uv.clear();
    }

    /// Adds `face` to the faces selected, as an Alt-Shift-click would.
    #[cfg(test)]
    pub(crate) fn select_face(&mut self, face: FaceRef, scene: &Scene) {
        self.uv.pick(Some(face), scene, PickMode::Toggle);
    }

    /// Forgets the selected brushes `scene` no longer has where they were: none is at the
    /// place, or another brush is, other than the one the last clip or vertex move reshaped.
    pub(crate) fn keep_found(&mut self, scene: &Scene) {
        let reshaped = self.reshaped.take();
        let seen = &mut self.seen;
        self.chosen.retain(|&chosen| {
            let Some(shape) = scene.brush(chosen.0, chosen.1) else {
                return false;
            };
            let same = seen.get(&chosen) == Some(&shape.vertices);
            if same || reshaped == Some(chosen) {
                seen.insert(chosen, shape.vertices.clone());
            }
            same || reshaped == Some(chosen)
        });
        let chosen = &self.chosen;
        self.seen.retain(|brush, _| chosen.contains(brush));
        self.uv.keep_found(scene);
    }

    /// The tool selector, and the clip's side when clipping.
    pub(crate) fn toolbar(&mut self, ui: &mut Ui) {
        for tool in MapTool::ALL {
            ui.selectable_value(&mut self.tool, tool, tool.text().get());
        }
        if self.tool == MapTool::Shape {
            ui.separator();
            self.shape.toolbar(ui);
        }
        if self.tool == MapTool::Clip {
            ui.separator();
            ui.selectable_value(&mut self.keep, KeepPart::Back, Text::MapKeepRight.get());
            ui.selectable_value(&mut self.keep, KeepPart::Front, Text::MapKeepLeft.get());
            ui.selectable_value(&mut self.keep, KeepPart::Both, Text::KeepBoth.get());
        }
        if self.tool == MapTool::Move {
            ui.separator();
            let mut locked = self.lock == Lock::Locked;
            if ui
                .toggle_value(&mut locked, Text::MapTextureLock.get())
                .changed()
            {
                self.lock = if locked { Lock::Locked } else { Lock::Free };
            }
        }
        if let Some(hint) = self.tool.hint() {
            ui.separator();
            ui.weak(hint.get());
        }
    }

    /// What the Move or Measure tool last showed.
    #[cfg(test)]
    pub(crate) const fn shown(&self) -> Option<Readout> {
        self.shown
    }

    /// The box around the selected brushes' corners, when any is selected.
    fn bounds(&self, scene: &Scene) -> Option<[[f64; 3]; 2]> {
        let corners = self
            .chosen
            .iter()
            .filter_map(|&(entity, brush)| scene.brush(entity, brush))
            .flat_map(|shape| shape.vertices.iter().copied());
        corners.fold(None, |bounds, v| {
            let [min, max] = bounds.unwrap_or([v, v]);
            Some([
                [min[0].min(v[0]), min[1].min(v[1]), min[2].min(v[2])],
                [max[0].max(v[0]), max[1].max(v[1]), max[2].max(v[2])],
            ])
        })
    }

    /// What a Move or Measure drag shows with the pointer on `ray`.
    fn readout(drag: Drag, ray: Ray) -> Option<Readout> {
        let moved = |from: [f64; 3], to: [f64; 3]| Readout::Move {
            by: [to[0] - from[0], to[1] - from[1], to[2] - from[2]],
            at: to,
        };
        match drag {
            Drag::Selection(from, axis) => {
                Some(moved(snapped(from), snapped(axis.meets(ray, from)?)))
            }
            Drag::Measure(from, axis) => Some(Readout::Measure {
                from: snapped(from),
                to: snapped(axis.meets(ray, from)?),
            }),
            Drag::Corner(_, from) => {
                let [x, y, _] = snapped(at_height(ray, single(from[2]))?);
                Some(moved(from, [x, y, from[2]]))
            }
            Drag::Ground(_) => None,
        }
    }

    /// The moves of the selected brushes by `by`, one per entity holding them; the brushes
    /// stay selected where they go.
    fn moved(&mut self, by: [f64; 3]) -> Vec<BrushAction> {
        let mut by_entity: BTreeMap<EntityIndex, Vec<BrushIndex>> = BTreeMap::new();
        for &(entity, brush) in &self.chosen {
            by_entity.entry(entity).or_default().push(brush);
            if let Some(corners) = self.seen.get_mut(&(entity, brush)) {
                for corner in corners {
                    *corner = [corner[0] + by[0], corner[1] + by[1], corner[2] + by[2]];
                }
            }
        }
        by_entity
            .into_iter()
            .map(|(entity, brushes)| BrushAction::Move(entity, brushes, by, self.lock))
            .collect()
    }

    /// Reads the left button in the pane `pane`, seen through `projection`, for the tool
    /// chosen, other than Select; a drag is read only in the pane it started in. What it asked
    /// for.
    pub(crate) fn input(
        &mut self,
        ui: &Ui,
        response: &Response,
        scene: &Scene,
        hidden: &BTreeSet<EntityIndex>,
        projection: &Projection,
        pane: Pane,
    ) -> Vec<BrushAction> {
        let ray = |at: Pos2| projection.ray(at);
        let mut actions = Vec::new();
        if response.clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
            let (start, direction) = ray(at);
            let (shift, alt, command) =
                ui.input(|i| (i.modifiers.shift, i.modifiers.alt, i.modifiers.command));
            if self.tool == MapTool::Vertex
                && command
                && let Some((brush, corner)) = self.corner_near(scene, projection, at)
                && self.chosen.first() == Some(&brush)
            {
                if let Some(place) = self.corner_place(corner) {
                    self.corners.remove(place);
                } else {
                    self.corners.push(corner);
                }
                return actions;
            }
            if alt {
                let mode = if shift {
                    PickMode::Toggle
                } else {
                    PickMode::Replace
                };
                self.pick_face((start, direction), scene, hidden, mode);
                return actions;
            }
            self.uv.clear();
            self.corners.clear();
            let picked = scene.pick_brush(start, direction, hidden);
            if let Some(brush) = picked
                && let Some(shape) = scene.brush(brush.0, brush.1)
            {
                self.seen.insert(brush, shape.vertices.clone());
            }
            match picked {
                Some(brush) if shift => {
                    if let Some(place) = self.chosen.iter().position(|&b| b == brush) {
                        self.chosen.remove(place);
                    } else {
                        self.chosen.push(brush);
                    }
                }
                Some(brush) => self.chosen = vec![brush],
                None if shift => {}
                None => self.chosen.clear(),
            }
        }
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(at) = ui.input(|i| i.pointer.press_origin())
        {
            self.drag = match self.tool {
                MapTool::Select => None,
                MapTool::Brush | MapTool::Clip | MapTool::Shape => {
                    at_height(ray(at), 0.0).map(Drag::Ground)
                }
                MapTool::Vertex => self
                    .corner_near(scene, projection, at)
                    .map(|(b, v)| Drag::Corner(b, v)),
                MapTool::Move => self.bounds(scene).and_then(|[min, max]| {
                    let mut middle = ORIGIN;
                    for ((m, low), high) in middle.iter_mut().zip(min).zip(max) {
                        *m = single((low + high) / 2.0);
                    }
                    let axis = MapAxis::facing(ray(at));
                    axis.meets(ray(at), middle)
                        .map(|from| Drag::Selection(from, axis))
                }),
                MapTool::Measure => {
                    let (start, direction) = ray(at);
                    let mut ahead = start;
                    for (a, d) in ahead.iter_mut().zip(direction) {
                        *a += d * MEASURE_AHEAD;
                    }
                    let axis = MapAxis::facing(ray(at));
                    axis.meets(ray(at), ahead)
                        .map(|from| Drag::Measure(from, axis))
                }
            };
            self.dragging_in = self.drag.map(|_| pane);
        }
        if self.dragging_in != Some(pane) {
            return actions;
        }
        if let Some(drag) = self.drag
            && let Some(at) = ui.ctx().pointer_latest_pos()
            && let Some(readout) = Self::readout(drag, ray(at))
        {
            self.shown = Some(readout);
        }
        if response.drag_stopped()
            && let Some(drag @ (Drag::Selection(..) | Drag::Measure(..))) = self.drag
        {
            self.drag = None;
            if let Some(at) = ui.ctx().pointer_latest_pos() {
                self.shown = Self::readout(drag, ray(at)).or(self.shown);
            }
            if let (Drag::Selection(..), Some(Readout::Move { by, .. })) = (drag, self.shown)
                && by.iter().any(|c| c.abs() > 0.0)
            {
                actions.extend(self.moved(by));
            }
        }
        if response.drag_stopped()
            && let Some(drag) = self.drag.take()
            && let Some(at) = ui.ctx().pointer_latest_pos()
        {
            let finished = self.finish(drag, ray(at));
            if let Some(
                BrushAction::Clip(entity, brush, ..) | BrushAction::Vertex(entity, brush, ..),
            ) = finished
            {
                self.reshaped = Some((entity, brush));
            }
            actions.extend(finished);
        }
        actions
    }

    /// Where `corner` is among the corners picked.
    #[expect(
        clippy::float_cmp,
        reason = "corners are copies of the scene's vertices: equal is the same corner"
    )]
    fn corner_place(&self, corner: [f64; 3]) -> Option<usize> {
        self.corners.iter().position(|&c| c == corner)
    }

    /// The corner of a selected brush nearest `at` in the view, within reach.
    fn corner_near(
        &self,
        scene: &Scene,
        projection: &Projection,
        at: Pos2,
    ) -> Option<(BrushRef, [f64; 3])> {
        let mut best: Option<(f32, BrushRef, [f64; 3])> = None;
        for &(entity, brush) in &self.chosen {
            let Some(shape) = scene.brush(entity, brush) else {
                continue;
            };
            for &corner in &shape.vertices {
                let Some(drawn) = projection.point(corner.map(single)) else {
                    continue;
                };
                let distance = drawn.distance(at);
                if distance <= VERTEX_REACH && best.is_none_or(|(d, _, _)| distance < d) {
                    best = Some((distance, (entity, brush), corner));
                }
            }
        }
        best.map(|(_, brush, corner)| (brush, corner))
    }

    /// The edit a drag ending on `ray` makes.
    #[expect(
        clippy::float_cmp,
        reason = "both ends are snapped to the grid: equal is the same grid point"
    )]
    fn finish(&self, drag: Drag, ray: Ray) -> Option<BrushAction> {
        match (self.tool, drag) {
            (MapTool::Brush, Drag::Ground(from)) => {
                let to = at_height(ray, from[2])?;
                let [min, max] = ground_box(from, to, BRUSH_HEIGHT);
                Some(BrushAction::Create(snapped(min), snapped(max)))
            }
            (MapTool::Shape, Drag::Ground(from)) => {
                let to = at_height(ray, from[2])?;
                let [min, max] = ground_box(from, to, SHAPE_HEIGHT);
                Some(BrushAction::Shape(
                    snapped(min),
                    snapped(max),
                    self.shape.spec(),
                ))
            }
            (MapTool::Clip, Drag::Ground(from)) => {
                let &(entity, brush) = self.chosen.first()?;
                let (a, b) = (snapped(from), snapped(at_height(ray, from[2])?));
                (a != b).then(|| {
                    let above = [b[0], b[1], b[2] + 1.0];
                    BrushAction::Clip(entity, brush, [a, b, above], self.keep)
                })
            }
            (MapTool::Vertex, Drag::Corner((entity, brush), from)) => {
                let to = at_height(ray, single(from[2]))?;
                let [x, y, _] = snapped(to);
                if self.corners.len() > 1 && self.corners.contains(&from) {
                    let by = [x - from[0], y - from[1]];
                    let moves = self
                        .corners
                        .iter()
                        .map(|&c| (c, [c[0] + by[0], c[1] + by[1], c[2]]))
                        .collect();
                    return Some(BrushAction::Vertices(entity, brush, moves));
                }
                Some(BrushAction::Vertex(entity, brush, from, [x, y, from[2]]))
            }
            (
                MapTool::Select
                | MapTool::Brush
                | MapTool::Clip
                | MapTool::Vertex
                | MapTool::Move
                | MapTool::Measure
                | MapTool::Shape,
                _,
            ) => None,
        }
    }

    /// Draws the selected brushes, the Vertex tool's corners and the drag under way.
    pub(crate) fn draw(
        &self,
        painter: &Painter,
        projection: &Projection,
        theme: &Theme,
        scene: &Scene,
        pointer: Option<Ray>,
    ) {
        let accent = Stroke::new(2.0, theme.color(Token::Accent));
        self.uv.draw(painter, projection, theme, scene);
        for &(entity, brush) in &self.chosen {
            let Some(shape) = scene.brush(entity, brush) else {
                continue;
            };
            for line in &shape.lines {
                projection.line(painter, *line, accent);
            }
            if self.tool == MapTool::Vertex {
                let picked = theme.color(Token::Focus);
                for &corner in &shape.vertices {
                    if let Some(at) = projection.point(corner.map(single)) {
                        if self.corners.contains(&corner) {
                            painter.circle_filled(at, VERTEX_DOT * 2.0, picked);
                        } else {
                            painter.circle_filled(at, VERTEX_DOT, accent.color);
                        }
                    }
                }
            }
        }
        let focus = Stroke::new(2.0, theme.color(Token::Focus));
        let showing = match self.tool {
            MapTool::Move | MapTool::Measure => true,
            MapTool::Vertex => self.drag.is_some(),
            MapTool::Select | MapTool::Brush | MapTool::Clip | MapTool::Shape => false,
        };
        if showing && let Some(readout) = self.shown {
            self.draw_readout(painter, projection, theme, scene, readout);
        }
        let Some(drag) = self.drag else {
            return;
        };
        let from = match drag {
            Drag::Ground(from) => snapped_f32(from),
            Drag::Corner(_, from) => from.map(single),
            Drag::Selection(..) | Drag::Measure(..) => return,
        };
        let Some(to) = pointer.and_then(|ray| at_height(ray, from[2])) else {
            return;
        };
        let to = match drag {
            Drag::Corner(..) => {
                let [x, y, _] = snapped_f32(to);
                [x, y, from[2]]
            }
            Drag::Ground(_) | Drag::Selection(..) | Drag::Measure(..) => snapped_f32(to),
        };
        match self.tool {
            MapTool::Brush => {
                for line in box_lines(ground_box(from, to, BRUSH_HEIGHT)) {
                    projection.line(painter, line, focus);
                }
            }
            MapTool::Shape => {
                for line in box_lines(ground_box(from, to, SHAPE_HEIGHT)) {
                    projection.line(painter, line, focus);
                }
            }
            MapTool::Clip => {
                let up = |p: [f32; 3]| [p[0], p[1], p[2] + CLIP_HEIGHT];
                for line in [
                    [from, to],
                    [to, up(to)],
                    [up(to), up(from)],
                    [up(from), from],
                ] {
                    projection.line(painter, line, focus);
                }
            }
            MapTool::Vertex => {
                projection.line(painter, [from, to], focus);
                if let Some(at) = projection.point(to) {
                    painter.circle_filled(at, VERTEX_DOT, focus.color);
                }
            }
            MapTool::Select | MapTool::Move | MapTool::Measure => {}
        }
    }

    /// Draws what the Move or Measure tool shows: the selection where it is going or the line
    /// measured, and the numbers in the view's lower left corner.
    fn draw_readout(
        &self,
        painter: &Painter,
        projection: &Projection,
        theme: &Theme,
        scene: &Scene,
        readout: Readout,
    ) {
        let focus = Stroke::new(2.0, theme.color(Token::Focus));
        match readout {
            Readout::Move { by, .. } if matches!(self.drag, Some(Drag::Selection(..))) => {
                let by = by.map(single);
                for &(entity, brush) in &self.chosen {
                    let Some(shape) = scene.brush(entity, brush) else {
                        continue;
                    };
                    for [a, b] in &shape.lines {
                        let shift = |p: [f32; 3]| [p[0] + by[0], p[1] + by[1], p[2] + by[2]];
                        projection.line(painter, [shift(*a), shift(*b)], focus);
                    }
                }
            }
            Readout::Move { .. } => {}
            Readout::Measure { from, to } => {
                let (from, to) = (from.map(single), to.map(single));
                projection.line(painter, [from, to], focus);
                for end in [from, to] {
                    if let Some(at) = projection.point(end) {
                        painter.circle_filled(at, VERTEX_DOT, focus.color);
                    }
                }
            }
        }
        let corner = painter.clip_rect().left_bottom();
        painter.text(
            egui::pos2(corner.x + READOUT_INSET, corner.y - READOUT_INSET),
            egui::Align2::LEFT_BOTTOM,
            readout.text(),
            egui::FontId::monospace(READOUT_SIZE),
            theme.color(Token::Text),
        );
    }

    /// The selected brushes' size, texture alignment and buttons in the Properties panel, the
    /// texture chosen in the browser `texture`; what they asked for.
    pub(crate) fn properties(
        &mut self,
        ui: &mut Ui,
        scene: &Scene,
        texture: Option<&str>,
        size_of: &dyn Fn(&str) -> Option<[u32; 2]>,
        entities: &[EntityIndex],
    ) -> Vec<BrushAction> {
        if !self.uv.is_empty() {
            let mut actions = self.uv.properties(ui, scene, size_of);
            actions.extend(self.face_properties(ui));
            return actions;
        }
        let mut actions = Vec::new();
        let Some(&(entity, first)) = self.chosen.first() else {
            return actions;
        };
        ui.label(egui::RichText::new(Text::MapBrushes.get()).strong());
        if let Some([min, max]) = self.bounds(scene) {
            ui.label(selection_size([
                max[0] - min[0],
                max[1] - min[1],
                max[2] - min[2],
            ]));
        }
        let mut by_entity: BTreeMap<EntityIndex, Vec<BrushIndex>> = BTreeMap::new();
        for &(e, b) in &self.chosen {
            by_entity.entry(e).or_default().push(b);
        }
        let change = |change: TextureChange| {
            by_entity
                .iter()
                .map(|(&e, brushes)| BrushAction::Texture(e, brushes.clone(), change.clone()))
                .collect::<Vec<_>>()
        };
        let align = &mut self.align;
        egui::Grid::new(ui.id().with(core::any::TypeId::of::<AlignFields>())).show(ui, |ui| {
            ui.label(Text::MapTextureShift.get());
            for shift in &mut align.shift {
                if ui
                    .add(egui::DragValue::new(shift).speed(ALIGN_STEP))
                    .changed()
                {
                    align.edited.insert(AlignPart::Shift);
                }
            }
            ui.end_row();
            ui.label(Text::MapTextureRotate.get());
            if ui
                .add(egui::DragValue::new(&mut align.rotate).speed(ALIGN_STEP))
                .changed()
            {
                align.edited.insert(AlignPart::Rotate);
            }
            ui.end_row();
            ui.label(Text::MapTextureScale.get());
            for scale in &mut align.scale {
                if ui
                    .add(egui::DragValue::new(scale).speed(SCALE_STEP))
                    .changed()
                {
                    align.edited.insert(AlignPart::Scale);
                }
            }
            ui.end_row();
        });
        ui.horizontal_wrapped(|ui| {
            let edited = !align.edited.is_empty();
            if ui
                .add_enabled(edited, egui::Button::new(Text::MapAlignTexture.get()))
                .clicked()
            {
                actions.extend(change(align.change()));
                align.edited.clear();
            }
            if let Some(name) = texture
                && ui.button(apply_texture(name)).clicked()
            {
                actions.extend(change(TextureChange {
                    texture: Some(name.to_owned()),
                    ..TextureChange::default()
                }));
            }
        });
        if !actions.is_empty() {
            return actions;
        }
        let one_entity = self.chosen.iter().all(|&(e, _)| e == entity);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.chosen.len() == 1,
                    egui::Button::new(Text::MapSubtract.get()),
                )
                .clicked()
            {
                actions.push(BrushAction::Subtract(entity, first));
            }
            let merge = self.chosen.len() >= 2 && one_entity;
            if ui
                .add_enabled(merge, egui::Button::new(Text::MapMerge.get()))
                .clicked()
            {
                let brushes = self.chosen.iter().map(|&(_, b)| b).collect();
                actions.push(BrushAction::Merge(entity, brushes));
            }
            if ui.button(Text::MapDeleteBrush.get()).clicked() {
                let mut doomed = self.chosen.clone();
                // Last first, so each index still names its brush when its turn comes.
                doomed.sort_by(|a, b| b.cmp(a));
                actions.extend(doomed.into_iter().map(|(e, b)| BrushAction::Delete(e, b)));
            }
        });
        if actions.is_empty() {
            let brushes: Vec<BrushIndex> = self
                .chosen
                .iter()
                .filter(|&&(e, _)| e == entity)
                .map(|&(_, b)| b)
                .collect();
            actions.extend(self.tool_properties(ui, entity, first, &brushes));
        }
        if actions.is_empty() && self.tool == MapTool::Vertex {
            actions.extend(self.corner_properties(ui));
        }
        if !actions.is_empty() {
            self.chosen.clear();
        }
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(Text::MapPrefabName.get());
            ui.add(
                egui::TextEdit::singleline(&mut self.prefab_name).desired_width(PREFAB_NAME_WIDTH),
            )
            .labelled_by(label.id);
            let named = !self.prefab_name.trim().is_empty();
            if ui
                .add_enabled(named, egui::Button::new(Text::MapSavePrefab.get()))
                .clicked()
            {
                actions.push(BrushAction::SavePrefab(
                    self.prefab_name.trim().to_owned(),
                    self.chosen.clone(),
                    entities.to_vec(),
                ));
            }
        });
        ui.separator();
        actions
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::panic,
        reason = "test code: a failed unwrap or a wrong variant is a failed test"
    )]

    use super::{
        AlignFields, AlignPart, BrushAction, BrushTools, Drag, MapAxis, MapTool, Readout,
        TextureChange, at_height,
    };
    use crate::map_scene::{BrushShape, Scene};
    use qs_workbench::map_edit::{BrushIndex, EntityIndex, KeepPart};

    fn down_from(x: f32, y: f32) -> super::Ray {
        ([x, y, 100.0], [0.0, 0.0, -1.0])
    }

    // No spec statement: P6-08: a ray meets the ground ahead of it, not behind or along it
    #[test]
    fn a_ray_meets_the_ground_ahead() {
        assert_eq!(at_height(down_from(3.0, 4.0), 0.0), Some([3.0, 4.0, 0.0]));
        assert_eq!(at_height(([0.0, 0.0, 100.0], [0.0, 0.0, 1.0]), 0.0), None);
        assert_eq!(at_height(([0.0, 0.0, 100.0], [1.0, 0.0, 0.0]), 0.0), None);
    }

    // No spec statement: P6-08: a box drawn on the ground snaps to the grid and is 64 tall
    #[test]
    fn a_drag_on_the_ground_draws_a_box() {
        let tools = BrushTools {
            tool: MapTool::Brush,
            ..BrushTools::default()
        };
        let made = tools.finish(Drag::Ground([70.0, 3.0, 0.0]), down_from(-20.0, 60.0));
        assert_eq!(
            made,
            Some(BrushAction::Create([-16.0, 0.0, 0.0], [64.0, 64.0, 64.0]))
        );
    }

    // No spec statement: P6-08: a cut line needs a selected brush and two grid points apart;
    // the plane is vertical through it, with the side chosen
    #[test]
    fn a_cut_line_clips_the_selected_brush() {
        let brush = (EntityIndex::new(0), BrushIndex::new(1));
        let mut tools = BrushTools {
            tool: MapTool::Clip,
            keep: KeepPart::Both,
            ..BrushTools::default()
        };
        let line = |tools: &BrushTools, to| tools.finish(Drag::Ground([0.0, 0.0, 0.0]), to);
        assert_eq!(line(&tools, down_from(64.0, 0.0)), None, "nothing selected");
        tools.chosen.push(brush);
        assert_eq!(
            line(&tools, down_from(3.0, 2.0)),
            None,
            "the same grid point"
        );
        let Some(BrushAction::Clip(_, _, points, keep)) = line(&tools, down_from(64.0, 0.0)) else {
            panic!("a clip");
        };
        assert_eq!(keep, KeepPart::Both);
        assert_eq!(points, [[0.0; 3], [64.0, 0.0, 0.0], [64.0, 0.0, 1.0]]);
    }

    // No spec statement: P6-18: dragging one of several picked corners moves them all by its
    // move, each at its height
    #[test]
    fn picked_corners_move_together() {
        let brush = (EntityIndex::new(0), BrushIndex::new(0));
        let tools = BrushTools {
            tool: MapTool::Vertex,
            corners: vec![[64.0, 64.0, 64.0], [0.0, 64.0, 32.0]],
            ..BrushTools::default()
        };
        assert_eq!(
            tools.finish(Drag::Corner(brush, [64.0; 3]), down_from(90.0, 70.0)),
            Some(BrushAction::Vertices(
                brush.0,
                brush.1,
                vec![
                    ([64.0, 64.0, 64.0], [96.0, 64.0, 64.0]),
                    ([0.0, 64.0, 32.0], [32.0, 64.0, 32.0])
                ]
            ))
        );
        assert!(matches!(
            tools.finish(Drag::Corner(brush, [0.0; 3]), down_from(90.0, 70.0)),
            Some(BrushAction::Vertex(..))
        ));
        assert_eq!(tools.corner_place([0.0, 64.0, 32.0]), Some(1));
        assert_eq!(tools.corner_place([1.0, 2.0, 3.0]), None);
    }

    // No spec statement: P6-08: a corner is dragged across the plane it lies in, to the grid
    #[test]
    fn a_corner_moves_at_its_height() {
        let brush = (EntityIndex::new(0), BrushIndex::new(0));
        let tools = BrushTools {
            tool: MapTool::Vertex,
            ..BrushTools::default()
        };
        let moved = tools
            .finish(
                Drag::Corner(brush, [64.0, 64.0, 64.0]),
                down_from(90.0, 70.0),
            )
            .unwrap();
        assert_eq!(
            moved,
            BrushAction::Vertex(brush.0, brush.1, [64.0; 3], [96.0, 64.0, 64.0])
        );
        let off_grid = tools
            .finish(
                Drag::Corner(brush, [64.0, 64.0, 10.0]),
                down_from(90.0, 70.0),
            )
            .unwrap();
        assert_eq!(
            off_grid,
            BrushAction::Vertex(brush.0, brush.1, [64.0, 64.0, 10.0], [96.0, 64.0, 10.0]),
            "a corner off the grid keeps its height"
        );
    }

    /// A scene holding one brush of the world at each place, with these corners.
    fn scene(corners: &[[f64; 3]]) -> Scene {
        let mut scene = Scene::default();
        for (place, &corner) in corners.iter().enumerate() {
            scene.brushes.push(BrushShape {
                entity: EntityIndex::new(0),
                brush: BrushIndex::new(place),
                lines: Vec::new(),
                vertices: vec![corner],
                planes: Vec::new(),
                textures: Vec::new(),
                faces: Vec::new(),
            });
        }
        scene
    }

    // No spec statement: P6-08: a selected brush stays selected while its place holds it; an
    // edit or undo that puts another brush there, or none, drops it, but the brush a clip or
    // vertex move reshaped stays
    #[test]
    fn the_selection_follows_the_brushes() {
        let (a, b) = ([0.0; 3], [64.0; 3]);
        let second = (EntityIndex::new(0), BrushIndex::new(1));
        let mut tools = BrushTools {
            chosen: vec![second],
            ..BrushTools::default()
        };
        tools.seen.insert(second, vec![b]);
        tools.keep_found(&scene(&[a, b]));
        assert_eq!(tools.chosen, vec![second], "unchanged");
        tools.keep_found(&scene(&[b, a]));
        assert!(tools.chosen.is_empty(), "another brush is there");
        assert!(tools.seen.is_empty());
        tools.chosen = vec![second];
        tools.seen.insert(second, vec![b]);
        tools.reshaped = Some(second);
        tools.keep_found(&scene(&[a, [32.0; 3]]));
        assert_eq!(tools.chosen, vec![second], "reshaped by the last edit");
        assert_eq!(tools.seen.get(&second), Some(&vec![[32.0; 3]]));
        assert_eq!(tools.reshaped, None);
        tools.keep_found(&scene(&[a]));
        assert!(tools.chosen.is_empty(), "gone");
    }

    // No spec statement: P6-09: a move or measure keeps the plane it started on, whatever way
    // the ray turns after
    #[test]
    fn a_drag_keeps_its_plane() {
        assert_eq!(MapAxis::facing(down_from(0.0, 0.0)), MapAxis::Z);
        assert_eq!(MapAxis::facing(([0.0; 3], [0.2, -0.9, 0.1])), MapAxis::Y);
        assert_eq!(MapAxis::facing(([0.0; 3], [-0.9, 0.2, 0.1])), MapAxis::X);
        // Started looking down; the pointer now runs nearly level, but the ground is kept.
        let level = ([0.0, 0.0, 100.0], [0.9, 0.0, -0.1]);
        let readout = BrushTools::readout(Drag::Selection([0.0; 3], MapAxis::Z), level);
        assert_eq!(
            readout,
            Some(Readout::Move {
                by: [896.0, 0.0, 0.0],
                at: [896.0, 0.0, 0.0]
            })
        );
        assert_eq!(
            MapAxis::Z.meets(([0.0, 0.0, 100.0], [0.0, 0.0, 1.0]), [0.0; 3]),
            None
        );
    }

    // No spec statement: P6-09: dragging a corner shows its move so far and where it is
    #[test]
    fn a_corner_drag_shows_its_move() {
        let brush = (EntityIndex::new(0), BrushIndex::new(0));
        let readout = BrushTools::readout(Drag::Corner(brush, [64.0; 3]), down_from(90.0, 70.0));
        assert_eq!(
            readout,
            Some(Readout::Move {
                by: [32.0, 0.0, 0.0],
                at: [96.0, 64.0, 64.0]
            })
        );
        assert_eq!(
            BrushTools::readout(Drag::Ground([0.0; 3]), down_from(1.0, 1.0)),
            None
        );
    }

    // No spec statement: P6-09, MAP-12.1: Align sets only the parts edited since the last one
    #[test]
    fn align_sets_only_what_was_edited() {
        let mut fields = AlignFields {
            rotate: 90.0,
            ..AlignFields::default()
        };
        assert_eq!(fields.change(), TextureChange::default());
        fields.edited.insert(AlignPart::Rotate);
        assert_eq!(
            fields.change(),
            TextureChange {
                rotate: Some(90.0),
                ..TextureChange::default()
            }
        );
        fields.edited.extend([AlignPart::Shift, AlignPart::Scale]);
        let all = fields.change();
        assert_eq!(
            (all.shift, all.scale, all.texture),
            (Some([0.0; 2]), Some([1.0; 2]), None)
        );
    }
}
