// SPDX-License-Identifier: GPL-3.0-or-later

//! Test maps: a BSP built from a spec of its planes, nodes, leaves and clipnodes.

#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or overflow is a failed test"
)]

use qs_bsp::{
    BSP_HEADER_LUMPS, BSP_MAX_MAP_HULLS, BSP_NUM_AMBIENTS, Bsp, BspVersion, ClipNode, Leaf, Lump,
    Model, Node, Plane,
};

pub(crate) const CONTENTS_EMPTY: i32 = -1;
pub(crate) const CONTENTS_SOLID: i32 = -2;

pub(crate) fn plane(normal: [f32; 3], dist: f32, plane_type: i32) -> Plane {
    let signbits = normal
        .iter()
        .enumerate()
        .map(|(i, &n)| u8::from(n < 0.0) << i)
        .sum();
    Plane {
        normal,
        dist,
        plane_type,
        signbits,
    }
}

fn leaf(contents: i32) -> Leaf {
    Leaf {
        contents,
        visofs: -1,
        mins: [0.0; 3],
        maxs: [0.0; 3],
        firstmarksurface: 0,
        nummarksurfaces: 0,
        ambient_level: [0; BSP_NUM_AMBIENTS],
    }
}

pub(crate) fn node(planenum: usize, children: [i32; 2]) -> Node {
    Node {
        planenum,
        children,
        mins: [0.0; 3],
        maxs: [0.0; 3],
        firstface: 0,
        numfaces: 0,
    }
}

/// A test map: what each part of its tree is.
#[derive(Clone)]
pub(crate) struct MapSpec {
    /// The planes.
    pub(crate) planes: Vec<Plane>,
    /// The drawing nodes; a child `-1 - c` is leaf `c`.
    pub(crate) nodes: Vec<Node>,
    /// The leaves' contents; leaf 0 is solid.
    pub(crate) leafs: Vec<i32>,
    /// The clipnodes of hulls 1 and 2.
    pub(crate) clipnodes: Vec<ClipNode>,
}

impl Default for MapSpec {
    /// A floor at z = 0 that is solid below, and, for hulls 1 and 2, at z = 24.
    fn default() -> Self {
        Self {
            planes: vec![
                plane([0.0, 0.0, 1.0], 0.0, 2),
                plane([0.0, 0.0, 1.0], 24.0, 2),
            ],
            nodes: vec![node(0, [-2, -1])],
            leafs: vec![CONTENTS_SOLID, CONTENTS_EMPTY],
            clipnodes: vec![ClipNode {
                planenum: 1,
                children: [CONTENTS_EMPTY, CONTENTS_SOLID],
            }],
        }
    }
}

pub(crate) fn bsp(spec: &MapSpec) -> Bsp<'static> {
    Bsp {
        version: BspVersion::Bsp29,
        lumps: [Lump::default(); BSP_HEADER_LUMPS],
        entity_lump: b"",
        planes: spec.planes.clone(),
        textures: Vec::new(),
        vertexes: Vec::new(),
        visibility: b"",
        nodes: spec.nodes.clone(),
        texinfo: Vec::new(),
        faces: Vec::new(),
        lighting: b"",
        clipnodes: spec.clipnodes.clone(),
        leafs: spec.leafs.iter().map(|&c| leaf(c)).collect(),
        marksurfaces: Vec::new(),
        edges: Vec::new(),
        surfedges: Vec::new(),
        models: vec![Model {
            mins: [-1000.0, -1000.0, -1000.0],
            maxs: [1000.0, 500.0, 1000.0],
            origin: [0.0; 3],
            headnode: [0; BSP_MAX_MAP_HULLS],
            visleafs: 0,
            firstface: 0,
            numfaces: 0,
            radius: 0.0,
        }],
    }
}
