// SPDX-License-Identifier: GPL-3.0-or-later

//! A bot's mind (BOT-3 to BOT-7): each frame, from what it sees, the move a client in its place
//! sends: where it goes along the graph, what it fights and aims at, which weapon it holds, and
//! fire pressed to respawn.

use qc_vm::convert::{double_to_float, float_to_int};
use qs_map_gen::playable::{Graph, Place};
use qs_sim::{Body, BotSkill, ClientId, UserCmd};

use crate::aim::{Aim, FIRE_CONE, angle_between, angles_to, turned};
use crate::goals::{choose, follow_need, item_need};
use crate::rng::BotRng;
use crate::weapons::Weapons;

/// The buttons of a move (PHYS-9.6): attack, and jump.
const ATTACK: u8 = 1;
const JUMP: u8 = 2;
/// How fast a bot asks to move (BOT-7.1), in units a second, and to swim up.
const RUN: f32 = 400.0;
const SWIM_UP: f32 = 200.0;
/// How long a bot keeps a route before choosing again, in seconds (BOT-4.1).
const RECHOOSE: f64 = 4.0;
/// How long a bot may move less than [`STUCK_DISTANCE`] before it chooses again (BOT-4.1).
const STUCK_TIME: f64 = 2.0;
const STUCK_DISTANCE: f32 = 32.0;
/// How long an item a bot reached is passed over (BOT-4.5), in seconds.
const VISITED: f64 = 10.0;
/// How close across, and how far below, a bot comes to a place of its route to take the next;
/// one above it is reached within a step, [`STEP`] (BOT-4.4).
const REACHED_ACROSS: f32 = 24.0;
const REACHED_HEIGHT: f32 = 48.0;
/// The heights and widths of BOT-4.4: a step a bot walks up, the most it jumps up, how near
/// across a ledge it jumps onto, and how far a gap is that it jumps.
const STEP: f32 = 18.0;
const JUMP_HEIGHT: f32 = 48.0;
const JUMP_NEAR: f32 = 64.0;
const GAP: f32 = 40.0;
/// How long apart a bot draws a fresh aim error, and sends a weapon's impulse, in seconds
/// (BOT-5.1, BOT-6.2).
const ERROR_TIME: f64 = 0.5;
const IMPULSE_TIME: f64 = 0.5;
/// A player's origin above the floor it stands on (`VEC_HULL_MIN`), where an item's place is
/// looked for.
const STANDING: f32 = 24.0;
/// How many places a bot with nothing to go to tries at random (BOT-4.1).
const WANDER_TRIES: usize = 4;
/// The origin, where a new mind takes the bot to be until it looks, and no aim error.
const NOWHERE: [f32; 3] = [0.0, 0.0, 0.0];
const NO_ERROR: [f32; 2] = [0.0, 0.0];

/// Which kind of game a bot plays, from the `deathmatch` setting (BOT-3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// `deathmatch` is not 0: every other player is a foe.
    Deathmatch,
    /// Coop or single player: monsters are foes and players friends.
    Coop,
}

/// What a bot sees in a frame (BOT-1.6, BOT-3.2).
#[derive(Clone, Copy, Debug)]
pub struct View<'a> {
    /// Its own edict.
    pub me: &'a Body,
    /// Every edict in use.
    pub bodies: &'a [Body],
    /// The foes it sees, by edict number.
    pub seen: &'a [usize],
    /// The kind of game.
    pub mode: Mode,
    /// The server's time, in seconds.
    pub time: f64,
    /// The frame's length, in seconds.
    pub frametime: f64,
}

impl<'a> View<'a> {
    /// The frame's length as an `f32`.
    fn seconds(&self) -> f32 {
        double_to_float(self.frametime)
    }

    /// The nearest other client that is alive (BOT-3.1).
    fn nearest_friend(&self) -> Option<&'a Body> {
        let me = self.me;
        self.bodies
            .iter()
            .filter(|b| b.number != me.number && is_client(b) && alive(b))
            .min_by(|a, b| distance(me.origin, a.origin).total_cmp(&distance(me.origin, b.origin)))
    }
}

/// Where a bot is going.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Goal {
    /// The item of this edict.
    Item(usize),
    /// The friend of this edict.
    Friend(usize),
    /// A place of the graph.
    Wander,
}

