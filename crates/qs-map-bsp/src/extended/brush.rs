// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 1997       Greg Lewis
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/brush.cc, is_visible, CheckFace, CreateBrushWindings, LoadBrush, Brush_LoadEntity, update_bounds
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/map.cc, IsWorldBrushEntity, IsNonRemoveWorldBrushEntity

//! The brushes of an entity for one hull, ericw-tools' `bspbrush_t` (QBSP-15.22 to
//! QBSP-15.24): each map brush's sides, grown for a clipping hull, with their polygons and
//! bounds, and the contents the hull sees.

use std::collections::HashSet;

use qs_map::cnum::c_atoi;
use qs_map::{Entity, MapFile};

use super::game::{Contents, Game, MirrorInside};
use super::map::{
    ChopOrder, EntityNum, LoadedMap, MapBrush, Rotation, SideKind, SurfFlags, TexinfoNum, Texinfos,
};
use super::vec::{
    AXES, Bounds, POINT_EPSILON, Plane, PlaneNum, Planes, cross, dot, length, normalize, sub,
};
use super::winding::{KeepOn, Winding};
use crate::Hull;

/// The fewest points a side's polygon has.
const MIN_POINTS: usize = 3;
/// The entities whose brushes the world takes, in any case, `IsWorldBrushEntity` and
/// `IsNonRemoveWorldBrushEntity`.
const WORLD_BRUSH_ENTITIES: [&[u8]; 7] = [
    b"func_detail",
    b"func_group",
    b"func_detail_illusionary",
    b"func_detail_wall",
    b"func_detail_fence",
    b"func_illusionary_visblocker",
    b"func_areaportal",
];

/// Whether `entity`'s brushes belong to the world (QBSP-15.24).
pub(crate) fn is_world_brush_entity(entity: &Entity) -> bool {
    entity.classname().is_some_and(|c| {
        WORLD_BRUSH_ENTITIES
            .iter()
            .any(|w| c.eq_ignore_ascii_case(w))
    })
}

/// A face of the map: its entity, brush and place in the brush.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FaceRef {
    pub(crate) entity: EntityNum,
    pub(crate) brush: usize,
    pub(crate) face: usize,
}

/// The map faces some part of which can be seen, `mapface_t::visible`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct VisibleFaces(HashSet<FaceRef>);

impl VisibleFaces {
    /// Whether `face` can be seen.
    pub(crate) fn contains(&self, face: FaceRef) -> bool {
        self.0.contains(&face)
    }

    /// Marks `face` seen.
    pub(crate) fn show(&mut self, face: FaceRef) {
        self.0.insert(face);
    }

    /// Marks `face` unseen.
    pub(crate) fn hide(&mut self, face: FaceRef) {
        self.0.remove(&face);
    }
}

/// A brush side, `side_t`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Side {
    /// Its plane.
    pub(crate) planenum: PlaneNum,
    /// Its texinfo.
    pub(crate) texinfo: TexinfoNum,
    /// Whether it is a bevel.
    pub(crate) kind: SideKind,
    /// The map face it came from.
    pub(crate) source: Option<FaceRef>,
    /// Its polygon, empty when it has none.
    pub(crate) winding: Winding,
    /// What the tree build has noted of it.
    pub(crate) marks: SideMarks,
}

/// What the tree build notes of a side, `side_t::onnode` and `side_t::tested`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SideMarks(u8);

impl SideMarks {
    /// Its plane is a node's: it never splits again.
    pub(crate) const ON_NODE: u8 = 0b01;
    /// Its plane has been scored as a splitter for this node.
    pub(crate) const TESTED: u8 = 0b10;

    /// Whether any of `bits` is set.
    pub(crate) const fn has(self, bits: u8) -> bool {
        self.0 & bits != 0
    }

    /// The marks with `bits` set.
    pub(crate) const fn with(self, bits: u8) -> Self {
        Self(self.0 | bits)
    }

    /// The marks with `bits` cleared.
    pub(crate) const fn without(self, bits: u8) -> Self {
        Self(self.0 & !bits)
    }
}

impl Side {
    /// Whether it can be seen, `side_t::is_visible` (QBSP-15.23): a side not drawn only when a
    /// hint; any other when its map face is visible.
    pub(crate) fn is_visible(&self, texinfos: &Texinfos, visible: &VisibleFaces) -> bool {
        let flags = texinfos.flags(self.texinfo);
        if flags.has(SurfFlags::NODRAW) {
            return flags.has(SurfFlags::HINT);
        }
        self.source.is_some_and(|f| visible.contains(f))
    }

