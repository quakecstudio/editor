// SPDX-License-Identifier: GPL-3.0-or-later

//! The typed view names the parts of each node.

use super::{SAMPLE, body, sexpr, statements};
use crate::ast::{self, AstNode as _};
use crate::{SyntaxKind, parse};

fn declarator(source: &str, name: &str) -> ast::Declarator {
    parse(source.as_bytes())
        .tree()
        .unwrap()
        .declarations()
        .flat_map(|d| d.declarators().collect::<Vec<_>>())
        .find(|d| d.name().is_some_and(|n| n.text() == name.as_bytes()))
        .unwrap()
}

#[test]
fn cast_checks_the_kind() {
    let root = parse(b"float a;").syntax();
    assert!(ast::SourceFile::cast(root.clone()).is_some());
    assert!(ast::Declaration::cast(root.clone()).is_none());
    assert!(ast::Expr::cast(root.clone()).is_none());
    let decl = root.children().next().unwrap();
    let typed = ast::Declaration::cast(decl.clone()).unwrap();
    assert_eq!(typed.syntax(), &decl);
    assert!(ast::Type::can_cast(SyntaxKind::FieldType));
    assert!(!ast::Type::can_cast(SyntaxKind::Literal));
}

#[test]
fn simple_types() {
    let kinds: Vec<_> = ["void", "float", "vector", "string", "entity"]
        .iter()
        .map(|t| {
            let source = format!("{t} x;");
            let decl = parse(source.as_bytes())
                .tree()
                .unwrap()
                .declarations()
                .next();
            let Some(ast::Type::Simple(simple)) = decl.unwrap().ty() else {
                panic!("{t}");
            };
            assert_eq!(simple.keyword().unwrap().text(), t.as_bytes());
            simple.kind().unwrap()
        })
        .collect();
    assert_eq!(
        kinds,
        [
            ast::SimpleTypeKind::Void,
            ast::SimpleTypeKind::Float,
            ast::SimpleTypeKind::Vector,
            ast::SimpleTypeKind::String,
            ast::SimpleTypeKind::Entity,
        ]
    );
}

#[test]
fn function_parts() {
    let stand = declarator(SAMPLE, "sample_stand1");
    let Some(ast::Initializer::Body(body)) = stand.initializer() else {
        panic!("no body");
    };
    let state = body.state().unwrap();
    assert_eq!(
        state.frame().unwrap().kind(),
        Some(ast::LiteralKind::FrameRef)
    );
    assert_eq!(state.next().unwrap().text(), b"sample_stand2");
    assert_eq!(body.block().unwrap().statements().count(), 1);

    let builtin = declarator(SAMPLE, "makevectors");
    let Some(ast::Initializer::Builtin(number)) = builtin.initializer() else {
        panic!("not a builtin");
    };
    assert_eq!(number.number().unwrap().text(), b"1");

    let constant = declarator(SAMPLE, "VEC_HULL_MIN");
    let Some(ast::Initializer::Value(ast::Expr::Literal(lit))) = constant.initializer() else {
        panic!("not a constant");
    };
    assert_eq!(lit.kind(), Some(ast::LiteralKind::Vector));
    let name = declarator(SAMPLE, "NAME");
    let Some(ast::Initializer::Value(ast::Expr::Literal(lit))) = name.initializer() else {
        panic!("not a constant");
    };
    assert_eq!(lit.kind(), Some(ast::LiteralKind::String));
}

#[test]
fn function_type_parts() {
    let parsed = parse(b"entity(entity start, .string fld) find;");
    let decl = parsed.tree().unwrap().declarations().next().unwrap();
    let Some(ast::Type::Function(function)) = decl.ty() else {
        panic!("not a function type");
    };
    assert_eq!(
        match function.return_type() {
            Some(ast::Type::Simple(simple)) => simple.kind(),
            _ => None,
        },
        Some(ast::SimpleTypeKind::Entity)
    );
    let params = function.params().unwrap();
    assert!(!params.is_variadic());
    let names: Vec<_> = params
        .params()
        .map(|p| p.name().unwrap().text_lossless())
        .collect();
    assert_eq!(names, ["start", "fld"]);
    let second = params.params().nth(1).unwrap();
    let Some(ast::Type::Field(field)) = second.ty() else {
        panic!("not a field");
    };
    assert!(matches!(field.inner(), Some(ast::Type::Simple(_))));
}

