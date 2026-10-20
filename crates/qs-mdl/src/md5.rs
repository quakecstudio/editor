// SPDX-License-Identifier: GPL-3.0-or-later

//! The 2021 rerelease's enhanced models: `.md5mesh` and `.md5anim` files (MD5-1 to MD5-3).
//!
//! id's rerelease engine is not published, so the format follows the engine that reads the
//! rerelease's files, QuakeSpasm-Spiked, cited in `docs/spec/md5.md` for description only.
//! Nothing here is ported from it: where its checks are missing or wrong, the spec fixes a
//! stricter answer.

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;

use core::fmt;
use std::collections::BTreeMap;

/// The four bytes a mesh file starts with, which pick the MD5 loader (MD5-2.1).
pub const MD5_MAGIC: [u8; 4] = *b"MD5V";

/// The only version token the loader accepts (MD5-2.2, MD5-3.2).
pub const MD5_VERSION: &str = "10";

/// The parent of a joint with no parent (MD5-2.5).
pub const MD5_NO_PARENT: i64 = -1;

/// The number of frames a skin may have (MD5-2.13).
pub const MD5_MAX_SKIN_FRAMES: u32 = 4;

/// The animation flags a hierarchy entry may set (MD5-3.3).
pub const MD5_ANIM_FLAGS: u64 = 63;

/// Seconds each animation frame is shown for (MD5-3.10).
pub const MD5_FRAME_INTERVAL: f32 = 0.1;

/// Why an MD5 file could not be read, or an animation does not fit its mesh.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Md5Error {
    /// The mesh file does not start with `MD5V` (MD5-2.1).
    NotMd5,
    /// A required keyword is missing or different (MD5-2, MD5-3).
    Expected {
        /// The keyword.
        keyword: &'static str,
    },
    /// The file ends where a token is required (MD5-1.9).
    UnexpectedEnd,
    /// A token is not the integer or float required (MD5-1.7, MD5-1.8).
    BadNumber {
        /// The token.
        token: Vec<u8>,
    },
    /// `numJoints` is 0 (MD5-2.3).
    NoBones,
    /// `numMeshes` is 0 (MD5-2.3).
    NoMeshes,
    /// A joint index is out of range (MD5-2.5, MD5-2.11).
    BoneOutOfBounds,
    /// A vertex index is out of range (MD5-2.9, MD5-2.10).
    VertexOutOfBounds,
    /// A triangle index is out of range (MD5-2.10).
    TriangleOutOfBounds,
    /// A weight index, or a vertex's weights, are out of range (MD5-2.11, MD5-2.12).
    WeightOutOfBounds,
    /// `numFrames` is 0 (MD5-3.2).
    NoPoses,
    /// A hierarchy entry sets flags above the low six bits (MD5-3.3).
    UnsupportedFlags,
    /// A hierarchy entry's components run past `numAnimatedComponents` (MD5-3.4).
    BadOffset,
    /// `numAnimatedComponents` is more than the file's length in bytes (MD5-3.11).
    TooManyComponents,
    /// A frame index is out of range (MD5-3.6).
    InvalidPoseIndex,
    /// The animation's joint count differs from the mesh's (MD5-3.9).
    IncorrectBoneCount,
    /// A hierarchy entry's name differs from the mesh joint's (MD5-3.9).
    BoneRenamed,
    /// A hierarchy entry's parent differs from the mesh joint's (MD5-3.9).
    WrongParent,
}

