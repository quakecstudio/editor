// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2010-2016 Lee Salzman
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: iqm/iqm.cpp, loadfbx, checkversion, parse, skipprop, findbegin, readarray, parsegeometry, parsemodel, parsedeformer, parsecurve, parsexform, parseanimlayer, parseanimstack, parseobjects, parseconnection, parseconnections, process, finish, setcurve, numframes

//! Autodesk's `.fbx` files in their text form, version 7, as a mesh of poses (MDL-10.4), read
//! as FTEQW's model converter reads them: the objects of the file (geometries, the models
//! they belong to, limbs, skin clusters, animation curves, layers and stacks) and the
//! connections among them. The binary form (MDL-11) is written out as text first, by
//! [`binary`], and read the same way.

mod binary;

use core::fmt;
use std::collections::BTreeMap;

use super::Mesh;
use super::obj::Up;
use super::rig::{Affine, Boned, Mat3, RigError, Vec3, blend, from_angles, scaled, turn};

/// The name of the one pose of a model with no animation.
pub const FBX_FRAME: &str = "base";
/// What an animation with no name is called.
pub const FBX_ANIMATION: &str = "anim";
/// What the file's first line holds: "`strstr(p.buf, "FBX 7")`".
pub const FBX_VERSION: &str = "FBX 7";
/// The greatest number of a limb a vertex is blended to: "`limb->index > 255`" is left out.
pub const FBX_MAX_LIMB: usize = 255;
/// The tokens between a property's name and its value: its two types and its flags.
const P_BEFORE_VALUES: usize = 3;
/// The corners of a triangle, and the numbers of a position.
const CORNERS: usize = 3;
/// The numbers of a texture coordinate.
const UV: usize = 2;
/// The ticks of an animation key's time in a second, "`#define FBX_SEC 46186158000.0`".
const FBX_SEC: f64 = 46_186_158_000.0;
/// How many poses a second an animation whose curves are not of one length is sampled at.
/// Ours: the rate QuakeC steps a model's frames at (MD5-3.10).
pub const FBX_SAMPLE_RATE: f64 = 10.0;
/// The most poses an animation is sampled to. Ours.
pub const FBX_MAX_FRAMES: usize = 4096;
/// Half a turn in degrees.
const HALF_TURN: f64 = 180.0;
/// No move, no turn, and a scale of one.
const NONE: Vec3 = [0.0, 0.0, 0.0];
const ONE: Vec3 = [1.0, 1.0, 1.0];

/// Why an `.fbx` file was not read, or is no mesh of poses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FbxError {
    /// The file is not of FBX 7: a text file whose first line does not say [`FBX_VERSION`],
    /// or a binary one of another version.
    Version,
    /// A binary file is cut short, or holds a record, a property or an array that cannot be.
    Binary,
    /// A limb's base pose has no size on an axis.
    Flat,
    /// More animations have one name than can be told apart in a frame's name.
    Names,
}

impl fmt::Display for FbxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Version => "the file is not an FBX of version 7",
            Self::Binary => "the binary file is cut short or damaged",
            Self::Flat => "a limb's base pose has no size on an axis",
            Self::Names => "too many animations have one name for their frames to be named",
        })
    }
}

impl core::error::Error for FbxError {}

/// A token of the file.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Token<'a> {
    /// A word, with the colon after it, if any, taken.
    Prop(&'a str),
    /// A number.
    Number(f64),
    /// What is between double quotes.
    Text(&'a str),
    /// `*` and a count, before an array.
    Array,
    /// `{`.
    Begin,
    /// `}`.
    End,
    /// The end of a line.
    Line,
}

/// The file's tokens (`tokenizer::parse`).
struct Tokens<'a> {
    rest: &'a str,
}

