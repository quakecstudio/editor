// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "test code: a failed unwrap or index is a failed test, and the test images are \
              small"
)]

use super::{
    Cycle, Dither, Glow, NameFault, Palette, PaletteIndex, RgbaImage, TextureError, TextureKind,
    TextureName, distance,
};
use crate::write::{WadBuilder, WadWriteError};
use crate::{MapWad, WAD_TYP_MIPTEX};

/// A palette of our own, never id's: a 6 by 6 by 6 colour cube at 0 to 215, greys at 216 to
/// 223, and bright colours at 224 to 255, 255 itself magenta.
fn palette() -> Palette {
    let mut bytes = Vec::new();
    for index in 0..256_u32 {
        let colour = if index < 216 {
            [index / 36, index / 6 % 6, index % 6].map(|c| (c * 51) as u8)
        } else if index < 224 {
            let grey = ((index - 216) * 32 + 16) as u8;
            [grey; 3]
        } else if index == 255 {
            [255, 0, 255]
        } else {
            let step = ((index - 224) * 8) as u8;
            [255, step, 128]
        };
        bytes.extend_from_slice(&colour);
    }
    Palette::from_lmp(&bytes).unwrap()
}

fn name(text: &str) -> TextureName {
    TextureName::parse(text.as_bytes()).unwrap()
}

/// A `size` by `size` RGBA image whose pixel at (x, y) is `pixel(x, y)`.
fn image(size: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..size {
        for x in 0..size {
            out.extend_from_slice(&pixel(x, y));
        }
    }
    out
}

/// The opaque pixel of `palette`'s colour `index`.
fn of(palette: &Palette, index: u8) -> [u8; 4] {
    let [r, g, b] = palette.colour(PaletteIndex::new(index));
    [r, g, b, 255]
}

fn encode(
    text: &str,
    pixels: &[u8],
    dither: Dither,
    mask: Option<&[Glow]>,
) -> Result<Vec<u8>, TextureError> {
    let size = (pixels.len() / 4).isqrt() as u32;
    RgbaImage::new(size, size, pixels)?.encode(&name(text), &palette(), dither, mask)
}

// spec WAD-6.1, WAD-6.2, WAD-6.3, WAD-6.4: names the engine reads, and what it makes of each;
// a sky in any case
#[test]
fn names_are_read_as_the_engine_reads_them() {
    let cases = [
        ("brick", TextureKind::Plain),
        ("*water1", TextureKind::Liquid),
        ("sky4", TextureKind::Sky),
        ("SKY4", TextureKind::Sky),
        ("Sky_x", TextureKind::Sky),
        ("{grate", TextureKind::Fence),
        (
            "+0lava",
            TextureKind::Frame {
                frame: 0,
                cycle: Cycle::Primary,
            },
        ),
        (
            "+9x",
            TextureKind::Frame {
                frame: 9,
                cycle: Cycle::Primary,
            },
        ),
        (
            "+Jx",
            TextureKind::Frame {
                frame: 9,
                cycle: Cycle::Alternate,
            },
        ),
        (
            "+ax",
            TextureKind::Frame {
                frame: 0,
                cycle: Cycle::Alternate,
            },
        ),
        ("a23456789012345", TextureKind::Plain),
    ];
    for (text, kind) in cases {
        assert_eq!(name(text).texture_kind(), kind, "{text}");
        assert_eq!(name(text).as_bytes(), text.as_bytes());
    }
}

