// SPDX-License-Identifier: GPL-3.0-or-later

//! UV editing (MAP-13): fitting, justifying, flipping, resetting and snapping a face's texture,
//! and carrying one face's alignment onto another so the texture continues across them.

use crate::geometry::{
    MIN_CORNERS, Plane, Turn, cross, dot, normalize, paraxial_axes, polygons_on, texvecs,
};
use crate::{Alignment, Brush, Face, TexDef};

/// A texture axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TexAxis {
    /// The s axis, across the texture.
    S,
    /// The t axis, down the texture.
    T,
}

impl TexAxis {
    const fn index(self) -> usize {
        match self {
            Self::S => 0,
            Self::T => 1,
        }
    }
}

/// Where a justified texture meets the face (MAP-13.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Justify {
    /// The texture's left edge on the face's least s.
    Left,
    /// The texture's right edge on the face's greatest s.
    Right,
    /// The texture's top edge on the face's least t.
    Top,
    /// The texture's bottom edge on the face's greatest t.
    Bottom,
    /// The texture's centre on the centre of the face's extent.
    Centre,
}

/// An edit of a face's texture alignment (MAP-13).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UvEdit {
    /// Stretch or shrink the texture so one copy spans the face (MAP-13.2).
    Fit,
    /// Shift the texture to an edge or the centre of the face (MAP-13.3).
    Justify(Justify),
    /// Mirror the texture along an axis about the face's centre (MAP-13.4).
    Flip(TexAxis),
    /// No shift, rotation or scaling (MAP-13.5).
    Reset,
    /// Shift the texture so its nearest edge lands on the nearest corner (MAP-13.6).
    Snap,
    /// Add to the shift and the rotation and multiply the scale, as a drag in the UV view
    /// does (MAP-13.11).
    Adjust {
        /// Added to the s and t shift.
        shift: [f64; 2],
        /// Added to the rotation, in degrees.
        rotate: f64,
        /// The s and t scale are multiplied by these.
        scale: [f64; 2],
    },
}

/// A texture's width and height in texels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureSize([u32; 2]);

impl TextureSize {
    /// The size of a texture `width` by `height` texels.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self([width, height])
    }

    /// Its extent along each axis.
    fn along(self) -> [f64; 2] {
        self.0.map(f64::from)
    }
}

/// How near 0 a length is for a direction or extent to count as none.
const NONE: f64 = 1e-9;
/// How near a whole number of degrees a computed rotation is taken as it.
const WHOLE_DEGREES: f64 = 1e-6;
/// The origin, a point of every line through it.
const ORIGIN: [f64; 3] = [0.0, 0.0, 0.0];
/// Degrees in a turn.
const TURN: f64 = 360.0;

/// A face's texture as a projection: texel `i` of point `p` is `dot(p, axes[i]) / scale[i] +
/// shift[i]` (MAP-4.2 to MAP-4.4, MAP-5.3).
struct Projection {
    axes: [[f64; 3]; 2],
    scale: [f64; 2],
    shift: [f64; 2],
}

impl Projection {
    /// The projection of `texdef` on a plane facing `normal`; none for a Valve 220 scale of 0
    /// (MAP-5.4).
    fn of(texdef: &TexDef, normal: [f64; 3]) -> Option<Self> {
        match *texdef {
            TexDef::Standard {
                shift,
                rotate,
                scale,
            } => {
                let [[sx, sy, sz, _], [tx, ty, tz, _]] =
                    texvecs(normal, [0.0; 2], rotate, [1.0; 2]);
                Some(Self {
                    axes: [[sx, sy, sz], [tx, ty, tz]],
                    scale: scale.map(|value| if value == 0.0 { 1.0 } else { value }),
                    shift,
                })
            }
            TexDef::Valve {
                s: [sx, sy, sz, s_shift],
                t: [tx, ty, tz, t_shift],
                scale,
                ..
            } => {
                if scale.contains(&0.0) {
                    return None;
                }
                Some(Self {
                    axes: [[sx, sy, sz], [tx, ty, tz]],
                    scale,
                    shift: [s_shift, t_shift],
                })
            }
        }
    }

    /// `point` along axis `i`, before the scale and the shift.
    fn along(&self, i: usize, point: [f64; 3]) -> f64 {
        self.axes.get(i).map_or(0.0, |&axis| dot(point, axis))
    }

