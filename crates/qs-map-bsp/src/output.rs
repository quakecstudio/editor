// SPDX-License-Identifier: GPL-3.0-or-later

//! Writing the trees into the BSP's lumps (QBSP-13): planes, vertexes, edges and faces, nodes
//! and leaves, clip nodes and models.

use std::collections::{BTreeMap, HashMap};

use qs_bsp::write::{BspFile, DiskClipNode, DiskFace, DiskLeaf, DiskModel, DiskNode, DiskPlane};
use qs_bsp::{BSP_MAXLIGHTMAPS, BSP_NUM_AMBIENTS};

use crate::math::{ORIGIN, Vec3, all, cyclic, rint, within};
use crate::planes::{PlaneId, PlaneType, Planes};
use crate::tree::{NodeKind, SIDESPACE, Tree};
use crate::{CompileError, Contents, Hull, Limits};

/// How near two vertices are one (QBSP-13.3).
const POINT_EPSILON: f64 = 0.01;
/// How near a whole number a vertex coordinate is set to it (QBSP-13.3).
const WHOLE_EPSILON: f64 = 0.001;
/// The vertex hash's buckets, and its box and scale (QBSP-13.3).
const VERTEX_BUCKETS: usize = 4096;
const VERTEX_HASH_MIN: f32 = -8000.0;
const VERTEX_HASH_SCALE: f32 = 0.004;
const VERTEX_HASH_ROW: f32 = 64.0;
/// A face's styles before light (QBSP-13.4).
const NO_STYLES: [u8; BSP_MAXLIGHTMAPS] = [255; BSP_MAXLIGHTMAPS];
/// BSP29's limits (QBSP-13.10, BSP-16).
const MAX_MAP_NODES: usize = 32767;
const MAX_MAP_CLIPNODES: usize = 32767;
const MAX_MAP_LEAFS: usize = 32767;
const MAX_MAP_VERTS: usize = 65535;
const MAX_MAP_FACES: usize = 65535;
const MAX_MAP_MARKSURFACES: usize = 65535;
const MAX_MAP_EDGES: usize = 256_000;
const MAX_MAP_SURFEDGES: usize = 512_000;
const MAX_MAP_MODELS: usize = 256;

fn index<T: TryFrom<usize>>(value: usize, limit: &'static str) -> Result<T, CompileError> {
    T::try_from(value).map_err(|_| CompileError::Limit(limit))
}

/// The lumps being written for one hull.
pub(crate) struct Output {
    pub(crate) file: BspFile,
    /// Each edge's first face's front contents, and whether a second face shares it.
    edge_faces: Vec<(Contents, bool)>,
    /// The edges from each vertex pair, oldest first.
    edge_index: HashMap<(usize, usize), Vec<usize>>,
    first_model_edge: usize,
    first_face: usize,
    /// The most output planes (QBSP-2.5).
    plane_limit: usize,
}

/// What a node writes its faces as.
#[derive(Clone, Copy, Debug, Default)]
struct FaceRange {
    first: usize,
    count: usize,
}

impl Output {
    /// The lumps as `BeginBSPFile` starts them: edge 0 unused, leaf 0 the shared solid leaf
    /// (QBSP-13.1).
    pub(crate) fn new(limits: Limits) -> Self {
        let mut file = BspFile::default();
        file.edges.push([0, 0]);
        file.leafs.push(DiskLeaf {
            contents: Contents::Solid.value(),
            visofs: 0,
            mins: ORIGIN,
            maxs: ORIGIN,
            firstmarksurface: 0,
            nummarksurfaces: 0,
            ambient_level: [0; BSP_NUM_AMBIENTS],
        });
        Self {
            file,
            edge_faces: vec![(Contents::Solid, true)],
            edge_index: HashMap::new(),
            first_model_edge: 1,
            first_face: 0,
            plane_limit: limits.planes,
        }
    }

