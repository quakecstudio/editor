// SPDX-License-Identifier: GPL-3.0-or-later

//! Opcodes the target lacks, as FTEQCC emulates or refuses them (`docs/spec/dialect-fteqcc.md`
//! FTEF-27). Each case's diagnostics are the pinned FTEQCC's on the standard target
//! (2026-10-27); a warning its settings make an error is `E0125` (F313).

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;

const FILE: FileId = FileId(0);

fn diags(source: &str) -> Vec<(Code, String)> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(FILE, PathBuf::from("t.qc"), source.as_bytes().to_vec());
    let analysis = db.analyze_module(&ModuleInput {
        compiled: vec![FILE],
        command_line: CommandLine::new::<&str>(&[]),
        ..ModuleInput::default()
    });
    analysis
        .diagnostics()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect()
}

fn body(statements: &str) -> String {
    format!(
        "void() g = {{ local float a; local int i; local string s; local entity e; {statements} \
         }};\n"
    )
}

fn d(code: Code, message: &str) -> (Code, String) {
    (code, message.to_owned())
}

// spec FTEF-27.2: an integer opcode with no helper is "NAME function not defined", then refused
// spec FTEF-27.5: refused, with the FTE target suggested when it has the opcode
// spec QCD-4.26: E0320 "Opcode ... not valid for target"
// spec QCD-7.77: W0235 for an emulation's notes
#[test]
fn integer_opcodes_need_a_helper() {
    assert_eq!(
        diags(&body("i = i + i;")),
        [
            d(
                Code::W0235,
                "AddInt function not defined: cannot emulate int+int"
            ),
            d(
                Code::E0320,
                "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte"
            ),
        ]
    );
    assert_eq!(
        diags(&body("i = i * i;")),
        [
            d(
                Code::W0235,
                "MulInt function not defined: cannot emulate int*int"
            ),
            d(
                Code::E0320,
                "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte"
            ),
        ]
    );
    let defined = "int(int a, int b) AddInt = { return a; };\n\
                   void() g = { local int i; i = i + i; };\n";
    assert_eq!(diags(defined), []);
}

// spec FTEF-27.4: an int converted to a float with no `itof` warns that denormals are unsafe,
// naming the value, which F313 makes an error
// spec QCD-7.79: W0237 for an emulation's denormal warnings
#[test]
fn conversions_warn_of_denormals() {
    assert_eq!(
        diags(&body("a = a + i;")),
        [d(Code::E0125, "itof emulation: denormals are unsafe i")]
    );
    assert_eq!(
        diags(&body("a = i < i;")),
        [
            d(Code::E0125, "itof emulation: denormals are unsafe i"),
            d(Code::E0125, "itof emulation: denormals are unsafe i"),
        ]
    );
}

// spec FTEF-27.3: a string offset warns, then converts the float and adds integers
// spec QCD-7.78: W0236 "OP_ADD_SF: string+float may be unsafe"
#[test]
fn string_offsets() {
    assert_eq!(
        diags(&body("s = s + a;")),
        [
            d(Code::W0236, "OP_ADD_SF: string+float may be unsafe"),
            d(Code::E0125, "ftoi emulation: denormals are unsafe"),
            d(
                Code::W0235,
                "AddInt function not defined: cannot emulate int+int"
            ),
            d(
                Code::E0320,
                "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte"
            ),
        ]
    );
}

// spec FTEF-27.3: an entity offset needs the nextent builtin, else ADD_EI is refused
#[test]
fn entity_offsets() {
    assert_eq!(
        diags(&body("e = e + 1;")),
        [
            d(Code::W0235, "the nextent builtin is not defined"),
            d(Code::E0320, "Opcode \"+|ADD_EI\" is not supported."),
        ]
    );
}

// spec FTEF-27.4: `%` with no `mod` function is expanded into opcodes the target has
#[test]
fn remainders_expand() {
    assert_eq!(diags(&body("a = a % a;")), []);
}

// spec FTEF-27.4: a float shift with no `pow`, `floor` or `bitshift` is refused, and an integer
// shift by a constant is a product
#[test]
fn shifts() {
    assert_eq!(
        diags(&body("a = a << a;")),
        [
            d(
                Code::W0235,
                "bitshift function not defined: cannot emulate OP_LSHIFT_F*"
            ),
            d(Code::E0320, "Opcode \"<<|LSHIFT_F\" is not supported."),
        ]
    );
    assert_eq!(
        diags(&body("i = i << 2i;")),
        [
            d(
                Code::W0235,
                "MulInt function not defined: cannot emulate int*int"
            ),
            d(
                Code::E0320,
                "Opcode \"*|MUL_I\" not valid for target. Consider the use of: #pragma target fte"
            ),
        ]
    );
}

// spec FTEF-27.3: `<=>` of strings needs `strcmp`, else it is a fatal error
// spec QCD-4.27: E0321 "strcmp function not defined: cannot emulate string<=>string"
#[test]
fn string_comparison_needs_strcmp() {
    assert_eq!(
        diags(&body("a = s <=> s;")),
        [d(
            Code::E0321,
            "strcmp function not defined: cannot emulate string<=>string"
        )]
    );
}

// spec FTEF-27.1: an opcode the target has is emitted as it is
#[test]
fn the_fte_target_has_them() {
    let source = format!(
        "#pragma target fte\n{}",
        body("i = i + i; i = i * i; a = a + i;")
    );
    assert_eq!(diags(&source), []);
}

// spec FTEF-27.6: `&&` of an entity and a float is `OP_AND_F` on the standard target, with no
// diagnostic
#[test]
fn logic_of_other_types() {
    assert_eq!(diags(&body("a = (e && a);")), []);
}

// spec FTEF-27.8: a cast of an operand emits its conversion, which is emulated in turn: an
// unsigned value converted to a float is `OP_CONV_ITOF`, then the utof warning
// spec QCD-7.80: W0238 "utof emulation: will break if NAME is big"
#[test]
fn operand_conversions_are_emulated() {
    let source = "void() g = { local float a; local __uint32 u; a = (a + u); };\n";
    assert_eq!(
        diags(source),
        [
            d(Code::E0125, "itof emulation: denormals are unsafe u"),
            d(Code::W0238, "utof emulation: will break if u is big"),
        ]
    );
}

// spec FTEF-26.5: an enum's item and a `length` are constants, so their conversions fold and
// nothing is emulated (the pinned FTEQCC compiles this with no diagnostic, 2026-10-27)
#[test]
fn constants_are_not_emulated() {
    let source = "enum : int { StateOff, StateOn };\n.float state; entity self;\n\
                  string names[] = { \"a\", \"b\" };\n\
                  void() g = { local float a; if (self.state == StateOff) a = 1; \
                  for (float i = 0; i < names.length; i++) a = i; };\n";
    assert_eq!(diags(source), []);
}