impl fmt::Display for Md5Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotMd5 => f.write_str("is not an MD5 model (no MD5Version)"),
            Self::Expected { keyword } => write!(f, "Expected \"{keyword}\""),
            Self::UnexpectedEnd => f.write_str("ends where a token is required"),
            Self::BadNumber { token } => write!(f, "\"{}\" is not a number", token.escape_ascii()),
            Self::NoBones => f.write_str("has no bones"),
            Self::NoMeshes => f.write_str("has no meshes"),
            Self::BoneOutOfBounds => f.write_str("bone index out of bounds"),
            Self::VertexOutOfBounds => f.write_str("vertex index out of bounds"),
            Self::TriangleOutOfBounds => f.write_str("triangle index out of bounds"),
            Self::WeightOutOfBounds => f.write_str("weight index out of bounds"),
            Self::NoPoses => f.write_str("has no poses"),
            Self::UnsupportedFlags => f.write_str("bone has unsupported flags"),
            Self::BadOffset => f.write_str("bone has bad offset"),
            Self::TooManyComponents => f.write_str("has too many animated components"),
            Self::InvalidPoseIndex => f.write_str("invalid pose index"),
            Self::IncorrectBoneCount => f.write_str("has incorrect bone count"),
            Self::BoneRenamed => f.write_str("bone was renamed"),
            Self::WrongParent => f.write_str("bone has wrong parent"),
        }
    }
}

impl core::error::Error for Md5Error {}

/// Splits a file into tokens as QSS's `COM_Parse` does (MD5-1.1 to MD5-1.5).
#[derive(Clone, Debug)]
pub struct Tokens<'a> {
    bytes: &'a [u8],
    pos: usize,
}

/// Whether `COM_Parse` skips `byte`: 0x01 to 0x20, and 0x80 to 0xFF, which are negative as
/// x86's signed `char` (MD5-1.2).
fn is_white(byte: u8) -> bool {
    byte <= b' ' || byte >= 0x80
}

/// The length of the text a file holds: its bytes up to its first NUL (MD5-1.1).
fn text_len(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len())
}

/// The bytes that are tokens by themselves (MD5-1.5).
fn is_single(byte: u8) -> bool {
    matches!(byte, b'{' | b'}' | b'(' | b')' | b'\'' | b':')
}

impl<'a> Tokens<'a> {
    /// Tokens of `bytes` up to its first NUL (MD5-1.1).
    #[must_use]
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes: bytes.get(..text_len(bytes)).unwrap_or_default(),
            pos: 0,
        }
    }

    fn at(&self, pos: usize) -> Option<u8> {
        self.bytes.get(pos).copied()
    }

    /// Skips whitespace and comments (MD5-1.2, MD5-1.3).
    fn skip(&mut self) {
        loop {
            while self.at(self.pos).is_some_and(is_white) {
                self.pos = self.pos.saturating_add(1);
            }
            let next = self.at(self.pos.saturating_add(1));
            match (self.at(self.pos), next) {
                (Some(b'/'), Some(b'/')) => {
                    while self.at(self.pos).is_some_and(|byte| byte != b'\n') {
                        self.pos = self.pos.saturating_add(1);
                    }
                }
                (Some(b'/'), Some(b'*')) => {
                    self.pos = self.pos.saturating_add(2);
                    loop {
                        match (self.at(self.pos), self.at(self.pos.saturating_add(1))) {
                            (None, _) => break,
                            (Some(b'*'), Some(b'/')) => {
                                self.pos = self.pos.saturating_add(2);
                                break;
                            }
                            _ => self.pos = self.pos.saturating_add(1),
                        }
                    }
                }
                _ => return,
            }
        }
    }
}

impl<'a> Iterator for Tokens<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        self.skip();
        let first = self.at(self.pos)?;
        let start = self.pos;
        if first == b'"' {
            let body = start.saturating_add(1);
            let rest = self.bytes.get(body..).unwrap_or_default();
            let len = rest.iter().position(|&byte| byte == b'"');
            let end = body.saturating_add(len.unwrap_or(rest.len()));
            self.pos = if len.is_some() {
                end.saturating_add(1)
            } else {
                end
            };
            return self.bytes.get(body..end);
        }
        if is_single(first) {
            self.pos = start.saturating_add(1);
            return self.bytes.get(start..self.pos);
        }
        self.pos = start.saturating_add(1);
        while let Some(byte) = self.at(self.pos) {
            if is_white(byte) || (is_single(byte) && byte != b':') {
                break;
            }
            self.pos = self.pos.saturating_add(1);
        }
        self.bytes.get(start..self.pos)
    }
}

