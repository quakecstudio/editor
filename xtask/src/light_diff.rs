// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask light-diff [--maps <dir>] [--map <name>]...`: the light compiler differential
//! (P7-01, MAP-10, MAP-21). Each map is compiled once, by id's qbsp when it reads the map and
//! else by ericw-tools' qbsp, and that BSP lit by id's light and by qs-map-light's id dialect
//! in both modes, fast and `-extra` (LIGHT-1.3). Both light the same faces, so their lightmaps
//! are compared luxel by luxel, as images, each style's block with the same style's. id's light
//! has no colour, so a coloured map is compared lit white.
//!
//! The extended dialect is compared with ericw-tools' light (LIGHT-8 to LIGHT-20): the map is
//! compiled by ericw-tools' qbsp with a WAD of blank textures for the names its faces have,
//! which light tells sky, liquids, fences and surface lights by, and lit by both with what that
//! qbsp wrote beside the BSP; the grey lumps and the `.lit` files are compared, a lightmap only
//! one has against darkness.
//!
//! The maps are `map-diff`'s: the fixtures and the corpus's (MAP-19), with the coloured room. The
//! oracles are built as `cargo xtask oracles maps` builds them.

use std::fs;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use qs_bsp::{Bsp, BspLoader, Face};
use qs_map::{Entity, MapFile, qbsp};
use qs_map_light::{
    CONTENT_EXTENSION, Dialect, Extras, Jitter, Light, LightError, Sampling, Sidecars,
    TEXINFO_EXTENSION,
};
use qs_wad::WAD_TYP_MIPTEX;
use qs_wad::write::WadBuilder;

use crate::download::Net;
use crate::exec::Exec;
use crate::map_diff::{corpus_maps, map_name, oracle_compile, verdict, without_wads};
use crate::oracles::OUT;
use crate::oracles::maps::{MapTools, WORK, build};

/// `cargo xtask light-diff`.
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// The folder of maps; when not given, the corpus's maps (MAP-19).
    #[arg(long)]
    maps: Option<PathBuf>,
    /// Only the maps of these names, without `.map`; every one when none is given.
    #[arg(long = "map")]
    only: Vec<String>,
}

/// The `.lit` file's header, `QLIT` and version 1, before its samples (BSP-18).
const LIT_HEADER: usize = 8;
/// The fixture whose colour is compared with ericw-tools': coloured lights of the keys id's
/// light reads and `_color` only.
const COLOURED_ROOM: &str = "oracles/maps/coloured.map";
/// A palette whose every colour is black: all the blank textures' texels show of Quake's.
const BLACK_PALETTE: [u8; 768] = [0; 768];
/// A blank texture of the WAD written for a map: its name's bytes, its side, its mip levels
/// and the bytes of its header (BSP-4).
const STUB_NAME: usize = 16;
const STUB_SIDE: u32 = 16;
const STUB_MIPS: u32 = 4;
const STUB_HEADER: u32 = 40;

