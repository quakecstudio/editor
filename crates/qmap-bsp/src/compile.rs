// SPDX-License-Identifier: GPL-3.0-or-later

//! The run: each hull's entities processed in order, the clipping hulls passed to the drawing
//! hull, and the BSP, portal file and leak file written (QBSP-1).

use qs_bsp::write::{BspFormat, BspWriteError, DiskTexinfo};
use qs_map::MapFile;
use qs_map::qbsp::{QbspMap, load_map_file};
use qs_wad::Wad;

use crate::brush::BrushSet;
use crate::csg::csg_faces;
use crate::math::{ORIGIN, Vec3, each};
use crate::merge::merge_all;
use crate::output::{ClipHull, Output};
use crate::outside::{Fill, fill_outside, gather};
use crate::planes::Planes;
use crate::portals::portalize;
use crate::prt::portal_file;
use crate::tree::{Builder, Choice, SIDESPACE, Tree};
use crate::{
    CompileError, Dialect, Faults, Hull, Limits, ModelKind, Surface, Tables, Warning, tjunc,
};

/// The default largest texture extent of a face before it is cut (QBSP-1).
const SUBDIVIDE_SIZE: f32 = 240.0;
/// The most bytes the textures lump may hold (QBSP-13.10).
const MAX_MAP_MIPTEX: usize = 0x20_0000;

/// What a compile wrote.
#[derive(Clone, Debug, PartialEq)]
pub struct Compiled {
    /// The BSP file: BSP29, or `BSP2` when the extended dialect needs it (QBSP-14.4).
    pub bsp: Vec<u8>,
    /// The portal file, written when the outside was filled (QBSP-12).
    pub portal_file: Option<Vec<u8>>,
    /// The leak file, empty as qbsp leaves it, written once an entity stands in empty space
    /// (QBSP-9.5).
    pub leak_file: Option<Vec<u8>>,
    /// What qbsp would have printed and gone on after.
    pub warnings: Vec<Warning>,
}

/// The map compiler and its options.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Compiler {
    subdivide: f32,
    dialect: Dialect,
}

impl Default for Compiler {
    fn default() -> Self {
        Self {
            subdivide: SUBDIVIDE_SIZE,
            dialect: Dialect::default(),
        }
    }
}

/// One hull's work: the map, the shared plane table and what it writes.
struct Run<'a> {
    map: &'a QbspMap,
    planes: &'a mut Planes,
    warnings: &'a mut Vec<Warning>,
    subdivide: f32,
    limits: Limits,
}

/// An entity's tree for a hull, and its head bounds.
struct Built {
    tree: Tree,
    mins: Vec3,
    maxs: Vec3,
    /// How far the entity's brushes reach from the origin on each axis (QBSP-11.1).
    reach: Vec3,
}

