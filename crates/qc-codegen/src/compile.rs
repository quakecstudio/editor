// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/pr_comp.c, PR_CompileFile, PR_ParseDefs, PR_ParseImmediateStatements, PR_ParseState, PR_ParseStatement, PR_Expression, PR_Term, PR_ParseValue, PR_ParseFunctionCall

//! Lowering a resolved module's syntax trees to statements (QC-13 to QC-22).
//!
//! `qcc` emits code while it parses: each construct's statements in the order the parser meets
//! its parts. This module walks each file's tree in that order, with the definition every name
//! is taken from the semantic model, and builds the [`Program`] as id's `PR_CompileFile`,
//! `PR_ParseDefs`, `PR_ParseImmediateStatements`, `PR_ParseStatement` and `PR_Expression` do. The
//! type rules of expressions, which the semantic model does not check, are this module's
//! (QC-16 to QC-19): an operator's operands select its opcode by their basic types, a call's
//! arguments must have its parameters' types.

use std::collections::HashMap;

use qc_diag::{Code, Diagnostic, Span};
use qc_progs::Function;
use qc_progs::opcode::Opcode;
use qc_sema::lower::declares_wrap;
use qc_sema::{DefId, FileDiagnostic, FileId, FunctionType, ModuleIndex};
use qc_syntax::ast::{self, AstNode};
use qc_syntax::c_scalar::CScalar;
use qc_syntax::{Dialect, Parse, SyntaxToken};
use qc_types::Etype;

use crate::Internal;
use crate::names::Names;
use crate::ops::{self, Operator};
use crate::precache::Precaches;
use crate::program::{self, Literal, OFS_PARM0, OFS_RETURN, Program, ValueType, operand};

mod arrays;

/// Why a declaration's code stopped: an error in the program, which abandons the declaration
/// as `PR_ParseError` does (QC-24.2), or a construct the compiler cannot lower.
#[derive(Debug)]
pub(crate) enum Abort {
    /// A diagnostic; the next declaration is compiled.
    Error(Diagnostic),
    /// The compiler met something the semantic model should have refused.
    Internal(Internal, Span),
}

type Lowered<T> = Result<T, Abort>;

/// Attaches `span` to an [`Internal`] error.
trait At<T> {
    fn at(self, span: Span) -> Lowered<T>;
}

impl<T> At<T> for Result<T, Internal> {
    fn at(self, span: Span) -> Lowered<T> {
        self.map_err(|internal| Abort::Internal(internal, span))
    }
}

impl<T> At<T> for Option<T> {
    fn at(self, span: Span) -> Lowered<T> {
        self.ok_or(Abort::Internal(Internal::Unlowered, span))
    }
}

/// A value an expression leaves: the global it is in and what that global is.
#[derive(Clone, Debug)]
struct Value {
    ofs: u32,
    kind: ValueKind,
}

/// What holds a value, which decides its type.
#[derive(Clone, Debug)]
enum ValueKind {
    /// A definition of the list, at its place: its type is the definition's.
    Def(usize),
    /// An operator's temporary, of the type the operator gives (QC-21.5).
    Temp(ValueType),
    /// The return value's global, `def_ret`: its type is that of the last call made, read when
    /// an operator checks it, since `qcc` keeps one `def_ret` and retypes it at each call
    /// (QC-18.4, QC-18.6).
    Return,
    /// A definition read as another type of its basic type by a cast, with no statement
    /// (FTEF-30.6).
    Cast(ValueType),
}

/// `a[i].m` as a member access names it: the array, the index and the member's word and type.
struct ArrayMember {
    array: arrays::Array,
    index: ast::Expr,
    member: (u32, ValueType),
}

/// An entity's field as an access names it: the entity's and the field's globals, and the
/// `LOAD_*` row that reads it into a temporary of type `ty` (QC-16.1).
#[derive(Clone, Debug)]
struct FieldSlot {
    entity: u32,
    field: u32,
    load: Opcode,
    loaded: Etype,
    ty: ValueType,
}

impl FieldSlot {
    /// Whether a store to the field makes its `ADDRESS` after the stored value, as FTEQCC's
    /// `QCC_StoreSRefToRef` does (FTE-6.4), rather than before it, as id's `qcc` does
    /// (QC-17.3).
    fn address_after_value(dialect: Dialect) -> bool {
        dialect == Dialect::Fteqcc
    }
}

/// Where an assignment stores: a definition or temporary, or a field through the pointer
/// `ADDRESS` makes (QC-17.6, QC-17.3).
enum Target {
    /// `STORE_*` into the global.
    Value(Value),
    /// `STOREP_*` to the field `slot`, through the pointer at `address` when it is already
    /// made, else through one made after the value (FTE-6.4).
    Field {
        slot: FieldSlot,
        address: Option<u32>,
    },
    /// `STOREP_*` through the pointer at `address` to a value of type `ty`, `*p = x`
    /// (FTEF-6.18).
    Pointer { address: u32, ty: ValueType },
    /// A call of an array's setter, for an element by an index that is a value (FTE-13.11).
    Element(arrays::ElementStore),
}

/// A store's target as the value it computes reads it: a global, or a field read only when
/// the value asks for it (FTE-6.4).
enum Pending {
    Value(Value),
    Field(FieldSlot),
}

/// Whether a store reads its target's value: `x op= e`, `++x` and `x++` do, `=` does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Current {
    /// The target's value is read once, when the stored value asks for it (FTE-6.4).
    Read,
    /// Nothing is read.
    Unread,
}

/// The compiler's state while it lowers a module.
pub(crate) struct Compiler<'a> {
    index: &'a ModuleIndex,
    names: &'a Names,
    pub(crate) program: Program,
    pub(crate) precaches: Precaches,
    pub(crate) errors: Vec<FileDiagnostic>,
    /// The file being compiled.
    file: FileId,
    /// The string table offset of its name, `s_file`.
    s_file: i32,
    /// The function whose body is being compiled, `pr_scope`.
    scope: Option<DefId>,
    /// The end of the last `local` declaration's globals, `locals_end` (QC-21.4).
    locals_end: u32,
    /// The type of the last call's value, `def_ret.type` (QC-18.4).
    return_type: ValueType,
    /// Whether an operator of two literals is folded into the literal of its value
    /// (`--opt fold`, COMP-5).
    pub(crate) fold: bool,
    /// The dialect the files are read in, whose lexer reads each literal and builtin number.
    pub(crate) dialect: Dialect,
    /// The compiler flags of the file being compiled, each from the byte it holds from
    /// (FTEF-21.1).
    flags: Vec<(usize, qc_syntax::CompilerFlags)>,
    /// The lexer flags of the file being compiled, each from the byte it holds from, which
    /// read a builtin's engine name (FTEF-12.12).
    lex_flags: Vec<(usize, qc_lexer::LexFlags)>,
    /// The loops and switches the statement is in, innermost last, with the jumps their
    /// `break` and `continue` statements left to patch.
    jumps: Vec<Jumps>,
    /// The default value of each parameter of each function declared so far, an expression of
    /// the file it is written in (FTE-18.7).
    defaults: HashMap<DefId, Vec<Option<(FileId, ast::Expr)>>>,
    /// The `$parmN` globals made for the parameter slots past the eighth (FTE-18.6).
    extra_parms: HashMap<usize, u32>,
    /// In a `wrap` function's body, the definition `prior` names (FTEF-15.11).
    prior: Option<usize>,
    /// The array getters and setters made so far, emitted after every file (FTE-13.8,
    /// FTE-13.11).
    array_functions: Vec<arrays::ArrayFunction>,
    /// Each file's `s_file`, which a getter's function record takes from its array's file.
    s_files: HashMap<FileId, i32>,
}

/// One loop or switch: the `GOTO`s of its `break`s, and of its `continue`s for a loop.
#[derive(Debug, Default)]
struct Jumps {
    breaks: Vec<usize>,
    continues: Vec<usize>,
    /// Whether it is a loop, which `continue` belongs to; a switch passes it on.
    is_loop: bool,
}

impl<'a> Compiler<'a> {
    pub(crate) fn new(index: &'a ModuleIndex, names: &'a Names) -> Self {
        Self {
            index,
            names,
            program: Program::new(),
            precaches: Precaches::default(),
            errors: Vec::new(),
            file: FileId(0),
            s_file: 0,
            scope: None,
            locals_end: 0,
            return_type: ValueType::simple(Etype::Void),
            fold: false,
            dialect: Dialect::Vanilla,
            flags: Vec::new(),
            lex_flags: Vec::new(),
            jumps: Vec::new(),
            array_functions: Vec::new(),
            prior: None,
            extra_parms: HashMap::new(),
            defaults: HashMap::new(),
            s_files: HashMap::new(),
        }
    }

    /// `PR_CompileFile`: the file's name is copied into the string table, then each of its
    /// declarations is compiled; an error abandons the declaration it is in (QC-24.2,
    /// QC-25.4).
    pub(crate) fn compile_file(
        &mut self,
        file: FileId,
        name: &str,
        parse: &Parse,
    ) -> Result<(), (Internal, FileId, Span)> {
        self.file = file;
        self.flags = parse.flags().to_vec();
        self.lex_flags = parse.lex_flags().to_vec();
        self.s_file = self
            .program
            .copy_string(name.as_bytes())
            .map_err(|i| (i, file, Span::new(0, 0)))?;
        self.s_files.insert(file, self.s_file);
        let Some(tree) = parse.tree() else {
            return Ok(());
        };
        for item in tree.items() {
            self.scope = None;
            let result = if let Some(declaration) = ast::Declaration::cast(item.clone()) {
                self.declaration(&declaration)
            } else if let Some(enumeration) = ast::EnumDecl::cast(item) {
                self.enumeration(&enumeration)
            } else {
                Ok(())
            };
            match result {
                Ok(()) => {}
                Err(Abort::Error(diagnostic)) => {
                    self.errors.push(FileDiagnostic { file, diagnostic });
                }
                Err(Abort::Internal(internal, span)) => return Err((internal, file, span)),
            }
        }
        Ok(())
    }

    /// FTE-22.3, FTE-22.7: FTEQCC's first pass with `-Fautoproto`, which reads only the
    /// definitions: each top-level declarator's definition, and each enum item's, made in the
    /// order of the files and of their text, before any body is compiled.
    pub(crate) fn prototype_file(
        &mut self,
        file: FileId,
        parse: &Parse,
    ) -> Result<(), (Internal, FileId, Span)> {
        self.file = file;
        let Some(tree) = parse.tree() else {
            return Ok(());
        };
        for item in tree.items() {
            self.scope = None;
            let result = if let Some(declaration) = ast::Declaration::cast(item.clone()) {
                declaration.declarators().try_for_each(|declarator| {
                    let name = declarator.name().at(declaration.syntax().span())?;
                    let id = self.declared(&name)?;
                    self.record_defaults(id, declaration.ty().as_ref(), &declarator);
                    self.program
                        .get_def(self.index, id)
                        .at(name.span())
                        .map(drop)
                })
            } else if let Some(enumeration) = ast::EnumDecl::cast(item) {
                self.enumeration(&enumeration)
            } else {
                Ok(())
            };
            match result {
                Ok(()) | Err(Abort::Error(_)) => {}
                Err(Abort::Internal(internal, span)) => return Err((internal, file, span)),
            }
        }
        Ok(())
    }

    /// FTE-18.7: the default values the parameters of function `id` are declared with, from
    /// the declarator's own list or its type's, kept with the file they are written in.
    fn record_defaults(&mut self, id: DefId, ty: Option<&ast::Type>, declarator: &ast::Declarator) {
        let list = declarator.params().or_else(|| match ty {
            Some(ast::Type::Function(function)) => function.params(),
            _ => None,
        });
        let Some(list) = list else {
            return;
        };
        let defaults: Vec<Option<(FileId, ast::Expr)>> = list
            .params()
            .map(|param| param.default_value().map(|value| (self.file, value)))
            .collect();
        if defaults.iter().any(Option::is_some) {
            self.defaults.insert(id, defaults);
        }
    }

    /// The definition a top-level declarator's name `name` makes or redeclares.
    fn declared(&self, name: &SyntaxToken) -> Lowered<DefId> {
        match self.names.def(self.file, name.span(), None) {
            Some(id) => Ok(id),
            None => self
                .names
                .global(&name.text_lossless())
                .ok_or(Abort::Internal(Internal::Unresolved, name.span())),
        }
    }

    /// An error at `span`.
    fn error<T>(code: Code, message: String, span: Span) -> Lowered<T> {
        Err(Abort::Error(Diagnostic {
            code,
            message,
            span,
        }))
    }

    /// The definition the name token `token` is (QC-12).
    fn def_of(&self, token: &SyntaxToken) -> Lowered<DefId> {
        self.names
            .def(self.file, token.span(), self.scope)
            .ok_or(Abort::Internal(Internal::Unresolved, token.span()))
    }

    /// The place in the list of the definition `token` names, made now if it is new: the
    /// allocating `PR_GetDef` a declaration calls (QC-11, QC-21.2).
    fn declare(&mut self, token: &SyntaxToken) -> Lowered<usize> {
        let id = self.def_of(token)?;
        self.program.get_def(self.index, id).at(token.span())
    }