    /// `WriteNodePlanes`: each plane of the tree once for this model, in tree order
    /// (QBSP-13.2).
    pub(crate) fn node_planes(
        &mut self,
        tree: &Tree,
        planes: &Planes,
    ) -> Result<BTreeMap<PlaneId, usize>, CompileError> {
        let mut mapping = BTreeMap::new();
        let mut work = vec![tree.head()];
        while let Some(node) = work.pop() {
            let Some(NodeKind::Decision {
                plane, children, ..
            }) = tree.nodes.get(node).map(|n| &n.kind)
            else {
                continue;
            };
            if !mapping.contains_key(plane) {
                if self.file.planes.len() >= self.plane_limit {
                    return Err(CompileError::Limit("numplanes == MAX_MAP_PLANES"));
                }
                let stored = planes.get(*plane);
                mapping.insert(*plane, self.file.planes.len());
                self.file.planes.push(DiskPlane {
                    normal: stored.normal,
                    dist: stored.dist,
                    plane_type: planes.kind(*plane).number(),
                });
            }
            work.push(children[1]);
            work.push(children[0]);
        }
        Ok(mapping)
    }

    /// `MakeFaceEdges` and `GrowNodeRegions`: the vertexes and edges of every node face, then
    /// the faces, node by node in tree order (QBSP-13.3, QBSP-13.4).
    fn face_edges(
        &mut self,
        tree: &mut Tree,
        mapping: &BTreeMap<PlaneId, usize>,
    ) -> Result<HashMap<usize, FaceRange>, CompileError> {
        let mut vertexes = VertexHash::new();
        let mut order = Vec::new();
        let mut work = vec![tree.head()];
        while let Some(node) = work.pop() {
            if let Some(NodeKind::Decision { children, .. }) = tree.nodes.get(node).map(|n| &n.kind)
            {
                order.push(node);
                work.push(children[1]);
                work.push(children[0]);
            }
        }
        for &node in &order {
            let Some(NodeKind::Decision { faces, .. }) =
                tree.nodes.get(node).map(|n| n.kind.clone())
            else {
                continue;
            };
            for id in faces {
                let Some(face) = tree.faces.get(id).map(|f| f.face.clone()) else {
                    continue;
                };
                let count = face.points.len();
                let mut edges = Vec::with_capacity(count);
                for i in 0..count {
                    let (Some(&a), Some(&b)) =
                        (face.points.get(i), face.points.get(cyclic(i, 1, count)))
                    else {
                        continue;
                    };
                    let v1 = vertexes.get(a, &mut self.file)?;
                    let v2 = vertexes.get(b, &mut self.file)?;
                    edges.push(self.edge(v1, v2, face.contents[0])?);
                }
                if let Some(node_face) = tree.faces.get_mut(id) {
                    node_face.edges = edges;
                }
            }
        }
        let mut ranges = HashMap::new();
        for node in order {
            let Some(NodeKind::Decision { plane, faces, .. }) =
                tree.nodes.get(node).map(|n| n.kind.clone())
            else {
                continue;
            };
            let first = self.file.faces.len();
            let planenum = mapping.get(&plane).copied().unwrap_or_default();
            for id in faces {
                let Some(node_face) = tree.faces.get_mut(id) else {
                    continue;
                };
                if self.file.faces.len() >= MAX_MAP_FACES {
                    return Err(CompileError::Limit("MAX_MAP_FACES"));
                }
                node_face.output = Some(self.file.faces.len());
                let firstedge = index::<i32>(
                    self.file.surfedges.len(),
                    "numsurfedges == MAX_MAP_SURFEDGES",
                )?;
                let surfedges = self.file.surfedges.len();
                if surfedges.saturating_add(node_face.edges.len())
                    >= MAX_MAP_SURFEDGES.saturating_add(1)
                {
                    return Err(CompileError::Limit("numsurfedges == MAX_MAP_SURFEDGES"));
                }
                self.file.surfedges.extend(node_face.edges.iter().copied());
                self.file.faces.push(DiskFace {
                    planenum: index(planenum, "MAX_MAP_PLANES")?,
                    side: node_face.face.side.number(),
                    firstedge,
                    numedges: index(node_face.edges.len(), "MAXEDGES")?,
                    texinfo: index(node_face.face.texinfo, "MAX_MAP_TEXINFO")?,
                    styles: NO_STYLES,
                    lightofs: -1,
                });
            }
            ranges.insert(
                node,
                FaceRange {
                    first,
                    count: self.file.faces.len().saturating_sub(first),
                },
            );
        }
        self.first_model_edge = self.file.edges.len();
        Ok(ranges)
    }