impl<'a> Tokens<'a> {
    /// The next token: spaces and commas are passed over, and a `;` starts a comment to the
    /// end of its line.
    fn token(&mut self) -> Option<Token<'a>> {
        loop {
            let mut chars = self.rest.chars();
            let first = chars.next()?;
            let after = chars.as_str();
            match first {
                '\n' => {
                    self.rest = after;
                    return Some(Token::Line);
                }
                ',' => self.rest = after,
                ';' => {
                    self.rest = after
                        .find('\n')
                        .map_or("", |at| after.get(at..).unwrap_or(""))
                }
                '{' => {
                    self.rest = after;
                    return Some(Token::Begin);
                }
                '}' => {
                    self.rest = after;
                    return Some(Token::End);
                }
                '"' => {
                    let end = after.find(['"', '\n']).unwrap_or(after.len());
                    let (text, rest) = after.split_at_checked(end)?;
                    self.rest = rest.strip_prefix('"').unwrap_or(rest);
                    return Some(Token::Text(text));
                }
                '*' => {
                    self.rest = after.trim_start_matches(|digit: char| digit.is_ascii_digit());
                    return Some(Token::Array);
                }
                '0'..='9' | '.' | '-' | '+' => return Some(Token::Number(self.number())),
                space if space.is_whitespace() => self.rest = after,
                _ => {
                    let end = self
                        .rest
                        .find(|c: char| c.is_whitespace() || c == ':')
                        .unwrap_or(self.rest.len());
                    let (word, rest) = self.rest.split_at_checked(end)?;
                    self.rest = rest.strip_prefix(':').unwrap_or(rest);
                    return Some(Token::Prop(word));
                }
            }
        }
    }

    /// The number the text starts with, as `strtod` reads one: a sign, digits with a point
    /// among them, and an exponent when digits follow its `e`; 0 with one byte taken where
    /// that is no number.
    fn number(&mut self) -> f64 {
        let bytes = self.rest.as_bytes();
        let digits = |from: usize| {
            bytes
                .get(from..)
                .unwrap_or(&[])
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count()
        };
        let is = |at: usize, wanted: &[u8]| bytes.get(at).is_some_and(|byte| wanted.contains(byte));
        let mut end = usize::from(is(0, b"+-"));
        end = end.saturating_add(digits(end));
        if is(end, b".") {
            end = end.saturating_add(1);
            end = end.saturating_add(digits(end));
        }
        if is(end, b"eE") {
            let sign = end.saturating_add(1);
            let from = sign.saturating_add(usize::from(is(sign, b"+-")));
            let exponent = digits(from);
            if exponent > 0 {
                end = from.saturating_add(exponent);
            }
        }
        let found = self
            .rest
            .split_at_checked(end)
            .and_then(|(text, rest)| Some((text.parse::<f64>().ok()?, rest)));
        if let Some((value, rest)) = found {
            self.rest = rest;
            value
        } else {
            self.rest = self.rest.get(1..).unwrap_or("");
            0.0
        }
    }

    /// Passes over the rest of a property: to the end of its line, or through the block it
    /// opens with everything in it. `false` at a `}` that closes the block the property is
    /// in, and at the end of the file.
    fn skip_prop(&mut self) -> bool {
        while let Some(token) = self.token() {
            match token {
                Token::Line => return true,
                Token::Begin => {
                    while let Some(inner) = self.token() {
                        match inner {
                            Token::Prop(_) => {
                                self.skip_prop();
                            }
                            Token::End => return true,
                            _ => {}
                        }
                    }
                    return true;
                }
                Token::End => return false,
                _ => {}
            }
        }
        false
    }

    /// Passes over tokens to the `{` on this line; `false` when the line has none.
    fn find_begin(&mut self) -> bool {
        while let Some(token) = self.token() {
            match token {
                Token::Line => return false,
                Token::Begin => return true,
                _ => {}
            }
        }
        false
    }

    /// The numbers of the block this line opens, an array's.
    fn array(&mut self) -> Vec<f64> {
        let mut values = Vec::new();
        if !self.find_begin() {
            return values;
        }
        while let Some(token) = self.token() {
            match token {
                Token::Number(value) => values.push(value),
                Token::End => break,
                _ => {}
            }
        }
        values
    }

    /// The three numbers of a property of three, after its name: its two types and its flags
    /// are passed over, and what is no number ends them. `None` at the end of the file.
    fn triple(&mut self, into: &mut Vec3) -> Option<()> {
        for _ in 0..P_BEFORE_VALUES {
            self.token()?;
        }
        for part in into {
            match self.token()? {
                Token::Number(value) => *part = value,
                _ => break,
            }
        }
        Some(())
    }
}

/// A geometry: its vertices, and which of them and of its texture coordinates each corner
/// of each polygon is.
#[derive(Clone, Debug, Default)]
struct Geometry {
    verts: Vec<f64>,
    polygons: Vec<f64>,
    uvs: Vec<f64>,
    uv_indexes: Vec<f64>,
    /// The mesh model it belongs to.
    model: Option<u64>,
}

/// A model of the kind `Mesh`: where its geometry stands.
#[derive(Clone, Copy, Debug)]
struct MeshModel {
    geometric_translation: Vec3,
    pre_rotation: Vec3,
    translation: Vec3,
    rotation: Vec3,
    scaling: Vec3,
}

/// A model of the kind `LimbNode`: a joint.
#[derive(Clone, Copy, Debug)]
struct Limb {
    /// Its number among the limbs, in the order the file has them.
    index: usize,
    parent: Option<u64>,
    translation: Vec3,
    rotation: Vec3,
    pre_rotation: Vec3,
    scaling: Vec3,
}

/// A skin's cluster: the vertices of a geometry that follow one limb, each with a weight.
#[derive(Clone, Debug, Default)]
struct Cluster {
    indexes: Vec<f64>,
    weights: Vec<f64>,
    skin: Option<u64>,
    limb: Option<u64>,
}

/// Which of a limb's three properties an animation's curves give.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Channel {
    Translation,
    Rotation,
    Scaling,
}

/// An animation curve node: one of a limb's properties over an animation's frames, a value
/// or a curve for each axis.
#[derive(Clone, Copy, Debug, Default)]
struct Xform {
    limb: Option<u64>,
    channel: Option<Channel>,
    value: Vec3,
    curves: [Option<u64>; 3],
}