    /// The least and greatest of `polygon` along axis `i`, before the scale and the shift.
    fn extent(&self, i: usize, polygon: &[[f64; 3]]) -> Option<[f64; 2]> {
        let mut found: Option<[f64; 2]> = None;
        for &point in polygon {
            let at = self.along(i, point);
            found = Some(found.map_or([at, at], |[lo, hi]| [lo.min(at), hi.max(at)]));
        }
        found
    }
}

/// `shift` with whole copies of `size` taken off, so it lies in `[0, size)`; as it is for a
/// size of 0.
fn wrapped(shift: f64, size: f64) -> f64 {
    if size > 0.0 {
        shift.rem_euclid(size)
    } else {
        shift
    }
}

/// `value` as the nearest whole number when it is within [`WHOLE_DEGREES`] of one.
fn tidied(value: f64) -> f64 {
    let whole = value.round();
    if (value - whole).abs() < WHOLE_DEGREES {
        whole
    } else {
        value
    }
}

impl Face {
    /// The texture definition `edit` gives the face, whose polygon is `polygon` and whose
    /// texture is `size`; none when the face has no plane or no projection (MAP-5.4), and the
    /// definition as it is along an axis the face has no extent on.
    ///
    /// Implements MAP-13.1 to MAP-13.6.
    #[must_use]
    pub fn uv_edited(
        &self,
        polygon: &[[f64; 3]],
        edit: UvEdit,
        size: TextureSize,
    ) -> Option<TexDef> {
        let normal = self.plane()?.normal;
        let texdef = *self.texdef();
        match edit {
            UvEdit::Reset => return Some(texdef.reset(normal)),
            UvEdit::Adjust {
                shift,
                rotate,
                scale,
            } => return Some(texdef.adjusted(shift, rotate, scale)),
            UvEdit::Fit | UvEdit::Justify(_) | UvEdit::Flip(_) | UvEdit::Snap => {}
        }
        let projection = Projection::of(&texdef, normal)?;
        let size = size.along();
        let mut shift = projection.shift;
        let mut scale = projection.scale;
        for i in 0..2 {
            let (Some(extent), Some(&length)) = (projection.extent(i, polygon), size.get(i)) else {
                continue;
            };
            let [raw_lo, raw_hi] = extent;
            let (Some(shift), Some(scale)) = (shift.get_mut(i), scale.get_mut(i)) else {
                continue;
            };
            let (lo, hi) = {
                let (a, b) = (raw_lo / *scale, raw_hi / *scale);
                (a.min(b), a.max(b))
            };
            match edit {
                UvEdit::Fit => {
                    let span = raw_hi - raw_lo;
                    if span <= NONE || length <= 0.0 {
                        continue;
                    }
                    let fitted = (span / length).copysign(*scale);
                    let least = (raw_lo / fitted).min(raw_hi / fitted);
                    *scale = fitted;
                    *shift = wrapped(-least, length);
                }
                UvEdit::Justify(to) => {
                    let target = match (to, i) {
                        (Justify::Left, 0) | (Justify::Top, 1) => Some(-lo),
                        (Justify::Right, 0) | (Justify::Bottom, 1) => Some(length - hi),
                        (Justify::Centre, _) => Some(length / 2.0 - (lo + hi) / 2.0),
                        (Justify::Left | Justify::Right | Justify::Top | Justify::Bottom, _) => {
                            None
                        }
                    };
                    if let Some(target) = target {
                        *shift = wrapped(target, length);
                    }
                }
                UvEdit::Flip(axis) => {
                    if axis.index() == i {
                        *shift += lo + hi;
                        *scale = -*scale;
                    }
                }
                UvEdit::Snap => {
                    if length <= 0.0 {
                        continue;
                    }
                    let mut nearest: Option<f64> = None;
                    for &point in polygon {
                        let at = projection.along(i, point) / *scale + *shift;
                        let to_line = (at / length).round() * length - at;
                        if nearest.is_none_or(|best| to_line.abs() < best.abs()) {
                            nearest = Some(to_line);
                        }
                    }
                    *shift = wrapped(*shift + nearest.unwrap_or_default(), length);
                }
                UvEdit::Reset | UvEdit::Adjust { .. } => {}
            }
        }
        Some(texdef.aligned(Alignment {
            shift: Some(shift),
            rotate: None,
            scale: Some(scale),
        }))
    }

