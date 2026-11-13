// SPDX-License-Identifier: GPL-3.0-or-later

//! Texture alignment (MAP-12): a face's shift, rotation and scale set as an editor's texture
//! panel sets them, in either form of texture definition.

use crate::TexDef;
use crate::geometry::{Turn, cross, dot};

/// The parts of a texture definition to set; a part not given is kept (MAP-12.1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Alignment {
    /// The s and t shift.
    pub shift: Option<[f64; 2]>,
    /// The rotation in degrees.
    pub rotate: Option<f64>,
    /// The s and t scale.
    pub scale: Option<[f64; 2]>,
}

impl TexDef {
    /// This definition with the parts `alignment` gives set: in the standard form each
    /// replaces its field; in the Valve 220 form the shift is each axis's fourth value, the
    /// scale replaces the scale, and a new rotation turns both axes about their normal by the
    /// difference (MAP-12.1, MAP-12.2).
    #[must_use]
    pub fn aligned(self, alignment: Alignment) -> Self {
        match self {
            Self::Standard {
                shift,
                rotate,
                scale,
            } => Self::Standard {
                shift: alignment.shift.unwrap_or(shift),
                rotate: alignment.rotate.unwrap_or(rotate),
                scale: alignment.scale.unwrap_or(scale),
            },
            Self::Valve {
                s: [sx, sy, sz, s_kept],
                t: [tx, ty, tz, t_kept],
                rotate,
                scale,
            } => {
                let [s_shift, t_shift] = alignment.shift.unwrap_or([s_kept, t_kept]);
                let turn = alignment.rotate.map_or(0.0, |to| to - rotate);
                let [[sx, sy, sz], [tx, ty, tz]] = turned([[sx, sy, sz], [tx, ty, tz]], turn);
                Self::Valve {
                    s: [sx, sy, sz, s_shift],
                    t: [tx, ty, tz, t_shift],
                    rotate: alignment.rotate.unwrap_or(rotate),
                    scale: alignment.scale.unwrap_or(scale),
                }
            }
        }
    }
}

/// The axes `s` and `t` turned by `degrees` about their normal, `s` toward `t` (MAP-12.2):
/// a rotation about the unit normal `s x t`; axes with no normal are kept.
fn turned([s, t]: [[f64; 3]; 2], degrees: f64) -> [[f64; 3]; 2] {
    let normal = cross(s, t);
    let length = dot(normal, normal).sqrt();
    if length <= 0.0 || !length.is_finite() {
        return [s, t];
    }
    let turn = Turn::about(normal.map(|c| c / length), degrees.to_radians());
    [turn.of(s), turn.of(t)]
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::indexing_slicing,
        clippy::unreachable,
        reason = "test code: a failed index or another form is a failed test"
    )]

    use proptest::prelude::*;

    use super::Alignment;
    use crate::TexDef;

    const STANDARD: TexDef = TexDef::Standard {
        shift: [4.0, 8.0],
        rotate: 15.0,
        scale: [0.5, 2.0],
    };

    // spec MAP-12.1: each part given replaces its field; the rest are kept
    #[test]
    fn a_standard_definition_takes_the_parts_given() {
        assert_eq!(STANDARD.aligned(Alignment::default()), STANDARD);
        assert_eq!(
            STANDARD.aligned(Alignment {
                shift: Some([1.0, 2.0]),
                ..Alignment::default()
            }),
            TexDef::Standard {
                shift: [1.0, 2.0],
                rotate: 15.0,
                scale: [0.5, 2.0],
            }
        );
        assert_eq!(
            STANDARD.aligned(Alignment {
                rotate: Some(90.0),
                scale: Some([1.0, 1.0]),
                ..Alignment::default()
            }),
            TexDef::Standard {
                shift: [4.0, 8.0],
                rotate: 90.0,
                scale: [1.0, 1.0],
            }
        );
    }

    // spec MAP-12.2: a Valve definition's shift is its axes' fourth values, its scale is
    // replaced, and a new rotation turns both axes about their normal by the difference
    #[test]
    fn a_valve_definition_turns_its_axes() {
        let valve = TexDef::Valve {
            s: [1.0, 0.0, 0.0, 3.0],
            t: [0.0, 1.0, 0.0, 5.0],
            rotate: 10.0,
            scale: [1.0, 1.0],
        };
        let TexDef::Valve {
            s,
            t,
            rotate,
            scale,
        } = valve.aligned(Alignment {
            shift: Some([7.0, 9.0]),
            rotate: Some(100.0),
            scale: Some([2.0, 0.5]),
        })
        else {
            unreachable!("the form is kept");
        };
        assert_eq!((s[3], t[3], rotate, scale), (7.0, 9.0, 100.0, [2.0, 0.5]));
        let near = |a: [f64; 4], b: [f64; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-12);
        assert!(near(s, [0.0, 1.0, 0.0]), "{s:?}");
        assert!(near(t, [-1.0, 0.0, 0.0]), "{t:?}");
        assert_eq!(valve.aligned(Alignment::default()), valve);
        let flat = TexDef::Valve {
            s: [0.0; 4],
            t: [0.0; 4],
            rotate: 0.0,
            scale: [1.0, 1.0],
        };
        let TexDef::Valve { s, t, .. } = flat.aligned(Alignment {
            rotate: Some(45.0),
            ..Alignment::default()
        }) else {
            unreachable!("the form is kept");
        };
        assert_eq!((s, t), ([0.0; 4], [0.0; 4]), "no normal: the axes are kept");
    }

    proptest! {
        // spec MAP-12.2: turning keeps the axes' lengths and the angle between them
        #[test]
        fn turning_keeps_the_axes_shape(
            s in prop::array::uniform3(-4.0..4.0_f64),
            t in prop::array::uniform3(-4.0..4.0_f64),
            from in -360.0..360.0_f64,
            to in -360.0..360.0_f64,
        ) {
            let valve = TexDef::Valve {
                s: [s[0], s[1], s[2], 0.0],
                t: [t[0], t[1], t[2], 0.0],
                rotate: from,
                scale: [1.0, 1.0],
            };
            let TexDef::Valve { s: a, t: b, .. } = valve.aligned(Alignment {
                rotate: Some(to),
                ..Alignment::default()
            }) else {
                unreachable!("the form is kept");
            };
            let dot = |x: [f64; 3], y: [f64; 3]| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
            let (a, b) = ([a[0], a[1], a[2]], [b[0], b[1], b[2]]);
            prop_assert!((dot(a, a) - dot(s, s)).abs() < 1e-9);
            prop_assert!((dot(b, b) - dot(t, t)).abs() < 1e-9);
            prop_assert!((dot(a, b) - dot(s, t)).abs() < 1e-9);
        }
    }
}
