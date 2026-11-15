// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask light-diff [--maps <dir>] [--map <name>]...`: the light compiler differential
//! (P7-01, MAP-10). Each map is compiled once, by id's qbsp when it reads the map and else by
//! ericw-tools' qbsp, and that BSP lit by id's light and by qs-map-light in both modes, fast and
//! `-extra` (LIGHT-1.3). Both light the same faces, so their lightmaps are compared luxel by
//! luxel, as images, each style's block with the same style's. id's light has no colour, so a
//! coloured map is compared lit white; its colour (LIGHT-6) is compared with ericw-tools' light,
//! `.lit` channel by channel, on a map whose lights ericw-tools lights as ours does.
//!
//! The maps are `map-diff`'s: the fixtures and the corpus's (MAP-19), with the coloured room. The
//! oracles are built as `cargo xtask oracles maps` builds them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use qs_bsp::{Bsp, BspLoader, Face};
use qs_map::qbsp;
use qs_map_light::{Dialect, Light, LightError, Sampling};

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
/// The keys, or the starts of keys, of ericw-tools' own lighting, which ours does not have yet
/// (L-12): a map with any is lit otherwise by ericw-tools' light, so its colour is not compared.
const ERICW_ONLY_KEYS: [&str; 18] = [
    "_sun",
    "_surface",
    "delay",
    "_deviance",
    "_minlight",
    "_bounce",
    "_dirt",
    "_anglescale",
    "_softangle",
    "_range",
    "_dist",
    "_gamma",
    "_phong",
    "_shadow",
    "_falloff",
    "_project_",
    "_lightmap_scale",
    "_world_units_per_luxel",
];
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

    /// A face that is not the same in both.
    const fn differ(&mut self) {
        self.faces_differing = self.faces_differing.saturating_add(1);
    }

    /// Whether the images agree: every face alike, no luxel past [`LUXEL_TOLERANCE`], and the
    /// mean within [`MEAN_BAR`].
    fn agree(&self) -> bool {
        self.faces_differing == 0 && self.over == 0 && self.mean() <= MEAN_BAR
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

/// The lightmaps of the lit BSPs `ours` and `theirs`, made from one BSP, compared face by face
/// and each style's block with the oracle's block of the same style, by `samples`.
fn compare(ours: &[u8], theirs: &[u8], samples: Samples<'_>) -> Result<Images, String> {
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
        let (Ok(at_a), Ok(at_b)) = (lit_a, lit_b) else {
            if lit_a.is_ok() != lit_b.is_ok() {
                images.differ();
            }
            continue;
        };
        let (styles_a, styles_b) = (styles(fa), styles(fb));
        let mut sorted = (styles_a.clone(), styles_b.clone());
        sorted.0.sort_unstable();
        sorted.1.sort_unstable();
        if fa.lightmap_size != fb.lightmap_size || sorted.0 != sorted.1 {
            images.differ();
            continue;
        }
        let block = fa.lightmap_size[0]
            .saturating_mul(fa.lightmap_size[1])
            .saturating_mul(scale);
        for (slot_a, style) in styles_a.iter().enumerate() {
            let slot_b = styles_b.iter().position(|s| s == style).unwrap_or(slot_a);
            let ours = lightmap(data_a, (at_a, slot_a), (scale, block));
            let theirs = lightmap(data_b, (at_b, slot_b), (scale, block));
            match (ours, theirs) {
                (Some(x), Some(y)) => images.add(x, y),
                _ => images.differ(),
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
        let compiled = match oracle_compile(self.exec, qbsp, path, &text, &[], &dir)? {
            Ok(compiled) => compiled,
            Err(why) => {
                println!("skip  {name}: {} stops: {why}", qbsp.display());
                return Ok(true);
            }
        };
        let past = past_id_tables(&compiled.bsp)?;
        let colour = words.contains("\"_color\"");
        let own: Vec<&str> = ERICW_ONLY_KEYS
            .into_iter()
            .filter(|key| words.contains(&format!("\"{key}")))
            .collect();
        let room = path.ends_with(COLOURED_ROOM);
        let mut passed = true;
        for mode in Mode::ALL {
            let label = format!("{name} {}", mode.name());
            match past {
                Some((table, count, size)) => println!(
                    "skip  {label} light: {count} past {table}, {size}, which id's light overruns"
                ),
                None => passed &= self.white(&compiled.bsp, mode, (&dir, &label))?,
            }
            if !colour {
                continue;
            }
            // ericw-tools' `-extra` takes its samples elsewhere than id's, which ours follows
            // (LIGHT-1.3): lit white, the coloured room's luxels differ from id's by 1.54 on
            // average in that mode and by 0.018 in the fast one.
            if let Mode::Extra = mode {
                println!("skip  {label} colour: ericw-tools' -extra samples elsewhere than id's");
            } else if own.is_empty() {
                passed &= self.colour(&compiled.bsp, mode, (&dir, &label), room)?;
            } else {
                println!(
                    "skip  {label} colour: it uses ericw-tools' own lighting, {}",
                    own.join(", ")
                );
            }
        }
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
                match compare(&ours.bsp, &theirs, Samples::Grey) {
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

    /// `bsp` lit by ours and by ericw-tools' light in `mode`, their `.lit` files compared; whether
    /// they agree. On the coloured room, which is there to be compared, an oracle that writes no
    /// `.lit` fails; elsewhere it is passed over.
    fn colour(
        &self,
        bsp: &[u8],
        mode: Mode,
        (dir, label): (&Path, &str),
        room: bool,
    ) -> Result<bool, String> {
        let [_, _, ericw_light] = &self.tools.ericw;
        let ours = match Light::new(mode.sampling()).run(bsp) {
            Ok(ours) => ours,
            Err(error) => {
                println!("FAIL  {label} colour: ours stops: {error}");
                return Ok(false);
            }
        };
        let Some(our_lit) = &ours.lit else {
            println!("FAIL  {label} colour: ours wrote no .lit");
            return Ok(false);
        };
        let name = format!("ericw-{}", mode.name());
        let lit = oracle_light(self.exec, ericw_light, mode.options(), bsp, (dir, &name))?;
        let (theirs, their_lit) = match lit {
            Ok(OracleLit {
                bsp: theirs,
                lit: Some(their_lit),
            }) => (theirs, their_lit),
            Ok(OracleLit { lit: None, .. }) => {
                println!(
                    "{} {label} colour: ericw-tools wrote no .lit",
                    verdict(!room)
                );
                return Ok(!room);
            }
            Err(stop) => {
                println!(
                    "{} {label} colour: ericw-tools stops: {stop}",
                    verdict(!room)
                );
                return Ok(!room);
            }
        };
        let (agrees, said) = match compare(&ours.bsp, &theirs, Samples::Colour(our_lit, &their_lit))
        {
            Ok(images) => (images.agree(), images.to_string()),
            Err(why) => (false, why),
        };
        println!(
            "{} {label} colour against ericw-tools: {said}",
            verdict(agrees)
        );
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
