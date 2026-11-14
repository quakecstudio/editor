// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 1997       Greg Lewis
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/qbsp.cc, ShouldGenerateClipnodes, ProcessEntity, CreateSingleHull, CreateHulls, ProcessFile, LoadTextureData
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/map.cc, WriteEntitiesToString

//! The extended dialect's compile, ericw-tools' `ProcessFile` (QBSP-15.48 to QBSP-15.50): the
//! map's brushes loaded once, each entity's tree built for each hull in turn, the world filled
//! from the void, and the trees, entities and textures written.

use qs_bsp::write::{BspFormat, BspWriteError, DiskModel};
use qs_map::cnum::c_atoi;
use qs_map::qbsp::QbspWarning;
use qs_map::{Entity, MapFile};
use qs_wad::Wad;

use super::brush::{HullBrushes, Loader, VisibleFaces, is_world_brush_entity};
use super::chop::Fragmentation;
use super::faces::FaceMaker;
use super::game::Game;
use super::map::{EntityNum, LoadedMap, Rotation, load};
use super::output::Writer;
use super::outside::{Fill, FillType, Filler, Occupant, Prefer};
use super::portals::{Portalizer, SideMarker};
use super::prt::fmt_number;
use super::tree::{BspInput, Tree, TreeSplit};
use super::vec::{Bounds, Vec3};
use crate::{CompileError, Compiled, Hull, Warning};

/// The hulls in the order they are made, `CreateHulls`.
const HULLS: [Hull; 3] = [Hull::Draw, Hull::Player, Hull::Large];
/// The keys left out of the entity lump, TrenchBroom's.
const TRENCHBROOM_PREFIX: &[u8] = b"_tb_";

/// What a compile keeps across entities and hulls.
struct Compile<'a> {
    file: &'a MapFile,
    map: LoadedMap,
    visible: VisibleFaces,
    occupants: Vec<Occupant>,
    writer: Writer,
    models: Vec<DiskModel>,
    leak: Option<(Vec3, Vec<Vec3>)>,
    portal_file: Option<Vec<u8>>,
    warnings: Vec<Warning>,
}

/// Whether `entity` has a clip tree for `hull`, `ShouldGenerateClipnodes` (QBSP-15.48): always
/// without `_hulls` or with it 0, else when its bit for the hull is set.
fn generates_clip_nodes(entity: &Entity, hull: Hull) -> bool {
    let Some(value) = entity.value(b"_hulls") else {
        return true;
    };
    let hulls = c_atoi(value);
    let bit = 1_i32
        .checked_shl(u32::try_from(hull.number()).unwrap_or(0))
        .unwrap_or(0);
    hulls == 0 || hulls & bit != 0
}

