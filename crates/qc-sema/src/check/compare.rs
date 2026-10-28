// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_StatementFlags

//! The warnings `QCC_PR_StatementFlags` gives of a comparison or logic opcode it emits (FTEF-28):
//! a result that is constant, the same operand on both sides, and operands whose types do not
//! match.

use qc_diag::{Code, Span};
use qc_preproc::Op;

use crate::check::expr::Checker;
use crate::check::opcodes::row;
use crate::check::{Sym, Value};

impl Checker<'_> {
    /// FTEF-28.1, FTEF-28.2: the warnings of the comparison or logic opcode `op` emitted on
    /// `lhs` and `rhs`.
    pub(super) fn compare(&mut self, op: Op, lhs: &Value, rhs: &Value, span: Span) {
        let same = lhs.sym != Sym::Temp && lhs.sym == rhs.sym;
        let constant = (lhs.sym.constant(), rhs.sym.constant());
        let warning = match op {
            Op::AndF => {
                if same {
                    self.warn(span, "Parameter offsets for && are the same");
                }
                constant.0 && constant.1
            }
            Op::OrF => {
                if same {
                    self.warn(span, "Parameters for || are the same");
                }
                constant.0 || constant.1
            }
            Op::EqF | Op::NeF | Op::EqV | Op::NeV | Op::LeF | Op::GeF => {
                self.mismatch_types(op, lhs, rhs, span);
                constant.0 && constant.1
            }
            Op::EqS | Op::EqE | Op::EqFnc | Op::NeS | Op::NeE | Op::NeFnc | Op::LtF | Op::GtF => {
                self.mismatch_types(op, lhs, rhs, span);
                (constant.0 && constant.1) || same
            }
            _ => false,
        };
        if warning {
            self.warn(span, "Result of comparison is constant");
        }
    }

    /// "'OP' type mismatch: A with B", when neither operand's type, or a parent of it, is the
    /// other's by `typecmp_lax` (FTEF-28.2).
    fn mismatch_types(&mut self, op: Op, lhs: &Value, rhs: &Value, span: Span) {
        if self.env.lax_same(&lhs.ty, &rhs.ty) {
            return;
        }
        let name = row(op).map_or("", |r| r.name);
        let message = format!(
            "'{name}' type mismatch: {} with {}",
            self.env.type_name(&lhs.ty),
            self.env.type_name(&rhs.ty)
        );
        self.env.report(Code::W0240, span, message);
    }

    /// The warning `W0239` with FTEQCC's text.
    fn warn(&mut self, span: Span, message: &str) {
        self.env.report(Code::W0239, span, message.to_owned());
    }
}