    /// The texture definition that carries `source`'s texture onto this face, in this face's
    /// form, so that every point of the line the two planes share has the same texel on both;
    /// none when either face has no plane or `source` no projection (MAP-5.4).
    ///
    /// Implements MAP-13.7 to MAP-13.9.
    #[must_use]
    pub fn aligned_from(&self, source: &Self) -> Option<TexDef> {
        let to = self.plane()?;
        let from = source.plane()?;
        let given = Projection::of(source.texdef(), from.normal)?;
        let [[sx, sy, sz, s_shift], [tx, ty, tz, t_shift]] = source.texvecs()?;
        let scaled = [[sx, sy, sz], [tx, ty, tz]];
        let shifts = [s_shift, t_shift];
        let rotate = match *source.texdef() {
            TexDef::Standard { rotate, .. } | TexDef::Valve { rotate, .. } => rotate,
        };
        let line = Line::shared(&from, &to);
        match *self.texdef() {
            TexDef::Valve { .. } => {
                let turn = line.map_or(Turn::NONE, |line| Turn::between(&from, &to, line.along));
                let point = line.map_or(ORIGIN, |line| line.point);
                let field = |axis: [f64; 3], shift: f64, scale: f64| {
                    let [x, y, z] = turn.of(axis);
                    let shift = shift + dot(point, axis) - dot(point, [x, y, z]);
                    [x * scale, y * scale, z * scale, shift]
                };
                let ([s_axis, t_axis], [s_scale, t_scale]) = (scaled, given.scale);
                let (s, t) = (
                    field(s_axis, s_shift, s_scale),
                    field(t_axis, t_shift, t_scale),
                );
                Some(TexDef::Valve {
                    s,
                    t,
                    rotate,
                    scale: given.scale,
                })
            }
            TexDef::Standard { .. } => {
                if line.is_none()
                    && let TexDef::Standard { .. } = source.texdef()
                {
                    return Some(*source.texdef());
                }
                let line = line.unwrap_or_else(|| Line::within(&to));
                Some(line.standard_on(&to, scaled, shifts, given.scale))
            }
        }
    }
}

/// A line: a point on it and its unit direction.
#[derive(Clone, Copy)]
struct Line {
    point: [f64; 3],
    along: [f64; 3],
}

impl Line {
    /// The line two planes share; none when they are parallel.
    fn shared(a: &Plane<f64>, b: &Plane<f64>) -> Option<Self> {
        let along = cross(a.normal, b.normal);
        let length = dot(along, along);
        if length.sqrt() <= NONE {
            return None;
        }
        let towards = [0, 1, 2].map(|i| {
            let (an, bn) = (
                a.normal.get(i).copied().unwrap_or_default(),
                b.normal.get(i).copied().unwrap_or_default(),
            );
            a.dist * bn - b.dist * an
        });
        let point = cross(towards, along).map(|c| c / length);
        Some(Self {
            point,
            along: normalize(along),
        })
    }

    /// The standard definition on the plane `to` whose texels along the line are those of the
    /// scaled axes `scaled` with offsets `shifts`, its scales in the proportion of `scale`
    /// (MAP-13.8).
    fn standard_on(
        self,
        to: &Plane<f64>,
        scaled: [[f64; 3]; 2],
        shifts: [f64; 2],
        scale: [f64; 2],
    ) -> TexDef {
        let [base_s, base_t] = paraxial_axes(to.normal);
        let e = self.along;
        // In every row of MAP-4.2 the s axis's one component is 1 and the t axis's -1, so in
        // the base axes' own coordinates MAP-4.3's rotation turns from t toward s.
        let edge = [dot(e, base_s), dot(e, base_t)];
        let [scaled_s, scaled_t] = scaled;
        let wanted = [scale[0] * dot(e, scaled_s), scale[1] * dot(e, scaled_t)];
        let wanted_length = wanted[0].hypot(wanted[1]);
        // The line lies in the plane, which its projection axis does not, so the line always
        // has a part along the plane's two base axes.
        let edge_length = edge[0].hypot(edge[1]);
        let (rotate, scale) = if wanted_length <= NONE {
            (0.0, scale)
        } else {
            let degrees = (wanted[1].atan2(wanted[0]) - edge[1].atan2(edge[0])).to_degrees();
            let k = edge_length / wanted_length;
            (
                tidied(degrees).rem_euclid(TURN),
                scale.map(|value| value * k),
            )
        };
        let [[sx, sy, sz, _], [tx, ty, tz, _]] = texvecs(to.normal, [0.0; 2], rotate, scale);
        let point = self.point;
        let shift = [
            dot(point, scaled_s) + shifts[0] - dot(point, [sx, sy, sz]),
            dot(point, scaled_t) + shifts[1] - dot(point, [tx, ty, tz]),
        ];
        TexDef::Standard {
            shift,
            rotate,
            scale,
        }
    }

