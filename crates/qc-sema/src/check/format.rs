// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_VerifyFormatString, QCC_VerifyArgs_sendevent, QCC_VerifyArgs_setviewprop, QCC_VerifyArgs_cvar, QCC_VerifyArgs_MatchingFieldType

//! The checks FTEQCC makes of some builtins' arguments (FTEF-33): `sprintf`'s format string
//! against its arguments, `sendevent`'s argument type list, `setviewprop`'s property, `cvar`'s
//! deprecated names, and `addstat`'s field type.

use qc_diag::{Code, Span};

use crate::cast::ConstType;
use crate::check::Value;
use crate::check::etype::Etype;
use crate::check::expr::Checker;
use crate::ty::Type;

/// `setviewprop`'s properties, each with its number and the basic types of up to three values.
const VIEW_PROPERTIES: &[(&str, i32, &[Etype])] = &[
    ("VF_MIN", 1, &[Etype::Vector]),
    ("VF_MIN_X", 2, &[Etype::Float]),
    ("VF_MIN_Y", 3, &[Etype::Float]),
    ("VF_SIZE", 4, &[Etype::Vector]),
    ("VF_SIZE_X", 5, &[Etype::Float]),
    ("VF_SIZE_Y", 6, &[Etype::Float]),
    ("VF_VIEWPORT", 7, &[Etype::Vector, Etype::Vector]),
    ("VF_FOV", 8, &[Etype::Vector]),
    ("VF_FOVX", 9, &[Etype::Float]),
    ("VF_FOVY", 10, &[Etype::Float]),
    ("VF_ORIGIN", 11, &[Etype::Vector]),
    ("VF_ORIGIN_X", 12, &[Etype::Float]),
    ("VF_ORIGIN_Y", 13, &[Etype::Float]),
    ("VF_ORIGIN_Z", 14, &[Etype::Float]),
    ("VF_ANGLES", 15, &[Etype::Vector]),
    ("VF_ANGLES_X", 16, &[Etype::Float]),
    ("VF_ANGLES_Y", 17, &[Etype::Float]),
    ("VF_ANGLES_Z", 18, &[Etype::Float]),
    ("VF_DRAWWORLD", 19, &[Etype::Float]),
    ("VF_ENGINESBAR", 20, &[Etype::Float]),
    ("VF_DRAWCROSSHAIR", 21, &[Etype::Float]),
    ("VF_MINDIST", 23, &[Etype::Float]),
    ("VF_MAXDIST", 24, &[Etype::Float]),
    ("VF_CL_VIEWANGLES_V", 33, &[Etype::Vector]),
    ("VF_CL_VIEWANGLES_X", 34, &[Etype::Float]),
    ("VF_CL_VIEWANGLES_X", 35, &[Etype::Float]),
    ("VF_CL_VIEWANGLES_X", 36, &[Etype::Float]),
    ("VF_PERSPECTIVE", 200, &[Etype::Float]),
    ("VF_ACTIVESEAT", 202, &[Etype::Float, Etype::Float]),
    ("VF_AFOV", 203, &[Etype::Float]),
    ("VF_VIEWENTITY", 206, &[Etype::Float]),
    ("VF_RT_SOURCECOLOUR", 209, &[Etype::String]),
    (
        "VF_RT_DEPTH",
        210,
        &[Etype::String, Etype::Float, Etype::Vector],
    ),
    (
        "VF_RT_RIPPLE",
        211,
        &[Etype::String, Etype::Float, Etype::Vector],
    ),
    (
        "VF_RT_DESTCOLOUR0",
        212,
        &[Etype::String, Etype::Float, Etype::Vector],
    ),
    (
        "VF_RT_DESTCOLOUR1",
        213,
        &[Etype::String, Etype::Float, Etype::Vector],
    ),
    (
        "VF_RT_DESTCOLOUR2",
        214,
        &[Etype::String, Etype::Float, Etype::Vector],
    ),
    (
        "VF_RT_DESTCOLOUR3",
        215,
        &[Etype::String, Etype::Float, Etype::Vector],
    ),
    (
        "VF_RT_DESTCOLOUR4",
        216,
        &[Etype::String, Etype::Float, Etype::Vector],
    ),
    (
        "VF_RT_DESTCOLOUR5",
        217,
        &[Etype::String, Etype::Float, Etype::Vector],
    ),
    (
        "VF_RT_DESTCOLOUR6",
        218,
        &[Etype::String, Etype::Float, Etype::Vector],
    ),
    (
        "VF_RT_DESTCOLOUR7",
        219,
        &[Etype::String, Etype::Float, Etype::Vector],
    ),
    ("VF_ENVMAP", 220, &[Etype::String]),
    ("VF_USERDATA", 221, &[Etype::Pointer, Etype::Uint]),
    ("VF_SKYROOM_CAMERA", 222, &[Etype::Vector]),
    ("VF_PROJECTIONOFFSET", 224, &[Etype::Vector]),
    ("VF_VRBASEORIENTATION", 225, &[Etype::Vector, Etype::Vector]),
    ("VF_DP_MAINVIEW", 400, &[Etype::Float]),
];