    /// The polygon checked as `CheckFace` checks it (QBSP-15.23): fewer than three points is
    /// none; a point closer than `-epsilon` to the next is dropped and the check starts again;
    /// a point more than `-epsilon` outside an edge makes the polygon none.
    fn check(&mut self, plane: Plane) {
        let mut points = self.winding.points().to_vec();
        'again: loop {
            let n = points.len();
            if n < MIN_POINTS {
                points.clear();
                break;
            }
            for (i, &p1) in points.iter().enumerate() {
                let Some(&p2) = i
                    .saturating_add(1)
                    .checked_rem(n)
                    .and_then(|k| points.get(k))
                else {
                    continue;
                };
                let edge = sub(p2, p1);
                if length(edge) < POINT_EPSILON {
                    points.remove(i);
                    continue 'again;
                }
                let edge_normal = normalize(cross(plane.normal, edge));
                let edge_dist = dot(p1, edge_normal) + POINT_EPSILON;
                let outside = points
                    .iter()
                    .enumerate()
                    .any(|(j, &p)| j != i && dot(p, edge_normal) > edge_dist);
                if outside {
                    points.clear();
                    break 'again;
                }
            }
            break;
        }
        self.winding = Winding::new(points);
    }
}

/// A map brush's place: its entity and its index there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct BrushRef {
    pub(crate) entity: EntityNum,
    pub(crate) brush: usize,
}

/// A brush for a hull, `bspbrush_t`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct BspBrush {
    /// Its sides.
    pub(crate) sides: Vec<Side>,
    /// What the hull sees in it.
    pub(crate) contents: Contents,
    /// The box around its sides' polygons.
    pub(crate) bounds: Bounds,
    /// The map brush it came from.
    pub(crate) source: BrushRef,
    /// The order it is chopped in, its map brush's `sort_key`.
    pub(crate) sort_key: (ChopOrder, u32),
    /// The hull brush it was cut from, `original_ptr`, its own number until it is cut.
    pub(crate) original: OriginalNum,
}

/// A brush's number in its hull's list as loaded and sorted, before any is chopped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct OriginalNum(usize);

impl OriginalNum {
    /// Its place in the list.
    pub(crate) const fn get(self) -> usize {
        self.0
    }
}

impl BspBrush {
    /// Its sides' polygons, `CreateBrushWindings` (QBSP-15.22): each side's plane's square
    /// clipped to the front of each other side's opposite plane but the bevels' within
    /// `-epsilon`, each map face with a polygon left marked visible and each without unmarked;
    /// a polygon reaching beyond the world's extent is none. Whether the bounds are sound.
    pub(crate) fn create_windings(
        &mut self,
        planes: &Planes,
        extent: f64,
        visible: &mut VisibleFaces,
    ) -> bool {
        let behind: Vec<(Option<Plane>, SideKind)> = self
            .sides
            .iter()
            .map(|s| (planes.get(s.planenum.opposite()).map(|p| p.plane()), s.kind))
            .collect();
        for (i, side) in self.sides.iter_mut().enumerate() {
            let mut w = planes
                .get(side.planenum)
                .and_then(|p| Winding::from_plane(p.plane(), extent));
            for (j, (plane, kind)) in behind.iter().enumerate() {
                if i == j || *kind == SideKind::Bevel {
                    continue;
                }
                let (Some(current), Some(plane)) = (w.take(), plane) else {
                    break;
                };
                w = current.clip(*plane, POINT_EPSILON, KeepOn::No).0;
            }
            let w = w.filter(|w| w.points().iter().flatten().all(|c| c.abs() <= extent));
            match (w, side.source) {
                (Some(w), source) => {
                    side.winding = w;
                    if let Some(f) = source {
                        visible.show(f);
                    }
                }
                (None, source) => {
                    side.winding = Winding::default();
                    if let Some(f) = source {
                        visible.hide(f);
                    }
                }
            }
        }
        self.update_bounds(extent)
    }

    /// The bounds of its sides' polygons, `update_bounds` (QBSP-15.22); whether they are within
    /// the world's extent on every axis and reach inside it.
    pub(crate) fn update_bounds(&mut self, extent: f64) -> bool {
        let mut bounds = Bounds::default();
        for side in &self.sides {
            bounds.union(&side.winding.bounds());
        }
        self.bounds = bounds;
        bounds
            .mins
            .iter()
            .zip(bounds.maxs)
            .all(|(lo, hi)| *lo > -extent && hi < extent && *lo < extent && hi > -extent)
    }
}

/// What an entity's classname makes its solid brushes in the drawing hull.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Detail {
    None,
    Solid,
    Wall,
    Fence,
    Illusionary,
}

