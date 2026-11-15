// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask light-diff [--maps <dir>] [--map <name>]...`: the light compiler differential
//! (P7-01, MAP-10). Each map is compiled once, by id's qbsp when it reads the map and else by
//! ericw-tools' qbsp, and that BSP lit by id's light and by qs-map-light in both modes, fast and `-extra` (LIGHT-1.3). The two compiles
//! light the same faces, so their lightmaps are compared luxel by luxel, as images. A map with
//! a coloured light (LIGHT-6) is also lit by ericw-tools' light, whose `.lit` ours is compared
//! with, channel by channel.
//!
//! The maps are `map-diff`'s: the fixtures and the corpus's (MAP-19). The oracles are built as
//! `cargo xtask oracles maps` builds them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use qs_bsp::{Bsp, BspLoader};
use qs_map::qbsp;
use qs_map_light::{Dialect, Light, Sampling};

use crate::download::Net;
use crate::exec::Exec;
use crate::map_diff::{corpus_maps, map_name, oracle_compile, without_wads};
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
/// The keys of ericw-tools' own lighting, which ours does not have yet (L-12): a map with any
/// is lit differently by ericw-tools' light, so its colour is not compared.
const ERICW_ONLY_KEYS: [&str; 12] = [
    "_sunlight",
    "_sun_mangle",
    "_sunlight2",
    "_sunlight3",
    "_surface",
    "delay",
    "_deviance",
    "_minlight",
    "_bounce",
    "_dirt",
    "_anglescale",
    "_softangle",
];
/// The classname of an instance of an external map (MAP-14).
const EXTERNAL_MAP: &str = "misc_external_map";
/// The least light a `.lit` sample needs, in its largest channel, for its hue to be compared.
const HUE_FLOOR: u8 = 16;
/// The channels of a `.lit` sample.
const CHANNELS: usize = 3;

/// A light mode, with the option that asks id's light for it.
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
    /// The faces whose styles, or lightmap sizes, are not the same in both.
    pub(crate) faces_differing: usize,
}

/// How far two images may be apart and agree: the mean difference of their luxels, and the share
/// of luxels past [`LUXEL_TOLERANCE`].
#[derive(Clone, Copy, Debug)]
struct Bar {
    mean: f64,
    share_over: f64,
}

/// The bar against id's light: no luxel more than [`LUXEL_TOLERANCE`] from id's, and on average
/// within a twentieth of a level. Every map of the corpus measured 0 in both modes (P7-01).
const GREY: Bar = Bar {
    mean: 0.05,
    share_over: 0.0,
};
/// The bar for the hues against ericw-tools' light, in the fast mode: on average within a tenth
/// of a level, and no more than one sample in a hundred past [`LUXEL_TOLERANCE`], where a hue
/// of a dim sample turns on a byte's rounding. The coloured room measured 0.038 and 0.63%.
const HUE: Bar = Bar {
    mean: 0.1,
    share_over: 0.01,
};

/// The word a line starts with.
const fn verdict(agrees: bool) -> &'static str {
    if agrees { "ok   " } else { "FAIL " }
}

/// How far a luxel may be from the oracle's and count as the same.
pub(crate) const LUXEL_TOLERANCE: u8 = 2;

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

    /// Adds the hues of the `.lit` samples `a` and `b`, the same samples of each: each
    /// channel scaled so the sample's largest is 255, for samples both light to at least
    /// [`HUE_FLOOR`], where a byte's rounding does not decide the hue.
    fn add_hues(&mut self, a: &[u8], b: &[u8]) {
        for (x, y) in a
            .as_chunks::<CHANNELS>()
            .0
            .iter()
            .zip(b.as_chunks::<CHANNELS>().0)
        {
            let (Some(&mx), Some(&my)) = (x.iter().max(), y.iter().max()) else {
                continue;
            };
            if mx < HUE_FLOOR || my < HUE_FLOOR {
                continue;
            }
            // Each channel as a share of the brightest, in 255ths, rounded down.
            let hue = |sample: &[u8; CHANNELS], largest: u8| -> Vec<u8> {
                sample
                    .iter()
                    .map(|&c| {
                        let scaled = u16::from(c)
                            .saturating_mul(255)
                            .checked_div(u16::from(largest))
                            .unwrap_or(0);
                        u8::try_from(scaled).unwrap_or(u8::MAX)
                    })
                    .collect()
            };
            self.add(&hue(x, mx), &hue(y, my));
        }
    }

    /// Whether the images agree within `bar`.
    fn within(&self, bar: Bar) -> bool {
        self.mean() <= bar.mean && self.share_over() <= bar.share_over
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

    /// The share of luxels past [`LUXEL_TOLERANCE`].
    fn share_over(&self) -> f64 {
        let compared = u32::try_from(self.compared).unwrap_or(u32::MAX);
        let over = u32::try_from(self.over).unwrap_or(u32::MAX);
        if compared == 0 {
            0.0
        } else {
            f64::from(over) / f64::from(compared)
        }
    }
}