/// QSS's parse state: the current token, as `com_token`, and the rest.
struct Parser<'a> {
    tokens: Tokens<'a>,
    token: Option<&'a [u8]>,
}

impl<'a> Parser<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        let mut tokens = Tokens::new(bytes);
        let token = tokens.next();
        Self { tokens, token }
    }

    fn advance(&mut self) {
        self.token = self.tokens.next();
    }

    /// Takes the current token, failing at the end of the file (MD5-1.9).
    fn take(&mut self) -> Result<&'a [u8], Md5Error> {
        let token = self.token.ok_or(Md5Error::UnexpectedEnd)?;
        self.advance();
        Ok(token)
    }

    /// Whether the current token is `keyword` (MD5-1.6); if so, moves past it.
    fn check(&mut self, keyword: &str) -> bool {
        if self.token == Some(keyword.as_bytes()) {
            self.advance();
            return true;
        }
        false
    }

    /// Requires `keyword` (MD5-1.6, MD5-1.9).
    fn expect(&mut self, keyword: &'static str) -> Result<(), Md5Error> {
        if self.check(keyword) {
            return Ok(());
        }
        Err(Md5Error::Expected { keyword })
    }

    /// An integer token (MD5-1.7): Rust's integer parser takes exactly an optional sign and
    /// one or more decimal digits.
    fn int(&mut self) -> Result<i128, Md5Error> {
        let token = self.take()?;
        core::str::from_utf8(token)
            .ok()
            .and_then(|text| text.parse().ok())
            .ok_or_else(|| Md5Error::BadNumber {
                token: token.to_vec(),
            })
    }

    /// An unsigned integer token: a count or an index (MD5-1.7).
    fn uint(&mut self) -> Result<u64, Md5Error> {
        let token = self.token.unwrap_or_default().to_vec();
        u64::try_from(self.int()?).map_err(|_| Md5Error::BadNumber { token })
    }

    /// A signed integer token: a parent joint (MD5-1.7).
    fn sint(&mut self) -> Result<i64, Md5Error> {
        let token = self.token.unwrap_or_default().to_vec();
        i64::try_from(self.int()?).map_err(|_| Md5Error::BadNumber { token })
    }

    /// A float token (MD5-1.8).
    fn float(&mut self) -> Result<f32, Md5Error> {
        let token = self.take()?;
        let value: f64 = core::str::from_utf8(token)
            .ok()
            .and_then(|text| text.parse().ok())
            .ok_or_else(|| Md5Error::BadNumber {
                token: token.to_vec(),
            })?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec MD5-1.8: a double stored in a float rounds to the nearest f32"
        )]
        Ok(value as f32)
    }

    /// `(`, three floats, `)`.
    fn vec3(&mut self) -> Result<[f32; 3], Md5Error> {
        self.expect("(")?;
        let value = [self.float()?, self.float()?, self.float()?];
        self.expect(")")?;
        Ok(value)
    }

    /// The optional `commandline` and its token (MD5-2.2, MD5-3.2).
    fn commandline(&mut self) -> Result<Option<Vec<u8>>, Md5Error> {
        if self.check("commandline") {
            return Ok(Some(self.take()?.to_vec()));
        }
        Ok(None)
    }
}

/// The w of a unit quaternion from its x, y and z (MD5-2.6): `-sqrt(1 - (x*x + y*y + z*z))`
/// in `f32`, with a negative value under the root taken as 0.
#[must_use]
pub fn md5_quat_w(xyz: [f32; 3]) -> f32 {
    let [x, y, z] = xyz;
    let rest = 1.0 - (x * x + y * y + z * z);
    -rest.max(0.0).sqrt()
}

/// A joint of the bind pose (MD5-2.4).
#[derive(Clone, Debug, PartialEq)]
pub struct Md5Joint {
    /// The joint's name.
    pub name: Vec<u8>,
    /// The parent joint's index, or [`MD5_NO_PARENT`] (MD5-2.5).
    pub parent: i64,
    /// The position.
    pub position: [f32; 3],
    /// The orientation as x, y, z and w, w computed by [`md5_quat_w`] (MD5-2.6).
    pub orientation: [f32; 4],
}