/// An animation curve: its keys' times, in ticks, and their values.
#[derive(Clone, Debug, Default)]
struct Curve {
    times: Vec<f64>,
    values: Vec<f64>,
}

impl Curve {
    /// The curve's value at the time `at`: the value of the key at that time, a value
    /// between the two keys around it, in a straight line, or the first or last key's
    /// outside them. `None` for a curve that has not a time for each value.
    fn at(&self, at: f64) -> Option<f64> {
        if self.times.len() != self.values.len() {
            return None;
        }
        let after = self.times.partition_point(|time| *time < at);
        let key = |place: usize| {
            self.times
                .get(place)
                .copied()
                .zip(self.values.get(place).copied())
        };
        match (after.checked_sub(1).and_then(key), key(after)) {
            (Some((t0, v0)), Some((t1, v1))) if t1 > t0 => {
                Some(v0 + (v1 - v0) * (at - t0) / (t1 - t0))
            }
            (_, Some((_, value))) | (Some((_, value)), None) => Some(value),
            (None, None) => None,
        }
    }
}

/// When a pose of an animation is: at one of its curves' keys, by its place, where the
/// curves are of one length, or at a time.
#[derive(Clone, Copy, Debug)]
enum Moment {
    Key(usize),
    Time(f64),
}

/// An object of the file.
#[derive(Clone, Debug)]
enum Node {
    Geometry(Geometry),
    Mesh(MeshModel),
    Limb(Limb),
    Cluster(Cluster),
    /// A skin deformer, and the geometry it deforms.
    Skin(Option<u64>),
    /// An animation curve.
    Curve(Curve),
    Xform(Xform),
    /// An animation layer: its curve nodes.
    Layer(Vec<u64>),
    /// An animation stack: its name and its layers.
    Stack(String, Vec<u64>),
}

/// Which of the two named objects of an animation a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Named {
    Layer,
    Stack,
}

/// An object's number, as the file writes it.
const fn id(number: f64) -> u64 {
    number.to_bits()
}

/// A whole number of an array that names a vertex or a limb.
fn index(value: f64) -> Option<usize> {
    format!("{value}").parse().ok()
}

/// The matrix of a turn by `degrees` about x, then y, then z: `Quat::fromdegrees`.
fn turned(degrees: Vec3) -> Mat3 {
    turn(from_angles(
        degrees.map(|angle| angle * core::f64::consts::PI / HALF_TURN),
    ))
}

/// A turn alone.
fn turning(matrix: Mat3) -> Affine {
    Affine {
        matrix,
        moved: NONE,
    }
}

/// What is read of a file.
#[derive(Default)]
struct Scene {
    nodes: BTreeMap<u64, Node>,
    /// How many limbs the file has had so far.
    limbs: usize,
}

impl Scene {
    /// `Geometry: id, "name", "Mesh" { ... }`.
    fn geometry(&mut self, tokens: &mut Tokens<'_>) {
        let Some(Token::Number(number)) = tokens.token() else {
            tokens.skip_prop();
            return;
        };
        if !tokens.find_begin() {
            return;
        }
        let mut geometry = Geometry::default();
        while let Some(token) = tokens.token() {
            match token {
                Token::End => break,
                Token::Prop("Vertices") => geometry.verts = tokens.array(),
                Token::Prop("PolygonVertexIndex") => geometry.polygons = tokens.array(),
                Token::Prop("LayerElementUV") => {
                    if tokens.find_begin() {
                        while let Some(inner) = tokens.token() {
                            match inner {
                                Token::Prop("UV") => geometry.uvs = tokens.array(),
                                Token::Prop("UVIndex") => geometry.uv_indexes = tokens.array(),
                                Token::Prop(_) => {
                                    tokens.skip_prop();
                                }
                                Token::End => break,
                                _ => {}
                            }
                        }
                    }
                }
                Token::Prop(_) => {
                    tokens.skip_prop();
                }
                _ => {}
            }
        }
        self.nodes.insert(id(number), Node::Geometry(geometry));
    }

