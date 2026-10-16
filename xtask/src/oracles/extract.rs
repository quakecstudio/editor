// SPDX-License-Identifier: GPL-3.0-or-later

//! Copies named function definitions out of a C file, unchanged.
//!
//! id's sources start every function definition at column 0 with its return type and name,
//! and end it with the brace that closes the body. A definition is found by a line of code (not
//! inside a comment) that names the function before its parameter list and is not a
//! declaration (the list is not followed by `;` or `,`); it runs to the line where
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
    let start = definition_start(&lines, name).ok_or_else(|| format!("no definition of {name}"))?;
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

/// The index of the line that starts the definition of `name`: a line that begins at column 0
/// outside a block comment, whose code names `name` as a whole word followed by `(`, and whose
/// parameter list is not followed by `;` or `,` (a declaration). Comments and literals are not
/// code, so a comment header that shows a call, or a declaration with a trailing comment, is
/// not a definition.
fn definition_start(lines: &[&str], name: &str) -> Option<usize> {
    let mut scan = Scanner::default();
    for (index, line) in lines.iter().enumerate() {
        let in_comment = scan.in_comment;
        let code = scan.code(line);
        if in_comment || line.starts_with([' ', '\t', '#', '/', '*', '{', '}']) {
            continue;
        }
        let Some(open) = parameter_list(&code, name) else {
            continue;
        };
        let mut rest = scan.clone();
        let first = code.get(open..).unwrap_or_default().to_owned();
        let following = lines
            .iter()
            .skip(index.saturating_add(1))
            .map(|next| rest.code(next));
        if !is_declaration(std::iter::once(first).chain(following)) {
            return Some(index);
        }
    }
    None
}

/// The byte offset of the `(` that follows `name` as a whole word in `code`, unless an `=`
/// comes before it (an initialiser that calls the function).
fn parameter_list(code: &str, name: &str) -> Option<usize> {
    let mut offset = 0_usize;
    while let Some(at) = code.get(offset..).and_then(|rest| rest.find(name)) {
        let at = offset.saturating_add(at);
        let before = code.get(..at).unwrap_or_default();
        let end = at.saturating_add(name.len());
        let after = code.get(end..).unwrap_or_default();
        let word_start = before
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
        let gap = after.len().saturating_sub(after.trim_start().len());
        if word_start && after.trim_start().starts_with('(') && !before.contains('=') {
            return Some(end.saturating_add(gap));
        }
        offset = end;
    }
    None
}

/// Whether the code after a parameter list, starting at its `(`, makes it a declaration: the
/// first code after the list closes is `;` or `,`.
fn is_declaration(code: impl Iterator<Item = String>) -> bool {
    let mut parens = 0_i64;
    for piece in code {
        for c in piece.chars() {
            match c {
                '(' => parens = parens.saturating_add(1),
                ')' => parens = parens.saturating_sub(1),
                c if parens == 0 && !c.is_whitespace() => return c == ';' || c == ',',
                _ => {}
            }
        }
    }
    false
}

/// Brace depth across lines, skipping comments and literals.
#[derive(Clone, Default)]
struct Scanner {
    depth: i64,
    in_comment: bool,
}

impl Scanner {
    /// The code of one line: a comment becomes a space and a literal keeps only its quotes.
    /// Tracks whether the next line starts inside a block comment.
    fn code(&mut self, line: &str) -> String {
        let mut code = String::with_capacity(line.len());
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if self.in_comment {
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    self.in_comment = false;
                    code.push(' ');
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
                    code.push(c);
                    while let Some(d) = chars.next() {
                        if d == '\\' {
                            chars.next();
                        } else if d == c {
                            code.push(c);
                            break;
                        }
                    }
                }
                _ => code.push(c),
            }
        }
        code
    }

    /// Scans one line; returns the highest depth reached on it.
    fn line(&mut self, line: &str) -> i64 {
        let mut highest = self.depth;
        for c in self.code(line).chars() {
            match c {
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
    fn skips_a_comment_line_naming_the_function() {
        let source = "/*\n==========\nf\n\nf (entity e)\n==========\n*/\nvoid f (int e)\n{\n}\n";
        assert_eq!(function(source, "f").unwrap().line, 8);
    }

    #[test]
    fn skips_a_declaration_with_a_trailing_comment() {
        let source = "void f (void); // declared\nvoid g (void)\n{\n}\nvoid f (void)\n{\n}\n";
        assert_eq!(function(source, "f").unwrap().line, 5);
    }

    #[test]
    fn skips_other_declarations_and_names_outside_code() {
        let cases = [
            // A block comment after the declaration.
            ("void f (void); /* declared */\nvoid f (void)\n{\n}\n", 2),
            // A declaration whose parameter list spans lines.
            (
                "void f (int a,\n\tint b);\nvoid f (int a,\n\tint b)\n{\n}\n",
                3,
            ),
            // A declaration of several functions.
            ("void f (void), g (void);\nvoid f (void)\n{\n}\n", 2),
            // The name inside a string at column 0.
            ("char *t[] = {\n\"f (x)\"};\nvoid f (void)\n{\n}\n", 3),
            // A one-line block comment that shows a call, then the definition.
            ("/* f (e) */ int x;\nvoid f (void)\n{\n}\n", 2),
        ];
        for (source, line) in cases {
            assert_eq!(function(source, "f").unwrap().line, line, "{source:?}");
        }
    }

    #[test]
    fn keeps_a_definition_with_a_comment_before_its_body() {
        let source = "void f (void) // the body follows; really\n{\n\tg ();\n}\n";
        let f = function(source, "f").unwrap();
        assert_eq!(f.line, 1);
        assert!(f.text.ends_with("\tg ();\n}\n"));
    }

    #[test]
    fn skips_an_assignment_naming_the_function() {
        let source = "builtin_t table[] = {f (0)};\nfp x = f (1),\nvoid f (int a)\n{\n}\n";
        assert_eq!(function(source, "f").unwrap().line, 3);
    }
}