impl std::fmt::Display for Images {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} compared, mean {:.3}, largest {}, {:.4}% past {LUXEL_TOLERANCE}, {} faces differ",
            self.compared,
            self.mean(),
            self.largest,
            self.share_over() * 100.0,
            self.faces_differing
        )
    }
}

/// The lightmaps of the lit BSPs `ours` and `theirs`, compiled from one BSP, compared face by
/// face; with `lits`, their `.lit` files' samples instead of the grey ones.
fn compare(ours: &[u8], theirs: &[u8], lits: Option<(&[u8], &[u8])>) -> Result<Images, String> {
    let a = Bsp::read(ours, BspLoader::FitzQuake).map_err(|e| format!("our BSP: {e}"))?;
    let b = Bsp::read(theirs, BspLoader::FitzQuake).map_err(|e| format!("the oracle's: {e}"))?;
    if a.faces.len() != b.faces.len() {
        return Err(format!("{} faces against {}", a.faces.len(), b.faces.len()));
    }
    let (data_a, data_b, scale) = match lits {
        Some((lit_a, lit_b)) => (
            lit_a.get(LIT_HEADER..).unwrap_or_default(),
            lit_b.get(LIT_HEADER..).unwrap_or_default(),
            CHANNELS,
        ),
        None => (a.lighting, b.lighting, 1),
    };
    let mut images = Images::default();
    for (fa, fb) in a.faces.iter().zip(&b.faces) {
        let size = fa.lightmap_size[0].saturating_mul(fa.lightmap_size[1]);
        if fa.styles != fb.styles || fa.lightmap_size != fb.lightmap_size {
            images.faces_differing = images.faces_differing.saturating_add(1);
            continue;
        }
        let (Ok(at_a), Ok(at_b)) = (usize::try_from(fa.lightofs), usize::try_from(fb.lightofs))
        else {
            continue;
        };
        let length = size.saturating_mul(fa.style_count()).saturating_mul(scale);
        let slice = |data: &[u8], at: usize| {
            let at = at.saturating_mul(scale);
            data.get(at..at.saturating_add(length)).map(<[u8]>::to_vec)
        };
        match (slice(data_a, at_a), slice(data_b, at_b)) {
            (Some(x), Some(y)) if lits.is_some() => images.add_hues(&x, &y),
            (Some(x), Some(y)) => images.add(&x, &y),
            _ => images.faces_differing = images.faces_differing.saturating_add(1),
        }
    }
    Ok(images)
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

/// `bsp` lit in `dir` by the oracle `light` with `options`, as `<name>.bsp`; the lit BSP and
/// its `.lit` when the oracle wrote one.
fn oracle_light(
    exec: &dyn Exec,
    light: &Path,
    options: &[&str],
    bsp: &[u8],
    (dir, name): (&Path, &str),
) -> Result<(Vec<u8>, Option<Vec<u8>>), String> {
    let file = dir.join(format!("{name}.bsp"));
    fs::write(&file, bsp).map_err(|e| format!("{}: {e}", file.display()))?;
    let lit_file = file.with_extension("lit");
    if lit_file.exists() {
        fs::remove_file(&lit_file).map_err(|e| format!("{}: {e}", lit_file.display()))?;
    }
    let mut command = Command::new(light);
    command.args(options).arg(&file).current_dir(dir);
    let output = exec
        .output(&mut command)
        .map_err(|e| format!("cannot run {}: {e}", light.display()))?;
    if !output.status.success() {
        return Err(format!(
            "{} stopped with {}: {}",
            light.display(),
            output.status,
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .last()
                .unwrap_or_default()
        ));
    }
    let lit = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok((lit, fs::read(&lit_file).ok()))
}

/// `cargo xtask light-diff`; whether every map's lighting agrees.
pub(crate) fn main(
    root: &Path,
    exec: &dyn Exec,
    net: &dyn Net,
    args: &Args,
) -> Result<bool, String> {
    let mut maps = corpus_maps(root, args.maps.as_deref(), &args.only)?;
    let coloured_room = root.join(COLOURED_ROOM);
    if args.only.is_empty() || args.only.contains(&map_name(&coloured_room)) {
        maps.insert(0, coloured_room);
    }
    let tools: MapTools = build(root, exec, net)?;
    let [id_qbsp, _, id_light] = &tools.id;
    let [ericw_qbsp, _, ericw_light] = &tools.ericw;
    let work = WORK
        .iter()
        .fold(root.join(OUT), |path, part| path.join(part))
        .join("light-diff");
    let mut passed = true;
    for path in &maps {
        let name = map_name(path);
        let text = without_wads(&fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if String::from_utf8_lossy(&text).contains(EXTERNAL_MAP) {
            println!("skip  {name}: its instances are compiled by map-diff");
            continue;
        }
        // id's light reads any BSP29, whichever qbsp wrote it.
        let qbsp = if qbsp::load_map_file(&text).is_ok() {
            id_qbsp
        } else {
            ericw_qbsp
        };
        let dir = work.join(&name);
        let compiled = match oracle_compile(exec, qbsp, path, &text, &[], &dir)? {
            Ok(compiled) => compiled,
            Err(why) => {
                println!("skip  {name}: {} stops: {why}", qbsp.display());
                continue;
            }
        };
        let coloured = String::from_utf8_lossy(&text).contains("\"_color\"");
        let past = past_id_tables(&compiled.bsp)?;
        for mode in Mode::ALL {
            let label = format!("{name} {}", mode.name());
            let light = Light::new(mode.sampling()).with_dialect(Dialect::Id);
            if let Some((table, count, size)) = past {
                println!(
                    "skip  {label} light: {count} past {table}, {size}, which id's light overruns"
                );
            }
            // id's light has no colour: our white light of the map is what it is compared with.
            let ours = light.run(&white(&compiled.bsp));
            let theirs = oracle_light(
                exec,
                id_light,
                mode.options(),
                &compiled.bsp,
                (&dir, &format!("id-{}", mode.name())),
            );
            match (ours, theirs) {
                _ if past.is_some() => {}
                (Ok(ours), Ok((theirs, _))) => {
                    let file = dir.join(format!("ours-{}.bsp", mode.name()));
                    fs::write(&file, &ours.bsp).map_err(|e| format!("{}: {e}", file.display()))?;
                    let images = compare(&ours.bsp, &theirs, None)?;
                    let agrees = images.within(GREY);
                    passed &= agrees;
                    println!("{} {label} light: {images}", verdict(agrees));
                }
                (Err(ours), Err(theirs)) => {
                    println!("ok    {label}: both stop: {ours}; {theirs}");
                }
                (Err(ours), Ok(_)) => {
                    passed = false;
                    println!("FAIL  {label}: only ours stops: {ours}");
                }
                (Ok(_), Err(theirs)) => {
                    passed = false;
                    println!("FAIL  {label}: only id's light stops: {theirs}");
                }
            }
            if !coloured {
                continue;
            }
            // ericw-tools' `-extra` takes its samples elsewhere than id's, which ours follows
            // (LIGHT-1.3): lit white, the coloured room's luxels differ from id's by 1.54 on
            // average in that mode and by 0.018 in the fast one, so only the fast mode's hues are
            // compared.
            if let Mode::Extra = mode {
                println!("skip  {label} colour: ericw-tools' -extra samples elsewhere than id's");
                continue;
            }
            let own: Vec<&str> = ERICW_ONLY_KEYS
                .into_iter()
                .filter(|key| String::from_utf8_lossy(&text).contains(&format!("\"{key}\"")))
                .collect();
            if !own.is_empty() {
                println!(
                    "skip  {label} colour: it uses ericw-tools' own lighting, {}",
                    own.join(", ")
                );
                continue;
            }
            // The colour, in the extended dialect, where a map is not held to id's tables,
            // against ericw-tools' light, whose colour LIGHT-6 follows.
            let ours = match Light::new(mode.sampling()).run(&compiled.bsp) {
                Ok(ours) => ours,
                Err(error) => {
                    passed = false;
                    println!("FAIL  {label} colour: ours stops: {error}");
                    continue;
                }
            };
            let Some(our_lit) = &ours.lit else {
                passed = false;
                println!("FAIL  {label} colour: ours wrote no .lit");
                continue;
            };
            let (ericw, ericw_lit) = match oracle_light(
                exec,
                ericw_light,
                mode.options(),
                &compiled.bsp,
                (&dir, &format!("ericw-{}", mode.name())),
            ) {
                Ok(lit) => lit,
                Err(why) => {
                    println!("skip  {label} colour: {why}");
                    continue;
                }
            };
            let Some(ericw_lit) = ericw_lit else {
                println!("skip  {label} colour: ericw-tools wrote no .lit");
                continue;
            };
            let hues = compare(&ours.bsp, &ericw, Some((our_lit, &ericw_lit)))?;
            let agrees = hues.within(HUE);
            passed &= agrees;
            println!(
                "{} {label} colour against ericw-tools: {hues}",
                verdict(agrees)
            );
        }
    }
    Ok(passed)
}

#[cfg(test)]
mod tests {
    use super::{Bar, HUE_FLOOR, Images, LUXEL_TOLERANCE, white};

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
        assert!(!images.within(Bar {
            mean: 4.0,
            share_over: 0.4
        }));
        assert!(images.within(Bar {
            mean: 4.0,
            share_over: 0.5
        }));
        assert_eq!(Images::default().mean(), 0.0, "nothing compared");
    }

    #[test]
    fn hues_compare_the_channels_against_the_brightest() {
        let mut images = Images::default();
        // The same hue at two brightnesses agrees; a sample too dim in either is not compared.
        images.add_hues(&[200, 100, 0, 15, 15, 15], &[100, 50, 0, 255, 0, 0]);
        assert_eq!(images.compared, 3, "one sample of three channels");
        assert_eq!(images.total, 0);
        let mut other = Images::default();
        other.add_hues(&[HUE_FLOOR, 0, 0], &[0, HUE_FLOOR, 0]);
        assert_eq!(other.largest, 255, "red against green");
    }
}