/// A vertex (MD5-2.9).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Md5Vert {
    /// Texture coordinates `s` and `t`.
    pub st: [f32; 2],
    /// The index of its first weight.
    pub first_weight: u64,
    /// Its number of weights.
    pub weight_count: u64,
}

/// A weight: a joint's share of a vertex (MD5-2.11).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Md5Weight {
    /// The joint's index, less than the number of joints.
    pub joint: u64,
    /// The share.
    pub bias: f32,
    /// The position in the joint's space.
    pub position: [f32; 3],
}

/// One mesh of a model (MD5-2.7).
///
/// Vertices, triangles and weights are kept as given: an index never given is all zero
/// (MD5-2.8), which the accessors return.
#[derive(Clone, Debug, PartialEq)]
pub struct Md5Surface {
    /// The shader token, which names the skin images (MD5-2.13).
    pub shader: Vec<u8>,
    /// `numverts`.
    pub num_verts: u64,
    /// `numtris`.
    pub num_tris: u64,
    /// `numweights`.
    pub num_weights: u64,
    verts: BTreeMap<u64, Md5Vert>,
    tris: BTreeMap<u64, [u64; 3]>,
    weights: BTreeMap<u64, Md5Weight>,
}

impl Md5Surface {
    /// Vertex `index`, zero if never given; `None` from `num_verts` on.
    #[must_use]
    pub fn vert(&self, index: u64) -> Option<Md5Vert> {
        (index < self.num_verts).then(|| self.verts.get(&index).copied().unwrap_or_default())
    }

    /// Triangle `index`'s vertices, zero if never given; `None` from `num_tris` on.
    #[must_use]
    pub fn tri(&self, index: u64) -> Option<[u64; 3]> {
        (index < self.num_tris).then(|| self.tris.get(&index).copied().unwrap_or_default())
    }

    /// Weight `index`, zero if never given; `None` from `num_weights` on.
    #[must_use]
    pub fn weight(&self, index: u64) -> Option<Md5Weight> {
        (index < self.num_weights).then(|| self.weights.get(&index).copied().unwrap_or_default())
    }
}

/// The image path of skin `skin` frame `frame` of a mesh with shader `shader`:
/// `progs/<shader>_<skin>_<frame>` with each number in two or more digits, without an
/// extension (MD5-2.13).
#[must_use]
pub fn md5_skin_path(shader: &[u8], skin: u32, frame: u32) -> Vec<u8> {
    let mut path = b"progs/".to_vec();
    path.extend_from_slice(shader);
    path.extend_from_slice(format!("_{skin:02}_{frame:02}").as_bytes());
    path
}

/// The path of a mesh's animation: the mesh's path with its extension replaced by
/// `.md5anim`, or `.md5anim` added when the last component has none (MD5-3.1).
#[must_use]
pub fn md5_anim_path(mesh: &str) -> String {
    let name = mesh.rfind('/').map_or(0, |slash| slash.saturating_add(1));
    let stem = match mesh.rfind('.') {
        Some(dot) if dot >= name => mesh.get(..dot).unwrap_or(mesh),
        _ => mesh,
    };
    format!("{stem}.md5anim")
}

/// A loaded `.md5mesh`: the bind-pose joints and the meshes.
#[derive(Clone, Debug, PartialEq)]
pub struct Md5Mesh {
    /// The `commandline` token, if any.
    pub commandline: Option<Vec<u8>>,
    /// `numJoints` joints, at least one.
    pub joints: Vec<Md5Joint>,
    /// `numMeshes` meshes, at least one.
    pub meshes: Vec<Md5Surface>,
}

