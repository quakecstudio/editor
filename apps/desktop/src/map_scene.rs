// SPDX-License-Identifier: GPL-3.0-or-later

//! What the map editor draws and picks (P5-06): the open map as lines in map space (a `.map`'s
//! brushes, a BSP's faces), each entity's box from its class or its brushes, the wireframes of
//! the classes' models, and where the camera starts.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use qs_bsp::{Bsp, BspLoader};
use qs_mdl::Mdl;
use qs_mdl::mdl::MdlFrameEntry;
use qs_render::Camera;
use qs_vfs::{StdFs, Vfs};
use qs_workbench::entity_classes::ClassInfo;
use qs_workbench::map_edit::{BrushIndex, EntityIndex, MapDocument, MapSource};

/// A line in map space.
pub(crate) type Line = [[f32; 3]; 2];

/// The box drawn for a point entity whose class gives none: id's `info_null`'s.
const DEFAULT_BOX: [[f32; 3]; 2] = [[-8.0; 3], [8.0; 3]];

/// How far behind and above the player start the camera starts, so the start's own box is in
/// view rather than around the eye.
const START_BACK: f32 = 128.0;
const START_UP: f32 = 64.0;
/// How far down the camera looks at first, in degrees.
const START_PITCH: f32 = 15.0;

/// The classname of the player's start, where the camera starts.
const PLAYER_START: &[u8] = b"info_player_start";
/// The key an entity is fired by, which the keys the code fires entities by name (MAP-8.8).
pub(crate) const TARGETNAME: &str = "targetname";
/// The key holding an entity's spawnflags (EDICT-8.4).
pub(crate) const SPAWNFLAGS: &str = "spawnflags";
/// The key holding a point entity's yaw, or -1 for up and -2 for down (`SetMovedir`).
pub(crate) const ANGLE: &str = "angle";
/// The end of a key holding a colour as three numbers: `_color`, `color`.
pub(crate) const COLOR_SUFFIX: &str = "color";
/// How `project.entities` names a float key's type.
pub(crate) const FLOAT_TYPE: &str = "float";

/// How an entity class is drawn: its box, colour and model.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ClassLook {
    /// Its box around the origin.
    pub(crate) size: Option<[[f32; 3]; 2]>,
    /// Its colour, each component 0 to 1.
    pub(crate) color: Option<[f32; 3]>,
    /// Its model.
    pub(crate) model: Option<String>,
    /// Whether the class is made of brushes rather than placed at a point.
    pub(crate) brush: bool,
}

/// How `project.entities` names a brush class's shape.
const BRUSH_SHAPE: &str = "brush";

impl From<&ClassInfo> for ClassLook {
    fn from(class: &ClassInfo) -> Self {
        Self {
            size: class.size,
            color: class.color,
            model: class.model.clone(),
            brush: class.shape == BRUSH_SHAPE,
        }
    }
}

/// One entity as drawn and picked.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EntityShape {
    /// Its place in the file.
    pub(crate) index: EntityIndex,
    /// Its box in map space.
    pub(crate) bounds: [[f32; 3]; 2],
    /// Its origin.
    pub(crate) origin: [f32; 3],
    /// Its yaw in degrees.
    pub(crate) yaw: f32,
    /// Its class's colour.
    pub(crate) color: Option<[f32; 3]>,
    /// Its class's model.
    pub(crate) model: Option<String>,
    /// Whether it has brushes.
    pub(crate) brush: bool,
    /// Its `targetname`, when it has one.
    pub(crate) targetname: Option<String>,
    /// The values of the keys it fires entities by (MAP-8.8).
    pub(crate) targets: Vec<String>,
}

impl EntityShape {
    /// The middle of its box, where its links start and end.
    pub(crate) fn centre(&self) -> [f32; 3] {
        let [min, max] = self.bounds;
        [
            (min[0] + max[0]) / 2.0,
            (min[1] + max[1]) / 2.0,
            (min[2] + max[2]) / 2.0,
        ]
    }
}

/// The open map as drawn: its world and brush lines and its entities.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Scene {
    /// The world's and the brush entities' lines, each with the entity it is drawn for: a BSP's
    /// faces are the world's.
    pub(crate) lines: Vec<(EntityIndex, Line)>,
    /// Every entity but the world.
    pub(crate) entities: Vec<EntityShape>,
    /// The values each key holds across the map, the choices its editor offers.
    pub(crate) values: BTreeMap<String, BTreeSet<String>>,
    /// Each link: an entity, one its keys fire, and the line between their centres.
    pub(crate) fired: Vec<(EntityIndex, EntityIndex, Line)>,
    /// Every brush of a `.map`, for picking and editing (P6-08).
    pub(crate) brushes: Vec<BrushShape>,
}