    /// `Model: id, "Model::name", "Mesh" { ... }` or `"LimbNode"`: where a geometry stands,
    /// or a joint, from the `P` lines of its `Properties70`.
    fn model(&mut self, tokens: &mut Tokens<'_>) -> Option<()> {
        let (number, _, kind) = (tokens.token()?, tokens.token()?, tokens.token()?);
        let (Token::Number(number), Token::Text(kind)) = (number, kind) else {
            tokens.skip_prop();
            return Some(());
        };
        let mut mesh = MeshModel {
            geometric_translation: NONE,
            pre_rotation: NONE,
            translation: NONE,
            rotation: NONE,
            scaling: ONE,
        };
        let limb = kind == "LimbNode";
        if !(limb || kind == "Mesh") {
            tokens.skip_prop();
            return Some(());
        }
        let number = id(number);
        let index = self.limbs;
        if limb {
            self.limbs = self.limbs.saturating_add(1);
        }
        let node = |mesh: MeshModel| {
            if limb {
                Node::Limb(Limb {
                    index,
                    parent: None,
                    translation: mesh.translation,
                    rotation: mesh.rotation,
                    pre_rotation: mesh.pre_rotation,
                    scaling: mesh.scaling,
                })
            } else {
                Node::Mesh(mesh)
            }
        };
        self.nodes.insert(number, node(mesh));
        if !tokens.find_begin() {
            return Some(());
        }
        while let Some(token) = tokens.token() {
            match token {
                Token::End => break,
                Token::Prop("Properties70") => {
                    if !tokens.find_begin() {
                        break;
                    }
                    while let Some(inner) = tokens.token() {
                        match inner {
                            Token::End => break,
                            Token::Prop("P") => {
                                let into = match tokens.token()? {
                                    Token::Text("PreRotation") => Some(&mut mesh.pre_rotation),
                                    Token::Text("GeometricTranslation") if !limb => {
                                        Some(&mut mesh.geometric_translation)
                                    }
                                    Token::Text("Lcl Translation") => Some(&mut mesh.translation),
                                    Token::Text("Lcl Rotation") => Some(&mut mesh.rotation),
                                    Token::Text("Lcl Scaling") => Some(&mut mesh.scaling),
                                    _ => None,
                                };
                                if let Some(into) = into {
                                    tokens.triple(into)?;
                                }
                                tokens.skip_prop();
                            }
                            Token::Prop(_) => {
                                tokens.skip_prop();
                            }
                            _ => {}
                        }
                    }
                    tokens.skip_prop();
                }
                Token::Prop(_) => {
                    tokens.skip_prop();
                }
                _ => {}
            }
        }
        self.nodes.insert(number, node(mesh));
        Some(())
    }

    /// `Deformer: id, "name", "Skin"` or `"Cluster" { ... }`.
    fn deformer(&mut self, tokens: &mut Tokens<'_>) -> Option<()> {
        let (number, _, kind) = (tokens.token()?, tokens.token()?, tokens.token()?);
        let (Token::Number(number), Token::Text(kind)) = (number, kind) else {
            tokens.skip_prop();
            return Some(());
        };
        match kind {
            "Skin" => {
                self.nodes.insert(id(number), Node::Skin(None));
                tokens.skip_prop();
            }
            "Cluster" => {
                if !tokens.find_begin() {
                    return Some(());
                }
                let mut cluster = Cluster::default();
                while let Some(token) = tokens.token() {
                    match token {
                        Token::End => break,
                        Token::Prop("Indexes") => cluster.indexes = tokens.array(),
                        Token::Prop("Weights") => cluster.weights = tokens.array(),
                        Token::Prop(_) => {
                            tokens.skip_prop();
                        }
                        _ => {}
                    }
                }
                self.nodes.insert(id(number), Node::Cluster(cluster));
            }
            _ => {
                tokens.skip_prop();
            }
        }
        Some(())
    }

    /// `AnimationCurve: id, ... { KeyTime: ..., KeyValueFloat: ... }`.
    fn curve(&mut self, tokens: &mut Tokens<'_>) {
        let Some(Token::Number(number)) = tokens.token() else {
            tokens.skip_prop();
            return;
        };
        let mut curve = Curve::default();
        while let Some(token) = tokens.token() {
            match token {
                Token::End => break,
                Token::Prop("KeyTime") => curve.times = tokens.array(),
                Token::Prop("KeyValueFloat") => curve.values = tokens.array(),
                Token::Prop(_) => {
                    tokens.skip_prop();
                }
                _ => {}
            }
        }
        self.nodes.insert(id(number), Node::Curve(curve));
    }