    /// A line in `plane` along its paraxial s axis, through its point nearest the origin.
    fn within(plane: &Plane<f64>) -> Self {
        let [s, _] = paraxial_axes(plane.normal);
        let into = dot(s, plane.normal);
        let along = normalize([0, 1, 2].map(|i| {
            s.get(i).copied().unwrap_or_default()
                - into * plane.normal.get(i).copied().unwrap_or_default()
        }));
        Self {
            point: plane.normal.map(|c| c * plane.dist),
            along,
        }
    }
}

impl Turn {
    /// The rotation about `axis`, the direction of the line `from` and `to` share, taking
    /// `from`'s normal onto `to`'s.
    fn between(from: &Plane<f64>, to: &Plane<f64>, axis: [f64; 3]) -> Self {
        let across = cross(from.normal, to.normal);
        Self::about(axis, dot(across, axis).atan2(dot(from.normal, to.normal)))
    }
}

impl TexDef {
    /// The definition with `shift` added to its shift, `rotate` to its rotation and its scale
    /// multiplied by `scale`, in the same form; a standard scale of 0 is taken as 1 (MAP-4.4),
    /// and a Valve 220 rotation turns its axes (MAP-12.2).
    #[must_use]
    pub fn adjusted(self, shift: [f64; 2], rotate: f64, scale: [f64; 2]) -> Self {
        let (now_shift, now_rotate, now_scale) = match self {
            Self::Standard {
                shift,
                rotate,
                scale,
            } => (
                shift,
                rotate,
                scale.map(|value| if value == 0.0 { 1.0 } else { value }),
            ),
            Self::Valve {
                s,
                t,
                rotate,
                scale,
                ..
            } => {
                let [.., s_shift] = s;
                let [.., t_shift] = t;
                ([s_shift, t_shift], rotate, scale)
            }
        };
        let [ss, ts] = now_shift;
        let [sk, tk] = now_scale;
        self.aligned(Alignment {
            shift: Some([ss + shift[0], ts + shift[1]]),
            rotate: Some(now_rotate + rotate),
            scale: Some([sk * scale[0], tk * scale[1]]),
        })
    }

    /// The definition with no shift, rotation or scaling, in the same form: a Valve 220 face
    /// takes the paraxial axes of its plane, facing `normal` (MAP-13.5).
    #[must_use]
    pub fn reset(self, normal: [f64; 3]) -> Self {
        match self {
            Self::Standard { .. } => Self::Standard {
                shift: [0.0; 2],
                rotate: 0.0,
                scale: [1.0; 2],
            },
            Self::Valve { .. } => {
                let [[sx, sy, sz], [tx, ty, tz]] = paraxial_axes(normal);
                Self::Valve {
                    s: [sx, sy, sz, 0.0],
                    t: [tx, ty, tz, 0.0],
                    rotate: 0.0,
                    scale: [1.0; 2],
                }
            }
        }
    }
}

impl Brush {
    /// Each face's polygon on the brush's solid, in the order of its faces: none for a face
    /// with no plane or nothing on the solid (MAP-3.4).
    #[must_use]
    pub fn face_polygons(&self) -> Vec<Option<Vec<[f64; 3]>>> {
        let planes: Vec<Option<Plane<f64>>> = self.faces.iter().map(Face::plane).collect();
        let known: Vec<Plane<f64>> = planes.iter().flatten().copied().collect();
        let mut polygons = polygons_on(&known).into_iter();
        planes
            .iter()
            .map(|plane| {
                plane
                    .and_then(|_| polygons.next())
                    .filter(|polygon| polygon.len() >= MIN_CORNERS)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