/// One brush as drawn, picked and edited (P6-08).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BrushShape {
    /// The entity holding it.
    pub(crate) entity: EntityIndex,
    /// Its place among the entity's brushes.
    pub(crate) brush: BrushIndex,
    /// Its edges.
    pub(crate) lines: Vec<Line>,
    /// Its corners.
    pub(crate) vertices: Vec<[f32; 3]>,
    /// Its faces' planes, each a normal pointing out and a distance: the solid behind them all.
    planes: Vec<([f32; 3], f32)>,
}

impl BrushShape {
    /// How far along the ray from `start` along `direction` it enters the brush's solid, 0
    /// when it starts inside; none when it misses.
    fn entered(&self, start: [f32; 3], direction: [f32; 3]) -> Option<f32> {
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let mut near = 0.0_f32;
        let mut far = f32::INFINITY;
        for &(normal, dist) in &self.planes {
            let towards = dot(normal, direction);
            let ahead = dist - dot(normal, start);
            if towards.abs() < f32::EPSILON {
                if ahead < 0.0 {
                    return None;
                }
                continue;
            }
            let t = ahead / towards;
            if towards < 0.0 {
                near = near.max(t);
            } else {
                far = far.min(t);
            }
            if near > far {
                return None;
            }
        }
        Some(near)
    }
}

impl Scene {
    /// The scene of `document`, its point entities sized by `classes`, linked by the keys in
    /// `targeting`.
    #[must_use]
    pub(crate) fn build(
        document: &MapDocument,
        classes: &HashMap<String, ClassLook>,
        targeting: &[String],
    ) -> Self {
        let mut scene = Self::default();
        let bsp = match document.source() {
            MapSource::Bsp(bytes) => Bsp::read(bytes, BspLoader::default()).ok(),
            MapSource::Map => None,
        };
        if let Some(bsp) = &bsp {
            let mut lines = Vec::new();
            for face in &bsp.faces {
                outline(&mut lines, &bsp.face_vertices(face));
            }
            let world = EntityIndex::new(0);
            scene
                .lines
                .extend(lines.into_iter().map(|line| (world, line)));
        }
        for (index, entity) in document.file().entities.iter().enumerate() {
            let mut bounds: Option<[[f32; 3]; 2]> = None;
            let mut lines = Vec::new();
            for (number, brush) in entity.brushes().enumerate() {
                let mut shape = BrushShape {
                    entity: EntityIndex::new(index),
                    brush: BrushIndex::new(number),
                    lines: Vec::new(),
                    vertices: brush.vertices().iter().map(|v| v.map(single)).collect(),
                    planes: brush
                        .faces
                        .iter()
                        .filter_map(qs_map::Face::plane)
                        .map(|p| (p.normal.map(single), single(p.dist)))
                        .collect(),
                };
                for polygon in brush.polygons() {
                    let points: Vec<[f32; 3]> = polygon.iter().map(|p| p.map(single)).collect();
                    for &point in &points {
                        bounds = Some(grow(bounds, point));
                    }
                    outline(&mut shape.lines, &points);
                }
                lines.extend(shape.lines.iter().copied());
                if !shape.lines.is_empty() {
                    scene.brushes.push(shape);
                }
            }
            let owner = EntityIndex::new(index);
            scene
                .lines
                .extend(lines.into_iter().map(|line| (owner, line)));
            let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
            for pair in entity.pairs() {
                scene
                    .values
                    .entry(text(pair.key()))
                    .or_default()
                    .insert(text(pair.value()));
            }
            if index == 0 {
                continue;
            }
            let class = entity
                .classname()
                .map(|name| String::from_utf8_lossy(name).into_owned())
                .and_then(|name| classes.get(&name));
            let origin = entity.origin().map(single);
            // A brush entity read from a BSP draws its submodel, whose box the BSP holds.
            let submodel = entity
                .submodel()
                .and_then(|n| bsp.as_ref()?.models.get(n))
                .map(|model| [add(origin, model.mins), add(origin, model.maxs)]);
            let bounds = bounds.or(submodel).unwrap_or_else(|| {
                let [min, max] = class.and_then(|c| c.size).unwrap_or(DEFAULT_BOX);
                [add(origin, min), add(origin, max)]
            });
            scene.entities.push(EntityShape {
                index: EntityIndex::new(index),
                bounds,
                origin,
                yaw: single(entity.yaw()),
                color: class.and_then(|c| c.color),
                model: class.and_then(|c| c.model.clone()),
                brush: entity.is_brush_entity(),
                targetname: entity
                    .value(TARGETNAME.as_bytes())
                    .filter(|name| !name.is_empty())
                    .map(text),
                targets: targeting
                    .iter()
                    .filter_map(|key| entity.value(key.as_bytes()))
                    .filter(|name| !name.is_empty())
                    .map(text)
                    .collect(),
            });
        }
        scene.fired = scene.find_links();
        scene
    }