    /// The type of `value` as an operator reads it.
    fn type_of(&self, value: &Value) -> ValueType {
        match &value.kind {
            ValueKind::Def(place) => self
                .program
                .def(*place)
                .map_or(ValueType::simple(Etype::Void), |d| d.ty.clone()),
            ValueKind::Temp(ty) | ValueKind::Cast(ty) => ty.clone(),
            ValueKind::Return => self.return_type.clone(),
        }
    }

    // ---------------------------------------------------------------------------------------
    // Declarations
    // ---------------------------------------------------------------------------------------

    /// `PR_ParseDefs` at file level (QC-11): each name is made or found, and an initialiser
    /// defines a function (QC-13) or stores a constant (QC-11.6).
    fn declaration(&mut self, declaration: &ast::Declaration) -> Lowered<()> {
        for declarator in declaration.declarators() {
            let name = declarator.name().at(declaration.syntax().span())?;
            let id = self.declared(&name)?;
            self.record_defaults(id, declaration.ty().as_ref(), &declarator);
            let place = self.program.get_def(self.index, id).at(name.span())?;
            match declarator.initializer() {
                None => {}
                Some(ast::Initializer::Value(expr)) => {
                    let span = expr.syntax().span();
                    // A literal is stored as it is (QC-20); under FTEQCC an expression is the
                    // value the semantic model folded it to (FTEF-34.9).
                    if let Some(other) = self.field_alias(place, &expr)? {
                        self.program
                            .alias_field(self.index, place, other)
                            .at(span)?;
                    } else if let Some(word) = self.integer_literal(place, &expr) {
                        self.program.initialise_words(place, &[word]).at(span)?;
                    } else if let Some(literal) = literal_of(&expr, self.dialect) {
                        self.program.initialise(place, &literal).at(span)?;
                    } else {
                        let value = self.index.initial_values.get(&id).at(span)?;
                        self.program
                            .initialise_words(place, value.words())
                            .at(span)?;
                    }
                }
                Some(ast::Initializer::Body(body)) => {
                    self.function(
                        (declaration, &declarator),
                        &name,
                        (id, place),
                        FunctionInit::Body(&body),
                    )?;
                }
                Some(ast::Initializer::Builtin(builtin)) => {
                    self.function(
                        (declaration, &declarator),
                        &name,
                        (id, place),
                        FunctionInit::Builtin(&builtin),
                    )?;
                }
                Some(ast::Initializer::Aggregate(aggregate)) => {
                    let span = aggregate.syntax().span();
                    let def = self.program.def(place).at(span)?;
                    let (ty, count) = (def.ty.clone(), def.count);
                    let words = self.aggregate_words(&ty, count, &aggregate)?;
                    self.program.initialise_words(place, &words).at(span)?;
                }
            }
        }
        Ok(())
    }

    /// FTEF-19.76: under FTEQCC, the field `expr` names when the definition at `place`, a
    /// field, is defined with it as its value; `None` for any other value.
    fn field_alias(&mut self, place: usize, expr: &ast::Expr) -> Lowered<Option<usize>> {
        let is_field = |program: &Program, place: usize| {
            program
                .def(place)
                .is_some_and(|def| def.ty.field_inner().is_some())
        };
        if !self.dialect.has_fteqcc_extensions() || !is_field(&self.program, place) {
            return Ok(None);
        }
        let ast::Expr::Name(name) = expr else {
            return Ok(None);
        };
        let token = name.name().at(expr.syntax().span())?;
        let other = self.declare(&token)?;
        Ok(is_field(&self.program, other).then_some(other))
    }

    /// FTE-15.5: an `enum`'s items, each a constant global made in turn and initialised with
    /// its value.
    fn enumeration(&mut self, enumeration: &ast::EnumDecl) -> Lowered<()> {
        let span = enumeration.syntax().span();
        if enumeration.declarators().next().is_some() {
            return Err(Abort::Internal(Internal::Unsupported, span));
        }
        for item in enumeration.enumerators() {
            let name = item
                .child_tokens()
                .find(|t| t.kind() == qc_syntax::SyntaxKind::Name)
                .at(item.span())?;
            let place = self.declare(&name)?;
            let id = self.def_of(&name)?;
            let value = self.index.initial_values.get(&id).at(name.span())?;
            self.program
                .initialise_words(place, value.words())
                .at(name.span())?;
        }
        Ok(())
    }

    /// A function definition, the function branch of `PR_ParseDefs` and
    /// `PR_ParseImmediateStatements` (QC-13): a body's parameters, state header and
    /// statements, or a builtin number; then the function's record (QC-13.9, QC-21.4,
    /// QC-25.2).
    fn function(
        &mut self,
        (declaration, declarator): (&ast::Declaration, &ast::Declarator),
        name: &SyntaxToken,
        (id, place): (DefId, usize),
        init: FunctionInit<'_>,
    ) -> Lowered<()> {
        let span = name.span();
        let def = self.program.def(place).at(span)?.clone();
        let function_type = def.ty.function().at(span)?.clone();
        self.set_line(span);
        let wrapped = self.wrap(declaration, &def, init, span)?;
        let locals_start = self.program.next_global().at(span)?;
        self.locals_end = locals_start;
        self.scope = Some(id);
        let mut record_name = def.name.clone();
        let builtin = matches!(init, FunctionInit::Builtin(_));
        let first_statement = match init {
            FunctionInit::Builtin(builtin) => {
                // QC-13.3: `#n` is builtin `n`; no parameter or local storage is made.
                let whole = builtin.syntax().span();
                let number = match builtin.number() {
                    Some(token) => qc_syntax::builtin_binum(&token.text_lossless(), self.dialect)
                        .at(token.span())?,
                    // FTE-17.5: FTEQCC binds builtin 0 when it reads a string or a name in
                    // place of the number; the parser refuses any other builtin without one.
                    None => 0,
                };
                // FTE-23.17: FTEQCC names a builtin's function after its engine name, when it
                // has one, read with the lexer flags it was lexed with (FTEF-12.12).
                let flags = qc_lexer::LexFlags::at(&self.lex_flags, whole.start);
                if let Some(name) = builtin.engine_name(flags) {
                    record_name = name;
                }
                number
                    .checked_neg()
                    .ok_or(Abort::Internal(Internal::TooLarge, whole))?
            }
            FunctionInit::Body(body) => {
                self.parameters(declaration, declarator, span)?;
                // FTE-21.6: FTEQCC's record counts the parameters among the locals, so a call
                // that enters the function again restores them; id's `qcc` counts only up to
                // the last `local` declaration (QC-21.4).
                if self.dialect == Dialect::Fteqcc {
                    self.locals_end = self.program.next_global().at(span)?;
                }
                let code = operand_index(self.program.statements.len()).at(span)?;
                if self.dialect.has_fteqcc_extensions() && function_type.variadic {
                    self.variadic_prologue(function_type.params.len(), span)?;
                }
                if let Some(state) = body.state() {
                    self.state(&state)?;
                }
                if let Some(block) = body.block() {
                    for statement in block.statements() {
                        self.statement(&statement)?;
                    }
                }
                // QC-13.7: the body ends with `DONE`.
                self.program.emit(Opcode::Done, 0, 0);
                code
            }
        };
        self.scope = None;
        self.prior = None;
        // `PR_ParseDefs` marks the function initialised here, which nothing reads for a
        // function: no literal is a function (QC-20.2), and no function is saved (QC-21.6).
        if wrapped.is_none() {
            let number = operand_index(self.program.functions.len()).at(span)?;
            self.program
                .set_words(def.ofs, &[u32::from_ne_bytes(number.to_ne_bytes())])
                .at(span)?;
        }
        let s_name = self.program.copy_string(&latin1(&record_name)).at(span)?;
        let numparms = if function_type.variadic {
            -1
        } else {
            i32::try_from(function_type.params.len())
                .map_err(|_| Abort::Internal(Internal::TooLarge, span))?
        };
        let mut parm_size = [0_u8; 8];
        for (size, param) in parm_size.iter_mut().zip(&function_type.params) {
            *size = if ValueType::Of(param.clone()).etype() == Etype::Vector {
                3
            } else {
                1
            };
        }
        let mut locals = self
            .locals_end
            .checked_sub(locals_start)
            .and_then(|l| i32::try_from(l).ok())
            .at(span)?;
        let mut parm_start = operand(locals_start).at(span)?;
        let mut numparms = numparms;
        // FTE-23.18: FTEQCC's `QCC_WriteData` writes a builtin's record with no parameter or
        // local slots and its named parameters' count, at most 8, whether or not it is
        // variadic.
        if builtin && self.dialect == Dialect::Fteqcc {
            parm_start = 0;
            locals = 0;
            numparms = i32::try_from(function_type.params.len().min(parm_size.len()))
                .map_err(|_| Abort::Internal(Internal::TooLarge, span))?;
            parm_size = [0; 8];
        }
        let record = Function {
            first_statement,
            parm_start,
            locals,
            profile: 0,
            s_name,
            s_file: self.s_file,
            numparms,
            parm_size,
        };
        match wrapped {
            Some(number) => {
                *self.program.functions.get_mut(number).at(span)? = record;
            }
            None => self.program.functions.push(record),
        }
        Ok(())
    }

    /// FTE-18.6: a variadic function's first statements, `STORE_V` of each parameter slot after
    /// the `named` ones to the element of a new static `__va_list` of 24 vectors, a slot past
    /// the eighth from its `$parmN` global.
    fn variadic_prologue(&mut self, named: usize, span: Span) -> Lowered<()> {
        const VA_COUNT: u32 = 24;
        const MAX_PARMS: usize = 8;
        let vector = ValueType::simple(Etype::Vector);
        let list = self
            .program
            .generated_array("__va_list", vector.clone(), VA_COUNT)
            .at(span)?;
        let base = self.program.def(list).at(span)?.ofs;
        for i in 0..VA_COUNT {
            let u = named
                .checked_add(
                    usize::try_from(i).map_err(|_| Abort::Internal(Internal::TooLarge, span))?,
                )
                .at(span)?;
            let slot = if u < MAX_PARMS {
                parm_slot(u).at(span)?
            } else if let Some(&slot) = self.extra_parms.get(&u) {
                slot
            } else {
                let made = self
                    .program
                    .generated(&format!("$parm{u}"), vector.clone(), 3)
                    .at(span)?;
                let slot = self.program.def(made).at(span)?.ofs;
                self.extra_parms.insert(u, slot);
                slot
            };
            let to = i
                .checked_mul(3)
                .and_then(|w| w.checked_add(base))
                .at(span)?;
            self.program.emit(
                Opcode::StoreV,
                operand(slot).at(span)?,
                operand(to).at(span)?,
            );
        }
        Ok(())
    }

    /// FTEF-15.11: for a `wrap` definition's body, the function it wraps moves to a new
    /// record at the end, `prior*NAME`, as `QCC_PR_GenerateQCFunction` moves it, and `prior` in
    /// the body is a constant of its number; the wrapper takes the old record's number, which
    /// is returned. `None` for any other definition.
    fn wrap(
        &mut self,
        declaration: &ast::Declaration,
        def: &program::Def,
        init: FunctionInit<'_>,
        span: Span,
    ) -> Lowered<Option<usize>> {
        let wraps = self.dialect.has_fteqcc_extensions()
            && matches!(init, FunctionInit::Body(_))
            && declares_wrap(declaration);
        let number = usize::try_from(self.program.word(def.ofs))
            .map_err(|_| Abort::Internal(Internal::TooLarge, span))?;
        if !wraps || number == 0 {
            return Ok(None);
        }
        let mut moved = *self.program.functions.get(number).at(span)?;
        moved.s_name = self
            .program
            .copy_string(&latin1(&format!("prior*{}", def.name)))
            .at(span)?;
        let prior_number = operand_index(self.program.functions.len()).at(span)?;
        self.program.functions.push(moved);
        let prior = self
            .program
            .generated("prior", def.ty.clone(), 1)
            .at(span)?;
        self.program
            .initialise_words(prior, &[u32::from_ne_bytes(prior_number.to_ne_bytes())])
            .at(span)?;
        self.prior = Some(prior);
        Ok(Some(number))
    }

    /// The parameters of a body, made at the next globals in order (QC-13.11, QC-13.5): the
    /// names id's qcc gives them from the defining declaration's type, or after the name of
    /// FTEQCC's C-style definition `float f(float x) { ... }` (FTE-17.1).
    fn parameters(
        &mut self,
        declaration: &ast::Declaration,
        declarator: &ast::Declarator,
        span: Span,
    ) -> Lowered<()> {
        let list = match (declarator.params(), declaration.ty()) {
            (Some(list), _) => list,
            (None, Some(ast::Type::Function(function))) => match function.params() {
                Some(list) => list,
                None => return Ok(()),
            },
            (None, _) => return Err(Abort::Internal(Internal::Unlowered, span)),
        };
        let names = if self.dialect == Dialect::Vanilla {
            list.qcc_parm_names()
        } else {
            list.params().map(|param| param.name()).collect()
        };
        for name in names.into_iter().flatten() {
            self.declare(&name)?;
        }
        Ok(())
    }

    /// `PR_ParseState`: `STATE frame, next`, the frame a shared literal, the next function
    /// made as a global `void()` if it is new (QC-13.6, QC-12.6).
    fn state(&mut self, state: &ast::State) -> Lowered<()> {
        let span = state.syntax().span();
        let frame = state.frame().at(span)?;
        let literal = literal_of_token(&frame, self.dialect).at(frame.syntax().span())?;
        let frame_place = self.program.immediate(&literal).at(span)?;
        let next = state.next().at(span)?;
        let next_place = self.declare(&next)?;
        let a = self.program.def(frame_place).at(span)?.ofs;
        let b = self.program.def(next_place).at(span)?.ofs;
        self.program
            .emit(Opcode::State, operand(a).at(span)?, operand(b).at(span)?);
        Ok(())
    }