// spec WAD-6.1, WAD-6.2, WAD-6.3: an empty or long name, a space, a quote or a control byte,
// `//` or `;`, a bad frame, or a prefix alone in any case is refused
#[test]
fn names_the_engine_refuses_are_refused() {
    let cases: [(&[u8], NameFault); 16] = [
        (b"", NameFault::Length),
        (b"a234567890123456", NameFault::Length),
        (b"a b", NameFault::Byte(b' ')),
        (b"a\"b", NameFault::Byte(b'"')),
        (b"a\x7f", NameFault::Byte(0x7f)),
        (b"a\xe9", NameFault::Byte(0xe9)),
        (b"+kx", NameFault::Frame),
        (b"+/x", NameFault::Frame),
        (b"+0", NameFault::NoBase),
        (b"+", NameFault::NoBase),
        (b"*", NameFault::NoBase),
        (b"sky", NameFault::NoBase),
        (b"SKY", NameFault::NoBase),
        (b"//metal", NameFault::Comment),
        (b"a//b", NameFault::Comment),
        (b"a;b", NameFault::Comment),
    ];
    for (text, fault) in cases {
        let error = TextureName::parse(text).unwrap_err();
        assert_eq!(
            error,
            TextureError::Name {
                name: text.to_vec(),
                fault
            },
            "{text:?}"
        );
        assert!(!error.to_string().is_empty());
    }
}

// spec WAD-6.5: a width or height of 0 or not a multiple of 16 is refused
#[test]
fn sizes_not_on_sixteen_are_refused() {
    for (width, height) in [(0, 16), (16, 0), (24, 16), (16, 8), (17, 32)] {
        let pixels = vec![0; (width * height * 4) as usize];
        let error = RgbaImage::new(width, height, &pixels)
            .unwrap()
            .encode(&name("a"), &palette(), Dither::None, None)
            .unwrap_err();
        assert_eq!(error, TextureError::Size { width, height });
        assert!(error.to_string().contains(&format!("{width}x{height}")));
    }
}

// No spec statement: P6-15: an image or a mask whose length does not match its size is refused
#[test]
fn pixel_counts_must_match_the_size() {
    assert_eq!(
        RgbaImage::new(16, 16, &[0; 16]),
        Err(TextureError::PixelCount {
            expected: 1024,
            got: 16
        })
    );
    let pixels = image(16, |_, _| [0, 0, 0, 255]);
    assert_eq!(
        encode("a", &pixels, Dither::None, Some(&[Glow::Lit; 3])),
        Err(TextureError::PixelCount {
            expected: 256,
            got: 3
        })
    );
    assert!(
        TextureError::PixelCount {
            expected: 2,
            got: 1
        }
        .to_string()
        .contains("1 pixels")
    );
}

// spec WAD-7.2: a palette is 768 bytes
#[test]
fn a_palette_is_768_bytes() {
    for len in [0, 767, 769] {
        let error = Palette::from_lmp(&vec![0; len]).unwrap_err();
        assert_eq!(error, TextureError::Palette { len });
        assert!(error.to_string().contains(&len.to_string()));
    }
    let palette = palette();
    assert_eq!(palette.colour(PaletteIndex::new(1)), [0, 0, 51]);
    assert_eq!(palette.colour(PaletteIndex::new(255)), [255, 0, 255]);
}

// spec WAD-7.3: the distance weighs green most and red and blue by the mean red, and of equal
// distances the lower index wins
#[test]
fn the_nearest_colour_is_perceptual_and_the_lower_of_equals() {
    assert_eq!(distance([10, 20, 30], [10, 20, 30]), 0);
    assert_eq!(distance([0, 1, 0], [0, 0, 0]), 2048);
    assert_eq!(distance([1, 0, 0], [0, 0, 0]), 1025);
    assert_eq!(distance([0, 0, 1], [0, 0, 0]), 1534);
    assert_eq!(distance([255, 0, 1], [255, 0, 0]), 1534 - 510);
    let mut bytes = vec![0_u8; 768];
    bytes[5 * 3..5 * 3 + 3].copy_from_slice(&[100, 100, 100]);
    bytes[9 * 3..9 * 3 + 3].copy_from_slice(&[100, 100, 100]);
    let palette = Palette::from_lmp(&bytes).unwrap();
    assert_eq!(palette.nearest([101, 100, 100], 0..=223).unwrap().get(), 5);
    assert_eq!(palette.nearest([101, 100, 100], 6..=223).unwrap().get(), 9);
    assert_eq!(palette.nearest([0, 0, 0], 3..=4).unwrap().get(), 3);
    let (high, low) = (4, 3);
    assert_eq!(
        palette.nearest([0, 0, 0], high..=low),
        None,
        "an empty range"
    );
}

