// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 1997       Greg Lewis
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/writebsp.cc, ExportMapPlane, ExportMapTexinfo, ExportClipNodes, ExportLeaf, ExportDrawNodes, BeginBSPFile, FinishBSPFile
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/faces.cc, EmitVertex, EmitFaceVertices, EmitVertices_R, GetEdge, EmitEdges, EmitFaceFragment, EmitFaces_R, EmitFaces
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/map.cc, find_emitted_hash_vector, add_hash_vector, add_hash_edge

//! The tree written into BSP lumps, ericw-tools' `writebsp.cc` and `EmitFaces` (QBSP-15.43 to
//! QBSP-15.46): planes and texinfo as they are first used, vertices welded, edges shared once,
//! faces, nodes and leaves in order, and each clipping hull's clip nodes.

use std::collections::HashMap;

use qs_bsp::write::{
    BspFile, DiskClipNode, DiskFace, DiskLeaf, DiskModel, DiskNode, DiskPlane, DiskTexinfo,
};
use qs_bsp::{BSP_MAXLIGHTMAPS, BSP_NUM_AMBIENTS};

use super::faces::{FaceId, FaceMaker};
use super::game::{Contents, Game};
use super::map::{SIDESPACE, SurfFlags, TexinfoNum, Texinfos};
use super::tree::{NodeId, Tree};
use super::vec::{AXES, PlaneNum, PlaneType, Planes, Vec3};
use crate::CompileError;

/// `POINT_EQUAL_EPSILON`: vertices this near are one.
const POINT_EQUAL_EPSILON: f64 = 0.05;
/// Half of [`POINT_EQUAL_EPSILON`], the box a vertex is looked for in.
const HALF_POINT_EPSILON: f64 = POINT_EQUAL_EPSILON * 0.5;
/// `TEX_SPECIAL`, Quake's one texinfo flag.
const TEX_SPECIAL: i32 = 1;
/// `CONTENTS_SOLID`, what a clip leaf of contents Quake cannot write becomes.
const SOLID_NATIVE: i32 = -2;
/// An unused light style.
const NO_STYLE: u8 = 255;
/// The fewest points a written face has.
const MIN_FACE_POINTS: usize = 3;
/// `PLANE_X` to `PLANE_ANYZ`, a plane's type as the file stores it (BSP-3.2).
const PLANE_X: i32 = 0;
const PLANE_Y: i32 = 1;
const PLANE_Z: i32 = 2;
const PLANE_ANYX: i32 = 3;
const PLANE_ANYY: i32 = 4;
const PLANE_ANYZ: i32 = 5;
/// What a Quake model's bounds are shrunk by, `uses_shrunk_model_bounds`.
const MODEL_SHRINK: f64 = 1.0;

/// A face's vertices and edges once written, `face_fragment_t`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Fragment {
    /// Its vertices' numbers.
    pub(crate) vertices: Vec<u32>,
    /// Its face's number once written.
    pub(crate) output: Option<u32>,
}

/// An edge written, `hashedge_t`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct HashEdge {
    index: i32,
    front: Contents,
    reused: bool,
}

/// Numbers a lump index as the disk record's field.
fn index<T: TryFrom<usize>>(value: usize, limit: &'static str) -> Result<T, CompileError> {
    T::try_from(value).map_err(|_| CompileError::Limit(limit))
}

/// The faces written for a model: its first face, and each node's first face and count.
pub(crate) type EmittedFaces = (u32, HashMap<NodeId, (u32, u32)>);

/// What writing a BSP keeps across its entities and hulls, `mapdata_t`'s output state.
pub(crate) struct Writer {
    pub(crate) file: BspFile,
    planes_out: HashMap<PlaneNum, i32>,
    texinfo_out: HashMap<TexinfoNum, i32>,
    vertex_cells: HashMap<[i64; 3], Vec<u32>>,
    vertex_points: Vec<Vec3>,
}