impl Md5Mesh {
    /// Reads a mesh file held in memory, as QSS's `Mod_LoadMD5MeshModel` does.
    ///
    /// Implements MD5-1 and MD5-2.
    ///
    /// # Errors
    ///
    /// Any [`Md5Error`] of MD5-1 and MD5-2.
    pub fn read(bytes: &[u8]) -> Result<Self, Md5Error> {
        // One 32-bit comparison, which coverage-guided fuzzers can solve.
        let magic = bytes
            .get(..4)
            .and_then(|word| <[u8; 4]>::try_from(word).ok())
            .map(u32::from_le_bytes);
        if magic != Some(u32::from_le_bytes(MD5_MAGIC)) {
            return Err(Md5Error::NotMd5);
        }
        let mut parser = Parser::new(bytes);
        parser.expect("MD5Version")?;
        parser.expect(MD5_VERSION)?;
        let commandline = parser.commandline()?;
        parser.expect("numJoints")?;
        let num_joints = parser.uint()?;
        parser.expect("numMeshes")?;
        let num_meshes = parser.uint()?;
        if num_joints == 0 {
            return Err(Md5Error::NoBones);
        }
        if num_meshes == 0 {
            return Err(Md5Error::NoMeshes);
        }
        parser.expect("joints")?;
        parser.expect("{")?;
        let mut joints = Vec::new();
        for _ in 0..num_joints {
            let name = parser.take()?.to_vec();
            let parent = parser.sint()?;
            if parent < MD5_NO_PARENT || u64::try_from(parent).is_ok_and(|p| p >= num_joints) {
                return Err(Md5Error::BoneOutOfBounds);
            }
            let position = parser.vec3()?;
            let [x, y, z] = parser.vec3()?;
            joints.push(Md5Joint {
                name,
                parent,
                position,
                orientation: [x, y, z, md5_quat_w([x, y, z])],
            });
        }
        parser.expect("}")?;
        let mut meshes = Vec::new();
        for _ in 0..num_meshes {
            meshes.push(read_surface(&mut parser, num_joints)?);
        }
        Ok(Self {
            commandline,
            joints,
            meshes,
        })
    }
}

/// Reads one `mesh { ... }` block (MD5-2.7 to MD5-2.12).
fn read_surface(parser: &mut Parser<'_>, num_joints: u64) -> Result<Md5Surface, Md5Error> {
    parser.expect("mesh")?;
    parser.expect("{")?;
    parser.expect("shader")?;
    let shader = parser.take()?.to_vec();
    parser.expect("numverts")?;
    let num_verts = parser.uint()?;
    let mut verts = BTreeMap::new();
    while parser.check("vert") {
        let index = parser.uint()?;
        if index >= num_verts {
            return Err(Md5Error::VertexOutOfBounds);
        }
        parser.expect("(")?;
        let st = [parser.float()?, parser.float()?];
        parser.expect(")")?;
        let first_weight = parser.uint()?;
        let weight_count = parser.uint()?;
        verts.insert(
            index,
            Md5Vert {
                st,
                first_weight,
                weight_count,
            },
        );
    }
    parser.expect("numtris")?;
    let num_tris = parser.uint()?;
    let mut tris = BTreeMap::new();
    while parser.check("tri") {
        let index = parser.uint()?;
        if index >= num_tris {
            return Err(Md5Error::TriangleOutOfBounds);
        }
        let mut tri = [0; 3];
        for vertex in &mut tri {
            *vertex = parser.uint()?;
            if *vertex >= num_verts {
                return Err(Md5Error::VertexOutOfBounds);
            }
        }
        tris.insert(index, tri);
    }
    parser.expect("numweights")?;
    let num_weights = parser.uint()?;
    let mut weights = BTreeMap::new();
    while parser.check("weight") {
        let index = parser.uint()?;
        if index >= num_weights {
            return Err(Md5Error::WeightOutOfBounds);
        }
        let joint = parser.uint()?;
        if joint >= num_joints {
            return Err(Md5Error::BoneOutOfBounds);
        }
        let bias = parser.float()?;
        let position = parser.vec3()?;
        weights.insert(
            index,
            Md5Weight {
                joint,
                bias,
                position,
            },
        );
    }
    for vert in verts.values() {
        let end = vert.first_weight.checked_add(vert.weight_count);
        if end.is_none_or(|end| end > num_weights) {
            return Err(Md5Error::WeightOutOfBounds);
        }
    }
    parser.expect("}")?;
    Ok(Md5Surface {
        shader,
        num_verts,
        num_tris,
        num_weights,
        verts,
        tris,
        weights,
    })
}