    // ---------------------------------------------------------------------------------------
    // Statements
    // ---------------------------------------------------------------------------------------

    /// `PR_ParseStatement` (QC-14, QC-22.1).
    /// Records `span`'s start as where the statements emitted from here come from (QC-27.19).
    fn set_line(&mut self, span: Span) {
        self.program.origin = Some((self.file, span.start));
    }

    fn statement(&mut self, statement: &ast::Stmt) -> Lowered<()> {
        let span = statement.syntax().span();
        self.set_line(span);
        match statement {
            ast::Stmt::Block(block) => {
                for inner in block.statements() {
                    self.statement(&inner)?;
                }
            }
            ast::Stmt::Return(ret) => {
                // QC-14.3, QC-19.5: the value is not checked against the return type.
                let a = match ret.value() {
                    Some(value) => operand(self.expr(&value)?.ofs).at(span)?,
                    None => 0,
                };
                self.program.emit(Opcode::Return, a, 0);
            }
            ast::Stmt::While(w) => {
                // QC-14.5: the condition, `IFNOT` past the loop, the body, `GOTO` back.
                let top = self.program.statements.len();
                let condition = self.expr(&w.condition().at(span)?)?;
                let test = self.unless(&condition, span)?;
                self.jumps.push(Jumps {
                    is_loop: true,
                    ..Jumps::default()
                });
                self.statement(&w.body().at(span)?)?;
                let back = branch(self.program.statements.len(), top).at(span)?;
                self.program.emit(Opcode::Goto, back, 0);
                self.patch_b(test, span)?;
                self.end_jumps(top, span)?;
            }
            ast::Stmt::Do(d) => {
                // QC-14.6: the body, the condition, `IF` back to the body.
                let top = self.program.statements.len();
                self.jumps.push(Jumps {
                    is_loop: true,
                    ..Jumps::default()
                });
                self.statement(&d.body().at(span)?)?;
                let test_at = self.program.statements.len();
                let condition = self.expr(&d.condition().at(span)?)?;
                let (op, tested) = self.test(&condition, true, span)?;
                let back = branch(self.program.statements.len(), top).at(span)?;
                self.program.emit(op, operand(tested).at(span)?, back);
                self.end_jumps(test_at, span)?;
            }
            ast::Stmt::Local(local) => self.local(local)?,
            ast::Stmt::If(i) => {
                // QC-14.4: `IFNOT` past the statement; with `else`, a `GOTO` past the other.
                let condition = self.expr(&i.condition().at(span)?)?;
                let test = self.unless(&condition, span)?;
                self.statement(&i.then_branch().at(span)?)?;
                if let Some(otherwise) = i.else_branch() {
                    let skip = self.program.emit(Opcode::Goto, 0, 0);
                    self.patch_b(test, span)?;
                    self.statement(&otherwise)?;
                    let past = branch(skip, self.program.statements.len()).at(span)?;
                    if let Some(s) = self.program.statements.get_mut(skip) {
                        s.a = past;
                    }
                } else {
                    self.patch_b(test, span)?;
                }
            }
            ast::Stmt::Expr(e) => {
                // QC-14.9: any expression, its statements emitted.
                self.discard(&e.expr().at(span)?)?;
            }
            ast::Stmt::Break(_) => {
                let jump = self.program.emit(Opcode::Goto, 0, 0);
                let jumps = self.jumps.last_mut().at(span)?;
                jumps.breaks.push(jump);
            }
            ast::Stmt::Continue(_) => {
                let jump = self.program.emit(Opcode::Goto, 0, 0);
                let jumps = self.jumps.iter_mut().rev().find(|j| j.is_loop).at(span)?;
                jumps.continues.push(jump);
            }
            ast::Stmt::Empty(_) => {}
            ast::Stmt::Switch(switch) => self.switch(switch, span)?,
            ast::Stmt::For(f) => self.for_loop(f, span)?,
            ast::Stmt::Label(_)
            | ast::Stmt::Goto(_)
            | ast::Stmt::Loop(_)
            | ast::Stmt::Thinktime(_)
            | ast::Stmt::State(_)
            | ast::Stmt::Using(_)
            | ast::Stmt::Exit(_)
            | ast::Stmt::Asm(_)
            | ast::Stmt::Extern(_) => return Err(Abort::Internal(Internal::Unsupported, span)),
        }
        Ok(())
    }

    /// Points the branch of statement `at`, an `IF` or `IFNOT`, at the next statement
    /// (QC-22.2).
    fn patch_b(&mut self, at: usize, span: Span) -> Lowered<()> {
        let to = branch(at, self.program.statements.len()).at(span)?;
        if let Some(s) = self.program.statements.get_mut(at) {
            s.b = to;
        }
        Ok(())
    }

    /// `local`: `PR_ParseDefs` in the function (QC-11.10); its globals end the function's
    /// locals so far (QC-21.4).
    fn local(&mut self, local: &ast::LocalDecl) -> Lowered<()> {
        let span = local.syntax().span();
        for declarator in local.declarators() {
            let name = declarator.name().at(span)?;
            let place = self.declare(&name)?;
            match declarator.initializer() {
                None => {}
                // Under FTEQCC a local's initialiser is its first store, made each time the
                // function runs (FTEF-32.4); `qcc` initialises the global once (QC-11.10).
                Some(ast::Initializer::Value(expr)) if self.dialect.has_fteqcc_extensions() => {
                    let to = self.def_value(place, span)?;
                    let value = self.expr(&expr)?;
                    self.store_to(&to, &value, expr.syntax().span())?;
                }
                Some(ast::Initializer::Value(expr)) => {
                    let literal = literal_of(&expr, self.dialect).at(expr.syntax().span())?;
                    self.program.initialise(place, &literal).at(span)?;
                }
                // FTE-13.1: `vector v = {x, y, z};`, each element stored to its component in
                // turn, as `QCC_PR_ParseInitializerType` stores an aggregate's elements.
                Some(ast::Initializer::Aggregate(aggregate))
                    if self.dialect.has_fteqcc_extensions()
                        && self.type_of(&self.def_value(place, span)?).etype() == Etype::Vector =>
                {
                    let base = self.def_value(place, span)?.ofs;
                    let elements: Vec<ast::Expr> = aggregate.elements().collect();
                    if elements.len() != 3 || aggregate.syntax().children().count() != 3 {
                        return Err(Abort::Internal(
                            Internal::Unsupported,
                            aggregate.syntax().span(),
                        ));
                    }
                    for (i, element) in (0_u32..3).zip(elements) {
                        let value = self.expr(&element)?;
                        let at = element.syntax().span();
                        if self.type_of(&value).etype() != Etype::Float {
                            return Self::mismatch(Operator::Assign, at);
                        }
                        let component = base.checked_add(i).at(at)?;
                        self.program.emit(
                            Opcode::StoreF,
                            operand(value.ofs).at(at)?,
                            operand(component).at(at)?,
                        );
                    }
                }
                // FTEF-32.13: a constant local's aggregate is its value at compile time, as a
                // global's is.
                Some(ast::Initializer::Aggregate(aggregate))
                    if self.dialect.has_fteqcc_extensions() && self.declared_constant(place) =>
                {
                    let span = aggregate.syntax().span();
                    let def = self.program.def(place).at(span)?;
                    let (ty, count) = (def.ty.clone(), def.count);
                    let words = self.aggregate_words(&ty, count, &aggregate)?;
                    self.program.initialise_words(place, &words).at(span)?;
                }
                Some(
                    ast::Initializer::Body(_)
                    | ast::Initializer::Builtin(_)
                    | ast::Initializer::Aggregate(_),
                ) => return Err(Abort::Internal(Internal::Unsupported, span)),
            }
        }
        self.locals_end = self.program.next_global().at(span)?;
        Ok(())
    }

    // ---------------------------------------------------------------------------------------
    // Expressions
    // ---------------------------------------------------------------------------------------

    /// `PR_Expression` and `PR_Term` (QC-15 to QC-19): the statements of `expr`, operands left
    /// to right, each operator's after its operands' (QC-15.9).
    fn expr(&mut self, expr: &ast::Expr) -> Lowered<Value> {
        let span = expr.syntax().span();
        match expr {
            ast::Expr::Literal(literal) => {
                let value = literal_of_token(literal, self.dialect).at(span)?;
                let place = self.program.immediate(&value).at(span)?;
                self.def_value(place, span)
            }
            ast::Expr::Name(name) => {
                let token = name.name().at(span)?;
                // FTEF-15.11: in a `wrap` function `prior` is the function it wraps.
                if let Some(prior) = self.prior
                    && token.text_lossless() == "prior"
                    && self
                        .names
                        .def(self.file, token.span(), self.scope)
                        .is_none()
                {
                    return self.def_value(prior, span);
                }
                let id = self.def_of(&token)?;
                // Under FTEQCC a name may be used before the definition it names is read, in a
                // second autoprototyping pass (FTE-22.3), whose first pass made it (FTE-22.7);
                // one the first pass did not make is made where it is first used.
                let place = match self.program.place(id) {
                    Some(place) => place,
                    None if self.dialect.has_fteqcc_extensions() => {
                        self.program.get_def(self.index, id).at(span)?
                    }
                    None => return Err(Abort::Internal(Internal::Unallocated, span)),
                };
                if let Some(float) = self.integer_constant(place) {
                    let place = self.program.immediate(&Literal::Float(float)).at(span)?;
                    return self.def_value(place, span);
                }
                self.def_value(place, span)
            }
            ast::Expr::Paren(paren) => self.expr(&paren.expr().at(span)?),
            ast::Expr::Not(not) => {
                // QC-16.4: `NOT_*` by the operand's type.
                let value = self.expr(&not.operand().at(span)?)?;
                let ty = self.type_of(&value).etype();
                let Some(op) = ops::not_opcode(ty) else {
                    return Self::error(Code::E0302, "type mismatch for !".to_owned(), span);
                };
                let ofs = self
                    .program
                    .emit_result(op, value.ofs, 0, Etype::Float)
                    .at(span)?;
                Ok(Value {
                    ofs,
                    kind: ValueKind::Temp(ValueType::simple(Etype::Float)),
                })
            }
            ast::Expr::Binary(binary) => self.binary(binary, span),
            ast::Expr::Field(field) => self.field(field, span),
            ast::Expr::Call(call) => self.call(call, span),
            ast::Expr::Postfix(postfix) => self.postfix(postfix, span),
            ast::Expr::Unary(unary) => self.unary(unary, span),
            ast::Expr::Index(index) => self.index(index, span),
            ast::Expr::Ternary(ternary) => self.ternary(ternary, span),
            ast::Expr::Vector(vector) => self.vector(vector, span),
            ast::Expr::Cast(cast) => self.cast(cast, span),
            ast::Expr::Sizeof(_) => Err(Abort::Internal(Internal::Unsupported, span)),
        }
    }