    /// The `P` lines of a `Properties70` block, each given to `take` with its name once the
    /// tokens before its value are passed over. `None` at the end of the file.
    fn properties<'a>(
        tokens: &mut Tokens<'a>,
        mut take: impl FnMut(&'a str, &mut Tokens<'a>),
    ) -> Option<()> {
        while let Some(token) = tokens.token() {
            match token {
                Token::End => break,
                Token::Prop("Properties70") => {
                    if !tokens.find_begin() {
                        return None;
                    }
                    while let Some(inner) = tokens.token() {
                        match inner {
                            Token::End => break,
                            Token::Prop("P") => {
                                let name = tokens.token()?;
                                for _ in 0..P_BEFORE_VALUES {
                                    tokens.token()?;
                                }
                                if let Token::Text(name) = name {
                                    take(name, tokens);
                                }
                                tokens.skip_prop();
                            }
                            Token::Prop(_) => {
                                tokens.skip_prop();
                            }
                            _ => {}
                        }
                    }
                }
                Token::Prop(_) => {
                    tokens.skip_prop();
                }
                _ => {}
            }
        }
        Some(())
    }

    /// `AnimationCurveNode: id, ... { Properties70: { P: "d|X", ... } }`.
    fn xform(&mut self, tokens: &mut Tokens<'_>) {
        let Some(Token::Number(number)) = tokens.token() else {
            tokens.skip_prop();
            return;
        };
        if !tokens.find_begin() {
            return;
        }
        let mut xform = Xform::default();
        let [x, y, z] = &mut xform.value;
        Self::properties(tokens, |name, tokens| {
            let into = match name {
                "d|X" => &mut *x,
                "d|Y" => &mut *y,
                "d|Z" => &mut *z,
                _ => return,
            };
            if let Some(Token::Number(value)) = tokens.token() {
                *into = value;
            }
        });
        self.nodes.insert(id(number), Node::Xform(xform));
    }

    /// `AnimationLayer: id, "AnimLayer::name", ...` and `AnimationStack: id,
    /// "AnimStack::name", ...`.
    fn named(&mut self, tokens: &mut Tokens<'_>, kind: Named) -> Option<()> {
        let (number, name) = (tokens.token()?, tokens.token()?);
        if let (Token::Number(number), Token::Text(name)) = (number, name) {
            let node = match kind {
                Named::Stack => {
                    let name = name.strip_prefix("AnimStack::").unwrap_or(name);
                    Node::Stack(name.to_owned(), Vec::new())
                }
                Named::Layer => Node::Layer(Vec::new()),
            };
            self.nodes.insert(id(number), node);
        }
        tokens.skip_prop();
        Some(())
    }

    /// The `Objects` block.
    fn objects(&mut self, tokens: &mut Tokens<'_>) {
        if !tokens.find_begin() {
            return;
        }
        while let Some(token) = tokens.token() {
            let read = match token {
                Token::End => break,
                Token::Prop("Geometry") => {
                    self.geometry(tokens);
                    Some(())
                }
                Token::Prop("Model") => self.model(tokens),
                Token::Prop("Deformer") => self.deformer(tokens),
                Token::Prop("AnimationCurve") => {
                    self.curve(tokens);
                    Some(())
                }
                Token::Prop("AnimationCurveNode") => {
                    self.xform(tokens);
                    Some(())
                }
                Token::Prop("AnimationLayer") => self.named(tokens, Named::Layer),
                Token::Prop("AnimationStack") => self.named(tokens, Named::Stack),
                Token::Prop(_) => {
                    tokens.skip_prop();
                    Some(())
                }
                _ => Some(()),
            };
            if read.is_none() {
                break;
            }
        }
    }

    /// A `C` line of the `Connections` block: `"OO", from, to` joins two objects, and `"OP",
    /// from, to, "property"` joins an object to a property of another.
    fn connection(&mut self, tokens: &mut Tokens<'_>) -> Option<()> {
        let (kind, from, to) = (tokens.token()?, tokens.token()?, tokens.token()?);
        let (Token::Text(kind), Token::Number(from), Token::Number(to)) = (kind, from, to) else {
            tokens.skip_prop();
            return Some(());
        };
        let (from, to) = (id(from), id(to));
        let property = match kind {
            "OP" => match tokens.token()? {
                Token::Text(property) => Some(property),
                _ => None,
            },
            _ => None,
        };
        let (source, target) = (self.nodes.get(&from).cloned(), self.nodes.get(&to).cloned());
        match (kind, source, target) {
            ("OO", Some(Node::Limb(_)), Some(Node::Limb(_))) => {
                if let Some(Node::Limb(limb)) = self.nodes.get_mut(&from) {
                    limb.parent = Some(to);
                }
            }
            ("OO", Some(Node::Limb(_)), Some(Node::Cluster(_))) => {
                if let Some(Node::Cluster(cluster)) = self.nodes.get_mut(&to) {
                    cluster.limb = Some(from);
                }
            }
            ("OO", Some(Node::Skin(_)), Some(Node::Geometry(_))) => {
                if let Some(Node::Skin(geometry)) = self.nodes.get_mut(&from) {
                    *geometry = Some(to);
                }
            }
            ("OO", Some(Node::Geometry(_)), Some(Node::Mesh(_))) => {
                if let Some(Node::Geometry(geometry)) = self.nodes.get_mut(&from) {
                    geometry.model = Some(to);
                }
            }
            ("OO", Some(Node::Cluster(_)), Some(Node::Skin(_))) => {
                if let Some(Node::Cluster(cluster)) = self.nodes.get_mut(&from) {
                    cluster.skin = Some(to);
                }
            }
            ("OO", Some(Node::Layer(_)), Some(Node::Stack(..))) => {
                if let Some(Node::Stack(_, layers)) = self.nodes.get_mut(&to) {
                    layers.push(from);
                }
            }
            ("OO", Some(Node::Xform(_)), Some(Node::Layer(_))) => {
                if let Some(Node::Layer(xforms)) = self.nodes.get_mut(&to) {
                    xforms.push(from);
                }
            }
            ("OP", Some(Node::Curve(curve)), Some(Node::Xform(_))) => {
                let axis = match property {
                    Some("d|X") => Some(0),
                    Some("d|Y") => Some(1),
                    Some("d|Z") => Some(2),
                    _ => None,
                };
                // A curve that changes is the axis's; one that does not is its value.
                let values = &curve.values;
                let varies = values.iter().any(|value| Some(value) != values.first());
                if let (Some(axis), Some(Node::Xform(xform))) = (axis, self.nodes.get_mut(&to)) {
                    if varies {
                        if let Some(slot) = xform.curves.get_mut(axis) {
                            *slot = Some(from);
                        }
                    } else if let (Some(value), Some(first)) =
                        (xform.value.get_mut(axis), values.first())
                    {
                        *value = *first;
                    }
                }
            }
            ("OP", Some(Node::Xform(_)), Some(Node::Limb(_))) => {
                let channel = match property {
                    Some("Lcl Translation") => Some(Channel::Translation),
                    Some("Lcl Rotation") => Some(Channel::Rotation),
                    Some("Lcl Scaling") => Some(Channel::Scaling),
                    _ => None,
                };
                if let Some(Node::Xform(xform)) = self.nodes.get_mut(&from) {
                    xform.limb = Some(to);
                    xform.channel = channel;
                }
            }
            _ => {}
        }
        tokens.skip_prop();
        Some(())
    }

    /// The `Connections` block.
    fn connections(&mut self, tokens: &mut Tokens<'_>) {
        if !tokens.find_begin() {
            return;
        }
        while let Some(token) = tokens.token() {
            match token {
                Token::End => break,
                Token::Prop("C") => {
                    if self.connection(tokens).is_none() {
                        break;
                    }
                }
                Token::Prop(_) => {
                    tokens.skip_prop();
                }
                _ => {}
            }
        }
    }

    /// The limbs in the order the file has them.
    fn limbs(&self) -> Vec<Limb> {
        let mut limbs: Vec<Limb> = self
            .nodes
            .values()
            .filter_map(|node| match node {
                Node::Limb(limb) => Some(*limb),
                _ => None,
            })
            .collect();
        limbs.sort_by_key(|limb| limb.index);
        limbs
    }

    /// The limb `number` names.
    fn limb(&self, number: Option<u64>) -> Option<&Limb> {
        match self.nodes.get(&number?) {
            Some(Node::Limb(limb)) => Some(limb),
            _ => None,
        }
    }

    /// The animations: for each stack its name and, for each frame of its first layer, a
    /// pose for each limb, the limb's own with what the layer's curve nodes give put in. A
    /// layer whose curves are not of one length is no animation.
    fn animations(&self, limbs: &[Limb]) -> Vec<(String, Vec<Vec<Affine>>)> {
        let mut animations = Vec::new();
        for node in self.nodes.values() {
            let Node::Stack(name, layers) = node else {
                continue;
            };
            let Some(Node::Layer(xforms)) = layers.first().and_then(|layer| self.nodes.get(layer))
            else {
                continue;
            };
            // Each curve node with the number of the limb it is joined to, found once.
            let xforms: Vec<(usize, Xform)> = xforms
                .iter()
                .filter_map(|xform| match self.nodes.get(xform) {
                    Some(Node::Xform(xform)) => Some((self.limb(xform.limb)?.index, *xform)),
                    _ => None,
                })
                .collect();
            let curve = |number: Option<u64>| match self.nodes.get(&number?) {
                Some(Node::Curve(curve)) => Some(curve),
                _ => None,
            };
            let curves: Vec<&Curve> = xforms
                .iter()
                .flat_map(|(_, xform)| xform.curves)
                .filter_map(curve)
                .collect();
            // Curves of one length are the animation's frames, a key each, as the converter
            // has them. Curves of other lengths, which the converter makes no animation of,
            // are sampled by their keys' times (MDL-10.5).
            let count = curves.first().map_or(0, |curve| curve.values.len());
            let moments: Vec<Moment> = if curves.iter().all(|curve| curve.values.len() == count) {
                (0..count).map(Moment::Key).collect()
            } else {
                let ends = |pick: fn(&Curve) -> Option<f64>| -> Vec<f64> {
                    curves.iter().filter_map(|curve| pick(curve)).collect()
                };
                let starts = ends(|curve| curve.times.first().copied());
                let stops = ends(|curve| curve.times.last().copied());
                let timed = curves.iter().all(|curve| curve.at(0.0).is_some());
                let start = starts.iter().copied().fold(f64::INFINITY, f64::min);
                let stop = stops.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let step = FBX_SEC / FBX_SAMPLE_RATE;
                if !timed || !(stop - start).is_finite() {
                    continue;
                }
                core::iter::successors(Some(start), |at| Some(at + step))
                    .take_while(|at| *at <= stop + step / 2.0)
                    .take(FBX_MAX_FRAMES)
                    .map(Moment::Time)
                    .collect()
            };
            let frames = moments
                .iter()
                .map(|moment| {
                    limbs
                        .iter()
                        .map(|limb| {
                            let mut parts = [limb.translation, limb.rotation, limb.scaling];
                            for (_, xform) in xforms.iter().filter(|(own, _)| *own == limb.index) {
                                let mut value = xform.value;
                                for (part, number) in value.iter_mut().zip(xform.curves) {
                                    let found = curve(number).and_then(|curve| match moment {
                                        Moment::Key(key) => curve.values.get(*key).copied(),
                                        Moment::Time(at) => curve.at(*at),
                                    });
                                    if let Some(found) = found {
                                        *part = found;
                                    }
                                }
                                let [translation, rotation, scaling] = &mut parts;
                                match xform.channel {
                                    Some(Channel::Translation) => *translation = value,
                                    Some(Channel::Rotation) => *rotation = value,
                                    Some(Channel::Scaling) => *scaling = value,
                                    None => {}
                                }
                            }
                            let [translation, rotation, scaling] = parts;
                            limb.pose(translation, rotation, scaling)
                        })
                        .collect()
                })
                .collect();
            animations.push((name.clone(), frames));
        }
        animations
    }

    /// The mesh with its skeleton.
    fn boned(&self) -> Boned {
        let limbs = self.limbs();
        let base: Vec<Affine> = limbs
            .iter()
            .map(|limb| limb.pose(limb.translation, limb.rotation, limb.scaling))
            .collect();
        let parents = limbs
            .iter()
            .map(|limb| self.limb(limb.parent).map(|parent| parent.index))
            .collect();

        let mut positions: Vec<Vec3> = Vec::new();
        let mut texcoords: Vec<[f32; 2]> = Vec::new();
        let mut blends: Vec<Vec<(usize, f64)>> = Vec::new();
        let mut triangles = Vec::new();
        for (number, node) in &self.nodes {
            let Node::Geometry(geometry) = node else {
                continue;
            };
            let corners = geometry.corners();
            let model = match geometry.model.and_then(|model| self.nodes.get(&model)) {
                Some(Node::Mesh(model)) => Some(*model),
                _ => None,
            };
            let place_of = |at: Vec3| model.map_or(at, |model| model.placed(at));
            let (verts, _) = geometry.verts.as_chunks::<CORNERS>();
            let coordinates = geometry.texcoords(&corners);
            // What each of the geometry's own vertices is blended to, by its skin's clusters.
            let mut weights: Vec<Vec<(usize, f64)>> = vec![Vec::new(); verts.len()];
            for cluster in self.nodes.values() {
                let Node::Cluster(cluster) = cluster else {
                    continue;
                };
                let skinned = match cluster.skin.and_then(|skin| self.nodes.get(&skin)) {
                    Some(Node::Skin(geometry)) => *geometry == Some(*number),
                    _ => false,
                };
                let limb = self
                    .limb(cluster.limb)
                    .map(|limb| limb.index)
                    .filter(|limb| *limb <= FBX_MAX_LIMB);
                let (true, Some(limb)) = (skinned, limb) else {
                    continue;
                };
                if cluster.indexes.len() != cluster.weights.len() {
                    continue;
                }
                for (vertex, weight) in cluster.indexes.iter().zip(&cluster.weights) {
                    if let Some(own) = index(*vertex).and_then(|vertex| weights.get_mut(vertex)) {
                        own.push((limb, *weight));
                    }
                }
            }
            // The corners that are one of the geometry's vertices with one texture
            // coordinate are one vertex of the mesh (MDL-10.5).
            let mut welded: BTreeMap<(usize, [u32; 2]), usize> = BTreeMap::new();
            let mut places = Vec::with_capacity(corners.len());
            for (corner, (vertex, _)) in corners.iter().enumerate() {
                let coordinate = coordinates.get(corner).copied();
                let key = (*vertex, coordinate.unwrap_or_default().map(f32::to_bits));
                let next = positions.len();
                let place = *welded.entry(key).or_insert(next);
                if place == next {
                    let at = verts.get(*vertex).copied().unwrap_or(NONE);
                    positions.push(place_of(at));
                    texcoords.extend(coordinate);
                    blends.push(blend(weights.get(*vertex).map_or(&[], Vec::as_slice)));
                }
                places.push(place);
            }
            // A polygon is a fan from its first corner, each triangle turned around.
            let mut polygon: Vec<usize> = Vec::new();
            for (place, (_, last)) in places.iter().zip(&corners) {
                polygon.push(*place);
                if !last {
                    continue;
                }
                if let Some((&hub, rest)) = polygon.split_first() {
                    for pair in rest.windows(2) {
                        if let [second, third] = *pair {
                            triangles.push([second, hub, third]);
                        }
                    }
                }
                polygon.clear();
            }
        }

        let animations = self.animations(&limbs);

        Boned {
            positions,
            texcoords,
            blends,
            triangles,
            parents,
            base,
            animations,
            rest: FBX_FRAME,
            unnamed: FBX_ANIMATION,
        }
    }
}

