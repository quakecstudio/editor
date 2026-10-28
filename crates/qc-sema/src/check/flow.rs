// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_CheckForDeadAndMissingReturns, QCC_AStatementJumpsTo, QCC_StatementIsAJump, QCC_PR_FinaliseFunction

//! The layout of a body's statements as FTEQCC generates them, as far as its checks of the
//! finished function read it (FTEF-34.6): which statements return, which jump and where to.
//! `QCC_CheckForDeadAndMissingReturns` finds in it a path that returns no value and code that no
//! path reaches.

/// What a statement of the layout does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// `OP_RETURN`.
    Return,
    /// `OP_DONE`, the end of the function or `exit`.
    Done,
    /// `OP_GOTO`.
    Goto,
    /// A jump taken on a condition: `OP_IF`, `OP_IFNOT`, `OP_CASE` and the like.
    Branch,
    /// Any statement that does not jump.
    Other,
}

/// One statement: what it does, where it jumps if it jumps, and its line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Statement {
    step: Step,
    target: Option<usize>,
    line: u32,
}

/// What the check of the finished function finds, in the order FTEQCC reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Finding {
    /// "not all control paths return a value" (`WARN_MISSINGRETURN`).
    MissingReturn,
    /// "contains unreachable code (line N)" (`WARN_UNREACHABLECODE`), at line N.
    Unreachable(u32),
}

/// The statements generated so far, with the jumps waiting for their targets: `break`s,
/// `continue`s and `goto`s.
#[derive(Debug, Default)]
pub(super) struct Layout {
    code: Vec<Statement>,
    breaks: Vec<usize>,
    continues: Vec<usize>,
    labels: Vec<(String, usize)>,
    gotos: Vec<(String, usize)>,
}

impl Layout {
    /// The index the next statement takes, `numstatements`.
    pub(super) fn here(&self) -> usize {
        self.code.len()
    }

    /// Adds a statement that does `step`, jumping to `target`; returns its index.
    pub(super) fn push(&mut self, step: Step, target: Option<usize>, line: u32) -> usize {
        let at = self.here();
        self.code.push(Statement { step, target, line });
        at
    }

    /// Sets the target of the jump at `at`.
    pub(super) fn patch(&mut self, at: usize, target: usize) {
        if let Some(statement) = self.code.get_mut(at) {
            statement.target = Some(target);
        }
    }

    /// `QCC_UngenerateStatements(from)`: drops the statements from `from` on, and the jumps among
    /// them that wait for a target.
    pub(super) fn truncate(&mut self, from: usize) {
        self.code.truncate(from);
        self.breaks.retain(|&at| at < from);
        self.continues.retain(|&at| at < from);
        self.gotos.retain(|(_, at)| *at < from);
    }

    /// What the last statement does, if there is one.
    pub(super) fn last(&self) -> Option<Step> {
        self.code.last().map(|s| s.step)
    }

    /// `QCC_AStatementJumpsTo(target, from, numstatements)`: whether a statement from `from` on
    /// jumps to `target`.
    pub(super) fn jumps_to(&self, from: usize, target: usize) -> bool {
        self.code
            .get(from..)
            .is_some_and(|code| code.iter().any(|s| s.target == Some(target)))
    }

    /// The labels defined so far, `num_labels`.
    pub(super) fn labels(&self) -> usize {
        self.labels.len()
    }

    /// `break;`: a `goto` to the end of the innermost loop or `switch`.
    pub(super) fn break_(&mut self, line: u32) {
        let at = self.push(Step::Goto, None, line);
        self.breaks.push(at);
    }

    /// `continue;`: a `goto` to the next pass of the innermost loop.
    pub(super) fn continue_(&mut self, line: u32) {
        let at = self.push(Step::Goto, None, line);
        self.continues.push(at);
    }

    /// The `break`s and `continue`s so far, `num_breaks` and `num_continues`, from which a loop
    /// patches its own.
    pub(super) fn marks(&self) -> (usize, usize) {
        (self.breaks.len(), self.continues.len())
    }

    /// Points the `break`s after `mark` at `target`.
    pub(super) fn patch_breaks(&mut self, mark: usize, target: usize) {
        for at in self.breaks.split_off(mark.min(self.breaks.len())) {
            self.patch(at, target);
        }
    }

    /// Points the `continue`s after `mark` at `target`.
    pub(super) fn patch_continues(&mut self, mark: usize, target: usize) {
        for at in self.continues.split_off(mark.min(self.continues.len())) {
            self.patch(at, target);
        }
    }

    /// A label `name:`, at the next statement.
    pub(super) fn label(&mut self, name: String) {
        let at = self.here();
        self.labels.push((name, at));
    }

    /// `goto name;`.
    pub(super) fn goto(&mut self, name: String, line: u32) {
        let at = self.push(Step::Goto, None, line);
        self.gotos.push((name, at));
    }

    /// `QCC_PR_FinaliseFunction`: the `return` of a value `return =` set, when `returned`; then
    /// `OP_DONE` unless the last statement returns; then `QCC_CheckForDeadAndMissingReturns`
    /// for a function whose value is `void` or not.
    pub(super) fn finish(mut self, returned: bool, void: bool, line: u32) -> Vec<Finding> {
        for (name, at) in std::mem::take(&mut self.gotos) {
            if let Some(&(_, target)) = self.labels.iter().find(|(label, _)| *label == name) {
                self.patch(at, target);
            }
        }
        if returned {
            self.push(Step::Return, None, line);
        }
        if !matches!(self.last(), Some(Step::Return | Step::Done)) {
            self.push(Step::Done, None, line);
        }
        self.dead_and_missing_returns(void)
    }

    /// `QCC_CheckForDeadAndMissingReturns` at the default optimisation level: a function that
    /// returns a value whose last statement is no `return` or backward `goto`, or that jumps on a
    /// condition to its end, misses a return; a statement after a `return` or `goto` that is no
    /// `return`, `goto` or `done` and that no jump reaches is unreachable.
    fn dead_and_missing_returns(&self, void: bool) -> Vec<Finding> {
        let mut last = self.code.len();
        if self.last() == Some(Step::Done) {
            last = last.saturating_sub(1);
        }
        let ends = last
            .checked_sub(1)
            .and_then(|at| self.code.get(at).map(|s| (at, s)));
        if !void {
            let returns = match ends {
                Some((_, s)) if s.step == Step::Return => true,
                Some((at, s)) if s.step == Step::Goto => s.target.is_some_and(|t| t <= at),
                Some(_) | None => false,
            };
            if !returns {
                return vec![Finding::MissingReturn];
            }
        }
        let mut findings = Vec::new();
        let mut st = 0;
        while st < last {
            let Some(statement) = self.code.get(st) else {
                break;
            };
            if matches!(statement.step, Step::Return | Step::Goto) {
                st = st.saturating_add(1);
                // Without `opt_compound_jumps`, a `goto` or `done` after a jump is the
                // compiler's own, and a `return` after one is allowed.
                if st < last
                    && let Some(next) = self.code.get(st)
                    && !matches!(next.step, Step::Return | Step::Goto | Step::Done)
                    && !self
                        .code
                        .get(..last)
                        .is_some_and(|code| code.iter().any(|s| s.target == Some(st)))
                {
                    findings.push(Finding::Unreachable(next.line));
                }
            } else if !void && statement.target == Some(last) {
                findings.push(Finding::MissingReturn);
                return findings;
            }
            st = st.saturating_add(1);
        }
        findings
    }
}