/// The level 0 indices of an encoded texture of `size` pixels square.
fn level0(encoded: &[u8], size: usize) -> &[u8] {
    &encoded[40..40 + size * size]
}

// spec WAD-7.4: ordinary pixels never take a fullbright index; marked pixels take only those,
// and in a fence texture never 255
#[test]
fn fullbright_colours_go_only_where_the_mask_asks() {
    let palette = palette();
    let bright = of(&palette, 255);
    let pixels = image(16, |x, _| if x < 8 { bright } else { of(&palette, 230) });
    let plain = encode("a", &pixels, Dither::None, None).unwrap();
    assert!(level0(&plain, 16).iter().all(|&i| i < 224));
    let mask = vec![Glow::Fullbright; 256];
    let marked = encode("a", &pixels, Dither::None, Some(&mask)).unwrap();
    assert_eq!(level0(&marked, 16)[0], 255);
    assert_eq!(level0(&marked, 16)[8], 230);
    let fence = encode("{a", &pixels, Dither::None, Some(&mask)).unwrap();
    assert!(level0(&fence, 16).iter().all(|&i| (224..255).contains(&i)));
}

// spec WAD-7.5, WAD-6.4: transparency is index 255 in a fence texture and refused elsewhere;
// alpha of 128 is opaque
#[test]
fn only_fence_textures_are_transparent() {
    let pixels = image(16, |x, y| {
        let alpha = if (x + y) % 2 == 0 { 127 } else { 128 };
        [0, 0, 0, alpha]
    });
    let error = encode("a", &pixels, Dither::None, None).unwrap_err();
    assert_eq!(error, TextureError::Transparent { count: 128 });
    assert!(error.to_string().contains("128 pixels"));
    let fence = encode("{a", &pixels, Dither::None, None).unwrap();
    let base = level0(&fence, 16);
    assert_eq!(base[0], 255);
    assert_eq!(base[1], 0);
}

// spec WAD-7.1: the header names the texture and places the four levels right after it
#[test]
fn the_header_places_the_four_levels() {
    let palette = palette();
    let pixels = image(32, |_, _| of(&palette, 7));
    let encoded = encode("brick", &pixels, Dither::None, None).unwrap();
    assert_eq!(encoded.len(), 40 + 32 * 32 / 64 * 85);
    assert_eq!(&encoded[..16], b"brick\0\0\0\0\0\0\0\0\0\0\0");
    let field = |at: usize| u32::from_le_bytes(encoded[at..at + 4].try_into().unwrap());
    assert_eq!((field(16), field(20)), (32, 32));
    assert_eq!(
        [field(24), field(28), field(32), field(36)],
        [40, 40 + 1024, 40 + 1024 + 256, 40 + 1024 + 256 + 64]
    );
    assert!(
        encoded[40..].iter().all(|&i| i == 7),
        "one colour at every level"
    );
}

// spec WAD-7.7: a block at least half transparent is transparent; one mostly fullbright takes
// the fullbright mean; otherwise the mean of its ordinary pixels
#[test]
fn mip_levels_take_each_blocks_mean() {
    let palette = palette();
    let mask: Vec<Glow> = (0..256_usize)
        .map(|at| {
            if at % 16 >= 12 {
                Glow::Fullbright
            } else {
                Glow::Lit
            }
        })
        .collect();
    let pixels = image(16, |x, y| match (x, y) {
        (0..=1, 0) => [0, 0, 0, 0],
        (12.., _) => of(&palette, 240),
        (x, _) if x % 2 == 0 => [0, 0, 0, 255],
        _ => [102, 102, 102, 255],
    });
    let fence = encode("{a", &pixels, Dither::None, Some(&mask)).unwrap();
    let level1 = &fence[40 + 256..40 + 256 + 64];
    assert_eq!(level1[0], 255, "half the block transparent");
    assert_eq!(
        level1[1],
        palette.nearest([51, 51, 51], 0..=223).unwrap().get(),
        "black and grey"
    );
    assert_eq!(level1[7], 240, "fullbright");
}