impl Limb {
    /// The limb's pose of a translation, a rotation and a scaling: its scale, its turn after
    /// its pre-rotation's, and its move.
    fn pose(&self, translation: Vec3, rotation: Vec3, scaling: Vec3) -> Affine {
        let matrix = turning(turned(self.pre_rotation))
            .after(&turning(turned(rotation)))
            .matrix;
        scaled(matrix, scaling, translation)
    }
}

impl MeshModel {
    /// Where a vertex of the model's geometry stands: moved by the geometric translation,
    /// scaled, turned by the rotation and then the pre-rotation, and moved by the
    /// translation.
    fn placed(&self, at: Vec3) -> Vec3 {
        let [gx, gy, gz] = self.geometric_translation;
        let [sx, sy, sz] = self.scaling;
        let [x, y, z] = at;
        let scaled = [(x + gx) * sx, (y + gy) * sy, (z + gz) * sz];
        let turned_once = turning(turned(self.rotation)).apply(scaled);
        Affine {
            matrix: turned(self.pre_rotation),
            moved: self.translation,
        }
        .apply(turned_once)
    }
}

impl Geometry {
    /// The corners of the polygons in turn: the vertex each is, and whether it is its
    /// polygon's last, which the file marks with a number below 0, `-(vertex + 1)`. A
    /// geometry with no polygons has its vertices three to a triangle.
    fn corners(&self) -> Vec<(usize, bool)> {
        if self.polygons.is_empty() {
            let count = self.verts.as_chunks::<CORNERS>().0.len();
            return (0..count)
                .map(|vertex| {
                    (
                        vertex,
                        vertex.checked_rem(CORNERS) == Some(CORNERS.saturating_sub(1)),
                    )
                })
                .collect();
        }
        self.polygons
            .iter()
            .map(|value| {
                let last = *value < 0.0;
                let vertex = if last { -(value + 1.0) } else { *value };
                (index(vertex).unwrap_or(0), last)
            })
            .collect()
    }

