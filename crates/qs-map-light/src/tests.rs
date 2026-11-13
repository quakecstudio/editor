// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code: a failed unwrap or index is a failed test, and the numbers are small \
              and exact"
)]

use qs_bsp::{BSP_CONTENTS_EMPTY, BSP_CONTENTS_SOLID, Bsp, BspLoader, LumpId};
use qs_map_bsp::Compiler;
use qs_wad::write::WadBuilder;
use sha2::{Digest as _, Sha256};

use crate::entities::Entities;
use crate::trace::Tree;
use crate::{Dialect, Light, LightError, LightWarning, Lighted, Sampling, Scale};

/// A brush, a box from `min` to `max`, faced with `texture`.
fn cuboid(min: [i32; 3], max: [i32; 3], texture: &str) -> String {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    format!(
        "{{\n( {x0} 0 0 ) ( {x0} 1 0 ) ( {x0} 0 1 ) {texture} 0 0 0 1 1\n\
         ( {x1} 0 0 ) ( {x1} 0 1 ) ( {x1} 1 0 ) {texture} 0 0 0 1 1\n\
         ( 0 {y0} 0 ) ( 0 {y0} 1 ) ( 1 {y0} 0 ) {texture} 0 0 0 1 1\n\
         ( 0 {y1} 0 ) ( 1 {y1} 0 ) ( 0 {y1} 1 ) {texture} 0 0 0 1 1\n\
         ( 0 0 {z0} ) ( 1 0 {z0} ) ( 0 1 {z0} ) {texture} 0 0 0 1 1\n\
         ( 0 0 {z1} ) ( 0 1 {z1} ) ( 1 0 {z1} ) {texture} 0 0 0 1 1\n}}\n"
    )
}

/// A texture lump `name`, 16 by 16.
fn miptex(name: &str) -> Vec<u8> {
    let mut lump = name.as_bytes().to_vec();
    lump.resize(16, 0);
    for value in [16_u32, 16, 40, 40 + 256, 40 + 256 + 64, 40 + 256 + 64 + 16] {
        lump.extend_from_slice(&value.to_le_bytes());
    }
    lump.resize(40 + 256 + 64 + 16 + 4, 1);
    lump
}

/// A box room 256 by 256 by 128 inside, compiled with these other entities.
fn room(entities: &str) -> Vec<u8> {
    let mut world = String::from("{\n\"classname\" \"worldspawn\"\n\"wad\" \"test.wad\"\n");
    for (min, max) in [
        ([-16, -16, -16], [272, 272, 0]),
        ([-16, -16, 128], [272, 272, 144]),
        ([-16, -16, 0], [0, 272, 128]),
        ([256, -16, 0], [272, 272, 128]),
        ([0, -16, 0], [256, 0, 128]),
        ([0, 256, 0], [256, 272, 128]),
    ] {
        world.push_str(&cuboid(min, max, "wall"));
    }
    world.push_str("}\n{\n\"classname\" \"info_player_start\"\n\"origin\" \"32 32 32\"\n}\n");
    world.push_str(entities);
    let mut wad = WadBuilder::new();
    wad.add(b"wall", qs_wad::WAD_TYP_MIPTEX, miptex("wall"))
        .unwrap();
    Compiler::default()
        .compile(world.as_bytes(), Some(&wad.to_bytes().unwrap()))
        .unwrap()
        .bsp
}

/// A light at `origin`, with these other keys.
fn light_at(origin: &str, keys: &str) -> String {
    format!("{{\n\"classname\" \"light\"\n\"origin\" \"{origin}\"\n{keys}}}\n")
}

/// The lump `id` of the BSP `bytes`.
fn lump(bytes: &[u8], id: LumpId) -> &[u8] {
    let read = Bsp::read(bytes, BspLoader::FitzQuake).unwrap();
    let entry = read.lumps[id.index()];
    let start = usize::try_from(entry.fileofs).unwrap();
    &bytes[start..start + usize::try_from(entry.filelen).unwrap()]
}

/// The entities of `text` as light reads them, in the id dialect.
fn entities(text: &[u8]) -> Result<Entities, LightError> {
    Entities::read(text, Dialect::Id, &mut Vec::new())
}