/// A WAD holding a blank texture of each name the faces of `file` have: light tells sky,
/// liquid, fence and the textures of surface lights by their names (LIGHT-11.2, LIGHT-16.1),
/// which a BSP compiled with no WAD does not have.
pub(crate) fn stub_wad(file: &MapFile) -> Result<Vec<u8>, String> {
    let mut names: Vec<Vec<u8>> = file
        .entities
        .iter()
        .flat_map(Entity::brushes)
        .flat_map(|brush| &brush.faces)
        .map(|face| face.texture().to_ascii_lowercase())
        .collect();
    // An animation's frames are a run from its first: the game refuses a BSP that has a
    // later frame without those before it (BSP-4.8), and a map may name only one.
    let earlier: Vec<Vec<u8>> = names
        .iter()
        .filter_map(|texture| match texture.as_slice() {
            [b'+', frame, rest @ ..] => Some((*frame, rest)),
            _ => None,
        })
        .flat_map(|(frame, rest)| {
            let first = if frame.is_ascii_digit() { b'0' } else { b'a' };
            (first..frame).map(move |before| {
                let mut name = vec![b'+', before];
                name.extend_from_slice(rest);
                name
            })
        })
        .collect();
    names.extend(earlier);
    names.sort_unstable();
    names.dedup();
    let mut wad = WadBuilder::new();
    for texture in &names {
        // A name longer than a WAD holds is no texture of a WAD: qbsp finds none.
        if texture.len() >= STUB_NAME {
            continue;
        }
        let mut lump = texture.clone();
        lump.resize(STUB_NAME, 0);
        let mut at = STUB_HEADER;
        for size in [STUB_SIDE, STUB_SIDE] {
            lump.extend_from_slice(&size.to_le_bytes());
        }
        for level in 0..STUB_MIPS {
            lump.extend_from_slice(&at.to_le_bytes());
            at = at.saturating_add((STUB_SIDE >> level).saturating_mul(STUB_SIDE >> level));
        }
        lump.resize(usize::try_from(at).unwrap_or(0), 0);
        wad.add(texture, WAD_TYP_MIPTEX, lump)
            .map_err(|e| e.to_string())?;
    }
    wad.to_bytes().map_err(|e| e.to_string())
}
/// The classname of an instance of an external map (MAP-14).
const EXTERNAL_MAP: &str = "misc_external_map";
/// The channels of a `.lit` sample.
const CHANNELS: usize = 3;
/// A style slot of a face that holds no lightmap (BSP-8).
const NO_STYLE: u8 = 255;
/// The longest an oracle may take to light one map.
const ORACLE_LIMIT: Duration = Duration::from_secs(600);
/// How far a luxel may be from the oracle's and count as the same.
const LUXEL_TOLERANCE: u8 = 2;
/// The bar against an oracle: no luxel, or channel, more than [`LUXEL_TOLERANCE`] from it, and on
/// average within a twentieth of a level. Every corpus map measured 0 against id's light in both
/// modes, and the coloured room 0.009 against ericw-tools' in the fast one (P7-01).
const MEAN_BAR: f64 = 0.05;

/// The bar against ericw-tools' light, which asks Embree what each ray meets: a ray that
/// touches the edge of a triangle, or runs along a plane of the grid, is answered by Embree's
/// own tree of boxes and arithmetic, which ours follows only by rule (LIGHT-11.9), and one
/// such ray moves a luxel by a whole light's worth. So the lightmaps are held to it on average,
/// within a level, and in the count of luxels past [`LUXEL_TOLERANCE`], [`ERICW_PAST_BAR`] in
/// a hundred. Measured on the corpus on 2026-11-17: means of 0.01 to 0.60, and 0.03 to 2.0 in
/// a hundred past.
const ERICW_MEAN_BAR: f64 = 1.0;
const ERICW_PAST_BAR: f64 = 3.0;
/// A hundred, of a part in a hundred.
const HUNDRED: f64 = 100.0;

/// A light mode, with the option that asks the oracles for it.
#[derive(Clone, Copy, Debug)]
enum Mode {
    /// One sample a luxel, light's default.
    Fast,
    /// Four samples a luxel averaged, `-extra`.
    Extra,
}

impl Mode {
    const ALL: [Self; 2] = [Self::Fast, Self::Extra];

    const fn sampling(self) -> Sampling {
        match self {
            Self::Fast => Sampling::Fast,
            Self::Extra => Sampling::Extra,
        }
    }

    const fn options(self) -> &'static [&'static str] {
        match self {
            Self::Fast => &[],
            Self::Extra => &["-extra"],
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Extra => "extra",
        }
    }
}

/// Which samples two lit BSPs are compared by.
#[derive(Clone, Copy, Debug)]
enum Samples<'a> {
    /// The lighting lumps' grey samples.
    Grey,
    /// The `.lit` files' channels, ours and the oracle's.
    Colour(&'a [u8], &'a [u8]),
}

/// Two images of the same luxels compared.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Images {
    /// The luxels, or channels, compared.
    pub(crate) compared: usize,
    /// Their differences added.
    pub(crate) total: u64,
    /// The largest difference.
    pub(crate) largest: u8,
    /// Those differing by more than [`LUXEL_TOLERANCE`].
    pub(crate) over: usize,
    /// The faces lit in only one, with another lightmap size, or a style only one has.
    pub(crate) faces_differing: usize,
}