impl Run<'_> {
    fn builder(&self, choice: Choice) -> Builder<'_> {
        Builder {
            planes: self.planes,
            texinfo: &self.map.texinfo,
            choice,
            subdivide: self.subdivide,
            faults: self.limits.faults,
            warnings: core::cell::RefCell::new(Vec::new()),
        }
    }

    /// The tree of `surfaces`, the builder's warnings kept.
    fn build(
        &mut self,
        choice: Choice,
        surfaces: Vec<Surface>,
        mins: Vec3,
        maxs: Vec3,
    ) -> Result<Tree, CompileError> {
        let builder = self.builder(choice);
        let tree = builder.build(surfaces, mins, maxs)?;
        let warnings = builder.warnings.into_inner();
        self.warnings.extend(warnings);
        Ok(tree)
    }

    /// An entity's brushes for `hull`, CSG and a first tree (QBSP-1.7).
    fn first_tree(
        &mut self,
        entity: usize,
        hull: Hull,
        choice: Choice,
    ) -> Result<Option<Built>, CompileError> {
        let Some(source) = self.map.entities.get(entity) else {
            return Ok(None);
        };
        if source.brushes.is_empty() {
            return Ok(None);
        }
        let model = if entity == 0 {
            ModelKind::World
        } else {
            ModelKind::Entity
        };
        let set = BrushSet::load(
            source,
            self.map,
            hull,
            model,
            self.planes,
            self.warnings,
            self.limits,
        )?;
        if set.brushes.is_empty() {
            return match self.limits.faults {
                Faults::Stop => Err(CompileError::NoValidBrushes),
                Faults::Warn => {
                    self.warnings.push(Warning::NoValidBrushes);
                    let origin = source.origin;
                    Ok(Some(Built {
                        tree: Tree::empty(origin, origin),
                        mins: origin,
                        maxs: origin,
                        reach: ORIGIN,
                    }))
                }
            };
        }
        let surfaces = csg_faces(&set, self.planes)?;
        let mins = set.mins.map(|c| c - SIDESPACE);
        let maxs = set.maxs.map(|c| c + SIDESPACE);
        let reach = each(set.maxs, set.mins, |high, low| high.abs().max(low.abs()));
        let tree = self.build(choice, surfaces, mins, maxs)?;
        Ok(Some(Built {
            tree,
            mins,
            maxs,
            reach,
        }))
    }

    /// The entities' origins after the world's, where the fill looks for occupants (QBSP-9.1).
    fn origins(&self) -> Vec<Vec3> {
        self.map.entities.iter().skip(1).map(|e| e.origin).collect()
    }

    /// A clipping hull: each entity's tree, the world's outside filled (QBSP-1.7).
    fn clip_hull(&mut self, hull: Hull) -> Result<ClipHull, CompileError> {
        let mut clip = ClipHull::new(self.limits.tables);
        for entity in 0..self.map.entities.len() {
            let Some(mut built) = self.first_tree(entity, hull, Choice::MidSplit)? else {
                continue;
            };
            if entity == 0 {
                let portals = portalize(
                    &mut built.tree,
                    self.planes,
                    built.mins,
                    built.maxs,
                    self.warnings,
                )?;
                let origins = self.origins();
                if fill_outside(&mut built.tree, self.planes, &portals, &origins) == Fill::Filled {
                    let surfaces = gather(&built.tree);
                    built.tree = self.build(Choice::Full, surfaces, built.mins, built.maxs)?;
                }
            }
            clip.add_model(&built.tree, self.planes)?;
        }
        Ok(clip)
    }

    /// The drawing hull into `out`; the portal and leak files the world's fill writes
    /// (QBSP-1.7).
    #[expect(
        clippy::type_complexity,
        reason = "the two optional files the world's fill writes"
    )]
    fn draw_hull(
        &mut self,
        out: &mut Output,
    ) -> Result<(Option<Vec<u8>>, Option<Vec<u8>>), CompileError> {
        let (mut prt, mut pts) = (None, None);
        for entity in 0..self.map.entities.len() {
            let choice = if entity == 0 {
                Choice::MidSplit
            } else {
                Choice::Full
            };
            let Some(mut built) = self.first_tree(entity, Hull::Draw, choice)? else {
                continue;
            };
            if entity == 0 {
                let portals = portalize(
                    &mut built.tree,
                    self.planes,
                    built.mins,
                    built.maxs,
                    self.warnings,
                )?;
                let origins = self.origins();
                match fill_outside(&mut built.tree, self.planes, &portals, &origins) {
                    Fill::NoOccupant => self.warnings.push(Warning::NoOccupant),
                    Fill::Leak => {
                        self.warnings.push(Warning::Leak);
                        pts = Some(Vec::new());
                    }
                    Fill::Filled => {
                        pts = Some(Vec::new());
                        let surfaces: Vec<Surface> = gather(&built.tree)
                            .into_iter()
                            .map(|surface| Surface {
                                faces: merge_all(surface.faces, self.planes),
                                ..surface
                            })
                            .collect();
                        built.tree = self.build(Choice::Full, surfaces, built.mins, built.maxs)?;
                        let portals = portalize(
                            &mut built.tree,
                            self.planes,
                            built.mins,
                            built.maxs,
                            self.warnings,
                        )?;
                        prt = Some(portal_file(&built.tree, &portals, self.planes));
                        tjunc::fix(&mut built.tree, built.reach)?;
                    }
                }
            }
            out.draw_model(&mut built.tree, self.planes)?;
        }
        Ok((prt, pts))
    }
}

impl Compiler {
    /// A compiler cutting faces past `subdivide` texels (`-subdivide`, QBSP-1.1).
    #[must_use]
    pub fn with_subdivide(self, subdivide: f32) -> Self {
        Self { subdivide, ..self }
    }

    /// A compiler of `dialect` (QBSP-14).
    #[must_use]
    pub fn with_dialect(self, dialect: Dialect) -> Self {
        Self { dialect, ..self }
    }

    /// The WAD path the map's world names (MAP-6.3), relative to the game directory, which the
    /// caller reads and passes to [`Compiler::compile`].
    ///
    /// # Errors
    ///
    /// [`CompileError::Map`] when the map cannot be read.
    pub fn wad_path(map: &[u8]) -> Result<Option<Vec<u8>>, CompileError> {
        Ok(load_map_file(map)?.wad_path().map(<[u8]>::to_vec))
    }