impl Writer {
    /// The file begun, `BeginBSPFile` (QBSP-15.43): edge 0 unused and leaf 0 the shared solid
    /// leaf.
    pub(crate) fn new() -> Self {
        let mut file = BspFile::default();
        file.edges.push([0, 0]);
        file.leafs.push(DiskLeaf {
            contents: Game::Quake
                .to_native(Contents::new(Contents::SOLID))
                .map_or(0, |n| n.value()),
            visofs: -1,
            mins: [0.0; AXES],
            maxs: [0.0; AXES],
            firstmarksurface: 0,
            nummarksurfaces: 0,
            ambient_level: [0; BSP_NUM_AMBIENTS],
        });
        Self {
            file,
            planes_out: HashMap::new(),
            texinfo_out: HashMap::new(),
            vertex_cells: HashMap::new(),
            vertex_points: Vec::new(),
        }
    }

    /// Plane `planenum`'s number in the file, written when first used, `ExportMapPlane`
    /// (QBSP-15.43).
    fn plane(&mut self, planes: &Planes, planenum: PlaneNum) -> Result<i32, CompileError> {
        if let Some(&out) = self.planes_out.get(&planenum) {
            return Ok(out);
        }
        let plane = planes
            .get(planenum)
            .ok_or(CompileError::Limit("MAX_MAP_PLANES"))?;
        let out = index(self.file.planes.len(), "MAX_MAP_PLANES")?;
        let p = plane.plane();
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec QBSP-15.43: `dplane_t` stores `float`s"
        )]
        let disk = DiskPlane {
            normal: p.normal.map(|c| c as f32),
            dist: p.dist as f32,
            plane_type: match plane.kind() {
                PlaneType::X => PLANE_X,
                PlaneType::Y => PLANE_Y,
                PlaneType::Z => PLANE_Z,
                PlaneType::AnyX => PLANE_ANYX,
                PlaneType::AnyY => PLANE_ANYY,
                PlaneType::AnyZ => PLANE_ANYZ,
            },
        };
        self.file.planes.push(disk);
        self.planes_out.insert(planenum, out);
        Ok(out)
    }

    /// Texinfo `num`'s number in the file, written when first used; -1 for one not drawn,
    /// `ExportMapTexinfo` (QBSP-15.43).
    fn texinfo(&mut self, texinfos: &Texinfos, num: TexinfoNum) -> Result<i32, CompileError> {
        if let Some(&out) = self.texinfo_out.get(&num) {
            return Ok(out);
        }
        let texinfo = texinfos
            .get(num)
            .ok_or(CompileError::Limit("MAX_MAP_TEXINFO"))?;
        if texinfo.flags.has(SurfFlags::NODRAW) {
            return Ok(-1);
        }
        let out = index(self.file.texinfo.len(), "MAX_MAP_TEXINFO")?;
        self.file.texinfo.push(DiskTexinfo {
            vecs: texinfo.vecs,
            miptex: index(texinfo.miptex, "MAX_MAP_TEXINFO")?,
            flags: if texinfo.flags.has(SurfFlags::SPECIAL) {
                TEX_SPECIAL
            } else {
                0
            },
        });
        self.texinfo_out.insert(num, out);
        Ok(out)
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "a coordinate within the world's extent, in half-epsilon cells, fits an i64"
    )]
    fn cell(p: Vec3) -> [i64; 3] {
        p.map(|c| (c / POINT_EQUAL_EPSILON).floor() as i64)
    }

    /// The number of a vertex at `p`, `EmitVertex` (QBSP-15.44): the first written within half
    /// `POINT_EQUAL_EPSILON` of it on every axis, else `p` written.
    fn vertex(&mut self, p: Vec3) -> Result<u32, CompileError> {
        let [cx, cy, cz] = Self::cell(p);
        let mut best: Option<u32> = None;
        for x in [cx.saturating_sub(1), cx, cx.saturating_add(1)] {
            for y in [cy.saturating_sub(1), cy, cy.saturating_add(1)] {
                for z in [cz.saturating_sub(1), cz, cz.saturating_add(1)] {
                    for &v in self.vertex_cells.get(&[x, y, z]).into_iter().flatten() {
                        let near = usize::try_from(v)
                            .ok()
                            .and_then(|i| self.vertex_points.get(i));
                        let inside = near.is_some_and(|q| {
                            q.iter()
                                .zip(p)
                                .all(|(a, b)| (a - b).abs() <= HALF_POINT_EPSILON)
                        });
                        if inside && best.is_none_or(|b| v < b) {
                            best = Some(v);
                        }
                    }
                }
            }
        }
        if let Some(v) = best {
            return Ok(v);
        }
        let v = index(self.file.vertexes.len(), "MAX_MAP_VERTS")?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec QBSP-15.44: the BSP's vertices are `float`s"
        )]
        self.file.vertexes.push(p.map(|c| c as f32));
        self.vertex_points.push(p);
        self.vertex_cells.entry(Self::cell(p)).or_default().push(v);
        Ok(v)
    }

    /// Each face's vertices, `EmitVertices` (QBSP-15.44): the faces of each node, the node's own
    /// before its children's, front first, but those left out.
    pub(crate) fn emit_vertices(
        &mut self,
        tree: &Tree,
        maker: &FaceMaker<'_>,
    ) -> Result<HashMap<FaceId, Vec<Fragment>>, CompileError> {
        let mut out = HashMap::new();
        for id in tree.preorder() {
            let Some(node) = tree.node(id) else {
                continue;
            };
            for &face_id in &node.faces {
                let Some(face) = tree.face(face_id) else {
                    continue;
                };
                if maker.omits(tree, face) {
                    continue;
                }
                let vertices = face
                    .winding
                    .points()
                    .iter()
                    .map(|&p| self.vertex(p))
                    .collect::<Result<Vec<u32>, CompileError>>()?;
                out.insert(
                    face_id,
                    vec![Fragment {
                        vertices,
                        output: None,
                    }],
                );
            }
        }
        Ok(out)
    }

    /// The faces of the tree written with their edges, `EmitFaces` (QBSP-15.45): for each node
    /// in order its faces' fragments, each edge the reverse of one written for a face of the
    /// same front contents used again once, else written; a fragment of fewer than three points
    /// is not written. Each node's first face and count; the model's first face.
    pub(crate) fn emit_faces(
        &mut self,
        tree: &Tree,
        planes: &Planes,
        texinfos: &Texinfos,
        fragments: &mut HashMap<FaceId, Vec<Fragment>>,
    ) -> Result<EmittedFaces, CompileError> {
        let first = index(self.file.faces.len(), "MAX_MAP_FACES")?;
        let mut edges: HashMap<(u32, u32), HashEdge> = HashMap::new();
        let mut ranges = HashMap::new();
        for id in tree.preorder() {
            let Some(node) = tree.node(id) else {
                continue;
            };
            let first_face = index(self.file.faces.len(), "MAX_MAP_FACES")?;
            for &face_id in &node.faces {
                let Some(face) = tree.face(face_id) else {
                    continue;
                };
                for fragment in fragments.get_mut(&face_id).into_iter().flatten() {
                    let n = fragment.vertices.len();
                    let mut surfedges = Vec::with_capacity(n);
                    for (i, &v1) in fragment.vertices.iter().enumerate() {
                        let Some(&v2) = i
                            .saturating_add(1)
                            .checked_rem(n)
                            .and_then(|k| fragment.vertices.get(k))
                        else {
                            continue;
                        };
                        if let Some(existing) = edges.get_mut(&(v2, v1))
                            && existing.front == face.contents[0]
                            && !existing.reused
                        {
                            existing.reused = true;
                            surfedges.push(existing.index.saturating_neg());
                            continue;
                        }
                        let e = index(self.file.edges.len(), "MAX_MAP_EDGES")?;
                        self.file.edges.push([v1, v2]);
                        edges.entry((v1, v2)).or_insert(HashEdge {
                            index: e,
                            front: face.contents[0],
                            reused: false,
                        });
                        surfedges.push(e);
                    }
                    if n < MIN_FACE_POINTS {
                        continue;
                    }
                    fragment.output = Some(index(self.file.faces.len(), "MAX_MAP_FACES")?);
                    let firstedge = index(self.file.surfedges.len(), "MAX_MAP_SURFEDGES")?;
                    let numedges = index(surfedges.len(), "MAX_MAP_SURFEDGES")?;
                    self.file.surfedges.extend(surfedges);
                    let side = i32::from(face.planenum.get() & 1 == 1);
                    let planenum = self.plane(planes, face.planenum.positive())?;
                    let texinfo = self.texinfo(texinfos, face.texinfo)?;
                    self.file.faces.push(DiskFace {
                        planenum,
                        side,
                        firstedge,
                        numedges,
                        texinfo,
                        styles: [NO_STYLE; BSP_MAXLIGHTMAPS],
                        lightofs: -1,
                    });
                }
            }
            let count =
                index::<u32>(self.file.faces.len(), "MAX_MAP_FACES")?.saturating_sub(first_face);
            ranges.insert(id, (first_face, count));
        }
        Ok((first, ranges))
    }

    /// A clipping hull's tree written, `ExportClipNodes` (QBSP-15.46): each node's clip node
    /// before its children's, a leaf its native contents; the head's number or contents.
    pub(crate) fn clip_nodes(&mut self, tree: &Tree, planes: &Planes) -> Result<i32, CompileError> {
        self.clip_node(tree, planes, Tree::HEAD)
    }

    fn clip_node(&mut self, tree: &Tree, planes: &Planes, id: NodeId) -> Result<i32, CompileError> {
        let Some((planenum, [front, back], _)) = tree.split_of(id) else {
            let contents = tree.leaf_data(id).map_or(Contents::EMPTY, |l| l.contents);
            return Ok(Game::Quake
                .to_native(contents)
                .map_or(SOLID_NATIVE, |n| n.value()));
        };
        let at = self.file.clipnodes.len();
        let num = index(at, "MAX_MAP_CLIPNODES")?;
        self.file.clipnodes.push(DiskClipNode {
            planenum: 0,
            children: [0, 0],
        });
        let children = [
            self.clip_node(tree, planes, front)?,
            self.clip_node(tree, planes, back)?,
        ];
        let planenum = self.plane(planes, planenum)?;
        if let Some(node) = self.file.clipnodes.get_mut(at) {
            *node = DiskClipNode { planenum, children };
        }
        Ok(num)
    }

    /// Leaf `id` written, `ExportLeaf` (QBSP-15.46): its contents remapped and made native, its
    /// bounds rounded out, and for a leaf not solid each drawn face it touches.
    fn leaf(
        &mut self,
        tree: &Tree,
        texinfos: &Texinfos,
        id: NodeId,
        fragments: &HashMap<FaceId, Vec<Fragment>>,
    ) -> Result<(), CompileError> {
        let (Some(node), Some(leaf)) = (tree.node(id), tree.leaf_data(id)) else {
            return Ok(());
        };
        let remapped = Game::Quake.remap_for_export(leaf.contents);
        let contents = Game::Quake.to_native(remapped).map_or(-1, |n| n.value());
        let firstmarksurface = index(self.file.marksurfaces.len(), "MAX_MAP_MARKSURFACES")?;
        if !remapped.is_any_solid() {
            for &face in &leaf.markfaces {
                let drawn = tree
                    .face(face)
                    .is_some_and(|f| !texinfos.flags(f.texinfo).has(SurfFlags::NODRAW));
                if !drawn {
                    continue;
                }
                for fragment in fragments.get(&face).into_iter().flatten() {
                    if let Some(out) = fragment.output {
                        self.file.marksurfaces.push(out);
                    }
                }
            }
        }
        let nummarksurfaces = index::<u32>(self.file.marksurfaces.len(), "MAX_MAP_MARKSURFACES")?
            .saturating_sub(firstmarksurface);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec QBSP-15.46: the bounds are rounded out to whole units"
        )]
        self.file.leafs.push(DiskLeaf {
            contents,
            visofs: -1,
            mins: node.bounds.mins.map(|c| c.floor() as f32),
            maxs: node.bounds.maxs.map(|c| c.ceil() as f32),
            firstmarksurface,
            nummarksurfaces,
            ambient_level: [0; BSP_NUM_AMBIENTS],
        });
        Ok(())
    }

    /// The drawing hull's tree written, `ExportDrawNodes` (QBSP-15.46): each node before its
    /// children, a child that is any solid the shared leaf 0, else its leaf written then; the
    /// model's head node, faces, leaves and bounds, the side space taken off and shrunk by 1.
    pub(crate) fn draw_nodes(
        &mut self,
        tree: &Tree,
        planes: &Planes,
        texinfos: &Texinfos,
        fragments: &HashMap<FaceId, Vec<Fragment>>,
        faces: (u32, &HashMap<NodeId, (u32, u32)>),
        model: &mut DiskModel,
    ) -> Result<(), CompileError> {
        model.headnode[0] = index(self.file.nodes.len(), "MAX_MAP_NODES")?;
        model.firstface = index(usize::try_from(faces.0).unwrap_or(0), "MAX_MAP_FACES")?;
        model.numfaces =
            index::<i32>(self.file.faces.len(), "MAX_MAP_FACES")?.saturating_sub(model.firstface);
        let leaves_before = self.file.leafs.len();
        if tree.split_of(Tree::HEAD).is_some() {
            self.draw_node(tree, planes, texinfos, fragments, faces.1, Tree::HEAD)?;
        } else {
            self.leaf(tree, texinfos, Tree::HEAD, fragments)?;
        }
        model.visleafs = index(
            self.file.leafs.len().saturating_sub(leaves_before),
            "MAX_MAP_LEAFS",
        )?;
        if let Some(head) = tree.node(Tree::HEAD) {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "spec QBSP-15.46: `dmodel_t` stores `float` bounds"
            )]
            {
                model.mins = head
                    .bounds
                    .mins
                    .map(|c| (c + SIDESPACE + MODEL_SHRINK) as f32);
                model.maxs = head
                    .bounds
                    .maxs
                    .map(|c| (c - SIDESPACE - MODEL_SHRINK) as f32);
            }
        }
        Ok(())
    }

    fn draw_node(
        &mut self,
        tree: &Tree,
        planes: &Planes,
        texinfos: &Texinfos,
        fragments: &HashMap<FaceId, Vec<Fragment>>,
        ranges: &HashMap<NodeId, (u32, u32)>,
        id: NodeId,
    ) -> Result<(), CompileError> {
        let (Some(node), Some((planenum, children, _))) = (tree.node(id), tree.split_of(id)) else {
            return Ok(());
        };
        let at = self.file.nodes.len();
        let (firstface, numfaces) = ranges.get(&id).copied().unwrap_or((0, 0));
        let planenum = self.plane(planes, planenum)?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec QBSP-15.46: the bounds are rounded out to whole units"
        )]
        self.file.nodes.push(DiskNode {
            planenum,
            children: [0, 0],
            mins: node.bounds.mins.map(|c| c.floor() as f32),
            maxs: node.bounds.maxs.map(|c| c.ceil() as f32),
            firstface,
            numfaces,
        });
        let mut written = [0_i32; 2];
        for (slot, child) in written.iter_mut().zip(children) {
            match tree.leaf_data(child) {
                Some(leaf) if leaf.contents.is_any_solid() => *slot = -1,
                Some(_) => {
                    let leaf_index: i32 = index(self.file.leafs.len(), "MAX_MAP_LEAFS")?;
                    *slot = leaf_index.saturating_add(1).saturating_neg();
                    self.leaf(tree, texinfos, child, fragments)?;
                }
                None => {
                    *slot = index(self.file.nodes.len(), "MAX_MAP_NODES")?;
                    self.draw_node(tree, planes, texinfos, fragments, ranges, child)?;
                }
            }
        }
        if let Some(n) = self.file.nodes.get_mut(at) {
            n.children = written;
        }
        Ok(())
    }

    /// The file finished, `FinishBSPFile`: a vertex 0 when none was written.
    pub(crate) fn finish(&mut self) {
        if self.file.vertexes.is_empty() {
            self.file.vertexes.push([0.0; AXES]);
        }
    }
}

impl Tree {
    /// Its split nodes in `_R` order: each before its children, the front first.
    fn preorder(&self) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![Self::HEAD];
        while let Some(id) = stack.pop() {
            if let Some((_, [front, back], _)) = self.split_of(id) {
                out.push(id);
                stack.push(back);
                stack.push(front);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