impl Compile<'_> {
    fn bsp(
        &mut self,
        bounds: Bounds,
        brushes: &[super::brush::BspBrush],
        split: TreeSplit,
    ) -> Tree {
        let extent = self.map.world_extent;
        BspInput {
            planes: &mut self.map.planes,
            texinfos: &self.map.texinfos,
            visible: &self.visible,
            extent,
        }
        .brush_bsp(bounds, brushes, split)
    }

    fn portals(&self, tree: &mut Tree) {
        Portalizer {
            planes: &self.map.planes,
            extent: self.map.world_extent,
        }
        .make_tree_portals(tree);
    }

    fn fill(
        &mut self,
        tree: &mut Tree,
        originals: &[super::brush::BspBrush],
        brushes: &[super::brush::BspBrush],
        fill: FillType,
    ) -> Fill {
        let result = Filler {
            planes: &self.map.planes,
            texinfos: &self.map.texinfos,
            originals,
        }
        .fill_outside(tree, &self.occupants, fill, brushes, &mut self.visible);
        match &result {
            Fill::Leak { origin, line, .. } => {
                if self.leak.is_none() {
                    self.warnings.push(Warning::Leak);
                    self.leak = Some((*origin, tree.leak_points(*origin, line)));
                }
            }
            Fill::NoOccupant => self.warnings.push(Warning::NoOccupant),
            Fill::Filled => {}
        }
        result
    }

    fn fill_detail(
        &mut self,
        tree: &mut Tree,
        originals: &[super::brush::BspBrush],
        brushes: &[super::brush::BspBrush],
    ) {
        Filler {
            planes: &self.map.planes,
            texinfos: &self.map.texinfos,
            originals,
        }
        .fill_detail(
            tree,
            &self.occupants,
            Prefer::Sealing,
            brushes,
            &mut self.visible,
        );
    }

    /// Entity `entity`'s tree for `hull` written into model `model`, `ProcessEntity`
    /// (QBSP-15.48, QBSP-15.49).
    fn process(&mut self, entity: EntityNum, model: usize, hull: Hull) -> Result<(), CompileError> {
        let Some(source) = self.file.entities.get(entity.get()) else {
            return Ok(());
        };
        let world = entity == EntityNum::WORLD;
        let HullBrushes { brushes, bounds } = Loader {
            file: self.file,
            map: &mut self.map,
            game: Game::Quake,
            visible: &mut self.visible,
        }
        .load_entity(entity, hull)
        .sorted();
        let originals = brushes.clone();
        let brushes = if hull == Hull::Draw {
            brushes
        } else {
            super::tree::Cutter {
                planes: &self.map.planes,
                extent: self.map.world_extent,
            }
            .chop(brushes, Fragmentation::Avoided)
        };
        if brushes.is_empty() && bounds == Bounds::default() {
            return Ok(());
        }
        let mut disk = self.models.get(model).copied().unwrap_or_default();
        if !generates_clip_nodes(source, hull) {
            let mut tree = self.bsp(bounds, &[], TreeSplit::Fast);
            if hull == Hull::Draw {
                self.portals(&mut tree);
                self.write_draw(&tree, &originals, &mut disk)?;
            } else {
                let head = self.writer.clip_nodes(&tree, &self.map.planes)?;
                if let Some(slot) = disk.headnode.get_mut(hull.number()) {
                    *slot = head;
                }
            }
            return self.store(model, disk);
        }
        if hull != Hull::Draw {
            let mut tree = self.bsp(bounds, &brushes, TreeSplit::Fast);
            if world {
                self.portals(&mut tree);
                if self.fill(&mut tree, &originals, &brushes, FillType::Outside) == Fill::Filled {
                    tree = self.bsp(bounds, &brushes, TreeSplit::Precise);
                    self.portals(&mut tree);
                    self.fill(&mut tree, &originals, &brushes, FillType::Outside);
                    tree.prune();
                }
            }
            let head = self.writer.clip_nodes(&tree, &self.map.planes)?;
            if let Some(slot) = disk.headnode.get_mut(hull.number()) {
                *slot = head;
            }
            return self.store(model, disk);
        }
        let split = if world {
            TreeSplit::Auto
        } else {
            TreeSplit::Fast
        };
        let mut tree = self.bsp(bounds, &brushes, split);
        self.portals(&mut tree);
        if world {
            if self.fill(&mut tree, &originals, &brushes, FillType::Inside) == Fill::Filled {
                self.fill_detail(&mut tree, &originals, &brushes);
                tree = self.bsp(bounds, &brushes, TreeSplit::Precise);
                self.portals(&mut tree);
                self.fill(&mut tree, &originals, &brushes, FillType::Inside);
                self.fill_detail(&mut tree, &originals, &brushes);
            }
        } else {
            Filler {
                planes: &self.map.planes,
                texinfos: &self.map.texinfos,
                originals: &originals,
            }
            .fill_brush_entity(&mut tree, &brushes, &mut self.visible);
            tree = self.bsp(bounds, &brushes, TreeSplit::Precise);
        }
        self.portals(&mut tree);
        SideMarker {
            planes: &self.map.planes,
            texinfos: &self.map.texinfos,
            originals: &originals,
        }
        .mark_visible_sides(&mut tree, &brushes, &mut self.visible);
        let maker = FaceMaker {
            map: &self.map,
            originals: &originals,
        };
        maker.make_faces(&mut tree);
        tree.prune();
        if world && self.leak.is_none() {
            let portalizer = Portalizer {
                planes: &self.map.planes,
                extent: self.map.world_extent,
            };
            self.portal_file = Some(tree.portal_file(&portalizer));
        }
        let maker = FaceMaker {
            map: &self.map,
            originals: &originals,
        };
        maker.make_mark_faces(&mut tree);
        self.write_draw(&tree, &originals, &mut disk)?;
        self.store(model, disk)
    }

    /// The drawing hull's tree written: its vertices, faces and nodes (QBSP-15.44 to
    /// QBSP-15.46).
    fn write_draw(
        &mut self,
        tree: &Tree,
        originals: &[super::brush::BspBrush],
        disk: &mut DiskModel,
    ) -> Result<(), CompileError> {
        let maker = FaceMaker {
            map: &self.map,
            originals,
        };
        let mut fragments = self.writer.emit_vertices(tree, &maker)?;
        let (first, ranges) =
            self.writer
                .emit_faces(tree, &self.map.planes, &self.map.texinfos, &mut fragments)?;
        self.writer.draw_nodes(
            tree,
            &self.map.planes,
            &self.map.texinfos,
            &fragments,
            (first, &ranges),
            disk,
        )
    }

    fn store(&mut self, model: usize, disk: DiskModel) -> Result<(), CompileError> {
        let slot = self
            .models
            .get_mut(model)
            .ok_or(CompileError::Limit("MAX_MAP_MODELS"))?;
        *slot = disk;
        Ok(())
    }

    /// The entity lump, `WriteEntitiesToString` (QBSP-15.50): each entity with pairs but the
    /// world brush entities, its pairs in order but TrenchBroom's, `model` set for each brush
    /// model after the world and `origin` where the compile set one.
    fn entity_lump(&self, model_of: &[Option<usize>]) -> Vec<u8> {
        let mut out = Vec::new();
        for (index, source) in self.file.entities.iter().enumerate() {
            if source.pairs().next().is_none() || is_world_brush_entity(source) {
                continue;
            }
            let mut pairs: Vec<(Vec<u8>, Vec<u8>)> = source
                .pairs()
                .map(|p| (p.key().to_vec(), p.value().to_vec()))
                .collect();
            let mut set =
                |key: &[u8], value: Vec<u8>| match pairs.iter_mut().find(|(k, _)| k == key) {
                    Some((_, v)) => *v = value,
                    None => pairs.push((key.to_vec(), value)),
                };
            if let Some(entity) = self.map.entities.get(index)
                && let Some(origin) = entity.origin_key
            {
                let text = if entity.rotation == Rotation::Hipnotic {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "spec QBSP-15.21: a Hipnotic origin is a `qvec3f`"
                    )]
                    let [x, y, z] = origin.map(|c| c as f32);
                    format!("{x} {y} {z}")
                } else {
                    let [x, y, z] = origin.map(fmt_number);
                    format!("{x} {y} {z}")
                };
                set(b"origin", text.into_bytes());
            }
            if index != 0
                && let Some(Some(model)) = model_of.get(index)
            {
                set(b"model", format!("*{model}").into_bytes());
            }
            out.extend_from_slice(b"{\n");
            for (key, value) in pairs {
                if key.starts_with(TRENCHBROOM_PREFIX) {
                    continue;
                }
                out.push(b'"');
                out.extend_from_slice(&key);
                out.extend_from_slice(b"\" \"");
                out.extend_from_slice(&value);
                out.extend_from_slice(b"\"\n");
            }
            out.extend_from_slice(b"}\n");
        }
        out.push(0);
        out
    }
}

