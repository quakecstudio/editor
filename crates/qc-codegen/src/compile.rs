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

use crate::back_end::{BackEnd, Layout};
use std::collections::HashMap;

use qc_diag::Span;
use qc_progs::Function;
use qc_sema::ir;
use qc_sema::lower::{Qualifier, declares};
use qc_sema::{DefId, DefKind, FileDiagnostic, FileId, ModuleIndex, Type};
use qc_syntax::ast::{self, AstNode};
use qc_syntax::c_scalar::CScalar;
use qc_syntax::{Dialect, Parse, SyntaxKind, SyntaxToken};
use qc_types::Etype;

use crate::Internal;
use crate::names::Names;
use crate::precache::Precaches;

use crate::program::{self, Literal, OFS_PARM0, Program, ValueType, operand, type_words};

mod allocate;
mod arrays;
mod body;

/// Why a declaration's code stopped: a construct the compiler cannot lower, which the semantic
/// model should have refused.
#[derive(Debug)]
pub(crate) enum Abort {
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
    /// Where the body being compiled starts, which of its function's bodies it is.
    body_start: Option<usize>,
    /// The syntax of the body being written from the semantic model, whose declarations a name
    /// or an effect of the model refers the writer to.
    writing: Option<std::rc::Rc<body::BodySyntax>>,
    /// The end of the last `local` declaration's globals, `locals_end` (QC-21.4).
    locals_end: u32,
    /// The dialect the files are read in, whose lexer reads each literal and builtin number.
    pub(crate) dialect: Dialect,
    /// The back end's passes the dialect's profile chooses.
    pub(crate) back_end: BackEnd,
    /// The compiler flags of the file being compiled, each from the byte it holds from
    /// (FTEF-21.1).
    flags: Vec<(usize, qc_syntax::CompilerFlags)>,
    /// The lexer flags of the file being compiled, each from the byte it holds from, which
    /// read a builtin's engine name (FTEF-12.12).
    lex_flags: Vec<(usize, qc_lexer::LexFlags)>,
    /// The default value of each parameter of each function declared so far, an expression of
    /// the file it is written in (FTE-18.7).
    defaults: HashMap<DefId, Vec<Option<(FileId, ast::Expr)>>>,
    /// Whether the module is compiled with FTEQCC's `-Fautoproto` (FTE-22.3, FTE-18.8).
    pub(crate) autoproto: bool,
    /// The return type of the function whose body is being compiled, `None` outside one.
    body_return: Option<ValueType>,
    /// The function's `ret*` local, once a bare `return` made it (FTE-18.10).
    ret_local: Option<usize>,
    /// The `$parmN` globals made for the parameter slots past the eighth (FTE-18.6).
    extra_parms: HashMap<usize, u32>,
    /// The `__va_list` of the variadic function being written, made before its parameters
    /// as FTEQCC's static is (FTE-18.6).
    va_list: Option<usize>,
    /// In a `wrap` function's body, the definition `prior` names (FTEF-15.11).
    prior: Option<usize>,
    /// The array getters and setters made so far, emitted after every file (FTE-13.8,
    /// FTE-13.11).
    array_functions: Vec<arrays::ArrayFunction>,
    /// The temporaries both arms of a `?:` write (FTEF-30.3), each with the index of the
    /// `?:`'s last statement: no store folds into that statement, since the other arm wrote
    /// the temporary too; a later statement that reuses the temporary is folded into as any.
    joined: HashMap<u32, usize>,
    /// The first statement of the body being compiled, `pr_scope->code`.
    body_first: usize,
    /// Each file's `s_file`, which a getter's function record takes from its array's file.
    s_files: HashMap<FileId, i32>,
    /// The functions defined in the body being compiled, compiled after it (FTEF-18.2).
    nested: Vec<Nested>,
    /// The record number the next nested function of the body being compiled takes
    /// (FTEF-18.2).
    next_nested: usize,
}

/// A function defined in another's body, compiled once that one is (FTEF-18.2).
struct Nested {
    written: Written,
    declarator: ast::Declarator,
    name: SyntaxToken,
    def: (DefId, usize),
    body: ast::FunctionBody,
}

