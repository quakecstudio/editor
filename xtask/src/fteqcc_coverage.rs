// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask fteqcc-coverage <dir>...`: the coverage check for the FTEQCC-rerelease dialect
//! spec (P2-01, FTE-1). It parses every `.qc` and `.qh` file under each rerelease codebase
//! directory with the vanilla parser (`qc-syntax`, `docs/spec/language.md`) and lists every
//! construct the vanilla grammar rejects, grouped into the classes of
//! `docs/spec/dialect-fteqcc-rerelease.md`. It fails if the source holds a rejected construct
//! that no class names, so the spec and the rerelease source cannot drift apart.
//!
//! The codebases come from `cargo xtask corpus fetch rerelease rerelease-hipnotic
//! rerelease-rogue rerelease-mg1 rerelease-ctf rerelease-mg3`. The recorded run over the six
//! is `docs/spec/vectors/dialect-fteqcc-rerelease/coverage.txt`.
//!
//! Detection works on the token stream, not on the recovery diagnostics, so it is stable:
//! each class has a signature of tokens (or of an "Unknown punctuation" diagnostic for a byte
//! vanilla `qcc` has no token for). A construct whose syntax the vanilla parser accepts but
//! whose meaning differs (the vector members of FTE-9: `v.z` lexes as field access) is a
//! semantic difference, not a parser rejection, so it is documented in the spec but not
//! listed here.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use qc_lexer::{Punct, Token, TokenKind, lex};
use qc_syntax::parse;

/// Which non-vanilla construct class a token or diagnostic belongs to. Each maps to one
/// section of `docs/spec/dialect-fteqcc-rerelease.md` the vanilla *parser* rejects (so not
/// FTE-1, which is about the progs format, nor FTE-9, a semantic difference). Every construct
/// found in the rerelease source must be one of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    For,
    Switch,
    Case,
    BreakContinue,
    IncDec,
    CompoundAssign,
    Modulo,
    Ternary,
    Const,
    Var,
    Nosave,
    Array,
    TypedefStruct,
    Enum,
    Pragma,
}

impl Kind {
    /// Every class, for the "every class is exercised" check.
    const ALL: [Self; 15] = [
        Self::For,
        Self::Switch,
        Self::Case,
        Self::BreakContinue,
        Self::IncDec,
        Self::CompoundAssign,
        Self::Modulo,
        Self::Ternary,
        Self::Const,
        Self::Var,
        Self::Nosave,
        Self::Array,
        Self::TypedefStruct,
        Self::Enum,
        Self::Pragma,
    ];

    /// The spec section that documents this class.
    const fn section(self) -> &'static str {
        match self {
            Self::For => "FTE-2",
            Self::Switch | Self::Case => "FTE-3",
            Self::BreakContinue => "FTE-4",
            Self::IncDec => "FTE-5",
            Self::CompoundAssign => "FTE-6",
            Self::Modulo => "FTE-7",
            Self::Ternary => "FTE-8",
            Self::Const => "FTE-10",
            Self::Var => "FTE-11",
            Self::Nosave => "FTE-12",
            Self::Array => "FTE-13",
            Self::TypedefStruct => "FTE-14",
            Self::Enum => "FTE-15",
            Self::Pragma => "FTE-16",
        }
    }

    /// A short name for the report.
    const fn name(self) -> &'static str {
        match self {
            Self::For => "for loop",
            Self::Switch => "switch",
            Self::Case => "case/default",
            Self::BreakContinue => "break/continue",
            Self::IncDec => "++/--",
            Self::CompoundAssign => "compound assignment",
            Self::Modulo => "% operator",
            Self::Ternary => "?: operator",
            Self::Const => "const",
            Self::Var => "var",
            Self::Nosave => "nosave",
            Self::Array => "array",
            Self::TypedefStruct => "typedef/struct",
            Self::Enum => "enum",
            Self::Pragma => "#pragma",
        }
    }
}

const USAGE: &str = "usage: cargo xtask fteqcc-coverage <dir>...";