impl Detail {
    /// `func_detail`, `func_detail_wall`, `func_detail_fence` or `func_detail_illusionary`, in
    /// any case.
    fn of(entity: &Entity) -> Self {
        let Some(classname) = entity.classname() else {
            return Self::None;
        };
        [
            (b"func_detail".as_slice(), Self::Solid),
            (b"func_detail_wall", Self::Wall),
            (b"func_detail_fence", Self::Fence),
            (b"func_detail_illusionary", Self::Illusionary),
        ]
        .into_iter()
        .find_map(|(name, detail)| classname.eq_ignore_ascii_case(name).then_some(detail))
        .unwrap_or(Self::None)
    }

    /// `contents` made this detail, `create_detail_*_contents`.
    const fn apply(self, contents: Contents) -> Contents {
        match self {
            Self::None => contents,
            Self::Solid => contents.detail_solid(),
            Self::Wall => contents.detail_wall(),
            Self::Fence => contents.detail_fence(),
            Self::Illusionary => contents.detail_illusionary(),
        }
    }
}

/// What a hull makes of a map brush (QBSP-15.24).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HullUse {
    /// Left out.
    Omitted,
    /// Only its bounds count: a clip brush in the drawing hull.
    Bounds(Contents),
    /// A brush of these contents.
    Brush(Contents),
}

impl HullUse {
    /// What `hull` makes of `mapbrush` in entity `dst`'s model, `Brush_LoadEntity`
    /// (QBSP-15.24): solid made detail by the entity's class in the drawing hull; detail
    /// illusionary in no clipping hull; clip only bounding the drawing hull and solid in the
    /// others; a hint brush empty in the drawing hull and in no other; a brush entity's brushes
    /// solid, as a detail fence; in a clipping hull only solid, sky and fences, all solid; the
    /// map brush's mirror and clipping flags kept.
    fn of(
        mapbrush: &MapBrush,
        detail: Detail,
        texinfos: &Texinfos,
        hull: Hull,
        dst: EntityNum,
        game: Game,
    ) -> Self {
        let solid = Contents::new(Contents::SOLID);
        let mut contents = mapbrush.contents;
        if hull == Hull::Draw && contents.is_any_solid() {
            contents = detail.apply(contents);
        }
        if hull != Hull::Draw && detail == Detail::Illusionary {
            return Self::Omitted;
        }
        if contents.is_clip() {
            if hull == Hull::Draw {
                return Self::Bounds(contents);
            }
            contents = solid;
        }
        if mapbrush.is_hint(texinfos) {
            if hull != Hull::Draw {
                return Self::Omitted;
            }
            contents = Contents::EMPTY;
        }
        if dst != EntityNum::WORLD && !game.bmodels_keep_contents() {
            contents = solid.detail_fence();
        }
        if hull != Hull::Draw {
            if !contents.is_any_solid() && !contents.is_sky() && !contents.is_fence() {
                return Self::Omitted;
            }
            contents = solid;
        }
        let mirror = mapbrush.contents.mirror_inside();
        if mirror != MirrorInside::Unset {
            contents = contents.with_mirrored(mirror);
        }
        if mapbrush.contents.suppresses_clipping_same_type() {
            contents = contents.without_clipping_same_type();
        }
        Self::Brush(contents)
    }
}

/// An entity's brushes for a hull and the box around them, the clip brushes' included.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HullBrushes {
    pub(crate) brushes: Vec<BspBrush>,
    pub(crate) bounds: Bounds,
}

impl HullBrushes {
    /// The brushes in chop order, `sort_key` then line, each its own original
    /// (`ProcessEntity`, QBSP-15.24).
    pub(crate) fn sorted(mut self) -> Self {
        self.brushes.sort_by_key(|b| b.sort_key);
        for (i, brush) in self.brushes.iter_mut().enumerate() {
            brush.original = OriginalNum(i);
        }
        self
    }
}

/// What loading brushes needs at hand.
pub(crate) struct Loader<'a> {
    pub(crate) file: &'a MapFile,
    pub(crate) map: &'a mut LoadedMap,
    pub(crate) game: Game,
    pub(crate) visible: &'a mut VisibleFaces,
}