    /// FTE-15.7: an initialised integer constant, an `enum : int`'s item, read where the
    /// vanilla opcodes take a float: the float its value converts to, `QCC_SupplyConversion`
    /// of a constant.
    fn integer_constant(&self, place: usize) -> Option<f32> {
        let def = self.program.def(place)?;
        let ValueType::Of(ty) = &def.ty else {
            return None;
        };
        if !self.constant(place) {
            return None;
        }
        let word = self.program.word(def.ofs);
        #[expect(
            clippy::cast_precision_loss,
            reason = "spec FTE-15.7: the conversion is C's (float) of the integer"
        )]
        match ty.fteqcc_scalar()? {
            CScalar::Integer => Some(i32::from_ne_bytes(word.to_ne_bytes()) as f32),
            CScalar::Uint => Some(word as f32),
            _ => None,
        }
    }

    /// FTE-15.8: a number literal initialising an `int` or `unsigned` global is the integer
    /// it converts to, C's conversion of the float the lexer reads (`QCC_SupplyConversion` of a
    /// constant), not the float's bits.
    fn integer_literal(&self, place: usize, expr: &ast::Expr) -> Option<u32> {
        let ValueType::Of(ty) = &self.program.def(place)?.ty else {
            return None;
        };
        let Some(Literal::Float(value)) = literal_of(expr, self.dialect) else {
            return None;
        };
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "spec FTE-15.8: the conversion is C's (int) or (unsigned) of the float"
        )]
        match ty.fteqcc_scalar()? {
            CScalar::Integer => Some(u32::from_ne_bytes((value as i32).to_ne_bytes())),
            CScalar::Uint => Some(value as u32),
            _ => None,
        }
    }

    /// Whether the definition at `place` is a constant with its value, as
    /// `QCC_SRef_EvalConst` reads one: a literal, or an initialised definition the model holds
    /// constant (FTEF-19.3).
    /// Whether the definition at `place` is one the semantic model makes a constant, initialised
    /// or not yet.
    fn declared_constant(&self, place: usize) -> bool {
        self.program.def(place).is_some_and(|def| def.constant)
    }

    fn constant(&self, place: usize) -> bool {
        self.program.def(place).is_some_and(|def| {
            def.initialized
                && def.count.is_none()
                && def.id.map_or(def.name == "IMMEDIATE", |id| {
                    self.index.constants.contains(&id)
                })
        })
    }

    /// Ends the innermost loop or switch: its `break`s go to the next statement, and a loop's
    /// `continue`s to `again`.
    fn end_jumps(&mut self, again: usize, span: Span) -> Lowered<()> {
        let jumps = self.jumps.pop().at(span)?;
        let end = self.program.statements.len();
        for (list, to) in [(jumps.breaks, end), (jumps.continues, again)] {
            for at in list {
                let offset = branch(at, to).at(span)?;
                if let Some(s) = self.program.statements.get_mut(at) {
                    s.a = offset;
                }
            }
        }
        Ok(())
    }

    /// FTEQCC's `for (init; condition; step) body` (FTE-2.1): the initialiser, then the
    /// condition with `IFNOT` past the loop, the body, the step, `GOTO` back to the condition;
    /// `continue` goes to the step.
    fn for_loop(&mut self, f: &ast::ForStmt, span: Span) -> Lowered<()> {
        if let Some(init) = f.init() {
            self.effects(&init, span)?;
        }
        let top = self.program.statements.len();
        let test = match f.condition() {
            Some(condition) => {
                let condition = self.clause(&condition, span)?.at(span)?;
                Some(self.unless(&condition, span)?)
            }
            None => None,
        };
        self.jumps.push(Jumps {
            is_loop: true,
            ..Jumps::default()
        });
        self.statement(&f.body().at(span)?)?;
        let step_at = self.program.statements.len();
        if let Some(step) = f.step() {
            self.effects(&step, span)?;
        }
        let back = branch(self.program.statements.len(), top).at(span)?;
        self.program.emit(Opcode::Goto, back, 0);
        if let Some(test) = test {
            self.patch_b(test, span)?;
        }
        self.end_jumps(step_at, span)
    }

    /// A `for` header clause: its expressions in turn and a local declaration (FTEF-16.9);
    /// the value of the last expression, the condition's.
    fn clause(&mut self, clause: &qc_syntax::SyntaxNode, span: Span) -> Lowered<Option<Value>> {
        let items: Vec<_> = clause.children().collect();
        let mut last = None;
        for (i, item) in items.iter().enumerate() {
            if let Some(local) = ast::LocalDecl::cast(item.clone()) {
                self.local(&local)?;
                continue;
            }
            let expr = ast::Expr::cast(item.clone()).at(span)?;
            if i.saturating_add(1) == items.len() {
                last = Some(self.expr(&expr)?);
            } else {
                self.discard(&expr)?;
            }
        }
        Ok(last)
    }

    /// A `for` header clause whose values are not read, its init or its step (FTEF-16.9).
    fn effects(&mut self, clause: &qc_syntax::SyntaxNode, span: Span) -> Lowered<()> {
        for item in clause.children() {
            if let Some(local) = ast::LocalDecl::cast(item.clone()) {
                self.local(&local)?;
            } else {
                self.discard(&ast::Expr::cast(item).at(span)?)?;
            }
        }
        Ok(())
    }

    /// An expression whose value is not read: under FTEQCC a postfix `x++` or `x--` is then the
    /// store of `x` plus or minus 1 with no copy of the old value, as FTEQCC copies it only
    /// when it is read (FTEF-29.13).
    fn discard(&mut self, expr: &ast::Expr) -> Lowered<()> {
        let span = expr.syntax().span();
        let mut inner = expr.clone();
        while let ast::Expr::Paren(paren) = &inner {
            inner = paren.expr().at(span)?;
        }
        if self.dialect == Dialect::Fteqcc
            && let ast::Expr::Postfix(postfix) = &inner
        {
            let target = postfix.operand().at(span)?;
            self.increment(&target, postfix_step(postfix), span)?;
        } else {
            self.expr(expr)?;
        }
        Ok(())
    }

    /// `x` stored as `x + step` (FTE-5.1), the new value.
    fn increment(&mut self, target: &ast::Expr, step: f32, span: Span) -> Lowered<Value> {
        let inc = self.float_literal(step, span)?;
        self.store(target, span, Current::Read, |compiler, current| {
            let current = compiler.current(current, span)?;
            compiler.operate(Operator::Add, &current, &inc, (span, span))
        })
    }

    /// FTEQCC's `switch (e) { case v: ... default: ... }` (FTE-3.1) on the vanilla opcodes: the
    /// value read once, `GOTO` past the body to the tests, the body with its labels, then each
    /// `case` tested in order by `EQ_*` of the value's type (a range `case a .. b:` by
    /// `GE_F`, `LE_F` and `AND`) with `IF` to the label, and `GOTO` to `default:` or past the
    /// switch; `break` goes past it.
    fn switch(&mut self, switch: &ast::SwitchStmt, span: Span) -> Lowered<()> {
        // FTE-3.5: a call's value is held before the body's calls overwrite the return global.
        let value = self.expr(&switch.scrutinee().at(span)?)?;
        let value = self.hold(value, span)?;
        let to_tests = self.program.emit(Opcode::Goto, 0, 0);
        self.jumps.push(Jumps::default());
        let mut cases: Vec<(ast::CaseLabel, usize)> = Vec::new();
        let block = switch.block().at(span)?;
        for node in block.syntax().children() {
            if let Some(label) = ast::CaseLabel::cast(node.clone()) {
                cases.push((label, self.program.statements.len()));
            } else if let Some(statement) = ast::Stmt::cast(node) {
                self.statement(&statement)?;
            }
        }
        let past_tests = self.program.emit(Opcode::Goto, 0, 0);
        let tests = self.program.statements.len();
        self.set_goto(to_tests, tests, span)?;
        let mut default = None;
        for (label, at) in &cases {
            let label_span = label.syntax().span();
            let Some(case) = label.value() else {
                default = Some(*at);
                continue;
            };
            let low = self.expr(&case)?;
            let hit = match label.range_end() {
                Some(end) => {
                    let high = self.expr(&end)?;
                    let spans = (label_span, label_span);
                    let above = self.operate(Operator::Ge, &value, &low, spans)?;
                    let below = self.operate(Operator::Le, &value, &high, spans)?;
                    self.operate(Operator::And, &above, &below, spans)?
                }
                None => self.operate(Operator::Eq, &value, &low, (label_span, label_span))?,
            };
            let to = branch(self.program.statements.len(), *at).at(label_span)?;
            self.program
                .emit(Opcode::If, operand(hit.ofs).at(label_span)?, to);
        }
        let fallback = self.program.emit(Opcode::Goto, 0, 0);
        let end = self.program.statements.len();
        self.set_goto(fallback, default.unwrap_or(end), span)?;
        self.set_goto(past_tests, end, span)?;
        self.end_jumps(end, span)
    }

    /// Points the `GOTO` at statement `at` at statement `to`.
    fn set_goto(&mut self, at: usize, to: usize, span: Span) -> Lowered<()> {
        let offset = branch(at, to).at(span)?;
        if let Some(s) = self.program.statements.get_mut(at) {
            s.a = offset;
        }
        Ok(())
    }

    /// A jump taken when `condition` is false, its target patched later; returns its index.
    fn unless(&mut self, condition: &Value, span: Span) -> Lowered<usize> {
        let (op, tested) = self.test(condition, false, span)?;
        Ok(self.program.emit(op, operand(tested).at(span)?, 0))
    }

    /// The opcode and operand of a jump taken when `condition` is `when`: `IF` or `IFNOT` of
    /// its word (QC-14.4); under FTEQCC with `ifvector` on, a vector is tested by `NOT_V` first,
    /// so every component counts (`QCC_Generate_OP_IFNOT`, FTEF-23.10).
    fn test(&mut self, condition: &Value, when: bool, span: Span) -> Lowered<(Opcode, u32)> {
        let flags = qc_syntax::CompilerFlags::at(&self.flags, span.start);
        let vector = self.type_of(condition).etype() == Etype::Vector;
        if self.dialect.has_fteqcc_extensions()
            && vector
            && flags.has(qc_preproc::FteFlag::Ifvector)
        {
            let not = self
                .program
                .emit_result(Opcode::NotV, condition.ofs, 0, Etype::Float)
                .at(span)?;
            return Ok((if when { Opcode::IfNot } else { Opcode::If }, not));
        }
        Ok((if when { Opcode::If } else { Opcode::IfNot }, condition.ofs))
    }

    /// FTEQCC's `c ? a : b` and `c ?: b` (FTE-8.1, FTEF-30.3): the condition tested, the first
    /// arm (the condition itself for `?:`) stored to a temporary of its size, `GOTO` past the
    /// last arm, which is stored to the same temporary; the arms must have one basic type.
    fn ternary(&mut self, ternary: &ast::TernaryExpr, span: Span) -> Lowered<Value> {
        let condition = self.expr(&ternary.condition().at(span)?)?;
        let (middle, last) = ternary.arms();
        let test = self.unless(&condition, span)?;
        let first = match middle {
            Some(middle) => self.expr(&middle)?,
            None => condition,
        };
        let ty = self.type_of(&first);
        let (words, store) = if ty.etype() == Etype::Vector {
            (3, Opcode::StoreV)
        } else {
            (1, Opcode::StoreF)
        };
        let result = self.program.allocate(words).at(span)?;
        self.program.emit(
            store,
            operand(first.ofs).at(span)?,
            operand(result).at(span)?,
        );
        let skip = self.program.emit(Opcode::Goto, 0, 0);
        self.patch_b(test, span)?;
        let second = self.expr(&last.at(span)?)?;
        if self.type_of(&second).etype() != ty.etype() {
            return Self::error(Code::E0302, "type mismatch for ?".to_owned(), span);
        }
        self.program.emit(
            store,
            operand(second.ofs).at(span)?,
            operand(result).at(span)?,
        );
        let past = branch(skip, self.program.statements.len()).at(span)?;
        if let Some(s) = self.program.statements.get_mut(skip) {
            s.a = past;
        }
        Ok(Value {
            ofs: result,
            kind: ValueKind::Temp(ty),
        })
    }

    /// The float literal `value`'s global (QC-20).
    fn float_literal(&mut self, value: f32, span: Span) -> Lowered<Value> {
        let place = self.program.immediate(&Literal::Float(value)).at(span)?;
        self.def_value(place, span)
    }

    /// FTEQCC's `x++` and `x--` (FTE-5.1): the old value copied to a temporary, then `x` stored
    /// as the temporary plus or minus 1 (`ADD_F` with the constant `inc`, as `QCC_RefToDef`'s
    /// post-increment does); the value is the old one.
    fn postfix(&mut self, postfix: &ast::PostfixExpr, span: Span) -> Lowered<Value> {
        let target = postfix.operand().at(span)?;
        let inc = self.float_literal(postfix_step(postfix), span)?;
        let mut old = None;
        self.store(&target, span, Current::Read, |compiler, current| {
            let current = compiler.current(current, span)?;
            let ty = compiler.type_of(&current);
            if ty.etype() != Etype::Float {
                return Self::mismatch(Operator::Add, span);
            }
            let temp = compiler.program.allocate(1).at(span)?;
            compiler.program.emit(
                Opcode::StoreF,
                operand(current.ofs).at(span)?,
                operand(temp).at(span)?,
            );
            let copy = Value {
                ofs: temp,
                kind: ValueKind::Temp(ty),
            };
            old = Some(copy.clone());
            compiler.operate(Operator::Add, &copy, &inc, (span, span))
        })?;
        old.at(span)
    }

    /// A prefix operator (FTE-5.1): `-x` is `0 - x`, `SUB_F` or `SUB_V` from a zero literal as
    /// FTEQCC writes it; `+x` is `x`; `++x` and `--x` store `x` plus or minus 1 and are the new
    /// value.
    fn unary(&mut self, unary: &ast::UnaryExpr, span: Span) -> Lowered<Value> {
        let operand = unary.operand().at(span)?;
        match unary.op().at(span)? {
            ast::UnaryOp::Neg => {
                let value = self.expr(&operand)?;
                let zero = match self.type_of(&value).etype() {
                    Etype::Vector => {
                        let place = self
                            .program
                            .immediate(&Literal::Vector([0.0; 3]))
                            .at(span)?;
                        self.def_value(place, span)?
                    }
                    _ => self.float_literal(0.0, span)?,
                };
                self.operate(Operator::Sub, &zero, &value, (span, span))
            }
            ast::UnaryOp::Plus => self.expr(&operand),
            op @ (ast::UnaryOp::PreIncrement | ast::UnaryOp::PreDecrement) => {
                let step = if op == ast::UnaryOp::PreDecrement {
                    -1.0
                } else {
                    1.0
                };
                self.increment(&operand, step, span)
            }
            // FTEQCC's `~x` on the vanilla opcodes, `-1 - x` (`OP_BITNOT_F`'s emulation).
            ast::UnaryOp::BitNot => {
                let value = self.expr(&operand)?;
                let minus_one = match self.type_of(&value).etype() {
                    Etype::Vector => {
                        let place = self
                            .program
                            .immediate(&Literal::Vector([-1.0; 3]))
                            .at(span)?;
                        self.def_value(place, span)?
                    }
                    _ => self.float_literal(-1.0, span)?,
                };
                self.operate(Operator::Sub, &minus_one, &value, (span, span))
            }
            ast::UnaryOp::AddressOf => self.address_of(&operand, span),
            ast::UnaryOp::Deref | ast::UnaryOp::AsString => {
                Err(Abort::Internal(Internal::Unsupported, span))
            }
        }
    }

    /// FTEQCC's `[x, y, z]` (FTEF-30.7): each component read in turn, a missing one the float 0;
    /// three constants are the vector immediate, two constant zeros the unit vector times the
    /// third by `MUL_VF`, and otherwise each component stored to its word of a temporary by
    /// `STORE_F`, as `QCC_PR_GenerateVector` builds it. A call's value is held when a later
    /// component makes a call (FTEF-30.13). A component that is not a float is not lowered yet.
    fn vector(&mut self, vector: &ast::VectorExpr, span: Span) -> Lowered<Value> {
        let elements: Vec<ast::Expr> = vector.elements().collect();
        if elements.is_empty() || elements.len() > 3 {
            return Err(Abort::Internal(Internal::Unsupported, span));
        }
        let mut parts = Vec::with_capacity(3);
        for (i, element) in elements.iter().enumerate() {
            let value = self.expr(element)?;
            if self.type_of(&value).etype() != Etype::Float {
                return Err(Abort::Internal(Internal::Unsupported, span));
            }
            let calls_later = elements.iter().skip(i.saturating_add(1)).any(|later| {
                later
                    .syntax()
                    .descendants()
                    .any(|node| ast::CallExpr::can_cast(node.kind()))
            });
            parts.push(if calls_later {
                self.hold(value, span)?
            } else {
                value
            });
        }
        while parts.len() < 3 {
            parts.push(self.float_literal(0.0, span)?);
        }
        let words: Vec<Option<u32>> = parts.iter().map(|p| self.constant_word(p)).collect();
        if let [Some(x), Some(y), Some(z)] = words[..] {
            let literal = Literal::Vector([x, y, z].map(f32::from_bits));
            let place = self.program.immediate(&literal).at(span)?;
            return self.def_value(place, span);
        }
        let null: Vec<bool> = words.iter().map(|w| *w == Some(0)).collect();
        for (axis, part) in parts.iter().enumerate() {
            if null.iter().enumerate().all(|(i, &n)| n || i == axis) {
                let mut unit = [0.0; 3];
                *unit.get_mut(axis).at(span)? = 1.0;
                let place = self.program.immediate(&Literal::Vector(unit)).at(span)?;
                let unit = self.def_value(place, span)?;
                return self.operate(Operator::Mul, &unit, part, (span, span));
            }
        }
        let temp = self.program.allocate(3).at(span)?;
        for (word, part) in (0_u32..).zip(&parts) {
            let to = temp.checked_add(word).at(span)?;
            self.program.emit(
                Opcode::StoreF,
                operand(part.ofs).at(span)?,
                operand(to).at(span)?,
            );
        }
        Ok(Value {
            ofs: temp,
            kind: ValueKind::Temp(ValueType::simple(Etype::Vector)),
        })
    }

    /// The word of a one-word constant, `QCC_SRef_EvalConst` (FTEF-30.7).
    fn constant_word(&self, value: &Value) -> Option<u32> {
        let ValueKind::Def(place) = value.kind else {
            return None;
        };
        self.constant(place)
            .then(|| {
                self.program
                    .def(place)
                    .map(|def| self.program.word(def.ofs))
            })
            .flatten()
    }

    /// `(TYPE)e` (FTEF-30.6) between two types of one basic type: the value read as `TYPE`, the
    /// type the semantic model gave the cast, with no statement; a temporary stays one, which a
    /// store may fold. Other casts, and a call's value cast, are not lowered yet.
    fn cast(&mut self, cast: &ast::CastExpr, span: Span) -> Lowered<Value> {
        let value = self.expr(&cast.operand().at(span)?)?;
        let target = self
            .scope
            .and_then(|function| self.index.bodies.get(&function))
            .and_then(|body| body.type_at(span))
            .cloned()
            .map(ValueType::Of)
            .at(span)?;
        if target.etype() != self.type_of(&value).etype() {
            return Err(Abort::Internal(Internal::Unsupported, span));
        }
        let kind = match value.kind {
            ValueKind::Def(_) | ValueKind::Cast(_) => ValueKind::Cast(target),
            ValueKind::Temp(_) => ValueKind::Temp(target),
            ValueKind::Return => return Err(Abort::Internal(Internal::Unsupported, span)),
        };
        Ok(Value {
            ofs: value.ofs,
            kind,
        })
    }

    /// FTEF-17.17: `autocvar(name, default[, "description"])`, an intrinsic when no definition
    /// is named `autocvar`: the global `autocvar_name`, made now if it is new and given the
    /// default's words when it has no value yet; the call is that global, with no statement.
    fn autocvar(&mut self, args: &[ast::Expr], span: Span) -> Lowered<Value> {
        let Some(ast::Expr::Name(name)) = args.first() else {
            return Err(Abort::Internal(Internal::Unlowered, span));
        };
        let name = name.name().at(span)?.text_lossless();
        let id = self
            .names
            .global(&format!("autocvar_{name}"))
            .ok_or(Abort::Internal(Internal::Unresolved, span))?;
        let place = self.program.get_def(self.index, id).at(span)?;
        let def = self.program.def(place).at(span)?;
        if !def.initialized {
            let ty = def.ty.clone();
            let words = self.value_words(&ty, args.get(1).at(span)?)?;
            self.program.initialise_words(place, &words).at(span)?;
        }
        self.def_value(place, span)
    }

    /// `&e.f` (FTEF-31.4): `ADDRESS` of the entity and the field, a pointer to what the field
    /// holds, as `QCC_PR_GenerateAddressOf` makes for a field's reference. Other operands are not
    /// lowered yet.
    fn address_of(&mut self, operand: &ast::Expr, span: Span) -> Lowered<Value> {
        let Some(ast::Expr::Field(field)) = unparenthesized(operand) else {
            return Err(Abort::Internal(Internal::Unsupported, span));
        };
        let Member::Base(base) = self.member(&field, span)? else {
            return Err(Abort::Internal(Internal::Unsupported, span));
        };
        let slot = self.field_slot(&base, &field, span)?;
        let ofs = self
            .program
            .emit_result(Opcode::Address, slot.entity, slot.field, Etype::Pointer)
            .at(span)?;
        Ok(Value {
            ofs,
            kind: ValueKind::Temp(slot.ty.pointer_to()),
        })
    }

    /// A vector's component by a constant index, `v[0]` to `v[2]` (FTE-13.2): the component's
    /// word of the vector, a field's vector loaded first by `LOAD_V`.
    fn index(&mut self, index: &ast::IndexExpr, span: Span) -> Lowered<Value> {
        let base_expr = index.base().at(span)?;
        let at = index.index().at(span)?;
        if let Some(array) = self.array_named(&base_expr)? {
            return self.element(&array, &at, None);
        }
        let base = self.expr(&base_expr)?;
        if self.type_of(&base).etype() != Etype::Vector {
            return Err(Abort::Internal(Internal::Unsupported, span));
        }
        let Some(Literal::Float(i)) = literal_of(&at, self.dialect) else {
            return Err(Abort::Internal(Internal::Unsupported, span));
        };
        let offset = [0.0_f32, 1.0, 2.0]
            .iter()
            .position(|c| c.to_bits() == i.to_bits())
            .and_then(|o| u32::try_from(o).ok())
            .ok_or(Abort::Internal(Internal::Unsupported, span))?;
        Ok(Value {
            ofs: base.ofs.checked_add(offset).at(span)?,
            kind: ValueKind::Temp(ValueType::simple(Etype::Float)),
        })
    }

    fn def_value(&self, place: usize, span: Span) -> Lowered<Value> {
        let ofs = self.program.def(place).at(span)?.ofs;
        Ok(Value {
            ofs,
            kind: ValueKind::Def(place),
        })
    }

    /// A binary operator (QC-16.1): both operands, then the first row of the operator whose
    /// operand types are theirs; `=` is [`Compiler::assign`].
    fn binary(&mut self, binary: &ast::BinaryExpr, span: Span) -> Lowered<Value> {
        let op_span = binary.op_token().map_or(span, |t| t.span());
        let operator = match binary.op().at(span)? {
            ast::BinaryOp::Assign => {
                let lhs = binary.lhs().at(span)?;
                let rhs = binary.rhs().at(span)?;
                return self.assign(&lhs, &rhs, op_span);
            }
            ast::BinaryOp::Mul => Operator::Mul,
            ast::BinaryOp::Div => Operator::Div,
            ast::BinaryOp::BitAnd => Operator::BitAnd,
            ast::BinaryOp::BitOr => Operator::BitOr,
            ast::BinaryOp::Add => Operator::Add,
            ast::BinaryOp::Sub => Operator::Sub,
            ast::BinaryOp::Eq => Operator::Eq,
            ast::BinaryOp::Ne => Operator::Ne,
            ast::BinaryOp::Le => Operator::Le,
            ast::BinaryOp::Ge => Operator::Ge,
            ast::BinaryOp::Lt => Operator::Lt,
            ast::BinaryOp::Gt => Operator::Gt,
            ast::BinaryOp::And => Operator::And,
            ast::BinaryOp::Or => Operator::Or,
            ast::BinaryOp::AddAssign
            | ast::BinaryOp::SubAssign
            | ast::BinaryOp::MulAssign
            | ast::BinaryOp::DivAssign
            | ast::BinaryOp::BitAndAssign
            | ast::BinaryOp::BitOrAssign => {
                let operator = match binary.op().at(span)? {
                    ast::BinaryOp::AddAssign => Operator::Add,
                    ast::BinaryOp::SubAssign => Operator::Sub,
                    ast::BinaryOp::MulAssign => Operator::Mul,
                    ast::BinaryOp::DivAssign => Operator::Div,
                    ast::BinaryOp::BitAndAssign => Operator::BitAnd,
                    _ => Operator::BitOr,
                };
                let lhs = binary.lhs().at(span)?;
                let rhs = binary.rhs().at(span)?;
                return self.compound(operator, (&lhs, &rhs), (op_span, span));
            }
            ast::BinaryOp::BitClearAssign => {
                let lhs = binary.lhs().at(span)?;
                let rhs = binary.rhs().at(span)?;
                return self.clear_bits((&lhs, &rhs), (op_span, span));
            }
            ast::BinaryOp::BitXor => {
                let left = self.left_operand(&binary.lhs().at(span)?)?;
                let right = self.expr(&binary.rhs().at(span)?)?;
                return self.xor(&left, &right, (op_span, span));
            }
            ast::BinaryOp::BitXorAssign => {
                let lhs = binary.lhs().at(span)?;
                let rhs = binary.rhs().at(span)?;
                return self.store(&lhs, op_span, Current::Read, |compiler, left| {
                    let right = compiler.expr(&rhs)?;
                    let left = compiler.current(left, span)?;
                    compiler.xor(&left, &right, (op_span, span))
                });
            }
            ast::BinaryOp::Rem => {
                let left = self.left_operand(&binary.lhs().at(span)?)?;
                let right = self.expr(&binary.rhs().at(span)?)?;
                return self.modulo(&left, &right, (op_span, span));
            }
            op @ (ast::BinaryOp::Shl | ast::BinaryOp::Shr) => {
                let left = self.left_operand(&binary.lhs().at(span)?)?;
                let right = self.expr(&binary.rhs().at(span)?)?;
                let shift = if op == ast::BinaryOp::Shl {
                    arrays::Shift::Left
                } else {
                    arrays::Shift::Right
                };
                return self.shift(shift, &left, &right, (op_span, span));
            }
            // FTEF-18.5: `a, b` reads `a`, discards it, and is `b`.
            ast::BinaryOp::Comma => {
                self.discard(&binary.lhs().at(span)?)?;
                return self.expr(&binary.rhs().at(span)?);
            }
            ast::BinaryOp::ShlAssign
            | ast::BinaryOp::ShrAssign
            | ast::BinaryOp::Pow
            | ast::BinaryOp::Cross
            | ast::BinaryOp::Spaceship => {
                return Err(Abort::Internal(Internal::Unsupported, op_span));
            }
        };
        let left = self.left_operand(&binary.lhs().at(span)?)?;
        let right = self.expr(&binary.rhs().at(span)?)?;
        self.operate(operator, &left, &right, (op_span, span))
    }

    /// FTEQCC's `a ^ b` on the vanilla opcodes (FTE-19.7, FTE-19.12), `OP_BITXOR_F`'s and
    /// `OP_BITXOR_V`'s emulation: `(a & ~b) | (b & ~a)`, each `~` as `-1 - x`, a vector's
    /// `'-1 -1 -1' - x`.
    fn xor(&mut self, left: &Value, right: &Value, spans: (Span, Span)) -> Lowered<Value> {
        let span = spans.1;
        let etype = self.type_of(left).etype();
        if !matches!(etype, Etype::Float | Etype::Vector) || self.type_of(right).etype() != etype {
            return Err(Abort::Internal(Internal::Unsupported, spans.0));
        }
        let minus_one = if etype == Etype::Vector {
            let place = self
                .program
                .immediate(&Literal::Vector([-1.0; 3]))
                .at(span)?;
            self.def_value(place, span)?
        } else {
            self.float_literal(-1.0, span)?
        };
        let not_b = self.operate(Operator::Sub, &minus_one, right, spans)?;
        let a_only = self.operate(Operator::BitAnd, left, &not_b, spans)?;
        let not_a = self.operate(Operator::Sub, &minus_one, left, spans)?;
        let b_only = self.operate(Operator::BitAnd, right, &not_a, spans)?;
        self.operate(Operator::BitOr, &a_only, &b_only, spans)
    }

    /// The row of `operator` for the two operands' types, folded under `--opt fold`, else
    /// emitted into a temporary (QC-16.1).
    fn operate(
        &mut self,
        operator: Operator,
        left: &Value,
        right: &Value,
        (op_span, span): (Span, Span),
    ) -> Lowered<Value> {
        // The types are read after both operands, as `PR_Expression` reads them: a call in the
        // right operand retypes the return value the left one may be (QC-18.6).
        let a = self.type_of(left).etype();
        let b = self.type_of(right).etype();
        let Some(row) =
            ops::select(operator, a, b, None).or_else(|| self.null_row(operator, left, right))
        else {
            if self.dialect.has_fteqcc_extensions()
                && matches!(operator, Operator::And | Operator::Or)
            {
                return self.any_logic(operator, left, right, op_span);
            }
            // FTE-19.12: `v & w` and `v | w` of vectors are `BITAND_F` or `BITOR_F` of each
            // component into a vector temporary.
            if self.dialect.has_fteqcc_extensions()
                && matches!(operator, Operator::BitAnd | Operator::BitOr)
                && (a, b) == (Etype::Vector, Etype::Vector)
            {
                return self.vector_bits(operator, left, right, span);
            }
            // FTE-19.9: `v / f` is `OP_DIV_VF`'s emulation, `v * (1 / f)`.
            if self.dialect.has_fteqcc_extensions()
                && operator == Operator::Div
                && (a, b) == (Etype::Vector, Etype::Float)
            {
                let one = self.float_literal(1.0, span)?;
                let reciprocal = self.operate(Operator::Div, &one, right, (op_span, span))?;
                return self.operate(Operator::Mul, left, &reciprocal, (op_span, span));
            }
            return Self::mismatch(operator, op_span);
        };
        if self.fold
            && let Some(value) = self.folded(row.opcode, left, right).at(span)?
        {
            return Ok(value);
        }
        let ofs = self
            .program
            .emit_result(row.opcode, left.ofs, right.ofs, row.c)
            .at(span)?;
        Ok(Value {
            ofs,
            kind: ValueKind::Temp(ValueType::simple(row.c)),
        })
    }

    /// FTEQCC's store of a null (FTEF-29.2): a float constant whose word is 0, such as
    /// `__NULL__`, stored to a target of another type is the target's zero, `rhsd =
    /// QCC_MakeIntConst(0);`, its word read as `ty`.
    fn null_as(&self, value: Value, ty: &ValueType) -> Value {
        let null = self.dialect.has_fteqcc_extensions()
            && self.type_of(&value).etype() == Etype::Float
            && ty.etype() != Etype::Float
            && self.constant_word(&value) == Some(0);
        if null {
            Value {
                ofs: value.ofs,
                kind: ValueKind::Cast(ty.clone()),
            }
        } else {
            value
        }
    }

    /// FTEF-23.3, FTEF-23.4: under FTEQCC, with no row for the two types, the row whose operand
    /// types are the other operand's twice when one operand is a null, a float constant whose
    /// word is 0 such as `__NULL__`, which converts to any type at cost 2: `f != __NULL__` of a
    /// function is `NE_FNC`.
    fn null_row(&self, operator: Operator, left: &Value, right: &Value) -> Option<ops::Row> {
        if !self.dialect.has_fteqcc_extensions() {
            return None;
        }
        let null = |value: &Value| {
            self.type_of(value).etype() == Etype::Float && self.constant_word(value) == Some(0)
        };
        let (a, b) = (self.type_of(left).etype(), self.type_of(right).etype());
        if null(right) {
            ops::select(operator, a, a, None)
        } else if null(left) {
            ops::select(operator, b, b, None)
        } else {
            None
        }
    }

    /// FTE-19.12: `OP_BITAND_V` or `OP_BITOR_V` on the vanilla opcodes, the float opcode of
    /// each component pair into the matching word of a vector temporary.
    fn vector_bits(
        &mut self,
        operator: Operator,
        left: &Value,
        right: &Value,
        span: Span,
    ) -> Lowered<Value> {
        let op = if operator == Operator::BitAnd {
            Opcode::BitAnd
        } else {
            Opcode::BitOr
        };
        let temp = self.program.allocate(3).at(span)?;
        for word in 0..3_u32 {
            let a = left.ofs.checked_add(word).at(span)?;
            let b = right.ofs.checked_add(word).at(span)?;
            let c = temp.checked_add(word).at(span)?;
            self.program.emit_into(op, a, b, c).at(span)?;
        }
        Ok(Value {
            ofs: temp,
            kind: ValueKind::Temp(ValueType::simple(Etype::Vector)),
        })
    }

    /// FTEQCC's `&&` and `||` of operands that are not both floats, `OP_AND_ANY` and
    /// `OP_OR_ANY` on the vanilla opcodes (FTE-19.10): a vector is first its dot product with itself under
    /// `vectorlogic`, then `AND_F` or `OR_F` of the two words.
    fn any_logic(
        &mut self,
        operator: Operator,
        left: &Value,
        right: &Value,
        span: Span,
    ) -> Lowered<Value> {
        let flags = qc_syntax::CompilerFlags::at(&self.flags, span.start);
        let truth = |compiler: &mut Self, value: &Value| -> Lowered<u32> {
            if compiler.type_of(value).etype() == Etype::Vector
                && flags.has(qc_preproc::FteFlag::Vectorlogic)
            {
                return compiler
                    .program
                    .emit_result(Opcode::MulV, value.ofs, value.ofs, Etype::Float)
                    .at(span);
            }
            Ok(value.ofs)
        };
        let a = truth(self, left)?;
        let b = truth(self, right)?;
        let op = if operator == Operator::And {
            Opcode::And
        } else {
            Opcode::Or
        };
        let ofs = self.program.emit_result(op, a, b, Etype::Float).at(span)?;
        Ok(Value {
            ofs,
            kind: ValueKind::Temp(ValueType::simple(Etype::Float)),
        })
    }

    /// `--opt fold` (COMP-5): the operator `opcode` of two literals as the literal of its value,
    /// computed as the VM computes it (VM-5): each product and sum an `f32` rounded in turn, the
    /// dot product `(a0*b0 + a1*b1) + a2*b2`. `None` for any other operator or operand, which is
    /// emitted as it is.
    fn folded(
        &mut self,
        opcode: Opcode,
        left: &Value,
        right: &Value,
    ) -> Result<Option<Value>, Internal> {
        let (Some(a), Some(b)) = (self.literal_of(left), self.literal_of(right)) else {
            return Ok(None);
        };
        let each =
            |x: [f32; 3], f: &dyn Fn(usize, f32) -> f32| [f(0, x[0]), f(1, x[1]), f(2, x[2])];
        let literal = match (opcode, a, b) {
            (Opcode::AddF, Literal::Float(x), Literal::Float(y)) => Literal::Float(x + y),
            (Opcode::SubF, Literal::Float(x), Literal::Float(y)) => Literal::Float(x - y),
            (Opcode::MulF, Literal::Float(x), Literal::Float(y)) => Literal::Float(x * y),
            (Opcode::DivF, Literal::Float(x), Literal::Float(y)) => Literal::Float(x / y),
            (Opcode::AddV, Literal::Vector(x), Literal::Vector(y)) => {
                Literal::Vector(each(x, &|i, v| v + component(y, i)))
            }
            (Opcode::SubV, Literal::Vector(x), Literal::Vector(y)) => {
                Literal::Vector(each(x, &|i, v| v - component(y, i)))
            }
            (Opcode::MulV, Literal::Vector(x), Literal::Vector(y)) => {
                Literal::Float((x[0] * y[0] + x[1] * y[1]) + x[2] * y[2])
            }
            (Opcode::MulFv, Literal::Float(s), Literal::Vector(v)) => {
                Literal::Vector(each(v, &|_, c| s * c))
            }
            (Opcode::MulVf, Literal::Vector(v), Literal::Float(s)) => {
                Literal::Vector(each(v, &|_, c| s * c))
            }
            _ => return Ok(None),
        };
        let place = self.program.immediate(&literal)?;
        let ofs = self.program.def(place).ok_or(Internal::NoDefinition)?.ofs;
        Ok(Some(Value {
            ofs,
            kind: ValueKind::Def(place),
        }))
    }

    /// The literal a value is, when it is one of the float or vector immediates
    /// `PR_ParseImmediate` makes (QC-20).
    fn literal_of(&self, value: &Value) -> Option<Literal> {
        let ValueKind::Def(place) = value.kind else {
            return None;
        };
        let def = self.program.def(place)?;
        if def.name != "IMMEDIATE" {
            return None;
        }
        let float = |i: u32| f32::from_bits(self.program.word(def.ofs.saturating_add(i)));
        match def.ty.etype() {
            Etype::Float => Some(Literal::Float(float(0))),
            Etype::Vector => Some(Literal::Vector([float(0), float(1), float(2)])),
            _ => None,
        }
    }

    fn mismatch<T>(operator: Operator, span: Span) -> Lowered<T> {
        Self::error(
            Code::E0302,
            format!("type mismatch for {}", operator.spelling()),
            span,
        )
    }

    /// A field access `e.f` (QC-16.1, QC-16.6): the entity, the field, then `LOAD_*` by the
    /// field's type into a temporary of that type.
    fn field(&mut self, field: &ast::FieldExpr, span: Span) -> Lowered<Value> {
        let base = match self.member(field, span)? {
            Member::Done(value) => return Ok(value),
            Member::Base(base) => base,
        };
        match self.component(&base, field, span)? {
            Some(component) => Ok(component),
            None => self.load_field(&base, field, span),
        }
    }

    /// The member forms of `base.name` FTEQCC adds (FTE-13.9, FTE-14.5): an array's `length`,
    /// a member of an array's element, `a[i].m`, or a member of a struct value; else the base
    /// read for an entity's field or a vector's component.
    fn member(&mut self, field: &ast::FieldExpr, span: Span) -> Lowered<Member> {
        let base_expr = field.base().at(span)?;
        let name = self.member_name(field);
        if let Some(name) = &name {
            if let Some(found) = self.array_member(&base_expr, name, span)? {
                return self
                    .element(&found.array, &found.index, Some(found.member))
                    .map(Member::Done);
            }
            if name == "length"
                && let Some(array) = self.array_named(&base_expr)?
            {
                return self.length(&array, span).map(Member::Done);
            }
        }
        let base = self.expr(&base_expr)?;
        if let Some(name) = &name
            && let Some((word, ty)) = self.member_of(&self.type_of(&base), name)
        {
            return Ok(Member::Done(Value {
                ofs: base.ofs.checked_add(word).at(span)?,
                kind: ValueKind::Temp(ty),
            }));
        }
        Ok(Member::Base(base))
    }

    /// The name after `.` under FTEQCC, which may name a member (FTE-14.5).
    fn member_name(&self, field: &ast::FieldExpr) -> Option<String> {
        match field.field() {
            Some(ast::Expr::Name(name)) if self.dialect.has_fteqcc_extensions() => {
                name.name().map(|t| t.text_lossless())
            }
            _ => None,
        }
    }

    /// `a[i].m` of an array of structs: the array, the index and the member (FTE-14.5).
    fn array_member(
        &mut self,
        base: &ast::Expr,
        name: &str,
        span: Span,
    ) -> Lowered<Option<ArrayMember>> {
        let ast::Expr::Index(index) = base else {
            return Ok(None);
        };
        let Some(array) = self.array_named(&index.base().at(span)?)? else {
            return Ok(None);
        };
        let Some(member) = self.member_of(&array.elem, name) else {
            return Ok(None);
        };
        Ok(Some(ArrayMember {
            array,
            index: index.index().at(span)?,
            member,
        }))
    }

    /// FTEQCC's `v.x`, `v.y` and `v.z` of a vector `v` (FTE-9.1): the component's word.
    /// `None` when the base is no vector or the selector no component's name.
    fn component(
        &self,
        base: &Value,
        field: &ast::FieldExpr,
        span: Span,
    ) -> Lowered<Option<Value>> {
        if !self.dialect.has_fteqcc_extensions() || self.type_of(base).etype() != Etype::Vector {
            return Ok(None);
        }
        let Some(ast::Expr::Name(name)) = field.field() else {
            return Ok(None);
        };
        let offset = match name.name().map(|t| t.text_lossless()).as_deref() {
            Some("x") => 0,
            Some("y") => 1,
            Some("z") => 2,
            _ => return Ok(None),
        };
        Ok(Some(Value {
            ofs: base.ofs.checked_add(offset).at(span)?,
            kind: ValueKind::Temp(ValueType::simple(Etype::Float)),
        }))
    }

    /// `e.f` of the entity `base` already read: `LOAD_*` by the field's type.
    fn load_field(&mut self, base: &Value, field: &ast::FieldExpr, span: Span) -> Lowered<Value> {
        let slot = self.field_slot(base, field, span)?;
        self.read_field(&slot, span)
    }

    /// `LOAD_*` of the field `slot` into a temporary of its type (QC-16.1).
    fn read_field(&mut self, slot: &FieldSlot, span: Span) -> Lowered<Value> {
        let ofs = self
            .program
            .emit_result(slot.load, slot.entity, slot.field, slot.loaded)
            .at(span)?;
        Ok(Value {
            ofs,
            kind: ValueKind::Temp(slot.ty.clone()),
        })
    }

    /// The value a store's `pending` target holds, read now (FTE-6.4).
    fn current(&mut self, pending: Option<Pending>, span: Span) -> Lowered<Value> {
        match pending.at(span)? {
            Pending::Value(value) => Ok(value),
            Pending::Field(slot) => self.read_field(&slot, span),
        }
    }

    /// The field `base.field` names: its selector evaluated and its `LOAD_*` row chosen
    /// (QC-16.1, QC-16.6), with no statement for the load itself.
    fn field_slot(
        &mut self,
        base: &Value,
        field: &ast::FieldExpr,
        span: Span,
    ) -> Lowered<FieldSlot> {
        let op_span = field
            .syntax()
            .child_tokens()
            .find(|t| t.kind() == qc_syntax::SyntaxKind::Dot)
            .map_or(span, |t| t.span());
        let selector = self.expr(&field.field().at(span)?)?;
        let a = self.type_of(base).etype();
        let field_type = self.type_of(&selector);
        let b = field_type.etype();
        let Some(inner) = field_type.field_inner() else {
            return Self::mismatch(Operator::Dot, op_span);
        };
        // QC-16.8: `type_c == ev_void` turns the result's check off, so a field of `void`
        // takes the first row, `LOAD_F`.
        let c = Some(inner.etype()).filter(|&c| c != Etype::Void);
        let Some(row) = ops::select(Operator::Dot, a, b, c) else {
            return Self::mismatch(Operator::Dot, op_span);
        };
        // `if (type_c != ev_void) e->type = e2->type->aux_type;`: a `void` field's value keeps
        // the row's type.
        let ty = if c.is_some() {
            inner
        } else {
            ValueType::simple(row.c)
        };
        Ok(FieldSlot {
            entity: base.ofs,
            field: selector.ofs,
            load: row.opcode,
            loaded: row.c,
            ty,
        })
    }

    /// `x = e` (QC-17): the left side first, a field access made an address (QC-17.3,
    /// QC-27.11), then the value, then `STORE_*` or `STOREP_*` by the two basic types. The
    /// value of the assignment is the value stored (QC-17.6, QC-17.7).
    fn assign(&mut self, lhs: &ast::Expr, rhs: &ast::Expr, op_span: Span) -> Lowered<Value> {
        self.store(lhs, op_span, Current::Unread, |compiler, _| {
            compiler.expr(rhs)
        })
    }

    /// FTEQCC's `x OP= e` (FTE-8.1): `x = x OP e`, the operator's row chosen as for `x OP e`;
    /// `&~=` clears bits, `x - (x & e)` on the vanilla opcodes.
    fn compound(
        &mut self,
        operator: Operator,
        (lhs, rhs): (&ast::Expr, &ast::Expr),
        (op_span, span): (Span, Span),
    ) -> Lowered<Value> {
        self.store(lhs, op_span, Current::Read, |compiler, left| {
            let right = compiler.expr(rhs)?;
            let left = compiler.current(left, op_span)?;
            compiler.operate(operator, &left, &right, (op_span, span))
        })
    }

    /// `x &~= e`: `x - (x & e)`.
    fn clear_bits(
        &mut self,
        (lhs, rhs): (&ast::Expr, &ast::Expr),
        (op_span, span): (Span, Span),
    ) -> Lowered<Value> {
        self.store(lhs, op_span, Current::Read, |compiler, left| {
            let right = compiler.expr(rhs)?;
            let left = compiler.current(left, op_span)?;
            let masked = compiler.operate(Operator::BitAnd, &left, &right, (op_span, span))?;
            compiler.operate(Operator::Sub, &left, &masked, (op_span, span))
        })
    }

    /// Stores the value `value` computes in `lhs` (QC-17): the target first, then the value,
    /// then the store of the row the two types select. With [`Current::Read`] the target is
    /// given to `value` to read once ([`Compiler::current`]), as FTEQCC reads a reference once
    /// for `x op= e`, `++x` and `x++` (`QCC_RefToDef`), after the right side (FTE-6.4).
    fn store(
        &mut self,
        lhs: &ast::Expr,
        op_span: Span,
        current: Current,
        value: impl FnOnce(&mut Self, Option<Pending>) -> Lowered<Value>,
    ) -> Lowered<Value> {
        let span = lhs.syntax().span();
        let reads = current == Current::Read;
        let target = match unparenthesized(lhs) {
            Some(ast::Expr::Field(field)) => {
                if let Some(which) = self.component_selector(&field)
                    && let Some(target) =
                        self.entity_component(&field.base().at(span)?, which, span)?
                {
                    target
                } else if let Some(name) = self.member_name(&field)
                    && let Some(found) = self.array_member(&field.base().at(span)?, &name, span)?
                {
                    match self.element_target(&found.array, &found.index, Some(found.member))? {
                        Ok(element) => Target::Value(element),
                        Err(store) => Target::Element(store),
                    }
                } else {
                    match self.member(&field, span)? {
                        Member::Done(member) => Target::Value(member),
                        Member::Base(base) => match self.component(&base, &field, span)? {
                            Some(component) => Target::Value(component),
                            None => self.field_target(&base, &field, span)?,
                        },
                    }
                }
            }
            Some(ast::Expr::Unary(unary)) if unary.op() == Some(ast::UnaryOp::Deref) => {
                let pointer = self.expr(&unary.operand().at(span)?)?;
                let ty = self.type_of(&pointer).pointee().at(span)?;
                Target::Pointer {
                    address: pointer.ofs,
                    ty,
                }
            }
            Some(ast::Expr::Index(index)) => {
                let base = index.base().at(span)?;
                let at = index.index().at(span)?;
                if let Some(array) = self.array_named(&base)? {
                    match self.element_target(&array, &at, None)? {
                        Ok(element) => Target::Value(element),
                        Err(store) => Target::Element(store),
                    }
                } else if let Some(which) = self.vector_index(&at)
                    && let Some(target) = self.entity_component(&base, which, span)?
                {
                    target
                } else {
                    Target::Value(self.expr(lhs)?)
                }
            }
            _ => Target::Value(self.expr(lhs)?),
        };
        let pending = match (&target, reads) {
            (_, false) => None,
            (Target::Value(to), true) => Some(Pending::Value(to.clone())),
            (Target::Field { slot, .. }, true) => Some(Pending::Field(slot.clone())),
            // A read through a pointer, `LOADP_*`, and a read of an element a setter stores
            // are not lowered yet.
            (Target::Pointer { .. } | Target::Element(_), true) => {
                return Err(Abort::Internal(Internal::Unsupported, span));
            }
        };
        let value = value(self, pending)?;
        let (to, inner) = match target {
            Target::Value(to) => {
                let value = self.null_as(value, &self.type_of(&to));
                if self.fold_store(&to, &value) {
                    return Ok(to);
                }
                self.store_to(&to, &value, op_span)?;
                return Ok(value);
            }
            Target::Field {
                slot,
                address: Some(to),
            } => (to, slot.ty),
            Target::Field {
                slot,
                address: None,
            } => {
                let to = self
                    .program
                    .emit_result(Opcode::Address, slot.entity, slot.field, Etype::Pointer)
                    .at(op_span)?;
                (to, slot.ty)
            }
            Target::Pointer { address, ty } => (address, ty),
            Target::Element(store) => {
                let value = self.null_as(value, &store.ty);
                self.store_element(&store, &value, op_span)?;
                return Ok(value);
            }
        };
        let value = self.null_as(value, &inner);
        let inner = inner.etype();
        let b = self.type_of(&value).etype();
        let Some(row) = ops::select(Operator::Assign, Etype::Pointer, b, None) else {
            return Self::mismatch(Operator::Assign, op_span);
        };
        if inner != b {
            return Self::mismatch(Operator::Assign, op_span);
        }
        self.program.emit(
            row.opcode,
            operand(value.ofs).at(op_span)?,
            operand(to).at(op_span)?,
        );
        Ok(value)
    }

    /// FTEF-29.13: under FTEQCC a store of the temporary the statement before made to a
    /// definition is folded into that statement, which then writes the definition, as
    /// `opt_assignments` folds it; true when it was.
    fn fold_store(&mut self, to: &Value, value: &Value) -> bool {
        if self.dialect != Dialect::Fteqcc
            || !matches!(to.kind, ValueKind::Def(_))
            || !matches!(value.kind, ValueKind::Temp(_))
            || self.type_of(to) != self.type_of(value)
        {
            return false;
        }
        let Some(last) = self.program.statements.last_mut() else {
            return false;
        };
        let (Ok(made), Ok(def)) = (operand(value.ofs), operand(to.ofs)) else {
            return false;
        };
        // Only a statement that writes its result to `c` names a temporary there: a store
        // writes to `b`, so a `?:`'s stores to its temporary in each arm (FTE-8.3) never match.
        if last.c != made {
            return false;
        }
        last.c = def;
        true
    }

    /// `e.f` as a store's target (QC-17.3, QC-27.11): the field's `ADDRESS` made now, as id's
    /// `qcc` makes it in the field's load's place, or under FTEQCC after the value (FTE-6.4).
    fn field_target(
        &mut self,
        base: &Value,
        field: &ast::FieldExpr,
        span: Span,
    ) -> Lowered<Target> {
        let slot = self.field_slot(base, field, span)?;
        self.target_of(slot, span)
    }

    /// The target that stores to `slot`, its `ADDRESS` made now unless the dialect makes it
    /// after the value (QC-17.3, FTE-6.4).
    fn target_of(&mut self, slot: FieldSlot, span: Span) -> Lowered<Target> {
        if FieldSlot::address_after_value(self.dialect) {
            return Ok(Target::Field {
                slot,
                address: None,
            });
        }
        // QC-17.3: the field's load becomes `ADDRESS`, a pointer to the field, in the
        // temporary the load took.
        let loaded = self.read_field(&slot, span)?;
        if let Some(last) = self.program.statements.last_mut() {
            last.op = u32::from(Opcode::Address.number());
        }
        Ok(Target::Field {
            slot,
            address: Some(loaded.ofs),
        })
    }

    /// The component `v.x`, `v.y` or `v.z` selects under FTEQCC (FTE-9.1), 0 to 2.
    fn component_selector(&self, field: &ast::FieldExpr) -> Option<u32> {
        if !self.dialect.has_fteqcc_extensions() {
            return None;
        }
        let Some(ast::Expr::Name(name)) = field.field() else {
            return None;
        };
        match name.name()?.text_lossless().as_str() {
            "x" => Some(0),
            "y" => Some(1),
            "z" => Some(2),
            _ => None,
        }
    }

    /// The component a constant vector index `[0]` to `[2]` selects under FTEQCC (FTE-13.2).
    fn vector_index(&self, at: &ast::Expr) -> Option<u32> {
        if !self.dialect.has_fteqcc_extensions() {
            return None;
        }
        let Some(Literal::Float(i)) = literal_of(at, self.dialect) else {
            return None;
        };
        (0_u32..3).find(|&c| {
            u16::try_from(c).is_ok_and(|small| f32::from(small).to_bits() == i.to_bits())
        })
    }

    /// FTE-9.4: a store to component `which` of an entity's vector field, `e.v.x` or `e.v[0]`,
    /// stores through `ADDRESS` of the component's own field, `v_x` (QC-11.12), and its value
    /// is read by `LOAD_F`; `None` when `base` is no entity's vector field.
    fn entity_component(
        &mut self,
        base: &ast::Expr,
        which: u32,
        span: Span,
    ) -> Lowered<Option<Target>> {
        let base = match base {
            ast::Expr::Paren(paren) => paren.expr().at(span)?,
            other => other.clone(),
        };
        let ast::Expr::Field(field) = &base else {
            return Ok(None);
        };
        let Some(ast::Expr::Name(name)) = field.field() else {
            return Ok(None);
        };
        let id = self.def_of(&name.name().at(span)?)?;
        let place = self.program.get_def(self.index, id).at(span)?;
        let is_vector_field = self
            .program
            .def(place)
            .and_then(|d| d.ty.field_inner())
            .is_some_and(|inner| inner.etype() == Etype::Vector);
        if !is_vector_field {
            return Ok(None);
        }
        let suffix = ["_x", "_y", "_z"]
            .get(usize::try_from(which).map_err(|_| Abort::Internal(Internal::TooLarge, span))?)
            .at(span)?;
        let component = program::component(self.index, id, suffix).at(span)?;
        let component = self.program.get_def(self.index, component).at(span)?;
        let component = self.program.def(component).at(span)?.ofs;
        let entity = self.expr(&field.base().at(span)?)?;
        let slot = FieldSlot {
            entity: entity.ofs,
            field: component,
            load: Opcode::LoadF,
            loaded: Etype::Float,
            ty: ValueType::simple(Etype::Float),
        };
        self.target_of(slot, span).map(Some)
    }

    /// Stores `value` in the definition or temporary `to` by the `STORE_*` row of their types.
    fn store_to(&mut self, to: &Value, value: &Value, span: Span) -> Lowered<()> {
        let ty = self.type_of(to);
        if ty == self.type_of(value) && self.copy_struct(&ty, value.ofs, to.ofs, span)? {
            return Ok(());
        }
        let a = self.type_of(to).etype();
        let b = self.type_of(value).etype();
        let Some(row) = ops::select(Operator::Assign, a, b, None) else {
            return Self::mismatch(Operator::Assign, span);
        };
        self.program.emit(
            row.opcode,
            operand(value.ofs).at(span)?,
            operand(to.ofs).at(span)?,
        );
        Ok(())
    }

    /// `PR_ParseFunctionCall` (QC-18): each argument evaluated and copied with `STORE_V` to
    /// its parameter slot, then `CALLn`; the value is the return global, typed as the
    /// function's return type.
    fn call(&mut self, call: &ast::CallExpr, span: Span) -> Lowered<Value> {
        let args: Vec<ast::Expr> = call.args().map(|a| a.args().collect()).unwrap_or_default();
        if self.dialect.has_fteqcc_extensions()
            && let Some(ast::Expr::Name(name)) = call.callee()
            && let Some(token) = name.name()
            && token.text_lossless() == "autocvar"
            && self
                .names
                .def(self.file, token.span(), self.scope)
                .is_none()
        {
            return self.autocvar(&args, span);
        }
        let callee = self.expr(&call.callee().at(span)?)?;
        let callee_type = self.type_of(&callee);
        let Some(function) = callee_type.function().cloned() else {
            return Self::error(Code::E0303, "not a function".to_owned(), span);
        };
        let name = match &callee.kind {
            ValueKind::Def(place) => self.program.def(*place).map(|d| d.name.clone()),
            ValueKind::Temp(_) | ValueKind::Return | ValueKind::Cast(_) => None,
        };
        let fixed = (!function.variadic).then_some(function.params.len());
        let mut count = 0_usize;
        // Under FTEQCC the semantic model has judged the call (FTEF-32.3): fewer arguments
        // than parameters leave the rest as the parameter slots hold them, and an argument past
        // the last parameter is read and ignored ("too many parameters ... will be ignored").
        let fteqcc = self.dialect.has_fteqcc_extensions();
        if fteqcc {
            let slots = call.args().map(|a| a.slots()).unwrap_or_default();
            return self.fteqcc_call(&callee, &function, name.as_deref(), &slots, span);
        }
        for arg in &args {
            let arg_span = arg.syntax().span();
            if fixed.is_some_and(|n| count >= n) {
                return Self::error(Code::E0304, "too many parameters".to_owned(), arg_span);
            }
            let value = self.expr(arg)?;
            if count == 0
                && let Some(name) = &name
            {
                self.precaches.record(name, &self.program, value.ofs);
            }
            if let Some(param) = fixed.and_then(|_| function.params.get(count))
                && self.type_of(&value) != ValueType::Of(param.clone())
            {
                return Self::error(
                    Code::E0306,
                    format!("type mismatch on parm {count}"),
                    arg_span,
                );
            }
            if let Some(slot) = parm_slot(count) {
                self.program.emit(
                    Opcode::StoreV,
                    operand(value.ofs).at(arg_span)?,
                    operand(slot).at(arg_span)?,
                );
            }
            count = count.saturating_add(1);
        }
        // QC-18.8: `if (!PR_Check(")"))` skips the count check of a call with no arguments.
        if !args.is_empty() && fixed.is_some_and(|n| count != n) {
            return Self::error(Code::E0305, "too few parameters".to_owned(), span);
        }
        let Some(op) = ops::call_opcode(count) else {
            return Self::error(Code::E0307, "More than eight parameters".to_owned(), span);
        };
        self.program.emit(op, operand(callee.ofs).at(span)?, 0);
        self.return_type = ValueType::Of(function.ret.clone());
        Ok(Value {
            ofs: OFS_RETURN,
            kind: ValueKind::Return,
        })
    }
}

