// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 1997       Greg Lewis
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/map.cc, set_planepts, ParseBrushFace, ParseBrush, ParseEntity, Brush_GetContents, IsSkipName, IsNoExpandName, IsSpecialName, IsHintName, SurfFlagsForEntity, FindMiptex, FindTexinfo, CalculateBrushBounds, AddBrushBevels, GetBrushExtents, CalculateWorldExtent, ProcessMapBrushes
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/brush.cc, FindTargetEntity, FixRotateOrigin
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/qbsp.cc, MakeSkipTexinfo
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: include/common/qvec.hh, Snap

//! Map brushes as ericw-tools' qbsp reads them from a map (QBSP-15.14 to QBSP-15.21): each
//! face's plane in double precision, its texinfo and surface flags, each brush's contents,
//! bounds and bevels, origin brushes and the world's extent.

use qs_map::cnum::{c_atof, c_atoi, scan_vector};
use qs_map::geometry::{TexVecs, texvecs};
use qs_map::{Entity, Face, MapFile};

use super::game::{Contents, Game, MirrorInside};
use super::vec::{
    AXES, Bounds, BspPlane, Facing, NORMAL_EPSILON, POINT_EPSILON, Plane, PlaneNum, Planes, Vec3,
    cross, dot, normalize_with_length, sub,
};
use super::winding::{self, KeepOn, Winding};

/// `SIDESPACE`: the room the world's extent leaves beyond the brushes and hulls (`qbsp.hh`).
pub(crate) const SIDESPACE: f64 = 24.0;
/// `ANGLEEPSILON`: below this sine an edge is parallel to an axis (`mathlib.hh`).
const ANGLE_EPSILON: f64 = NORMAL_EPSILON;
/// An edge shorter than this gets no bevel, `normalizeInPlace(vec) < 0.5`.
const SHORTEST_BEVELLED_EDGE: f64 = 0.5;
/// A box's sides: a brush with no more is axial and needs no edge bevels.
const BOX_SIDES: usize = 6;
/// Twice: the world's extent reaches twice the farthest point.
const TWICE: f64 = 2.0;
/// The texture names that are skip, in any case, Quake's `IsSkipName`.
const SKIP_NAMES: [&[u8]; 6] = [
    b"skip",
    b"*waterskip",
    b"*slimeskip",
    b"*lavaskip",
    b"bevel",
    b"null",
];
/// The texture name that is not grown for the clipping hulls, `IsNoExpandName`.
const NO_EXPAND_NAME: &[u8] = b"bevel";
/// The texture names that are hint, in any case, `IsHintName`.
const HINT_NAMES: [&[u8]; 2] = [b"hint", b"hintskip"];
/// The texture the compile's own sides have.
const SKIP_TEXTURE: &[u8] = b"skip";
/// The texture axes of a face that has none.
const NO_TEXVECS: TexVecs<f32> = [[0.0; 4]; 2];
/// The prefix of a liquid's texture, special unless lit water is asked for.
const LIQUID_PREFIX: u8 = b'*';
/// The prefix of a sky's texture, in any case.
const SKY_PREFIX: &[u8] = b"sky";
/// The classname prefix of a Hipnotic rotating entity, in any case.
const HIPNOTIC_ROTATE_PREFIX: &[u8] = b"rotate_";
/// The classname of an entity whose brushes block sight but are not drawn or solid.
const VISBLOCKER: &[u8] = b"func_illusionary_visblocker";

/// A face's surface flags that the compile reads, ericw-tools' `surfflags_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SurfFlags(u8);

impl SurfFlags {
    /// Not drawn: a skip texture.
    pub(crate) const NODRAW: u8 = 0b0001;
    /// A hint face, which splits the tree but is not drawn.
    pub(crate) const HINT: u8 = 0b0010;
    /// A hint brush's other faces, which are discarded.
    pub(crate) const HINTSKIP: u8 = 0b0100;
    /// `TEX_SPECIAL`: not lightmapped and not subdivided.
    pub(crate) const SPECIAL: u8 = 0b1000;
    /// Not grown for the clipping hulls: a `bevel` texture.
    pub(crate) const NO_EXPAND: u8 = 0b1_0000;