/// A bot's mind (BOT-3 to BOT-8).
#[derive(Clone, Debug)]
pub struct Mind {
    id: ClientId,
    skill: BotSkill,
    aim: Aim,
    rng: BotRng,
    /// The route it walks, and the place of it it goes to next.
    route: Vec<[f32; 3]>,
    next: usize,
    goal: Option<Goal>,
    /// When it chose, and where it was when it last checked it was moving.
    chosen_at: f64,
    moved: (f64, [f32; 3]),
    /// The foe it fights, and since when it has seen it.
    foe: Option<(usize, f64)>,
    /// Its aim's error, and when it was drawn.
    error: ([f32; 2], f64),
    /// When it last sent a weapon's impulse.
    impulse_at: f64,
    /// Whether it pressed fire last frame while dead (BOT-3.3).
    pressed: bool,
    /// The items it reached, each with when (BOT-4.5).
    visited: Vec<(usize, f64)>,
    /// Whether its route is done and it should choose again.
    stale: bool,
}

/// How far apart `a` and `b` are across, and up or down.
pub(crate) fn apart(a: [f32; 3], b: [f32; 3]) -> (f32, f32) {
    ((b[0] - a[0]).hypot(b[1] - a[1]), b[2] - a[2])
}

/// The middle of `body`'s box.
pub(crate) fn middle(body: &Body) -> [f32; 3] {
    let ([a, b, c], [d, e, f]) = (body.absmin, body.absmax);
    [(a + d) / 2.0, (b + e) / 2.0, (c + f) / 2.0]
}

/// `body`'s eye.
pub(crate) fn eye(body: &Body) -> [f32; 3] {
    let ([x, y, z], [a, b, c]) = (body.origin, body.view_ofs);
    [x + a, y + b, z + c]
}

/// How far apart two points are.
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (across, up) = apart(a, b);
    across.hypot(up)
}

impl Mind {
    /// The mind of the bot in slot `id`, at `skill`, its random numbers from `seed`
    /// (BOT-8.1).
    #[must_use]
    pub fn new(id: ClientId, skill: BotSkill, seed: u64) -> Self {
        Self {
            id,
            skill,
            aim: Aim::of(skill),
            rng: BotRng::new(seed, id),
            route: Vec::new(),
            next: 0,
            goal: None,
            chosen_at: f64::NEG_INFINITY,
            moved: (0.0, NOWHERE),
            foe: None,
            error: (NO_ERROR, f64::NEG_INFINITY),
            impulse_at: f64::NEG_INFINITY,
            pressed: false,
            visited: Vec::new(),
            stale: true,
        }
    }

    /// Its slot.
    #[must_use]
    pub const fn id(&self) -> ClientId {
        self.id
    }

    /// Its skill.
    #[must_use]
    pub const fn skill(&self) -> BotSkill {
        self.skill
    }

    /// The move it sends this frame (BOT-3 to BOT-7).
    pub fn decide(&mut self, view: &View<'_>, graph: Option<&Graph>, weapons: &Weapons) -> UserCmd {
        let me = view.me;
        if me.deadflag != 0.0 {
            return self.respawn(me);
        }
        self.pressed = false;
        let look = [me.v_angle[0], me.v_angle[1]];
        let step = self.aim.turn * view.seconds();
        let mut cmd = UserCmd::default();
        // What to fight (BOT-3.2): the foe it fights while it still sees it, else the nearest.
        self.foe = self.pick_foe(view);
        let aim = self.aim_at(view);
        // Where to go (BOT-4).
        if let Some(graph) = graph {
            self.walk(view, graph, weapons, &mut cmd);
        }
        self.advance(me);
        let want = aim.unwrap_or_else(|| {
            let toward = self.route.get(self.next).copied();
            toward.map_or(look, |point| {
                let [_, yaw] = angles_to(me.origin, point);
                [0.0, yaw]
            })
        });
        let [pitch, yaw] = turned(look, want, step);
        cmd.angles = [pitch, yaw, 0.0];
        // The move asked for is where it goes, from where it now looks (BOT-7.1).
        self.steer(me, yaw, &mut cmd);
        // Fire (BOT-5.3).
        if let (Some(aim), Some((_, since))) = (aim, self.foe) {
            let seen_long = view.time - since >= f64::from(self.aim.reaction);
            let on_target = angle_between(yaw, aim[1]).abs() <= FIRE_CONE
                && angle_between(pitch, aim[0]).abs() <= FIRE_CONE;
            if seen_long && on_target {
                cmd.buttons |= ATTACK;
            }
        }
        // The weapon (BOT-6.2).
        if let Some(best) = weapons.best(me)
            && !best.in_hand(me)
            && view.time - self.impulse_at >= IMPULSE_TIME
        {
            cmd.impulse = best.impulse;
            self.impulse_at = view.time;
        }
        cmd
    }