/// A joint's entry in an animation's hierarchy (MD5-3.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Md5HierarchyEntry {
    /// The joint's name.
    pub name: Vec<u8>,
    /// The parent joint's index.
    pub parent: i64,
    /// The components it animates, bits 1 to 32 (MD5-3.4).
    pub flags: u64,
    /// The index of its first component.
    pub first_component: u64,
}

/// A joint's pose in one frame: position, and orientation as x, y, z and w (MD5-3.7).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Md5Pose {
    /// The position.
    pub position: [f32; 3],
    /// The orientation, w computed by [`md5_quat_w`].
    pub orientation: [f32; 4],
}

/// A loaded `.md5anim`.
#[derive(Clone, Debug, PartialEq)]
pub struct Md5Anim {
    /// The `commandline` token, if any.
    pub commandline: Option<Vec<u8>>,
    /// `numFrames`, at least 1.
    pub num_frames: u64,
    /// The token after `frameRate`, which QSS does not read (MD5-3.2).
    pub frame_rate: Vec<u8>,
    /// `numAnimatedComponents`.
    pub num_animated_components: u64,
    /// One entry per joint.
    pub hierarchy: Vec<Md5HierarchyEntry>,
    /// The `bounds` pairs, minimum then maximum, not used (MD5-3.5).
    pub bounds: Vec<[[f32; 3]; 2]>,
    /// The `baseframe` pairs, position then quaternion x, y, z, not used (MD5-3.5).
    pub baseframe: Vec<[[f32; 3]; 2]>,
    frames: BTreeMap<u64, Vec<f32>>,
}

impl Md5Anim {
    /// Reads an animation file held in memory, as QSS's `MD5Anim_Begin` and `MD5Anim_Load`
    /// do, but on its own: [`Md5Anim::check_mesh`] makes the checks that need the mesh.
    ///
    /// Implements MD5-1, MD5-3.2 to MD5-3.8 and MD5-3.11.
    ///
    /// # Errors
    ///
    /// Any [`Md5Error`] of MD5-1 and MD5-3.
    pub fn read(bytes: &[u8]) -> Result<Self, Md5Error> {
        let mut parser = Parser::new(bytes);
        parser.expect("MD5Version")?;
        parser.expect(MD5_VERSION)?;
        let commandline = parser.commandline()?;
        parser.expect("numFrames")?;
        let num_frames = parser.uint()?;
        parser.expect("numJoints")?;
        let num_joints = parser.uint()?;
        parser.expect("frameRate")?;
        let frame_rate = parser.take()?.to_vec();
        if num_frames == 0 {
            return Err(Md5Error::NoPoses);
        }
        parser.expect("numAnimatedComponents")?;
        let num_animated_components = parser.uint()?;
        // MD5-3.11: no more components than the text has bytes, so a frame never given
        // (MD5-3.6) is bounded by the input.
        let limit = u64::try_from(text_len(bytes)).unwrap_or(u64::MAX);
        if num_animated_components > limit {
            return Err(Md5Error::TooManyComponents);
        }
        parser.expect("hierarchy")?;
        parser.expect("{")?;
        let mut hierarchy = Vec::new();
        for _ in 0..num_joints {
            let name = parser.take()?.to_vec();
            let parent = parser.sint()?;
            let flags = parser.uint()?;
            if flags & !MD5_ANIM_FLAGS != 0 {
                return Err(Md5Error::UnsupportedFlags);
            }
            let first_component = parser.uint()?;
            let end = first_component.checked_add(u64::from(flags.count_ones()));
            if end.is_none_or(|end| end > num_animated_components) {
                return Err(Md5Error::BadOffset);
            }
            hierarchy.push(Md5HierarchyEntry {
                name,
                parent,
                flags,
                first_component,
            });
        }
        parser.expect("}")?;
        let bounds = read_pairs(&mut parser, "bounds")?;
        let baseframe = read_pairs(&mut parser, "baseframe")?;
        let mut frames = BTreeMap::new();
        while parser.check("frame") {
            let index = parser.uint()?;
            if index >= num_frames {
                return Err(Md5Error::InvalidPoseIndex);
            }
            parser.expect("{")?;
            let mut components = Vec::new();
            for _ in 0..num_animated_components {
                components.push(parser.float()?);
            }
            parser.expect("}")?;
            frames.insert(index, components);
        }
        Ok(Self {
            commandline,
            num_frames,
            frame_rate,
            num_animated_components,
            hierarchy,
            bounds,
            baseframe,
            frames,
        })
    }