    /// Whether any of `bits` is set.
    pub(crate) const fn has(self, bits: u8) -> bool {
        self.0 & bits != 0
    }

    const fn with(self, bits: u8) -> Self {
        Self(self.0 | bits)
    }

    const fn without(self, bits: u8) -> Self {
        Self(self.0 & !bits)
    }
}

/// A texinfo: texture axes, texture and surface flags, `maptexinfo_t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Texinfo {
    /// The s and t axes and offsets, in single precision as the BSP stores them.
    pub(crate) vecs: TexVecs<f32>,
    /// The texture, an index into [`Texinfos::miptex`].
    pub(crate) miptex: usize,
    /// Its surface flags.
    pub(crate) flags: SurfFlags,
}

impl Texinfo {
    /// Whether it is exactly `other`, every axis value's bits the same, as the texinfo
    /// lookup's ordering compares them.
    fn is(&self, other: &Self) -> bool {
        self.miptex == other.miptex
            && self.flags == other.flags
            && self
                .vecs
                .iter()
                .flatten()
                .zip(other.vecs.iter().flatten())
                .all(|(a, b)| a.to_bits() == b.to_bits())
    }
}

/// A texinfo's number in [`Texinfos`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct TexinfoNum(usize);

/// The textures and texinfo the map uses, `mapdata_t::miptex` and `mtexinfos`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Texinfos {
    miptex: Vec<Vec<u8>>,
    texinfo: Vec<Texinfo>,
}

impl Texinfos {
    /// The skip texinfo the compile gives the sides it makes, `map.skip_texinfo`.
    pub(crate) const SKIP: TexinfoNum = TexinfoNum(0);

    /// The table ericw-tools' qbsp starts with, `MakeSkipTexinfo`: the texture `skip` and
    /// [`Texinfos::SKIP`], not drawn, with no axes (QBSP-15.16).
    pub(crate) fn with_skip() -> Self {
        Self {
            miptex: vec![SKIP_TEXTURE.to_vec()],
            texinfo: vec![Texinfo {
                vecs: NO_TEXVECS,
                miptex: 0,
                flags: SurfFlags::default().with(SurfFlags::NODRAW),
            }],
        }
    }

    /// The textures' names, in first-use order.
    pub(crate) fn miptex(&self) -> &[Vec<u8>] {
        &self.miptex
    }

    /// The texinfo `num`.
    pub(crate) fn get(&self, num: TexinfoNum) -> Option<&Texinfo> {
        self.texinfo.get(num.0)
    }

    /// The flags of texinfo `num`, none when there is no such texinfo.
    pub(crate) fn flags(&self, num: TexinfoNum) -> SurfFlags {
        self.get(num).map(|t| t.flags).unwrap_or_default()
    }

    /// `name`'s texture, `FindMiptex` for Quake (QBSP-15.16): the part after the last `/`,
    /// found in any case or added.
    fn find_miptex(&mut self, name: &[u8]) -> usize {
        let name = name
            .iter()
            .rposition(|&b| b == b'/')
            .and_then(|slash| name.get(slash.saturating_add(1)..))
            .unwrap_or(name);
        if let Some(found) = self
            .miptex
            .iter()
            .position(|m| m.eq_ignore_ascii_case(name))
        {
            return found;
        }
        self.miptex.push(name.to_vec());
        self.miptex.len().saturating_sub(1)
    }

    /// `texinfo`'s number, added when no texinfo is exactly it, `FindTexinfo` (QBSP-15.16).
    fn find(&mut self, texinfo: Texinfo) -> TexinfoNum {
        if let Some(found) = self.texinfo.iter().position(|t| t.is(&texinfo)) {
            return TexinfoNum(found);
        }
        self.texinfo.push(texinfo);
        TexinfoNum(self.texinfo.len().saturating_sub(1))
    }

    /// `num` with its flags changed by `change`.
    fn reflagged(
        &mut self,
        num: TexinfoNum,
        change: impl Fn(SurfFlags) -> SurfFlags,
    ) -> TexinfoNum {
        match self.get(num).copied() {
            Some(texinfo) => self.find(Texinfo {
                flags: change(texinfo.flags),
                ..texinfo
            }),
            None => num,
        }
    }
}

