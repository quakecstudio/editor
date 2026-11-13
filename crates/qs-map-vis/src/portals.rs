// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading the portal file (VIS-2): each portal of the file as two one-way portals, one in each
//! of its leaves looking into the other.

use crate::VisError;
use crate::winding::{Plane, Winding};

/// The portal file's first word (VIS-2.1).
const PORTAL_FILE: &str = "PRT1";
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

/// The portal file read: the vis leaves' count, the one-way portals in file order (each file
/// portal's forward one, then its backward one) and each leaf's portals by place.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PortalFile {
    pub(crate) leafs: usize,
    pub(crate) portals: Vec<Portal>,
    pub(crate) leaf_portals: Vec<Vec<usize>>,
}

/// The words of the text after `PRT1`, as `fscanf` reads them, the parentheses apart.
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
        let (Some(leafs), Some(count)) = (words.int(), words.int()) else {
            return Err(VisError::Header);
        };
        if magic != PORTAL_FILE {
            return Err(VisError::NotPortalFile);
        }
        let leafs = usize::try_from(leafs).map_err(|_| VisError::Header)?;
        let count = usize::try_from(count).map_err(|_| VisError::Header)?;
        let mut file = Self {
            leafs,
            portals: Vec::with_capacity(count.saturating_mul(2)),
            leaf_portals: vec![Vec::new(); leafs],
        };
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
        Ok(file)
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
        if list.len() >= VIS_MAX_PORTALS_ON_LEAF {
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
