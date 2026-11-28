// SPDX-License-Identifier: GPL-3.0-or-later

//! The ground as a graph a bot routes along (L-87, BOT-2): every place a player reaches from a
//! set of starts with the moves of [`Ground`], each place a cell of the search whatever the
//! player has opened, and each move between two places, weighted by how far it goes.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use super::{CLIMB, Cell, Ground, PACE, Search, cell};

/// How wide a square of the index that finds the place nearest a point is, in map units.
const BUCKET: f32 = 64.0;

/// A place of a [`Graph`], by its index in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Place(u32);

impl Place {
    /// The place's index in its graph's places.
    fn index(self) -> usize {
        usize::try_from(self.0).unwrap_or(usize::MAX)
    }
}

/// A way along a [`Graph`]: the places it passes, from where it starts to where it ends, and
/// how far it goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    points: Vec<[f32; 3]>,
    length: f32,
}

impl Route {
    /// The points it passes, each a player's origin, the first where it starts.
    #[must_use]
    pub fn points(&self) -> &[[f32; 3]] {
        &self.points
    }

    /// How far it goes, the sum of its moves' lengths, in map units.
    #[must_use]
    pub const fn length(&self) -> f32 {
        self.length
    }
}

/// The places a player reaches and the moves between them (BOT-2.1).
#[derive(Clone, Debug, Default)]
pub struct Graph {
    /// Each place's point, a player's origin there.
    places: Vec<[f32; 3]>,
    /// The moves from each place: where to, and how far.
    moves: Vec<Vec<(u32, f32)>>,
    /// The places by the square of [`BUCKET`] their point is over.
    buckets: HashMap<[i32; 2], Vec<u32>>,
}

/// The square of the index `at` is over.
fn bucket(at: [f32; 3]) -> [i32; 2] {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a map's coordinates are far inside i32's range"
    )]
    let square = |c: f32| (c / BUCKET).floor() as i32;
    [square(at[0]), square(at[1])]
}

/// How far apart `a` and `b` are.
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let [dx, dy, dz] = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

impl Graph {
    /// How many places it has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.places.len()
    }

    /// Whether it has no places.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.places.is_empty()
    }

    /// The place of index `n`, from 0, `None` past the last.
    #[must_use]
    pub fn place(&self, n: usize) -> Option<Place> {
        if n < self.places.len() {
            u32::try_from(n).ok().map(Place)
        } else {
            None
        }
    }

    /// The point of `place`.
    #[must_use]
    pub fn point(&self, place: Place) -> Option<[f32; 3]> {
        self.places.get(place.index()).copied()
    }

    /// The place nearest `at` (BOT-2.2): among those over the squares around it, else among
    /// all; a height apart counts twice, so a floor above or below is farther than one beside.
    #[must_use]
    pub fn nearest(&self, at: [f32; 3]) -> Option<Place> {
        let weighed = |n: u32| {
            let point = self.places.get(usize::try_from(n).ok()?)?;
            let [dx, dy, dz] = [point[0] - at[0], point[1] - at[1], (point[2] - at[2]) * 2.0];
            Some((dx * dx + dy * dy + dz * dz, n))
        };
        let [x, y] = bucket(at);
        let mut best: Option<(f32, u32)> = None;
        for bx in x.saturating_sub(1)..=x.saturating_add(1) {
            for by in y.saturating_sub(1)..=y.saturating_add(1) {
                for &n in self.buckets.get(&[bx, by]).into_iter().flatten() {
                    best = nearer(best, weighed(n));
                }
            }
        }
        if best.is_none() {
            for n in 0..u32::try_from(self.places.len()).unwrap_or(u32::MAX) {
                best = nearer(best, weighed(n));
            }
        }
        best.map(|(_, n)| Place(n))
    }

    /// The shortest route from `from` to each of `to` (BOT-2.3), `None` for one not reached:
    /// one search, which ends once every place of `to` is reached.
    #[must_use]
    pub fn routes(&self, from: Place, to: &[Place]) -> Vec<Option<Route>> {
        let count = self.places.len();
        let mut best = vec![f32::INFINITY; count];
        let mut came: Vec<Option<u32>> = vec![None; count];
        let mut left: Vec<Place> = to.to_vec();
        let mut heap = BinaryHeap::new();
        if let Some(first) = best.get_mut(from.index()) {
            *first = 0.0;
            heap.push(Reverse((0.0_f32.to_bits(), from.0)));
        }
        while let Some(Reverse((bits, n))) = heap.pop() {
            let so_far = f32::from_bits(bits);
            let here = usize::try_from(n).unwrap_or(usize::MAX);
            if best.get(here).is_none_or(|&b| so_far > b) {
                continue;
            }
            left.retain(|place| place.0 != n);
            if left.is_empty() {
                break;
            }
            for &(next, length) in self.moves.get(here).into_iter().flatten() {
                let further = so_far + length;
                let there = usize::try_from(next).unwrap_or(usize::MAX);
                if let Some(slot) = best.get_mut(there)
                    && further < *slot
                {
                    *slot = further;
                    if let Some(back) = came.get_mut(there) {
                        *back = Some(n);
                    }
                    // A non-negative float's bits order as it does.
                    heap.push(Reverse((further.to_bits(), next)));
                }
            }
        }
        to.iter()
            .map(|&place| {
                let length = *best.get(place.index())?;
                if !length.is_finite() {
                    return None;
                }
                let mut points = Vec::new();
                let mut at = Some(place.0);
                while let Some(n) = at {
                    points.push(*self.places.get(usize::try_from(n).ok()?)?);
                    at = if n == from.0 {
                        None
                    } else {
                        *came.get(usize::try_from(n).ok()?)?
                    };
                }
                points.reverse();
                Some(Route { points, length })
            })
            .collect()
    }

    /// The shortest route from `from` to `to` (BOT-2.3).
    #[must_use]
    pub fn route(&self, from: Place, to: Place) -> Option<Route> {
        self.routes(from, &[to]).into_iter().next().flatten()
    }

    /// The place of `at`'s cell, added when it has none.
    fn place_of(&mut self, cells: &mut HashMap<Cell, u32>, at: [f32; 3]) -> Option<u32> {
        if let Some(&n) = cells.get(&cell(at)) {
            return Some(n);
        }
        let n = u32::try_from(self.places.len()).ok()?;
        cells.insert(cell(at), n);
        self.places.push(at);
        self.moves.push(Vec::new());
        self.buckets.entry(bucket(at)).or_default().push(n);
        Some(n)
    }
}