/// `etype_t`'s basic types in order, as `addstat`'s type argument numbers them.
const BASIC_TYPES: [Etype; 13] = [
    Etype::Void,
    Etype::String,
    Etype::Float,
    Etype::Vector,
    Etype::Entity,
    Etype::Field,
    Etype::Function,
    Etype::Pointer,
    Etype::Integer,
    Etype::Uint,
    Etype::Int64,
    Etype::Uint64,
    Etype::Double,
];

impl Checker<'_> {
    /// FTEF-33: the checks of the builtin `name`'s arguments `args`, each of the type it is
    /// passed as.
    pub(super) fn verify_arguments(&mut self, name: &str, args: &[Value], span: Span) {
        match name {
            "sprintf" => self.verify_format(name, args, span),
            "cvar" if args.len() == 1 => self.verify_cvar(name, args, span),
            "clientstat" | "addstat" if args.len() == 3 => self.verify_stat(name, args, span),
            "setviewprop" | "setproperty" => self.verify_view_property(name, args, span),
            "sendevent" => self.verify_event(name, args, span),
            _ => {}
        }
    }

    /// The basic type of argument `at`, a truth value its parent's, `void` past the last.
    fn argument_type(&self, args: &[Value], at: usize) -> Etype {
        args.get(at)
            .and_then(|value| self.etype_of(&value.ty))
            .unwrap_or(Etype::Void)
    }

    /// The type name of argument `at`, `void` past the last.
    fn argument_name(&self, args: &[Value], at: usize) -> String {
        args.get(at)
            .map_or_else(|| "void".to_owned(), |value| self.env.type_name(&value.ty))
    }

    /// FTEF-33.1: `QCC_VerifyFormatString`: a format string that is a constant is read
    /// directive by directive, each one's argument checked against its conversion; a surplus
    /// argument, a bad directive, a width or precision `*` that is no float, and a placeholder
    /// length modifier warn. `F324`.
    fn verify_format(&mut self, name: &str, args: &[Value], span: Span) {
        let Some(format) = args.first().and_then(|f| f.text.clone()) else {
            return;
        };
        let mut cursor = Cursor {
            at: 0,
            position: 1,
            last: 1,
        };
        let bytes = format.as_bytes();
        loop {
            match bytes.get(cursor.at) {
                None => break,
                Some(b'%') if bytes.get(cursor.at.saturating_add(1)) == Some(&b'%') => {
                    cursor.at = cursor.at.saturating_add(2);
                }
                Some(b'%') => {
                    if !self.format_directive(name, &format, args, &mut cursor, span) {
                        return;
                    }
                }
                Some(_) => cursor.at = cursor.at.saturating_add(1),
            }
        }
        if cursor.position < args.len() && cursor.last < args.len() {
            let message = format!(
                "{name}: surplus trailing {} argument(s) for format \"{format}\"",
                self.argument_name(args, cursor.position)
            );
            self.env.report(Code::W0254, span, message);
        }
    }

    /// One directive of `format`, from its `%` at `cursor.at`: its argument position, flags,
    /// width, precision and length modifiers, then its conversion against the argument's type.
    /// Whether the string is read on; an invalid directive ends it.
    fn format_directive(
        &mut self,
        name: &str,
        format: &str,
        args: &[Value],
        cursor: &mut Cursor,
        span: Span,
    ) -> bool {
        let bytes = format.as_bytes();
        let start = cursor.at;
        let invalid = |checker: &mut Self| {
            let rest = format.get(start..).unwrap_or_default();
            let message = format!("{name}: invalid format string: {rest}");
            checker.env.report(Code::W0254, span, message);
            false
        };
        let mut at = start.saturating_add(1);
        let mut this = None;
        let mut has_width = false;
        if bytes.get(at).is_some_and(u8::is_ascii_digit) {
            let (number, end) = digits(bytes, at);
            if bytes.get(end) == Some(&b'$') {
                this = Some(number);
                at = end.saturating_add(1);
            } else {
                has_width = !(bytes.get(at) == Some(&b'0') && number == 0);
                at = end;
            }
        }
        if !has_width {
            while matches!(bytes.get(at), Some(b'#' | b'0' | b'-' | b' ' | b'+')) {
                at = at.saturating_add(1);
            }
            if bytes.get(at) == Some(&b'*') {
                let Some(next) = self.star(name, "width", bytes, at, args, cursor, span) else {
                    return invalid(self);
                };
                at = next;
            } else if bytes.get(at).is_some_and(u8::is_ascii_digit) {
                at = digits(bytes, at).1;
            }
        }
        if bytes.get(at) == Some(&b'.') {
            at = at.saturating_add(1);
            if bytes.get(at) == Some(&b'*') {
                let Some(next) = self.star(name, "precision", bytes, at, args, cursor, span) else {
                    return invalid(self);
                };
                at = next;
            } else if bytes.get(at).is_some_and(u8::is_ascii_digit) {
                at = digits(bytes, at).1;
            } else {
                return invalid(self);
            }
        }
        let mut float = None;
        let mut wide = false;
        loop {
            match bytes.get(at) {
                Some(b'h') => float = Some(true),
                Some(b'l' | b'L') => float = Some(false),
                Some(b'q') => wide = true,
                Some(&c @ (b'j' | b'z' | b't')) => {
                    let message =
                        format!("{name}: length modifier {} is a placeholder", char::from(c));
                    self.env.report(Code::W0254, span, message);
                }
                _ => break,
            }
            at = at.saturating_add(1);
        }
        let Some(&conversion) = bytes.get(at) else {
            return invalid(self);
        };
        if matches!(conversion, b'p' | b'P' | b'i' | b'I') && float.is_none() {
            float = Some(false);
        }
        let this = this.unwrap_or_else(|| cursor.next());
        cursor.last = cursor.last.max(this.saturating_add(1));
        let written = format.get(start..=at).unwrap_or_default();
        let given = self.argument_type(args, this);
        let got = self.argument_name(args, this);
        let arg = this.saturating_add(1);
        let Some(requirement) = requirement(conversion, float.unwrap_or(true), wide, given) else {
            return invalid(self);
        };
        if let Requirement::Twice(kind) = requirement {
            let message = format!("{name}: {written} requires {kind} at arg {arg} (got {got})");
            self.env.report(Code::W0254, span, message);
        }
        if let Requirement::Once(kind) | Requirement::Twice(kind) = requirement {
            let message = format!("{name}: {written} requires {kind} at arg {arg} (got {got})");
            self.env.report(Code::W0254, span, message);
        }
        cursor.at = at.saturating_add(1);
        true
    }

    /// A `*` width or precision at `at`: its argument, the next or one written `N$`, which must
    /// be a float. Where the directive goes on, `None` for a number with no `$`.
    #[expect(
        clippy::too_many_arguments,
        reason = "spec FTEF-33.1: one step of QCC_VerifyFormatString's directive"
    )]
    fn star(
        &mut self,
        name: &str,
        what: &str,
        bytes: &[u8],
        at: usize,
        args: &[Value],
        cursor: &mut Cursor,
        span: Span,
    ) -> Option<usize> {
        let mut at = at.saturating_add(1);
        let argument = if bytes.get(at).is_some_and(u8::is_ascii_digit) {
            let (number, end) = digits(bytes, at);
            if bytes.get(end) != Some(&b'$') {
                return None;
            }
            at = end.saturating_add(1);
            number
        } else {
            cursor.next()
        };
        if self.argument_type(args, argument) != Etype::Float {
            let message = format!(
                "{name}: {what} modifier requires float at arg {}",
                argument.saturating_add(1)
            );
            self.env.report(Code::W0254, span, message);
        }
        Some(at)
    }

    /// FTEF-33.2: `QCC_VerifyArgs_cvar`: `cvar("vid_conwidth")` and `cvar("vid_conheight")`
    /// are deprecated. `F333`.
    fn verify_cvar(&mut self, name: &str, args: &[Value], span: Span) {
        let Some(cvar) = args.first().and_then(|a| a.text.clone()) else {
            return;
        };
        if matches!(&*cvar, "vid_conwidth" | "vid_conheight") {
            let message = format!(
                "{name}: cvar(\"{cvar}\") is deprecated and is likely to give some hacky value to \
                 work around old API usage which does not necessarily reflect the actual cvar \
                 value. Use getviewprop(VF_SCREENVSIZE) for the screen's virtual use, or use \
                 ftos+cvar_string to read the actual value of the cvar, or cast to variant to mute \
                 this warning."
            );
            self.env.report(Code::W0255, span, message);
        }
    }

    /// FTEF-33.2: `QCC_VerifyArgs_MatchingFieldType`: `addstat`'s constant type number must
    /// be the field's basic type, unless the field holds a `variant`.
    fn verify_stat(&mut self, name: &str, args: &[Value], span: Span) {
        let (Some(kind), Some(field)) = (args.get(1), args.get(2)) else {
            return;
        };
        let Type::Field(held) = &field.ty else {
            return;
        };
        let Some(constant) = kind.known() else {
            return;
        };
        let number = match constant.ty {
            ConstType::Integer | ConstType::Uint => i64::from(constant.i()),
            ConstType::Float => f32_whole(constant.f()),
            _ => return,
        };
        let Some(held_etype) = self.etype_of(held) else {
            return;
        };
        if held_etype == Etype::Variant {
            return;
        }
        let indicated = usize::try_from(number)
            .ok()
            .and_then(|n| BASIC_TYPES.get(n).copied());
        if indicated == Some(held_etype) {
            return;
        }
        let message = match indicated {
            Some(basic) => format!(
                "{name}: indicated type ev_{} does not match passed field type .{}",
                basic.basic_name(),
                self.env.type_name(held)
            ),
            None => format!("{name}: indicated type {number} is not a basic type"),
        };
        self.env.report(Code::W0255, span, message);
    }

    /// FTEF-33.2: `QCC_VerifyArgs_setviewprop`: a constant property's values must be of its
    /// types; an unknown one warns "unknown argument N".
    fn verify_view_property(&mut self, name: &str, args: &[Value], span: Span) {
        let Some(property) = args.first().and_then(Value::known) else {
            return;
        };
        let number = f32_whole(property.f());
        let Some((property, _, types)) = VIEW_PROPERTIES
            .iter()
            .find(|(_, n, _)| i64::from(*n) == number)
        else {
            let message = format!("{name}: unknown argument {number}");
            self.env.report(Code::W0255, span, message);
            return;
        };
        let prefixes = ["", "X, ", "X, Y, "];
        for (at, prefix) in prefixes.iter().enumerate() {
            let argument = at.saturating_add(1);
            if argument >= args.len() {
                return;
            }
            let expected = types.get(at).copied().unwrap_or(Etype::Void);
            if self.argument_type(args, argument) != expected {
                let message = format!(
                    "{name}({property}, {prefix}...): expected {}, got {}",
                    expected.basic_name(),
                    self.argument_name(args, argument)
                );
                self.env.report(Code::W0255, span, message);
                return;
            }
        }
    }

    /// FTEF-33.2: `QCC_VerifyArgs_sendevent`: a constant argument type list names each
    /// argument after the second's type; a list longer than the arguments, a letter that is no
    /// type, a type that differs, and more than 8 arguments warn.
    fn verify_event(&mut self, name: &str, args: &[Value], span: Span) {
        let Some(types) = args.get(1).and_then(|a| a.text.clone()) else {
            return;
        };
        let mut argument = 2_usize;
        for letter in types.chars() {
            let wanted = match letter {
                's' => Some(Etype::String),
                'f' => Some(Etype::Float),
                'F' => Some(Etype::Double),
                'i' => Some(Etype::Integer),
                'I' => Some(Etype::Int64),
                'u' => Some(Etype::Uint),
                'U' => Some(Etype::Uint64),
                'v' => Some(Etype::Vector),
                'e' => Some(Etype::Entity),
                _ => None,
            };
            if argument >= args.len() {
                let message = format!("{name}: arg type list longer than args");
                self.env.report(Code::W0255, span, message);
                break;
            }
            match wanted {
                None => {
                    let message = format!("{name}: '{letter}' is not a recognised arg type");
                    self.env.report(Code::W0255, span, message);
                }
                Some(wanted) if self.argument_type(args, argument) != wanted => {
                    let message = format!(
                        "{name}: '{letter}' specified, but was passed {}",
                        self.argument_name(args, argument)
                    );
                    self.env.report(Code::W0255, span, message);
                }
                Some(_) => {}
            }
            argument = argument.saturating_add(1);
        }
        if argument > 8 {
            let message = format!("{name}: cannot pass more than 8 args to a builtin.");
            self.env.report(Code::W0255, span, message);
        }
    }
}