/// A gradient across the colour cube, between its colours.
fn gradient(size: u32) -> Vec<u8> {
    image(size, |x, y| {
        let scale = |v: u32| (v * 255 / (size - 1)) as u8;
        [scale(x), scale(y), scale((x + y) / 2), 255]
    })
}

/// The mean RGB distance of level 0 from the image.
fn mean_error(pixels: &[u8], encoded: &[u8], palette: &Palette) -> f64 {
    let base = &encoded[40..40 + pixels.len() / 4];
    let mut total = 0.0;
    for (pixel, &index) in pixels.as_chunks::<4>().0.iter().zip(base) {
        let colour = palette.colour(PaletteIndex::new(index));
        let squares: u32 = (0..3)
            .map(|c| {
                (i32::from(pixel[c]) - i32::from(colour[c]))
                    .unsigned_abs()
                    .pow(2)
            })
            .sum();
        total += f64::from(squares).sqrt();
    }
    total / (base.len() as f64)
}

/// id's `BestColor` match over the ordinary colours, by plain squared RGB distance: the
/// reference quantizer.
fn reference(pixels: &[u8], palette: &Palette) -> Vec<u8> {
    let mut out = vec![0; 40];
    for pixel in pixels.as_chunks::<4>().0 {
        let best = (0..=223_u8)
            .min_by_key(|&i| {
                let colour = palette.colour(PaletteIndex::new(i));
                (0..3)
                    .map(|c| (i32::from(pixel[c]) - i32::from(colour[c])).pow(2))
                    .sum::<i32>()
            })
            .unwrap();
        out.push(best);
    }
    out
}

/// The mean error of level 0 recorded for the gradient, without and with dithering: a change
/// that raises either is a regression.
const RECORDED_ERROR: f64 = 22.74;
const RECORDED_DITHERED_ERROR: f64 = 29.57;

// No spec statement: P6-15: the quantizer's mean colour error on a gradient stays within what
// is recorded, and within 10% of id's plain RGB match
#[test]
fn the_mean_colour_error_does_not_regress() {
    let palette = palette();
    let pixels = gradient(64);
    let plain = encode("a", &pixels, Dither::None, None).unwrap();
    let dithered = encode("a", &pixels, Dither::FloydSteinberg, None).unwrap();
    let ours = mean_error(&pixels, &plain, &palette);
    let ours_dithered = mean_error(&pixels, &dithered, &palette);
    let theirs = mean_error(&pixels, &reference(&pixels, &palette), &palette);
    assert!(ours <= RECORDED_ERROR, "{ours}");
    assert!(ours_dithered <= RECORDED_DITHERED_ERROR, "{ours_dithered}");
    assert!(ours <= theirs * 1.1, "{ours} against {theirs}");
}

// spec WAD-7.6: dithering spreads each pixel's error, so a flat colour between two of the
// palette's comes out as a mix of both, where without it every pixel is the nearer one
#[test]
fn dithering_mixes_the_neighbouring_colours() {
    let pixels = image(16, |_, _| [25, 25, 25, 255]);
    let plain = encode("a", &pixels, Dither::None, None).unwrap();
    let dithered = encode("a", &pixels, Dither::FloydSteinberg, None).unwrap();
    let distinct = |encoded: &[u8]| {
        let mut seen: Vec<u8> = level0(encoded, 16).to_vec();
        seen.sort_unstable();
        seen.dedup();
        seen.len()
    };
    assert_eq!(distinct(&plain), 1);
    assert!(distinct(&dithered) >= 2);
}

/// The gradient's encoding, dithered, as committed: bytes in hex, `#` starting a comment.
const GOLDEN: &str = include_str!("gradient.hex");