    /// The texture coordinate of each corner, `v` measured up the picture ("`1-uvs[idx+1]`");
    /// none when the file has not one for each.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec MDL-10.4: a texture coordinate is kept in single precision"
    )]
    fn texcoords(&self, corners: &[(usize, bool)]) -> Vec<[f32; 2]> {
        let (uvs, _) = self.uvs.as_chunks::<UV>();
        let by_vertex = uvs.len() == self.verts.as_chunks::<CORNERS>().0.len();
        let found: Option<Vec<[f32; 2]>> = corners
            .iter()
            .enumerate()
            .map(|(corner, (vertex, _))| {
                let at = if self.uv_indexes.is_empty() {
                    if by_vertex && !self.polygons.is_empty() {
                        *vertex
                    } else {
                        corner
                    }
                } else {
                    index(*self.uv_indexes.get(corner)?)?
                };
                let [u, v] = *uvs.get(at)?;
                Some([u as f32, (1.0 - v) as f32])
            })
            .collect();
        found.unwrap_or_default()
    }
}

/// Reads a text `.fbx` file as a mesh: a pose for each frame of each of its animation
/// stacks, or one of the mesh as it is.
///
/// Its axes are turned as `up` says, as an `.obj`'s are (MDL-9.1): with y up, a position
/// `(x, y, z)` is Quake's `(-z, -x, y)`, so that a model facing -z faces forward. The
/// converter turns a file a quarter about x instead, "`erotate *= Quat(M_PI/2, Vec3(1, 0,
/// 0));`", which leaves such a model facing left (MDL-10.5).
///
/// Implements MDL-10.4.
///
/// # Errors
///
/// [`FbxError`]: a file whose first line does not say `FBX 7`, a limb of no size, and
/// animations whose frames cannot be named apart.
pub fn read(text: &str, up: Up) -> Result<Mesh, FbxError> {
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    if !first.contains(FBX_VERSION) {
        return Err(FbxError::Version);
    }
    let mut tokens = Tokens { rest };
    let mut scene = Scene::default();
    while let Some(token) = tokens.token() {
        match token {
            Token::Prop("Objects") => scene.objects(&mut tokens),
            Token::Prop("Connections") => scene.connections(&mut tokens),
            Token::Prop(_) => {
                tokens.skip_prop();
            }
            _ => {}
        }
    }
    scene.boned().mesh(up).map_err(|error| match error {
        RigError::Flat => FbxError::Flat,
        RigError::Names => FbxError::Names,
    })
}

/// Reads an `.fbx` file of either form as a mesh: a binary file, known by its first bytes,
/// is written out as the text form and read as [`read`] reads one.
///
/// Implements MDL-10.4, MDL-10.5 and MDL-11.
///
/// # Errors
///
/// As [`read`], and [`FbxError::Binary`] for a binary file that is cut short or damaged.
pub fn read_bytes(bytes: &[u8], up: Up) -> Result<Mesh, FbxError> {
    if binary::is_binary(bytes) {
        read(&binary::text(bytes)?, up)
    } else {
        read(&String::from_utf8_lossy(bytes), up)
    }
}

#[cfg(test)]
mod tests;
