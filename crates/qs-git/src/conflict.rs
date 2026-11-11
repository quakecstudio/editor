// SPDX-License-Identifier: GPL-3.0-or-later

//! Merge conflicts as git writes them into a file (`git help merge`, "How conflicts are
//! presented"): `<<<<<<<` then our side, optionally `|||||||` and the common base (the `diff3`
//! style), `=======`, their side and `>>>>>>>`, each marker at the start of a line. Each
//! conflict is resolved by keeping our side, theirs, or both, as VS Code's merge editor offers.

/// Where one conflict's markers are, by line from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// The `<<<<<<<` line.
    pub start: usize,
    /// The `|||||||` line, in the `diff3` style.
    pub base: Option<usize>,
    /// The `=======` line.
    pub middle: usize,
    /// The `>>>>>>>` line.
    pub end: usize,
}

/// Which side of a conflict to keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Ours: the lines between `<<<<<<<` and `|||||||` or `=======`.
    Ours,
    /// Theirs: the lines between `=======` and `>>>>>>>`.
    Theirs,
    /// Ours, then theirs.
    Both,
}

/// Whether `line` starts with marker `mark`, seven times its character, then a space or the
/// line's end.
fn is_marker(line: &str, mark: &str) -> bool {
    line.strip_prefix(mark)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\r', '\n']))
}

/// The complete conflicts in `text`, in order; markers that do not make a whole conflict are
/// left as text.
#[must_use]
pub fn conflicts(text: &str) -> Vec<Conflict> {
    let mut out = Vec::new();
    let mut open: Option<(usize, Option<usize>, Option<usize>)> = None;
    for (index, line) in text.lines().enumerate() {
        if is_marker(line, "<<<<<<<") {
            open = Some((index, None, None));
        } else if let Some((start, base, middle)) = open.as_mut() {
            if is_marker(line, "|||||||") && base.is_none() && middle.is_none() {
                *base = Some(index);
            } else if is_marker(line, "=======") && middle.is_none() {
                *middle = Some(index);
            } else if is_marker(line, ">>>>>>>")
                && let Some(middle) = *middle
            {
                out.push(Conflict {
                    start: *start,
                    base: *base,
                    middle,
                    end: index,
                });
                open = None;
            }
        }
    }
    out
}

/// `text` with conflict `index` replaced by `side`; `None` when there is no such conflict.
#[must_use]
pub fn resolve(text: &str, index: usize, side: Side) -> Option<String> {
    let conflict = *conflicts(text).get(index)?;
    let ours_end = conflict.base.unwrap_or(conflict.middle);
    let mut out = String::with_capacity(text.len());
    for (line_index, line) in text.split_inclusive('\n').enumerate() {
        let keep = if line_index < conflict.start || line_index > conflict.end {
            true
        } else {
            let ours = line_index > conflict.start && line_index < ours_end;
            let theirs = line_index > conflict.middle && line_index < conflict.end;
            match side {
                Side::Ours => ours,
                Side::Theirs => theirs,
                Side::Both => ours || theirs,
            }
        };
        if keep {
            out.push_str(line);
        }
    }
    Some(out)
}