/// Where a brush side came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SideKind {
    /// A face of the map.
    Given,
    /// A bevel `AddBrushBevels` added, never a splitter of the drawing hull.
    Bevel,
}

/// A brush side, `mapface_t`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MapFace {
    /// Its plane.
    pub(crate) planenum: PlaneNum,
    /// Its texinfo.
    pub(crate) texinfo: TexinfoNum,
    /// The texture name as the map writes it.
    pub(crate) texture: Vec<u8>,
    /// Its polygon on the brush, empty when it has none.
    pub(crate) winding: Winding,
    /// Where it came from.
    pub(crate) kind: SideKind,
    /// The line it is on.
    pub(crate) line: u32,
}

/// A brush's `_chop_order`: a brush of a higher order takes what it overlaps from a lower.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ChopOrder(i32);

/// The chop order of `"_chop" "0"`, which puts a brush after the others.
const UNCHOPPED: i32 = 1;

/// A brush, `mapbrush_t`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MapBrush {
    /// Its sides, the axial ones first once bevelled.
    pub(crate) faces: Vec<MapFace>,
    /// Its contents.
    pub(crate) contents: Contents,
    /// The box around its sides' polygons.
    pub(crate) bounds: Bounds,
    /// The line of its `{`.
    pub(crate) line: u32,
    /// The order it is chopped in.
    pub(crate) chop_order: ChopOrder,
}

impl MapBrush {
    /// The order the brushes are chopped in, `sort_key`.
    pub(crate) const fn sort_key(&self) -> (ChopOrder, u32) {
        (self.chop_order, self.line)
    }

    /// Whether a side is a hint, `MapBrush_IsHint`.
    pub(crate) fn is_hint(&self, texinfos: &Texinfos) -> bool {
        self.faces
            .iter()
            .any(|f| texinfos.flags(f.texinfo).has(SurfFlags::HINT))
    }
}

/// An entity's place in the map, the world first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct EntityNum(usize);

impl EntityNum {
    /// The world, the map's first entity.
    pub(crate) const WORLD: Self = Self(0);

    /// The entity at `index`.
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Its place in the map.
    pub(crate) const fn get(self) -> usize {
        self.0
    }
}

/// How an entity's brushes are placed about its origin, `rotation_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rotation {
    /// Where the map has them.
    None,
    /// About the centre of its origin brush.
    OriginBrush,
    /// About its target's origin, Hipnotic's `rotate_` entities.
    Hipnotic,
}

/// An entity's brushes, `mapentity_t`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MapEntity {
    /// Its brushes, in map order.
    pub(crate) brushes: Vec<MapBrush>,
    /// Its origin.
    pub(crate) origin: Vec3,
    /// How its brushes are placed about it.
    pub(crate) rotation: Rotation,
    /// The `origin` the compile writes in its place when it sets one.
    pub(crate) origin_key: Option<Vec3>,
    /// Its faces' lightmap scale as a power of two, from `_lmscale` (QBSP-15.21).
    pub(crate) lmshift: u32,
}

/// The texels of a luxel per unit of `_lmscale`.
const LUXELS_PER_SCALE: f64 = 16.0;
/// A lightmap luxel's size in texels with no `_lmscale`.
const DEFAULT_LUXEL: i64 = 16;

/// The lightmap shift of `_lmscale` `scale`, `ProcessMapBrushes` (QBSP-15.21): 16 times it,
/// truncated, 16 when that is 0, then how many times it halves to 1.
fn lmshift(scale: f64) -> u32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec QBSP-15.21: `int i = 16 * get_float(\"_lmscale\")` truncates"
    )]
    let luxel = (LUXELS_PER_SCALE * scale) as i64;
    let mut luxel = if luxel == 0 { DEFAULT_LUXEL } else { luxel };
    let mut shift = 0_u32;
    while luxel > 1 {
        shift = shift.saturating_add(1);
        luxel = luxel.checked_div(2).unwrap_or(0);
    }
    shift
}