/// The bytes a `.hex` golden file holds.
fn golden_bytes(text: &str) -> Vec<u8> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .flat_map(str::split_whitespace)
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}

// spec WAD-7.1, WAD-7.3, WAD-7.6, WAD-7.7: the gradient's encoding is the one committed
#[test]
fn the_gradient_matches_its_golden_encoding() {
    let encoded = encode("gradient", &gradient(32), Dither::FloydSteinberg, None).unwrap();
    if std::env::var_os("QS_WRITE_GOLDEN").is_some() {
        let header: String = GOLDEN
            .lines()
            .take_while(|line| line.starts_with('#'))
            .map(|line| format!("{line}\n"))
            .collect();
        let rows: String = encoded
            .chunks(40)
            .map(|row| {
                let bytes: Vec<String> = row.iter().map(|byte| format!("{byte:02x}")).collect();
                format!("{}\n", bytes.join(" "))
            })
            .collect();
        std::fs::write(
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/texture/gradient.hex"),
            header + &rows,
        )
        .unwrap();
    }
    assert_eq!(encoded, golden_bytes(GOLDEN));
}

// spec WAD-5.1, WAD-7.1: a texture written to a WAD reads back byte for byte, its level 0 as
// a map WAD gives it
#[test]
fn a_written_texture_reads_back_byte_for_byte() {
    let encoded = encode("gradient", &gradient(32), Dither::FloydSteinberg, None).unwrap();
    let mut wad = WadBuilder::new();
    wad.add(b"gradient", WAD_TYP_MIPTEX, encoded.clone())
        .unwrap();
    let bytes = wad.to_bytes().unwrap();
    let read = MapWad::read(&bytes).unwrap();
    let texture = read.texture(b"gradient").unwrap();
    assert_eq!((texture.width, texture.height), (32, 32));
    assert_eq!(texture.name, b"gradient");
    assert_eq!(texture.pixels, &encoded[40..40 + 1024]);
    let entry = read.find(b"GRADIENT").unwrap();
    let start = usize::try_from(entry.filepos).unwrap();
    assert_eq!(&bytes[start..start + encoded.len()], encoded.as_slice());
}

// spec WAD-7.7: in 4 by 4 blocks, 8 transparent pixels of 16 make the block transparent and 7
// do not; 9 fullbright of 16 opaque take the fullbright mean and 8 do not
#[test]
fn mip_blocks_split_at_half() {
    let palette = palette();
    let bright = of(&palette, 240);
    // Block columns of level 2: 0 has 8 transparent, 1 has 7, 2 has 9 fullbright, 3 has 8.
    let counted = |x: u32, y: u32| (y % 4) * 4 + x % 4;
    let pixels = image(16, |x, y| match x / 4 {
        0 if y < 4 && counted(x, y) < 8 => [0, 0, 0, 0],
        1 if y < 4 && counted(x, y) < 7 => [0, 0, 0, 0],
        2 if y < 4 && counted(x, y) < 9 => bright,
        3 if y < 4 && counted(x, y) < 8 => bright,
        _ => [102, 102, 102, 255],
    });
    let mask: Vec<Glow> = (0..256_u32)
        .map(|at| {
            let (x, y) = (at % 16, at / 16);
            let limit = if x / 4 == 2 { 9 } else { 8 };
            if y < 4 && matches!(x / 4, 2 | 3) && counted(x, y) < limit {
                Glow::Fullbright
            } else {
                Glow::Lit
            }
        })
        .collect();
    let fence = encode("{a", &pixels, Dither::None, Some(&mask)).unwrap();
    let level2 = &fence[40 + 256 + 64..40 + 256 + 64 + 16];
    let grey = palette.nearest([102, 102, 102], 0..=223).unwrap().get();
    assert_eq!(level2[0], 255, "8 of 16 transparent");
    assert_eq!(level2[1], grey, "7 of 16 transparent");
    assert_eq!(level2[2], 240, "9 of 16 fullbright");
    assert_eq!(level2[3], grey, "8 of 16 fullbright");
}