    /// `GetEdge`: the reverse of an edge of this model one face uses with the same front
    /// contents as `-n`, else a new edge (QBSP-13.3).
    fn edge(&mut self, v1: usize, v2: usize, contents: Contents) -> Result<i32, CompileError> {
        if let Some(candidates) = self.edge_index.get(&(v2, v1)) {
            for &i in candidates {
                if i < self.first_model_edge {
                    continue;
                }
                if let Some(slot) = self.edge_faces.get_mut(i)
                    && !slot.1
                    && slot.0 == contents
                {
                    slot.1 = true;
                    return Ok(index::<i32>(i, "numedges == MAX_MAP_EDGES")?.wrapping_neg());
                }
            }
        }
        if self.file.edges.len() >= MAX_MAP_EDGES {
            return Err(CompileError::Limit("numedges == MAX_MAP_EDGES"));
        }
        let i = self.file.edges.len();
        self.file
            .edges
            .push([index(v1, "MAX_MAP_VERTS")?, index(v2, "MAX_MAP_VERTS")?]);
        self.edge_faces.push((contents, false));
        self.edge_index.entry((v1, v2)).or_default().push(i);
        index(i, "numedges == MAX_MAP_EDGES")
    }

    /// The drawing hull of a model: its planes, faces, nodes and leaves, and the model,
    /// `WriteNodePlanes`, `MakeFaceEdges` and `WriteDrawNodes` (QBSP-13.2 to QBSP-13.5).
    pub(crate) fn draw_model(
        &mut self,
        tree: &mut Tree,
        planes: &Planes,
    ) -> Result<(), CompileError> {
        let mapping = self.node_planes(tree, planes)?;
        let ranges = self.face_edges(tree, &mapping)?;
        if self.file.models.len() >= MAX_MAP_MODELS {
            return Err(CompileError::Limit("nummodels == MAX_MAP_MODELS"));
        }
        let headnode = index::<i32>(self.file.nodes.len(), "MAX_MAP_NODES")?;
        let firstface = self.first_face;
        self.first_face = self.file.faces.len();
        let start = self.file.leafs.len();
        let head = tree.head();
        if tree.contents(head).is_some() {
            // qbsp writes the leaf alone, and the head names the next node (QBSP-13.5).
            self.leaf(tree, head)?;
        } else {
            self.draw_node(tree, head, &mapping, &ranges)?;
        }
        let (mins, maxs) = tree
            .nodes
            .get(head)
            .map_or((ORIGIN, ORIGIN), |n| (n.mins, n.maxs));
        self.file.models.push(DiskModel {
            mins: mins.map(|c| c + SIDESPACE + 1.0),
            maxs: maxs.map(|c| c - SIDESPACE - 1.0),
            origin: ORIGIN,
            headnode: [headnode, 0, 0, 0],
            visleafs: index(self.file.leafs.len().saturating_sub(start), "MAX_MAP_LEAFS")?,
            firstface: index(firstface, "MAX_MAP_FACES")?,
            numfaces: index(
                self.file.faces.len().saturating_sub(firstface),
                "MAX_MAP_FACES",
            )?,
        });
        Ok(())
    }

    /// `WriteDrawNodes_r` (QBSP-13.5).
    fn draw_node(
        &mut self,
        tree: &Tree,
        node: usize,
        mapping: &BTreeMap<PlaneId, usize>,
        ranges: &HashMap<usize, FaceRange>,
    ) -> Result<(), CompileError> {
        let Some(n) = tree.nodes.get(node) else {
            return Ok(());
        };
        let NodeKind::Decision {
            plane, children, ..
        } = &n.kind
        else {
            return Ok(());
        };
        if self.file.nodes.len() >= MAX_MAP_NODES {
            return Err(CompileError::Limit("numnodes == MAX_MAP_NODES"));
        }
        let at = self.file.nodes.len();
        let range = ranges.get(&node).copied().unwrap_or_default();
        self.file.nodes.push(DiskNode {
            planenum: index(
                mapping.get(plane).copied().unwrap_or_default(),
                "MAX_MAP_PLANES",
            )?,
            children: [0, 0],
            mins: n.mins,
            maxs: n.maxs,
            firstface: index(range.first, "MAX_MAP_FACES")?,
            numfaces: index(range.count, "MAX_MAP_FACES")?,
        });
        for (slot, &child) in children.iter().enumerate() {
            let value = match tree.contents(child) {
                Some(Contents::Solid) => -1,
                Some(_) => {
                    let leaf = self.file.leafs.len();
                    self.leaf(tree, child)?;
                    index::<i32>(leaf.saturating_add(1), "MAX_MAP_LEAFS")?.wrapping_neg()
                }
                None => {
                    let next = index::<i32>(self.file.nodes.len(), "MAX_MAP_NODES")?;
                    self.draw_node(tree, child, mapping, ranges)?;
                    next
                }
            };
            if let Some(written) = self.file.nodes.get_mut(at)
                && let Some(c) = written.children.get_mut(slot)
            {
                *c = value;
            }
        }
        Ok(())
    }