impl Images {
    /// Adds the luxels `a` and `b`, the same luxels of each image.
    fn add(&mut self, a: &[u8], b: &[u8]) {
        for (&x, &y) in a.iter().zip(b) {
            let d = x.abs_diff(y);
            self.compared = self.compared.saturating_add(1);
            self.total = self.total.saturating_add(u64::from(d));
            self.largest = self.largest.max(d);
            if d > LUXEL_TOLERANCE {
                self.over = self.over.saturating_add(1);
            }
        }
    }

    /// Adds the luxels `lit` of a face only one image has, against the darkness the other has
    /// there.
    fn add_against_dark(&mut self, lit: &[u8]) {
        for &x in lit {
            self.compared = self.compared.saturating_add(1);
            self.total = self.total.saturating_add(u64::from(x));
            self.largest = self.largest.max(x);
            if x > LUXEL_TOLERANCE {
                self.over = self.over.saturating_add(1);
            }
        }
    }

    /// A face that is not the same in both.
    const fn differ(&mut self) {
        self.faces_differing = self.faces_differing.saturating_add(1);
    }

    /// Whether the images agree: every face alike, no luxel past [`LUXEL_TOLERANCE`], and the
    /// mean within [`MEAN_BAR`].
    fn agree(&self) -> bool {
        self.faces_differing == 0 && self.over == 0 && self.mean() <= MEAN_BAR
    }

    /// Whether the images are as near as ericw-tools' light is held to: every face alike, the
    /// mean within [`ERICW_MEAN_BAR`], and no more than [`ERICW_PAST_BAR`] luxels in a hundred
    /// past [`LUXEL_TOLERANCE`].
    fn near(&self) -> bool {
        self.faces_differing == 0
            && self.mean() <= ERICW_MEAN_BAR
            && self.past_in_a_hundred() <= ERICW_PAST_BAR
    }

    /// How many luxels in a hundred are past [`LUXEL_TOLERANCE`].
    fn past_in_a_hundred(&self) -> f64 {
        let compared = u32::try_from(self.compared).unwrap_or(u32::MAX);
        let over = u32::try_from(self.over).unwrap_or(u32::MAX);
        if compared == 0 {
            0.0
        } else {
            f64::from(over) * HUNDRED / f64::from(compared)
        }
    }

    /// The mean difference.
    fn mean(&self) -> f64 {
        let compared = u32::try_from(self.compared).unwrap_or(u32::MAX);
        let total = u32::try_from(self.total).unwrap_or(u32::MAX);
        if compared == 0 {
            0.0
        } else {
            f64::from(total) / f64::from(compared)
        }
    }
}

impl std::fmt::Display for Images {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} compared, mean {:.3}, largest {}, {} past {LUXEL_TOLERANCE}, {} faces differ",
            self.compared,
            self.mean(),
            self.largest,
            self.over,
            self.faces_differing
        )
    }
}

/// A face's styles, in its slots, up to the first empty one.
fn styles(face: &Face) -> Vec<u8> {
    face.styles
        .iter()
        .copied()
        .take_while(|&style| style != NO_STYLE)
        .collect()
}

/// What a lightmap only one of two lit BSPs has counts as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OnlyOne {
    /// A difference of the face: id's light and ours light the same faces in the same styles.
    Differs,
    /// A lightmap against darkness: ericw-tools keeps a lightmap whose brightest sample is 1
    /// or more (LIGHT-18.2), so a sample a hair either side of 1 gives or takes one.
    Dark,
}