// spec WAD-5.4, WAD-6.5: a texture whose lump would pass 32-bit sizes is refused, its size given
#[test]
fn a_texture_too_large_for_a_wad_is_refused() {
    let error = RgbaImage {
        width: 65536,
        height: 65536,
        pixels: &[],
    }
    .encode(&name("a"), &palette(), Dither::None, None)
    .unwrap_err();
    assert_eq!(
        error,
        TextureError::Size {
            width: 65536,
            height: 65536
        }
    );
}

// spec WAD-5.5: a texture lump is added under the name in its header, and one whose header names
// another texture is refused
#[test]
fn texture_lumps_go_under_their_own_names() {
    let palette = palette();
    let pixels = image(16, |_, _| of(&palette, 3));
    let lump = encode("{grate", &pixels, Dither::None, None).unwrap();
    let mut wad = WadBuilder::new();
    let error = wad.add_texture(&name("grate"), lump.clone()).unwrap_err();
    assert_eq!(
        error,
        WadWriteError::HeaderName {
            name: b"grate".to_vec()
        }
    );
    assert!(error.to_string().contains("grate"));
    assert!(wad.add_texture(&name("grate"), Vec::new()).is_err());
    wad.add_texture(&name("{grate"), lump).unwrap();
    let bytes = wad.to_bytes().unwrap();
    let read = MapWad::read(&bytes).unwrap();
    assert_eq!(read.find(b"{grate").unwrap().ty, WAD_TYP_MIPTEX);
}

/// A 16 by 16 texture lump named `name` whose pixels are the indices 0 to 255 in order.
fn every_index(name: &[u8]) -> Vec<u8> {
    let mut lump = name.to_vec();
    lump.resize(16, 0);
    for value in [16_u32, 16, 40, 40 + 256, 40 + 256 + 64, 40 + 256 + 64 + 16] {
        lump.extend_from_slice(&value.to_le_bytes());
    }
    lump.extend((0..=255_u8).collect::<Vec<_>>());
    lump
}

// spec WAD-8.1: the first level's indices, row by row, from the first offset
// spec WAD-8.2: each index is its palette colour, opaque
#[test]
fn a_texture_decodes_to_its_palette_colours() {
    let palette = palette();
    let decoded = palette.decode_miptex(&every_index(b"plain")).unwrap();
    assert_eq!((decoded.width, decoded.height), (16, 16));
    assert_eq!(decoded.rgba.len(), 256 * 4);
    for index in 0..=255_u8 {
        let at = usize::from(index) * 4;
        let [r, g, b] = palette.colour(PaletteIndex(index));
        assert_eq!(decoded.rgba[at..at + 4], [r, g, b, 255], "index {index}");
    }
}

// spec WAD-8.2: a fence texture's index 255 is transparent, and only that index
#[test]
fn a_fence_texture_hides_index_255() {
    let decoded = palette().decode_miptex(&every_index(b"{grate")).unwrap();
    assert_eq!(decoded.rgba[255 * 4 + 3], 0);
    assert!(decoded.rgba[..255 * 4].chunks(4).all(|p| p[3] == 255));
}

// spec WAD-8.1: a lump shorter than its header or its first level is refused with what it needs
#[test]
fn a_short_lump_is_refused() {
    let lump = every_index(b"plain");
    assert_eq!(
        palette().decode_miptex(&lump[..39]),
        Err(TextureError::Truncated {
            needed: 40,
            len: 39
        })
    );
    assert_eq!(
        palette().decode_miptex(&lump[..40 + 255]),
        Err(TextureError::Truncated {
            needed: 40 + 256,
            len: 40 + 255
        })
    );
    let mut far = lump.clone();
    far[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        palette().decode_miptex(&far),
        Err(TextureError::Truncated { .. })
    ));
    assert!(
        TextureError::Truncated {
            needed: 40,
            len: 39
        }
        .to_string()
        .contains("needs 40")
    );
}