/// A map's brushes, ready for each hull.
#[derive(Debug)]
pub(crate) struct LoadedMap {
    /// The entities, the world first, as the map file has them.
    pub(crate) entities: Vec<MapEntity>,
    /// Every plane.
    pub(crate) planes: Planes,
    /// Every texture and texinfo.
    pub(crate) texinfos: Texinfos,
    /// How far from the origin a polygon on a plane reaches, `-worldextent`.
    pub(crate) world_extent: f64,
}

/// What reading a map needs at hand.
struct Reader<'a> {
    planes: &'a mut Planes,
    texinfos: &'a mut Texinfos,
    game: Game,
}

/// The integer value of `key`, `get_int`: 0 when it is missing.
fn int_value(entity: &Entity, key: &[u8]) -> i32 {
    entity.value(key).map_or(0, c_atoi)
}

/// Whether `name` is a skip texture, `IsSkipName`.
fn is_skip_name(name: &[u8]) -> bool {
    SKIP_NAMES
        .iter()
        .any(|skip| name.eq_ignore_ascii_case(skip))
}

/// Whether `name` is not lightmapped, `IsSpecialName`: a liquid unless lit, or a sky.
fn is_special_name(name: &[u8], lit_water: LitWater) -> bool {
    (name.first() == Some(&LIQUID_PREFIX) && lit_water == LitWater::Off)
        || name
            .get(..SKY_PREFIX.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(SKY_PREFIX))
}

/// Whether an entity's liquids are lightmapped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LitWater {
    Off,
    On,
}

impl LitWater {
    /// `_litwater`, or `_splitturb`, above 0; off otherwise.
    fn of(entity: &Entity) -> Self {
        let lit = [b"_litwater".as_slice(), b"_splitturb"]
            .into_iter()
            .find_map(|key| entity.value(key).map(|v| c_atoi(v) > 0));
        if lit == Some(true) {
            Self::On
        } else {
            Self::Off
        }
    }
}

/// The surface flags of a face with texture `name` in `entity`, `SurfFlagsForEntity`
/// (QBSP-15.16): skip textures are not drawn, hint textures are hints and liquids and skies are
/// special.
fn surf_flags(name: &[u8], entity: &Entity) -> SurfFlags {
    let mut flags = SurfFlags::default();
    if is_skip_name(name) {
        flags = flags.with(SurfFlags::NODRAW);
    }
    if HINT_NAMES
        .iter()
        .any(|hint| name.eq_ignore_ascii_case(hint))
    {
        flags = flags.with(SurfFlags::HINT);
    }
    if is_special_name(name, LitWater::of(entity)) {
        flags = flags.with(SurfFlags::SPECIAL);
    }
    if name.eq_ignore_ascii_case(NO_EXPAND_NAME) {
        flags = flags.with(SurfFlags::NO_EXPAND);
    }
    flags
}

/// `v` with a component within [`NORMAL_EPSILON`] of 1 or -1 made that axis alone, `qv::Snap`.
fn snap(v: Vec3) -> Vec3 {
    for (axis, c) in v.iter().enumerate() {
        for unit in [1.0, -1.0] {
            if (c - unit).abs() < NORMAL_EPSILON {
                let mut snapped = [0.0; AXES];
                if let Some(s) = snapped.get_mut(axis) {
                    *s = unit;
                }
                return snapped;
            }
        }
    }
    v
}

