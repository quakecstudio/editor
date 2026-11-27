// SPDX-License-Identifier: GPL-3.0-or-later

//! What compiling the open map found (P6-11): whether it leaks and where, the textures and
//! WADs it lacks, and each lump's count and the format it was written in (ENGT-5.4), for
//! `map.budget`. The report is kept with the map, for the map view.

use std::path::PathBuf;

use qs_bsp::LumpId;
use qs_bsp::write::BspCounts;
use qs_map::qbsp::QbspWarning;
use qs_map_bsp::{Compiled, Warning};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Whether the outside of a map was filled (QBSP-9.2, QBSP-9.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Seal {
    /// The outside was filled: no entity is in reach of the void.
    #[default]
    Sealed,
    /// The outside reaches an entity.
    Leaks,
    /// No entity stands in empty space, so nothing could be filled and a leak cannot be found.
    NoOccupant,
}

/// The format a compile wrote (QBSP-14.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Written {
    /// id's BSP29.
    #[default]
    Bsp29,
    /// BSP2.
    Bsp2 {
        /// The lump a value of which is too wide for BSP29; none for a BSP opened as BSP2.
        lump: Option<BspLump>,
    },
}

/// A lump of a BSP file (BSP-1.1), as `qs_bsp::LumpId` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BspLump {
    /// The entity text.
    Entities,
    /// The planes.
    Planes,
    /// The textures.
    Textures,
    /// The vertexes.
    Vertexes,
    /// The visibility.
    Visibility,
    /// The nodes.
    Nodes,
    /// The texinfo.
    Texinfo,
    /// The faces.
    Faces,
    /// The lighting.
    Lighting,
    /// The clipnodes.
    Clipnodes,
    /// The leafs.
    Leafs,
    /// The marksurfaces.
    Marksurfaces,
    /// The edges.
    Edges,
    /// The surfedges.
    Surfedges,
    /// The models.
    Models,
}

impl From<LumpId> for BspLump {
    fn from(lump: LumpId) -> Self {
        match lump {
            LumpId::Entities => Self::Entities,
            LumpId::Planes => Self::Planes,
            LumpId::Textures => Self::Textures,
            LumpId::Vertexes => Self::Vertexes,
            LumpId::Visibility => Self::Visibility,
            LumpId::Nodes => Self::Nodes,
            LumpId::Texinfo => Self::Texinfo,
            LumpId::Faces => Self::Faces,
            LumpId::Lighting => Self::Lighting,
            LumpId::Clipnodes => Self::Clipnodes,
            LumpId::Leafs => Self::Leafs,
            LumpId::Marksurfaces => Self::Marksurfaces,
            LumpId::Edges => Self::Edges,
            LumpId::Surfedges => Self::Surfedges,
            LumpId::Models => Self::Models,
        }
    }
}

impl BspLump {
    /// Its name in id's messages, such as `clipnodes`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Entities => "entities",
            Self::Planes => "planes",
            Self::Textures => "textures",
            Self::Vertexes => "vertexes",
            Self::Visibility => "visibility",
            Self::Nodes => "nodes",
            Self::Texinfo => "texinfo",
            Self::Faces => "faces",
            Self::Lighting => "lighting",
            Self::Clipnodes => "clipnodes",
            Self::Leafs => "leafs",
            Self::Marksurfaces => "marksurfaces",
            Self::Edges => "edges",
            Self::Surfedges => "surfedges",
            Self::Models => "models",
        }
    }
}

/// What the BSP a compile wrote holds, counted (ENGT-5.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LumpCounts {
    /// Models, the world's first.
    pub models: usize,
    /// Entities of the entity lump.
    pub entities: usize,
    /// Bytes of entity text, its NUL included.
    pub entity_text: usize,
    /// Planes.
    pub planes: usize,
    /// Nodes.
    pub nodes: usize,
    /// Clipnodes.
    pub clipnodes: usize,
    /// Leaves.
    pub leafs: usize,
    /// The world model's `visleafs`.
    pub visleafs: usize,
    /// Vertexes.
    pub vertexes: usize,
    /// Faces.
    pub faces: usize,
    /// Marksurfaces.
    pub marksurfaces: usize,
    /// Texinfo.
    pub texinfo: usize,
    /// Edges.
    pub edges: usize,
    /// Surfedges.
    pub surfedges: usize,
    /// Textures.
    pub textures: usize,
    /// Bytes of lighting: none from a compile, which does not light the map.
    pub lighting: usize,
    /// Bytes of visibility: none from a compile, which does not vis the map.
    pub visibility: usize,
    /// The largest extent in texels of a face that is not `TEX_SPECIAL`, by id's rule.
    pub face_extent: usize,
    /// The same by the FitzQuake lineage's rule.
    pub fitz_face_extent: usize,
}

impl From<BspCounts> for LumpCounts {
    fn from(c: BspCounts) -> Self {
        Self {
            models: c.models,
            entities: c.entities,
            entity_text: c.entity_text,
            planes: c.planes,
            nodes: c.nodes,
            clipnodes: c.clipnodes,
            leafs: c.leafs,
            visleafs: c.visleafs,
            vertexes: c.vertexes,
            faces: c.faces,
            marksurfaces: c.marksurfaces,
            texinfo: c.texinfo,
            edges: c.edges,
            surfedges: c.surfedges,
            textures: c.textures,
            lighting: c.lighting,
            visibility: c.visibility,
            face_extent: c.face_extent,
            fitz_face_extent: c.fitz_face_extent,
        }
    }
}

/// What compiling the open map found.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapReport {
    /// Whether the outside was filled.
    pub seal: Seal,
    /// When the map leaks, the way from the entity the outside reached out to the void: its
    /// origin, then the middle of each portal crossed; empty when it does not (QBSP-9.8).
    pub leak: Vec<[f32; 3]>,
    /// The textures the map's faces name that its WADs lack, each once (MAP-6.8).
    pub missing_textures: Vec<String>,
    /// The WADs the world names that were not found, whose textures could not be read.
    pub missing_wads: Vec<String>,
    /// What qbsp would print and go on after, one line each.
    pub warnings: Vec<String>,
    /// The files written, for `map.compile`.
    pub written: Vec<PathBuf>,
    /// The format the BSP was written in.
    pub format: Written,
    /// What the BSP holds, counted.
    pub lumps: LumpCounts,
}

impl MapReport {
    /// The report of `compiled`, whose map named the WADs `missing_wads` that were not found.
    #[must_use]
    pub fn of(compiled: &Compiled, missing_wads: Vec<String>) -> Self {
        let mut missing_textures = Vec::new();
        let mut seal = Seal::Sealed;
        let mut format = Written::Bsp29;
        for warning in &compiled.warnings {
            match warning {
                Warning::Map(QbspWarning::TextureNotFound { name }) => {
                    let name = String::from_utf8_lossy(name).into_owned();
                    if !missing_textures.contains(&name) {
                        missing_textures.push(name);
                    }
                }
                Warning::Leak => seal = Seal::Leaks,
                Warning::NoOccupant => seal = Seal::NoOccupant,
                Warning::Bsp2(lump) => {
                    format = Written::Bsp2 {
                        lump: Some((*lump).into()),
                    };
                }
                Warning::NoBrushFaces | Warning::PortalClippedAway | Warning::Map(_) => {}
            }
        }
        Self {
            seal,
            leak: compiled.leak_trail.clone(),
            missing_textures,
            missing_wads,
            warnings: compiled.warnings.iter().map(ToString::to_string).collect(),
            written: Vec::new(),
            format,
            lumps: compiled.counts.into(),
        }
    }
}