    /// Where it aims, pitch and yaw, at the foe it fights (BOT-5.1): the middle of the foe's box
    /// from its eye, off by its error, drawn afresh every half second.
    pub(crate) fn aim_at(&mut self, view: &View<'_>) -> Option<[f32; 2]> {
        let (foe, _) = self.foe?;
        let body = view.bodies.iter().find(|b| b.number == foe)?;
        let [pitch, yaw] = angles_to(eye(view.me), middle(body));
        if view.time - self.error.1 >= ERROR_TIME {
            self.error = (self.aim.draw_error(&mut self.rng), view.time);
        }
        let [ep, ey] = self.error.0;
        Some([pitch + ep, yaw + ey])
    }

    /// A dead bot's move (BOT-3.3): fire released, then pressed, in turn.
    fn respawn(&mut self, me: &Body) -> UserCmd {
        self.pressed = !self.pressed;
        self.route.clear();
        self.stale = true;
        self.foe = None;
        UserCmd {
            angles: me.v_angle,
            buttons: if self.pressed { ATTACK } else { 0 },
            ..UserCmd::default()
        }
    }

    /// The foe it fights (BOT-3.2): the one it fights when it still sees it, else the nearest
    /// it sees, with since when it has seen it.
    fn pick_foe(&self, view: &View<'_>) -> Option<(usize, f64)> {
        if let Some((foe, since)) = self.foe
            && view.seen.contains(&foe)
        {
            return Some((foe, since));
        }
        let me = view.me.origin;
        view.seen
            .iter()
            .filter_map(|&n| view.bodies.iter().find(|b| b.number == n))
            .map(|body| (body.number, distance(me, body.origin)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(n, _)| (n, view.time))
    }

    /// Walks its route, choosing a goal and a route to it when it needs one (BOT-4.1,
    /// BOT-4.4).
    fn walk(&mut self, view: &View<'_>, graph: &Graph, weapons: &Weapons, cmd: &mut UserCmd) {
        let me = view.me;
        self.visited.retain(|&(_, at)| view.time - at < VISITED);
        if let Some(Goal::Item(item)) = self.goal
            && let Some(body) = view.bodies.iter().find(|b| b.number == item)
        {
            let (across, up) = apart(me.origin, body.origin);
            if across <= REACHED_ACROSS * 2.0 && up.abs() <= REACHED_HEIGHT {
                self.visited.push((item, view.time));
                self.stale = true;
            }
        }
        // Stuck: it moved less than STUCK_DISTANCE in STUCK_TIME.
        if view.time - self.moved.0 >= STUCK_TIME {
            if distance(self.moved.1, me.origin) < STUCK_DISTANCE && !self.route.is_empty() {
                self.stale = true;
                cmd.buttons |= JUMP;
            }
            self.moved = (view.time, me.origin);
        }
        if self.stale || self.next >= self.route.len() || view.time - self.chosen_at >= RECHOOSE {
            self.choose(view, graph, weapons);
        }
    }

    /// Takes the places of its route it has come to (BOT-4.4).
    fn advance(&mut self, me: &Body) {
        while let Some(&point) = self.route.get(self.next) {
            let (across, up) = apart(me.origin, point);
            if across <= REACHED_ACROSS && (-REACHED_HEIGHT..=STEP).contains(&up) {
                self.next = self.next.saturating_add(1);
            } else {
                break;
            }
        }
    }

    /// Walks `route` from its start, as though it had chosen it.
    #[cfg(test)]
    pub(crate) fn follow(&mut self, route: Vec<[f32; 3]>, time: f64) {
        self.route = route;
        self.next = 0;
        self.stale = false;
        self.chosen_at = time;
    }

    /// The friend it goes to, if any.
    #[cfg(test)]
    pub(crate) const fn friend_goal(&self) -> Option<usize> {
        match self.goal {
            Some(Goal::Friend(n)) => Some(n),
            Some(Goal::Item(_) | Goal::Wander) | None => None,
        }
    }

    /// The item it goes for, if any.
    #[cfg(test)]
    pub(crate) const fn item_goal(&self) -> Option<usize> {
        match self.goal {
            Some(Goal::Item(n)) => Some(n),
            Some(Goal::Friend(_) | Goal::Wander) | None => None,
        }
    }

    /// Chooses a goal and its route (BOT-4.1 to BOT-4.3, BOT-4.5).
    fn choose(&mut self, view: &View<'_>, graph: &Graph, weapons: &Weapons) {
        let me = view.me;
        self.chosen_at = view.time;
        self.stale = false;
        self.route.clear();
        self.next = 0;
        self.goal = None;
        let Some(from) = graph.nearest(me.origin) else {
            return;
        };
        let mut goals: Vec<(Goal, f32, Place)> = Vec::new();
        for body in view.bodies {
            if self.visited.iter().any(|&(n, _)| n == body.number) {
                continue;
            }
            let need = item_need(me, body, weapons);
            if need <= 0.0 {
                continue;
            }
            let [x, y, z] = body.origin;
            if let Some(place) = graph.nearest([x, y, z + STANDING]) {
                goals.push((Goal::Item(body.number), need, place));
            }
        }
        if view.mode == Mode::Coop
            && let Some(friend) = view.nearest_friend()
        {
            // A friend near enough weighs nothing, which `choose` passes over.
            let need = follow_need(distance(me.origin, friend.origin));
            if let Some(place) = graph.nearest(friend.origin) {
                goals.push((Goal::Friend(friend.number), need, place));
            }
        }
        let places: Vec<Place> = goals.iter().map(|&(_, _, place)| place).collect();
        let routes = graph.routes(from, &places);
        let weighed: Vec<(f32, Option<f32>)> = goals
            .iter()
            .zip(&routes)
            .map(|(&(_, need, _), route)| (need, route.as_ref().map(|r| r.length())))
            .collect();
        if let Some(best) = choose(&weighed)
            && let (Some(&(goal, _, _)), Some(Some(route))) = (goals.get(best), routes.get(best))
        {
            self.goal = Some(goal);
            self.route = route.points().to_vec();
            return;
        }
        // Nothing to go to: a place at random.
        for _ in 0..WANDER_TRIES {
            let Some(place) = graph.place(self.rng.below(graph.len())) else {
                break;
            };
            if let Some(route) = graph.route(from, place) {
                self.goal = Some(Goal::Wander);
                self.route = route.points().to_vec();
                return;
            }
        }
    }

    /// The speeds, jump and swim of its move toward the next place of its route, from where it
    /// looks at `yaw` (BOT-4.4, BOT-7.1).
    fn steer(&self, me: &Body, yaw: f32, cmd: &mut UserCmd) {
        let Some(&point) = self.route.get(self.next) else {
            return;
        };
        let (across, up) = apart(me.origin, point);
        // On a lift going up, it waits.
        if up > JUMP_HEIGHT && across <= REACHED_ACROSS {
            return;
        }
        let [_, toward] = angles_to(me.origin, point);
        let off = angle_between(yaw, toward).to_radians();
        cmd.forward = RUN * off.cos();
        // `side` is to the right, a negative angle from the view.
        cmd.side = -RUN * off.sin();
        let ledge = up > STEP && up <= JUMP_HEIGHT && across <= JUMP_NEAR;
        // A move of the graph longer than a pace or two is a leap across a gap.
        let gap = self
            .next
            .checked_sub(1)
            .and_then(|before| self.route.get(before))
            .is_some_and(|&from| apart(from, point).0 > GAP);
        if ledge || gap {
            cmd.buttons |= JUMP;
        }
        if me.waterlevel >= 2.0 && up > 0.0 {
            cmd.up = SWIM_UP;
        }
    }
}

/// `FL_CLIENT` and `FL_MONSTER` (`defs.qc`).
const FL_CLIENT: u32 = 8;
const FL_MONSTER: u32 = 32;

/// Whether `body`'s `flags` hold `flag`.
fn has_flag(body: &Body, flag: u32) -> bool {
    u32::try_from(float_to_int(body.flags)).is_ok_and(|flags| flags & flag != 0)
}

/// Whether `body` is a client.
#[must_use]
pub(crate) fn is_client(body: &Body) -> bool {
    has_flag(body, FL_CLIENT)
}

/// Whether `body` is alive (BOT-3.1).
#[must_use]
pub(crate) fn alive(body: &Body) -> bool {
    body.deadflag == 0.0 && body.health > 0.0
}

/// Whether `body` is a foe of `me` in `mode` (BOT-3.1).
#[must_use]
pub(crate) fn is_foe(me: &Body, body: &Body, mode: Mode) -> bool {
    if body.number == me.number || !alive(body) {
        return false;
    }
    match mode {
        Mode::Deathmatch => is_client(body) && body.modelindex != 0.0,
        Mode::Coop => has_flag(body, FL_MONSTER) && body.takedamage != 0.0,
    }
}

/// How far a bot sees, and how far to either side of where it looks (BOT-3.2).
pub(crate) const SIGHT_RANGE: f32 = 2048.0;
pub(crate) const SIGHT_CONE: f32 = 90.0;

/// Whether `body` is where `me` could see it (BOT-3.2), before the line to it is traced: in
/// range and within the cone of where it looks; and the two ends of that line.
#[must_use]
pub(crate) fn in_sight(me: &Body, body: &Body) -> Option<([f32; 3], [f32; 3])> {
    let from = eye(me);
    let to = middle(body);
    if distance(from, to) > SIGHT_RANGE {
        return None;
    }
    let [_, yaw] = angles_to(from, to);
    (angle_between(me.v_angle[1], yaw).abs() <= SIGHT_CONE).then_some((from, to))
}