    /// `WriteLeaf`: its contents, bounds and marked faces with the pieces cut from them
    /// (QBSP-13.5).
    fn leaf(&mut self, tree: &Tree, node: usize) -> Result<(), CompileError> {
        let Some(n) = tree.nodes.get(node) else {
            return Ok(());
        };
        let NodeKind::Leaf { contents, marks } = &n.kind else {
            return Ok(());
        };
        if self.file.leafs.len() >= MAX_MAP_LEAFS {
            return Err(CompileError::Limit("MAX_MAP_LEAFS"));
        }
        let first = self.file.marksurfaces.len();
        for &mark in marks {
            let Some(face) = tree.faces.get(mark) else {
                continue;
            };
            for id in core::iter::once(mark).chain(face.pieces.iter().copied()) {
                let output = tree
                    .faces
                    .get(id)
                    .and_then(|f| f.output)
                    .unwrap_or_default();
                if self.file.marksurfaces.len() >= MAX_MAP_MARKSURFACES {
                    return Err(CompileError::Limit(
                        "nummarksurfaces == MAX_MAP_MARKSURFACES",
                    ));
                }
                self.file.marksurfaces.push(index(output, "MAX_MAP_FACES")?);
            }
        }
        self.file.leafs.push(DiskLeaf {
            contents: contents.value(),
            visofs: -1,
            mins: n.mins,
            maxs: n.maxs,
            firstmarksurface: index(first, "MAX_MAP_MARKSURFACES")?,
            nummarksurfaces: index(
                self.file.marksurfaces.len().saturating_sub(first),
                "MAX_MAP_MARKSURFACES",
            )?,
            ambient_level: [0; BSP_NUM_AMBIENTS],
        });
        Ok(())
    }
}

/// `GetVertex`'s hash of one model's vertexes (QBSP-13.3).
struct VertexHash {
    buckets: Vec<Vec<(Vec3, usize)>>,
}

impl VertexHash {
    fn new() -> Self {
        Self {
            buckets: vec![Vec::new(); VERTEX_BUCKETS],
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "spec QBSP-13.3: qbsp converts the float to an unsigned bucket"
    )]
    fn bucket(v: Vec3) -> usize {
        let h = VERTEX_HASH_SCALE * (v[0] - VERTEX_HASH_MIN) * VERTEX_HASH_ROW
            + VERTEX_HASH_SCALE * (v[1] - VERTEX_HASH_MIN);
        if h < 0.0 {
            return VERTEX_BUCKETS - 1;
        }
        (h as usize).min(VERTEX_BUCKETS - 1)
    }

    /// The vertex for `point`, written when new.
    fn get(&mut self, point: Vec3, file: &mut BspFile) -> Result<usize, CompileError> {
        let vert = point.map(|c| {
            let whole = rint(c);
            if within(c, whole, WHOLE_EPSILON) {
                whole
            } else {
                c
            }
        });
        let bucket = Self::bucket(vert);
        if let Some(found) = self.buckets.get(bucket).and_then(|list| {
            list.iter()
                .rev()
                .find(|(known, _)| all(*known, vert, |a, b| within(a, b, POINT_EPSILON)))
        }) {
            return Ok(found.1);
        }
        if file.vertexes.len() >= MAX_MAP_VERTS {
            return Err(CompileError::Limit("numvertexes == MAX_MAP_VERTS"));
        }
        let id = file.vertexes.len();
        file.vertexes.push(vert);
        if let Some(list) = self.buckets.get_mut(bucket) {
            list.push((vert, id));
        }
        Ok(id)
    }
}

/// A clipping hull as the text file passes it to the drawing hull: each model's head clip node
/// and the clip nodes with their planes (QBSP-13.6, QBSP-13.7).
#[derive(Clone, Debug)]
pub(crate) struct ClipHull {
    pub(crate) heads: Vec<i32>,
    pub(crate) nodes: Vec<(Vec3, f32, [i32; 2])>,
}

impl ClipHull {
    /// No model yet.
    pub(crate) const fn new() -> Self {
        Self {
            heads: Vec::new(),
            nodes: Vec::new(),
        }
    }