/// The lightmaps of the lit BSPs `ours` and `theirs`, made from one BSP, compared face by face
/// and each style's block with the oracle's block of the same style, by `samples`.
fn compare(
    ours: &[u8],
    theirs: &[u8],
    samples: Samples<'_>,
    only_one: OnlyOne,
) -> Result<Images, String> {
    let a = Bsp::read(ours, BspLoader::FitzQuake).map_err(|e| format!("our BSP: {e}"))?;
    let b = Bsp::read(theirs, BspLoader::FitzQuake).map_err(|e| format!("the oracle's: {e}"))?;
    if a.faces.len() != b.faces.len() {
        return Err(format!("{} faces against {}", a.faces.len(), b.faces.len()));
    }
    let (data_a, data_b, scale) = match samples {
        Samples::Colour(lit_a, lit_b) => (
            lit_a.get(LIT_HEADER..).unwrap_or_default(),
            lit_b.get(LIT_HEADER..).unwrap_or_default(),
            CHANNELS,
        ),
        Samples::Grey => (a.lighting, b.lighting, 1),
    };
    let mut images = Images::default();
    for (fa, fb) in a.faces.iter().zip(&b.faces) {
        let (lit_a, lit_b) = (usize::try_from(fa.lightofs), usize::try_from(fb.lightofs));
        if lit_a.is_err() && lit_b.is_err() {
            continue;
        }
        let (styles_a, styles_b) = (styles(fa), styles(fb));
        let mut sorted = (styles_a.clone(), styles_b.clone());
        sorted.0.sort_unstable();
        sorted.1.sort_unstable();
        let alike = lit_a.is_ok() && lit_b.is_ok() && sorted.0 == sorted.1;
        if fa.lightmap_size != fb.lightmap_size || (only_one == OnlyOne::Differs && !alike) {
            images.differ();
            continue;
        }
        let block = fa.lightmap_size[0]
            .saturating_mul(fa.lightmap_size[1])
            .saturating_mul(scale);
        let of = |data: &'_ [u8], lit: Result<usize, _>, styles: &[u8], style: u8| {
            let at = lit.ok()?;
            let slot = styles.iter().position(|s| *s == style)?;
            lightmap(data, (at, slot), (scale, block)).map(<[u8]>::to_vec)
        };
        let mut all = styles_a.clone();
        all.extend(styles_b.iter().filter(|style| !styles_a.contains(style)));
        for style in all {
            let ours = of(data_a, lit_a, &styles_a, style);
            let theirs = of(data_b, lit_b, &styles_b, style);
            match (ours, theirs) {
                (Some(x), Some(y)) => images.add(&x, &y),
                (Some(lit), None) | (None, Some(lit)) => images.add_against_dark(&lit),
                (None, None) => images.differ(),
            }
        }
    }
    Ok(images)
}

/// The lightmap in slot `slot` of the face whose lightmaps start at sample `at` of `data`, each
/// sample `scale` bytes and each lightmap `block` bytes.
fn lightmap(
    data: &[u8],
    (at, slot): (usize, usize),
    (scale, block): (usize, usize),
) -> Option<&[u8]> {
    let start = at
        .saturating_mul(scale)
        .saturating_add(slot.saturating_mul(block));
    data.get(start..start.saturating_add(block))
}

/// The first of id's fixed tables (BSP-16) the BSP `bsp` is past, with its count and the
/// table's size: `light`'s `LoadBSPFile` copies each lump into its table with no check, so past
/// one it writes over what follows and its result, or its stop, is undefined.
fn past_id_tables(bsp: &[u8]) -> Result<Option<(&'static str, usize, usize)>, String> {
    use qs_bsp::limits::{
        BSP_MAX_MAP_EDGES, BSP_MAX_MAP_FACES, BSP_MAX_MAP_MODELS, BSP_MAX_MAP_PLANES,
        BSP_MAX_MAP_SURFEDGES, BSP_MAX_MAP_TEXINFO, BSP_MAX_MAP_VERTS,
    };
    let read = Bsp::read(bsp, BspLoader::FitzQuake).map_err(|e| format!("the BSP: {e}"))?;
    let tables = [
        ("MAX_MAP_MODELS", read.models.len(), BSP_MAX_MAP_MODELS),
        ("MAX_MAP_PLANES", read.planes.len(), BSP_MAX_MAP_PLANES),
        ("MAX_MAP_VERTS", read.vertexes.len(), BSP_MAX_MAP_VERTS),
        ("MAX_MAP_TEXINFO", read.texinfo.len(), BSP_MAX_MAP_TEXINFO),
        ("MAX_MAP_FACES", read.faces.len(), BSP_MAX_MAP_FACES),
        ("MAX_MAP_EDGES", read.edges.len(), BSP_MAX_MAP_EDGES),
        (
            "MAX_MAP_SURFEDGES",
            read.surfedges.len(),
            BSP_MAX_MAP_SURFEDGES,
        ),
    ];
    Ok(tables.into_iter().find(|&(_, count, size)| count > size))
}

