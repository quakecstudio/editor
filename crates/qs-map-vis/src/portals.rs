// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: common/prtfile.cc, LoadPrtFile

//! Reading the portal file (VIS-2): each portal of the file as two one-way portals, one in each
//! of its leaves looking into the other. A `PRT2` file, ericw-tools' (VIS-7), numbers its portals
//! by cluster of leaves and ends with the leaves of each cluster.

use crate::VisError;
use crate::winding::{Plane, Winding};

/// The portal file's first word (VIS-2.8).
const PORTAL_FILE: &str = "PRT1";
/// The first word of ericw-tools' portal file numbered by cluster (VIS-7.1).
const PORTAL_FILE_2: &str = "PRT2";
/// `MAX_POINTS_ON_WINDING` (VIS-2.2).
pub(crate) const VIS_MAX_POINTS_ON_WINDING: usize = 64;
/// `MAX_PORTALS_ON_LEAF` (VIS-2.4).
pub(crate) const VIS_MAX_PORTALS_ON_LEAF: usize = 128;

/// A one-way portal: its winding, its plane facing the leaf it looks into, and that leaf.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Portal {
    pub(crate) winding: Winding,
    pub(crate) plane: Plane,
    pub(crate) leaf: usize,
}

/// Which portal file it is: id's, numbered by leaf, or ericw-tools', numbered by cluster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    /// `PRT1` (VIS-2).
    Leaves,
    /// `PRT2` (VIS-7).
    Clusters,
}

/// The portal file read: what its portals number (for `PRT2` clusters, for `PRT1` leaves, here
/// both called leaves), the one-way portals in file order (each file portal's forward one, then
/// its backward one), each leaf's portals by place, and each real leaf's cluster.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PortalFile {
    pub(crate) format: Format,
    pub(crate) leafs: usize,
    pub(crate) portals: Vec<Portal>,
    pub(crate) leaf_portals: Vec<Vec<usize>>,
    /// The cluster of each of the BSP's vis leaves: itself for `PRT1`.
    pub(crate) clusters: Vec<usize>,
}

/// The words of the text after the first, as `fscanf` reads them, the parentheses apart.
struct Words<'a> {
    rest: std::str::SplitWhitespace<'a>,
}

impl<'a> Words<'a> {
    fn next(&mut self) -> Option<&'a str> {
        self.rest.next()
    }

    /// The next word as an integer.
    fn int(&mut self) -> Option<i64> {
        self.next()?.parse().ok()
    }

    /// The next word as a `double` stored as a `float` (VIS-2.2).
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec VIS-2.2: vis reads each coordinate as a double and stores it as a float"
    )]
    fn float(&mut self) -> Option<f32> {
        let value: f64 = self.next()?.parse().ok()?;
        Some(value as f32)
    }
}

impl PortalFile {
    /// The portal file `text` read (VIS-2).
    pub(crate) fn read(text: &str) -> Result<Self, VisError> {
        // `fscanf`'s `(%lf %lf %lf )` reads the parentheses as text; apart they are words.
        let spaced = text.replace('(', " ( ").replace(')', " ) ");
        let mut words = Words {
            rest: spaced.split_whitespace(),
        };
        let magic = words.next().ok_or(VisError::Header)?;
        let format = match magic {
            PORTAL_FILE => Format::Leaves,
            PORTAL_FILE_2 => Format::Clusters,
            _ => {
                // id's vis reads the two numbers before it looks at the word (VIS-2.8).
                return Err(match (words.int(), words.int()) {
                    (Some(_), Some(_)) => VisError::NotPortalFile,
                    _ => VisError::Header,
                });
            }
        };
        let count = |n: Option<i64>| {
            n.and_then(|n| usize::try_from(n).ok())
                .ok_or(VisError::Header)
        };
        let real = match format {
            Format::Leaves => None,
            Format::Clusters => Some(count(words.int())?),
        };
        let leafs = count(words.int())?;
        let portals = count(words.int())?;
        let mut file = Self {
            format,
            leafs,
            portals: Vec::with_capacity(portals.saturating_mul(2)),
            leaf_portals: vec![Vec::new(); leafs],
            clusters: (0..leafs).collect(),
        };
        let count = portals;
        for number in 0..count {
            let bad = VisError::Portal { number };
            let (Some(points), Some(a), Some(b)) = (words.int(), words.int(), words.int()) else {
                return Err(bad);
            };
            let points = usize::try_from(points).map_err(|_| VisError::Portal { number })?;
            if points > VIS_MAX_POINTS_ON_WINDING {
                return Err(VisError::TooManyPoints { number });
            }
            // A leaf number past the leaves stops the run; one equal to the count, past the
            // end in C, too (VIS-2.2, VIS-2.6).
            let leaf = |n: i64| usize::try_from(n).ok().filter(|&n| n < leafs);
            let (Some(front), Some(back)) = (leaf(a), leaf(b)) else {
                return Err(VisError::Portal { number });
            };
            let mut winding = Vec::with_capacity(points);
            for _ in 0..points {
                let open = words.next();
                let (Some(x), Some(y), Some(z)) = (words.float(), words.float(), words.float())
                else {
                    return Err(VisError::Portal { number });
                };
                if open != Some("(") || words.next() != Some(")") {
                    return Err(VisError::Portal { number });
                }
                winding.push([x, y, z]);
            }
            let winding = Winding::new(winding);
            let plane = winding.plane();
            file.add(front, back, &winding, plane.flipped())?;
            file.add(back, front, &winding, plane)?;
        }
        if let Some(real) = real {
            file.clusters = Self::cluster_map(&mut words, leafs, real)?;
        }
        Ok(file)
    }

    /// `LoadPrtFile`'s cluster map: for each cluster in turn, the numbers of its leaves ended by
    /// a negative one; a leaf in no cluster is in cluster 0 (VIS-7.2).
    fn cluster_map(
        words: &mut Words<'_>,
        clusters: usize,
        real: usize,
    ) -> Result<Vec<usize>, VisError> {
        let mut map = vec![0; real];
        for cluster in 0..clusters {
            loop {
                let leaf = words.int().ok_or(VisError::ClusterMap { cluster })?;
                let Ok(leaf) = usize::try_from(leaf) else {
                    break;
                };
                let slot = map.get_mut(leaf).ok_or(VisError::ClusterLeaf { leaf })?;
                *slot = cluster;
            }
        }
        Ok(map)
    }

    /// Adds the one-way portal in `leaf` through `winding` on `plane`, looking into `into`.
    fn add(
        &mut self,
        leaf: usize,
        into: usize,
        winding: &Winding,
        plane: Plane,
    ) -> Result<(), VisError> {
        let place = self.portals.len();
        let list = self
            .leaf_portals
            .get_mut(leaf)
            .ok_or(VisError::Portal { number: place })?;
        // ericw-tools keeps a cluster's portals in a vector, with no limit (VIS-7.1).
        if self.format == Format::Leaves && list.len() >= VIS_MAX_PORTALS_ON_LEAF {
            return Err(VisError::TooManyLeafPortals);
        }
        list.push(place);
        self.portals.push(Portal {
            winding: winding.clone(),
            plane,
            leaf: into,
        });
        Ok(())
    }
}