/// The textures lump, `LoadTextureData` (QBSP-15.50): each of the map's textures in order, its
/// WAD lump when the WAD has it, else an offset of -1 and a warning ("unable to find texture"),
/// but for the skip texture the compile made itself when no face names it.
fn textures(
    miptex: &[Vec<u8>],
    wad: Option<&[u8]>,
    skip_use: SkipUse,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<u8>, CompileError> {
    let wad = wad.map(Wad::read).transpose().map_err(CompileError::Wad)?;
    let as_i32 = |v: usize| {
        i32::try_from(v).map_err(|_| CompileError::Limit("Textures exceeded MAX_MAP_MIPTEX"))
    };
    let header = miptex
        .len()
        .saturating_add(1)
        .saturating_mul(size_of::<i32>());
    let mut offsets = Vec::with_capacity(miptex.len());
    let mut data = Vec::new();
    for (index, name) in miptex.iter().enumerate() {
        let lump = wad
            .as_ref()
            .and_then(|w| w.find(name).ok().and_then(|l| w.data(l).ok()));
        match lump {
            Some(lump) if !lump.is_empty() => {
                offsets.push(as_i32(header.saturating_add(data.len()))?);
                data.extend_from_slice(lump);
            }
            _ => {
                offsets.push(-1);
                if index != SKIP_MIPTEX || skip_use == SkipUse::Named {
                    warnings.push(Warning::Map(QbspWarning::TextureNotFound {
                        name: name.clone(),
                    }));
                }
            }
        }
    }
    let mut out = Vec::with_capacity(header.saturating_add(data.len()));
    out.extend_from_slice(&as_i32(miptex.len())?.to_le_bytes());
    for offset in offsets {
        out.extend_from_slice(&offset.to_le_bytes());
    }
    out.extend_from_slice(&data);
    Ok(out)
}

/// The skip texture's place among the map's textures, made before any face's.
const SKIP_MIPTEX: usize = 0;

/// Whether a face of the map names the skip texture, which the compile makes for its own sides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SkipUse {
    Named,
    Unnamed,
}