/// `bsp` with each `"_color"` key renamed to `"_colox"`, of the same length, which no light
/// reads: the map lit white, as id's light, which has no colour, lights it (LIGHT-6.5).
fn white(bsp: &[u8]) -> Vec<u8> {
    const KEY: &[u8] = b"\"_color\"";
    let mut out = bsp.to_vec();
    let mut at = 0;
    while let Some(found) = out
        .get(at..)
        .and_then(|rest| rest.windows(KEY.len()).position(|window| window == KEY))
    {
        let start = at.saturating_add(found);
        if let Some(letter) = out.get_mut(start.saturating_add(KEY.len()).saturating_sub(2)) {
            *letter = b'x';
        }
        at = start.saturating_add(KEY.len());
    }
    out
}

/// What an oracle's light wrote: the lit BSP and its `.lit`, when it wrote one.
struct OracleLit {
    bsp: Vec<u8>,
    lit: Option<Vec<u8>>,
}

/// How an oracle's light stopped.
#[derive(Clone, Debug)]
struct OracleStop {
    /// Whether a signal ended it, as when it writes past one of its fixed tables.
    crashed: bool,
    /// What it said, its last line or its status.
    said: String,
}

impl std::fmt::Display for OracleStop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.said)
    }
}

/// Whether our light stopping with `ours` is alike with the oracle's `theirs`: it names the same
/// error, or ours stops at one of light's fixed tables where light, writing past it, crashes.
fn stop_alike(ours: &LightError, theirs: &OracleStop) -> bool {
    let fixed_table = matches!(
        ours,
        LightError::TooManyEntities
            | LightError::Overrun
            | LightError::EntityTextTooLong
            | LightError::PairTooLong { .. }
    );
    theirs.said.contains(&ours.to_string()) || (fixed_table && theirs.crashed)
}

/// `bsp` lit in `dir` by the oracle `light` with `options`, as `<name>.bsp`, within
/// [`ORACLE_LIMIT`]; the lit BSP and its `.lit` when the oracle wrote one, or how it stopped.
fn oracle_light(
    exec: &dyn Exec,
    light: &Path,
    options: &[&str],
    bsp: &[u8],
    (dir, name): (&Path, &str),
) -> Result<Result<OracleLit, OracleStop>, String> {
    let file = dir.join(format!("{name}.bsp"));
    fs::write(&file, bsp).map_err(|e| format!("{}: {e}", file.display()))?;
    let lit_file = file.with_extension("lit");
    if lit_file.exists() {
        fs::remove_file(&lit_file).map_err(|e| format!("{}: {e}", lit_file.display()))?;
    }
    let mut command = Command::new(light);
    command.args(options).arg(&file).current_dir(dir);
    let output = exec
        .output_within(&mut command, ORACLE_LIMIT)
        .map_err(|e| format!("cannot run {}: {e}", light.display()))?
        .ok_or_else(|| format!("{} took over {ORACLE_LIMIT:?}", light.display()))?;
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let last = stdout.lines().rev().find(|line| !line.trim().is_empty());
        return Ok(Err(OracleStop {
            crashed: output.status.code().is_none(),
            said: format!("{}: {}", output.status, last.unwrap_or("no message")),
        }));
    }
    let lit = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(Ok(OracleLit {
        bsp: lit,
        lit: fs::read(&lit_file).ok(),
    }))
}

/// The oracles and where they light.
struct Oracles<'a> {
    exec: &'a dyn Exec,
    tools: &'a MapTools,
    work: PathBuf,
}

