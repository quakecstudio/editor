// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap, index or overflow is a failed test"
)]

use qs_bsp::{
    BSP_HEADER_LUMPS, BSP_MAX_MAP_HULLS, BSP_NUM_AMBIENTS, Bsp, BspVersion, ClipNode, Leaf, Lump,
    Model, Node, Plane,
};

use super::{
    Body, BoxHull, CONTENTS_EMPTY, CONTENTS_SOLID, CONTENTS_WATER, Collision, CollisionError,
    FL_ITEM, FL_MONSTER, Host, MoveKind, Solid, Trace, box_on_plane_side, clip_move_to_entity,
    hull_for_entity, link_edict, recursive_hull_check, sv_move, test_entity_position,
};

fn plane(normal: [f32; 3], dist: f32, plane_type: i32) -> Plane {
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

fn node(planenum: usize, children: [i32; 2]) -> Node {
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
struct MapSpec {
    /// The planes.
    planes: Vec<Plane>,
    /// The drawing nodes; a child `-1 - c` is leaf `c`.
    nodes: Vec<Node>,
    /// The leaves' contents; leaf 0 is solid.
    leafs: Vec<i32>,
    /// The clipnodes of hulls 1 and 2.
    clipnodes: Vec<ClipNode>,
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

fn bsp(spec: &MapSpec) -> Bsp<'static> {
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

fn body() -> Body {
    Body {
        free: false,
        solid: Solid::BBox,
        pusher: false,
        modelindex: 0.0,
        origin: [0.0; 3],
        mins: [-16.0; 3],
        maxs: [16.0; 3],
        absmin: [0.0; 3],
        absmax: [0.0; 3],
        size: [32.0; 3],
        flags: 0,
        owner: None,
        touch: 0,
    }
}

/// The world entity: the map's world model, solid and a pusher.
fn world() -> Body {
    Body {
        solid: Solid::Bsp,
        pusher: true,
        modelindex: 1.0,
        mins: [-1000.0; 3],
        maxs: [1000.0; 3],
        ..body()
    }
}

/// What a trigger's touch does to the area, beyond being recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Effect {
    /// Takes this entity out of its list.
    Unlink(usize),
    /// Puts this entity at the end of the trigger list of `node`.
    Append(usize, usize),
}

struct TestHost {
    collision: Collision,
    bodies: Vec<Body>,
    touched: Vec<(usize, usize)>,
    effects: Vec<(usize, Effect)>,
}

impl TestHost {
    fn new(spec: &MapSpec, bodies: Vec<Body>) -> Self {
        Self {
            collision: Collision::new(&bsp(spec)),
            bodies: [vec![world()], bodies].concat(),
            touched: Vec::new(),
            effects: Vec::new(),
        }
    }
}

impl Host for TestHost {
    fn collision(&self) -> &Collision {
        &self.collision
    }

    fn collision_mut(&mut self) -> &mut Collision {
        &mut self.collision
    }

    fn body(&self, n: usize) -> Option<Body> {
        self.bodies.get(n).copied()
    }

    fn set_abs(&mut self, n: usize, absmin: [f32; 3], absmax: [f32; 3]) {
        let body = &mut self.bodies[n];
        body.absmin = absmin;
        body.absmax = absmax;
    }

    fn touch(&mut self, trigger: usize, other: usize) -> Result<(), CollisionError> {
        self.touched.push((trigger, other));
        let effects: Vec<Effect> = self
            .effects
            .iter()
            .filter(|(t, _)| *t == trigger)
            .map(|(_, e)| *e)
            .collect();
        for effect in effects {
            match effect {
                Effect::Unlink(n) => self.collision.unlink(n),
                Effect::Append(n, node) => self.collision.area.insert_at(n, node, true),
            }
        }
        Ok(())
    }
}

fn link(host: &mut TestHost, n: usize) {
    link_edict(host, n, false).unwrap();
}

fn trace_world(spec: &MapSpec, hull: usize, from: [f32; 3], to: [f32; 3]) -> Trace {
    let collision = Collision::new(&bsp(spec));
    let model = collision.model(1.0).copied().unwrap();
    let hull = collision.sets()[0].hull(hull, model.heads);
    let mut trace = Trace::new(to);
    recursive_hull_check(&hull, hull.first, (from, to), &mut trace).unwrap();
    trace
}

// spec WORLD-1.1: hull 0 is the drawing nodes, a child leaf read as its contents
// spec WORLD-2.1: the walk takes child 1 below the plane and child 0 on or above it
#[test]
fn hull_0_is_the_drawing_nodes() {
    let collision = Collision::new(&bsp(&MapSpec::default()));
    let at = |p| collision.true_point_contents(p).unwrap();
    assert_eq!(at([0.0, 0.0, 5.0]), CONTENTS_EMPTY);
    assert_eq!(at([0.0, 0.0, 0.0]), CONTENTS_EMPTY);
    assert_eq!(at([0.0, 0.0, -0.5]), CONTENTS_SOLID);
}

// spec WORLD-1.2: hulls 1 and 2 are the clipnodes from their own head nodes, with the
// player's and the large box's sizes
#[test]
fn hulls_1_and_2_are_the_clipnodes() {
    let collision = Collision::new(&bsp(&MapSpec::default()));
    let heads = collision.model(1.0).unwrap().heads;
    let set = &collision.sets()[0];
    let hull1 = set.hull(1, heads);
    let at = |p| hull1.point_contents(hull1.first, p).unwrap();
    assert_eq!(at([0.0, 0.0, 23.0]), CONTENTS_SOLID);
    assert_eq!(at([0.0, 0.0, 24.0]), CONTENTS_EMPTY);
    assert_eq!(hull1.clip_mins, [-16.0, -16.0, -24.0]);
    assert_eq!(set.hull(2, heads).clip_mins, [-32.0, -32.0, -24.0]);
    assert_eq!(set.hull(0, heads).clip_mins, [0.0; 3]);
    assert_eq!((hull1.first, hull1.last), (0, 0));
}

// spec WORLD-1.3: a box is six clipnodes, solid only inside all six planes
#[test]
fn a_box_is_a_hull_of_six_clipnodes() {
    let box_hull = BoxHull::new([-1.0, -2.0, -3.0], [1.0, 2.0, 3.0]);
    let hull = box_hull.hull();
    let at = |p| hull.point_contents(0, p).unwrap();
    assert_eq!(at([0.0, 0.0, 0.0]), CONTENTS_SOLID);
    assert_eq!(at([0.5, 1.5, -2.5]), CONTENTS_SOLID);
    // On a maximum face the distance is 0, which is in front: empty.
    assert_eq!(at([1.0, 0.0, 0.0]), CONTENTS_EMPTY);
    // On a minimum face too the distance is 0, which there is the inside.
    assert_eq!(at([0.0, -2.0, 0.0]), CONTENTS_SOLID);
    assert_eq!(at([0.0, -2.5, 0.0]), CONTENTS_EMPTY);
    for p in [
        [2.0, 0.0, 0.0],
        [-2.0, 0.0, 0.0],
        [0.0, 3.0, 0.0],
        [0.0, 0.0, 4.0],
        [0.0, 0.0, -4.0],
    ] {
        assert_eq!(at(p), CONTENTS_EMPTY, "{p:?}");
    }
    assert_eq!((hull.first, hull.last), (0, 5));
}

// spec WORLD-1.4: a brush entity's hull is chosen by the box's width, its offset is the
// hull's box less the moving box plus the origin; other entities are a box grown by it
#[test]
fn hull_for_entity_picks_by_width() {
    let collision = Collision::new(&bsp(&MapSpec::default()));
    let model = collision.model(1.0);
    let sets = collision.sets();
    let mut ent = world();
    ent.origin = [5.0, 6.0, 7.0];
    let pick = |mins: [f32; 3], maxs: [f32; 3]| {
        let (hull, offset) = hull_for_entity(&ent, model, sets, (mins, maxs)).unwrap();
        (hull.hull().clip_mins, offset)
    };
    assert_eq!(pick([0.0; 3], [0.0; 3]), ([0.0; 3], [5.0, 6.0, 7.0]));
    assert_eq!(pick([-1.4; 3], [1.4; 3]).0, [0.0; 3]);
    assert_eq!(pick([-1.5; 3], [1.5; 3]).0, [-16.0, -16.0, -24.0]);
    assert_eq!(
        pick([-16.0, -16.0, -24.0], [16.0, 16.0, 32.0]),
        ([-16.0, -16.0, -24.0], [5.0, 6.0, 7.0])
    );
    assert_eq!(
        pick([-16.0, -16.0, 0.0], [16.0, 16.0, 0.0]).0,
        [-16.0, -16.0, -24.0]
    );
    assert_eq!(
        pick([-16.0, -16.0, 0.0], [16.1, 16.0, 0.0]).0,
        [-32.0, -32.0, -24.0]
    );
    // offset = clip_mins - mins + origin
    assert_eq!(pick([-10.0; 3], [10.0; 3]).1, [-1.0, 0.0, -7.0]);

    let boxed = Body {
        origin: [1.0, 2.0, 3.0],
        ..body()
    };
    let (hull, offset) = hull_for_entity(&boxed, None, sets, ([-4.0; 3], [4.0; 3])).unwrap();
    assert_eq!(offset, [1.0, 2.0, 3.0]);
    let hull = hull.hull();
    // The box is -16 - 4 to 16 + 4.
    assert_eq!(
        hull.point_contents(0, [19.9, 0.0, 0.0]).unwrap(),
        CONTENTS_SOLID
    );
    assert_eq!(
        hull.point_contents(0, [20.0, 0.0, 0.0]).unwrap(),
        CONTENTS_EMPTY
    );
    assert_eq!(
        hull.point_contents(0, [-19.9, 0.0, 0.0]).unwrap(),
        CONTENTS_SOLID
    );
    assert_eq!(
        hull.point_contents(0, [-20.1, 0.0, 0.0]).unwrap(),
        CONTENTS_EMPTY
    );
}

// spec WORLD-1.4: a SOLID_BSP entity must be a pusher with a brush model
#[test]
fn solid_bsp_needs_a_pusher_and_a_brush_model() {
    let collision = Collision::new(&bsp(&MapSpec::default()));
    let sets = collision.sets();
    let not_pushing = Body {
        pusher: false,
        ..world()
    };
    assert!(matches!(
        hull_for_entity(
            &not_pushing,
            collision.model(1.0),
            sets,
            ([0.0; 3], [0.0; 3])
        ),
        Err(CollisionError::SolidBspWithoutPush)
    ));
    assert!(matches!(
        hull_for_entity(&world(), None, sets, ([0.0; 3], [0.0; 3])),
        Err(CollisionError::PushWithoutBsp)
    ));
    assert_eq!(
        CollisionError::SolidBspWithoutPush.to_string(),
        "SOLID_BSP without MOVETYPE_PUSH"
    );
    assert_eq!(
        CollisionError::PushWithoutBsp.to_string(),
        "MOVETYPE_PUSH with a non bsp model"
    );
    // Model index 0 and indices past the list are no brush model.
    assert!(collision.model(0.0).is_none());
    assert!(collision.model(9.0).is_none());
    assert!(collision.model(-1.0).is_none());
    assert!(collision.model(1.9).is_some());
}

// spec WORLD-2.1: a walk outside the hull's clipnodes is id's Sys_Error
#[test]
fn a_bad_node_is_an_error() {
    let collision = Collision::new(&bsp(&MapSpec::default()));
    let hull = collision.sets()[0].hull(0, [0; 3]);
    let error = hull.point_contents(1, [0.0; 3]).unwrap_err();
    assert_eq!(error.to_string(), "SV_HullPointContents: bad node number");
    let below_first = collision.sets()[0].hull(0, [1, 0, 0]);
    assert!(below_first.point_contents(0, [0.0; 3]).is_err());
    // A cycle ends instead of walking for ever.
    let looped = MapSpec {
        nodes: vec![node(0, [0, 0])],
        ..MapSpec::default()
    };
    let collision = Collision::new(&bsp(&looped));
    assert!(collision.true_point_contents([0.0; 3]).is_err());
}

// spec WORLD-2.1: a general plane uses the dot product, and a negative type is general too
#[test]
fn general_planes_use_the_dot_product() {
    for kind in [3, -1] {
        let spec = MapSpec {
            planes: vec![plane([0.6, 0.8, 0.0], 10.0, kind)],
            ..MapSpec::default()
        };
        let collision = Collision::new(&bsp(&spec));
        let at = |p| collision.true_point_contents(p).unwrap();
        assert_eq!(at([10.0, 5.0, 0.0]), CONTENTS_EMPTY);
        assert_eq!(at([10.0, 4.0, 0.0]), CONTENTS_SOLID);
        assert_eq!(at([0.0, 12.6, 0.0]), CONTENTS_EMPTY);
    }
}

// spec WORLD-2.2: SV_PointContents reads a current as water; SV_TruePointContents keeps it
#[test]
fn currents_read_as_water() {
    let contents = |value: i32| {
        let spec = MapSpec {
            leafs: vec![CONTENTS_SOLID, value],
            ..MapSpec::default()
        };
        let collision = Collision::new(&bsp(&spec));
        let p = [0.0, 0.0, 1.0];
        (
            collision.point_contents(p).unwrap(),
            collision.true_point_contents(p).unwrap(),
        )
    };
    for current in [-9, -12, -14] {
        assert_eq!(contents(current), (CONTENTS_WATER, current));
    }
    assert_eq!(contents(-8), (-8, -8));
    assert_eq!(contents(-15), (-15, -15));
    assert_eq!(contents(-4), (-4, -4));
}

// spec WORLD-3.1: a line in open space clears allsolid and sets inopen; in liquid, inwater
// spec WORLD-3.3: a line wholly on one side of a plane goes on in that child only
#[test]
fn a_line_in_open_space_or_liquid() {
    let open = trace_world(&MapSpec::default(), 0, [0.0, 0.0, 10.0], [5.0, 0.0, 20.0]);
    assert!(!open.allsolid && open.inopen && !open.inwater && !open.startsolid);
    assert_eq!(open.fraction, 1.0);
    let water = MapSpec {
        leafs: vec![CONTENTS_SOLID, CONTENTS_WATER],
        ..MapSpec::default()
    };
    let wet = trace_world(&water, 0, [0.0, 0.0, 10.0], [0.0, 0.0, 20.0]);
    assert!(!wet.allsolid && wet.inwater && !wet.inopen);
    let solid = trace_world(&MapSpec::default(), 0, [0.0, 0.0, -10.0], [0.0, 0.0, -20.0]);
    assert!(solid.allsolid && solid.startsolid && !solid.inopen);
    assert_eq!(solid.fraction, 1.0);
    // One end on the plane: both distances 0 or more, so the front only.
    let touching = trace_world(&MapSpec::default(), 0, [0.0, 0.0, 0.0], [0.0, 0.0, 10.0]);
    assert!(touching.inopen && !touching.startsolid);
}

// spec WORLD-3.4: the crossing is DIST_EPSILON on the near side
// spec WORLD-3.6: the impact plane faces the side the move came from
#[test]
fn a_line_stops_an_epsilon_short_of_the_plane() {
    // t1 = 1.03125 and t2 = -0.96875: frac = (t1 - 1/32) / 2 = 0.5.
    let down = trace_world(
        &MapSpec::default(),
        0,
        [0.0, 0.0, 1.03125],
        [0.0, 0.0, -0.96875],
    );
    assert_eq!(down.fraction, 0.5);
    assert_eq!(down.endpos, [0.0, 0.0, 0.03125]);
    assert_eq!(down.plane.normal, [0.0, 0.0, 1.0]);
    assert_eq!(down.plane.dist, 0.0);
    assert!(!down.allsolid && !down.startsolid);
    // Only part of the move: midf is p1f + (p2f - p1f) * frac of the whole.
    let half = trace_world(
        &MapSpec::default(),
        0,
        [0.0, 0.0, 3.03125],
        [0.0, 0.0, -0.96875],
    );
    assert_eq!(half.fraction, 0.75);
    // A move up from solid into the open goes through: it started solid, it is not all solid.
    let up = trace_world(&MapSpec::default(), 0, [0.0, 0.0, -10.0], [0.0, 0.0, 10.0]);
    assert!(up.startsolid && !up.allsolid && up.inopen);
    assert_eq!(up.fraction, 1.0);
    // Hull 1's floor is at 24.
    let player = trace_world(
        &MapSpec::default(),
        1,
        [0.0, 0.0, 25.03125],
        [0.0, 0.0, 23.03125],
    );
    assert_eq!(player.fraction, 0.5);
    assert_eq!(player.endpos[2], 24.03125);
}

// spec WORLD-3.6: hitting the underside of a ceiling gives the plane's negation
#[test]
fn the_impact_plane_is_negated_from_behind() {
    // Solid above z = 0, open below.
    let ceiling = MapSpec {
        nodes: vec![node(0, [-1, -2])],
        ..MapSpec::default()
    };
    let up = trace_world(&ceiling, 0, [0.0, 0.0, -1.03125], [0.0, 0.0, 0.96875]);
    assert_eq!(up.fraction, 0.5);
    assert_eq!(up.endpos, [0.0, 0.0, -0.03125]);
    assert_eq!(up.plane.normal, [0.0, 0.0, -1.0]);
    assert_eq!(up.plane.dist, 0.0);
    let raised = MapSpec {
        planes: vec![plane([0.0, 0.0, 1.0], 8.0, 2)],
        ..ceiling
    };
    let up = trace_world(&raised, 0, [0.0, 0.0, 6.96875], [0.0, 0.0, 8.96875]);
    assert_eq!(up.plane.dist, -8.0);
}

// spec WORLD-3.4: frac is kept within 0 to 1, so a start closer than the epsilon stays put
#[test]
fn a_crossing_closer_than_the_epsilon_stays_at_the_start() {
    let close = trace_world(&MapSpec::default(), 0, [0.0, 0.0, 0.01], [0.0, 0.0, -1.0]);
    assert_eq!(close.fraction, 0.0);
    assert_eq!(close.endpos, [0.0, 0.0, 0.01]);
}

// spec WORLD-3.5: the line goes up to the node and past it when the far side is open there
// spec WORLD-3.6: when the far side is solid and the move was all solid, there is no impact
#[test]
fn a_line_passes_planes_with_open_space_beyond() {
    // Open above z = 10, open between 0 and 10, solid below 0.
    let spec = MapSpec {
        planes: vec![
            plane([0.0, 0.0, 1.0], 10.0, 2),
            plane([0.0, 0.0, 1.0], 0.0, 2),
        ],
        nodes: vec![node(0, [-2, 1]), node(1, [-2, -1])],
        leafs: vec![CONTENTS_SOLID, CONTENTS_EMPTY],
        ..MapSpec::default()
    };
    let through = trace_world(&spec, 0, [0.0, 0.0, 20.0], [0.0, 0.0, 5.0]);
    assert_eq!(through.fraction, 1.0);
    assert!(through.inopen && !through.allsolid);
    let to_floor = trace_world(&spec, 0, [0.0, 0.0, 20.0], [0.0, 0.0, -20.0]);
    assert!(to_floor.fraction < 1.0);
    assert!((to_floor.endpos[2] - 0.031_25).abs() < 1e-4);
    let in_solid = MapSpec {
        nodes: vec![node(0, [-1, -1])],
        ..spec
    };
    let stuck = trace_world(&in_solid, 0, [0.0, 0.0, 20.0], [0.0, 0.0, -20.0]);
    assert!(stuck.allsolid && stuck.startsolid);
    assert_eq!(stuck.fraction, 1.0);
}

// spec WORLD-3.2: a check through a clipnode outside the hull is id's Sys_Error
#[test]
fn a_check_through_a_bad_node_fails() {
    let spec = MapSpec {
        nodes: vec![node(0, [5, -1])],
        ..MapSpec::default()
    };
    let collision = Collision::new(&bsp(&spec));
    let hull = collision.sets()[0].hull(0, [0; 3]);
    let mut trace = Trace::new([0.0; 3]);
    let line = ([0.0, 0.0, 5.0], [0.0, 0.0, 6.0]);
    let error = recursive_hull_check(&hull, 0, line, &mut trace).unwrap_err();
    assert_eq!(error.to_string(), "SV_RecursiveHullCheck: bad node number");
    let looped = MapSpec {
        nodes: vec![node(0, [0, 0])],
        ..MapSpec::default()
    };
    let collision = Collision::new(&bsp(&looped));
    let hull = collision.sets()[0].hull(0, [0; 3]);
    assert!(recursive_hull_check(&hull, 0, line, &mut trace).is_err());
}

// spec WORLD-4.1: the move is clipped in the hull's frame and its end moved back; the entity
// is named only when the move was clipped or started in solid
#[test]
fn a_move_clipped_to_a_box_entity() {
    let collision = Collision::new(&bsp(&MapSpec::default()));
    let ent = Body {
        origin: [100.0, 0.0, 0.0],
        ..body()
    };
    let clip = |start, end| {
        clip_move_to_entity(
            (3, &ent),
            None,
            collision.sets(),
            (start, [0.0; 3], [0.0; 3], end),
        )
        .unwrap()
    };
    let hit = clip([0.0; 3], [200.0, 0.0, 0.0]);
    assert_eq!(hit.ent, Some(3));
    assert!((hit.endpos[0] - 83.968_75).abs() < 1e-3, "{:?}", hit.endpos);
    assert!(
        (hit.fraction - 0.419_843_75).abs() < 1e-5,
        "{}",
        hit.fraction
    );
    assert_eq!(hit.plane.normal, [-1.0, 0.0, 0.0]);
    let miss = clip([0.0, 50.0, 0.0], [200.0, 50.0, 0.0]);
    assert_eq!(miss.ent, None);
    assert_eq!(miss.fraction, 1.0);
    assert_eq!(miss.endpos, [200.0, 50.0, 0.0]);
    let inside = clip([100.0, 0.0, 0.0], [100.0, 1.0, 0.0]);
    assert!(inside.startsolid);
    assert_eq!(inside.ent, Some(3));
    assert_eq!(inside.endpos, [100.0, 1.0, 0.0]);
    // A brush entity moved by its origin.
    let mut lift = world();
    lift.origin = [0.0, 0.0, 100.0];
    let on_lift = clip_move_to_entity(
        (4, &lift),
        collision.model(1.0),
        collision.sets(),
        (
            [0.0, 0.0, 101.03125],
            [0.0; 3],
            [0.0; 3],
            [0.0, 0.0, 99.03125],
        ),
    )
    .unwrap();
    assert_eq!(on_lift.fraction, 0.5);
    assert_eq!(on_lift.endpos, [0.0, 0.0, 100.03125]);
}

// spec WORLD-5.1: 31 nodes, numbered before their children, split on the longer of x and y
#[test]
fn area_nodes_split_the_world_in_halves() {
    let collision = Collision::new(&bsp(&MapSpec::default()));
    let area = collision.area();
    assert_eq!(area.len(), 31);
    // The world is 2000 by 1500: the first split is on x at 0.
    assert_eq!(area.split(0), Some((0, 0.0, [1, 16])));
    // Child 0 is x 0 to 1000 by 1500: split on y at -250.
    assert_eq!(area.split(1), Some((1, -250.0, [2, 9])));
    // Child 1 is x -1000 to 0: also on y.
    assert_eq!(area.split(16), Some((1, -250.0, [17, 24])));
    // Node 2 is x 0 to 1000 by y -250 to 500: 1000 by 750, on x at 500.
    assert_eq!(area.split(2), Some((0, 500.0, [3, 6])));
    let leaves = (0..31).filter(|&n| area.split(n).is_none()).count();
    assert_eq!(leaves, 16);
    assert_eq!(area.split(4), None);
    // A square splits on y: size[0] > size[1] is false.
    let square = Collision::new(&Bsp {
        models: vec![Model {
            mins: [0.0; 3],
            maxs: [100.0; 3],
            ..bsp(&MapSpec::default()).models[0]
        }],
        ..bsp(&MapSpec::default())
    });
    assert_eq!(square.area().split(0), Some((1, 50.0, [1, 16])));
}

// spec WORLD-5.2: a list keeps the order entities were linked in, less those unlinked
// spec WORLD-6.8: unlinking an entity that is not linked changes nothing
#[test]
fn lists_keep_their_order() {
    let mut host = TestHost::new(&MapSpec::default(), vec![body(), body(), body()]);
    for n in 1..=3 {
        link(&mut host, n);
    }
    assert_eq!(host.collision.area().listed(0, false), [1, 2, 3]);
    host.collision.unlink(2);
    host.collision.unlink(2);
    host.collision.unlink(40);
    assert_eq!(host.collision.area().listed(0, false), [1, 3]);
    link(&mut host, 2);
    assert_eq!(host.collision.area().listed(0, false), [1, 3, 2]);
    assert_eq!(host.collision.area().link_of(2), Some((0, false)));
    assert_eq!(host.collision.area().link_of(9), None);
}

// spec WORLD-6.1: linking again moves an entity; the world and free edicts are not linked
#[test]
fn the_world_and_free_edicts_are_not_linked() {
    let free = Body {
        free: true,
        ..body()
    };
    let mut host = TestHost::new(&MapSpec::default(), vec![body(), free]);
    link(&mut host, 0);
    link(&mut host, 2);
    assert_eq!(host.collision.area().link_of(0), None);
    assert_eq!(host.collision.area().link_of(2), None);
    assert_eq!(host.bodies[2].absmin, [0.0; 3]);
    link(&mut host, 1);
    host.bodies[1].origin = [500.0, 100.0, 0.0];
    link(&mut host, 1);
    let (node, _) = host.collision.area().link_of(1).unwrap();
    assert_ne!(node, 0);
    assert_eq!(host.collision.area().listed(0, false), [0_usize; 0]);
    // Freed after linking: linking again takes it out.
    host.bodies[1].free = true;
    link(&mut host, 1);
    assert_eq!(host.collision.area().link_of(1), None);
}

// spec WORLD-6.2: the box is origin plus mins and maxs, widened by 15 on x and y for an item
// and by 1 everywhere else
#[test]
fn abs_boxes_are_widened() {
    let item = Body {
        flags: FL_ITEM,
        origin: [10.0, 20.0, 30.0],
        ..body()
    };
    let plain = Body {
        flags: FL_MONSTER,
        origin: [10.0, 20.0, 30.0],
        ..body()
    };
    let mut host = TestHost::new(&MapSpec::default(), vec![item, plain]);
    link(&mut host, 1);
    link(&mut host, 2);
    assert_eq!(host.bodies[1].absmin, [-21.0, -11.0, 14.0]);
    assert_eq!(host.bodies[1].absmax, [41.0, 51.0, 46.0]);
    assert_eq!(host.bodies[2].absmin, [-7.0, 3.0, 13.0]);
    assert_eq!(host.bodies[2].absmax, [27.0, 37.0, 47.0]);
}

/// A row of 20 planes at x = 0, 10, ..., 190, each with a leaf in front.
fn row() -> MapSpec {
    let planes = (0..20_u8)
        .map(|i| plane([1.0, 0.0, 0.0], f32::from(i) * 10.0, 0))
        .collect();
    let nodes = (0..20_i32)
        .map(|i| {
            let back = if i == 19 { -22 } else { i + 1 };
            node(usize::try_from(i).unwrap(), [-2 - i, back])
        })
        .collect();
    let leafs = (0..22)
        .map(|l| {
            if l == 0 {
                CONTENTS_SOLID
            } else {
                CONTENTS_EMPTY
            }
        })
        .collect();
    MapSpec {
        planes,
        nodes,
        leafs,
        ..MapSpec::default()
    }
}

// spec WORLD-6.3: an entity with a model records the open leaves it is in, each its number
// less one, up to 16; one with no model records none
#[test]
fn entities_record_their_leaves() {
    let modelled = |origin| Body {
        modelindex: 2.0,
        origin,
        mins: [-1.0; 3],
        maxs: [1.0; 3],
        ..body()
    };
    let mut host = TestHost::new(&MapSpec::default(), vec![modelled([0.0, 0.0, 50.0])]);
    link(&mut host, 1);
    assert_eq!(host.collision.leafs_of(1), [0]);
    // Straddling the floor: the solid leaf adds nothing.
    host.bodies[1].origin = [0.0, 0.0, 0.0];
    link(&mut host, 1);
    assert_eq!(host.collision.leafs_of(1), [0]);
    // Wholly in the solid: none.
    host.bodies[1].origin = [0.0, 0.0, -50.0];
    link(&mut host, 1);
    assert_eq!(host.collision.leafs_of(1), [0_i32; 0]);
    host.bodies[1].origin = [0.0, 0.0, 50.0];
    host.bodies[1].modelindex = 0.0;
    link(&mut host, 1);
    assert_eq!(host.collision.leafs_of(1), [0_i32; 0]);
    assert_eq!(host.collision.leafs_of(30), [0_i32; 0]);

    let mut wide = modelled([95.0, 0.0, 0.0]);
    wide.mins = [-100.0; 3];
    wide.maxs = [100.0; 3];
    let mut host = TestHost::new(&row(), vec![wide]);
    link(&mut host, 1);
    assert_eq!(host.collision.leafs_of(1), (0..16).collect::<Vec<i32>>());
    // Every plane's front is a leaf: a box at x 21 to 29 is in front of plane 0, leaf 1.
    host.bodies[1].origin = [25.0, 0.0, 0.0];
    host.bodies[1].mins = [-3.0; 3];
    host.bodies[1].maxs = [3.0; 3];
    link(&mut host, 1);
    assert_eq!(host.collision.leafs_of(1), [0]);
    // Behind every plane: the last leaf, 21.
    host.bodies[1].origin = [-50.0, 0.0, 0.0];
    link(&mut host, 1);
    assert_eq!(host.collision.leafs_of(1), [20]);
    // Across plane 0 only: its front leaf first, then down its back to the last leaf.
    host.bodies[1].origin = [0.0, 0.0, 0.0];
    link(&mut host, 1);
    assert_eq!(host.collision.leafs_of(1), [0, 20]);
}

// spec WORLD-6.4: an axial plane compares the distance with the box; a general plane the
// corners its sign bits pick
#[test]
fn box_on_plane_side_is_front_back_or_both() {
    let axial = super::Plane {
        normal: [1.0, 0.0, 0.0],
        dist: 5.0,
        kind: 0,
    };
    assert_eq!(box_on_plane_side(([5.0; 3], [9.0; 3]), &axial, 0), 1);
    assert_eq!(box_on_plane_side(([0.0; 3], [5.0; 3]), &axial, 0), 2);
    assert_eq!(box_on_plane_side(([0.0; 3], [6.0; 3]), &axial, 0), 3);
    let on_y = super::Plane {
        normal: [0.0, 1.0, 0.0],
        dist: 5.0,
        kind: 1,
    };
    assert_eq!(
        box_on_plane_side(([0.0, 6.0, 0.0], [1.0, 9.0, 1.0]), &on_y, 0),
        1
    );
    let general = super::Plane {
        normal: [-0.6, 0.8, 0.0],
        dist: 0.0,
        kind: 3,
    };
    // Sign bit 0: the far corner takes mins on x.
    let side = |mins, maxs| box_on_plane_side((mins, maxs), &general, 1);
    assert_eq!(side([-10.0, 1.0, 0.0], [-1.0, 2.0, 0.0]), 1);
    assert_eq!(side([1.0, -2.0, 0.0], [10.0, -1.0, 0.0]), 2);
    assert_eq!(side([-1.0, -1.0, 0.0], [1.0, 1.0, 0.0]), 3);
    // On the plane: dist1 >= dist is front, dist2 < dist is not.
    assert_eq!(side([0.0; 3], [0.0; 3]), 1);
    // The sign bits, not the normal, pick the corners: with none set the far corner is the
    // maxs, (1, 1), and the near one the mins, (0, 0), which is on the plane, not behind it.
    assert_eq!(side([0.0; 3], [1.0, 1.0, 0.0]), 3);
    assert_eq!(
        box_on_plane_side(([0.0; 3], [1.0, 1.0, 0.0]), &general, 0),
        1
    );
    // z's sign bit.
    let down = super::Plane {
        normal: [0.0, 0.6, -0.8],
        dist: 0.0,
        kind: 4,
    };
    assert_eq!(
        box_on_plane_side(([0.0, 0.0, -2.0], [1.0, 1.0, -1.0]), &down, 4),
        1
    );
    assert_eq!(
        box_on_plane_side(([0.0, -2.0, 1.0], [1.0, -1.0, 2.0]), &down, 4),
        2
    );
}

// spec WORLD-6.5: an entity goes in the first node its box crosses, triggers and solids
// apart; SOLID_NOT goes nowhere
#[test]
fn entities_go_where_their_box_crosses() {
    let trigger = Body {
        solid: Solid::Trigger,
        ..body()
    };
    let none = Body {
        solid: Solid::Not,
        ..body()
    };
    let east = Body {
        origin: [600.0, 0.0, 0.0],
        ..body()
    };
    let west = Body {
        origin: [-600.0, 0.0, 0.0],
        ..body()
    };
    let mut host = TestHost::new(&MapSpec::default(), vec![body(), trigger, none, east, west]);
    for n in 1..=5 {
        link(&mut host, n);
    }
    let area = host.collision.area();
    assert_eq!(area.link_of(1), Some((0, false)));
    assert_eq!(area.link_of(2), Some((0, true)));
    assert_eq!(area.link_of(3), None);
    // x 583 to 617: past 0 (node 1), y -17 to 17 above -250 (node 2), past x 500 (node 3),
    // which splits y at 125: below it (node 5)... with y -17 to 17 crossing nothing at 125.
    let (east_node, _) = area.link_of(4).unwrap();
    let (west_node, _) = area.link_of(5).unwrap();
    assert!((1..16).contains(&east_node), "{east_node}");
    assert!((16..31).contains(&west_node), "{west_node}");
    assert_eq!(area.listed(0, true), [2]);
    assert_eq!(area.listed(0, false), [1]);
}

fn trigger_at(origin: [f32; 3]) -> Body {
    Body {
        solid: Solid::Trigger,
        touch: 7,
        origin,
        ..body()
    }
}

// spec WORLD-6.6: linking with touch runs each overlapping trigger's touch, skipping the
// entity itself, a trigger with no touch or not SOLID_TRIGGER, and one that does not overlap
#[test]
fn linking_touches_overlapping_triggers() {
    let no_touch = Body {
        touch: 0,
        ..trigger_at([0.0; 3])
    };
    let mut host = TestHost::new(
        &MapSpec::default(),
        vec![
            trigger_at([0.0; 3]),
            no_touch,
            trigger_at([0.0; 3]),
            trigger_at([300.0, 0.0, 0.0]),
            trigger_at([10.0, 0.0, 0.0]),
            body(),
        ],
    );
    for n in 1..=5 {
        link(&mut host, n);
    }
    // Trigger 3 is linked as a trigger but is no longer SOLID_TRIGGER.
    host.bodies[3].solid = Solid::BBox;
    link_edict(&mut host, 6, true).unwrap();
    assert_eq!(host.touched, [(1, 6), (5, 6)]);
    // A trigger linked with touch does not touch itself.
    host.touched.clear();
    link_edict(&mut host, 1, true).unwrap();
    assert_eq!(host.touched, [(5, 1)]);
    // Without touch, nothing runs.
    host.touched.clear();
    link_edict(&mut host, 6, false).unwrap();
    assert!(host.touched.is_empty());
    // A box that meets a trigger's edge exactly still touches it.
    let mut host = TestHost::new(&MapSpec::default(), vec![trigger_at([0.0; 3]), body()]);
    link(&mut host, 1);
    host.bodies[2].origin = [34.0, 0.0, 0.0];
    link_edict(&mut host, 2, true).unwrap();
    assert_eq!(host.touched, [(1, 2)]);
    host.touched.clear();
    host.bodies[2].origin = [34.5, 0.0, 0.0];
    link_edict(&mut host, 2, true).unwrap();
    assert!(host.touched.is_empty());
    host.bodies[2].origin = [0.0, 0.0, -34.5];
    link_edict(&mut host, 2, true).unwrap();
    assert!(host.touched.is_empty());
}

// spec WORLD-6.6: the touch walk goes on into the children the entity's box reaches
#[test]
fn touches_reach_triggers_in_child_nodes() {
    let mut host = TestHost::new(
        &MapSpec::default(),
        vec![
            trigger_at([600.0, 0.0, 0.0]),
            trigger_at([-600.0, 0.0, 0.0]),
        ],
    );
    link(&mut host, 1);
    link(&mut host, 2);
    let mut wide = body();
    wide.mins = [-700.0, -16.0, -16.0];
    wide.maxs = [700.0, 16.0, 16.0];
    host.bodies.push(wide);
    link_edict(&mut host, 3, true).unwrap();
    assert_eq!(host.touched, [(1, 3), (2, 3)]);
    // A box east of the root's split reaches only child 0.
    host.touched.clear();
    host.bodies[3].mins = [0.0, -16.0, -16.0];
    link_edict(&mut host, 3, true).unwrap();
    assert_eq!(host.touched, [(1, 3)]);
    // And one west of it only child 1.
    host.touched.clear();
    host.bodies[3].mins = [-700.0, -16.0, -16.0];
    host.bodies[3].maxs = [-2.0, 16.0, 16.0];
    link_edict(&mut host, 3, true).unwrap();
    assert_eq!(host.touched, [(2, 3)]);
}

// spec WORLD-6.7: the walk takes the next trigger before a touch runs: one appended later is
// visited, one moved to the end still is, one taken out ends the list
#[test]
fn touches_that_change_the_list() {
    let triggers = vec![trigger_at([0.0; 3]); 3];
    let mut host = TestHost::new(&MapSpec::default(), [triggers, vec![body()]].concat());
    for n in 1..=2 {
        link(&mut host, n);
    }
    // 1's touch appends 3.
    host.effects = vec![(1, Effect::Append(3, 0))];
    link_edict(&mut host, 4, true).unwrap();
    assert_eq!(host.touched, [(1, 4), (2, 4), (3, 4)]);
    // 1's touch moves 2, the next, to the end: still visited, from its new place, so 3, which
    // the walk moved past, is not.
    host.touched.clear();
    host.effects = vec![(1, Effect::Append(2, 0))];
    link_edict(&mut host, 4, true).unwrap();
    assert_eq!(host.touched, [(1, 4), (2, 4)]);
    assert_eq!(host.collision.area().listed(0, true), [1, 3, 2]);
    // 1's touch takes 3, the next, out: the rest of the list is skipped.
    host.touched.clear();
    host.effects = vec![(1, Effect::Unlink(3))];
    link_edict(&mut host, 4, true).unwrap();
    assert_eq!(host.touched, [(1, 4)]);
    // A touch that takes itself out does not stop the walk.
    host.touched.clear();
    host.effects = vec![(1, Effect::Unlink(1))];
    link_edict(&mut host, 4, true).unwrap();
    assert_eq!(host.touched, [(1, 4), (2, 4)]);
}

fn host_with(bodies: Vec<Body>) -> TestHost {
    let count = bodies.len();
    let mut host = TestHost::new(&MapSpec::default(), bodies);
    for n in 1..=count {
        link(&mut host, n);
    }
    host
}

fn moved(
    host: &TestHost,
    (from, to): ([f32; 3], [f32; 3]),
    kind: MoveKind,
    pass: Option<usize>,
) -> Trace {
    sv_move(host, (from, [0.0; 3], [0.0; 3], to), kind, pass).unwrap()
}

/// From x = 0 to x = 200 at height 50.
const ACROSS: ([f32; 3], [f32; 3]) = ([0.0, 0.0, 50.0], [200.0, 0.0, 50.0]);

// spec WORLD-7.1: the trace starts as the move clipped to the world
#[test]
fn a_move_hits_the_world() {
    let host = host_with(Vec::new());
    let down = ([0.0, 0.0, 1.03125], [0.0, 0.0, -0.96875]);
    let trace = moved(&host, down, MoveKind::Normal, None);
    assert_eq!(trace.fraction, 0.5);
    assert_eq!(trace.ent, Some(0));
    let player = sv_move(
        &host,
        (
            [0.0, 0.0, 100.0],
            [-16.0, -16.0, -24.0],
            [16.0, 16.0, 32.0],
            [0.0, 0.0, 0.0],
        ),
        MoveKind::Normal,
        None,
    )
    .unwrap();
    assert!((player.endpos[2] - 24.031_25).abs() < 1e-3);
}

// spec WORLD-7.1: a missile is clipped against monsters with a ±15 box and its bounds grow by
// it, in either direction along each axis
// spec WORLD-7.3: a monster is clipped with the monster box, anything else with the move's
#[test]
fn missiles_hit_monsters_with_a_wider_box() {
    let monster = Body {
        flags: FL_MONSTER,
        origin: [100.0, 30.0, 50.0],
        ..body()
    };
    let crate_box = Body {
        origin: [100.0, -30.0, 50.0],
        ..body()
    };
    let host = host_with(vec![monster, crate_box]);
    // A point 30 off the monster's middle misses it, a missile 15 wider does not.
    assert_eq!(moved(&host, ACROSS, MoveKind::Normal, None).fraction, 1.0);
    let missile = moved(&host, ACROSS, MoveKind::Missile, None);
    assert_eq!(missile.ent, Some(1));
    assert!(missile.fraction < 1.0);
    let back = moved(&host, (ACROSS.1, ACROSS.0), MoveKind::Missile, None);
    assert_eq!(back.ent, Some(1));
    // The crate is no monster: the missile's point misses it.
    let host = host_with(vec![crate_box]);
    assert_eq!(moved(&host, ACROSS, MoveKind::Missile, None).fraction, 1.0);
    // The bounds reach the monster box past the end: a monster whose box starts 11 past the
    // end of a missile's path is in them, and the missile's box reaches it.
    let beyond = Body {
        flags: FL_MONSTER,
        origin: [228.0, 0.0, 50.0],
        ..body()
    };
    let host = host_with(vec![beyond]);
    assert_eq!(moved(&host, ACROSS, MoveKind::Missile, None).ent, Some(1));
    assert_eq!(moved(&host, ACROSS, MoveKind::Normal, None).ent, None);
}

// spec WORLD-7.2: SOLID_NOT and the passedict are skipped, MOVE_NOMONSTERS skips all but
// brush models, entities outside the bounds are skipped, and points never meet points
#[test]
fn moves_skip_what_they_cannot_hit() {
    let in_way = Body {
        origin: [100.0, 0.0, 50.0],
        ..body()
    };
    let host = host_with(vec![in_way]);
    assert_eq!(moved(&host, ACROSS, MoveKind::Normal, None).ent, Some(1));
    assert_eq!(
        moved(&host, ACROSS, MoveKind::Normal, Some(1)).fraction,
        1.0
    );
    assert_eq!(
        moved(&host, ACROSS, MoveKind::NoMonsters, None).fraction,
        1.0
    );
    // Short of it: outside the move's bounds.
    let short = ([0.0, 0.0, 50.0], [60.0, 0.0, 50.0]);
    assert_eq!(moved(&host, short, MoveKind::Normal, None).fraction, 1.0);
    // Below and above it on z.
    let under = ([0.0, 0.0, 10.0], [200.0, 0.0, 10.0]);
    assert_eq!(moved(&host, under, MoveKind::Normal, None).fraction, 1.0);

    let mut host = host_with(vec![in_way]);
    host.bodies[1].solid = Solid::Not;
    assert_eq!(moved(&host, ACROSS, MoveKind::Normal, None).fraction, 1.0);

    // A brush entity stops a MOVE_NOMONSTERS.
    let mut host = host_with(vec![in_way]);
    host.bodies[1] = Body {
        origin: [0.0, 0.0, 100.0],
        ..world()
    };
    link(&mut host, 1);
    let onto = ([0.0, 0.0, 101.03125], [0.0, 0.0, 99.03125]);
    assert_eq!(moved(&host, onto, MoveKind::NoMonsters, None).ent, Some(1));

    // A point entity is not hit by a passedict with a size.
    let point = Body {
        origin: [100.0, 0.0, 50.0],
        size: [0.0; 3],
        ..body()
    };
    let sized = Body {
        origin: [0.0, 0.0, 500.0],
        ..body()
    };
    let pointlike = Body {
        size: [0.0; 3],
        ..sized
    };
    let host = host_with(vec![point, sized, pointlike]);
    assert_eq!(
        moved(&host, ACROSS, MoveKind::Normal, Some(2)).fraction,
        1.0
    );
    assert_eq!(moved(&host, ACROSS, MoveKind::Normal, Some(3)).ent, Some(1));
    assert_eq!(moved(&host, ACROSS, MoveKind::Normal, None).ent, Some(1));
}

// spec WORLD-7.2: a trigger in the solids is id's Sys_Error
#[test]
fn a_trigger_among_the_solids_is_an_error() {
    let mut host = host_with(vec![body()]);
    host.bodies[1].solid = Solid::Trigger;
    let error = sv_move(
        &host,
        ([0.0, 0.0, 50.0], [0.0; 3], [0.0; 3], [0.0, 0.0, 60.0]),
        MoveKind::Normal,
        None,
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "Trigger in clipping list");
}

// spec WORLD-7.3: the passedict's missiles and its owner are not hit; an all-solid trace
// stops the clipping
#[test]
fn owners_and_missiles_pass_each_other() {
    let missile = Body {
        origin: [100.0, 0.0, 50.0],
        owner: Some(2),
        ..body()
    };
    let shooter = Body {
        origin: [0.0, 0.0, 500.0],
        ..body()
    };
    let host = host_with(vec![missile, shooter]);
    assert_eq!(
        moved(&host, ACROSS, MoveKind::Normal, Some(2)).fraction,
        1.0
    );
    assert_eq!(moved(&host, ACROSS, MoveKind::Normal, None).ent, Some(1));
    // The shooter's owner is the box in the way: the shooter's move passes it too.
    let mut host = host_with(vec![body(), shooter]);
    host.bodies[1].origin = [100.0, 0.0, 50.0];
    link(&mut host, 1);
    host.bodies[2].owner = Some(1);
    assert_eq!(
        moved(&host, ACROSS, MoveKind::Normal, Some(2)).fraction,
        1.0
    );
    // Inside the floor the world's trace is all solid: no entity is clipped.
    let mut host = host_with(vec![body()]);
    host.bodies[1].origin = [0.0, 0.0, -50.0];
    link(&mut host, 1);
    let buried = moved(
        &host,
        ([0.0, 0.0, -40.0], [0.0, 0.0, -60.0]),
        MoveKind::Normal,
        None,
    );
    assert!(buried.allsolid);
    assert_eq!(buried.ent, Some(0));
}

// spec WORLD-7.4: the nearest hit wins and names its entity; a start in solid is kept
#[test]
fn the_nearest_hit_wins() {
    let near = Body {
        origin: [100.0, 0.0, 50.0],
        ..body()
    };
    let far = Body {
        origin: [150.0, 0.0, 50.0],
        ..body()
    };
    // Linked far first, so the near one replaces it; linked near first, it stays.
    let host = host_with(vec![far, near]);
    assert_eq!(moved(&host, ACROSS, MoveKind::Normal, None).ent, Some(2));
    let host = host_with(vec![near, far]);
    let trace = moved(&host, ACROSS, MoveKind::Normal, None);
    assert_eq!(trace.ent, Some(1));
    assert!((trace.endpos[0] - 83.968_75).abs() < 1e-3);
    // Starting inside one box and hitting another: startsolid is kept from the first.
    let around = Body {
        origin: [0.0, 0.0, 50.0],
        ..body()
    };
    let host = host_with(vec![around, near]);
    let trace = moved(&host, ACROSS, MoveKind::Normal, None);
    assert!(trace.startsolid);
    assert_eq!(trace.ent, Some(2));
}

// spec WORLD-7.5: the clip walk goes into the children the move's bounds reach
#[test]
fn moves_reach_entities_in_child_nodes() {
    let east = Body {
        origin: [600.0, 0.0, 50.0],
        ..body()
    };
    let west = Body {
        origin: [-600.0, 0.0, 50.0],
        ..body()
    };
    let host = host_with(vec![east, west]);
    let eastward = ([0.0, 0.0, 50.0], [900.0, 0.0, 50.0]);
    let westward = ([0.0, 0.0, 50.0], [-900.0, 0.0, 50.0]);
    assert_eq!(moved(&host, eastward, MoveKind::Normal, None).ent, Some(1));
    assert_eq!(moved(&host, westward, MoveKind::Normal, None).ent, Some(2));
}

// spec WORLD-7.6: an entity in solid tests as stuck, one in the open does not
#[test]
fn test_entity_position_finds_solid() {
    let buried = Body {
        origin: [0.0, 0.0, -50.0],
        ..body()
    };
    let standing = Body {
        origin: [300.0, 0.0, 100.0],
        mins: [-16.0, -16.0, -24.0],
        maxs: [16.0, 16.0, 32.0],
        ..body()
    };
    let host = host_with(vec![buried, standing]);
    assert!(test_entity_position(&host, 1).unwrap());
    assert!(!test_entity_position(&host, 2).unwrap());
    assert!(!test_entity_position(&host, 9).unwrap());
}

#[test]
fn solid_values_read_as_id_compares_them() {
    assert_eq!(Solid::from_float(0.0), Solid::Not);
    assert_eq!(Solid::from_float(-0.0), Solid::Not);
    assert_eq!(Solid::from_float(1.0), Solid::Trigger);
    assert_eq!(Solid::from_float(2.0), Solid::BBox);
    assert_eq!(Solid::from_float(3.0), Solid::SlideBox);
    assert_eq!(Solid::from_float(4.0), Solid::Bsp);
    assert_eq!(Solid::from_float(4.5), Solid::Other(4.5));
    assert_eq!(MoveKind::from_int(1), MoveKind::NoMonsters);
    assert_eq!(MoveKind::from_int(2), MoveKind::Missile);
    assert_eq!(MoveKind::from_int(0), MoveKind::Normal);
    assert_eq!(MoveKind::from_int(7), MoveKind::Normal);
    assert_eq!(CollisionError::BadNode("x").to_string(), "x");
}

// spec WORLD-2.1: a plane of type 0 to 2 reads that coordinate of the point, whatever its
// normal says; any other type the dot product, z included
#[test]
fn a_planes_type_picks_how_it_is_read() {
    let crooked = MapSpec {
        planes: vec![plane([0.0, 1.0, 0.0], 0.0, 0)],
        ..MapSpec::default()
    };
    let collision = Collision::new(&bsp(&crooked));
    // Type 0 reads x, though the normal is y.
    assert_eq!(
        collision.true_point_contents([1.0, -5.0, 0.0]).unwrap(),
        CONTENTS_EMPTY
    );
    assert_eq!(
        collision.true_point_contents([-1.0, 5.0, 0.0]).unwrap(),
        CONTENTS_SOLID
    );
    let tilted = MapSpec {
        planes: vec![plane([0.0, 0.6, 0.8], 10.0, 4)],
        ..MapSpec::default()
    };
    let collision = Collision::new(&bsp(&tilted));
    assert_eq!(
        collision.true_point_contents([0.0, 0.0, 12.5]).unwrap(),
        CONTENTS_EMPTY
    );
    assert_eq!(
        collision.true_point_contents([0.0, 0.0, 12.4]).unwrap(),
        CONTENTS_SOLID
    );
    assert_eq!(
        collision.true_point_contents([0.0, 5.0, 9.0]).unwrap(),
        CONTENTS_EMPTY
    );
}

// spec WORLD-3.3: a line starting on a plane and going behind it is a crossing, not wholly
// behind; one ending on it from behind crosses too
// spec WORLD-3.4: from on the plane the crossing is an epsilon before the start, kept at 0
#[test]
fn lines_with_an_end_on_a_plane() {
    let from_floor = trace_world(&MapSpec::default(), 0, [0.0, 0.0, 0.0], [0.0, 0.0, -1.0]);
    assert_eq!(from_floor.fraction, 0.0);
    assert_eq!(from_floor.endpos, [0.0, 0.0, 0.0]);
    assert!(!from_floor.startsolid && !from_floor.allsolid);
    assert_eq!(from_floor.plane.normal, [0.0, 0.0, 1.0]);
    let up_to_floor = trace_world(&MapSpec::default(), 0, [0.0, 0.0, -1.0], [0.0, 0.0, 0.0]);
    assert!(up_to_floor.startsolid && !up_to_floor.allsolid && up_to_floor.inopen);
}

// spec WORLD-3.4: the crossing point moves along every axis of the line
// spec WORLD-3.5: past a node the line goes on from the crossing, as its share of the move
#[test]
fn crossings_keep_the_whole_line() {
    let slanted = trace_world(
        &MapSpec::default(),
        0,
        [5.0, -2.0, 1.03125],
        [15.0, 6.0, -0.96875],
    );
    assert_eq!(slanted.fraction, 0.5);
    assert_eq!(slanted.endpos, [10.0, 2.0, 0.03125]);
    let spec = MapSpec {
        planes: vec![
            plane([0.0, 0.0, 1.0], 10.0, 2),
            plane([0.0, 0.0, 1.0], 0.0, 2),
        ],
        nodes: vec![node(0, [-2, 1]), node(1, [-2, -1])],
        ..MapSpec::default()
    };
    let to_floor = trace_world(&spec, 0, [0.0, 0.0, 20.0], [0.0, 0.0, -20.0]);
    assert!(
        (to_floor.fraction - 0.499_218_75).abs() < 1e-5,
        "{}",
        to_floor.fraction
    );
    assert!((to_floor.endpos[2] - 0.031_25).abs() < 1e-4);
}

// spec WORLD-3.6: the plane is negated on every axis when it is hit from behind
#[test]
fn a_wall_hit_from_behind() {
    // Solid where x >= 0.
    let wall = MapSpec {
        planes: vec![plane([1.0, 0.0, 0.0], 0.0, 0)],
        nodes: vec![node(0, [-1, -2])],
        ..MapSpec::default()
    };
    let hit = trace_world(&wall, 0, [-1.03125, 0.0, 0.0], [0.96875, 0.0, 0.0]);
    assert_eq!(hit.plane.normal, [-1.0, 0.0, 0.0]);
    assert_eq!(hit.endpos, [-0.03125, 0.0, 0.0]);
    let sloped = MapSpec {
        planes: vec![plane([0.6, 0.8, 0.0], 0.0, 3)],
        nodes: vec![node(0, [-1, -2])],
        ..MapSpec::default()
    };
    let hit = trace_world(&sloped, 0, [-3.0, -4.0, 0.0], [3.0, 4.0, 0.0]);
    assert_eq!(hit.plane.normal, [-0.6, -0.8, 0.0]);
}

// spec WORLD-7.4: of two hits at the same fraction, the first in the list stays
#[test]
fn a_tie_keeps_the_first_hit() {
    let twin = Body {
        origin: [100.0, 0.0, 50.0],
        ..body()
    };
    let host = host_with(vec![twin, twin]);
    assert_eq!(moved(&host, ACROSS, MoveKind::Normal, None).ent, Some(1));
}

// spec WORLD-7.1: the move's bounds take the moving box's size on each axis, from either
// end, so an entity overlapping the start or the side of the path is clipped
#[test]
fn move_bounds_hold_the_whole_box() {
    let boxed = |host: &TestHost, from: [f32; 3], to: [f32; 3]| {
        sv_move(
            host,
            (from, [-16.0; 3], [16.0; 3], to),
            MoveKind::Normal,
            None,
        )
        .unwrap()
    };
    let behind = Body {
        origin: [-20.0, 0.0, 50.0],
        ..body()
    };
    let host = host_with(vec![behind]);
    let trace = boxed(&host, [0.0, 0.0, 50.0], [200.0, 0.0, 50.0]);
    assert!(trace.startsolid);
    assert_eq!(trace.ent, Some(1));
    let ahead_of_back = Body {
        origin: [220.0, 0.0, 50.0],
        ..body()
    };
    let host = host_with(vec![ahead_of_back]);
    let trace = boxed(&host, [200.0, 0.0, 50.0], [0.0, 0.0, 50.0]);
    assert!(trace.startsolid);
    for side in [-20.0, 20.0] {
        let beside = Body {
            origin: [100.0, side, 50.0],
            ..body()
        };
        let host = host_with(vec![beside]);
        let trace = boxed(&host, [0.0, 0.0, 50.0], [200.0, 0.0, 50.0]);
        assert_eq!(trace.ent, Some(1), "{side}");
    }
    // A missile meets a monster on the other side of its path too.
    let monster = Body {
        flags: FL_MONSTER,
        origin: [100.0, -30.0, 50.0],
        ..body()
    };
    let host = host_with(vec![monster]);
    assert_eq!(moved(&host, ACROSS, MoveKind::Missile, None).ent, Some(1));
}

// spec WORLD-6.5: a box whose edge is exactly on a node's plane is linked in that node
#[test]
fn a_box_on_a_split_stays_in_the_node() {
    let east_edge = Body {
        origin: [17.0, 0.0, 0.0],
        ..body()
    };
    let west_edge = Body {
        origin: [-17.0, 0.0, 0.0],
        ..body()
    };
    let host = host_with(vec![east_edge, west_edge]);
    assert_eq!(host.bodies[1].absmin[0], 0.0);
    assert_eq!(host.bodies[2].absmax[0], 0.0);
    assert_eq!(host.collision.area().link_of(1), Some((0, false)));
    assert_eq!(host.collision.area().link_of(2), Some((0, false)));
}

// spec WORLD-6.4: each axis's sign bit picks that axis's corner
#[test]
fn every_axis_has_its_sign_bit() {
    let on_y = super::Plane {
        normal: [0.6, -0.8, 0.0],
        dist: 0.0,
        kind: 3,
    };
    // Bit 1: the far corner takes the mins on y. Box x 0 to 1, y 0 to 1: far (1, 0), 0.6 in
    // front; near (0, 1), -0.8 behind.
    assert_eq!(box_on_plane_side(([0.0; 3], [1.0, 1.0, 0.0]), &on_y, 2), 3);
    // With no bits the far corner is (1, 1), -0.2, not in front, and the near (0, 0), on the
    // plane, not behind: no side at all, which the wrong bits can give.
    assert_eq!(box_on_plane_side(([0.0; 3], [1.0, 1.0, 0.0]), &on_y, 0), 0);
    let on_z = super::Plane {
        normal: [0.0, 0.6, -0.8],
        dist: 0.0,
        kind: 4,
    };
    assert_eq!(box_on_plane_side(([0.0; 3], [0.0, 1.0, 1.0]), &on_z, 4), 3);
    assert_eq!(box_on_plane_side(([0.0; 3], [0.0, 1.0, 1.0]), &on_z, 0), 0);
}

// spec WORLD-1.2: another BSP's model 0 takes a model index of its own, with its own hulls
#[test]
fn a_second_bsp_is_its_own_brush_model() {
    let mut collision = Collision::new(&bsp(&MapSpec::default()));
    let raised = MapSpec {
        planes: vec![
            plane([0.0, 0.0, 1.0], 50.0, 2),
            plane([0.0, 0.0, 1.0], 74.0, 2),
        ],
        ..MapSpec::default()
    };
    collision.add_brush(5, &bsp(&raised));
    let model = *collision.model(5.0).unwrap();
    assert_eq!(model.set, 1);
    let hull = collision.sets()[model.set].hull(0, model.heads);
    assert_eq!(
        hull.point_contents(hull.first, [0.0, 0.0, 49.0]).unwrap(),
        CONTENTS_SOLID
    );
    assert!(collision.model(4.0).is_none());
    // Below the list's end it replaces the index and keeps the rest.
    collision.add_brush(1, &bsp(&raised));
    assert_eq!(collision.model(1.0).unwrap().set, 2);
    assert_eq!(collision.model(5.0).unwrap().set, 1);
}