    /// The components of frame `index`, all zero if never given; `None` from
    /// `num_frames` on (MD5-3.6). Either way it holds `num_animated_components` values, which
    /// [`Md5Anim::read`] bounded by the file's length (MD5-3.11).
    #[must_use]
    pub fn frame(&self, index: u64) -> Option<Vec<f32>> {
        if index >= self.num_frames {
            return None;
        }
        Some(self.frames.get(&index).cloned().unwrap_or_else(|| {
            let len = usize::try_from(self.num_animated_components).unwrap_or_default();
            vec![0.0; len]
        }))
    }

    /// Joint `joint`'s pose in frame `frame` (MD5-3.4, MD5-3.7): the components its flags
    /// select, 0 for the rest. `None` when either index is out of range.
    #[must_use]
    pub fn pose(&self, frame: u64, joint: usize) -> Option<Md5Pose> {
        let entry = self.hierarchy.get(joint)?;
        if frame >= self.num_frames {
            return None;
        }
        let components = self.frames.get(&frame);
        let mut next = usize::try_from(entry.first_component).ok()?;
        let mut values = [0.0_f32; 6];
        for (bit, value) in [1, 2, 4, 8, 16, 32].into_iter().zip(&mut values) {
            if entry.flags & bit != 0 {
                *value = components
                    .and_then(|components| components.get(next).copied())
                    .unwrap_or_default();
                next = next.saturating_add(1);
            }
        }
        let [x, y, z, qx, qy, qz] = values;
        Some(Md5Pose {
            position: [x, y, z],
            orientation: [qx, qy, qz, md5_quat_w([qx, qy, qz])],
        })
    }

    /// Checks that the animation goes with `mesh`, as QSS's `MD5Anim_Load` does.
    ///
    /// Implements MD5-3.9.
    ///
    /// # Errors
    ///
    /// [`Md5Error::IncorrectBoneCount`], [`Md5Error::BoneRenamed`] or
    /// [`Md5Error::WrongParent`].
    pub fn check_mesh(&self, mesh: &Md5Mesh) -> Result<(), Md5Error> {
        if self.hierarchy.len() != mesh.joints.len() {
            return Err(Md5Error::IncorrectBoneCount);
        }
        for (entry, joint) in self.hierarchy.iter().zip(&mesh.joints) {
            if entry.name != joint.name {
                return Err(Md5Error::BoneRenamed);
            }
            if entry.parent != joint.parent {
                return Err(Md5Error::WrongParent);
            }
        }
        Ok(())
    }
}

/// `keyword`, `{`, any number of `( x y z ) ( x y z )` pairs, `}` (MD5-3.5).
fn read_pairs(
    parser: &mut Parser<'_>,
    keyword: &'static str,
) -> Result<Vec<[[f32; 3]; 2]>, Md5Error> {
    parser.expect(keyword)?;
    parser.expect("{")?;
    let mut pairs = Vec::new();
    while parser.token == Some(b"(".as_slice()) {
        pairs.push([parser.vec3()?, parser.vec3()?]);
    }
    parser.expect("}")?;
    Ok(pairs)
}