/// What [`Compiler::member`] made of a member access.
enum Member {
    /// The member's value.
    Done(Value),
    /// No member form: the base, read.
    Base(Value),
}

impl Compiler<'_> {
    /// `value`, copied to a temporary when it is a call's value in the return global, so a
    /// call made before it is read cannot overwrite it, as FTEQCC's `QCC_ClobberDef` copies
    /// the return global's aliases before each call (FTE-18.4, FTEF-30.13).
    fn hold(&mut self, value: Value, span: Span) -> Lowered<Value> {
        if !matches!(value.kind, ValueKind::Return) {
            return Ok(value);
        }
        let ty = self.type_of(&value);
        let temp = self.program.allocate(ty.words()).at(span)?;
        let copy = Value {
            ofs: temp,
            kind: ValueKind::Temp(ty),
        };
        self.store_to(&copy, &value, span)?;
        Ok(copy)
    }

    /// The left operand `lhs` of a binary operator: under FTEQCC a call's value is held before
    /// the right operand runs (FTE-18.5); under id's `qcc` it stays in the return global, which
    /// a call in the right operand overwrites (QC-18.6).
    fn left_operand(&mut self, lhs: &ast::Expr) -> Lowered<Value> {
        let value = self.expr(lhs)?;
        if self.dialect == Dialect::Fteqcc {
            self.hold(value, lhs.syntax().span())
        } else {
            Ok(value)
        }
    }

    /// A call under FTEQCC (FTE-18.4): every argument is read first, a call's value copied out
    /// of the return global before the next argument runs, then each copied to its parameter
    /// slot and `CALLn` made, so a call in an argument overwrites no slot. The semantic model
    /// has judged the call (FTEF-32.3): fewer arguments than parameters leave the rest as the
    /// slots hold them, and an argument past the last parameter is read and ignored.
    fn fteqcc_call(
        &mut self,
        callee: &Value,
        function: &FunctionType,
        name: Option<&str>,
        slots: &[Option<ast::Expr>],
        span: Span,
    ) -> Lowered<Value> {
        let fixed = (!function.variadic).then_some(function.params.len());
        let defaults = match callee.kind {
            ValueKind::Def(place) => self
                .program
                .def(place)
                .and_then(|def| def.id)
                .and_then(|id| self.defaults.get(&id))
                .cloned()
                .unwrap_or_default(),
            ValueKind::Temp(_) | ValueKind::Return | ValueKind::Cast(_) => Vec::new(),
        };
        // FTE-18.7: an empty slot takes its parameter's default, and so does each parameter
        // after the last argument that has one and is not `optional`.
        let mut count = slots.len();
        while count < function.params.len()
            && defaults.get(count).is_some_and(Option::is_some)
            && !function
                .labels
                .0
                .get(count)
                .is_some_and(|label| label.optional)
        {
            count = count.saturating_add(1);
        }
        let mut values = Vec::with_capacity(count);
        for i in 0..count {
            let last = i.saturating_add(1) >= count;
            let (value, arg_span) = match slots.get(i).cloned().flatten() {
                Some(arg) => (self.expr(&arg)?, arg.syntax().span()),
                None => {
                    let (file, default) = defaults
                        .get(i)
                        .cloned()
                        .flatten()
                        .ok_or(Abort::Internal(Internal::Unlowered, span))?;
                    // A default is stored by its words, so a cast within its basic type, which
                    // makes no statement (FTEF-30.6), is its operand; the cast's type is the
                    // declaration's, not this body's.
                    let mut default = default;
                    while let ast::Expr::Cast(cast) = &default {
                        default = cast.operand().at(span)?;
                    }
                    let here = std::mem::replace(&mut self.file, file);
                    let value = self.expr(&default);
                    self.file = here;
                    (value?, span)
                }
            };
            let value = if last {
                value
            } else {
                self.hold(value, arg_span)?
            };
            values.push((value, arg_span));
        }
        // FTEF-32.14: a call of constants that FTEQCC folds by its function's name.
        if let Some(name) = name {
            let words: Vec<Option<u32>> =
                values.iter().map(|(v, _)| self.constant_word(v)).collect();
            let floats = values
                .iter()
                .all(|(v, _)| self.type_of(v).etype() == Etype::Float);
            if let Some(folded) = qc_sema::fold_builtin_call(name, &words, floats) {
                return self.float_literal(folded, span);
            }
        }
        let passed = fixed.map_or(values.len(), |n| n.min(values.len()));
        for (slot, (value, arg_span)) in values.iter().take(passed).enumerate() {
            if slot == 0
                && let Some(name) = name
            {
                self.precaches.record(name, &self.program, value.ofs);
            }
            let to = parm_slot(slot).at(*arg_span)?;
            self.program.emit(
                Opcode::StoreV,
                operand(value.ofs).at(*arg_span)?,
                operand(to).at(*arg_span)?,
            );
        }
        let Some(op) = ops::call_opcode(passed) else {
            return Self::error(Code::E0307, "More than eight parameters".to_owned(), span);
        };
        self.program.emit(op, operand(callee.ofs).at(span)?, 0);
        self.return_type = ValueType::Of(function.ret.clone());
        Ok(Value {
            ofs: OFS_RETURN,
            kind: ValueKind::Return,
        })
    }
}