/// Where `QCC_VerifyFormatString` is in its format string and its arguments.
struct Cursor {
    /// The byte it reads.
    at: usize,
    /// The next argument a directive with no position takes, `argpos`.
    position: usize,
    /// One past the last argument a directive took, `argn_last`.
    last: usize,
}

impl Cursor {
    /// The next argument, taken.
    fn next(&mut self) -> usize {
        let argument = self.position;
        self.position = self.position.saturating_add(1);
        argument
    }
}

/// What a directive's argument must be: nothing more, a type it warns of once, or an integer
/// type it warns of twice (`QCC_VerifyFormatString` warns in the integer case and again after).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Requirement {
    /// The argument is of a type the directive takes.
    Met,
    /// The type the directive requires, warned of once.
    Once(&'static str),
    /// The integer type the directive requires, warned of twice.
    Twice(&'static str),
}

/// The type the conversion `conversion` requires of an argument of basic type `given`, with
/// `float` and `wide` (`q`) as the length modifiers leave them; `None` for no conversion.
fn requirement(conversion: u8, float: bool, wide: bool, given: Etype) -> Option<Requirement> {
    let unless = |accepted: bool, kind| {
        if accepted {
            Requirement::Met
        } else {
            Requirement::Once(kind)
        }
    };
    Some(match conversion {
        b'd' | b'i' | b'I' | b'c' | b'o' | b'u' | b'x' | b'X' | b'p' | b'P' | b'e' | b'E'
        | b'f' | b'F' | b'g' | b'G' => {
            if float && wide {
                unless(matches!(given, Etype::Double | Etype::Variant), "double")
            } else if float {
                unless(matches!(given, Etype::Float | Etype::Variant), "float")
            } else if matches!(conversion, b'p' | b'P') && wide {
                Requirement::Once("some kind of double-size pointer! oh noes!")
            } else if matches!(conversion, b'p' | b'P') {
                unless(matches!(given, Etype::Pointer | Etype::Variant), "pointer")
            } else if wide {
                if matches!(given, Etype::Int64 | Etype::Uint64 | Etype::Variant) {
                    Requirement::Met
                } else {
                    Requirement::Twice("__int64")
                }
            } else if matches!(given, Etype::Integer | Etype::Uint | Etype::Variant)
                || (given == Etype::Entity && conversion == b'i')
            {
                Requirement::Met
            } else {
                Requirement::Twice("int")
            }
        }
        b'v' | b'V' if float => unless(matches!(given, Etype::Vector | Etype::Variant), "vector"),
        b'v' | b'V' => Requirement::Once("intvector"),
        b's' | b'S' => unless(matches!(given, Etype::String | Etype::Variant), "string"),
        _ => return None,
    })
}

/// `strtol` of the decimal digits at `at`: the number and where the digits end.
fn digits(bytes: &[u8], at: usize) -> (usize, usize) {
    let mut end = at;
    let mut number = 0_usize;
    while let Some(&b) = bytes.get(end).filter(|b| b.is_ascii_digit()) {
        number = number
            .saturating_mul(10)
            .saturating_add(usize::from(b.saturating_sub(b'0')));
        end = end.saturating_add(1);
    }
    (number, end)
}

/// C's `(int)` of a float, as the property and type numbers are read.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec FTEF-33.2: `vf = ev->_float;` is C's float to int conversion"
)]
const fn f32_whole(value: f32) -> i64 {
    value as i64
}
