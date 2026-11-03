// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_StatementFlags, QCC_PR_RefExpression

//! The patched FTEQCC that proves a divergence is an FTEQCC defect (ADR-0020 tier 3,
//! DIFF-14.12, DIFF-14.5): the pinned `qcc_pr_comp.c` with each defect this module names fixed, and
//! nothing else changed. A generated program on which qc-codegen and the pinned FTEQCC
//! diverge is an FTEQCC defect only when this build agrees with qc-codegen.

/// The file the fixes edit, under the FTEQCC tree.
pub(crate) const FILE: &str = "engine/qclib/qcc_pr_comp.c";

/// One fix: what it fixes, its spec statement, and the text it replaces, which must occur
/// exactly once in the pinned file.
pub(crate) struct Fix {
    /// The spec statement that describes the defect.
    pub(crate) statement: &'static str,
    /// The pinned text.
    pub(crate) find: &'static str,
    /// What replaces it.
    pub(crate) replace: &'static str,
    /// How many times the pinned text occurs; each is replaced.
    pub(crate) times: usize,
}

/// The defects fixed. DIFF-14.6: a `?:`'s result is a one-word local named `ternary` when its
/// arms are one word, and `opt_assignments` folds the statement that made an arm into it, so a
/// `LOAD_V` (FTE-9.3's `(e.v).x`) writes the two globals after it; the local is made a vector,
/// three words, read and written as the arm's type. DIFF-14.7: a statement frees
/// its operands before it takes its result's temporary, so a vector result may take the slot
/// of an operand the opcode reads again after writing its first word (`MUL_VF` reads its float
/// operand for each component); those multiplies free their operands only after they have taken
/// the result's temporary. DIFF-14.10: stripping a constant `if` forgets the `case` labels at
/// the statement it rewinds to, its own included; only those after it are forgotten.
/// DIFF-14.11: `opt_assignments` folds the store of one word of a wider result into the
/// statement that made it, which then writes every word of it; a result wider than the store is
/// not folded.
pub(crate) const FIXES: [Fix; 6] = [
    Fix {
        statement: "DIFF-14.6",
        find: "r = QCC_MakeSRefForce(QCC_PR_DummyDef(r.cast=val.cast, \"ternary\", pr_scope, 0, NULL, 0, false, GDF_STRIP), 0, val.cast);",
        replace: "r = QCC_MakeSRefForce(QCC_PR_DummyDef((r.cast=val.cast, val.cast->size < 3 ? type_vector : val.cast), \"ternary\", pr_scope, 0, NULL, 0, false, GDF_STRIP), 0, val.cast);",
        times: 2,
    },
    Fix {
        statement: "DIFF-14.7",
        times: 1,
        find: "\tif (op->type_c == &type_void || op->associative==ASSOC_RIGHT || op->type_c == NULL)\n\t{\n\t\tQCC_FreeTemp(var_b);\t//returns a instead of some result/temp\n",
        replace: "\tpbool qs_free_later = false;\n\tif (op->type_c == &type_void || op->associative==ASSOC_RIGHT || op->type_c == NULL)\n\t{\n\t\tQCC_FreeTemp(var_b);\t//returns a instead of some result/temp\n",
    },
    Fix {
        statement: "DIFF-14.7",
        times: 1,
        find: "\telse\n\t{\n\t\tQCC_FreeTemp(var_a);\n\t\tQCC_FreeTemp(var_b);\n\t}\n\n\tif (outstatement)\n\t\tQCC_ClobberDef(NULL);\n",
        replace: "\telse if (op == &pr_opcodes[OP_MUL_VF] || op == &pr_opcodes[OP_MUL_FV] || op == &pr_opcodes[OP_MUL_VI] || op == &pr_opcodes[OP_MUL_IV])\n\t\tqs_free_later = true;\n\telse\n\t{\n\t\tQCC_FreeTemp(var_a);\n\t\tQCC_FreeTemp(var_b);\n\t}\n\n\tif (outstatement)\n\t\tQCC_ClobberDef(NULL);\n",
    },
    Fix {
        statement: "DIFF-14.7",
        times: 1,
        find: "\tif (flags&STFL_DISCARDRESULT)\n\t{\n\t\tQCC_FreeTemp(var_c);\n\t\tvar_c = nullsref;\n\t}\n\treturn var_c;\n}\n",
        replace: "\tif (qs_free_later)\n\t{\n\t\tQCC_FreeTemp(var_a);\n\t\tQCC_FreeTemp(var_b);\n\t}\n\tif (flags&STFL_DISCARDRESULT)\n\t{\n\t\tQCC_FreeTemp(var_c);\n\t\tvar_c = nullsref;\n\t}\n\treturn var_c;\n}\n",
    },
    Fix {
        statement: "DIFF-14.10",
        times: 1,
        find: "\t\tif (pr_cases[i] >= newstatementcount)\n",
        replace: "\t\tif (pr_cases[i] > newstatementcount)\n",
    },
    Fix {
        statement: "DIFF-14.11",
        times: 1,
        find: "\tif (opt_assignments && OpAssignsToC(statements[numstatements-1].op) && source.sym == statements[numstatements-1].c.sym && source.ofs == statements[numstatements-1].c.ofs)\n",
        replace: "\tif (opt_assignments && OpAssignsToC(statements[numstatements-1].op) && source.sym == statements[numstatements-1].c.sym && source.ofs == statements[numstatements-1].c.ofs && !(pr_opcodes[statements[numstatements-1].op].type_c && *pr_opcodes[statements[numstatements-1].op].type_c && (*pr_opcodes[statements[numstatements-1].op].type_c)->size > type->size))\n",
    },
];