    /// Compiles `map` with the textures of `wad`, as id's qbsp does (QBSP-1).
    ///
    /// # Errors
    ///
    /// The [`CompileError`] qbsp stops with.
    pub fn compile(&self, map: &[u8], wad: Option<&[u8]>) -> Result<Compiled, CompileError> {
        let mut qmap = match self.dialect {
            Dialect::Id => load_map_file(map)?,
            Dialect::Extended => {
                let file = MapFile::parse(map).map_err(CompileError::Document)?;
                QbspMap::from_map_file(&file)?
            }
        };
        qmap.number_models();
        let limits = self.dialect.limits();
        let mut planes = Planes::new(limits.planes);
        let mut warnings = Vec::new();
        let mut run = Run {
            map: &qmap,
            planes: &mut planes,
            warnings: &mut warnings,
            subdivide: self.subdivide,
            limits,
        };
        let player = run.clip_hull(Hull::Player)?;
        let large = run.clip_hull(Hull::Large)?;
        let mut out = Output::new(limits);
        let (portal_file, leak_file) = run.draw_hull(&mut out)?;
        out.read_clip_hull(Hull::Player, &player)?;
        out.read_clip_hull(Hull::Large, &large)?;
        out.file.texinfo = qmap
            .texinfo
            .iter()
            .map(|texinfo| -> Result<DiskTexinfo, CompileError> {
                Ok(DiskTexinfo {
                    vecs: texinfo.vecs,
                    miptex: i32::try_from(texinfo.miptex)
                        .map_err(|_| CompileError::Limit("MAX_MAP_TEXINFO"))?,
                    flags: texinfo.flags,
                })
            })
            .collect::<Result<_, _>>()?;
        out.file.entities = match self.dialect {
            Dialect::Id => qmap.entity_lump()?,
            Dialect::Extended => qmap.entity_lump_unbounded(),
        };
        out.file.textures = textures(&qmap, wad)?;
        let bsp = match out.file.to_bytes(BspFormat::Bsp29) {
            Err(BspWriteError::TooWide(lump)) if limits.tables == Tables::Format => {
                warnings.push(Warning::Bsp2(lump));
                out.file.to_bytes(BspFormat::Bsp2)
            }
            written => written,
        }
        .map_err(CompileError::Write)?;
        Ok(Compiled {
            bsp,
            portal_file,
            leak_file,
            warnings,
        })
    }
}

/// The textures lump, `WriteMiptex`: a slot per texture the map uses and each animation frame
/// the WAD holds, each lump copied from the WAD, -1 for one it lacks; empty with no WAD
/// (MAP-6.3, MAP-6.8).
fn textures(map: &QbspMap, wad: Option<&[u8]>) -> Result<Vec<u8>, CompileError> {
    let Some(bytes) = wad else {
        return Ok(Vec::new());
    };
    let Ok(wad) = Wad::read(bytes) else {
        return Ok(Vec::new());
    };
    let names: Vec<&[u8]> = wad
        .lumps()
        .iter()
        .map(|lump| lump.name.as_bytes())
        .collect();
    let (slots, _) = map.texture_slots(&names)?;
    let count = slots.len();
    let mut out = Vec::new();
    let as_i32 = |v: usize| {
        i32::try_from(v).map_err(|_| CompileError::Limit("Textures exceeded MAX_MAP_MIPTEX"))
    };
    out.extend_from_slice(&as_i32(count)?.to_le_bytes());
    let header = count.saturating_add(1).saturating_mul(size_of::<i32>());
    let mut data = Vec::new();
    let mut offsets = Vec::new();
    for slot in slots {
        let lump = wad
            .find(&slot.name)
            .ok()
            .and_then(|lump| wad.data(lump).ok());
        match lump {
            Some(lump) if !lump.is_empty() => {
                offsets.push(as_i32(header.saturating_add(data.len()))?);
                data.extend_from_slice(lump);
            }
            _ => offsets.push(-1),
        }
        if header.saturating_add(data.len()) >= MAX_MAP_MIPTEX {
            return Err(CompileError::Limit("Textures exceeded MAX_MAP_MIPTEX"));
        }
    }
    for offset in offsets {
        out.extend_from_slice(&offset.to_le_bytes());
    }
    out.extend_from_slice(&data);
    Ok(out)
}

#[cfg(test)]
mod tests;