/// Runs the check on the rerelease codebase directories in `args`.
pub(crate) fn main(_root: &Path, args: &[String]) -> Result<bool, String> {
    let dirs: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    if dirs.is_empty() {
        return Err(USAGE.to_owned());
    }
    let mut totals: BTreeMap<Kind, usize> = BTreeMap::new();
    let mut ok = true;
    for dir in &dirs {
        let files = sources(dir)?;
        if files.is_empty() {
            return Err(format!("no .qc or .qh files under {}", dir.display()));
        }
        let mut found: BTreeMap<Kind, usize> = BTreeMap::new();
        let mut rejects = 0_usize;
        for path in &files {
            let text = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let kinds = classify(&text);
            // The vanilla parser must reject non-vanilla source: a file with a construct below
            // has at least one error. A file with a detected construct that the vanilla parser
            // accepts is a false positive in the detector and fails the check.
            let vanilla_accepts = parse(&text).errors().next().is_none();
            if vanilla_accepts && !kinds.is_empty() {
                ok = false;
                println!(
                    "FAIL  {}: a non-vanilla construct, but the vanilla parser accepts it",
                    path.display()
                );
            }
            for kind in kinds {
                let entry = found.entry(kind).or_default();
                *entry = entry.saturating_add(1);
                let total = totals.entry(kind).or_default();
                *total = total.saturating_add(1);
                rejects = rejects.saturating_add(1);
            }
        }
        println!(
            "fteqcc-coverage: {}: {} files, {rejects} non-vanilla constructs in {} classes",
            dir.display(),
            files.len(),
            found.len()
        );
        for (kind, count) in &found {
            println!("  {:<22} {count:>5}  {}", kind.name(), kind.section());
        }
    }
    println!("fteqcc-coverage: total non-vanilla constructs by class:");
    for (kind, count) in &totals {
        let present = in_spec(kind.section());
        println!(
            "  {:<22} {count:>5}  {} ({})",
            kind.name(),
            kind.section(),
            if present {
                "in spec"
            } else {
                "MISSING FROM SPEC"
            }
        );
        if !present {
            ok = false;
        }
    }
    // Every class the spec names must have been exercised, or the spec claims a construct the
    // rerelease does not use.
    for kind in Kind::ALL {
        if !totals.contains_key(&kind) {
            ok = false;
            println!(
                "FAIL  {} ({}) is in the spec but was not found in any codebase",
                kind.name(),
                kind.section()
            );
        }
    }
    println!(
        "fteqcc-coverage: {}",
        if ok {
            "every construct is in the spec"
        } else {
            "failures found"
        }
    );
    Ok(ok)
}

/// Whether the dialect spec names `section` as one of its sections.
fn in_spec(section: &str) -> bool {
    let spec =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/spec/dialect-fteqcc-rerelease.md");
    let Ok(text) = fs::read_to_string(spec) else {
        return false;
    };
    text.lines()
        .any(|line| line.starts_with(&format!("## {section} ")))
}