impl LoadedMap {
    /// Whether a face of the map names the skip texture.
    fn skip_use(&self) -> SkipUse {
        let named = self
            .entities
            .iter()
            .flat_map(|e| &e.brushes)
            .flat_map(|b| &b.faces)
            .any(|f| {
                self.texinfos
                    .get(f.texinfo)
                    .is_some_and(|t| t.miptex == SKIP_MIPTEX)
            });
        if named {
            SkipUse::Named
        } else {
            SkipUse::Unnamed
        }
    }
}

/// `file` compiled as ericw-tools' qbsp compiles it for Quake, `ProcessFile` (QBSP-15.48 to
/// QBSP-15.50): the brushes loaded; a model for the world and each other entity with brushes but
/// the world brush entities; each hull in turn made for each of them; then the entities and
/// textures written, as BSP29 or, when a value does not fit, `BSP2`.
///
/// # Errors
///
/// A [`CompileError`] when a lump passes its limit.
pub(crate) fn compile(file: &MapFile, wad: Option<&[u8]>) -> Result<Compiled, CompileError> {
    let map = load(file, Game::Quake);
    let occupants = Occupant::of(map.entities.iter().map(|e| e.origin).zip(&file.entities));
    let mut model_of: Vec<Option<usize>> = Vec::with_capacity(map.entities.len());
    let mut count = 0_usize;
    for (index, (entity, source)) in map.entities.iter().zip(&file.entities).enumerate() {
        let has_model =
            index == 0 || (!entity.brushes.is_empty() && !is_world_brush_entity(source));
        model_of.push(has_model.then_some(count));
        if has_model {
            count = count.saturating_add(1);
        }
    }
    let mut run = Compile {
        file,
        map,
        visible: VisibleFaces::default(),
        occupants,
        writer: Writer::new(),
        models: vec![DiskModel::default(); count],
        leak: None,
        portal_file: None,
        warnings: Vec::new(),
    };
    for hull in HULLS {
        for (index, model) in model_of.iter().enumerate() {
            if let Some(model) = model {
                run.process(EntityNum::new(index), *model, hull)?;
            }
        }
    }
    run.writer.finish();
    run.writer.file.models = run.models.clone();
    run.writer.file.entities = run.entity_lump(&model_of);
    let skip_use = run.map.skip_use();
    run.writer.file.textures =
        textures(run.map.texinfos.miptex(), wad, skip_use, &mut run.warnings)?;
    let bsp = match run.writer.file.to_bytes(BspFormat::Bsp29) {
        Err(BspWriteError::TooWide(lump)) => {
            run.warnings.push(Warning::Bsp2(lump));
            run.writer.file.to_bytes(BspFormat::Bsp2)
        }
        written => written,
    }
    .map_err(CompileError::Write)?;
    let (leak_file, leak_trail) = match &run.leak {
        Some((_, points)) => {
            let mut text = String::new();
            for p in points {
                let [x, y, z] = p.map(fmt_number);
                text.push_str(&format!("{x} {y} {z}\n"));
            }
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the trail is drawn in single precision"
            )]
            let trail = points
                .iter()
                .step_by(2)
                .chain(points.last())
                .map(|p| p.map(|c| c as f32))
                .collect();
            (Some(text.into_bytes()), trail)
        }
        None => (None, Vec::new()),
    };
    Ok(Compiled {
        bsp,
        portal_file: run.portal_file,
        leak_file,
        leak_trail,
        warnings: run.warnings,
    })
}

#[cfg(test)]
mod tests;