impl Oracles<'_> {
    /// The map at `path` compared: its white light with id's in both modes, and its colour with
    /// ericw-tools' in the fast mode; whether it agrees.
    fn map(&self, path: &Path) -> Result<bool, String> {
        let name = map_name(path);
        let text = without_wads(&fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let words = String::from_utf8_lossy(&text).into_owned();
        if words.contains(EXTERNAL_MAP) {
            println!("skip  {name}: its instances are compiled by map-diff");
            return Ok(true);
        }
        let [id_qbsp, _, _] = &self.tools.id;
        let [ericw_qbsp, _, _] = &self.tools.ericw;
        // id's light reads any BSP29, whichever qbsp wrote it.
        let qbsp = if qbsp::load_map_file(&text).is_ok() {
            id_qbsp
        } else {
            ericw_qbsp
        };
        let dir = self.work.join(&name);
        let mut passed = true;
        match oracle_compile(self.exec, qbsp, path, &text, &[], &dir)? {
            Ok(compiled) => {
                let past = past_id_tables(&compiled.bsp)?;
                for mode in Mode::ALL {
                    let label = format!("{name} {}", mode.name());
                    match past {
                        Some((table, count, size)) => println!(
                            "skip  {label} light: {count} past {table}, {size}, which id's light \
                             overruns"
                        ),
                        None => passed &= self.white(&compiled.bsp, mode, (&dir, &label))?,
                    }
                }
            }
            Err(why) => println!("skip  {name}: {} stops: {why}", qbsp.display()),
        }
        passed &= self.ericw(path, &text, (&dir, &name))?;
        Ok(passed)
    }

    /// `bsp` lit white by ours, in id's dialect, and by id's light in `mode`, compared; whether
    /// they agree or stop alike.
    fn white(&self, bsp: &[u8], mode: Mode, (dir, label): (&Path, &str)) -> Result<bool, String> {
        let [_, _, id_light] = &self.tools.id;
        // id's light has no colour: our white light of the map is what it is compared with.
        let ours = Light::new(mode.sampling())
            .with_dialect(Dialect::Id)
            .run(&white(bsp));
        let name = format!("id-{}", mode.name());
        let theirs = oracle_light(self.exec, id_light, mode.options(), bsp, (dir, &name))?;
        let (agrees, said) = match (ours, theirs) {
            (Ok(ours), Ok(OracleLit { bsp: theirs, .. })) => {
                let file = dir.join(format!("ours-{}.bsp", mode.name()));
                fs::write(&file, &ours.bsp).map_err(|e| format!("{}: {e}", file.display()))?;
                match compare(&ours.bsp, &theirs, Samples::Grey, OnlyOne::Differs) {
                    Ok(images) => (images.agree(), format!("light: {images}")),
                    Err(why) => (false, format!("light: {why}")),
                }
            }
            (Err(ours), Err(theirs)) => (
                stop_alike(&ours, &theirs),
                format!("both stop: {ours}; {theirs}"),
            ),
            (Err(ours), Ok(_)) => (false, format!("only ours stops: {ours}")),
            (Ok(_), Err(theirs)) => (false, format!("only id's light stops: {theirs}")),
        };
        println!("{} {label} {said}", verdict(agrees));
        Ok(agrees)
    }

    /// The map `text` with its world's `wad` a WAD written for it ([`stub_wad`]). The
    /// corpus's maps come without their WADs, so the textures are blank.
    fn with_textures(&self, text: &[u8], name: &str) -> Result<Vec<u8>, String> {
        let mut file = MapFile::parse(text).map_err(|e| format!("{name}: {e}"))?;
        let bytes = stub_wad(&file).map_err(|e| format!("{name}: {e}"))?;
        let dir = self.work.join("textures");
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(format!("{name}.wad"));
        fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        if let Some(world) = file.entities.first_mut() {
            world
                .set(b"wad", path.to_string_lossy().as_bytes())
                .map_err(|e| format!("{name}: {e}"))?;
        }
        Ok(file.to_bytes())
    }

    /// The map compiled by ericw-tools' qbsp and lit by ours, in the extended dialect, and by
    /// ericw-tools' light, both with one sample a luxel: the grey lumps compared, and the `.lit`
    /// files when both wrote one; whether they agree.
    fn ericw(&self, path: &Path, text: &[u8], (dir, name): (&Path, &str)) -> Result<bool, String> {
        let [ericw_qbsp, _, ericw_light] = &self.tools.ericw;
        let dir = dir.join("ericw");
        let label = format!("{name} ericw");
        let text = &self.with_textures(text, name)?;
        let compiled = match oracle_compile(self.exec, ericw_qbsp, path, text, &[], &dir)? {
            Ok(compiled) => compiled,
            Err(why) => {
                println!("skip  {label}: ericw-tools' qbsp stops: {why}");
                return Ok(true);
            }
        };
        // What ericw-tools' qbsp wrote beside the BSP for its light (LIGHT-9.8, LIGHT-9.9).
        let beside = |extension: &str| fs::read(dir.join(format!("{name}.{extension}"))).ok();
        let (texinfo, content) = (beside(TEXINFO_EXTENSION), beside(CONTENT_EXTENSION));
        let sidecars = Sidecars::read(texinfo.as_deref(), content.as_deref())
            .map_err(|file| format!("{label}: {}", LightError::Sidecar(file)))?;
        // ericw-tools scatters by its C library's rand (LIGHT-14.5).
        let jitter = if cfg!(target_os = "macos") {
            Jitter::Bsd
        } else {
            Jitter::Glibc
        };
        // The blank textures' texels are index 0, black in Quake's palette, which
        // ericw-tools has built in (LIGHT-20.6).
        let extras = Extras {
            palette: Some(&BLACK_PALETTE),
            ..sidecars.extras()
        };
        let ours = match Light::new(Sampling::Fast)
            .with_jitter(jitter)
            .run_with(&compiled.bsp, extras)
        {
            Ok(ours) => ours,
            Err(error) => {
                println!("FAIL  {label}: ours stops: {error}");
                return Ok(false);
            }
        };
        let file = dir.join("ours.bsp");
        fs::write(&file, &ours.bsp).map_err(|e| format!("{}: {e}", file.display()))?;
        // Lit where it was compiled, beside the `.texinfo.json` ericw-tools' qbsp wrote for it.
        let theirs = match oracle_light(self.exec, ericw_light, &[], &compiled.bsp, (&dir, name))? {
            Ok(theirs) => theirs,
            Err(stop) => {
                println!("FAIL  {label}: ericw-tools' light stops: {stop}");
                return Ok(false);
            }
        };
        let mut agrees = match compare(&ours.bsp, &theirs.bsp, Samples::Grey, OnlyOne::Dark) {
            Ok(images) => {
                println!(
                    "{} {label} light: {images}, {:.2} in a hundred",
                    verdict(images.near()),
                    images.past_in_a_hundred()
                );
                images.near()
            }
            Err(why) => {
                println!("FAIL  {label} light: {why}");
                false
            }
        };
        match (&ours.lit, &theirs.lit) {
            (Some(our_lit), Some(their_lit)) => {
                let samples = Samples::Colour(our_lit, their_lit);
                match compare(&ours.bsp, &theirs.bsp, samples, OnlyOne::Dark) {
                    Ok(images) => {
                        println!(
                            "{} {label} colour: {images}, {:.2} in a hundred",
                            verdict(images.near()),
                            images.past_in_a_hundred()
                        );
                        agrees &= images.near();
                    }
                    Err(why) => {
                        println!("FAIL  {label} colour: {why}");
                        agrees = false;
                    }
                }
            }
            (None, None) => {}
            (ours, _) => {
                let only = if ours.is_some() {
                    "ours"
                } else {
                    "ericw-tools"
                };
                println!("FAIL  {label} colour: only {only} wrote a .lit");
                agrees = false;
            }
        }
        Ok(agrees)
    }
}