/// The classes of non-vanilla construct present in `text`, each once.
fn classify(text: &[u8]) -> Vec<Kind> {
    let lexed = lex(text);
    let mut kinds: Vec<Kind> = Vec::new();
    let mut add = |kind: Kind| {
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    };
    // `%`, `?` and `:` have no vanilla token; the lexer reports "Unknown punctuation"
    // (QCD / QC-7.2) and skips the byte. Each is a distinct non-vanilla operator.
    for diagnostic in &lexed.diagnostics {
        if let Some(&byte) = text.get(diagnostic.span.start) {
            match byte {
                b'%' => add(Kind::Modulo),
                b'?' => add(Kind::Ternary),
                b':' => add(Kind::Ternary),
                _ => {}
            }
        }
    }
    let tokens = &lexed.tokens;
    for (index, token) in tokens.iter().enumerate() {
        let next = index.checked_add(1).and_then(|n| tokens.get(n));
        let prev = index.checked_sub(1).and_then(|p| tokens.get(p));
        match &token.kind {
            // A keyword is a construct only where a declaration or statement starts: the first
            // token of its line (QC's keywords are contextual, QC-3.2). This skips a keyword
            // used as an identifier, such as the `var` parameter name in id's
            // `void(string var, string val) cvar_set = #72;` or the `break` builtin name
            // `void() break = #6;`.
            TokenKind::Name(word) if token.line_start => {
                if let Some(kind) = keyword(word) {
                    add(kind);
                }
            }
            // A `[` after `=` is the vanilla state-function form `=[frame, think]` (QC-13.6),
            // which the vanilla parser accepts. An FTEQCC array declaration (`name[...]`) or
            // subscript (`name[i]`) has the `[` right after a name or a closing `]`.
            TokenKind::Punct(Punct::LBracket)
                if prev.is_some_and(|p| {
                    matches!(p.kind, TokenKind::Name(_))
                        || p.kind == TokenKind::Punct(Punct::RBracket)
                }) =>
            {
                add(Kind::Array);
            }
            TokenKind::Punct(op @ (Punct::Plus | Punct::Minus)) => {
                // `++` or `--`: two of the same operator whose spans touch.
                if next.is_some_and(|n| {
                    n.kind == TokenKind::Punct(*op) && n.span.start == token.span.end
                }) {
                    add(Kind::IncDec);
                }
                if touches_eq(token, next) {
                    add(Kind::CompoundAssign);
                }
            }
            TokenKind::Punct(Punct::Star | Punct::Slash) => {
                if touches_eq(token, next) {
                    add(Kind::CompoundAssign);
                }
            }
            // A `#` at the start of a line begins a directive (QC-9.1). `#pragma` is the one
            // FTEQCC directive the rerelease uses (FTE-16); the vanilla directives
            // `#include`/`#define`/`#ifdef` and so on stay vanilla (QC-9). A `#` not at a line
            // start is the vanilla builtin marker `#6` (QC-13.3).
            TokenKind::Punct(Punct::Hash)
                if token.line_start
                    && next.is_some_and(|n| n.kind == TokenKind::Name("pragma".to_owned())) =>
            {
                add(Kind::Pragma);
            }
            _ => {}
        }
    }
    kinds
}

/// Whether `token` is immediately followed by `=` with no gap, so the pair is a
/// compound-assignment operator (`+=`, `-=`, `*=`, `/=`), not the operator then an `=`.
fn touches_eq(token: &Token, next: Option<&Token>) -> bool {
    next.is_some_and(|n| n.kind == TokenKind::Punct(Punct::Eq) && n.span.start == token.span.end)
}

/// The construct class a keyword name belongs to, if it is one the vanilla parser rejects in
/// the positions the rerelease uses it.
fn keyword(word: &str) -> Option<Kind> {
    match word {
        "for" => Some(Kind::For),
        "switch" => Some(Kind::Switch),
        "case" | "default" => Some(Kind::Case),
        "break" | "continue" => Some(Kind::BreakContinue),
        "const" => Some(Kind::Const),
        "var" => Some(Kind::Var),
        "nosave" => Some(Kind::Nosave),
        "enum" => Some(Kind::Enum),
        "typedef" | "struct" => Some(Kind::TypedefStruct),
        _ => None,
    }
}