impl Reader<'_> {
    fn plane(&self, num: PlaneNum) -> Option<Plane> {
        self.planes.get(num).map(BspPlane::plane)
    }

    /// A face of `entity`, `ParseBrushFace` (QBSP-15.14, QBSP-15.16): its plane from its points
    /// in double precision, added to the table even with no normal, and then `None`; its texture
    /// found first; a side not drawn given the standard axes of its plane.
    fn face(&mut self, face: &Face, entity: &Entity) -> Option<MapFace> {
        let [p0, p1, p2] = *face.points();
        let (normal, len) = normalize_with_length(cross(sub(p0, p1), sub(p2, p1)));
        let planenum = self.planes.add_or_find(Plane {
            normal,
            dist: dot(p1, normal),
        });
        let miptex = self.texinfos.find_miptex(face.texture());
        if len < NORMAL_EPSILON {
            return None;
        }
        let name = self
            .texinfos
            .miptex
            .get(miptex)
            .map_or(face.texture(), Vec::as_slice);
        let flags = surf_flags(name, entity);
        let invisible = flags.has(SurfFlags::NODRAW | SurfFlags::HINTSKIP | SurfFlags::HINT);
        let vecs = match (invisible, self.plane(planenum)) {
            (true, Some(plane)) => Some(texvecs(plane.normal, [0.0; 2], 0.0, [1.0; 2])),
            _ => face.texvecs(),
        };
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec QBSP-15.16: texture axes are stored as `float`"
        )]
        let vecs = vecs.map_or(NO_TEXVECS, |v| v.map(|axis| axis.map(|c| c as f32)));
        let texinfo = self.texinfos.find(Texinfo {
            vecs,
            miptex,
            flags,
        });
        Some(MapFace {
            planenum,
            texinfo,
            texture: face.texture().to_vec(),
            winding: Winding::default(),
            kind: SideKind::Given,
            line: face.line(),
        })
    }

    /// A brush of `entity`, `ParseBrush` (QBSP-15.14, QBSP-15.17): its faces, a face on the
    /// plane of one before it dropped; in a brush with a hint face every other face a hintskip,
    /// unless every face would be; its contents.
    fn brush(&mut self, brush: &qs_map::Brush, entity: &Entity) -> MapBrush {
        let mut faces: Vec<MapFace> = Vec::new();
        let mut is_hint = false;
        for source in &brush.faces {
            let Some(face) = self.face(source, entity) else {
                continue;
            };
            let plane = self.planes.get(face.planenum);
            let duplicate = faces.iter().any(|known| {
                self.planes
                    .get(known.planenum)
                    .zip(plane)
                    .is_some_and(|(a, b)| a.epsilon_equal(b))
            });
            if duplicate {
                continue;
            }
            is_hint |= self.texinfos.flags(face.texinfo).has(SurfFlags::HINT);
            faces.push(face);
        }
        if is_hint {
            for face in &mut faces {
                if !self.texinfos.flags(face.texinfo).has(SurfFlags::HINT) {
                    face.texinfo = self
                        .texinfos
                        .reflagged(face.texinfo, |f| f.with(SurfFlags::HINTSKIP));
                }
            }
        }
        if faces
            .iter()
            .all(|f| self.texinfos.flags(f.texinfo).has(SurfFlags::HINTSKIP))
        {
            for face in &mut faces {
                face.texinfo = self
                    .texinfos
                    .reflagged(face.texinfo, |f| f.without(SurfFlags::HINTSKIP));
            }
        }
        let contents = self.contents(&faces, entity);
        MapBrush {
            faces,
            contents,
            bounds: Bounds::default(),
            line: brush.line(),
            chop_order: ChopOrder::default(),
        }
    }

    /// A brush's contents, `Brush_GetContents` (QBSP-15.15): the first face's that is not
    /// empty, read as far as a face of another type; `_mirrorinside` and `_noclipfaces`; a
    /// visblocker's own.
    fn contents(&self, faces: &[MapFace], entity: &Entity) -> Contents {
        let mut base: Option<Contents> = None;
        for face in faces {
            let contents = self.game.face_contents(&face.texture);
            if contents.is_empty() {
                continue;
            }
            match base {
                None => base = Some(contents),
                Some(first) if !contents.types_equal(first) => break,
                Some(_) => {}
            }
        }
        let mut base = base.unwrap_or(Contents::EMPTY);
        let mirror = match entity.value(b"_mirrorinside").map(c_atoi) {
            None => MirrorInside::Unset,
            Some(0) => MirrorInside::Off,
            Some(_) => MirrorInside::On,
        };
        base = base.with_mirrored(mirror);
        if entity
            .value(b"_noclipfaces")
            .is_some_and(|v| c_atoi(v) != 0)
        {
            base = base.without_clipping_same_type();
        }
        if entity
            .classname()
            .is_some_and(|c| c.eq_ignore_ascii_case(VISBLOCKER))
        {
            base = base.as_visblocker();
        }
        base
    }

    /// Each side's polygon and the brush's bounds, `CalculateBrushBounds` (QBSP-15.18): a
    /// side's square of `extent` clipped behind every other side but the bevels; a side clipped
    /// away keeps the polygon it had.
    fn bounds(&self, brush: &mut MapBrush, extent: f64) {
        let mut bounds = Bounds::default();
        let planes: Vec<(Option<Plane>, Option<Plane>, SideKind)> = brush
            .faces
            .iter()
            .map(|f| {
                (
                    self.plane(f.planenum),
                    self.plane(f.planenum.opposite()),
                    f.kind,
                )
            })
            .collect();
        for (i, face) in brush.faces.iter_mut().enumerate() {
            let Some(Some(plane)) = planes.get(i).map(|p| p.0) else {
                continue;
            };
            let mut w = Winding::from_plane(plane, extent);
            for (j, (_, behind, kind)) in planes.iter().enumerate() {
                if i == j || *kind == SideKind::Bevel {
                    continue;
                }
                let (Some(current), Some(behind)) = (w.take(), behind) else {
                    break;
                };
                w = current.clipped(*behind, 0.0, KeepOn::No, winding::Part::Front);
            }
            if let Some(w) = w {
                for &p in w.points() {
                    bounds.add(p);
                }
                face.winding = w;
            }
        }
        brush.bounds = bounds;
    }

    /// A side of `plane` with the surface of `brush`'s side `from`, a bevel.
    fn bevel(&mut self, brush: &MapBrush, from: usize, plane: Plane) -> Option<MapFace> {
        let source = brush.faces.get(from)?;
        Some(MapFace {
            planenum: self.planes.add_or_find(plane),
            texinfo: source.texinfo,
            texture: source.texture.clone(),
            winding: Winding::default(),
            kind: SideKind::Bevel,
            line: source.line,
        })
    }

    /// The brush's bevels, `AddBrushBevels` (QBSP-15.20): each axial plane it lacks at its
    /// bounds, the six put first in the order -x, +x, -y, +y, -z, +z; then, for each edge of a
    /// side after those six that is not axial, each plane through it along an axis that every
    /// side's polygon is behind and no side already has.
    fn bevels(&mut self, brush: &mut MapBrush) {
        let mut order = 0_usize;
        for axis in 0..AXES {
            for dir in [-1.0, 1.0] {
                let found = brush.faces.iter().position(|s| {
                    self.plane(s.planenum)
                        .and_then(|p| p.normal.get(axis).copied())
                        == Some(dir)
                });
                let i = match found {
                    Some(i) => i,
                    None => {
                        let mut normal = [0.0; AXES];
                        if let Some(n) = normal.get_mut(axis) {
                            *n = dir;
                        }
                        let bound = if dir > 0.0 {
                            brush.bounds.maxs.get(axis).copied()
                        } else {
                            brush.bounds.mins.get(axis).map(|m| -m)
                        };
                        let plane = Plane {
                            normal,
                            dist: bound.unwrap_or_default(),
                        };
                        let Some(side) = self.bevel(brush, 0, plane) else {
                            return;
                        };
                        brush.faces.push(side);
                        brush.faces.len().saturating_sub(1)
                    }
                };
                if i != order && i < brush.faces.len() && order < brush.faces.len() {
                    brush.faces.swap(order, i);
                }
                order = order.saturating_add(1);
            }
        }
        if brush.faces.len() == BOX_SIDES {
            return;
        }
        let mut i = BOX_SIDES;
        while let Some(face) = brush.faces.get(i) {
            let w = face.winding.clone();
            let n = w.len();
            for (j, &start) in w.points().iter().enumerate() {
                let Some(&end) = j
                    .saturating_add(1)
                    .checked_rem(n)
                    .and_then(|k| w.points().get(k))
                else {
                    continue;
                };
                let (vec, len) = normalize_with_length(sub(start, end));
                if len < SHORTEST_BEVELLED_EDGE {
                    continue;
                }
                let vec = snap(vec);
                #[expect(
                    clippy::float_cmp,
                    reason = "spec QBSP-15.20: a snapped axial edge is exactly 1 or -1"
                )]
                let axial = vec.iter().any(|c| c.abs() == 1.0);
                if axial {
                    continue;
                }
                self.edge_bevels(brush, i, vec, start);
            }
            i = i.saturating_add(1);
        }
    }

    /// The bevels through the edge along `vec` from `start` of side `from` (QBSP-15.20).
    fn edge_bevels(&mut self, brush: &mut MapBrush, from: usize, vec: Vec3, start: Vec3) {
        for axis in 0..AXES {
            for dir in [-1.0, 1.0] {
                let mut unit = [0.0; AXES];
                if let Some(u) = unit.get_mut(axis) {
                    *u = dir;
                }
                let (normal, sin_of_angle) = normalize_with_length(cross(vec, unit));
                if sin_of_angle < ANGLE_EPSILON {
                    continue;
                }
                let plane = Plane {
                    normal,
                    dist: dot(start, normal),
                };
                let (candidate, _) = BspPlane::new(plane, Facing::AsGiven);
                let outer = brush.faces.iter().all(|side| {
                    let used = self
                        .planes
                        .get(side.planenum)
                        .is_some_and(|p| p.epsilon_equal(&candidate));
                    !used
                        && side
                            .winding
                            .points()
                            .iter()
                            .all(|&p| plane.distance_to(p) <= POINT_EPSILON)
                });
                if !outer {
                    continue;
                }
                if let Some(side) = self.bevel(brush, from, plane) {
                    brush.faces.push(side);
                }
            }
        }
    }

    /// The farthest a brush's corner is from the origin on an axis, `GetBrushExtents`
    /// (QBSP-15.19): each point where three of its planes meet that no plane has in front.
    fn extents(&self, brush: &MapBrush) -> f64 {
        let planes: Vec<Plane> = brush
            .faces
            .iter()
            .filter_map(|f| self.plane(f.planenum))
            .collect();
        let mut extents = f64::NEG_INFINITY;
        for (i, &a) in planes.iter().enumerate() {
            for (j, &b) in planes.iter().enumerate().skip(i.saturating_add(1)) {
                for &c in planes.iter().skip(j.saturating_add(1)) {
                    let Some(vertex) = a.intersection(b, c) else {
                        continue;
                    };
                    if planes
                        .iter()
                        .all(|p| p.distance_to(vertex) <= NORMAL_EPSILON)
                    {
                        for v in vertex {
                            extents = extents.max(v.abs());
                        }
                    }
                }
            }
        }
        extents
    }

    /// The world's extent, `CalculateWorldExtent` (QBSP-15.19): twice the farthest brush corner
    /// and hull reach, rounded up, and [`SIDESPACE`].
    fn world_extent(&self, entities: &[MapEntity]) -> f64 {
        let extents = entities
            .iter()
            .flat_map(|e| &e.brushes)
            .map(|b| self.extents(b))
            .fold(f64::NEG_INFINITY, f64::max);
        let hull = self
            .game
            .hull_sizes()
            .iter()
            .flat_map(|h| h.mins.into_iter().chain(h.maxs))
            .fold(0.0, |m: f64, v| m.max(v.abs()));
        ((extents + hull) * TWICE).ceil() + SIDESPACE
    }

    /// The entity's brushes moved by its origin, `ProcessMapBrushes`'s offset (QBSP-15.21):
    /// each texture offset by the origin along its axis, each plane moved back by it.
    fn offset(&mut self, entity: &mut MapEntity, extent: f64) {
        let origin = entity.origin;
        let mut brushes = core::mem::take(&mut entity.brushes);
        for brush in &mut brushes {
            for face in &mut brush.faces {
                if let Some(mut texinfo) = self.texinfos.get(face.texinfo).copied() {
                    for axis in &mut texinfo.vecs {
                        let [x, y, z, offset] = *axis;
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "spec QBSP-15.21: the offset is stored back as `float`"
                        )]
                        let moved =
                            (f64::from(offset) + dot(origin, [x, y, z].map(f64::from))) as f32;
                        *axis = [x, y, z, moved];
                    }
                    face.texinfo = self.texinfos.find(texinfo);
                }
                if let Some(plane) = self.plane(face.planenum) {
                    face.planenum = self.planes.add_or_find(Plane {
                        normal: plane.normal,
                        dist: plane.dist - dot(plane.normal, origin),
                    });
                }
            }
            self.bounds(brush, extent);
        }
        entity.brushes = brushes;
    }
}