    /// `WriteClipNodes` for one model's tree: its head clip node (QBSP-13.6).
    pub(crate) fn add_model(&mut self, tree: &Tree, planes: &Planes) -> Result<(), CompileError> {
        let next = index(self.nodes.len(), "MAX_MAP_CLIPNODES")?;
        // qbsp names the next clip node even when the head is a leaf (QBSP-13.6).
        self.clip_node(tree, tree.head(), planes)?;
        self.heads.push(next);
        Ok(())
    }

    fn clip_node(
        &mut self,
        tree: &Tree,
        node: usize,
        planes: &Planes,
    ) -> Result<i32, CompileError> {
        let Some(n) = tree.nodes.get(node) else {
            return Ok(Contents::Solid.value());
        };
        match &n.kind {
            NodeKind::Leaf { contents, .. } => Ok(contents.value()),
            NodeKind::Decision {
                plane, children, ..
            } => {
                if self.nodes.len() >= MAX_MAP_CLIPNODES {
                    return Err(CompileError::Limit("MAX_MAP_CLIPNODES"));
                }
                let at = self.nodes.len();
                let stored = planes.get(*plane);
                self.nodes.push((stored.normal, stored.dist, [0, 0]));
                let front = self.clip_node(tree, children[0], planes)?;
                let back = self.clip_node(tree, children[1], planes)?;
                if let Some(slot) = self.nodes.get_mut(at) {
                    slot.2 = [front, back];
                }
                index(at, "MAX_MAP_CLIPNODES")
            }
        }
    }
}

/// A float through `%f` and back through `%f`: six decimals (QBSP-13.7, QBSP-13.8).
fn through_text(value: f32) -> f32 {
    format!("{:.6}", f64::from(value))
        .parse::<f32>()
        .unwrap_or(value)
}

impl Output {
    /// `ReadClipHull`: the hull's clip nodes appended, their planes found among the output
    /// planes exactly after the text round trip, each model's head set (QBSP-13.8).
    pub(crate) fn read_clip_hull(
        &mut self,
        hull: Hull,
        clip: &ClipHull,
    ) -> Result<(), CompileError> {
        if clip.heads.len() != self.file.models.len() {
            return Err(CompileError::Limit(
                "ReadClipHull: hull had a different model count",
            ));
        }
        let first = index::<i32>(self.file.clipnodes.len(), "MAX_MAP_CLIPNODES")?;
        for (model, &head) in self.file.models.iter_mut().zip(&clip.heads) {
            if let Some(slot) = model.headnode.get_mut(hull.number()) {
                *slot = if head < 0 {
                    head
                } else {
                    first.saturating_add(head)
                };
            }
        }
        for &(normal, dist, children) in &clip.nodes {
            if self.file.clipnodes.len() >= MAX_MAP_CLIPNODES {
                return Err(CompileError::Limit("ReadClipHull: MAX_MAP_CLIPNODES"));
            }
            let normal = normal.map(through_text);
            let dist = through_text(dist);
            let plane_type = PlaneType::of(normal).number();
            let planenum = self.final_plane(normal, dist, plane_type)?;
            // A node child moves past the clip nodes already written; a contents value stays.
            let child = |c: i32| {
                let moved = if c >= 0 { c.saturating_add(first) } else { c };
                Ok::<i32, CompileError>(moved)
            };
            self.file.clipnodes.push(DiskClipNode {
                planenum: index(planenum, "MAX_MAP_PLANES")?,
                children: [child(children[0])?, child(children[1])?],
            });
        }
        Ok(())
    }

    /// `FindFinalPlane`: the first output plane equal in type, distance and normal, or a new
    /// one (QBSP-13.8).
    #[expect(
        clippy::float_cmp,
        reason = "spec QBSP-13.8: qbsp compares the planes exactly"
    )]
    fn final_plane(
        &mut self,
        normal: Vec3,
        dist: f32,
        plane_type: i32,
    ) -> Result<usize, CompileError> {
        if let Some(found) = self
            .file
            .planes
            .iter()
            .position(|p| p.plane_type == plane_type && p.dist == dist && p.normal == normal)
        {
            return Ok(found);
        }
        if self.file.planes.len() >= self.plane_limit {
            return Err(CompileError::Limit("numplanes == MAX_MAP_PLANES"));
        }
        self.file.planes.push(DiskPlane {
            normal,
            dist,
            plane_type,
        });
        Ok(self.file.planes.len().saturating_sub(1))
    }
}