/// The nearer of two weighed places.
fn nearer(best: Option<(f32, u32)>, next: Option<(f32, u32)>) -> Option<(f32, u32)> {
    match (best, next) {
        (Some(b), Some(n)) if n.0 < b.0 => Some(n),
        (None, next) => next,
        (best, _) => best,
    }
}

impl Ground<'_> {
    /// Where a player spawns: each `info_player_start`, `info_player_coop` and
    /// `info_player_deathmatch` the game keeps, in the lump's order.
    #[must_use]
    pub fn spawns(&self) -> &[[f32; 3]] {
        &self.spawns
    }

    /// The graph of every place reached from each of `starts` (BOT-2.1), a place a cell
    /// whatever has been opened there, and a move from one to another wherever a player makes
    /// it in some state; `None` when a search passes [`super::MOST_PLACES`].
    #[must_use]
    pub fn graph(&self, starts: &[[f32; 3]]) -> Option<Graph> {
        let mut graph = Graph::default();
        let mut cells: HashMap<Cell, u32> = HashMap::new();
        for &start in starts {
            // A start a search already reached adds nothing: what it reaches was searched. The
            // places are a pace apart from the first start, so one within a pace across and a
            // step up or down of where another lands is where it stands.
            let landed = self.fall(super::Held::default(), start).at;
            let reached = graph.nearest(landed).and_then(|place| graph.point(place));
            if reached.is_some_and(|[x, y, z]| {
                (x - landed[0]).abs() <= PACE
                    && (y - landed[1]).abs() <= PACE
                    && (z - landed[2]).abs() <= CLIMB
            }) {
                continue;
            }
            let explored = self.explore(start, Search::Covering)?;
            let mut ids = Vec::with_capacity(explored.nodes.len());
            for &(_, at) in &explored.nodes {
                ids.push(graph.place_of(&mut cells, at)?);
            }
            for &(to, from) in &explored.moves {
                let (Some(&to), Some(&from)) = (
                    ids.get(usize::try_from(to).ok()?),
                    ids.get(usize::try_from(from).ok()?),
                ) else {
                    continue;
                };
                if to == from {
                    continue;
                }
                let (Some(a), Some(b)) = (
                    graph.places.get(usize::try_from(from).ok()?),
                    graph.places.get(usize::try_from(to).ok()?),
                ) else {
                    continue;
                };
                let length = distance(*a, *b);
                let Some(out) = graph.moves.get_mut(usize::try_from(from).ok()?) else {
                    continue;
                };
                if !out.iter().any(|&(next, _)| next == to) {
                    out.push((to, length));
                }
            }
        }
        Some(graph)
    }

    /// The shortest way a player at `from` goes to `to` (BOT-2.3): the route along the graph
    /// reached from `from` to its place nearest `to`; `None` when none leads there.
    #[must_use]
    pub fn path(&self, from: [f32; 3], to: [f32; 3]) -> Option<Route> {
        let graph = self.graph(&[from])?;
        let start = graph.nearest(from)?;
        let end = graph.nearest(to)?;
        graph.route(start, end)
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use super::{Graph, Place, bucket, distance, nearer};

    /// A graph of `points` and one way `moves`, each its length.
    fn graph(points: &[[f32; 3]], moves: &[(u32, u32)]) -> Graph {
        let mut graph = Graph::default();
        for (n, &at) in points.iter().enumerate() {
            graph.places.push(at);
            graph.moves.push(Vec::new());
            graph
                .buckets
                .entry(bucket(at))
                .or_default()
                .push(u32::try_from(n).unwrap());
        }
        for &(from, to) in moves {
            let length = distance(
                graph.places[usize::try_from(from).unwrap()],
                graph.places[usize::try_from(to).unwrap()],
            );
            graph.moves[usize::try_from(from).unwrap()].push((to, length));
        }
        graph
    }

    // spec BOT-2.1: a move's length is the straight distance; a point's square is 64 across
    #[test]
    fn lengths_and_squares() {
        assert_eq!(distance([1.0, 2.0, 3.0], [4.0, 6.0, 15.0]), 13.0);
        assert_eq!(bucket([0.0, 63.9, 0.0]), [0, 0]);
        assert_eq!(bucket([64.0, -0.1, 0.0]), [1, -1]);
        assert_eq!(bucket([-130.0, 200.0, 0.0]), [-3, 3]);
    }

    // spec BOT-2.2: the nearest place among the squares around a point, a height counting
    // twice; else the nearest of all; the first of two as near
    #[test]
    fn the_nearest_place() {
        let g = graph(
            &[
                [0.0, 0.0, 0.0],
                [40.0, 0.0, 0.0],
                [0.0, 0.0, 30.0],
                [1000.0, 0.0, 0.0],
            ],
            &[],
        );
        assert_eq!(g.nearest([25.0, 0.0, 0.0]), Some(Place(1)));
        // 30 away along the floor beats 20 up, which counts as 40.
        let floors = graph(&[[30.0, 0.0, 0.0], [0.0, 0.0, 20.0]], &[]);
        assert_eq!(floors.nearest([0.0, 0.0, 0.0]), Some(Place(0)));
        assert_eq!(g.nearest([0.0, 0.0, 26.0]), Some(Place(2)));
        assert_eq!(
            g.nearest([900.0, 0.0, 0.0]),
            Some(Place(3)),
            "the square around it"
        );
        assert_eq!(
            g.nearest([600.0, 0.0, 0.0]),
            Some(Place(3)),
            "no square near: of all"
        );
        assert_eq!(
            g.nearest([20.0, 0.0, 0.0]),
            Some(Place(0)),
            "as near: the first"
        );
        assert_eq!(nearer(Some((1.0, 0)), Some((1.0, 1))), Some((1.0, 0)));
        assert_eq!(nearer(None, Some((1.0, 1))), Some((1.0, 1)));
        assert_eq!(nearer(Some((1.0, 0)), None), Some((1.0, 0)));
        assert_eq!(Graph::default().nearest([0.0; 3]), None);
        assert!(Graph::default().is_empty());
        assert!(!g.is_empty());
        assert_eq!(g.len(), 4);
        assert_eq!(g.place(3), Some(Place(3)));
        assert_eq!(g.place(4), None);
        assert_eq!(g.point(Place(1)), Some([40.0, 0.0, 0.0]));
    }

    // spec BOT-2.3: the shortest route by length, not by moves; none against a move's way; one
    // search answers every place sought; of two as short, the one found first
    #[test]
    fn the_shortest_routes() {
        // 0 -> 1 -> 2 is 20; 0 -> 2 straight is 30 but goes up a long way round (3).
        let g = graph(
            &[
                [0.0, 0.0, 0.0],
                [10.0, 0.0, 0.0],
                [20.0, 0.0, 0.0],
                [10.0, 0.0, 100.0],
            ],
            &[(0, 1), (1, 2), (0, 3), (3, 2)],
        );
        let route = g.route(Place(0), Place(2)).unwrap();
        assert_eq!(
            route.points(),
            [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [20.0, 0.0, 0.0]]
        );
        assert_eq!(route.length(), 20.0);
        assert!(
            g.route(Place(1), Place(3)).is_none(),
            "nothing leads to 3 but from 0"
        );
        assert!(g.route(Place(2), Place(0)).is_none(), "one way");
        let both = g.routes(Place(0), &[Place(3), Place(1)]);
        assert_eq!(
            both[0].as_ref().unwrap().length(),
            distance([0.0; 3], [10.0, 0.0, 100.0])
        );
        assert_eq!(both[1].as_ref().unwrap().length(), 10.0);
        let tie = graph(
            &[
                [0.0, 0.0, 0.0],
                [10.0, 10.0, 0.0],
                [10.0, -10.0, 0.0],
                [20.0, 0.0, 0.0],
            ],
            &[(0, 1), (0, 2), (1, 3), (2, 3)],
        );
        let first = tie.route(Place(0), Place(3)).unwrap();
        assert_eq!(
            first.points()[1],
            [10.0, 10.0, 0.0],
            "through the first found"
        );
    }
}