/// What initialises a function.
#[derive(Clone, Copy)]
enum FunctionInit<'t> {
    /// `= { ... }` or `= [frame, next] { ... }`.
    Body(&'t ast::FunctionBody),
    /// `= #n`.
    Builtin(&'t ast::Builtin),
}

/// `expr` with any parentheses around it taken off, when it is a field access, a subscript or a
/// prefix operator such as `*p`: the statement `qcc` makes an address is the last one emitted,
/// which for `(e.f)` is still the field's load (QC-17.3).
fn unparenthesized(expr: &ast::Expr) -> Option<ast::Expr> {
    match expr {
        ast::Expr::Paren(paren) => paren.expr().and_then(|inner| unparenthesized(&inner)),
        ast::Expr::Field(_) | ast::Expr::Index(_) | ast::Expr::Unary(_) => Some(expr.clone()),
        ast::Expr::Literal(_)
        | ast::Expr::Name(_)
        | ast::Expr::Not(_)
        | ast::Expr::Binary(_)
        | ast::Expr::Call(_)
        | ast::Expr::Postfix(_)
        | ast::Expr::Ternary(_)
        | ast::Expr::Cast(_)
        | ast::Expr::Vector(_)
        | ast::Expr::Sizeof(_) => None,
    }
}

/// Parameter slot `i`'s global offset, `OFS_PARM0 + 3 * i` (QC-18.3). A ninth argument's slot
/// is past the eight, as id's `qcc` writes `def_parms[8]`; its call is refused (QC-18.2).
fn parm_slot(i: usize) -> Option<u32> {
    u32::try_from(i)
        .ok()?
        .checked_mul(3)?
        .checked_add(OFS_PARM0)
}

/// The relative branch from statement `from` to statement `to` (QC-22.2).
fn branch(from: usize, to: usize) -> Result<i32, Internal> {
    let from = i64::try_from(from).map_err(|_| Internal::TooLarge)?;
    let to = i64::try_from(to).map_err(|_| Internal::TooLarge)?;
    i32::try_from(to.checked_sub(from).ok_or(Internal::TooLarge)?).map_err(|_| Internal::TooLarge)
}

/// The bytes of a name the syntax tree gave as one `char` per byte (Latin-1).
pub(crate) fn latin1(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect()
}

/// The step of a postfix `x++` (1) or `x--` (-1).
fn postfix_step(postfix: &ast::PostfixExpr) -> f32 {
    let decrement = postfix
        .syntax()
        .child_tokens()
        .any(|t| t.kind() == qc_syntax::SyntaxKind::MinusMinus);
    if decrement { -1.0 } else { 1.0 }
}

/// A statement or function index as the `i32` a record stores.
fn operand_index(index: usize) -> Result<i32, Internal> {
    i32::try_from(index).map_err(|_| Internal::TooLarge)
}

/// The literal an initialiser is (QC-11.3; QC-27.8 refuses any other).
fn literal_of(expr: &ast::Expr, dialect: Dialect) -> Option<Literal> {
    match expr {
        ast::Expr::Literal(literal) => literal_of_token(literal, dialect),
        ast::Expr::Paren(_)
        | ast::Expr::Name(_)
        | ast::Expr::Not(_)
        | ast::Expr::Binary(_)
        | ast::Expr::Field(_)
        | ast::Expr::Call(_)
        | ast::Expr::Postfix(_)
        | ast::Expr::Ternary(_)
        | ast::Expr::Index(_)
        | ast::Expr::Unary(_)
        | ast::Expr::Cast(_)
        | ast::Expr::Vector(_)
        | ast::Expr::Sizeof(_) => None,
    }
}

/// A literal's value as the vanilla lexer reads its text (QC-4 to QC-6); a frame macro
/// reference is the number the preprocessor wrote in its place (QC-8.5).
fn literal_of_token(literal: &ast::Literal, dialect: Dialect) -> Option<Literal> {
    let token = literal.token()?;
    let lexed = qc_lexer::lex_with(token.text(), dialect);
    let [token] = lexed.tokens.as_slice() else {
        return None;
    };
    match &token.kind {
        qc_lexer::TokenKind::Float(value) => Some(Literal::Float(*value)),
        qc_lexer::TokenKind::Vector(value) => Some(Literal::Vector(*value)),
        qc_lexer::TokenKind::String(bytes) => Some(Literal::String(bytes.clone())),
        qc_lexer::TokenKind::Name(_)
        | qc_lexer::TokenKind::Punct(_)
        | qc_lexer::TokenKind::FrameRef(_)
        | qc_lexer::TokenKind::Frames(_)
        | qc_lexer::TokenKind::FrameCommand(_) => None,
    }
}

/// Component `i` of `v`, 0 for none.
fn component(v: [f32; 3], i: usize) -> f32 {
    v.get(i).copied().unwrap_or(0.0)
}
