// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::float_cmp,
    reason = "test code: exact inputs give exact floats"
)]

use super::Look;
use crate::mesh::GAMMA;

// spec REND-12.1: QuakeSpasm's look is the default
#[test]
fn quakespasm_is_the_default_look() {
    assert_eq!(Look::default(), Look::QuakeSpasm);
}

// spec REND-12.2: QuakeSpasm's gamma leaves the palette as it is; GLQuake's is 0.7
#[test]
fn each_look_has_its_gamma() {
    assert_eq!(Look::QuakeSpasm.gamma(), 1.0);
    assert_eq!(Look::GlQuake.gamma(), GAMMA);
}

// spec REND-12.5: overbright shifts by 8 and doubles the light; GLQuake shifts by 7
#[test]
fn overbright_halves_the_lightmap_and_doubles_it_back() {
    assert_eq!(
        (Look::QuakeSpasm.light_shift(), Look::QuakeSpasm.light_scale()),
        (8, 2.0)
    );
    assert_eq!(
        (Look::GlQuake.light_shift(), Look::GlQuake.light_scale()),
        (7, 1.0)
    );
}

// spec REND-12.3, REND-12.4: QuakeSpasm filters between mip levels and lights fullbrights
#[test]
fn quakespasm_filters_trilinearly_and_glows() {
    assert!(Look::QuakeSpasm.trilinear() && Look::QuakeSpasm.fullbrights());
    assert!(!Look::GlQuake.trilinear() && !Look::GlQuake.fullbrights());
}

// spec REND-12.6: AdaptFovx keeps a 4:3 image's height: 90 across at 4:3, about 100.39 at
// 16:10 and 106.26 at 16:9; GLQuake keeps 90; an aspect that is not positive keeps it too
#[test]
fn the_view_widens_with_the_image() {
    assert_eq!(Look::QuakeSpasm.fov_x(90.0, 4.0 / 3.0), 90.0);
    let wide = Look::QuakeSpasm.fov_x(90.0, 1.6);
    assert!((wide - 100.388_6).abs() < 1e-3, "{wide}");
    let wider = Look::QuakeSpasm.fov_x(90.0, 16.0 / 9.0);
    assert!((wider - 106.260_2).abs() < 1e-3, "{wider}");
    assert!(Look::QuakeSpasm.fov_x(90.0, 0.75) < 90.0, "a tall image narrows");
    assert_eq!(Look::GlQuake.fov_x(90.0, 1.6), 90.0);
    assert_eq!(Look::QuakeSpasm.fov_x(90.0, 0.0), 90.0);
    assert_eq!(Look::QuakeSpasm.fov_x(90.0, -1.0), 90.0);
}