#[test]
fn statement_parts() {
    let parsed = body(
        "local float x, y;\nwhile (a) a = 0;\ndo a = 1; while (b);\nif (a) b = 1; else b = 2;\n\
         return a;\n{ a = 1; b = 2; }",
    );
    let stmts = statements(&parsed);
    let ast::Stmt::Local(local) = &stmts[0] else {
        panic!()
    };
    assert!(local.local_token().is_some());
    assert!(matches!(local.ty(), Some(ast::Type::Simple(_))));
    assert_eq!(local.declarators().count(), 2);
    let ast::Stmt::While(w) = &stmts[1] else {
        panic!()
    };
    assert_eq!(sexpr(&w.condition().unwrap()), "a");
    assert!(matches!(w.body(), Some(ast::Stmt::Expr(_))));
    let ast::Stmt::Do(d) = &stmts[2] else {
        panic!()
    };
    assert_eq!(sexpr(&d.condition().unwrap()), "b");
    assert!(matches!(d.body(), Some(ast::Stmt::Expr(_))));
    let ast::Stmt::If(i) = &stmts[3] else {
        panic!()
    };
    assert_eq!(sexpr(&i.condition().unwrap()), "a");
    assert!(i.then_branch().is_some());
    assert!(i.else_branch().is_some());
    let ast::Stmt::Return(r) = &stmts[4] else {
        panic!()
    };
    assert_eq!(sexpr(&r.value().unwrap()), "a");
    let ast::Stmt::Block(b) = &stmts[5] else {
        panic!()
    };
    assert_eq!(b.statements().count(), 2);
}

#[test]
fn binary_operators() {
    let ops = [
        ("*", ast::BinaryOp::Mul),
        ("/", ast::BinaryOp::Div),
        ("&", ast::BinaryOp::BitAnd),
        ("|", ast::BinaryOp::BitOr),
        ("+", ast::BinaryOp::Add),
        ("-", ast::BinaryOp::Sub),
        ("==", ast::BinaryOp::Eq),
        ("!=", ast::BinaryOp::Ne),
        ("<=", ast::BinaryOp::Le),
        (">=", ast::BinaryOp::Ge),
        ("<", ast::BinaryOp::Lt),
        (">", ast::BinaryOp::Gt),
        ("=", ast::BinaryOp::Assign),
        ("&&", ast::BinaryOp::And),
        ("||", ast::BinaryOp::Or),
    ];
    for (text, op) in ops {
        let parsed = body(&format!("a {text} b;"));
        let ast::Stmt::Expr(stmt) = &statements(&parsed)[0] else {
            panic!()
        };
        let Some(ast::Expr::Binary(bin)) = stmt.expr() else {
            panic!("{text}")
        };
        assert_eq!(bin.op(), Some(op), "{text}");
        assert_eq!(sexpr(&bin.lhs().unwrap()), "a");
        assert_eq!(sexpr(&bin.rhs().unwrap()), "b");
    }
}

#[test]
fn expression_parts() {
    let parsed = body("e.th (a, !b);");
    let ast::Stmt::Expr(stmt) = &statements(&parsed)[0] else {
        panic!()
    };
    let Some(ast::Expr::Call(call)) = stmt.expr() else {
        panic!()
    };
    let Some(ast::Expr::Field(field)) = call.callee() else {
        panic!()
    };
    assert_eq!(sexpr(&field.base().unwrap()), "e");
    assert_eq!(sexpr(&field.field().unwrap()), "th");
    let args: Vec<_> = call.args().unwrap().args().collect();
    assert_eq!(args.len(), 2);
    let ast::Expr::Not(not) = &args[1] else {
        panic!()
    };
    assert_eq!(sexpr(&not.operand().unwrap()), "b");
    let parsed = body("a = (b);");
    let ast::Stmt::Expr(stmt) = &statements(&parsed)[0] else {
        panic!()
    };
    let Some(ast::Expr::Binary(bin)) = stmt.expr() else {
        panic!()
    };
    let Some(ast::Expr::Paren(paren)) = bin.rhs() else {
        panic!()
    };
    assert_eq!(sexpr(&paren.expr().unwrap()), "b");
}

#[test]
fn missing_parts_are_none() {
    let parsed = body("a = ;");
    let ast::Stmt::Expr(stmt) = &statements(&parsed)[0] else {
        panic!()
    };
    let Some(ast::Expr::Binary(bin)) = stmt.expr() else {
        panic!()
    };
    assert!(bin.lhs().is_some());
    assert!(bin.rhs().is_none());
    let decl = parse(b"float")
        .tree()
        .unwrap()
        .declarations()
        .next()
        .unwrap();
    assert!(decl.ty().is_some());
    assert!(decl.declarators().next().unwrap().name().is_none());
}

#[test]
fn ancestors_and_tokens() {
    let parsed = body("a = b;");
    let root = parsed.syntax();
    let name = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::NameRef && n.text() == b"b")
        .unwrap();
    let up: Vec<SyntaxKind> = name.ancestors().map(|n| n.kind()).collect();
    assert_eq!(
        up,
        [
            SyntaxKind::BinaryExpr,
            SyntaxKind::ExprStmt,
            SyntaxKind::Block,
            SyntaxKind::FunctionBody,
            SyntaxKind::Declarator,
            SyntaxKind::Declaration,
            SyntaxKind::SourceFile,
        ]
    );
    let token = name.token_of(SyntaxKind::Name).unwrap();
    assert_eq!(token.parent(), name);
    assert_eq!(
        format!("{token:?}"),
        format!("Name@{0}..{1} \"b\"", token.span().start, token.span().end)
    );
}