/// `source`, the pinned file's bytes, with every fix applied; an error names the first fix
/// whose text is not in it exactly once.
pub(crate) fn apply(source: &[u8]) -> Result<Vec<u8>, String> {
    let mut text = source.to_vec();
    for (i, fix) in FIXES.iter().enumerate() {
        let find = fix.find.as_bytes();
        let at: Vec<usize> = text
            .windows(find.len())
            .enumerate()
            .filter(|(_, window)| *window == find)
            .map(|(at, _)| at)
            .collect();
        if at.len() != fix.times {
            return Err(format!(
                "{FILE}: fix {i} ({}) expects its text {} times, found it {} times",
                fix.statement,
                fix.times,
                at.len()
            ));
        }
        // From the last, so each earlier offset still holds.
        for start in at.iter().rev() {
            let end = start.saturating_add(find.len());
            text.splice(*start..end, fix.replace.bytes());
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use super::{FIXES, apply};

    fn pinned() -> String {
        FIXES
            .iter()
            .map(|fix| format!("{}/* between */\n", fix.find).repeat(fix.times))
            .collect()
    }

    // spec DIFF-14.12: every fix is applied to the text it names, and nothing else changes
    // spec DIFF-14.6: the `?:` local's two definitions are made vectors
    // spec DIFF-14.7: the vector multiplies free their operands after taking the result's
    // temporary
    // spec DIFF-14.10: a case label at the rewound statement is kept
    // spec DIFF-14.11: a store of one word of a wider result is not folded
    #[test]
    fn each_fix_replaces_its_text_once() {
        let fixed = String::from_utf8(apply(pinned().as_bytes()).unwrap()).unwrap();
        for fix in &FIXES {
            assert!(fixed.contains(fix.replace), "{}", fix.statement);
        }
        assert_eq!(fixed.matches(FIXES[0].replace).count(), 2, "both ?: sites");
        let between: usize = FIXES.iter().map(|fix| fix.times).sum();
        assert_eq!(fixed.matches("/* between */").count(), between);
    }

    // spec DIFF-14.12: a pinned file without a fix's text, or with it twice, is an error, so a
    // changed upstream cannot build an unfixed oracle
    #[test]
    fn a_missing_or_repeated_text_is_an_error() {
        let missing = pinned().replacen(FIXES[1].find, "", 1);
        assert!(apply(missing.as_bytes()).unwrap_err().contains("fix 1"));
        let thrice = format!("{}{}", pinned(), FIXES[0].find);
        assert!(apply(thrice.as_bytes()).unwrap_err().contains("fix 0"));
        let once = pinned().replacen(FIXES[0].find, "", 1);
        assert!(apply(once.as_bytes()).unwrap_err().contains("fix 0"));
    }
}