/// The origin of the entity `rotating` targets, `FixRotateOrigin`, in single precision; the
/// origin when it has no target.
fn rotate_origin(file: &MapFile, rotating: &Entity) -> Vec3 {
    let Some(target) = rotating.value(b"target").filter(|t| !t.is_empty()) else {
        return [0.0; AXES];
    };
    let found = file.entities.iter().find(|e| {
        e.value(b"targetname")
            .is_some_and(|name| name.eq_ignore_ascii_case(target))
    });
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec QBSP-15.21: the target's origin is read into a `qvec3f`"
    )]
    let single = |v: f64| f64::from(v as f32);
    found.map_or([0.0; AXES], |e| {
        scan_vector(e.value(b"origin").unwrap_or_default()).map(single)
    })
}

/// `file`'s brushes as ericw-tools' qbsp loads them for `game`, `LoadMapFile` and
/// `ProcessMapBrushes` (QBSP-15.14 to QBSP-15.21). The world brush entities keep their own
/// entities here; the world takes their brushes for each hull (QBSP-15.24).
pub(crate) fn load(file: &MapFile, game: Game) -> LoadedMap {
    let mut planes = Planes::default();
    let mut texinfos = Texinfos::with_skip();
    let mut reader = Reader {
        planes: &mut planes,
        texinfos: &mut texinfos,
        game,
    };
    let mut entities: Vec<MapEntity> = file
        .entities
        .iter()
        .map(|source| {
            let brushes = if int_value(source, b"_omitbrushes") == 0 {
                source
                    .brushes()
                    .map(|b| reader.brush(b, source))
                    .filter(|b| !b.faces.is_empty())
                    .collect()
            } else {
                Vec::new()
            };
            MapEntity {
                brushes,
                origin: source.value(b"origin").map_or([0.0; AXES], scan_vector),
                rotation: Rotation::None,
                origin_key: None,
                lmshift: lmshift(source.value(b"_lmscale").map_or(0.0, c_atof)),
            }
        })
        .collect();
    let extent = reader.world_extent(&entities);
    for (index, (entity, source)) in entities.iter_mut().zip(&file.entities).enumerate() {
        let mut kept = Vec::with_capacity(entity.brushes.len());
        for mut brush in core::mem::take(&mut entity.brushes) {
            if source.value(b"_chop").is_some_and(|v| c_atoi(v) == 0) {
                brush.chop_order = ChopOrder(UNCHOPPED);
            }
            if let Some(order) = source.value(b"_chop_order") {
                brush.chop_order = ChopOrder(c_atoi(order));
            }
            reader.bounds(&mut brush, extent);
            if brush.contents.is_origin() {
                let has_origin = source.value(b"origin").is_some() || entity.origin_key.is_some();
                if index != 0 && !has_origin {
                    entity.origin = brush.bounds.centroid();
                    entity.origin_key = Some(entity.origin);
                }
                entity.rotation = Rotation::OriginBrush;
                continue;
            }
            reader.bevels(&mut brush);
            kept.push(brush);
        }
        entity.brushes = kept;
        let hipnotic = source.classname().is_some_and(|c| {
            c.get(..HIPNOTIC_ROTATE_PREFIX.len())
                .is_some_and(|p| p.eq_ignore_ascii_case(HIPNOTIC_ROTATE_PREFIX))
        });
        if entity.rotation == Rotation::None && hipnotic {
            entity.origin = rotate_origin(file, source);
            entity.origin_key = Some(entity.origin);
            entity.rotation = Rotation::Hipnotic;
        }
        if entity.rotation != Rotation::None {
            reader.offset(entity, extent);
        }
    }
    LoadedMap {
        entities,
        planes,
        texinfos,
        world_extent: extent,
    }
}

#[cfg(test)]
mod tests;