    /// Every link between the entities, found through an index of their targetnames.
    fn find_links(&self) -> Vec<(EntityIndex, EntityIndex, Line)> {
        let mut named: HashMap<&str, Vec<&EntityShape>> = HashMap::new();
        for shape in &self.entities {
            if let Some(name) = &shape.targetname {
                named.entry(name).or_default().push(shape);
            }
        }
        let mut fired = Vec::new();
        for from in &self.entities {
            let mut targets: Vec<&str> = from.targets.iter().map(String::as_str).collect();
            targets.sort_unstable();
            targets.dedup();
            for to in targets
                .into_iter()
                .filter_map(|name| named.get(name))
                .flatten()
            {
                if to.index != from.index {
                    fired.push((from.index, to.index, [from.centre(), to.centre()]));
                }
            }
        }
        fired
    }

    /// The entity not in `hidden` whose box the ray from `start` along `direction` enters
    /// first; one whose box holds `start`, such as a trigger the camera is in, only when it
    /// enters none.
    #[must_use]
    pub(crate) fn pick(
        &self,
        start: [f32; 3],
        direction: [f32; 3],
        hidden: &BTreeSet<EntityIndex>,
    ) -> Option<EntityIndex> {
        self.entities
            .iter()
            .filter(|shape| !hidden.contains(&shape.index))
            .filter_map(|shape| {
                hit(start, direction, shape.bounds).map(|t| (t <= 0.0, t, shape.index))
            })
            .min_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)))
            .map(|(_, _, index)| index)
    }

    /// The brush the ray from `start` along `direction` enters first, by its faces, none in
    /// `hidden`.
    #[must_use]
    pub(crate) fn pick_brush(
        &self,
        start: [f32; 3],
        direction: [f32; 3],
        hidden: &BTreeSet<EntityIndex>,
    ) -> Option<(EntityIndex, BrushIndex)> {
        self.brushes
            .iter()
            .filter(|shape| !hidden.contains(&shape.entity))
            .filter_map(|shape| shape.entered(start, direction).map(|t| (t, shape)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, shape)| (shape.entity, shape.brush))
    }

    /// The shape of `brush` of `entity`.
    #[must_use]
    pub(crate) fn brush(&self, entity: EntityIndex, brush: BrushIndex) -> Option<&BrushShape> {
        self.brushes
            .iter()
            .find(|shape| shape.entity == entity && shape.brush == brush)
    }

    /// The links to draw, each from an entity to one its keys fire: every link, or only those
    /// with an end in `selection`; none to or from an entity in `hidden`.
    #[must_use]
    pub(crate) fn links(
        &self,
        view: LinkView,
        selection: &BTreeSet<EntityIndex>,
        hidden: &BTreeSet<EntityIndex>,
    ) -> Vec<Line> {
        self.fired
            .iter()
            .filter(|(from, to, _)| !hidden.contains(from) && !hidden.contains(to))
            .filter(|(from, to, _)| match view {
                LinkView::All => true,
                LinkView::Selection => selection.contains(from) || selection.contains(to),
            })
            .map(|(_, _, line)| *line)
            .collect()
    }
}

/// Which links between entities the map view draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LinkView {
    /// Those to and from the selection.
    #[default]
    Selection,
    /// Every one.
    All,
}

/// The map's origin, where the camera starts in a map with no player start.
const MAP_ORIGIN: [f32; 3] = [0.0; 3];
/// The yaw the camera starts at in a map with no player start, in degrees: along +y.
const NO_START_YAW: f32 = 90.0;