impl Loader<'_> {
    /// A map brush for `hull` with `contents`, `LoadBrush` (QBSP-15.22): its sides but the
    /// bevels in the drawing hull; for a clipping hull each side's plane moved out by the hull
    /// box's corner its normal points to, unless its texture is not grown; its polygons, each
    /// checked; a Hipnotic rotating entity's bounds widened to every rotation.
    fn load_brush(&mut self, at: BrushRef, contents: Contents, hull: Hull) -> Option<BspBrush> {
        let entity = self.map.entities.get(at.entity.get())?;
        let source = entity.brushes.get(at.brush)?;
        let (origin, rotation) = (entity.origin, entity.rotation);
        let sort_key = source.sort_key();
        let mut sides: Vec<Side> = source
            .faces
            .iter()
            .enumerate()
            .filter(|(_, f)| hull != Hull::Draw || f.kind != SideKind::Bevel)
            .map(|(face, f)| Side {
                planenum: f.planenum,
                texinfo: f.texinfo,
                kind: f.kind,
                source: Some(FaceRef {
                    entity: at.entity,
                    brush: at.brush,
                    face,
                }),
                winding: Winding::default(),
                marks: SideMarks::default(),
            })
            .collect();
        if hull != Hull::Draw {
            let size = self.game.hull_size(hull);
            for side in &mut sides {
                if self
                    .map
                    .texinfos
                    .flags(side.texinfo)
                    .has(SurfFlags::NO_EXPAND)
                {
                    continue;
                }
                let Some(mut plane) = self.map.planes.get(side.planenum).map(|p| p.plane()) else {
                    continue;
                };
                let mut corner = [0.0; AXES];
                for (((c, n), lo), hi) in corner
                    .iter_mut()
                    .zip(plane.normal)
                    .zip(size.mins)
                    .zip(size.maxs)
                {
                    if n > 0.0 {
                        *c = hi;
                    } else if n < 0.0 {
                        *c = lo;
                    }
                }
                plane.dist += dot(corner, plane.normal);
                side.planenum = self.map.planes.add_or_find(plane);
                side.kind = SideKind::Given;
            }
        }
        let mut brush = BspBrush {
            sides,
            contents,
            bounds: Bounds::default(),
            source: at,
            sort_key,
            original: OriginalNum::default(),
        };
        if !brush.create_windings(&self.map.planes, self.map.world_extent, self.visible) {
            return None;
        }
        for side in &mut brush.sides {
            if let Some(plane) = self.map.planes.get(side.planenum) {
                side.check(plane.plane());
            }
        }
        if rotation == Rotation::Hipnotic && origin.iter().any(|c| *c != 0.0) {
            let delta = brush
                .bounds
                .mins
                .into_iter()
                .chain(brush.bounds.maxs)
                .fold(0.0, |m: f64, v| m.max(v.abs()));
            brush.bounds = Bounds {
                mins: [-delta; AXES],
                maxs: [delta; AXES],
            };
        }
        Some(brush)
    }

    /// The brushes of entity `src` for `hull`, as the world's when `dst` is 0,
    /// `Brush_LoadEntity` (QBSP-15.24).
    fn load_from(&mut self, dst: EntityNum, src: EntityNum, hull: Hull, out: &mut HullBrushes) {
        let Some(entity) = self.file.entities.get(src.get()) else {
            return;
        };
        let detail = Detail::of(entity);
        let super_detail = entity
            .value(b"_super_detail")
            .is_some_and(|v| c_atoi(v) != 0);
        let count = self
            .map
            .entities
            .get(src.get())
            .map_or(0, |e| e.brushes.len());
        for index in 0..count {
            let Some(mapbrush) = self
                .map
                .entities
                .get(src.get())
                .and_then(|e| e.brushes.get(index))
            else {
                continue;
            };
            if hull == Hull::Draw && super_detail {
                continue;
            }
            let use_in_hull =
                HullUse::of(mapbrush, detail, &self.map.texinfos, hull, dst, self.game);
            let contents = match use_in_hull {
                HullUse::Omitted => continue,
                HullUse::Bounds(contents) | HullUse::Brush(contents) => contents,
            };
            let at = BrushRef {
                entity: src,
                brush: index,
            };
            let Some(brush) = self.load_brush(at, contents, hull) else {
                continue;
            };
            out.bounds.union(&brush.bounds);
            if let HullUse::Brush(_) = use_in_hull {
                out.brushes.push(brush);
            }
        }
    }

    /// Entity `entity`'s brushes for `hull` (QBSP-15.24): its own, and for the world those of
    /// every world brush entity after it.
    pub(crate) fn load_entity(&mut self, entity: EntityNum, hull: Hull) -> HullBrushes {
        let mut out = HullBrushes::default();
        self.load_from(entity, entity, hull, &mut out);
        if entity == EntityNum::WORLD {
            for source in 1..self.file.entities.len() {
                if self
                    .file
                    .entities
                    .get(source)
                    .is_some_and(is_world_brush_entity)
                {
                    self.load_from(EntityNum::WORLD, EntityNum::new(source), hull, &mut out);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
