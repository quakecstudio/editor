// SPDX-License-Identifier: GPL-3.0-or-later

//! Copies named function definitions out of a C file, unchanged.
//!
//! id's sources start every function definition at column 0 with its return type and name,
//! and end it with the brace that closes the body. A definition is found by a line that names
//! the function before its parameter list and is not a declaration; it runs to the line where
//! the body's braces balance outside any `#if` the definition opened, so a function whose body
//! is split by `#ifdef QUAKE2 ... #else ... #endif` (id's `PF_Find`) is copied whole. Braces in
//! comments, strings and character literals do not count.

/// One function's text and the 1-based line it starts on.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Function {
    pub(crate) line: usize,
    pub(crate) text: String,
}

/// The definition of `name` in `source`.
pub(crate) fn function(source: &str, name: &str) -> Result<Function, String> {
    let lines: Vec<&str> = source.lines().collect();
    let start = lines
        .iter()
        .position(|line| is_definition_start(line, name))
        .ok_or_else(|| format!("no definition of {name}"))?;
    let mut scan = Scanner::default();
    let mut opened = false;
    let mut if_depth = 0_i64;
    for (index, line) in lines.iter().enumerate().skip(start) {
        let directive = line.trim_start();
        if !scan.in_comment && directive.starts_with('#') {
            let word = directive
                .trim_start_matches('#')
                .trim_start()
                .split(|c: char| !c.is_ascii_alphanumeric())
                .next()
                .unwrap_or_default();
            match word {
                "if" | "ifdef" | "ifndef" => if_depth = if_depth.saturating_add(1),
                "endif" => if_depth = if_depth.saturating_sub(1),
                _ => {}
            }
        } else if scan.line(line) > 0 {
            opened = true;
        }
        if opened && scan.depth == 0 && if_depth <= 0 {
            let end = index.saturating_add(1);
            let text = lines.get(start..end).unwrap_or_default().join("\n");
            return Ok(Function {
                line: start.saturating_add(1),
                text: format!("{text}\n"),
            });
        }
    }
    Err(format!("the definition of {name} does not end"))
}

/// Whether `line` starts the definition of `name`: it begins at column 0, names `name` as a
/// whole word followed by `(`, and does not end with `;` (a declaration).
fn is_definition_start(line: &str, name: &str) -> bool {
    if line.starts_with([' ', '\t', '#', '/', '*', '{', '}']) || line.trim_end().ends_with(';') {
        return false;
    }
    let mut rest = line;
    while let Some(at) = rest.find(name) {
        let before = rest.get(..at).unwrap_or_default();
        let after = rest
            .get(at.saturating_add(name.len())..)
            .unwrap_or_default();
        let word_start = before
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
        if word_start && after.trim_start().starts_with('(') && !before.contains('=') {
            return true;
        }
        rest = after;
    }
    false
}

/// Brace depth across lines, skipping comments and literals.
#[derive(Default)]
struct Scanner {
    depth: i64,
    in_comment: bool,
}

impl Scanner {
    /// Scans one line; returns the highest depth reached on it.
    fn line(&mut self, line: &str) -> i64 {
        let mut highest = self.depth;
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if self.in_comment {
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    self.in_comment = false;
                }
                continue;
            }
            match c {
                '/' if chars.peek() == Some(&'/') => break,
                '/' if chars.peek() == Some(&'*') => {
                    chars.next();
                    self.in_comment = true;
                }
                '"' | '\'' => {
                    while let Some(d) = chars.next() {
                        if d == '\\' {
                            chars.next();
                        } else if d == c {
                            break;
                        }
                    }
                }
                '{' => {
                    self.depth = self.depth.saturating_add(1);
                    highest = highest.max(self.depth);
                }
                '}' => self.depth = self.depth.saturating_sub(1),
                _ => {}
            }
        }
        highest
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use super::function;

    const SOURCE: &str = "\
int helper (int x);

/*
==========
helper
==========
*/
int helper (int x)
{
\tif (x == '{')
\t\treturn 1;
\tprintf (\"}\"); // }
\treturn 0;
}

void split (void)
#ifdef QUAKE2
{
\tone ();
}
#else
{
\ttwo ();
}
#endif

void after (void)
{
}
";

    #[test]
    fn copies_a_definition_ignoring_braces_in_literals_and_comments() {
        let f = function(SOURCE, "helper").unwrap();
        assert_eq!(f.line, 8);
        assert!(f.text.starts_with("int helper (int x)\n{"));
        assert!(f.text.ends_with("return 0;\n}\n"));
    }

    #[test]
    fn copies_both_branches_of_a_split_body() {
        let f = function(SOURCE, "split").unwrap();
        assert!(f.text.contains("one ();"));
        assert!(f.text.ends_with("#endif\n"));
        assert!(!f.text.contains("after"));
    }

    #[test]
    fn skips_declarations_and_reports_a_missing_function() {
        assert!(function(SOURCE, "missing").is_err());
        assert!(function("int helper (int x);\n", "helper").is_err());
        assert!(function("void f (void)\n{\n", "f").is_err());
    }

    #[test]
    fn matches_whole_names_only() {
        let source = "void PF_WriteByte2 (void)\n{\n}\nvoid PF_WriteByte (void)\n{\n}\n";
        assert_eq!(function(source, "PF_WriteByte").unwrap().line, 4);
    }

    #[test]
    fn copies_one_line_and_same_line_brace_definitions() {
        let source = "void one (void) { }\nvoid two (void) {\n\tx ();\n}\n";
        assert_eq!(
            function(source, "one").unwrap().text,
            "void one (void) { }\n"
        );
        let two = function(source, "two").unwrap();
        assert_eq!(two.line, 2);
        assert_eq!(two.text, "void two (void) {\n\tx ();\n}\n");
    }

    #[test]
    fn ignores_directives_and_braces_inside_block_comments() {
        let source = "void f (void)\n{\n/*\n#if 0\n}\n*/\n\tg ();\n}\nvoid h (void)\n{\n}\n";
        let f = function(source, "f").unwrap();
        assert!(f.text.ends_with("\tg ();\n}\n"));
        assert!(!f.text.contains("void h"));
    }

    #[test]
    fn handles_escaped_quotes_and_char_literals() {
        let source = "void f (void)\n{\n\tp (\"\\\"{\");\n\tq ('}');\n\tr ('\\'');\n}\n\
                      void g (void)\n{\n}\n";
        let f = function(source, "f").unwrap();
        assert!(f.text.ends_with("\tr ('\\'');\n}\n"));
    }

    #[test]
    fn skips_an_assignment_naming_the_function() {
        let source = "builtin_t table[] = {f (0)};\nfp x = f (1),\nvoid f (int a)\n{\n}\n";
        assert_eq!(function(source, "f").unwrap().line, 3);
    }
}