/// Where the camera starts: behind and above the first player start, facing as it does and
/// looking down at it, else at the map's origin.
#[must_use]
pub(crate) fn start_camera(document: &MapDocument) -> Camera {
    let start = document
        .file()
        .entities
        .iter()
        .find(|entity| entity.classname() == Some(PLAYER_START));
    let mut camera = Camera::new(MAP_ORIGIN, NO_START_YAW);
    if let Some(start) = start {
        let origin = start.origin().map(single);
        camera.yaw = single(start.yaw());
        let (sin, cos) = camera.yaw.to_radians().sin_cos();
        camera.origin = [
            origin[0] - cos * START_BACK,
            origin[1] - sin * START_BACK,
            origin[2] + START_UP,
        ];
        camera.pitch = START_PITCH;
    }
    camera
}

/// The wireframe of each class model the project's folder or the game holds: the edges of its
/// first frame's triangles (MDL-4, MDL-5).
#[must_use]
pub(crate) fn load_models(
    classes: &HashMap<String, ClassLook>,
    project: Option<&Path>,
    game: Option<&Path>,
) -> HashMap<String, Vec<Line>> {
    let vfs = game.and_then(|base| {
        let base = base.to_string_lossy().into_owned();
        Vfs::new(StdFs, &["-basedir", base.as_str()], ".").ok()
    });
    let mut models = HashMap::new();
    for model in classes.values().filter_map(|class| class.model.as_ref()) {
        if models.contains_key(model) {
            continue;
        }
        let bytes = project
            .and_then(|root| std::fs::read(root.join(model)).ok())
            .or_else(|| {
                vfs.as_ref()
                    .and_then(|vfs| vfs.load(model.as_bytes()).ok().flatten())
                    .map(qs_vfs::LoadedFile::into_bytes)
            });
        if let Some(lines) = bytes.as_deref().and_then(mdl_lines) {
            models.insert(model.clone(), lines);
        }
    }
    models
}

/// The edges of an MDL's first frame's triangles, in the model's space.
fn mdl_lines(bytes: &[u8]) -> Option<Vec<Line>> {
    let mdl = Mdl::read(bytes).ok()?;
    let frame = match mdl.frames.first()? {
        MdlFrameEntry::Single(frame) => frame,
        MdlFrameEntry::Group { frames, .. } => frames.first()?,
    };
    let mut lines = Vec::new();
    for triangle in &mdl.triangles {
        let corners: Vec<[f32; 3]> = triangle
            .vertindex
            .iter()
            .filter_map(|&index| frame.vertex(index))
            .map(|vertex| mdl.position(vertex))
            .collect();
        outline(&mut lines, &corners);
    }
    Some(lines)
}

/// The lines around `points`, a closed polygon.
fn outline(lines: &mut Vec<Line>, points: &[[f32; 3]]) {
    for (index, &point) in points.iter().enumerate() {
        let next = points
            .get(index.saturating_add(1))
            .or_else(|| points.first())
            .copied()
            .unwrap_or(point);
        lines.push([point, next]);
    }
}

/// Where along the ray from `start` along `direction` it enters `bounds`, by the slab test.
fn hit(start: [f32; 3], direction: [f32; 3], [min, max]: [[f32; 3]; 2]) -> Option<f32> {
    let mut near = 0.0_f32;
    let mut far = f32::INFINITY;
    for axis in 0..start.len() {
        let (s, d) = (start.get(axis)?, direction.get(axis)?);
        let (lo, hi) = (min.get(axis)?, max.get(axis)?);
        if d.abs() < f32::EPSILON {
            if s < lo || s > hi {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo - s) / d, (hi - s) / d);
        near = near.max(a.min(b));
        far = far.min(a.max(b));
        if near > far {
            return None;
        }
    }
    Some(near)
}

fn grow(bounds: Option<[[f32; 3]; 2]>, point: [f32; 3]) -> [[f32; 3]; 2] {
    match bounds {
        None => [point, point],
        Some([min, max]) => [
            [
                min[0].min(point[0]),
                min[1].min(point[1]),
                min[2].min(point[2]),
            ],
            [
                max[0].max(point[0]),
                max[1].max(point[1]),
                max[2].max(point[2]),
            ],
        ],
    }
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// A map coordinate as the renderer takes it.
#[expect(
    clippy::cast_possible_truncation,
    reason = "map coordinates lie far inside f32's range"
)]
const fn single(value: f64) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests;