/// `cargo xtask light-diff`; whether every map's lighting agrees.
pub(crate) fn main(
    root: &Path,
    exec: &dyn Exec,
    net: &dyn Net,
    args: &Args,
) -> Result<bool, String> {
    let room = root.join(COLOURED_ROOM);
    let mut maps = vec![room.clone()];
    let room_only = args.only.len() == 1 && args.only.contains(&map_name(&room));
    if !room_only {
        maps.extend(corpus_maps(root, args.maps.as_deref(), &args.only)?);
    }
    if !args.only.is_empty() && !args.only.contains(&map_name(&room)) {
        maps.retain(|path| *path != room);
    }
    let tools = build(root, exec, net)?;
    let work = WORK
        .iter()
        .fold(root.join(OUT), |path, part| path.join(part))
        .join("light-diff");
    let oracles = Oracles {
        exec,
        tools: &tools,
        work,
    };
    let mut passed = true;
    for path in &maps {
        match oracles.map(path) {
            Ok(agrees) => passed &= agrees,
            Err(why) => {
                passed = false;
                println!("FAIL  {}: {why}", map_name(path));
            }
        }
    }
    Ok(passed)
}

#[cfg(test)]
mod tests {
    use qs_map_light::LightError;

    use super::{Images, LUXEL_TOLERANCE, OracleStop, stop_alike, white};