impl<'a> Compiler<'a> {
    pub(crate) fn new(index: &'a ModuleIndex, names: &'a Names) -> Self {
        Self {
            index,
            names,
            program: Program::new(),
            precaches: Precaches::default(),
            errors: Vec::new(),
            file: FileId::new(0),
            s_file: 0,
            scope: None,
            body_start: None,
            writing: None,
            locals_end: 0,
            dialect: Dialect::Vanilla,
            back_end: BackEnd::of(Dialect::Vanilla),
            flags: Vec::new(),
            lex_flags: Vec::new(),
            array_functions: Vec::new(),
            joined: HashMap::new(),
            body_first: 0,
            prior: None,
            extra_parms: HashMap::new(),
            va_list: None,
            body_return: None,
            ret_local: None,
            defaults: HashMap::new(),
            autoproto: false,
            s_files: HashMap::new(),
            nested: Vec::new(),
            next_nested: 0,
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
            if let Err(Abort::Internal(internal, span)) = result {
                return Err((internal, file, span));
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
                    self.array_size_immediates(&declarator)?;
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
            if let Err(Abort::Internal(internal, span)) = result {
                return Err((internal, file, span));
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
        // FTE-18.8: with autoprototyping a default the first pass did not find initialised, any
        // but a literal or a field, is no default.
        let kept = |value: &ast::Expr| {
            !self.autoproto || {
                let mut value = value.clone();
                while let ast::Expr::Cast(cast) = &value {
                    match cast.operand() {
                        Some(operand) => value = operand,
                        None => break,
                    }
                }
                match &value {
                    ast::Expr::Literal(_) => true,
                    ast::Expr::Name(name) => name
                        .name()
                        .and_then(|token| self.names.def(self.file, token.span(), None))
                        .and_then(|id| self.index.def(id))
                        .is_some_and(|def| def.kind == DefKind::Field),
                    _ => false,
                }
            }
        };
        let defaults: Vec<Option<(FileId, ast::Expr)>> = list
            .params()
            .map(|param| {
                param
                    .default_value()
                    .filter(|value| kept(value))
                    .map(|value| (self.file, value))
            })
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

    /// The definition the name token `token` is (QC-12).
    fn def_of(&self, token: &SyntaxToken) -> Lowered<DefId> {
        self.names
            .def(self.file, token.span(), self.scope)
            .ok_or(Abort::Internal(Internal::Unresolved, token.span()))
    }

    /// FTEF-32.16: `QCC_UnmarshalLocals` finalises each `static` local and each initialised
    /// constant one with the globals, before the locals it lays out: so they are made here,
    /// before the body's parameters, and the body's own locals of nested functions are left to
    /// those functions.
    fn hoist_static_locals(&mut self, body: &ast::FunctionBody) -> Lowered<()> {
        let root = body.syntax();
        for local in root.descendants().filter_map(ast::LocalDecl::cast) {
            let nested = local
                .syntax()
                .ancestors()
                .skip(1)
                .take_while(|node| node != root)
                .any(|node| node.kind() == SyntaxKind::FunctionBody);
            if nested {
                continue;
            }
            let is_static = declares(local.syntax(), &Qualifier::Static);
            for declarator in local.declarators() {
                let init = declarator.initializer();
                let Some(name) = declarator.name() else {
                    continue;
                };
                // A nested function's local is stored at run time (FTEF-18.2), a static one's
                // word holds its record's number, so only the static takes storage now.
                if matches!(init, Some(ast::Initializer::Body(_))) && !is_static {
                    continue;
                }
                let id = self.def_of(&name)?;
                if is_static || (init.is_some() && self.index.is_constant(id)) {
                    self.program.get_def(self.index, id).at(name.span())?;
                }
            }
        }
        Ok(())
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
            ValueKind::Temp(ty) => ty.clone(),
        }
    }

    // ---------------------------------------------------------------------------------------
    // Declarations
    // ---------------------------------------------------------------------------------------

    /// `PR_ParseDefs` at file level (QC-11): each name is made or found, and an initialiser
    /// defines a function (QC-13) or stores a constant (QC-11.6).
    fn declaration(&mut self, declaration: &ast::Declaration) -> Lowered<()> {
        let written = Written {
            ty: declaration.ty(),
            wrap: declares(declaration.syntax(), &Qualifier::Wrap),
            nested: false,
        };
        for declarator in declaration.declarators() {
            let name = declarator.name().at(declaration.syntax().span())?;
            let id = self.declared(&name)?;
            self.record_defaults(id, declaration.ty().as_ref(), &declarator);
            self.array_size_immediates(&declarator)?;
            let place = self.program.get_def(self.index, id).at(name.span())?;
            match declarator.initializer() {
                None => {}
                Some(ast::Initializer::Value(expr)) => {
                    let span = expr.syntax().span();
                    // FTEF-19.79: under FTEQCC a definition already initialised keeps its value
                    // when initialised again: the model has judged a differing value (FTE-22.6),
                    // and one of a `#pragma noref 1` definition is dropped without a word.
                    if self.dialect.has_fteqcc_extensions()
                        && self.program.def(place).is_some_and(|def| def.initialized)
                    {
                        continue;
                    }
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
                        // FTE-15.9: the literal's `IMMEDIATE` record follows the constant's.
                        if self.dialect.has_fteqcc_extensions() && self.constant(place) {
                            self.program.alias_immediate(place).at(span)?;
                        }
                    } else {
                        let value = self.index.initial_value(id).at(span)?;
                        self.program
                            .initialise_words(place, value.words())
                            .at(span)?;
                    }
                }
                Some(ast::Initializer::Body(body)) => {
                    self.function(
                        (&written, &declarator),
                        &name,
                        (id, place),
                        FunctionInit::Body(&body),
                    )?;
                }
                Some(ast::Initializer::Builtin(builtin)) => {
                    self.function(
                        (&written, &declarator),
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

    /// FTE-13.16: under FTEQCC an array's size is read as an expression
    /// (`QCC_PR_IntConstExpr`), so each literal in it is an immediate made before the array's
    /// globals, in the order written.
    fn array_size_immediates(&mut self, declarator: &ast::Declarator) -> Lowered<()> {
        if !self.dialect.has_fteqcc_extensions() {
            return Ok(());
        }
        for suffix in declarator.array_suffixes() {
            for node in suffix.descendants() {
                let Some(expr) = ast::Expr::cast(node) else {
                    continue;
                };
                if !matches!(expr, ast::Expr::Literal(_)) {
                    continue;
                }
                if let Some(literal) = literal_of(&expr, self.dialect) {
                    self.program.immediate(&literal).at(expr.syntax().span())?;
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
            // FTE-15.9: under FTEQCC a float item's value is made an immediate first
            // (`QCC_MakeFloatConst`, one shared by value) and the item a name for its global,
            // so the `IMMEDIATE` record comes before the item's and the two share the slot.
            let id = self.def_of(&name)?;
            let value = self.index.initial_value(id).at(name.span())?;
            if self.dialect.has_fteqcc_extensions()
                && self.program.place(id).is_none()
                && let [word] = value.words()
                && self.enum_item_is_float(id)
            {
                let immediate = self
                    .program
                    .immediate(&Literal::Float(f32::from_bits(*word)))
                    .at(name.span())?;
                self.program
                    .alias_def(self.index, id, immediate)
                    .at(name.span())?;
                continue;
            }
            let place = self.declare(&name)?;
            self.program
                .initialise_words(place, value.words())
                .at(name.span())?;
        }
        Ok(())
    }

    /// Whether the enum item `id` is a float, as a plain `enum`'s items are (FTE-15.6), rather
    /// than an `int` or another C scalar (FTE-15.7).
    fn enum_item_is_float(&self, id: DefId) -> bool {
        let Some(definition) = self.index.def(id) else {
            return false;
        };
        let written = &definition.ty;
        let ty = match self.index.alias(written) {
            Some(alias) => alias.clone(),
            None => written.clone(),
        };
        ty.fteqcc_scalar().is_none() && ValueType::Of(ty).etype() == Etype::Float
    }

    /// A function definition, the function branch of `PR_ParseDefs` and
    /// `PR_ParseImmediateStatements` (QC-13): a body's parameters, state header and
    /// statements, or a builtin number; then the function's record (QC-13.9, QC-21.4,
    /// QC-25.2).
    fn function(
        &mut self,
        (written, declarator): (&Written, &ast::Declarator),
        name: &SyntaxToken,
        (id, place): (DefId, usize),
        init: FunctionInit<'_>,
    ) -> Lowered<()> {
        let span = name.span();
        let def = self.program.def(place).at(span)?.clone();
        let function_type = def.ty.function().at(span)?.clone();
        self.set_line(span);
        let wrapped = self.wrap(written, &def, init, span)?;
        // FTEF-18.2: the functions this body defines take the records after its own.
        self.next_nested = self
            .program
            .functions
            .len()
            .checked_add(usize::from(wrapped.is_none()))
            .at(span)?;
        self.scope = Some(id);
        self.body_start = match init {
            FunctionInit::Body(body) => Some(body.syntax().span().start),
            FunctionInit::Builtin(_) => None,
        };
        // FTEF-32.16: a body's static locals and its initialised constant ones take storage
        // before its own, outside the locals its record names.
        self.va_list = None;
        if let FunctionInit::Body(body) = init
            && self.dialect.has_fteqcc_extensions()
        {
            self.hoist_static_locals(body)?;
            // FTE-18.6: the `__va_list` is a static of the function, `QCC_PR_GetSRef` with the
            // scope and `saved`, so it takes the globals before the parameters, which FTEQCC's
            // deferred locals follow.
            if function_type.variadic {
                let vector = ValueType::simple(Etype::Vector);
                self.va_list = Some(
                    self.program
                        .generated_array("__va_list", vector, VA_COUNT)
                        .at(span)?,
                );
            }
        }
        let locals_start = self.program.next_global().at(span)?;
        self.locals_end = locals_start;
        // FTE-21.9: under the block layout the parameters, locals and temporaries of a body form
        // a block laid out after every global, static and immediate of the module
        // (`QCC_UnmarshalLocals`).
        let block = self.back_end.layout == Layout::Blocks && matches!(init, FunctionInit::Body(_));
        if block {
            self.program.open_block();
        }
        let mut record_name = if written.nested {
            String::new()
        } else {
            def.name.clone()
        };
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
                self.parameters(written.ty.as_ref(), declarator, span)?;
                // FTE-21.10: FTEQCC's record counts the parameters among the locals, so a call
                // that enters the function again restores them; id's `qcc` counts only up to
                // the last `local` declaration (QC-21.4).
                if self.dialect == Dialect::Fteqcc {
                    self.locals_end = self.program.next_global().at(span)?;
                }
                let code = operand_index(self.program.statements.len()).at(span)?;
                self.body_first = self.program.statements.len();
                self.body_return = Some(ValueType::Of(function_type.ret.clone()));
                self.ret_local = None;
                // Every body is written from its generated statements, the intermediate
                // layer's whole body; one the semantic model did not check is not lowered.
                let typed = self
                    .typed_body()
                    .ok_or(Abort::Internal(Internal::Unfollowed, span))?;
                self.write_body(typed, body, span)?;
                code
            }
        };
        self.scope = None;
        self.prior = None;
        self.body_return = None;
        self.ret_local = None;
        // `PR_ParseDefs` marks the function initialised here, which nothing reads for a
        // function: no literal is a function (QC-20.2), and no function is saved (QC-21.6).
        // FTEF-18.2: a nested function's local is stored when the outer function runs.
        if wrapped.is_none() && !written.nested {
            let number = operand_index(self.program.functions.len()).at(span)?;
            self.program
                .set_words(def.ofs, &[u32::from_ne_bytes(number.to_ne_bytes())])
                .at(span)?;
        }
        let s_name = self.program.copy_string(&latin1(&record_name)).at(span)?;
        // FTE-21.10: FTEQCC's record counts the named parameters of a variadic function
        // (`funcs[i].numparms = p`); id's `qcc` writes -1 (QC-13.3).
        let numparms = if function_type.variadic && !self.dialect.has_fteqcc_extensions() {
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
        if block {
            // FTE-21.9: the block's words, temporaries among them, are the record's locals;
            // its `parm_start` is set when the module's blocks are laid out.
            let record = wrapped.unwrap_or(self.program.functions.len());
            let len = self.program.close_block(record).at(span)?;
            locals = i32::try_from(len).map_err(|_| Abort::Internal(Internal::TooLarge, span))?;
            parm_start = 0;
        }
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
        // FTEF-18.2: `QCC_PR_FinaliseFunctions` writes each function's statements in the order
        // of the records, so a nested function's follow its outer function's.
        for nested in std::mem::take(&mut self.nested) {
            self.function(
                (&nested.written, &nested.declarator),
                &nested.name,
                nested.def,
                FunctionInit::Body(&nested.body),
            )?;
        }
        Ok(())
    }

    /// The global of parameter slot `u`: `OFS_PARM0 + 3 * u` for the first eight, after them
    /// FTEQCC's `$parmN` global, made the first time it is needed (FTE-18.6).
    pub(super) fn parameter_slot(&mut self, u: usize, span: Span) -> Lowered<u32> {
        const MAX_PARMS: usize = 8;
        if u < MAX_PARMS {
            return parm_slot(u).at(span);
        }
        if let Some(&slot) = self.extra_parms.get(&u) {
            return Ok(slot);
        }
        let made = self
            .program
            .generated(
                &format!("$parm{u}"),
                ValueType::simple(Etype::Vector),
                VECTOR_WORDS,
            )
            .at(span)?;
        let slot = self.program.def(made).at(span)?.ofs;
        self.extra_parms.insert(u, slot);
        Ok(slot)
    }

    /// FTE-18.10: what a bare `return;` returns: under FTEQCC, in a function that returns a
    /// value, its local `ret*` of the return type, made at the first such `return`; else 0.
    fn bare_return(&mut self, span: Span) -> Lowered<u32> {
        if self.bare_return_kind() == body::BareReturn::Plain {
            return Ok(0);
        }
        let Some(ty) = self.body_return.clone() else {
            return Ok(0);
        };
        if let Some(place) = self.ret_local {
            return Ok(self.program.def(place).at(span)?.ofs);
        }
        let place = self
            .program
            .generated("ret*", ty.clone(), ty.words())
            .at(span)?;
        self.ret_local = Some(place);
        self.locals_end = self.program.next_global().at(span)?;
        Ok(self.program.def(place).at(span)?.ofs)
    }

    /// FTEF-18.2: numbers the nested function `body` the local `id` at `place` is given, and
    /// each nested in it, and keeps it to compile after this one; the word of its number.
    fn register_nested(
        &mut self,
        written: Written,
        declarator: &ast::Declarator,
        name: &SyntaxToken,
        (id, place): (DefId, usize),
        body: &ast::FunctionBody,
    ) -> Lowered<u32> {
        let span = name.span();
        let number = self.next_nested;
        let records = body
            .syntax()
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::FunctionBody)
            .count();
        self.next_nested = number.checked_add(records).at(span)?;
        let word = u32::from_ne_bytes(operand_index(number).at(span)?.to_ne_bytes());
        self.nested.push(Nested {
            written,
            declarator: declarator.clone(),
            name: name.clone(),
            def: (id, place),
            body: body.clone(),
        });
        Ok(word)
    }

    /// FTEF-15.11: for a `wrap` definition's body, the function it wraps moves to a new
    /// record at the end, `prior*NAME`, as `QCC_PR_GenerateQCFunction` moves it, and `prior` in
    /// the body is a constant of its number; the wrapper takes the old record's number, which
    /// is returned. `None` for any other definition.
    fn wrap(
        &mut self,
        written: &Written,
        def: &program::Def,
        init: FunctionInit<'_>,
        span: Span,
    ) -> Lowered<Option<usize>> {
        let wraps = self.dialect.has_fteqcc_extensions()
            && matches!(init, FunctionInit::Body(_))
            && written.wrap;
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
        // FTE-21.9: the moved body's block of locals goes with its record.
        self.program.rekey_block(
            number,
            usize::try_from(prior_number).map_err(|_| Abort::Internal(Internal::TooLarge, span))?,
        );
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
        ty: Option<&ast::Type>,
        declarator: &ast::Declarator,
        span: Span,
    ) -> Lowered<()> {
        let list = match (declarator.params(), ty) {
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

    // ---------------------------------------------------------------------------------------
    // Statements
    // ---------------------------------------------------------------------------------------

    /// `PR_ParseStatement` (QC-14, QC-22.1).
    /// Records `span`'s start as where the statements emitted from here come from (QC-27.19).
    fn set_line(&mut self, span: Span) {
        self.program.origin = Some((self.file, span.start));
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

    // ---------------------------------------------------------------------------------------
    // Expressions
    // ---------------------------------------------------------------------------------------

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

    fn constant(&self, place: usize) -> bool {
        self.program.def(place).is_some_and(|def| {
            def.initialized
                && def.count.is_none()
                && def
                    .id
                    .map_or(def.name == "IMMEDIATE", |id| self.index.is_constant(id))
        })
    }

    /// Whether `value` is a `?:`'s temporary whose last arm's statement is the last one made,
    /// which no store folds into (FTEF-30.3).
    fn is_joined(&self, value: &Value) -> bool {
        self.joined.get(&value.ofs) == Some(&self.program.statements.len().saturating_sub(1))
    }

    fn def_value(&self, place: usize, span: Span) -> Lowered<Value> {
        let ofs = self.program.def(place).at(span)?.ofs;
        Ok(Value {
            ofs,
            kind: ValueKind::Def(place),
        })
    }

    /// The place of the immediate holding `immediate`'s value, made now if there is none.
    fn immediate_place(&mut self, immediate: ir::Immediate, span: Span) -> Lowered<usize> {
        let [x, y, z] = immediate.words.map(f32::from_bits);
        let literal = match immediate.kind {
            ir::ImmediateKind::Vector => Literal::Vector([x, y, z]),
            ir::ImmediateKind::Float => Literal::Float(x),
            // FTEF-26.14: an integer constant, such as the truth `&&` and `||` of two
            // constants fold to (`QCC_MakeIntConst`).
            ir::ImmediateKind::Integer => {
                let [bits, _, _] = immediate.words;
                let int = ValueType::Of(Type::Named(CScalar::Integer.name().into()));
                return self.program.int_immediate(bits, int).at(span);
            }
        };
        self.program.immediate(&literal).at(span)
    }

    /// The typed body of the function being compiled, as the semantic model checked it.
    fn typed_body(&self) -> Option<&'a qc_sema::TypedBody> {
        let index = self.index;
        let function = self.scope?;
        match self.body_start {
            Some(start) => index.body_at(function, start),
            None => index.body(function),
        }
    }

    /// Whether `value` is a definition or a word of one (a local vector's component, a
    /// struct's member), rather than a temporary: FTEQCC's `sym->temp` is false for it.
    fn is_definition_word(&self, value: &Value) -> bool {
        match value.kind {
            ValueKind::Def(_) => true,
            ValueKind::Temp(_) => self.program.defs.iter().any(|def| {
                let words = type_words(self.index, &def.ty).saturating_mul(def.count.unwrap_or(1));
                def.name != "IMMEDIATE"
                    && value.ofs >= def.ofs
                    && value.ofs < def.ofs.saturating_add(words)
            }),
        }
    }
}

impl Compiler<'_> {
    /// FTE-18.12: `opt_assignments` for a store to the global `to`: a temporary that the
    /// statement before writes is written to `to` by that statement instead; true when it was.
    fn fold_into(&mut self, value: &Value, to: u32) -> bool {
        if !self.dialect.has_fteqcc_extensions()
            || !matches!(value.kind, ValueKind::Temp(_))
            || self.is_joined(value)
            || self.is_definition_word(value)
        {
            return false;
        }
        let Some(last) = self.program.statements.last_mut() else {
            return false;
        };
        let (Ok(made), Ok(to)) = (operand(value.ofs), operand(to)) else {
            return false;
        };
        if last.c != made {
            return false;
        }
        last.c = to;
        true
    }
}

/// What a function's declaration writes besides its declarator: its type, whether it is a
/// `wrap` definition (FTEF-15.11), and whether it is a nested function with no name of its own
/// (FTEF-18.2).
struct Written {
    ty: Option<ast::Type>,
    wrap: bool,
    nested: bool,
}

/// What initialises a function.
#[derive(Clone, Copy)]
enum FunctionInit<'t> {
    /// `= { ... }` or `= [frame, next] { ... }`.
    Body(&'t ast::FunctionBody),
    /// `= #n`.
    Builtin(&'t ast::Builtin),
}

/// The elements of a variadic function's `__va_list` (FTE-18.6: `int maxvacount = 24;`).
const VA_COUNT: u32 = 24;
/// A vector's words, `qc-types`' `PROGS_VECTOR_SLOTS`, as a definition's size.
const VECTOR_WORDS: u32 = 3;

/// Parameter slot `i`'s global offset, `OFS_PARM0 + 3 * i` (QC-18.3). A ninth argument's slot
/// is past the eight, as id's `qcc` writes `def_parms[8]`; its call is refused (QC-18.2).
fn parm_slot(i: usize) -> Option<u32> {
    u32::try_from(i)
        .ok()?
        .checked_mul(VECTOR_WORDS)?
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