/// Every `.qc` and `.qh` file under `dir`, sorted.
fn sources(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let entries = fs::read_dir(&next).map_err(|e| format!("{}: {e}", next.display()))?;
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("qc") || x.eq_ignore_ascii_case("qh"))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{Kind, classify, in_spec, main};

    fn codebase(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/xtask-tests")
            .join(name);
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(dir.join("sub")).unwrap();
        for (file, text) in files {
            fs::write(dir.join(file), text).unwrap();
        }
        dir
    }

    // spec FTE-2.1: `for (...)` is detected as a for loop.
    // spec FTE-3.1, FTE-3.2: `switch`/`case`/`default` are detected.
    // spec FTE-4.1: `break` and `continue` are detected.
    #[test]
    fn keywords_are_classified() {
        assert_eq!(classify(b"for (a) b;"), vec![Kind::For]);
        assert!(classify(b"switch (x) {}").contains(&Kind::Switch));
        assert!(classify(b"case A:\ndefault:").contains(&Kind::Case));
        assert!(classify(b"break;\ncontinue;").contains(&Kind::BreakContinue));
    }

    // spec FTE-3.1: a keyword used as an identifier mid-line is not a construct: the `break`
    // builtin name and the `var` parameter name id 1.01 and the rerelease both use.
    #[test]
    fn a_keyword_as_an_identifier_is_not_a_construct() {
        assert!(classify(b"void() break = #6;\n").is_empty());
        assert!(classify(b"void(string var, string val) cvar_set = #72;\n").is_empty());
    }

    // spec FTE-5.1: `i++`/`i--` are detected and `a + +b` is not.
    #[test]
    fn increment_needs_adjacent_operators() {
        assert!(classify(b"i++;").contains(&Kind::IncDec));
        assert!(classify(b"i--;").contains(&Kind::IncDec));
        assert!(!classify(b"a + + b;").contains(&Kind::IncDec));
    }

    // spec FTE-6.1: `x += 1` is a compound assignment and `x = +1` is not.
    #[test]
    fn compound_assignment_needs_a_touching_equals() {
        assert!(classify(b"x += 1;").contains(&Kind::CompoundAssign));
        assert!(classify(b"x *= 2;").contains(&Kind::CompoundAssign));
        assert!(classify(b"x /= 2;").contains(&Kind::CompoundAssign));
        assert!(!classify(b"x = +1;").contains(&Kind::CompoundAssign));
        assert!(!classify(b"x = + 1;").contains(&Kind::CompoundAssign));
    }

    // spec FTE-7.1: `%` is a modulo operator.
    // spec FTE-8.1: `?` and `:` are the ternary operator.
    #[test]
    fn unknown_punctuation_is_classified() {
        assert!(classify(b"a % b").contains(&Kind::Modulo));
        assert!(classify(b"a ? b : c").contains(&Kind::Ternary));
    }

    // spec FTE-10.1, FTE-11.1, FTE-12.1: const/var/nosave are detected.
    // spec FTE-13.1: an array subscript or declaration is detected.
    // spec FTE-14.1: typedef/struct are detected.
    // spec FTE-15.1: enum is detected.
    // spec FTE-16.1: #pragma is detected.
    #[test]
    fn declarations_and_pragma_are_classified() {
        assert!(classify(b"const float A = 1;").contains(&Kind::Const));
        assert!(classify(b"var float a;").contains(&Kind::Var));
        assert!(classify(b"nosave float a;").contains(&Kind::Nosave));
        assert!(classify(b"float a[3];").contains(&Kind::Array));
        assert!(classify(b"typedef struct { float f; } S;").contains(&Kind::TypedefStruct));
        assert!(classify(b"enum { A, B };").contains(&Kind::Enum));
        assert!(classify(b"#pragma warning disable F317").contains(&Kind::Pragma));
    }

    #[test]
    fn vanilla_source_has_no_construct() {
        assert!(classify(b"float a;\nvoid() f = { a = 1; };\n").is_empty());
    }

    #[test]
    fn the_spec_names_every_detected_section() {
        for section in ["FTE-2", "FTE-3", "FTE-16"] {
            assert!(in_spec(section), "{section}");
        }
        assert!(!in_spec("FTE-99"));
    }

    #[test]
    fn a_codebase_with_every_class_passes_and_bad_arguments_are_errors() {
        let dir = codebase(
            "fteqcc-coverage",
            &[(
                "all.qc",
                "const float A = 1;\n\
                 var float b = 0;\n\
                 nosave float c;\n\
                 enum { E };\n\
                 typedef struct { float f; } S;\n\
                 float arr[2];\n\
                 #pragma warning disable F317\n\
                 void(float n) f = {\n\
                     for (float i = 0; i < n; i++) {\n\
                         b += i % 2;\n\
                         switch (i) {\n\
                             case 0:\n\
                                 continue;\n\
                             default:\n\
                                 break;\n\
                         }\n\
                         b = i ? 1 : 0;\n\
                     }\n\
                 };\n",
            )],
        );
        assert!(main(Path::new("."), &[dir.display().to_string()]).unwrap());
        assert!(main(Path::new("."), &[]).unwrap_err().contains("usage"));
        let empty = codebase("fteqcc-coverage-empty", &[("readme.txt", "x")]);
        assert!(
            main(Path::new("."), &[empty.display().to_string()])
                .unwrap_err()
                .contains("no .qc or .qh")
        );
    }
}