/// The bytes of an `xxd` hex dump.
fn unxxd(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((_, rest)) = line.split_once(": ") else {
            continue;
        };
        let hex: String = rest
            .get(..40)
            .unwrap_or(rest)
            .chars()
            .filter(char::is_ascii_hexdigit)
            .collect();
        for pair in hex.as_bytes().chunks(2) {
            out.push(u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap());
        }
    }
    out
}

/// A file of `docs/spec/vectors/light/`.
fn vector(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/spec/vectors/light")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// spec LIGHT-1.1, LIGHT-1.3, LIGHT-1.4, LIGHT-1.5, LIGHT-1.6, LIGHT-1.8, LIGHT-2.1, LIGHT-2.2, LIGHT-2.3, LIGHT-2.4, LIGHT-2.5, LIGHT-2.6, LIGHT-3.1, LIGHT-3.2, LIGHT-3.3, LIGHT-3.4, LIGHT-3.5, LIGHT-3.6, LIGHT-4.1, LIGHT-4.2, LIGHT-4.3, LIGHT-4.4, LIGHT-4.5, LIGHT-4.6, LIGHT-4.7, LIGHT-5.1, LIGHT-5.2, LIGHT-5.3:
// every map of the vectors gives the file id's light wrote, byte for byte, in both modes, with its
// warnings and switchable styles
#[test]
fn every_vector_matches_id_light() {
    let mut checked = 0;
    for line in vector("results.tsv").lines().skip(1) {
        let [map, mode, size, warnings, switchable, file] =
            line.split('\t').collect::<Vec<_>>()[..]
        else {
            panic!("a bad row: {line}");
        };
        let sampling = if mode == "extra" {
            Sampling::Extra
        } else {
            Sampling::Fast
        };
        let bsp = unxxd(&vector(&format!("{map}.bsp.txt")));
        let lit = Light::new(sampling).run(&bsp).unwrap();
        assert_eq!(hex(&Sha256::digest(&lit.bsp)), file, "{map} {mode}");
        assert_eq!(lump(&lit.bsp, LumpId::Lighting).len().to_string(), size);
        assert_eq!(lit.warnings.len().to_string(), warnings, "{map} {mode}");
        assert_eq!(lit.switchable.to_string(), switchable);
        assert_eq!(lit.lit, None, "white light writes no .lit");
        checked += 1;
    }
    assert_eq!(checked, 8, "four maps, two modes each");
}

// spec LIGHT-2.1, LIGHT-2.2:
// a light value from `light*` or `_light`, the last such key winning and truncated; a `light*`
// class with none, or 0, gets 300; any other entity is no light without one
#[test]
fn light_values_are_read_as_light_reads_them() {
    let read = entities(
        b"{ \"classname\" \"light\" }\n\
          { \"classname\" \"light_torch\" \"light\" \"0\" }\n\
          { \"classname\" \"info_null\" \"_light\" \"150.9\" }\n\
          { \"classname\" \"x\" \"light\" \"10\" \"lightning\" \"-20.5\" }\n\
          { \"classname\" \"x\" \"Light\" \"10\" }\n",
    )
    .unwrap();
    let lights: Vec<i32> = read.all.iter().map(|e| e.light).collect();
    assert_eq!(lights, [300, 300, 150, -20, 0]);
    assert!(read.all[3].is_light() && !read.all[4].is_light());
}

// spec LIGHT-2.7: COM_Parse's tokens: comments, quoted strings, the six single bytes, bytes
// over 127 as white space, and a string with no closing quote running to the end
#[test]
fn entity_text_is_read_as_com_parse_reads_it() {
    let read = entities(
        b"// a comment { \"classname\" \"no\" }\n\
          {\"classname\"\"light\"\"origin\"\"1 2 3\"}{\x80classname\xff\"a b\"}{ k(v) }",
    )
    .unwrap();
    assert_eq!(read.all.len(), 3);
    assert_eq!(read.all[0].origin, [1.0, 2.0, 3.0]);
    assert_eq!(read.all[0].light, 300);
    let written = read.written(Dialect::Id).unwrap();
    assert_eq!(
        written,
        b"{\n\"origin\" \"1 2 3\"\n\"classname\" \"light\"\n}\n{\n\"classname\" \"a b\"\n}\n\
          {\n\"v\" \")\"\n\"k\" \"(\"\n}\n\0"
    );
    assert_eq!(
        entities(b"{ \"style\" \"7\" \"angle\" \"45"),
        Err(LightError::Unclosed),
        "the last string runs to the end"
    );
    let read = entities(b"{ \"angle\" \"45 }").unwrap_err();
    assert_eq!(read, LightError::Unclosed);
}

// spec LIGHT-2.7, LIGHT-2.3, LIGHT-1.6:
// each error LoadEntities stops on
#[test]
fn bad_entity_text_stops_the_run() {
    assert_eq!(
        entities(b"\"classname\""),
        Err(LightError::ExpectedBrace {
            found: b"classname".to_vec()
        })
    );
    assert_eq!(entities(b"{ \"classname\""), Err(LightError::Unclosed));
    assert_eq!(entities(b"{ \"classname\" }"), Err(LightError::NoValue));
    assert_eq!(
        entities(b"{ \"classname\" \"}x\" }"),
        Err(LightError::NoValue)
    );
    assert_eq!(entities(b"{ \"origin\" \"1 2\" }"), Err(LightError::Origin));
    assert_eq!(
        entities(b"{ \"style\" \"255\" }"),
        Err(LightError::BadStyle { style: 255 })
    );
    assert_eq!(
        entities(b"{ \"style\" \"-1\" }"),
        Err(LightError::BadStyle { style: -1 })
    );
    assert!(entities(b"{ \"style\" \"254\" }").is_ok());
    let many = "{ }\n".repeat(1025);
    assert_eq!(entities(many.as_bytes()), Err(LightError::TooManyEntities));
    assert!(entities("{ }\n".repeat(1024).as_bytes()).is_ok());
}

// spec LIGHT-1.7: the extended dialect has none of light's fixed tables
#[test]
fn the_extended_dialect_has_no_fixed_tables() {
    let many = "{ \"classname\" \"info_null\" }\n".repeat(1100);
    let read = Entities::read(many.as_bytes(), Dialect::Extended, &mut Vec::new()).unwrap();
    assert_eq!(read.all.len(), 1100);
    // Each entity is `{\n"classname" "info_null"\n}\n`, 28 bytes.
    assert_eq!(
        read.written(Dialect::Extended).unwrap().len(),
        1100 * 28 + 1
    );
    let long = format!("{{ \"note\" \"{}\" }}\n", "x".repeat(70_000));
    let read = Entities::read(long.as_bytes(), Dialect::Extended, &mut Vec::new()).unwrap();
    assert_eq!(read.written(Dialect::Extended).unwrap().len(), 70_015);
}

// spec LIGHT-1.6: text past MAX_MAP_ENTSTRING once an entity is written stops the run in the
// id dialect; text up to it is written
#[test]
fn entity_text_has_light_s_limit_in_the_id_dialect() {
    // Each entity is `{\n"k" "<value>"\n}\n`: 11 bytes and the value, 111 with 100.
    let one = format!("{{ \"k\" \"{}\" }}\n", "v".repeat(100));
    let fits = one.repeat(590);
    assert_eq!(
        entities(fits.as_bytes())
            .unwrap()
            .written(Dialect::Id)
            .unwrap()
            .len(),
        590 * 111 + 1
    );
    let over = one.repeat(591);
    assert_eq!(
        entities(over.as_bytes()).unwrap().written(Dialect::Id),
        Err(LightError::EntityTextTooLong)
    );
}

// spec LIGHT-2.8: a key and value of more than 121 bytes together are refused in the id
// dialect, and written whole in the extended one
#[test]
fn a_long_pair_is_refused_in_the_id_dialect() {
    let pair = |len: usize| format!("{{ \"key\" \"{}\" }}", "v".repeat(len));
    assert!(
        entities(pair(118).as_bytes())
            .unwrap()
            .written(Dialect::Id)
            .is_ok()
    );
    assert_eq!(
        entities(pair(119).as_bytes()).unwrap().written(Dialect::Id),
        Err(LightError::PairTooLong {
            key: "key".to_owned()
        })
    );
    let text = entities(pair(119).as_bytes())
        .unwrap()
        .written(Dialect::Extended)
        .unwrap();
    assert_eq!(text.len(), 133);
}

// spec LIGHT-2.4, LIGHT-2.5, LIGHT-1.6:
// named lights get styles from 32, a name its first style; a target with a style gives it to the
// entity that targets it; the style key is written over the last one read, or first; a 33rd name is
// refused
#[test]
fn switchable_styles_and_targets() {
    let mut warnings = Vec::new();
    let read = Entities::read(
        b"{ \"classname\" \"light\" \"targetname\" \"a\" }\n\
          { \"classname\" \"light\" \"targetname\" \"b\" \"style\" \"0\" \"style\" \"0\" }\n\
          { \"classname\" \"light\" \"targetname\" \"a\" }\n\
          { \"classname\" \"light\" \"targetname\" \"c\" \"style\" \"3\" }\n\
          { \"classname\" \"light_x\" \"targetname\" \"d\" }\n\
          { \"classname\" \"trigger_once\" \"target\" \"b\" }\n\
          { \"classname\" \"trigger_once\" \"target\" \"nobody\" \"origin\" \"1.9 -2.5 3\" }\n\
          { \"classname\" \"light\" \"target\" \"c\" }\n",
        Dialect::Id,
        &mut warnings,
    )
    .unwrap();
    let styles: Vec<i32> = read.all.iter().map(|e| e.style).collect();
    assert_eq!(styles, [32, 33, 32, 3, 0, 33, 0, 3]);
    assert_eq!(read.switched(), 2);
    assert_eq!(read.all[5].target_entity, Some(1));
    assert_eq!(read.all[7].target_entity, Some(3));
    assert_eq!(
        warnings,
        [LightWarning::UnmatchedTarget {
            origin: [1, -2, 3],
            classname: "trigger_once".to_owned()
        }]
    );
    let text = String::from_utf8(read.written(Dialect::Id).unwrap()).unwrap();
    assert!(
        text.starts_with("{\n\"style\" \"32\"\n\"targetname\" \"a\"\n\"classname\" \"light\"\n}\n")
    );
    assert!(text.contains(
        "{\n\"style\" \"33\"\n\"style\" \"0\"\n\"targetname\" \"b\"\n\"classname\" \"light\"\n}\n"
    ));
    assert!(
        text.contains("{\n\"style\" \"33\"\n\"target\" \"b\"\n\"classname\" \"trigger_once\"\n}\n")
    );
    let names = |count: usize| -> String {
        (0..count)
            .map(|i| format!("{{ \"classname\" \"light\" \"targetname\" \"n{i}\" }}\n"))
            .collect()
    };
    assert_eq!(
        entities(names(33).as_bytes()),
        Err(LightError::TooManySwitched)
    );
    assert_eq!(entities(names(32).as_bytes()).unwrap().all[31].style, 63);
}

// spec LIGHT-6.1, LIGHT-6.5:
// `_color` scaled by 255 when every component is 0 to 1, else taken as it is; white is no colour
#[test]
fn colours_are_read_as_ericw_tools_reads_them() {
    let read = entities(
        b"{ \"_color\" \"1 0.5 0\" }{ \"_color\" \"2 1 1\" }{ \"_color\" \"255 255 255\" }\
          { \"_color\" \"1 1 1\" }{ \"_color\" \"1 1\" }{ \"k\" \"v\" }",
    )
    .unwrap();
    let colours: Vec<Option<[f32; 3]>> = read.all.iter().map(|e| e.colour).collect();
    assert_eq!(
        colours,
        [
            Some([255.0, 127.5, 0.0]),
            Some([2.0, 1.0, 1.0]),
            Some([255.0; 3]),
            Some([255.0; 3]),
            None,
            None
        ]
    );
    let coloured: Vec<bool> = read.all.iter().map(|e| e.is_coloured()).collect();
    assert_eq!(coloured, [true, true, false, false, false, false]);
}

// spec LIGHT-4.1, LIGHT-4.8, LIGHT-4.9:
// a line split at a hundred nodes reaches its end through empty leaves and is stopped by a solid
// far part, the stack of split parts having no limit; a line within ON_EPSILON of a plane goes to
// the front, and one 0.1f behind it is split
#[test]
fn lines_are_traced_through_the_tree() {
    let dists: Vec<f32> = (0..100_u16).map(|i| f32::from(i) * 10.0).collect();
    let open = Tree::chain(&dists, BSP_CONTENTS_EMPTY);
    assert!(open.test_line([1005.0, 0.0, 0.0], [-5.0, 0.0, 0.0]));
    let walled = Tree::chain(&dists, BSP_CONTENTS_SOLID);
    assert!(!walled.test_line([1005.0, 0.0, 0.0], [-5.0, 0.0, 0.0]));
    assert!(walled.test_line([1005.0, 0.0, 0.0], [5.0, 0.0, 0.0]));
    let wall = Tree::chain(&[0.0], BSP_CONTENTS_SOLID);
    let near = (-0.1_f32).next_up();
    assert!(
        wall.test_line([0.0, 0.0, 0.0], [near, 0.0, 0.0]),
        "on the plane"
    );
    assert!(
        !wall.test_line([5.0, 0.0, 0.0], [-0.1, 0.0, 0.0]),
        "0.1f behind"
    );
    assert!(!wall.test_line([-5.0, 0.0, 0.0], [5.0, 0.0, 0.0]));
}

/// The BSP `bytes` with every texinfo's axes made `axes`.
fn with_axes(bytes: &[u8], axes: impl Fn([[f32; 4]; 2]) -> [[f32; 4]; 2]) -> Vec<u8> {
    let read = Bsp::read(bytes, BspLoader::FitzQuake).unwrap();
    let entry = read.lumps[LumpId::Texinfo.index()];
    let start = usize::try_from(entry.fileofs).unwrap();
    let mut out = bytes.to_vec();
    for (i, info) in read.texinfo.iter().enumerate() {
        let at = start + i * 40;
        for (j, value) in axes(info.vecs).iter().flatten().enumerate() {
            out[at + j * 4..at + j * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    out
}

// spec LIGHT-3.2, LIGHT-3.6:
// a face over 17 luxels, and a texture axis at right angles to a face, stop the run
#[test]
fn bad_texture_axes_stop_the_run() {
    let bsp = room(&light_at("128 128 64", ""));
    let wide = with_axes(&bsp, |v| v.map(|axis| axis.map(|c| c * 4.0)));
    assert!(matches!(
        Light::default().run(&wide),
        Err(LightError::Extents { .. })
    ));
    let flat = with_axes(&bsp, |_| [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]]);
    assert!(matches!(
        Light::default().run(&flat),
        Err(LightError::TextureAxis { .. })
    ));
}

// spec LIGHT-5.1: a face no light reaches has lightofs -1 and styles 255, and a map with no
// light has an empty lighting lump; each lit face starts on four bytes
#[test]
fn unlit_faces_have_no_lightmap() {
    let dark = Light::default().run(&room("")).unwrap();
    let read = Bsp::read(&dark.bsp, BspLoader::FitzQuake).unwrap();
    assert!(read.lighting.is_empty());
    assert!(
        read.faces
            .iter()
            .all(|f| f.lightofs == -1 && f.styles == [255; 4])
    );
    let lit = Light::default()
        .run(&room(&light_at("128 128 64", "")))
        .unwrap();
    let read = Bsp::read(&lit.bsp, BspLoader::FitzQuake).unwrap();
    assert!(
        read.faces
            .iter()
            .all(|f| f.lightofs >= 0 && f.lightofs % 4 == 0 && f.styles == [0, 255, 255, 255])
    );
}

// spec LIGHT-1.2: -range scales each byte, and -dist the distance a light reaches
#[test]
fn the_scales_change_the_bytes() {
    let bsp = room(&light_at("128 128 64", "\"light\" \"100\"\n"));
    let bytes = |scale: Scale| {
        let lit = Light::default().with_scale(scale).run(&bsp).unwrap();
        lump(&lit.bsp, LumpId::Lighting).to_vec()
    };
    let plain = bytes(Scale::default());
    let double = bytes(Scale {
        range: 1.0,
        ..Scale::default()
    });
    let mut compared = 0;
    for (&p, &d) in plain.iter().zip(&double) {
        if p > 0 && p < 100 {
            assert!(d.abs_diff(p * 2) <= 1, "{p} {d}");
            compared += 1;
        }
    }
    assert!(compared > 0);
    let far = bytes(Scale {
        dist: 0.5,
        ..Scale::default()
    });
    assert!(far.len() > plain.len(), "the light reaches more faces");
}

// spec LIGHT-1.8: the bytes skipped to align a face, and the padding after the lump, keep the
// input's lighting bytes, here a longer lump of two styles a face
#[test]
fn padding_keeps_the_input_lighting() {
    let lights = light_at("128 128 64", "\"style\" \"1\"\n") + &light_at("100 90 64", "");
    let first = Light::new(Sampling::Fast).run(&room(&lights)).unwrap().bsp;
    let entry = Bsp::read(&first, BspLoader::FitzQuake).unwrap().lumps[LumpId::Lighting.index()];
    let start = usize::try_from(entry.fileofs).unwrap();
    let old_len = usize::try_from(entry.filelen).unwrap();
    let mut marked = first.clone();
    for byte in &mut marked[start..start + old_len] {
        *byte = 0xab;
    }
    // The same lights, both of style 0: one lightmap a face, a shorter lump.
    let style = marked
        .windows(11)
        .position(|w| w == b"\"style\" \"1\"")
        .unwrap();
    marked[style + 9] = b'0';
    let again = Light::new(Sampling::Fast).run(&marked).unwrap().bsp;
    let read = Bsp::read(&again, BspLoader::FitzQuake).unwrap();
    assert!(read.lighting.len() < old_len);
    let mut covered = vec![false; read.lighting.len()];
    for face in read.faces.iter().filter(|f| f.lightofs >= 0) {
        let at = usize::try_from(face.lightofs).unwrap();
        let size = face.lightmap_size[0] * face.lightmap_size[1] * face.style_count();
        covered[at..at + size].iter_mut().for_each(|c| *c = true);
    }
    let gaps: Vec<u8> = read
        .lighting
        .iter()
        .zip(&covered)
        .filter(|(_, c)| !**c)
        .map(|(b, _)| *b)
        .collect();
    assert!(!gaps.is_empty(), "the map has gaps");
    assert!(gaps.iter().all(|&b| b == 0xab));
    let entry = read.lumps[LumpId::Lighting.index()];
    let end = usize::try_from(entry.fileofs).unwrap() + usize::try_from(entry.filelen).unwrap();
    assert!(end % 4 != 0, "the lump needs padding");
    assert!(
        again[end..end.next_multiple_of(4)]
            .iter()
            .all(|&b| b == 0xab)
    );
    let fresh = Light::new(Sampling::Fast).run(&room(&lights)).unwrap().bsp;
    let entry = Bsp::read(&fresh, BspLoader::FitzQuake).unwrap().lumps[LumpId::Lighting.index()];
    let end = usize::try_from(entry.fileofs).unwrap() + usize::try_from(entry.filelen).unwrap();
    assert!(
        fresh[end..end.next_multiple_of(4)].iter().all(|&b| b == 0),
        "zero past the input's"
    );
}

// spec LIGHT-6.2, LIGHT-6.3, LIGHT-6.4, LIGHT-6.5:
// a coloured light writes a .lit of each sample's channels, and the grey byte is the brightest;
// white writes none and lights as id's light; a channel past 255 scales the sample down
#[test]
fn coloured_light_writes_a_lit_file() {
    let red = Light::default()
        .run(&room(&light_at("128 128 64", "\"_color\" \"1 0 0\"\n")))
        .unwrap();
    let lit = red.lit.as_ref().unwrap();
    let lighting = lump(&red.bsp, LumpId::Lighting);
    assert_eq!(&lit[..8], b"QLIT\x01\0\0\0");
    assert_eq!(lit.len(), 8 + 3 * lighting.len());
    for (grey, rgb) in lighting.iter().zip(lit[8..].chunks(3)) {
        assert_eq!(rgb[1..], [0, 0]);
        assert_eq!(*grey, rgb[0]);
    }
    assert!(lighting.iter().any(|&b| b > 0));
    let plain = Light::default()
        .run(&room(&light_at("128 128 64", "")))
        .unwrap();
    let white = Light::default()
        .run(&room(&light_at(
            "128 128 64",
            "\"_color\" \"255 255 255\"\n",
        )))
        .unwrap();
    assert_eq!(white.lit, None);
    assert_eq!(
        lump(&white.bsp, LumpId::Lighting),
        lump(&plain.bsp, LumpId::Lighting)
    );
    let bright = Light::default()
        .run(&room(&light_at(
            "128 128 64",
            "\"_color\" \"510 255 0\"\n\"light\" \"600\"\n",
        )))
        .unwrap();
    let lit = bright.lit.unwrap();
    let full: Vec<&[u8]> = lit[8..].chunks(3).filter(|c| c[0] == 255).collect();
    assert!(!full.is_empty());
    for rgb in full {
        assert!(
            (126..=128).contains(&rgb[1]),
            "red scaled down to 255: {rgb:?}"
        );
    }
}

// spec LIGHT-4.5: a light of a fifth style on a face warns and adds nothing
#[test]
fn a_fifth_style_warns() {
    let lights: String = (1..=5)
        .map(|style| light_at("128 128 64", &format!("\"style\" \"{style}\"\n")))
        .collect();
    let lit = Light::default().run(&room(&lights)).unwrap();
    assert!(lit.warnings.contains(&LightWarning::TooManyStyles));
    let read = Bsp::read(&lit.bsp, BspLoader::FitzQuake).unwrap();
    assert!(read.faces.iter().all(|f| f.styles == [1, 2, 3, 4]));
}

// spec LIGHT-1.5, LIGHT-1.6, LIGHT-2.3, LIGHT-2.5, LIGHT-2.7, LIGHT-2.8, LIGHT-3.2, LIGHT-3.6:
// each error and warning reads as light's message
#[test]
fn errors_and_warnings_read_as_light_writes_them() {
    let cases = [
        (
            LightError::ExpectedBrace {
                found: b"x".to_vec(),
            },
            "LoadEntities: found x when expecting {",
        ),
        (
            LightError::TooManyEntities,
            "LoadEntities: MAX_MAP_ENTITIES",
        ),
        (
            LightError::Unclosed,
            "LoadEntities: EOF without closing brace",
        ),
        (
            LightError::NoValue,
            "LoadEntities: closing brace without data",
        ),
        (LightError::Origin, "LoadEntities: not 3 values for origin"),
        (
            LightError::BadStyle { style: 300 },
            "Bad light style 300 (must be 0-254)",
        ),
        (
            LightError::TooManySwitched,
            "more than 32 switchable light styles",
        ),
        (
            LightError::TextureAxis { face: 3 },
            "Texture axis perpendicular to face (face 3)",
        ),
        (
            LightError::Extents { face: 4 },
            "Bad surface extents (face 4)",
        ),
        (LightError::Overrun, "GetFileSpace: overrun"),
        (LightError::EntityTextTooLong, "Entity text too long"),
        (
            LightError::PairTooLong {
                key: "wad".to_owned(),
            },
            "the pair of key wad is too long for id's light",
        ),
        (LightError::Format, "the BSP cannot be lit"),
        (LightError::Thread, "a light thread could not be run"),
    ];
    for (error, text) in cases {
        assert_eq!(error.to_string(), text);
    }
    assert_eq!(
        LightWarning::UnmatchedTarget {
            origin: [1, -2, 3],
            classname: "a".to_owned()
        }
        .to_string(),
        "WARNING: entity at (1,-2,3) (a) has unmatched target"
    );
    assert_eq!(
        LightWarning::TooManyStyles.to_string(),
        "WARNING: Too many light styles on a face"
    );
    assert_eq!(
        Light::default().run(b"not a bsp"),
        Err::<Lighted, _>(LightError::Format)
    );
}