    #[test]
    fn white_renames_every_colour_key_and_keeps_the_length() {
        let text = b"{\n\"_color\" \"1 0 0\"\n}\n{\n\"_color\" \"0 1 0\"\n\"_colorx\" \"1\"\n}";
        let out = white(text);
        assert_eq!(out.len(), text.len());
        assert_eq!(
            out,
            b"{\n\"_colox\" \"1 0 0\"\n}\n{\n\"_colox\" \"0 1 0\"\n\"_colorx\" \"1\"\n}"
        );
        assert_eq!(white(b"no light"), b"no light");
    }

    #[test]
    fn luxels_past_the_tolerance_are_counted() {
        let mut images = Images::default();
        images.add(&[10, 10, 10, 10], &[10, 12, 13, 0]);
        assert_eq!(images.compared, 4);
        assert_eq!(images.largest, 10);
        assert_eq!(
            images.over, 2,
            "3 and 10 are past {LUXEL_TOLERANCE}, 2 is not"
        );
        assert_eq!(images.mean(), 15.0 / 4.0);
        assert!(!images.agree());
        let mut close = Images::default();
        close.add(&[10, 10], &[10, 10]);
        assert!(close.agree());
        close.differ();
        assert!(!close.agree(), "a face lit in only one disagrees");
        assert_eq!(Images::default().mean(), 0.0, "nothing compared");
    }

    #[test]
    fn ericw_tools_light_is_held_on_average_and_in_the_count_past_tolerance() {
        let mut images = Images::default();
        // Two luxels in a hundred a whole light off: within the bar, though not alike.
        images.add(&[10; 98], &[10; 98]);
        images.add(&[10, 10], &[40, 40]);
        assert!(!images.agree());
        assert!(images.near());
        assert_eq!(images.past_in_a_hundred(), 2.0);
        // Four in a hundred are past it.
        images.add(&[10, 10], &[40, 40]);
        assert!(!images.near());
        // A lightmap only one has is compared against darkness.
        let mut dark = Images::default();
        dark.add_against_dark(&[0, 1, 2, 9]);
        assert_eq!((dark.compared, dark.over, dark.largest), (4, 1, 9));
        let mut dim = Images::default();
        dim.add(&[10; 100], &[12; 100]);
        assert!(
            !dim.near(),
            "two levels off on average is past the mean's bar"
        );
        assert_eq!(Images::default().past_in_a_hundred(), 0.0);
    }

    #[test]
    fn stops_are_alike_by_the_error_or_a_crash_at_a_fixed_table() {
        let said = |text: &str, crashed| OracleStop {
            crashed,
            said: text.to_owned(),
        };
        let entities = LightError::TooManyEntities;
        let named = said(&format!("exit status: 1: {entities}"), false);
        assert!(stop_alike(&entities, &named));
        assert!(stop_alike(&entities, &said("signal: 10 (SIGBUS)", true)));
        assert!(!stop_alike(
            &entities,
            &said("exit status: 1: something else", false)
        ));
        let unclosed = LightError::Unclosed;
        assert!(
            !stop_alike(&unclosed, &said("signal: 10 (SIGBUS)", true)),
            "a crash is alike only with a stop at a fixed table"
        );
    }
}
