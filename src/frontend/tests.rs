//! Frontend tests. Synthetic fixtures written for this project, not
//! light-cljs output and not adapted from QuickJS, except one line in
//! `hashbang_comment_only_at_start`, three scripts in
//! `arrow_functions_and_asi`, one in `break_and_continue`, two in
//! `labelled_statements` and six in
//! `number_literal_member_access` (each marked there, with the QuickJS
//! notice), the keys and shorthands of `object_literals`, the `${a, b}`
//! and nested-template forms of `template_literals` (notice in
//! `tests/js_source.rs` `template_literal_values`),
//! and the name `Ãµ` in `unicode_identifiers`, from QuickJS
//! `test_unicode_ident` (notice in `tests/js_source.rs`
//! `identifiers_are_not_normalized`).

use super::ast::{
    BinaryOp, Expr, ExprKind, Function, FunctionKind, LogicalOp, PropertyName, Script, Stmt,
    StmtKind, UnaryOp, UpdateOp,
};
use super::lexer::{Lexer, Punct, TokenKind};
use super::{
    MAX_NESTING, ScriptOptions, SyntaxError, SyntaxErrorKind, compile_script, line_column,
    parse_script, parse_script_with,
};

// ----- S-expression rendering of the AST ------------------------------------

fn render_expr(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Number(n) => format!("{n}"),
        ExprKind::String(s) => format!("{s:?}"),
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::Null => "null".into(),
        ExprKind::This => "this".into(),
        ExprKind::Undefined => "undefined".into(),
        ExprKind::Identifier(name) => name.clone(),
        ExprKind::Unary { op, operand } => {
            let op = match op {
                UnaryOp::Not => "!",
                UnaryOp::Neg => "neg",
                UnaryOp::Plus => "pos",
                UnaryOp::BitNot => "~",
                UnaryOp::Typeof => "typeof",
                UnaryOp::Void => "void",
            };
            format!("({op} {})", render_expr(operand))
        }
        ExprKind::Binary {
            op, left, right, ..
        } => {
            let op = binary_name(*op);
            format!("({op} {} {})", render_expr(left), render_expr(right))
        }
        ExprKind::CompoundAssign {
            op, target, value, ..
        } => {
            let op = binary_name(*op);
            format!("({op}= {} {})", target.name, render_expr(value))
        }
        ExprKind::CompoundAssignProperty {
            op, target, value, ..
        } => {
            let op = binary_name(*op);
            format!("({op}= {} {})", render_expr(target), render_expr(value))
        }
        ExprKind::Logical { op, left, right } => {
            let op = match op {
                LogicalOp::And => "&&",
                LogicalOp::Or => "||",
            };
            format!("({op} {} {})", render_expr(left), render_expr(right))
        }
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => format!(
            "(? {} {} {})",
            render_expr(test),
            render_expr(consequent),
            render_expr(alternate)
        ),
        ExprKind::Update {
            op, prefix, target, ..
        } => {
            let op = match op {
                UpdateOp::Inc => "++",
                UpdateOp::Dec => "--",
            };
            let fix = if *prefix { "pre" } else { "post" };
            format!("({fix}{op} {})", target.name)
        }
        ExprKind::UpdateProperty {
            op, prefix, target, ..
        } => {
            let op = match op {
                UpdateOp::Inc => "++",
                UpdateOp::Dec => "--",
            };
            let fix = if *prefix { "pre" } else { "post" };
            format!("({fix}{op} {})", render_expr(target))
        }
        ExprKind::Assign { target, value } => {
            format!("(= {} {})", target.name, render_expr(value))
        }
        ExprKind::Call { callee, args, .. } | ExprKind::New { callee, args, .. } => {
            let head = match &e.kind {
                ExprKind::New { .. } => "new",
                _ => "call",
            };
            let mut out = format!("({head} {}", render_expr(callee));
            for arg in args {
                out.push(' ');
                out.push_str(&render_expr(arg));
            }
            out.push(')');
            out
        }
        ExprKind::Member {
            object, property, ..
        } => {
            format!("(. {} {})", render_expr(object), property.name)
        }
        ExprKind::Function(f) => render_function(f),
        ExprKind::Sequence { left, right } => {
            format!("(, {} {})", render_expr(left), render_expr(right))
        }
        ExprKind::Object(properties) => {
            let mut out = "(object".to_string();
            for p in properties {
                let name = match &p.name {
                    PropertyName::Identifier(name) => name.clone(),
                    PropertyName::String(s) => format!("{s:?}"),
                    PropertyName::Number(n) => format!("{n}"),
                };
                if p.shorthand {
                    out.push_str(&format!(" (shorthand {name} {})", render_expr(&p.value)));
                } else {
                    out.push_str(&format!(" ({name} {})", render_expr(&p.value)));
                }
            }
            out.push(')');
            out
        }
        ExprKind::AssignMember {
            object,
            property,
            value,
            ..
        } => format!(
            "(= (. {} {}) {})",
            render_expr(object),
            property.name,
            render_expr(value)
        ),
        // Text parts render as their cooked value, followed by `raw` and
        // the raw text where that differs.
        ExprKind::Template { quasis, exprs } => {
            let mut out = "(template".to_string();
            for (i, part) in quasis.iter().enumerate() {
                out.push_str(&format!(" {:?}", part.cooked));
                if part.raw != part.cooked.to_string() {
                    out.push_str(&format!(" raw {:?}", part.raw));
                }
                if let Some(e) = exprs.get(i) {
                    out.push(' ');
                    out.push_str(&render_expr(e));
                }
            }
            out.push(')');
            out
        }
        ExprKind::Array(elements) => {
            let mut out = "(array".to_string();
            for element in elements {
                out.push(' ');
                match element {
                    Some(e) => out.push_str(&render_expr(e)),
                    None => out.push_str("<hole>"),
                }
            }
            out.push(')');
            out
        }
        ExprKind::Index { object, index, .. } => {
            format!("([] {} {})", render_expr(object), render_expr(index))
        }
        ExprKind::AssignIndex {
            object,
            index,
            value,
            ..
        } => format!(
            "(= ([] {} {}) {})",
            render_expr(object),
            render_expr(index),
            render_expr(value)
        ),
    }
}

fn binary_name(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Rem => "%",
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Lt => "<",
        BinaryOp::Gt => ">",
        BinaryOp::Le => "<=",
        BinaryOp::Ge => ">=",
        BinaryOp::LooseEq => "==",
        BinaryOp::LooseNe => "!=",
        BinaryOp::StrictEq => "===",
        BinaryOp::StrictNe => "!==",
        BinaryOp::BitAnd => "&",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::Shl => "<<",
        BinaryOp::Sar => ">>",
        BinaryOp::UShr => ">>>",
        BinaryOp::Exp => "**",
    }
}

fn render_function(f: &Function) -> String {
    // A parameter with a default renders as `name=default`.
    let params: Vec<String> = f
        .params
        .iter()
        .zip(&f.defaults)
        .map(|(p, d)| match d {
            Some(d) => format!("{}={}", p.name, render_expr(d)),
            None => p.name.clone(),
        })
        .collect();
    let mut out = match f.kind {
        FunctionKind::Ordinary => format!(
            "(function {} ({})",
            f.name.as_ref().map_or("-", |n| n.name.as_str()),
            params.join(" ")
        ),
        FunctionKind::Arrow => format!("(arrow ({})", params.join(" ")),
        FunctionKind::ConciseArrow => {
            let [
                Stmt {
                    kind: StmtKind::Return(Some(value)),
                    span,
                    ..
                },
            ] = f.body.as_slice()
            else {
                panic!("a concise arrow body is one return: {f:?}");
            };
            assert_eq!(
                *span, value.span,
                "the concise body's return is its expression"
            );
            return format!("(arrow ({}) => {})", params.join(" "), render_expr(value));
        }
    };
    for s in &f.body {
        out.push(' ');
        out.push_str(&render_stmt(s));
    }
    out.push(')');
    out
}

fn render_list(head: &str, body: &[Stmt]) -> String {
    let mut out = format!("({head}");
    for s in body {
        out.push(' ');
        out.push_str(&render_stmt(s));
    }
    out.push(')');
    out
}

fn render_lexical(decl: &super::ast::LexicalDecl) -> String {
    let keyword = match decl.kind {
        super::ast::LexicalKind::Let => "let",
        super::ast::LexicalKind::Const => "const",
    };
    render_declarators(keyword, &decl.declarators)
}

fn render_declarators(keyword: &str, declarators: &[super::ast::Declarator]) -> String {
    let mut out = format!("({keyword}");
    for d in declarators {
        match &d.init {
            Some(init) => out.push_str(&format!(" ({} {})", d.name.name, render_expr(init))),
            None => out.push_str(&format!(" ({})", d.name.name)),
        }
    }
    out.push(')');
    out
}

fn render_stmt(s: &Stmt) -> String {
    match &s.kind {
        StmtKind::FunctionDecl(f) => render_function(f),
        StmtKind::Lexical(decl) => render_lexical(decl),
        StmtKind::Var(declarators) => render_declarators("var", declarators),
        StmtKind::Expr(e) => render_expr(e),
        StmtKind::Return(None) => "(return)".into(),
        StmtKind::Return(Some(e)) => format!("(return {})", render_expr(e)),
        StmtKind::Block(body) => render_list("block", body),
        StmtKind::Throw(e) => format!("(throw {})", render_expr(e)),
        StmtKind::Try {
            block,
            handler,
            finalizer,
        } => {
            let mut out = format!("(try {}", render_list("block", block));
            if let Some(h) = handler {
                let head = match &h.param {
                    Some(p) => format!("catch {}", p.name),
                    None => "catch".to_string(),
                };
                out.push(' ');
                out.push_str(&render_list(&head, &h.body));
            }
            if let Some(f) = finalizer {
                out.push(' ');
                out.push_str(&render_list("finally", f));
            }
            out.push(')');
            out
        }
        StmtKind::If {
            test,
            consequent,
            alternate,
        } => match alternate {
            Some(alt) => format!(
                "(if {} {} {})",
                render_expr(test),
                render_stmt(consequent),
                render_stmt(alt)
            ),
            None => format!("(if {} {})", render_expr(test), render_stmt(consequent)),
        },
        StmtKind::While { test, body } => {
            format!("(while {} {})", render_expr(test), render_stmt(body))
        }
        StmtKind::DoWhile { body, test } => {
            format!("(do {} {})", render_stmt(body), render_expr(test))
        }
        StmtKind::For {
            init,
            test,
            update,
            body,
        } => {
            let init = match init {
                Some(super::ast::ForInit::Expr(e)) => render_expr(e),
                Some(super::ast::ForInit::Lexical(decl)) => render_lexical(decl),
                None => "_".into(),
            };
            let part = |e: &Option<Expr>| e.as_ref().map_or("_".into(), render_expr);
            format!(
                "(for {init} {} {} {})",
                part(test),
                part(update),
                render_stmt(body)
            )
        }
        StmtKind::Break(None) => "(break)".into(),
        StmtKind::Break(Some(l)) => format!("(break {})", l.name),
        StmtKind::Continue(None) => "(continue)".into(),
        StmtKind::Continue(Some(l)) => format!("(continue {})", l.name),
        StmtKind::Labelled { label, body } => {
            format!("(label {} {})", label.name, render_stmt(body))
        }
        StmtKind::Switch {
            discriminant,
            cases,
        } => {
            let mut out = format!("(switch {}", render_expr(discriminant));
            for case in cases {
                let head = match &case.test {
                    Some(test) => format!("case {}", render_expr(test)),
                    None => "default".to_string(),
                };
                out.push(' ');
                out.push_str(&render_list(&head, &case.body));
            }
            out.push(')');
            out
        }
        StmtKind::Empty => "(empty)".into(),
    }
}

/// Parse and render; panics (failing the test) with the diagnostic.
fn ast(src: &str) -> String {
    let script = parse_script(src).unwrap_or_else(|e| panic!("{src:?} failed: {e}"));
    script
        .body
        .iter()
        .map(render_stmt)
        .collect::<Vec<_>>()
        .join(" ")
}

fn err(src: &str) -> SyntaxError {
    match parse_script(src) {
        Ok(script) => panic!("{src:?} parsed but must be rejected: {script:?}"),
        Err(e) => e,
    }
}

/// Assert `src` is rejected with `kind` at 1-based `line:column`.
#[track_caller]
fn rejects(src: &str, kind: SyntaxErrorKind, line: u32, column: u32) {
    let e = err(src);
    assert_eq!(
        (e.kind, e.line, e.column),
        (kind, line, column),
        "{src:?} gave {e}"
    );
}

#[track_caller]
fn rejects_with(src: &str, kind: SyntaxErrorKind, line: u32, column: u32, needle: &str) {
    let e = err(src);
    assert_eq!(
        (e.kind, e.line, e.column),
        (kind, line, column),
        "{src:?} gave {e}"
    );
    assert!(e.message.contains(needle), "{src:?}: {e} lacks {needle:?}");
}

// ----- positive: precedence and associativity -------------------------------

#[test]
fn binary_precedence_and_left_associativity() {
    assert_eq!(ast("1 + 2 * 3;"), "(+ 1 (* 2 3))");
    assert_eq!(ast("(1 + 2) * 3;"), "(* (+ 1 2) 3)");
    assert_eq!(ast("1 - 2 - 3;"), "(- (- 1 2) 3)");
    assert_eq!(ast("8 / 4 / 2 % 3;"), "(% (/ (/ 8 4) 2) 3)");
    assert_eq!(ast("a < b + 1 === c > d;"), "(=== (< a (+ b 1)) (> c d))");
    assert_eq!(ast("a == b != c;"), "(!= (== a b) c)");
    assert_eq!(ast("a !== b <= c;"), "(!== a (<= b c))");
    assert_eq!(ast("a || b && c || d;"), "(|| (|| a (&& b c)) d)");
    assert_eq!(ast("a && b === c;"), "(&& a (=== b c))");
}

/// The comma operator (clj-quickjs-61): `Expression : Expression ,
/// AssignmentExpression`, only where the grammar has Expression.
#[test]
fn comma_operator() {
    use SyntaxErrorKind::{Invalid, UnexpectedToken};
    // Left-associative, loosest of all operators.
    assert_eq!(ast("a, b, c;"), "(, (, a b) c)");
    assert_eq!(ast("x = 1, 2;"), "(, (= x 1) 2)");
    assert_eq!(ast("a ? b : c, d || e;"), "(, (? a b c) (|| d e))");
    // Arguments are AssignmentExpressions: parentheses make one argument.
    assert_eq!(ast("f((a, b));"), "(call f (, a b))");
    assert_eq!(ast("f(a, b);"), "(call f a b)");
    assert_eq!(ast("x = (a, b);"), "(= x (, a b))");
    assert_eq!(ast("(a, b)(c);"), "(call (, a b) c)");
    // Declarators are AssignmentExpressions too.
    assert_eq!(ast("let a = 1, b = (2, 3);"), "(let (a 1) (b (, 2 3)))");
    // Every Expression position: tests and `return`/`throw` operands.
    assert_eq!(
        ast("if (a, b) c, d; else while (e, f) g;"),
        "(if (, a b) (, c d) (while (, e f) g))"
    );
    assert_eq!(
        ast("function f() { return a, b; }"),
        "(function f () (return (, a b)))"
    );
    assert_eq!(ast("throw a, b;"), "(throw (, a b))");
    // The arrow cover grammar: a parenthesized list before `=>` is still a
    // parameter list; an arrow ends at its concise body's `,`.
    assert_eq!(ast("(a, b) => a, b;"), "(, (arrow (a b) => a) b)");
    assert_eq!(ast("a, b => c;"), "(, a (arrow (b) => c))");
    // A line break after a block body does not end the statement before
    // `,`, which continues the enclosing Expression.
    assert_eq!(ast("() => {}\n, 1"), "(, (arrow ()) 1)");
    assert_eq!(ast("((a, b) => a)(1, 2);"), "(call (arrow (a b) => a) 1 2)");
    rejects_with(
        "f = 1 + (a, b) => a;",
        UnexpectedToken,
        1,
        16,
        "parenthesized",
    );
    // `? :` branches are AssignmentExpressions.
    rejects("a ? b, c : d;", UnexpectedToken, 1, 6);
    // No empty operand, and a sequence is not an assignment target.
    rejects("a, ;", UnexpectedToken, 1, 4);
    rejects("(a, );", UnexpectedToken, 1, 5);
    rejects("(a, b) = 1;", Invalid, 1, 1);
    // The span covers the whole list (and the parentheses, if any).
    for (src, start, end) in [("x; (a,  b);", 3, 10), ("x; a , b;", 3, 8)] {
        let script = parse_script(src).unwrap();
        let StmtKind::Expr(e) = &script.body[1].kind else {
            panic!("{script:?}")
        };
        assert_eq!((e.span.start, e.span.end), (start, end), "{src:?}");
    }
}

#[test]
fn throw_and_try_statements() {
    assert_eq!(ast("throw a + 1;"), "(throw (+ a 1))");
    assert_eq!(ast("if (a) throw b;"), "(if a (throw b))");
    assert_eq!(
        ast("try { a; } catch (e) { b; }"),
        "(try (block a) (catch e b))"
    );
    assert_eq!(ast("try {} catch {}"), "(try (block) (catch))");
    assert_eq!(
        ast("try { a; } finally { b; }"),
        "(try (block a) (finally b))"
    );
    assert_eq!(
        ast("try {} catch (e) { let x; } finally {}"),
        "(try (block) (catch e (let (x))) (finally))"
    );
    // ASI after `throw expr`, but no line break after `throw`.
    assert_eq!(ast("throw a\nb;"), "(throw a) b");
    rejects("throw\na;", SyntaxErrorKind::Invalid, 1, 1);
    rejects("throw;", SyntaxErrorKind::UnexpectedToken, 1, 6);
    rejects("try {}", SyntaxErrorKind::UnexpectedEnd, 1, 7);
    rejects("try {} x;", SyntaxErrorKind::UnexpectedToken, 1, 8);
    rejects("try x; catch {}", SyntaxErrorKind::UnexpectedToken, 1, 5);
    rejects(
        "try {} catch (e) { const e = 1; }",
        SyntaxErrorKind::Invalid,
        1,
        26,
    );
    rejects(
        "try {} catch (e) { function e() {} }",
        SyntaxErrorKind::Invalid,
        1,
        29,
    );
    rejects(
        "try {} catch (eval) {}",
        SyntaxErrorKind::ReservedWord,
        1,
        15,
    );
    rejects(
        "try {} catch (e, f) {}",
        SyntaxErrorKind::UnexpectedToken,
        1,
        16,
    );
}

/// Unlabelled `break` and `continue` (clj-quickjs-78). The script
/// `while (1) break` newline `x` is adapted from the first statement of
/// `test_labels2` in `research/quickjs/tests/test_language.js` (QuickJS,
/// revision 7d1c13c5d8f8; see `docs/architecture.md` for the attribution):
/// its label is dropped (the labelled form is in `labelled_statements`) and
/// the next line is `x`. The other cases are written for this project.
///
/// QuickJS is Copyright (c) 2017-2021 Fabrice Bellard and Charlie Gordon,
/// released under the MIT license: Permission is hereby granted, free of
/// charge, to any person obtaining a copy of this software and associated
/// documentation files (the "Software"), to deal in the Software without
/// restriction, including without limitation the rights to use, copy, modify,
/// merge, publish, distribute, sublicense, and/or sell copies of the
/// Software, and to permit persons to whom the Software is furnished to do
/// so, subject to the following conditions: The above copyright notice and
/// this permission notice shall be included in all copies or substantial
/// portions of the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT
/// WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE
/// WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
/// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
/// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF
/// CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH
/// THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
#[test]
fn break_and_continue() {
    use SyntaxErrorKind::{Invalid, UnexpectedToken};
    assert_eq!(ast("while (x) break;"), "(while x (break))");
    assert_eq!(
        ast("while (x) { continue; }"),
        "(while x (block (continue)))"
    );
    // Restricted production: a line break after the keyword inserts `;`.
    assert_eq!(ast("while (1) break\nx"), "(while 1 (break)) x");
    assert_eq!(ast("while (1) continue\nx;"), "(while 1 (continue)) x");
    assert_eq!(ast("while (x) { break }"), "(while x (block (break)))");
    // Anywhere inside the loop body of the same function.
    assert_eq!(
        ast("while (a) { if (b) break; else { continue; } }"),
        "(while a (block (if b (break) (block (continue)))))"
    );
    assert_eq!(
        ast("while (a) try { break; } catch (e) { continue; } finally {}"),
        "(while a (try (block (break)) (catch e (continue)) (finally)))"
    );
    assert_eq!(
        ast("while (a) { function f() { while (b) break; } continue; }"),
        "(while a (block (function f () (while b (break))) (continue)))"
    );
    // Early errors: outside a loop, or in a function nested in the loop.
    rejects_with("break;", Invalid, 1, 1, "inside a loop");
    rejects_with("{ continue; }", Invalid, 1, 3, "inside a loop");
    rejects_with("if (x) break;", Invalid, 1, 8, "inside a loop");
    rejects_with(
        "while (x) { function f() { break; } }",
        Invalid,
        1,
        28,
        "inside a loop",
    );
    rejects("while (x) { (() => { continue; })(); }", Invalid, 1, 22);
    rejects("while (x) { g = function () { break; }; }", Invalid, 1, 31);
    rejects("function f() { break; }", Invalid, 1, 16);
    rejects("while (x) {} break;", Invalid, 1, 14);
    // A label after the keyword on the same line must be declared
    // (clj-quickjs-82; see `labelled_statements`).
    rejects_with("while (x) break l;", Invalid, 1, 17, "undefined label `l`");
    rejects_with("while (x) continue l;", Invalid, 1, 20, "undefined label");
    rejects("while (x) break 1;", UnexpectedToken, 1, 17);
    rejects("while (x) break if;", UnexpectedToken, 1, 17);
}

/// Labelled statements and labelled `break`/`continue` (clj-quickjs-82).
/// `while (1) L: break` newline `y` is adapted from the first statement of
/// `test_labels2` in `research/quickjs/tests/test_language.js` (the next
/// line is `y` instead of `var i = 0`), and the script compiled at the end
/// is `test_cyclic_labels` from `research/quickjs/tests/test_loop.js`
/// (QuickJS, revision 7d1c13c5d8f8; see `docs/architecture.md` for the
/// attribution). The other cases are written for this project.
///
/// QuickJS is Copyright (c) 2017-2021 Fabrice Bellard and Charlie Gordon,
/// released under the MIT license: Permission is hereby granted, free of
/// charge, to any person obtaining a copy of this software and associated
/// documentation files (the "Software"), to deal in the Software without
/// restriction, including without limitation the rights to use, copy, modify,
/// merge, publish, distribute, sublicense, and/or sell copies of the
/// Software, and to permit persons to whom the Software is furnished to do
/// so, subject to the following conditions: The above copyright notice and
/// this permission notice shall be included in all copies or substantial
/// portions of the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT
/// WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE
/// WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
/// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
/// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF
/// CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH
/// THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
#[test]
fn labelled_statements() {
    use SyntaxErrorKind::{Invalid, ReservedWord, UnexpectedToken};
    assert_eq!(ast("L: x;"), "(label L x)");
    assert_eq!(ast("L: { break L; }"), "(label L (block (break L)))");
    assert_eq!(
        ast("L1: L2: while (x) continue L1;"),
        "(label L1 (label L2 (while x (continue L1))))"
    );
    // ASI after `break`: the label belongs to the statement before it; `y`
    // starts a new statement.
    assert_eq!(
        ast("while (1) L: break\ny"),
        "(while 1 (label L (break))) y"
    );
    assert_eq!(
        ast("while (1) L: break\nL"),
        "(while 1 (label L (break))) L"
    );
    assert_eq!(
        ast("if (a) L: ; else M: ;"),
        "(if a (label L (empty)) (label M (empty)))"
    );
    // Bodies of loops, and labels on other statements.
    assert_eq!(
        ast("do x: { break x; } while (0);"),
        "(do (label x (block (break x))) 0)"
    );
    assert_eq!(
        ast("L: do { continue L; } while (a);"),
        "(label L (do (block (continue L)) a))"
    );
    assert_eq!(
        ast("for (;;) L: { if (a) break; continue; }"),
        "(for _ _ _ (label L (block (if a (break)) (continue))))"
    );
    assert_eq!(
        ast("L: try { break L; } finally { L2: if (a) break L; }"),
        "(label L (try (block (break L)) (finally (label L2 (if a (break L))))))"
    );
    assert_eq!(
        ast("L: while (a) { M: while (b) { continue L; } }"),
        "(label L (while a (block (label M (while b (block (continue L)))))))"
    );
    // A label is not a binding: the same name may be both.
    assert_eq!(
        ast("let x; x: { break x; } x;"),
        "(let (x)) (label x (block (break x))) x"
    );
    // Sibling statements may reuse a label (`test_cyclic_labels`).
    assert_eq!(
        ast("L: ; L: { break L; }"),
        "(label L (empty)) (label L (block (break L)))"
    );
    // The label of `break`/`continue` must be on the same line.
    assert_eq!(
        ast("L: while (1) { break\nL; }"),
        "(label L (while 1 (block (break) L)))"
    );
    // `a ? b : c` is not a label.
    assert_eq!(ast("a ? b : c;"), "(? a b c)");

    // Early errors, at the label reference or the repeated label.
    rejects_with("break L;", Invalid, 1, 7, "undefined label `L`");
    rejects_with("L: ; break L;", Invalid, 1, 12, "undefined label");
    rejects_with(
        "while (x) { continue L; }",
        Invalid,
        1,
        22,
        "undefined label",
    );
    rejects_with(
        "L: { continue L; }",
        Invalid,
        1,
        15,
        "label of an enclosing loop",
    );
    rejects_with(
        "L: if (x) while (y) continue L;",
        Invalid,
        1,
        30,
        "label of an enclosing loop",
    );
    rejects_with(
        "L: { while (x) continue L; }",
        Invalid,
        1,
        25,
        "label of an enclosing loop",
    );
    rejects_with("L: L: ;", Invalid, 1, 4, "already declared");
    rejects_with("L: { L: ; }", Invalid, 1, 6, "already declared");
    rejects_with(
        "L: while (x) { M: L: ; }",
        Invalid,
        1,
        19,
        "already declared",
    );
    // Labels do not cross function boundaries.
    rejects_with(
        "L: { (function () { break L; }); }",
        Invalid,
        1,
        27,
        "undefined label",
    );
    rejects(
        "L: while (x) { (() => { continue L; })(); }",
        Invalid,
        1,
        34,
    );
    // ... and so a function may reuse an enclosing function's label.
    assert_eq!(
        ast("L: { function f() { L: ; } }"),
        "(label L (block (function f () (label L (empty)))))"
    );
    // A labelled declaration (strict mode).
    rejects_with("L: function f() {}", Invalid, 1, 4, "labelled statement");
    rejects_with("L: M: let x;", Invalid, 1, 7, "labelled statement");
    rejects_with(
        "if (a) L: const c = 1;",
        Invalid,
        1,
        11,
        "labelled statement",
    );
    // Unlabelled `break` and `continue` still need a loop.
    rejects_with("L: { break; }", Invalid, 1, 6, "inside a loop");
    // Reserved words are not labels.
    rejects("yield: x;", ReservedWord, 1, 1);
    rejects("if: x;", UnexpectedToken, 1, 3);
    rejects("while (x) break yield;", UnexpectedToken, 1, 17);

    // `test_cyclic_labels` compiles (calling it would loop forever).
    super::compile_script(
        "cyclic",
        "function test_cyclic_labels()
         {
             /* just check that it compiles without a crash */
             for (;;) {
                 l: break l;
                 l: break l;
                 l: break l;
             }
         }",
    )
    .unwrap_or_else(|e| panic!("test_cyclic_labels: {e}"));
}

/// `for (init; test; update)` with an empty, expression or `let`/`const`
/// head (clj-quickjs-81). `_` renders an absent part.
#[test]
fn for_statements() {
    use SyntaxErrorKind::{Invalid, ReservedWord, UnexpectedEnd, UnexpectedToken, Unsupported};
    assert_eq!(ast("for (;;) x;"), "(for _ _ _ x)");
    assert_eq!(
        ast("for (i = 0; i < 3; i = i + 1) {}"),
        "(for (= i 0) (< i 3) (= i (+ i 1)) (block))"
    );
    assert_eq!(
        ast("for (let i = 0, j; ; ) {}"),
        "(for (let (i 0) (j)) _ _ (block))"
    );
    assert_eq!(
        ast("for (const k = 1; k < 0; ) {}"),
        "(for (const (k 1)) (< k 0) _ (block))"
    );
    assert_eq!(
        ast("for (a, b; c, d; e, f) ;"),
        "(for (, a b) (, c d) (, e f) (empty))"
    );
    assert_eq!(
        ast("for (i++; ; i--) ;"),
        "(for (post++ i) _ (post-- i) (empty))"
    );
    assert_eq!(
        ast("for (;;) { if (a) break; continue; }"),
        "(for _ _ _ (block (if a (break)) (continue)))"
    );
    assert_eq!(
        ast("for (;;) for (;;) break;"),
        "(for _ _ _ (for _ _ _ (break)))"
    );
    assert_eq!(ast("if (a) for (;;) x; else y;"), "(if a (for _ _ _ x) y)");
    assert_eq!(ast("for (;;) x\ny"), "(for _ _ _ x) y");
    // A function in the head starts a new `break` context.
    rejects("for (;; function () { break; }) ;", Invalid, 1, 23);
    rejects("for (;;) ; break;", Invalid, 1, 12);
    // `const` needs an initializer; a declaration cannot be the body.
    rejects_with("for (const k; ;) {}", Invalid, 1, 12, "missing initializer");
    rejects_with("for (;;) let x;", Invalid, 1, 10, "cannot be the body");
    rejects("for (;;) function f() {}", Invalid, 1, 10);
    // For-in, for-of and `var` heads are separate features.
    rejects_with("for (x of y) {}", Unsupported, 1, 1, "`for`-`of`");
    rejects_with("for (x in y) {}", Unsupported, 1, 1, "`for`-`in`");
    rejects_with("for (let x of y) {}", Unsupported, 1, 1, "`for`-`of`");
    rejects_with("for (const x in y) {}", Unsupported, 1, 1, "`for`-`in`");
    rejects_with("for (a.b of y) {}", Unsupported, 1, 1, "`for`-`of`");
    rejects_with("for (var i = 0; ;) {}", Unsupported, 1, 6, "`var`");
    rejects("for (let [a] = b; ;) {}", Unsupported, 1, 10);
    // `let` is a keyword (strict mode); the head is parenthesized and has
    // exactly two `;`.
    rejects("for (let let = 1; ;) {}", ReservedWord, 1, 10);
    rejects("for (;) {}", UnexpectedToken, 1, 7);
    rejects("for (x) {}", UnexpectedToken, 1, 7);
    rejects("for ;; {}", UnexpectedToken, 1, 5);
    rejects("for (;;", UnexpectedEnd, 1, 8);
    rejects("for (let i = 0, ; ;) {}", UnexpectedToken, 1, 17);
    // No semicolon is inserted inside the head.
    rejects("for (a\nb;;) {}", UnexpectedToken, 2, 1);
    rejects("for (;\n) {}", UnexpectedToken, 2, 1);
}

/// `do`-`while` (clj-quickjs-80), including its ASI rule: the `;` after
/// `while (...)` is inserted even without a line break.
#[test]
fn do_while_statements() {
    use SyntaxErrorKind::{Invalid, UnexpectedEnd, UnexpectedToken};
    assert_eq!(ast("do x; while (y);"), "(do x y)");
    assert_eq!(ast("do { } while (y)"), "(do (block) y)");
    assert_eq!(ast("do x; while (0) y;"), "(do x 0) y");
    assert_eq!(ast("do x; while (0)\ny"), "(do x 0) y");
    assert_eq!(ast("do ; while (a, b);"), "(do (empty) (, a b))");
    assert_eq!(
        ast("do { if (a) break; continue; } while (b);"),
        "(do (block (if a (break)) (continue)) b)"
    );
    assert_eq!(ast("do do x; while (a) while (b)"), "(do (do x a) b)");
    assert_eq!(ast("{ do x; while (y) }"), "(block (do x y))");
    assert_eq!(ast("if (a) do x; while (b) else y;"), "(if a (do x b) y)");
    // A declaration cannot be the body.
    rejects_with(
        "do let a = 1; while (0);",
        Invalid,
        1,
        4,
        "cannot be the body",
    );
    rejects("do function f() {} while (0);", Invalid, 1, 4);
    // `break`/`continue` belong to the body only, not to the test.
    rejects("do x; while (y); break;", Invalid, 1, 18);
    rejects("do x; while (function () { break; });", Invalid, 1, 28);
    // The `while` part is required.
    rejects("do x; y;", UnexpectedToken, 1, 7);
    rejects("do x;", UnexpectedEnd, 1, 6);
    rejects("do x; while y;", UnexpectedToken, 1, 13);
}

/// `switch` statements (clj-quickjs-83).
#[test]
fn switch_statements() {
    use SyntaxErrorKind::{Invalid, UnexpectedEnd, UnexpectedToken};
    assert_eq!(ast("switch (x) {}"), "(switch x)");
    assert_eq!(ast("switch (a, b) { }"), "(switch (, a b))");
    // Clauses with and without bodies; a body is a statement list.
    assert_eq!(
        ast("switch (x) { case 1: case 2: a; b; case 3: { c; } }"),
        "(switch x (case 1) (case 2 a b) (case 3 (block c)))"
    );
    assert_eq!(
        ast("switch (x) { case 1: let a = 1; function f() {} const c = 2; }"),
        "(switch x (case 1 (let (a 1)) (function f ()) (const (c 2))))"
    );
    assert_eq!(
        ast("switch (x) { case y ? 1 : 2: z; }"),
        "(switch x (case (? y 1 2) z))"
    );
    // `default` first, in the middle and last.
    assert_eq!(
        ast("switch (x) { default: a; case 1: b; }"),
        "(switch x (default a) (case 1 b))"
    );
    assert_eq!(
        ast("switch (x) { case 1: default: case 2: c; }"),
        "(switch x (case 1) (default) (case 2 c))"
    );
    assert_eq!(
        ast("switch (x) { case 1: break; default: }"),
        "(switch x (case 1 (break)) (default))"
    );
    // `break` inside a clause; `continue` targets an enclosing loop; labels.
    assert_eq!(
        ast("while (a) switch (x) { case 1: continue; default: break; }"),
        "(while a (switch x (case 1 (continue)) (default (break))))"
    );
    assert_eq!(
        ast("L: switch (x) { case 1: if (y) break L; }"),
        "(label L (switch x (case 1 (if y (break L)))))"
    );
    assert_eq!(
        ast("switch (x) { case 1: while (y) break; break; }"),
        "(switch x (case 1 (while y (break)) (break)))"
    );
    // Early errors.
    rejects_with(
        "switch (x) { default: default: }",
        Invalid,
        1,
        23,
        "only one `default`",
    );
    rejects_with(
        "switch (x) { case 1: continue; }",
        Invalid,
        1,
        22,
        "inside a loop",
    );
    rejects("L: switch (x) { case 1: continue L; }", Invalid, 1, 34);
    rejects(
        "switch (x) { case 1: function f() { break; } }",
        Invalid,
        1,
        37,
    );
    rejects("switch (x) {} break;", Invalid, 1, 15);
    // Malformed case blocks.
    rejects("switch (x) { a; }", UnexpectedToken, 1, 14);
    rejects("switch (x) { case 1 }", UnexpectedToken, 1, 21);
    rejects("switch (x) { default }", UnexpectedToken, 1, 22);
    rejects("switch x {}", UnexpectedToken, 1, 8);
    rejects("switch (x) { case 1:", UnexpectedEnd, 1, 21);
    // The case block is one scope: a redeclaration across clauses is a
    // compile-time early error at the second declaration.
    let e = super::compile_script("s", "switch (x) { case 1: let a; case 2: let a; }")
        .expect_err("redeclaration across clauses");
    assert_eq!((e.kind, e.line, e.column), (Invalid, 1, 41), "{e}");
    let e = super::compile_script(
        "s",
        "switch (x) { case 1: function f() {} default: const f = 1; }",
    )
    .expect_err("redeclaration across clauses");
    assert_eq!((e.kind, e.line, e.column), (Invalid, 1, 53), "{e}");
    // Sibling switches and nested blocks may reuse names.
    super::compile_script(
        "s",
        "switch (x) { case 1: let a; } switch (x) { case 1: { let a; } let a; }",
    )
    .expect("no redeclaration");
}

#[test]
fn unary_binds_tighter_than_binary_and_looser_than_calls() {
    assert_eq!(ast("-x * y;"), "(* (neg x) y)");
    assert_eq!(ast("!a + -b * +c;"), "(+ (! a) (* (neg b) (pos c)))");
    assert_eq!(ast("!!f(x).y;"), "(! (! (. (call f x) y)))");
    assert_eq!(ast("- -1;"), "(neg (neg 1))");
    assert_eq!(ast("a - -b;"), "(- a (neg b))");
    // `typeof` has the precedence of `!` (clj-quickjs-69); parentheses
    // leave its operand an identifier.
    assert_eq!(
        ast("typeof x === \"number\";"),
        "(=== (typeof x) \"number\")"
    );
    assert_eq!(
        ast("typeof typeof f(x).y;"),
        "(typeof (typeof (. (call f x) y)))"
    );
    assert_eq!(ast("typeof -x + 1;"), "(+ (typeof (neg x)) 1)");
    assert_eq!(ast("typeof (x);"), "(typeof x)");
    assert_eq!(ast("!typeof x;"), "(! (typeof x))");
    // So does `void` (clj-quickjs-133).
    assert_eq!(ast("void 0 === x;"), "(=== (void 0) x)");
    assert_eq!(ast("void void f(x).y;"), "(void (void (. (call f x) y)))");
    assert_eq!(ast("void -x + 1;"), "(+ (void (neg x)) 1)");
    assert_eq!(ast("typeof void x;"), "(typeof (void x))");
    assert_eq!(ast("void a++;"), "(void (post++ a))");
}

/// Bitwise and shift operators and `~` (clj-quickjs-62), at ECMAScript
/// precedence: `||` < `&&` < `|` < `^` < `&` < equality < relational <
/// shift < additive < multiplicative, all left-associative.
#[test]
fn bitwise_and_shift_precedence() {
    assert_eq!(ast("a | b ^ c & d;"), "(| a (^ b (& c d)))");
    assert_eq!(ast("a & b | c ^ d;"), "(| (& a b) (^ c d))");
    assert_eq!(ast("a & b == c;"), "(& a (== b c))");
    assert_eq!(ast("a << b + c;"), "(<< a (+ b c))");
    assert_eq!(ast("a < b << c;"), "(< a (<< b c))");
    assert_eq!(ast("a || b | c;"), "(|| a (| b c))");
    assert_eq!(ast("a && b ^ c;"), "(&& a (^ b c))");
    assert_eq!(ast("~-a;"), "(~ (neg a))");
    assert_eq!(ast("-~a;"), "(neg (~ a))");
    assert_eq!(ast("!~a;"), "(! (~ a))");
    assert_eq!(ast("~~a * ~b;"), "(* (~ (~ a)) (~ b))");
    // Left-associative within a level.
    assert_eq!(ast("a | b | c;"), "(| (| a b) c)");
    assert_eq!(ast("a ^ b ^ c;"), "(^ (^ a b) c)");
    assert_eq!(ast("a & b & c;"), "(& (& a b) c)");
    assert_eq!(ast("a << b >> c >>> d;"), "(>>> (>> (<< a b) c) d)");
    assert_eq!(ast("a >>> b * c - d;"), "(>>> a (- (* b c) d))");
    // The compound forms are assignments (clj-quickjs-84).
    assert_eq!(ast("a |= 1;"), "(|= a 1)");
    assert_eq!(ast("a &= 1;"), "(&= a 1)");
    assert_eq!(ast("a ^= 1;"), "(^= a 1)");
    assert_eq!(ast("a <<= 1;"), "(<<= a 1)");
    assert_eq!(ast("a >>= 1;"), "(>>= a 1)");
    assert_eq!(ast("a >>>= b | c;"), "(>>>= a (| b c))");
    // `??` stays rejected at the operator.
    use SyntaxErrorKind::Unsupported;
    rejects_with("a | b ?? c;", Unsupported, 1, 7, "??");
}

/// The exponentiation operator (clj-quickjs-63): tighter than `*`,
/// right-associative, a UnaryExpression on the right, and never an
/// unparenthesized prefix-operator expression on the left.
#[test]
fn exponent_operator_syntax() {
    assert_eq!(ast("2 ** 3 ** 2;"), "(** 2 (** 3 2))");
    assert_eq!(ast("a * b ** c;"), "(* a (** b c))");
    assert_eq!(ast("a ** b * c;"), "(* (** a b) c)");
    assert_eq!(ast("-(a ** b);"), "(neg (** a b))");
    assert_eq!(ast("(-a) ** b;"), "(** (neg a) b)");
    assert_eq!(ast("a ** -b;"), "(** a (neg b))");
    assert_eq!(ast("a ** f(b).c ** d;"), "(** a (** (. (call f b) c) d))");
    assert_eq!(ast("a | b ** c;"), "(| a (** b c))");
    assert_eq!(ast("x => x ** 2;"), ast("x => (x ** 2);"));
    // A prefix-operator expression may not be the base: `Invalid` at `**`.
    use SyntaxErrorKind::Invalid;
    rejects_with("-a ** b;", Invalid, 1, 4, "parenthesize");
    rejects_with("!a ** b;", Invalid, 1, 4, "parenthesize");
    rejects_with("+a ** b;", Invalid, 1, 4, "parenthesize");
    rejects_with("~a ** b;", Invalid, 1, 4, "parenthesize");
    rejects_with("- -a ** b;", Invalid, 1, 6, "parenthesize");
    rejects_with("-2 ** 2;", Invalid, 1, 4, "parenthesize");
    // Also in the exponent: `a ** -b ** c` has the unary `-b` as a base.
    rejects_with("a ** -b ** c;", Invalid, 1, 9, "parenthesize");
    // `typeof` is a prefix operator like the others (clj-quickjs-69).
    rejects_with("typeof a ** b;", Invalid, 1, 10, "parenthesize");
    assert_eq!(ast("typeof (a ** b);"), "(typeof (** a b))");
    // So is `void` (clj-quickjs-133).
    rejects_with("void a ** b;", Invalid, 1, 8, "parenthesize");
    assert_eq!(ast("void (a ** b);"), "(void (** a b))");
    // `**=` is a compound assignment (clj-quickjs-84).
    assert_eq!(ast("a **= b ** 2;"), "(**= a (** b 2))");
}

/// Prefix and postfix `++`/`--` on identifier bindings (clj-quickjs-64).
#[test]
fn update_expressions() {
    assert_eq!(ast("a++;"), "(post++ a)");
    assert_eq!(ast("++a;"), "(pre++ a)");
    assert_eq!(ast("a--;"), "(post-- a)");
    assert_eq!(ast("--a;"), "(pre-- a)");
    // Two `-` tokens are two negations; `--` is one decrement.
    assert_eq!(ast("- -a;"), "(neg (neg a))");
    assert_eq!(ast("-(-a);"), "(neg (neg a))");
    assert_eq!(ast("- --a;"), "(neg (pre-- a))");
    // No line break before a postfix operator: it starts a new statement.
    assert_eq!(ast("a\n++b"), "a (pre++ b)");
    assert_eq!(ast("x = a\n++b"), "(= x a) (pre++ b)");
    assert_eq!(ast("a\n--\nb"), "a (pre-- b)");
    // `<!--` is not a comment: `a < !(--b)`.
    assert_eq!(ast("a <!-- b"), "(< a (! (pre-- b)))");
    // Precedence: postfix binds tighter than prefix and binary operators.
    assert_eq!(ast("-a++;"), "(neg (post++ a))");
    assert_eq!(ast("a++ + ++b;"), "(+ (post++ a) (pre++ b))");
    assert_eq!(ast("a+++b;"), "(+ (post++ a) b)");
    assert_eq!(ast("a---b;"), "(- (post-- a) b)");
    assert_eq!(ast("x = y++;"), "(= x (post++ y))");
    // An UpdateExpression may be the base of `**`.
    assert_eq!(ast("++a ** 2;"), "(** (pre++ a) 2)");
    assert_eq!(ast("a-- ** 2;"), "(** (post-- a) 2)");
    // A parenthesized identifier is a valid target.
    assert_eq!(ast("(a)++;"), "(post++ a)");
    assert_eq!(ast("--((a));"), "(pre-- a)");
    assert_eq!(ast("f(a++, --b);"), "(call f (post++ a) (pre-- b))");
    // The span covers the operator and the operand.
    let script = parse_script("  (b)--;").unwrap();
    let StmtKind::Expr(e) = &script.body[0].kind else {
        panic!("not an expression statement")
    };
    assert_eq!((e.span.start, e.span.end), (2, 7));
    let ExprKind::Update {
        target, op_span, ..
    } = &e.kind
    else {
        panic!("not an update")
    };
    assert_eq!((op_span.start, op_span.end), (5, 7));
    assert_eq!((target.span.start, target.span.end), (2, 5));

    use SyntaxErrorKind::{Invalid, ReservedWord, UnexpectedToken};
    // The operand must be an identifier binding (early errors).
    rejects_with("1++;", Invalid, 1, 1, "increment/decrement operand");
    rejects_with("--1;", Invalid, 1, 3, "increment/decrement operand");
    rejects_with("(a + b)++;", Invalid, 1, 1, "increment/decrement operand");
    rejects_with("f()++;", Invalid, 1, 1, "increment/decrement operand");
    rejects_with("++a++;", Invalid, 1, 3, "increment/decrement operand");
    rejects_with("++-a;", Invalid, 1, 3, "increment/decrement operand");
    rejects_with("++ ++a;", Invalid, 1, 4, "increment/decrement operand");
    rejects_with("(a, b)++;", Invalid, 1, 1, "increment/decrement operand");
    rejects_with("(a = 1)++;", Invalid, 1, 1, "increment/decrement operand");
    // Reserved constants, like assignment.
    rejects_with("undefined++;", ReservedWord, 1, 1, "undefined");
    rejects_with("--NaN;", ReservedWord, 1, 3, "NaN");
    rejects_with("(Infinity)--;", ReservedWord, 1, 1, "Infinity");
    // The reserved intrinsic binding's intrinsics, like assignment.
    rejects_with("__rt.count++;", ReservedWord, 1, 1, "__rt");
    rejects_with("++__rt.count;", ReservedWord, 1, 3, "__rt");
    rejects("__rt[0]++;", ReservedWord, 1, 5);
    // Property targets (clj-quickjs-76), possibly parenthesized.
    assert_eq!(ast("a.b++;"), "(post++ (. a b))");
    assert_eq!(ast("--a.b;"), "(pre-- (. a b))");
    assert_eq!(ast("a[0]++;"), "(post++ ([] a 0))");
    assert_eq!(ast("++(a[i++]);"), "(pre++ ([] a (post++ i)))");
    assert_eq!(ast("(a.b).c--;"), "(post-- (. (. a b) c))");
    assert_eq!(
        ast("f().x++ + -a[0]--;"),
        "(+ (post++ (. (call f) x)) (neg (post-- ([] a 0))))"
    );
    let script = parse_script("  (o[k])++;").unwrap();
    let StmtKind::Expr(e) = &script.body[0].kind else {
        panic!("not an expression statement")
    };
    assert_eq!((e.span.start, e.span.end), (2, 10));
    let ExprKind::UpdateProperty {
        target, op_span, ..
    } = &e.kind
    else {
        panic!("not a property update")
    };
    assert_eq!((op_span.start, op_span.end), (8, 10));
    assert_eq!((target.span.start, target.span.end), (2, 8));
    rejects_with("a.b++ = 1;", Invalid, 1, 1, "assignment target");
    rejects("a[0]++.b;", UnexpectedToken, 1, 7);
    // The result of a postfix update is not a callee or member object, and
    // two operands need an operator between them.
    rejects("a++();", UnexpectedToken, 1, 4);
    rejects("a++.b;", UnexpectedToken, 1, 4);
    rejects("a++ ++;", UnexpectedToken, 1, 5);
    rejects("a--b", UnexpectedToken, 1, 4);
    // The unary-operand restriction of `**` still applies around updates.
    rejects_with("-a++ ** 2;", Invalid, 1, 6, "parenthesize");
    // `typeof` applies to the whole update (clj-quickjs-69).
    assert_eq!(ast("typeof a++;"), "(typeof (post++ a))");
    assert_eq!(ast("typeof --a;"), "(typeof (pre-- a))");
    // An update is not a compound-assignment target (clj-quickjs-84).
    rejects_with("a++ += 1;", Invalid, 1, 1, "invalid assignment target");
}

#[test]
fn conditional_and_assignment_are_right_associative() {
    assert_eq!(ast("a = b = c;"), "(= a (= b c))");
    assert_eq!(ast("a ? b : c ? d : e;"), "(? a b (? c d e))");
    assert_eq!(ast("x = a || b ? c : d;"), "(= x (? (|| a b) c d))");
    assert_eq!(ast("a ? b = 1 : c = 2;"), "(? a (= b 1) (= c 2))");
    assert_eq!(ast("(a) = 1;"), "(= a 1)");
    assert_eq!(ast("a ?.5 : 1;"), "(? a 0.5 1)");
}

/// Arithmetic, bitwise, shift and `**` compound assignment on identifier
/// bindings (clj-quickjs-84): the target rules of `=`, right-associative,
/// and the logical assignments stay unsupported.
#[test]
fn compound_assignment() {
    use SyntaxErrorKind::{Invalid, ReservedWord, Unsupported};
    assert_eq!(ast("a += 1;"), "(+= a 1)");
    assert_eq!(ast("a -= b *= 2;"), "(-= a (*= b 2))");
    assert_eq!(ast("(a) %= 3;"), "(%= a 3)");
    assert_eq!(ast("a /= b = c;"), "(/= a (= b c))");
    assert_eq!(ast("a = b += c;"), "(= a (+= b c))");
    assert_eq!(ast("x = a ? b : c += 1;"), "(= x (? a b (+= c 1)))");
    assert_eq!(ast("a += b, c;"), "(, (+= a b) c)");
    assert_eq!(ast("f(a += 1);"), "(call f (+= a 1))");
    // The target and the `op=` token are recorded separately.
    let script = parse_script("((a)) *= 2;").unwrap();
    let StmtKind::Expr(e) = &script.body[0].kind else {
        panic!("not an expression statement")
    };
    assert_eq!((e.span.start, e.span.end), (0, 10));
    let &ExprKind::CompoundAssign {
        op,
        ref target,
        op_span,
        ..
    } = &e.kind
    else {
        panic!("not a compound assignment")
    };
    assert_eq!(op, BinaryOp::Mul);
    assert_eq!((target.span.start, target.span.end), (0, 5));
    assert_eq!((op_span.start, op_span.end), (6, 8));
    // Invalid targets.
    rejects_with("1 += 2;", Invalid, 1, 1, "invalid assignment target");
    rejects_with("f() += 1;", Invalid, 1, 1, "invalid assignment target");
    rejects_with("a + b -= 1;", Invalid, 1, 1, "invalid assignment target");
    rejects_with("(a, b) += 1;", Invalid, 1, 1, "invalid assignment target");
    rejects_with("undefined += 1;", ReservedWord, 1, 1, "undefined");
    rejects_with("NaN -= 1;", ReservedWord, 1, 1, "NaN");
    rejects_with("__rt.count += 1;", ReservedWord, 1, 1, "__rt");
    rejects_with("__rt[0] += 1;", ReservedWord, 1, 5, "computed access");
    // Property targets (clj-quickjs-260), possibly parenthesized.
    assert_eq!(ast("a.b += 1;"), "(+= (. a b) 1)");
    assert_eq!(ast("a[0] **= 2;"), "(**= ([] a 0) 2)");
    assert_eq!(ast("(a.b) -= c.d *= 2;"), "(-= (. a b) (*= (. c d) 2))");
    assert_eq!(
        ast("f().x >>>= a[i] = 1;"),
        "(>>>= (. (call f) x) (= ([] a i) 1))"
    );
    assert_eq!(
        ast("a.b.c[d] |= 1, e;"),
        "(, (|= ([] (. (. a b) c) d) 1) e)"
    );
    let script = parse_script("  (o[k]) <<= 1;").unwrap();
    let StmtKind::Expr(e) = &script.body[0].kind else {
        panic!("not an expression statement")
    };
    assert_eq!((e.span.start, e.span.end), (2, 14));
    let &ExprKind::CompoundAssignProperty {
        op,
        ref target,
        op_span,
        ..
    } = &e.kind
    else {
        panic!("not a property compound assignment")
    };
    assert_eq!(op, BinaryOp::Shl);
    assert_eq!((target.span.start, target.span.end), (2, 8));
    assert_eq!((op_span.start, op_span.end), (9, 12));
    rejects_with("a.b++ += 1;", Invalid, 1, 1, "invalid assignment target");
    rejects_with("a.b &&= 1;", Unsupported, 1, 5, "&&=");
    rejects_with("a[0] ??= 1;", Unsupported, 1, 6, "??=");
    // Logical assignment is not supported.
    rejects_with("a &&= 1;", Unsupported, 1, 3, "&&=");
    rejects_with("a ||= 1;", Unsupported, 1, 3, "||=");
    rejects_with("a ??= 1;", Unsupported, 1, 3, "??=");
}

#[test]
fn calls_and_member_chains() {
    assert_eq!(ast("f();"), "(call f)");
    assert_eq!(ast("f(1, g(2), h)(3);"), "(call (call f 1 (call g 2) h) 3)");
    assert_eq!(
        ast("a.b.c(d)(e).f;"),
        "(. (call (call (. (. a b) c) d) e) f)"
    );
    assert_eq!(ast("f(a, b,);"), "(call f a b)");
    assert_eq!(ast("__rt.need(1);"), "(call (. __rt need) 1)");
    // Reserved words are valid property names.
    assert_eq!(ast("x.if.new.default;"), "(. (. (. x if) new) default)");
    assert_eq!(
        ast("(function (x) { return x; })(1);"),
        "(call (function - (x) (return x)) 1)"
    );
    assert_eq!(ast("1..toString;"), "(. 1 toString)");
}

/// Object literals and property assignment (clj-quickjs-114). The
/// `{if: 1, get: 2, ...}` keys and the `{x, get}` shorthands follow
/// `test_object_literal` (line 378) of
/// `research/quickjs/tests/test_language.js` (QuickJS, revision
/// 7d1c13c5d8f8; see `docs/architecture.md` for the attribution and the
/// notice on `number_literal_member_access` below), checked as parse trees;
/// the other cases are written for this project.
#[test]
fn object_literals() {
    use SyntaxErrorKind::{Invalid, ReservedWord, UnexpectedToken, Unsupported};
    assert_eq!(ast("({});"), "(object)");
    assert_eq!(
        ast(r#"({a: 1, "b": 2, 3: 4,});"#),
        r#"(object (a 1) ("b" 2) (3 4))"#
    );
    assert_eq!(
        ast("({if: 1, get: 2, set: 3, async: 4, static: 5});"),
        "(object (if 1) (get 2) (set 3) (async 4) (static 5))"
    );
    assert_eq!(
        ast("({x, get});"),
        "(object (shorthand x x) (shorthand get get))"
    );
    // Values are assignment expressions; literals nest.
    assert_eq!(
        ast("({a: b = 1, c: {d: e}.d, f: (1, 2)});"),
        "(object (a (= b 1)) (c (. (object (d e)) d)) (f (, 1 2)))"
    );
    // `__proto__` (clj-quickjs-121): a colon form sets the prototype, a
    // shorthand is an ordinary property (so it may repeat a colon form),
    // and the AST keeps both as written.
    let proto = ast(r#"({__proto__: p, __proto__, "x": 1});"#);
    assert_eq!(
        proto,
        "(object (__proto__ p) (shorthand __proto__ __proto__) (\"x\" 1))"
    );
    let script = parse_script(r#"({"__proto__": p, __proto__});"#).unwrap();
    let Some(StmtKind::Expr(e)) = script.body.first().map(|s| &s.kind) else {
        panic!("not an expression statement");
    };
    let ExprKind::Object(properties) = &e.kind else {
        panic!("not an object literal");
    };
    let sets: Vec<bool> = properties.iter().map(|p| p.sets_prototype()).collect();
    assert_eq!(sets, [true, false]);
    // At statement start `{` is a block, so a literal is parenthesized.
    assert_eq!(ast("{ a: 1 }"), "(block (label a 1))");
    assert_eq!(ast("x = {a: {b: 2}};"), "(= x (object (a (object (b 2)))))");

    // A key is an IdentifierName: a reserved word written with escapes is
    // an ordinary key, first or later, but not a shorthand or a label.
    assert_eq!(
        ast(r"({\u0069f: 1, n\u{65}w: 2});"),
        "(object (if 1) (new 2))"
    );
    rejects_with(r"({\u0069f});", ReservedWord, 1, 3, "shorthand");
    rejects(r"{ \u0069f: 1 }", ReservedWord, 1, 3);
    rejects_with("({if});", ReservedWord, 1, 3, "shorthand");
    rejects_with("({1});", UnexpectedToken, 1, 3, "needs a value");
    rejects_with(r#"({"s"});"#, UnexpectedToken, 1, 3, "needs a value");
    rejects_with("({a = 1});", Invalid, 1, 3, "destructuring");
    rejects_with("({[k]: 1});", Unsupported, 1, 3, "computed property names");
    rejects_with("({...o});", Unsupported, 1, 3, "spread properties");
    rejects_with("({m() {}});", Unsupported, 1, 3, "methods");
    rejects_with("({get x() {}});", Unsupported, 1, 3, "getters and setters");
    rejects_with("({set x(v) {}});", Unsupported, 1, 3, "getters and setters");
    rejects_with("({async m() {}});", Unsupported, 1, 3, "async methods");
    rejects_with("({*g() {}});", Unsupported, 1, 3, "generator methods");
    // `__proto__: value` sets the prototype (clj-quickjs-121): only one per
    // literal, whether written as a name or a string.
    rejects_with(
        "({__proto__: 1, __proto__: 2});",
        Invalid,
        1,
        17,
        "duplicate `__proto__`",
    );
    rejects_with(
        r#"({__proto__: 1, "__proto__": 2});"#,
        Invalid,
        1,
        17,
        "duplicate",
    );
    rejects_with(
        r#"({"__proto__": 1, a: 2, __proto__: 3});"#,
        Invalid,
        1,
        25,
        "duplicate",
    );
    rejects_with("({a: 1 b: 2});", UnexpectedToken, 1, 8, "");
    rejects_with("({a});;({b: 1", SyntaxErrorKind::UnexpectedEnd, 1, 14, "");
}

/// `o.name = v` (clj-quickjs-114), compound assignment (clj-quickjs-260)
/// and `++`/`--` (clj-quickjs-76) of a property; any assignment to the
/// reserved intrinsic binding stays rejected.
#[test]
fn property_assignment() {
    use SyntaxErrorKind::{ReservedWord, Unsupported};
    assert_eq!(ast("a.b = 1;"), "(= (. a b) 1)");
    assert_eq!(ast("(a.b) = 1;"), "(= (. a b) 1)");
    assert_eq!(ast("a.b.c = d.e = f;"), "(= (. (. a b) c) (= (. d e) f))");
    assert_eq!(ast("f().x = 1;"), "(= (. (call f) x) 1)");
    // Compound assignment (clj-quickjs-260) and `++`/`--` (clj-quickjs-76)
    // on a property.
    assert_eq!(ast("a.b += 1;"), "(+= (. a b) 1)");
    assert_eq!(ast("a.b++;"), "(post++ (. a b))");
    rejects_with("a.b ||= 1;", Unsupported, 1, 5, "||=");
    rejects_with("__rt.x = 1;", ReservedWord, 1, 1, "cannot be assigned");
}

/// Array literals (clj-quickjs-118): elisions are holes, one final comma
/// adds nothing, elements are assignment expressions; spread elements and
/// array assignment patterns stay rejected.
#[test]
fn array_literals() {
    use SyntaxErrorKind::{Invalid, UnexpectedEnd, UnexpectedToken, Unsupported};
    assert_eq!(ast("[];"), "(array)");
    assert_eq!(ast("[1, 2,];"), "(array 1 2)");
    assert_eq!(ast("[ , ];"), "(array <hole>)");
    assert_eq!(ast("[1, , 3];"), "(array 1 <hole> 3)");
    assert_eq!(ast("[,,];"), "(array <hole> <hole>)");
    assert_eq!(ast("[1,,];"), "(array 1 <hole>)");
    assert_eq!(ast("[, 1];"), "(array <hole> 1)");
    assert_eq!(
        ast("x = [a = 1, [b], {c: [d, (1, 2)]}, f => f];"),
        "(= x (array (= a 1) (array b) (object (c (array d (, 1 2)))) (arrow (f) => f)))"
    );
    // A literal is a member object and a callee like any primary.
    assert_eq!(ast("[1].length;"), "(. (array 1) length)");
    assert_eq!(ast("[f][0](1);"), "(call ([] (array f) 0) 1)");
    // Spans cover the brackets.
    let src = "x = [1, , 3,];";
    let script = parse_script(src).unwrap();
    let StmtKind::Expr(e) = &script.body[0].kind else {
        panic!("not an expression statement")
    };
    let ExprKind::Assign { value, .. } = &e.kind else {
        panic!("not an assignment")
    };
    assert_eq!(value.span.text(src), "[1, , 3,]");

    rejects_with("x = [...a];", Unsupported, 1, 6, "spread elements");
    rejects_with("x = [1, ...a];", Unsupported, 1, 9, "spread elements");
    rejects_with("[a] = b;", Unsupported, 1, 1, "destructuring assignment");
    rejects_with("[a, b] = [b, a];", Unsupported, 1, 1, "destructuring");
    rejects_with("let [a] = b;", Unsupported, 1, 5, "destructuring");
    rejects("[a] += 1;", Invalid, 1, 1);
    rejects("x = [1 2];", UnexpectedToken, 1, 8);
    rejects("x = [1,", UnexpectedEnd, 1, 8);
}

/// Computed member access `a[k]` and assignment `a[k] = v` in member and
/// call chains (clj-quickjs-115; parsed since clj-quickjs-118). A computed
/// callee is a method call; compound assignment and `++`/`--` of a computed
/// property parse (clj-quickjs-260, -76), and `__rt` cannot be indexed.
#[test]
fn computed_member_access() {
    use SyntaxErrorKind::{ReservedWord, Unsupported};
    assert_eq!(ast("a[b];"), "([] a b)");
    assert_eq!(ast("a[b][c];"), "([] ([] a b) c)");
    assert_eq!(ast("a.b[c].d;"), "(. ([] (. a b) c) d)");
    assert_eq!(ast("a[b, c];"), "([] a (, b c))");
    assert_eq!(ast("a[b](c);"), "(call ([] a b) c)");
    assert_eq!(ast("a.b[c](d);"), "(call ([] (. a b) c) d)");
    assert_eq!(ast("a[b].c = d;"), "(= (. ([] a b) c) d)");
    assert_eq!(ast("a[b] = c;"), "(= ([] a b) c)");
    assert_eq!(
        ast("(a[b]) = c = d[e] = f;"),
        "(= ([] a b) (= c (= ([] d e) f)))"
    );
    // No line-terminator restriction: `a` newline `[0]` is one member.
    assert_eq!(ast("a\n[0]"), "([] a 0)");
    // Compound assignment to a computed property (clj-quickjs-260).
    assert_eq!(ast("a[0] += 1;"), "(+= ([] a 0) 1)");
    rejects_with("a[0] ??= 1;", Unsupported, 1, 6, "??=");
    rejects_with("__rt[0];", ReservedWord, 1, 5, "computed access");
    rejects_with("__rt[0] = 1;", ReservedWord, 1, 5, "computed access");
}

/// A number literal followed by a member access (clj-quickjs-48). The
/// `0.1.a`, `0x1.a`, `0b1.a`, `0o1.a`, `01.a` and `0.a` cases are those of
/// `test_number_literals` (line 667) in
/// `research/quickjs/tests/test_language.js` (QuickJS, revision
/// 7d1c13c5d8f8; see `docs/architecture.md` for the attribution), checked
/// as parse trees instead of evaluated to `undefined`; `01.a` is checked in
/// sloppy code, where legacy octal literals are lexed (clj-quickjs-51), and
/// is rejected in strict code. The other cases are written for this
/// project.
///
/// QuickJS is Copyright (c) 2017-2021 Fabrice Bellard and Charlie Gordon,
/// released under the MIT license: Permission is hereby granted, free of
/// charge, to any person obtaining a copy of this software and associated
/// documentation files (the "Software"), to deal in the Software without
/// restriction, including without limitation the rights to use, copy, modify,
/// merge, publish, distribute, sublicense, and/or sell copies of the
/// Software, and to permit persons to whom the Software is furnished to do
/// so, subject to the following conditions: The above copyright notice and
/// this permission notice shall be included in all copies or substantial
/// portions of the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT
/// WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE
/// WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
/// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
/// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF
/// CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH
/// THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
#[test]
fn number_literal_member_access() {
    use SyntaxErrorKind::{InvalidToken, UnexpectedToken};
    // The fraction ends at the second `.`, which starts a member access.
    assert_eq!(ast("0.1.a;"), "(. 0.1 a)");
    // An integer in another radix has no fraction, so its `.` is a member
    // access.
    assert_eq!(ast("0x1.a;"), "(. 1 a)");
    assert_eq!(ast("0b1.a;"), "(. 1 a)");
    assert_eq!(ast("0o1.a;"), "(. 1 a)");
    assert_eq!(ast("0x1a.b;"), "(. 26 b)");
    assert_eq!(ast(".5.a;"), "(. 0.5 a)");
    assert_eq!(ast("1.5e3.a;"), "(. 1500 a)");
    assert_eq!(ast("1..a.b;"), "(. (. 1 a) b)");
    // `0.` is a complete literal, so an identifier may not follow it.
    rejects("0.a", InvalidToken, 1, 3);
    rejects("0.a;", InvalidToken, 1, 3);
    // `.2` after `0.1` is a second number literal, not a property name.
    rejects("0.1.2", UnexpectedToken, 1, 4);
    // Upstream's `01.a` is the legacy octal integer 1 then a member access
    // in sloppy code (the upstream test script is sloppy); legacy octal is
    // an early error in strict code.
    assert_eq!(sloppy_ast("01.a;"), "(. 1 a)");
    rejects("01.a", InvalidToken, 1, 1);
}

// ----- positive: statements --------------------------------------------------

#[test]
fn declarations() {
    assert_eq!(ast("let a;"), "(let (a))");
    assert_eq!(ast("let a, b = 1, c;"), "(let (a) (b 1) (c))");
    assert_eq!(ast("const d = 2, e = d;"), "(const (d 2) (e d))");
    assert_eq!(ast("let $_x9 = async;"), "(let ($_x9 async))");
}

/// `var` statements (clj-quickjs-94) in every statement position, and the
/// early errors of a `var` that a lexical declaration would shadow.
#[test]
fn var_statements() {
    use SyntaxErrorKind::{Invalid, UnexpectedToken};
    assert_eq!(ast("var a, b = 1;"), "(var (a) (b 1))");
    assert_eq!(ast("if (x) var y = 1;"), "(if x (var (y 1)))");
    assert_eq!(
        ast("while (x) var y; L: var z = (1, 2)\nvar w"),
        "(while x (var (y))) (label L (var (z (, 1 2)))) (var (w))"
    );
    assert_eq!(ast("{ var a = b = c; }"), "(block (var (a (= b c))))");
    rejects("var;", UnexpectedToken, 1, 4);
    rejects("var a b;", UnexpectedToken, 1, 7);
    rejects("var a = 1, ;", UnexpectedToken, 1, 12);
    rejects_with("var let = 1;", SyntaxErrorKind::ReservedWord, 1, 5, "let");
    // A `var` names the function's binding: a `let`/`const` (or block
    // function) of the same name in its own scope or an enclosing block is
    // an early error at the `var` name.
    let compile_err = |src: &str| {
        let e = super::compile_script("v", src).expect_err(src);
        (e.kind, e.line, e.column)
    };
    assert_eq!(compile_err("let a; var a;"), (Invalid, 1, 12));
    assert_eq!(compile_err("var a; const a = 1;"), (Invalid, 1, 5));
    assert_eq!(compile_err("{ let a; { var a; } }"), (Invalid, 1, 16));
    assert_eq!(compile_err("{ { var a; } let a; }"), (Invalid, 1, 9));
    assert_eq!(compile_err("{ function a() {} var a; }"), (Invalid, 1, 23));
    assert_eq!(
        compile_err("for (let i = 0; ;) { var i; }"),
        (Invalid, 1, 26)
    );
    assert_eq!(
        compile_err("switch (x) { case 1: let a; case 2: var a; }"),
        (Invalid, 1, 41)
    );
    assert_eq!(
        compile_err("function f() { if (x) { const k = 1; var k; } }"),
        (Invalid, 1, 42)
    );
    assert_eq!(
        compile_err("function f(a) { let a; var a; }"),
        (Invalid, 1, 21)
    );
    // Not conflicts: a sibling block, a nested function, a parameter, a
    // repeated `var`, a top-level function declaration and (ECMAScript
    // Annex B) a catch parameter.
    for src in [
        "{ let a; } var a;",
        "let a; function f() { var a; }",
        "function f(a) { var a; { var a; } }",
        "var a; var a; { var a; }",
        "function a() {} var a;",
        "var a; function a() {}",
        "try {} catch (e) { var e = 1; }",
    ] {
        super::compile_script("v", src).unwrap_or_else(|e| panic!("{src:?}: {e}"));
    }
}

#[test]
fn functions() {
    assert_eq!(
        ast("function add(a, b) { return a + b; }"),
        "(function add (a b) (return (+ a b)))"
    );
    assert_eq!(ast("function f() {}"), "(function f ())");
    assert_eq!(
        ast("function f(a,) { return; }"),
        "(function f (a) (return))"
    );
    assert_eq!(
        ast("let f = function named(n) { function inner() { return n; } return inner; };"),
        "(let (f (function named (n) (function inner () (return n)) (return inner))))"
    );
    // A function body may contain if/while/blocks with return.
    assert_eq!(
        ast("function f(x) { while (x) { if (x) { return 1; } } }"),
        "(function f (x) (while x (block (if x (block (return 1))))))"
    );
}

#[test]
fn this_expressions() {
    use SyntaxErrorKind::{Invalid, Unsupported};
    // clj-quickjs-116: `this` is a primary expression inside non-arrow
    // functions, declarations and expressions alike, nested blocks,
    // default parameters and functions nested in arrows included.
    assert_eq!(
        ast("function f() { return this; }"),
        "(function f () (return this))"
    );
    assert_eq!(
        ast("let g = function () { return this.x; };"),
        "(let (g (function - () (return (. this x)))))"
    );
    assert_eq!(
        ast("function f() { if (this) { return this.m(this); } }"),
        "(function f () (if this (block (return (call (. this m) this)))))"
    );
    assert_eq!(
        ast("function f(a = this) { return a; }"),
        "(function f (a=this) (return a))"
    );
    assert_eq!(
        ast("() => function () { return this; }"),
        "(arrow () => (function - () (return this)))"
    );
    // Still rejected with the existing kind and message at the script top
    // level (clj-quickjs-104) and inside arrows (lexical `this`,
    // clj-quickjs-98), even an arrow nested in a function.
    rejects_with("this;", Unsupported, 1, 1, "`this`");
    rejects_with("x = this.y;", Unsupported, 1, 5, "`this`");
    rejects_with("() => this", Unsupported, 1, 7, "`this`");
    rejects_with("(a = this) => a", Unsupported, 1, 6, "`this`");
    rejects_with(
        "function f() { return () => this; }",
        Unsupported,
        1,
        29,
        "`this`",
    );
    rejects_with(
        "function f() { let g = (a) => { return this; }; }",
        Unsupported,
        1,
        40,
        "`this`",
    );
    // After an arrow or function ends, the enclosing context applies again.
    rejects_with("function f() {} this;", Unsupported, 1, 17, "`this`");
    assert_eq!(
        ast("function f() { let g = () => 1; return this; }"),
        "(function f () (let (g (arrow () => 1))) (return this))"
    );
    // `this` is not an assignment target.
    rejects("function f() { this = 1; }", Invalid, 1, 16);
}

#[test]
fn new_expressions() {
    use SyntaxErrorKind::{Invalid, UnexpectedToken, Unsupported};
    // clj-quickjs-71: `new MemberExpression Arguments?`.
    assert_eq!(ast("new F;"), "(new F)");
    assert_eq!(ast("new F(1, 2);"), "(new F 1 2)");
    assert_eq!(ast("new a.b(c);"), "(new (. a b) c)");
    // The first argument list belongs to `new`; the next one calls the
    // constructed value.
    assert_eq!(ast("new F()();"), "(call (new F))");
    assert_eq!(ast("new new F()();"), "(new (new F))");
    assert_eq!(ast("new new F;"), "(new (new F))");
    // Member accesses before the arguments belong to the constructor,
    // after them to the result.
    assert_eq!(ast("new a[b].c(1).d;"), "(. (new (. ([] a b) c) 1) d)");
    assert_eq!(ast("new F().x;"), "(. (new F) x)");
    assert_eq!(ast("new F.x;"), "(new (. F x))");
    assert_eq!(ast("(new F).x;"), "(. (new F) x)");
    assert_eq!(ast("new (f())(1);"), "(new (call f) 1)");
    assert_eq!(ast("new (a.b);"), "(new (. a b))");
    // `new` binds tighter than unary and binary operators and postfix `++`
    // applies to the whole expression (and is then rejected as a target).
    assert_eq!(ast("!new F() + 1;"), "(+ (! (new F)) 1)");
    assert_eq!(
        ast("x = new F(1) === new F(1);"),
        "(= x (=== (new F 1) (new F 1)))"
    );
    assert_eq!(
        ast("function F(x) { this.x = x; } const b = new F(2);"),
        "(function F (x) (= (. this x) x)) (const (b (new F 2)))"
    );
    rejects("new F()++;", Invalid, 1, 1);
    rejects("new F() = 1;", Invalid, 1, 1);
    // `new.target` (clj-quickjs-99) stays unsupported; a missing
    // constructor or an unclosed argument list is a syntax error.
    rejects_with(
        "function f() { return new.target; }",
        Unsupported,
        1,
        23,
        "`new.target`",
    );
    rejects("new;", UnexpectedToken, 1, 4);
    rejects("new F(1;", UnexpectedToken, 1, 8);
}

#[test]
fn default_parameters() {
    use SyntaxErrorKind::{Invalid, UnexpectedToken};
    // clj-quickjs-100: `name = AssignmentExpression`, in function
    // declarations, expressions and arrow parameter lists.
    assert_eq!(ast("function f(a, b = 1) {}"), "(function f (a b=1))");
    assert_eq!(ast("function f(a, b = a) {}"), "(function f (a b=a))");
    assert_eq!(ast("(a = 1) => a"), "(arrow (a=1) => a)");
    assert_eq!(ast("(a, b = 1, c,) => {}"), "(arrow (a b=1 c))");
    assert_eq!(
        ast("x = function (a = 1, b = a ? 2 : 3) {};"),
        "(= x (function - (a=1 b=(? a 2 3))))"
    );
    // A default is an AssignmentExpression: an arrow (even one with its
    // own defaults), an assignment, but not the comma operator.
    assert_eq!(
        ast("function f(g = (a = 1) => a, h = x = 2) {}"),
        "(function f (g=(arrow (a=1) => a) h=(= x 2)))"
    );
    assert_eq!(
        ast("((a = (b = 1) => b) => a)"),
        "(arrow (a=(arrow (b=1) => b)) => a)"
    );
    // Its span is its own.
    let src = "function f(a, b = a + 1) {}";
    let script = parse_script(src).unwrap();
    let StmtKind::FunctionDecl(f) = &script.body[0].kind else {
        panic!("{script:?}")
    };
    assert!(f.defaults[0].is_none());
    assert_eq!(f.defaults[1].as_ref().unwrap().span.text(src), "a + 1");
    // Duplicate names are an early error with or without a default.
    rejects_with(
        "function f(a = 1, a) {}",
        Invalid,
        1,
        19,
        "duplicate parameter",
    );
    rejects_with("(a, b = 1, b) => 1", Invalid, 1, 12, "duplicate parameter");
    rejects("function f(a =) {}", UnexpectedToken, 1, 15);
    rejects("function f(a = 1, 2) {}", UnexpectedToken, 1, 19);
    rejects("function f(a = (1, 2) {}", UnexpectedToken, 1, 23);
    // An object literal key spelled as an escaped reserved word does not
    // hide the arrow from the cover-grammar lookahead.
    assert_eq!(
        ast("(a = {\\u0069f: 1, \\u0069n: 2}) => a"),
        "(arrow (a=(object (if 1) (in 2))) => a)"
    );
    // A `"use strict"` directive is an early error in a function with a
    // default, strict or sloppy, ...
    for src in [
        "function f(a = 1) { \"use strict\"; }",
        "x = function (a, b = 1) { 'use strict'; };",
        "(a = 1) => { \"use strict\"; }",
        "function f(a = 1) { \"other\"; \"use strict\"; }",
    ] {
        let e = err(src);
        assert_eq!(e.kind, Invalid, "{src}: {e}");
        assert_eq!(e.span.text(src).trim_matches(['"', '\'']), "use strict");
        sloppy_rejects(src, Invalid, e.line, e.column);
    }
    // ... but not an escaped one, a parenthesized one, one after the
    // prologue, or one in a nested function without defaults.
    for src in [
        "function f(a = 1) { \"use\\x20strict\"; }",
        "function f(a = 1) { (\"use strict\"); }",
        "function f(a = 1) { 1; \"use strict\"; }",
        "function f(a = 1) { function g() { \"use strict\"; } }",
    ] {
        ast(src);
        sloppy_script(src);
    }
}

#[test]
fn nested_blocks_and_control_flow() {
    assert_eq!(
        ast("{ { let x = 1; { const y = x; } } }"),
        "(block (block (let (x 1)) (block (const (y x)))))"
    );
    assert_eq!(ast("{ function f() {} }"), "(block (function f ()))");
    assert_eq!(
        ast("if (a) b; else if (c) d; else { e; }"),
        "(if a b (if c d (block e)))"
    );
    // Dangling else binds to the nearest if.
    assert_eq!(ast("if (a) if (b) c; else d;"), "(if a (if b c d))");
    assert_eq!(
        ast("while (i < 3) i = i + 1;"),
        "(while (< i 3) (= i (+ i 1)))"
    );
    assert_eq!(ast(";;"), "(empty) (empty)");
    assert_eq!(ast("if (a) ; else ;"), "(if a (empty) (empty))");
    assert_eq!(ast(""), "");
    assert_eq!(ast("  // only a comment\n/* and a block */"), "");
}

#[test]
fn literals() {
    assert_eq!(
        ast("true; false; null; undefined; NaN; Infinity; -Infinity;"),
        "true false null undefined NaN inf (neg inf)"
    );
    assert_eq!(
        ast("0; 42; 1.5; .5; 5.; 1e3; 1E-3; 2.5e+2; 0x1F; 0XfF; 0o17; 0b101;"),
        "0 42 1.5 0.5 5 1000 0.001 250 31 255 15 5"
    );
    assert_eq!(ast("1e400;"), "inf");
    assert_eq!(ast("'a\\'b' \n \"c\\\"d\";"), "\"a'b\" \"c\\\"d\"");
    assert_eq!(
        ast(r#"'\n\t\r\b\f\v\0\x41B\u{43}\u{1F600}😀\q\\';"#),
        "\"\\n\\t\\r\\u{8}\\u{c}\\u{b}\\0ABC😀😀q\\\\\""
    );
    // Line continuation (LF and CRLF) contributes nothing.
    assert_eq!(ast("'a\\\nb\\\r\nc';"), "\"abc\"");
    // Raw U+2028 / U+2029 are allowed in strings.
    assert_eq!(ast("'\u{2028}\u{2029}';"), "\"\\u{2028}\\u{2029}\"");
}

#[test]
fn unicode_string_escapes_decode_code_units() {
    // clj-quickjs-141: a string literal is a sequence of UTF-16 code units.
    // Each `\u` escape contributes its code unit, so two escapes that spell
    // a surrogate pair are the same string as the astral character itself.
    let src = "x = 'A\\uD83D\\uDE00\\u00E9z';";
    let script = parse_script(src).unwrap();
    let StmtKind::Expr(Expr {
        kind: ExprKind::Assign { value, .. },
        ..
    }) = &script.body[0].kind
    else {
        panic!()
    };
    assert_eq!(value.kind, ExprKind::String("A\u{1F600}\u{E9}z".into()));
    assert_eq!(value.span.text(src), "'A\\uD83D\\uDE00\\u00E9z'");
    // Lone surrogate escapes are kept as code units (formerly rejected with
    // `Unsupported`): a high surrogate followed by a non-surrogate, a lone
    // high or low surrogate in either escape form, and a low surrogate
    // before a high one (not a pair).
    let units = |src: &str| match parse_script(src).unwrap().body[0].kind {
        StmtKind::Expr(Expr {
            kind: ExprKind::String(ref s),
            ..
        }) => s.code_units().to_vec(),
        ref other => panic!("{src:?}: not a string literal: {other:?}"),
    };
    assert_eq!(units("'\\uD83DA'"), [0xD83D, 0x41]);
    assert_eq!(units("'\\uD800'"), [0xD800]);
    assert_eq!(units("'\\u{dfff}'"), [0xDFFF]);
    assert_eq!(units("'\\u{D800}x'"), [0xD800, 0x78]);
    assert_eq!(units("'\\uDC00\\uD800'"), [0xDC00, 0xD800]);
    // Braced surrogate escapes pair up too; U+10FFFF is DBFF DFFF.
    assert_eq!(units("'\\u{dbff}\\u{dfff}'"), [0xDBFF, 0xDFFF]);
    assert_eq!(units("'\\u{10ffff}'"), [0xDBFF, 0xDFFF]);
    assert_eq!(units("'\u{10FFFF}'"), [0xDBFF, 0xDFFF]);
    // The other escape groups give the same units as the raw characters.
    assert_eq!(units("'\\x61'"), units("'a'"));
    assert_eq!(units("'\\u{20ac}'"), [0x20AC]);
    assert_eq!(units("'\\u20AC'"), units("'\u{20AC}'"));
    assert_eq!(units("'\\0'"), [0]);
    assert_eq!(units("'\\xff\\u{0}'"), [0xFF, 0]);
    assert_eq!(units("''"), [0u16; 0]);
    // The AST rendering shows a lone surrogate exactly.
    assert_eq!(ast("'a\\uD800';"), "\"a\\u{d800}\"");
    // Identifiers still reject surrogates.
    rejects(
        "let \\uD835\\uDC9C = 1;",
        SyntaxErrorKind::InvalidToken,
        1,
        5,
    );
    rejects("let \\u{D800} = 1;", SyntaxErrorKind::InvalidToken, 1, 5);
}

#[test]
fn radix_literals_round_correctly() {
    let value = |src: &str| match parse_script(src).unwrap().body[0].kind {
        StmtKind::Expr(Expr {
            kind: ExprKind::Number(n),
            ..
        }) => n,
        _ => panic!("not a number"),
    };
    // 2^53 + 1 rounds to even (2^53); 2^53 + 3 rounds up to 2^53 + 4.
    assert_eq!(value("0x20000000000001;"), 9007199254740992.0);
    assert_eq!(value("0x20000000000003;"), 9007199254740996.0);
    // Beyond 128 bits: a sticky bit far below the rounding point must still
    // break the tie upward.
    let tie_up = format!("0x{}{};", "20000000000001", "0".repeat(40) + "1");
    assert_eq!(value(&tie_up), 9007199254740994.0f64 * 2f64.powi(164));
    assert_eq!(value(&format!("0x1{};", "0".repeat(300))), f64::INFINITY);
    assert_eq!(
        value("0b11111111111111111111111111111111111111111111111111111;"),
        9007199254740991.0
    );
    assert_eq!(value(&format!("{};", "9".repeat(400))), f64::INFINITY);
}

#[test]
fn automatic_semicolon_insertion() {
    assert_eq!(ast("let a = 1\nlet b = 2"), "(let (a 1)) (let (b 2))");
    assert_eq!(ast("a\nb"), "a b");
    assert_eq!(ast("{ a } b"), "(block a) b");
    assert_eq!(ast("x = 1 /* block\n comment */ y = 2"), "(= x 1) (= y 2)");
    assert_eq!(ast("x = 1 // line comment\ny"), "(= x 1) y");
    // A line break does not end a statement the next token continues.
    assert_eq!(ast("a\n(b)"), "(call a b)");
    assert_eq!(ast("a = b\n+c"), "(= a (+ b c))");
    assert_eq!(ast("a\n/ b / g"), "(/ (/ a b) g)");
    assert_eq!(ast("f\n.g()"), "(call (. f g))");
    // No `;` is needed after a function declaration.
    assert_eq!(ast("function f() {}\nf()"), "(function f ()) (call f)");
    assert_eq!(ast("function f() {} f()"), "(function f ()) (call f)");
    // Restricted production: `return` LineTerminator.
    assert_eq!(
        ast("function f(x) { return\nx }"),
        "(function f (x) (return) x)"
    );
    assert_eq!(
        ast("function f(x) { return x }"),
        "(function f (x) (return x))"
    );
    // CR, CRLF, U+2028 and U+2029 are line terminators.
    assert_eq!(ast("a\rb\r\nc\u{2028}d\u{2029}e"), "a b c d e");
    // Unicode whitespace is not a line terminator.
    rejects("a\u{3000}b", SyntaxErrorKind::UnexpectedToken, 1, 3);
    // `async` then a line break is just an identifier.
    assert_eq!(ast("async\nfunction f() {}"), "async (function f ())");
}

/// Arrow functions, and ASI after an arrow body: an arrow function is an
/// AssignmentExpression, so no operator, call or member access continues it.
/// The three two-statement scripts are the `test_parse_arrow_function`
/// cases of `research/quickjs/tests/test_language.js` (QuickJS, revision
/// 7d1c13c5d8f8; see `docs/architecture.md` for the attribution); the
/// other cases are written for this project.
///
/// QuickJS is Copyright (c) 2017-2021 Fabrice Bellard and Charlie Gordon,
/// released under the MIT license: Permission is hereby granted, free of
/// charge, to any person obtaining a copy of this software and associated
/// documentation files (the "Software"), to deal in the Software without
/// restriction, including without limitation the rights to use, copy, modify,
/// merge, publish, distribute, sublicense, and/or sell copies of the
/// Software, and to permit persons to whom the Software is furnished to do
/// so, subject to the following conditions: The above copyright notice and
/// this permission notice shall be included in all copies or substantial
/// portions of the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT
/// WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE
/// WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
/// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
/// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF
/// CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH
/// THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
#[test]
fn arrow_functions_and_asi() {
    use SyntaxErrorKind::{Invalid, ReservedWord, UnexpectedToken, Unsupported};
    assert_eq!(ast("x => x"), "(arrow (x) => x)");
    assert_eq!(ast("() => {}"), "(arrow ())");
    assert_eq!(ast("(a, b,) => { return a; }"), "(arrow (a b) (return a))");
    // Two statements each: neither `(` nor `+` can continue an arrow.
    assert_eq!(ast("() => {}\n() => {}"), "(arrow ()) (arrow ())");
    assert_eq!(ast("x => {}\n() => {}"), "(arrow (x)) (arrow ())");
    assert_eq!(ast("() => {}\n+1"), "(arrow ()) (pos 1)");
    // ... but they do continue a concise body, which is an expression.
    assert_eq!(ast("x => x\n(1)"), "(arrow (x) => (call x 1))");
    assert_eq!(ast("x => x\n+1"), "(arrow (x) => (+ x 1))");
    // On one line, the token after the arrow is a syntax error.
    rejects("() => {} + 1", UnexpectedToken, 1, 10);
    rejects("() => {}()", UnexpectedToken, 1, 9);
    rejects("() => {} ? a : b", UnexpectedToken, 1, 10);
    // `[no LineTerminator here]` before `=>`.
    rejects_with("x\n=> 1", UnexpectedToken, 2, 1, "line break");
    rejects_with("(a)\n=> 1", UnexpectedToken, 2, 1, "line break");
    rejects_with("f(1 + x\n=> 1)", UnexpectedToken, 2, 1, "line break");
    rejects_with("(a, a) => 1", Invalid, 1, 5, "duplicate parameter");
    // Arrows nest, are right-associative and are AssignmentExpressions:
    // arguments, initializers, conditional branches, assigned values.
    assert_eq!(
        ast("f = x => y => x + y;"),
        "(= f (arrow (x) => (arrow (y) => (+ x y))))"
    );
    assert_eq!(
        ast("let f = () => {}, g = (a) => a ? b => 1 : () => 2;"),
        "(let (f (arrow ())) (g (arrow (a) => (? a (arrow (b) => 1) (arrow () => 2)))))"
    );
    assert_eq!(
        ast("g(x => x, () => {},)"),
        "(call g (arrow (x) => x) (arrow ()))"
    );
    assert_eq!(ast("(() => 1)()"), "(call (arrow () => 1))");
    assert_eq!(ast("(x => x).y"), "(. (arrow (x) => x) y)");
    assert_eq!(ast("async => async"), "(arrow (async) => async)");
    assert_eq!(
        ast("x => { if (x) return 1; return function () { return 2; }; }"),
        "(arrow (x) (if x (return 1)) (return (function - () (return 2))))"
    );
    // A concise body that is an object-literal-looking `{` is a block.
    assert_eq!(ast("x => { x }"), "(arrow (x) x)");
    // An arrow as an operand, callee or member object needs parentheses.
    rejects_with("1 + x => x", UnexpectedToken, 1, 7, "parenthesized");
    rejects_with("!(a) => a", UnexpectedToken, 1, 6, "parenthesized");
    rejects_with("f(a) => a", UnexpectedToken, 1, 6, "parenthesized");
    rejects_with("a.b => a", UnexpectedToken, 1, 5, "parenthesized");
    // The parameter list holds plain binding identifiers only.
    rejects("(1) => 1", UnexpectedToken, 1, 2);
    rejects("((a)) => 1", UnexpectedToken, 1, 2);
    rejects("(a b) => 1", UnexpectedToken, 1, 4);
    rejects_with("(...a) => 1", Unsupported, 1, 2, "rest parameters");
    rejects_with("([a]) => 1", Unsupported, 1, 2, "destructuring");
    rejects_with("({ a }) => 1", Unsupported, 1, 2, "destructuring");
    rejects("(eval) => 1", ReservedWord, 1, 2);
    rejects("arguments => 1", ReservedWord, 1, 1);
    rejects("undefined => 1", ReservedWord, 1, 1);
    rejects("yield => 1", ReservedWord, 1, 1);
    // No `arguments` in the subset, inside arrows too; an arrow's lexical
    // `this` is not supported yet (clj-quickjs-98).
    rejects_with("() => this", Unsupported, 1, 7, "`this`");
    rejects_with(
        "() => { return arguments; }",
        Unsupported,
        1,
        16,
        "arguments",
    );
    // `return` is allowed in a block body, not in a concise body or after
    // the arrow.
    rejects("() => return", UnexpectedToken, 1, 7);
    rejects("() => {}\nreturn 1;", Invalid, 2, 1);
    // Async arrows stay unsupported.
    rejects_with("async () => {}\n() => {}", Unsupported, 1, 1, "async arrow");
    rejects_with("async x => {}\n() => {}", Unsupported, 1, 1, "async arrow");
}

#[test]
fn spans_cover_source_text() {
    let src = "let x = f(1,  2);\nfunction g(a) { return a; }\n";
    let script = parse_script(src).unwrap();
    assert_eq!(script.body[0].span.text(src), "let x = f(1,  2);");
    let StmtKind::Lexical(decl) = &script.body[0].kind else {
        panic!()
    };
    assert_eq!(decl.declarators[0].span.text(src), "x = f(1,  2)");
    assert_eq!(decl.declarators[0].name.span.text(src), "x");
    let init = decl.declarators[0].init.as_ref().unwrap();
    assert_eq!(init.span.text(src), "f(1,  2)");
    let ExprKind::Call { args, .. } = &init.kind else {
        panic!()
    };
    assert_eq!(args[1].span.text(src), "2");
    assert_eq!(script.body[1].span.text(src), "function g(a) { return a; }");
    let StmtKind::FunctionDecl(g) = &script.body[1].kind else {
        panic!()
    };
    assert_eq!(g.params[0].span.text(src), "a");
    assert_eq!(g.body[0].span.text(src), "return a;");
    assert_eq!(line_column(src, g.body[0].span.start), (2, 17));

    let src = "x = (a + b) * -c.d;";
    let StmtKind::Expr(e) = &parse_script(src).unwrap().body[0].kind else {
        panic!()
    };
    assert_eq!(e.span.text(src), "x = (a + b) * -c.d");
    let ExprKind::Assign { value, .. } = &e.kind else {
        panic!()
    };
    let ExprKind::Binary { left, right, .. } = &value.kind else {
        panic!()
    };
    assert_eq!(left.span.text(src), "(a + b)");
    assert_eq!(right.span.text(src), "-c.d");
}

#[test]
fn line_column_counts_lines_and_scalar_values() {
    let src = "ab\r\ncd\ré\u{2028}x\ny";
    assert_eq!(line_column(src, 0), (1, 1));
    assert_eq!(line_column(src, 1), (1, 2));
    assert_eq!(line_column(src, 4), (2, 1));
    assert_eq!(line_column(src, 7), (3, 1));
    // After the two-byte 'é' the column advances by one.
    assert_eq!(line_column(src, 9), (3, 2));
    assert_eq!(line_column(src, 12), (4, 1));
    assert_eq!(line_column(src, src.len()), (5, 2));
    // Offsets past the end or inside a character are clamped.
    assert_eq!(line_column(src, 8), (3, 1));
    assert_eq!(line_column(src, 1000), (5, 2));
    // A diagnostic after a multi-byte string reports a character column.
    rejects("'é€😀' @", SyntaxErrorKind::UnknownToken, 1, 7);
}

#[test]
fn hashbang_comment_only_at_start() {
    use SyntaxErrorKind::UnknownToken;
    // ECMAScript HashbangComment, skipped as QuickJS `skip_shebang` does:
    // only at byte 0, ending before the first line terminator, which still
    // separates lines.
    //
    // The hashbang line below is adapted from line 1 of
    // `research/quickjs/tests/test_std.js`, revision 7d1c13c5d8f8 (followed
    // here by `x`); see `docs/architecture.md`, "QuickJS references and
    // provenance".
    //
    // QuickJS is Copyright (c) 2017-2021 Fabrice Bellard and Charlie Gordon,
    // released under the MIT license: Permission is hereby granted, free of
    // charge, to any person obtaining a copy of this software and associated
    // documentation files (the "Software"), to deal in the Software without
    // restriction, including without limitation the rights to use, copy,
    // modify, merge, publish, distribute, sublicense, and/or sell copies of
    // the Software, and to permit persons to whom the Software is furnished
    // to do so, subject to the following conditions: The above copyright
    // notice and this permission notice shall be included in all copies or
    // substantial portions of the Software. THE SOFTWARE IS PROVIDED "AS IS",
    // WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT
    // LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
    // PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
    // HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN
    // AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
    // CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
    // SOFTWARE.
    let src = "#! (shebang test)\nx";
    assert_eq!(ast(src), "x");
    let script = parse_script(src).unwrap();
    assert_eq!(script.body[0].span.text(src), "x");
    assert_eq!(line_column(src, script.body[0].span.start), (2, 1));
    assert!(parse_script("#!x").unwrap().body.is_empty());
    assert!(parse_script("#!").unwrap().body.is_empty());
    // Every line terminator ends it, and the next line is line 2.
    for src in [
        "#!a\nb",
        "#!a\rb",
        "#!a\r\nb",
        "#!a\u{2028}b",
        "#!a\u{2029}b",
    ] {
        let script = parse_script(src).unwrap_or_else(|e| panic!("{src:?}: {e}"));
        assert_eq!(ast(src), "b", "{src:?}");
        assert_eq!(
            line_column(src, script.body[0].span.start),
            (2, 1),
            "{src:?}"
        );
    }
    // The comment runs to the line end; `/*` inside it opens nothing.
    assert_eq!(ast("#! /* \nb"), "b");
    // Anywhere else `#` is not a token.
    rejects(" #!x", UnknownToken, 1, 2);
    rejects("\n#!x", UnknownToken, 2, 1);
    rejects("1;\n#!x", UnknownToken, 2, 1);
    rejects("x = #!y", UnknownToken, 1, 5);
    rejects("// c\n#!x", UnknownToken, 2, 1);
}

#[test]
fn lexer_maximal_munch() {
    let kinds = |src: &str| {
        let mut lexer = Lexer::new(src);
        let mut out = Vec::new();
        loop {
            let t = lexer.next_token().unwrap();
            if t.kind == TokenKind::Eof {
                return out;
            }
            out.push(t.kind);
        }
    };
    use Punct::*;
    let p = TokenKind::Punct;
    assert_eq!(
        kinds(">>>= >>> >>= >> >= > === == => = !== != !"),
        vec![
            p(UShrAssign),
            p(UShr),
            p(ShrAssign),
            p(Shr),
            p(Ge),
            p(Gt),
            p(StrictEq),
            p(Eq),
            p(Arrow),
            p(Assign),
            p(StrictNotEq),
            p(NotEq),
            p(Bang)
        ]
    );
    assert_eq!(
        kinds("a===b"),
        vec![
            TokenKind::Identifier("a".into()),
            p(StrictEq),
            TokenKind::Identifier("b".into())
        ]
    );
    assert_eq!(kinds("?.5"), vec![p(Question), TokenKind::Number(0.5)]);
    assert_eq!(
        kinds("?.x"),
        vec![p(OptionalChain), TokenKind::Identifier("x".into())]
    );
    assert_eq!(
        kinds("a--b"),
        vec![
            TokenKind::Identifier("a".into()),
            p(MinusMinus),
            TokenKind::Identifier("b".into())
        ]
    );
    assert_eq!(kinds("..."), vec![p(Ellipsis)]);
    assert_eq!(kinds("**="), vec![p(StarStarAssign)]);
}

#[test]
fn unicode_identifiers() {
    use SyntaxErrorKind::{InvalidToken, ReservedWord, UnknownToken};
    let name = |src: &str| {
        let mut lexer = Lexer::new(src);
        let token = lexer
            .next_token()
            .unwrap_or_else(|e| panic!("{src:?}: {e}"));
        let next = lexer.next_token().unwrap();
        assert_eq!(next.kind, TokenKind::Eof, "{src:?} is one token");
        match token.kind {
            TokenKind::Identifier(name) => name,
            other => panic!("{src:?} lexed as {other:?}"),
        }
    };
    // ID_Start / ID_Continue code points, with `$`, `_`, ZWNJ and ZWJ.
    assert_eq!(name("Ãµ"), "\u{C3}\u{B5}");
    assert_eq!(name("π"), "π");
    assert_eq!(name("℮"), "℮"); // U+212E, Other_ID_Start
    assert_eq!(name("𝒜"), "\u{1D49C}"); // outside the BMP
    assert_eq!(name("é"), "é");
    assert_eq!(name("x·"), "x\u{B7}"); // U+00B7, Other_ID_Continue
    assert_eq!(name("e\u{301}"), "e\u{301}"); // combining mark, not normalized
    assert_eq!(name("a\u{200C}b\u{200D}c"), "a\u{200C}b\u{200D}c");
    assert_eq!(name("$_\u{2118}"), "$_\u{2118}");
    // Escapes decode to code points; the name is the decoded text.
    assert_eq!(name("x\\u{200C}y"), "x\u{200C}y");
    assert_eq!(name("a\\u0062c"), "abc");
    assert_eq!(name("\\u0061"), "a");
    assert_eq!(name("\\u{1D49C}\\u{00000062}"), "\u{1D49C}b");
    assert_eq!(name("x\\u0030"), "x0");
    assert_eq!(name("\\u0024\\u005F"), "$_");
    assert_eq!(ast("let a\\u0062c = 1; abc;"), ast("let abc = 1; abc;"));
    assert_eq!(ast("let é = 1;"), "(let (é 1))");
    assert_eq!(ast("let \\u0061 = 1;"), "(let (a 1))");
    assert_eq!(
        ast("f = (\\u0061, π) => \\u0061 + π;"),
        ast("f = (a, π) => a + π;")
    );
    // Characters that cannot start, or continue, an identifier.
    rejects("let € = 1;", UnknownToken, 1, 5);
    rejects("let x€ = 1;", UnknownToken, 1, 6);
    rejects("let \u{301}e = 1;", UnknownToken, 1, 5);
    rejects("let \u{2E2F} = 1;", UnknownToken, 1, 5); // Pattern_Syntax, not ID_Start
    rejects("3π", InvalidToken, 1, 2);
    rejects("3\\u0061", InvalidToken, 1, 2);
    // An escape must name a valid character for its place.
    rejects_with(
        "let \\u0030x = 1;",
        InvalidToken,
        1,
        5,
        "U+0030 cannot start",
    );
    rejects_with(
        "let a\\u0020b = 1;",
        InvalidToken,
        1,
        6,
        "U+0020 cannot appear",
    );
    rejects("let \\u20AC = 1;", InvalidToken, 1, 5);
    rejects("let a\\u{2E2F} = 1;", InvalidToken, 1, 6);
    rejects("let \\u200C = 1;", InvalidToken, 1, 5); // ZWNJ continues only
    rejects("let \\uD835\\uDC9C = 1;", InvalidToken, 1, 5); // no surrogates
    rejects("let \\u{110000} = 1;", InvalidToken, 1, 5);
    rejects("let \\u12 = 1;", InvalidToken, 1, 5);
    rejects("let \\u{} = 1;", InvalidToken, 1, 5);
    rejects("let \\x61 = 1;", InvalidToken, 1, 5);
    rejects("let a\\ = 1;", InvalidToken, 1, 6);
    rejects("\\", InvalidToken, 1, 1);
    // A reserved word written with escapes is never the keyword: it is
    // rejected where the word is reserved and an ordinary name after `.`.
    rejects_with(
        "l\\u0065t x = 1;",
        ReservedWord,
        1,
        1,
        "reserved word `let`",
    );
    rejects_with(
        "let \\u0069f = 1;",
        ReservedWord,
        1,
        5,
        "reserved word `if`",
    );
    rejects("x = tru\\u0065;", ReservedWord, 1, 5);
    rejects("\\u0061wait;", ReservedWord, 1, 1);
    rejects("function f(\\u{79}ield) {}", ReservedWord, 1, 12);
    assert_eq!(ast("x = a.l\\u0065t;"), ast("x = a.let;"));
    assert_eq!(ast("x = a.\\u0069f.b;"), ast("x = a.if.b;"));
    assert_eq!(ast("x = a. /* c */ \\u0069f;"), ast("x = a.if;"));
    // Only the token right after `.` is a property name.
    rejects("a.b; l\\u0065t;", ReservedWord, 1, 6);
    rejects("a.\\u0069f \\u0069f", ReservedWord, 1, 11);
    // Names compare by decoded code points: `__rt` with an escaped `_` is
    // the reserved intrinsic binding.
    rejects_with("__rt[0];", ReservedWord, 1, 5, "computed access");
    rejects_with("\\u005f_rt[0];", ReservedWord, 1, 10, "computed access");
}

#[test]
fn unicode_ident_tables_are_sorted_disjoint_ranges() {
    use super::unicode_ident::{ID_CONTINUE, ID_START};
    for table in [ID_START, ID_CONTINUE] {
        assert!(
            table
                .iter()
                .all(|&(lo, hi)| 0x80 <= lo && lo <= hi && hi <= 0x10FFFF)
        );
        assert!(table.iter().all(|&(lo, hi)| hi < 0xD800 || lo > 0xDFFF));
        // Adjacent ranges would have been merged by the generator.
        assert!(table.windows(2).all(|w| w[0].1 + 1 < w[1].0));
    }
    // ID_Start is a subset of ID_Continue (UAX #31).
    for &(lo, hi) in ID_START {
        assert!(
            ID_CONTINUE.iter().any(|&(l, h)| l <= lo && hi <= h),
            "{lo:X}..={hi:X}"
        );
    }
}

/// Untagged template literals (clj-quickjs-57): one token per part, the
/// part after a substitution starting at the `}` that closes it, cooked
/// and raw text, and substitutions that are full Expressions. The
/// `${a, b}` and nested-template forms follow `test_template` and
/// `test_template_skip` in `research/quickjs/tests/test_language.js`
/// (see `tests/js_source.rs` `template_literal_values`).
#[test]
fn template_literals() {
    use SyntaxErrorKind::*;
    assert_eq!(ast("`abc`"), r#"(template "abc")"#);
    assert_eq!(ast("`abc${b}d`"), r#"(template "abc" b "d")"#);
    assert_eq!(ast("`${a}${b}`"), r#"(template "" a "" b "")"#);
    assert_eq!(ast("``"), r#"(template "")"#);
    // Nested: a template in a substitution, at any depth.
    assert_eq!(
        ast("`${ `a${a}` }baz`"),
        r#"(template "" (template "a" a "") "baz")"#
    );
    assert_eq!(
        ast("`${`${`${x}`}`}`"),
        r#"(template "" (template "" (template "" x "") "") "")"#
    );
    // A substitution is an Expression: the comma operator, operators,
    // calls, members.
    assert_eq!(ast("`${a, b}`"), r#"(template "" (, a b) "")"#);
    assert_eq!(
        ast("`x${a + f(1)}y${o.p}z`"),
        r#"(template "x" (+ a (call f 1)) "y" (. o p) "z")"#
    );
    // Braces inside a substitution: only the `}` that closes it ends it.
    assert_eq!(
        ast("`${ {a: {b: 1}} }`"),
        r#"(template "" (object (a (object (b 1)))) "")"#
    );
    assert_eq!(
        ast("`${function () { return `}`; }}!`"),
        r#"(template "" (function - () (return (template "}"))) "!")"#
    );
    // Escapes: an escaped backtick or `$` does not end the part; `$` and
    // `{` alone are text.
    assert_eq!(ast(r"`a\`b`"), r#"(template "a`b" raw "a\\`b")"#);
    assert_eq!(
        ast(r"`\${a}$ {b} $`"),
        r#"(template "${a}$ {b} $" raw "\\${a}$ {b} $")"#
    );
    assert_eq!(ast(r"`\u{41}`"), r#"(template "A" raw "\\u{41}")"#);
    assert_eq!(
        ast(r"`\x41\n\0\'`"),
        r#"(template "A\n\0'" raw "\\x41\\n\\0\\'")"#
    );
    // Line terminators are text; CR and CRLF read as LF in both values, a
    // line continuation is nothing cooked.
    assert_eq!(ast("`a\r\nb`"), r#"(template "a\nb")"#);
    assert_eq!(ast("`a\rb\nc`"), r#"(template "a\nb\nc")"#);
    assert_eq!(ast("`a\\\r\nb`"), r#"(template "ab" raw "a\\\nb")"#);
    assert_eq!(ast("`a\u{2028}b`"), r#"(template "a\u{2028}b")"#);
    // ... and count for positions: the error is on line 3.
    rejects("x = `a\r\nb\nc` + @;", UnknownToken, 3, 6);
    // A template is not a directive, and ends a statement by ASI.
    assert_eq!(ast("`a`\nb"), r#"(template "a") b"#);
    assert!(
        !parse_script_with("`use strict`; 010", ScriptOptions::SLOPPY)
            .unwrap()
            .strict
    );
    // Lookahead past a template: an arrow with a template default.
    assert_eq!(
        ast("f = (a = `${b}`) => a;"),
        r#"(= f (arrow (a=(template "" b "")) => a))"#
    );

    // Unterminated: at the backtick, or at the `}` of the open part.
    rejects_with("`abc", UnexpectedEnd, 1, 1, "unterminated template");
    rejects_with("x = `a${b}c", UnexpectedEnd, 1, 10, "unterminated template");
    rejects_with("`a\\", UnexpectedEnd, 1, 1, "unterminated template");
    rejects("`${a", UnexpectedEnd, 1, 5);
    rejects("`${", UnexpectedEnd, 1, 4);
    // Invalid escapes are errors in an untagged template, in either mode;
    // octal and `\8` `\9` escapes too, also in sloppy code.
    rejects("`\\x4`", InvalidToken, 1, 2);
    rejects("`a${b}\\u12`", InvalidToken, 1, 7);
    rejects("`\\u{110000}`", InvalidToken, 1, 2);
    rejects_with("`\\1`", InvalidToken, 1, 2, "template");
    rejects_with("`\\00`", InvalidToken, 1, 2, "template");
    rejects_with("`\\8`", InvalidToken, 1, 2, "template");
    sloppy_rejects("`\\07`", InvalidToken, 1, 2);
    sloppy_rejects("`\\9`", InvalidToken, 1, 2);
    // An escape cannot read past the end of its part.
    rejects("`\\u{41`}", InvalidToken, 1, 2);
    // The substitution must be one Expression closed by `}`.
    rejects("`${}`", UnexpectedToken, 1, 4);
    rejects_with("`${a b}`", UnexpectedToken, 1, 6, "`}` closing");
    rejects_with("`${a)}`", UnexpectedToken, 1, 5, "`}` closing");
    rejects_with("`${a + }`", UnexpectedToken, 1, 8, "`}` closing");
    // A `}` closing a substitution is not a punctuator.
    rejects("{ `${ }` }", UnexpectedToken, 1, 7);
    // Tagged templates are clj-quickjs-58, also across a line break.
    rejects_with("f`x`", Unsupported, 1, 2, "tagged template");
    rejects_with("a.b\n`x${1}`", Unsupported, 2, 1, "tagged template");
    rejects_with("f()`x`", Unsupported, 1, 4, "tagged template");
    rejects_with("`${f`x`}`", Unsupported, 1, 5, "tagged template");
}

// ----- negative: lexical ambiguity -------------------------------------------

#[test]
fn rejects_lexically_ambiguous_input() {
    use SyntaxErrorKind::*;
    // `/` where an operand is expected starts a regular expression.
    rejects_with("x = /ab+c/g;", Unsupported, 1, 5, "regular expression");
    rejects_with("x = /=/;", Unsupported, 1, 5, "regular expression");
    rejects_with(
        "if (a) /re/.test(b);",
        Unsupported,
        1,
        8,
        "regular expression",
    );
    // The regular expression is diagnosed before its body is lexed.
    rejects_with("f(/'/)", Unsupported, 1, 3, "regular expression");
    // A number literal must not run into an identifier or digit.
    rejects("3.toString()", InvalidToken, 1, 3);
    rejects("let n = 3in x;", InvalidToken, 1, 10);
    rejects("0b102", InvalidToken, 1, 5);
    rejects("1.5n", InvalidToken, 1, 4);
    // Maximal munch: these are single tokens (the first two are compound
    // assignments, clj-quickjs-84; the others are unsupported).
    assert_eq!(ast("a>>>=b"), "(>>>= a b)");
    assert_eq!(ast("a**=b"), "(**= a b)");
    rejects_with("a&&=b", Unsupported, 1, 2, "&&=");
    rejects_with("a ?? b", Unsupported, 1, 3, "??");
    rejects_with("a?.b", Unsupported, 1, 2, "optional chaining");
    // `a--b` is `a--` then `b`, with no operator between them. (HTML-like
    // comments are not comments here: `a <!-- b` is `a < !--b`, and `++`
    // after a line break begins the next statement; both parse, see
    // `update_expressions`.)
    rejects("a--b", UnexpectedToken, 1, 4);
    // `(a, b)` before `=>` is an arrow parameter list (accepted, see
    // `arrow_functions_and_asi`), otherwise the comma operator (accepted,
    // see `comma_operator`).
    // An arrow function cannot be an operand without parentheses.
    rejects_with(
        "f = 1 + (a, b) => a;",
        UnexpectedToken,
        1,
        16,
        "parenthesized",
    );
    rejects_with("f = 1 + x => x;", UnexpectedToken, 1, 11, "parenthesized");
    rejects("f = ();", UnexpectedToken, 1, 6);
    // Legacy and modern number forms outside the subset.
    rejects("017", InvalidToken, 1, 1);
    rejects("08", InvalidToken, 1, 1);
    rejects_with("1_000", Unsupported, 1, 2, "separator");
    rejects_with("10n", Unsupported, 1, 1, "BigInt");
    rejects("0x;", InvalidToken, 1, 1);
    rejects("1e+;", InvalidToken, 1, 1);
    // Strings.
    rejects("'\\07'", InvalidToken, 1, 2);
    rejects("'\\1'", InvalidToken, 1, 2);
    rejects("'\\8'", InvalidToken, 1, 2);
    rejects("'\\x4'", InvalidToken, 1, 2);
    rejects("'\\u12'", InvalidToken, 1, 2);
    rejects("'\\u{110000}'", InvalidToken, 1, 2);
    rejects("'abc\ndef'", InvalidToken, 1, 1);
    // Characters and constructs that are not tokens of the subset.
    // (Template literals are tokens since clj-quickjs-57: `template_literals`.)
    assert_eq!(ast("x = `t`;"), r#"(= x (template "t"))"#);
    rejects("x = @y;", UnknownToken, 1, 5);
    // A hashbang is a comment only at the start of the source.
    assert_eq!(ast("#!/usr/bin/env node"), "");
    rejects("x;\n#!/usr/bin/env node", UnknownToken, 2, 1);
    rejects("x.#y", UnknownToken, 1, 3);
    // Unicode identifiers and identifier escapes: `unicode_identifiers`.
}

// ----- negative: truncated input -----------------------------------------------

#[test]
fn rejects_truncated_input_at_the_end() {
    use SyntaxErrorKind::UnexpectedEnd;
    for src in [
        "let x =",
        "let",
        "const",
        "function",
        "function f",
        "function f(a, b",
        "function f(a) {",
        "function f(a) { return a +",
        "if (x) {",
        "if (x",
        "if (x)",
        "while (",
        "{",
        "f(1,",
        "a ?",
        "a ? b :",
        "a.",
        "(a",
        "x = !",
        "x = 1 +",
        "if (a) b; else",
    ] {
        let e = err(src);
        assert_eq!(e.kind, UnexpectedEnd, "{src:?} gave {e}");
        assert_eq!(e.span.start, src.len(), "{src:?} gave {e}");
        assert_eq!((e.line, e.column), (1, src.chars().count() as u32 + 1));
    }
    // Unterminated tokens are reported at their start.
    rejects("x = 'abc", UnexpectedEnd, 1, 5);
    rejects("x = 'abc\\", UnexpectedEnd, 1, 9);
    rejects("x = 1 /* never closed", UnexpectedEnd, 1, 7);
    rejects("function f() {\n  return 1;\n", UnexpectedEnd, 3, 1);
}

// ----- negative: reserved words ------------------------------------------------

#[test]
fn rejects_reserved_word_misuse() {
    use SyntaxErrorKind::{Invalid, ReservedWord};
    rejects("let if = 1;", ReservedWord, 1, 5);
    rejects("let let = 1;", ReservedWord, 1, 5);
    rejects("const class = 1;", ReservedWord, 1, 7);
    rejects("let a, while;", ReservedWord, 1, 8);
    rejects("function return() {}", ReservedWord, 1, 10);
    rejects("function f(a, static) {}", ReservedWord, 1, 15);
    rejects("let yield = 1;", ReservedWord, 1, 5);
    rejects("let await = 1;", ReservedWord, 1, 5);
    rejects("let enum = 1;", ReservedWord, 1, 5);
    rejects("x = interface;", ReservedWord, 1, 5);
    rejects("x = yield;", ReservedWord, 1, 5);
    rejects("let private;", ReservedWord, 1, 5);
    rejects("let undefined = 1;", ReservedWord, 1, 5);
    rejects("function f(NaN) {}", ReservedWord, 1, 12);
    rejects("function Infinity() {}", ReservedWord, 1, 10);
    rejects("undefined = 1;", ReservedWord, 1, 1);
    rejects("(NaN) = 1;", ReservedWord, 1, 1);
    rejects("let eval = 1;", ReservedWord, 1, 5);
    rejects("function f(arguments) {}", ReservedWord, 1, 12);
    rejects("x = function eval() {};", ReservedWord, 1, 14);
    // Plain keywords in expression position are unexpected tokens.
    rejects("x = if;", SyntaxErrorKind::UnexpectedToken, 1, 5);
    rejects("true = 1;", Invalid, 1, 1);
}

// ----- directive prologues and sloppy mode (clj-quickjs-95) --------------------

fn sloppy_script(src: &str) -> Script {
    parse_script_with(src, ScriptOptions::SLOPPY)
        .unwrap_or_else(|e| panic!("sloppy {src:?} failed: {e}"))
}

/// Assert sloppy `src` is rejected with `kind` at 1-based `line:column`.
#[track_caller]
fn sloppy_rejects(src: &str, kind: SyntaxErrorKind, line: u32, column: u32) {
    match parse_script_with(src, ScriptOptions::SLOPPY) {
        Ok(script) => panic!("sloppy {src:?} parsed but must be rejected: {script:?}"),
        Err(e) => assert_eq!(
            (e.kind, e.line, e.column),
            (kind, line, column),
            "sloppy {src:?} gave {e}"
        ),
    }
}

/// `(name, strict)` of each function declaration in `body` and of each
/// function a `return` statement returns (named `=>` for an arrow), each
/// followed by those in its own body, in source order.
fn strictness(body: &[Stmt], out: &mut Vec<(String, bool)>) {
    for s in body {
        let f = match &s.kind {
            StmtKind::FunctionDecl(f) => f,
            StmtKind::Return(Some(Expr {
                kind: ExprKind::Function(f),
                ..
            })) => f,
            _ => continue,
        };
        let name = f.name.as_ref().map_or("=>", |n| n.name.as_str());
        out.push((name.to_string(), f.strict));
        strictness(&f.body, out);
    }
}

fn strictness_of(script: &Script) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    strictness(&script.body, &mut out);
    out
}

#[test]
fn directive_prologues_set_strictness() {
    use SyntaxErrorKind::{ReservedWord, Unsupported};
    // Sloppy code may bind `eval` and `arguments`.
    for src in [
        "function f(eval) {}",
        "let arguments = 1;",
        "var eval, arguments = 2;",
        "const eval = 1;",
        "function eval() {} function arguments() {}",
        "x = function eval(arguments) {};",
        "(eval) => 1; arguments => { return 1; };",
        "try {} catch (eval) {}",
        // Not a directive prologue: the directive follows a statement.
        "function f() { 1; \"use strict\"; let arguments; }",
        // Not `"use strict"` directives: parenthesized, an operand, an
        // escape sequence, a line continuation.
        "function f() { (\"use strict\"); let eval; }",
        "function f() { \"use strict\" + 1; let eval; }",
        "function f() { \"use strict\"\n+ 1; let eval; }",
        "function f() { \"use\\x20strict\"; let eval; }",
        "function f() { 'use \\\nstrict'; let eval; }",
        "function f() { \"use strict \"; let eval; }",
        // A directive applies to its own function only.
        "function f() { \"use strict\"; } let eval;",
        "x = function () { 'use strict'; }; function g(eval) {}",
    ] {
        sloppy_script(src);
    }
    // `"use strict"` checks the name and parameters again, retroactively.
    sloppy_rejects("function f(eval) { \"use strict\"; }", ReservedWord, 1, 12);
    sloppy_rejects("function eval() { 'use strict'; }", ReservedWord, 1, 10);
    sloppy_rejects(
        "x = function arguments() { 'use strict' };",
        ReservedWord,
        1,
        14,
    );
    sloppy_rejects(
        "function f(a, arguments) { \"a\"; \"use strict\"; }",
        ReservedWord,
        1,
        15,
    );
    // A script-level directive makes everything after it strict.
    sloppy_rejects("\"use strict\"; function f(eval) {}", ReservedWord, 1, 26);
    sloppy_rejects("'use strict'\nlet arguments;", ReservedWord, 2, 5);
    // Arrow functions may have their own directive, and inherit strictness.
    sloppy_rejects("(eval) => { \"use strict\"; }", ReservedWord, 1, 2);
    sloppy_rejects("eval => { 'use strict' }", ReservedWord, 1, 1);
    sloppy_rejects(
        "function f() { \"use strict\"; return (eval) => 1; }",
        ReservedWord,
        1,
        38,
    );
    sloppy_rejects(
        "function f() { 'use strict'; function g() { let eval; } }",
        ReservedWord,
        1,
        49,
    );
    // Sloppy references to them parse; the compiler decides whether they
    // denote a binding (`tests/js_source.rs` `use_strict_directives`).
    // Strict references stay unsupported.
    sloppy_script("let eval = 1; eval = eval + 1; eval++; typeof arguments;");
    sloppy_rejects(
        "function f() { 'use strict'; return eval; }",
        Unsupported,
        1,
        37,
    );
    rejects("let a = 1; a = eval;", Unsupported, 1, 16);

    // The recorded mode of the script and of each function.
    let script = sloppy_script(
        "function sloppy() { return () => 1; }
         function strict() { 'use strict'; function inner() {} return () => { return 2; }; }
         function own() { return () => { 'use strict'; }; }",
    );
    assert!(!script.strict);
    let expected = [
        ("sloppy", false),
        ("=>", false),
        ("strict", true),
        ("inner", true),
        ("=>", true),
        ("own", false),
        ("=>", true),
    ];
    let expected: Vec<_> = expected.iter().map(|&(n, s)| (n.to_string(), s)).collect();
    assert_eq!(strictness_of(&script), expected);
    let script = sloppy_script("\"use strict\"; function f() { return () => 1; }");
    assert!(script.strict);
    assert_eq!(
        strictness_of(&script),
        [("f".to_string(), true), ("=>".to_string(), true)]
    );
    // Directives stay ordinary expression statements.
    assert_eq!(sloppy_script("'use strict'; 1;").body.len(), 2);

    // Strict defaults unchanged: `parse_script` is strict throughout.
    rejects("function f(eval) {}", ReservedWord, 1, 12);
    rejects("let arguments = 1;", ReservedWord, 1, 5);
    let script = parse_script("function f() { return () => 1; }").unwrap();
    assert!(script.strict);
    assert_eq!(
        strictness_of(&script),
        [("f".to_string(), true), ("=>".to_string(), true)]
    );
    assert_eq!(
        parse_script("let a = 1;"),
        parse_script_with("let a = 1;", ScriptOptions::STRICT)
    );
}

fn sloppy_ast(src: &str) -> String {
    sloppy_script(src)
        .body
        .iter()
        .map(render_stmt)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Legacy octal (`017`) and leading-zero decimal (`08`, `09.5`) number
/// literals are lexed in sloppy code and stay early errors in strict code
/// (clj-quickjs-51; ECMAScript B.1.1, QuickJS `next_token` and `js_atof`).
#[test]
fn legacy_octal_literals_in_sloppy_mode() {
    use SyntaxErrorKind::{InvalidToken, UnexpectedToken};
    for (src, expected) in [
        ("01;", "1"),
        ("017;", "15"),
        ("00;", "0"),
        ("0777;", "511"),
        ("08;", "8"),
        ("09.5;", "9.5"),
        ("08e1;", "80"),
        ("08.5e1;", "85"),
        ("078;", "78"),
        ("0189.5;", "189.5"),
        ("09.;", "9"),
        // A legacy octal integer has no fraction: its `.` is a member access.
        ("01.a;", "(. 1 a)"),
        ("08.5.a;", "(. 8.5 a)"),
        ("x = 010 + 0.5;", "(= x (+ 8 0.5))"),
    ] {
        assert_eq!(sloppy_ast(src), expected, "sloppy {src:?}");
    }
    // Beyond 2^53 the octal value is correctly rounded.
    assert_eq!(
        sloppy_ast("01000000000000000001;"),
        format!("{}", 2f64.powi(54))
    );
    // A legacy octal integer takes no fraction, exponent, BigInt suffix or
    // separator, and neither does a leading-zero decimal take a suffix or
    // separator.
    sloppy_rejects("01.5", UnexpectedToken, 1, 3);
    sloppy_rejects("01e1", InvalidToken, 1, 3);
    sloppy_rejects("01n", InvalidToken, 1, 3);
    sloppy_rejects("08n", InvalidToken, 1, 3);
    sloppy_rejects("0_1", InvalidToken, 1, 2);
    sloppy_rejects("01_2", InvalidToken, 1, 3);
    sloppy_rejects("08_1", InvalidToken, 1, 3);
    sloppy_rejects("09.a", InvalidToken, 1, 4);
    // Strict code (the default) rejects them at the literal.
    rejects("01", InvalidToken, 1, 1);
    rejects("08", InvalidToken, 1, 1);
    rejects("x = 09.5;", InvalidToken, 1, 5);
    rejects("0_1", InvalidToken, 1, 1);
    // A `"use strict"` directive makes the literal right after it strict,
    // although that token is read before the directive is recognised.
    sloppy_rejects("\"use strict\"; 010", InvalidToken, 1, 15);
    sloppy_rejects("'use strict'\n08;", InvalidToken, 2, 1);
    sloppy_rejects("\"a\"; 'use strict'; 01.a", InvalidToken, 1, 20);
    sloppy_rejects("function f() { 'use strict'; 010; }", InvalidToken, 1, 30);
    sloppy_rejects("() => { 'use strict'\n07 }", InvalidToken, 2, 1);
    sloppy_rejects(
        "function f() { 'use strict'; x = 010; }",
        InvalidToken,
        1,
        34,
    );
    // That token is strict even when lexing it as sloppy code fails: the
    // strict error at its leading zero is reported, not the sloppy one.
    sloppy_rejects("\"use strict\"; 08.1_2;", InvalidToken, 1, 15);
    sloppy_rejects("\"use strict\"; 01n;", InvalidToken, 1, 15);
    sloppy_rejects("'use strict'\n01n;", InvalidToken, 2, 1);
    sloppy_rejects("'use strict' /*\n*/ 0_1;", InvalidToken, 2, 4);
    sloppy_rejects("function f() { 'use strict'; 08n }", InvalidToken, 1, 30);
    // Not a directive: the sloppy error stands.
    sloppy_rejects("'use strict' 01n;", InvalidToken, 1, 16);
    sloppy_rejects("1; 'use strict'; 01n;", InvalidToken, 1, 20);
    sloppy_rejects("x = 'use strict'; 01n;", InvalidToken, 1, 21);
    // ... and only to the code it applies to: the literal after the
    // function's closing brace is sloppy again.
    assert_eq!(
        sloppy_ast("function f() { 'use strict'; }\n010;")
            .rsplit(' ')
            .next(),
        Some("8")
    );
    assert_eq!(
        sloppy_ast("x = () => { 'use strict' }; 010;")
            .rsplit(' ')
            .next(),
        Some("8")
    );
    // A string that is not a `"use strict"` directive leaves it sloppy.
    assert_eq!(sloppy_ast("'use strict' + 010;"), "(+ \"use strict\" 8)");
    assert_eq!(
        sloppy_ast("1; 'use strict'; 010;").rsplit(' ').next(),
        Some("8")
    );
}

/// Legacy octal escapes (`\07`, `\101`) and `\8` `\9` in string literals
/// are lexed in sloppy code and stay early errors in strict code, also in
/// the directives of a prologue before and the token right after a
/// `"use strict"` directive (clj-quickjs-258; ECMAScript B.1.2, QuickJS
/// `js_parse_string` and `js_parse_directives`).
#[test]
fn legacy_octal_escapes_in_sloppy_mode() {
    use SyntaxErrorKind::{InvalidToken, UnexpectedEnd};
    let value = |src: &str| match &sloppy_script(src).body[0].kind {
        StmtKind::Expr(Expr {
            kind: ExprKind::String(s),
            ..
        }) => s.clone(),
        other => panic!("sloppy {src:?} is not a string statement: {other:?}"),
    };
    for (src, expected) in [
        (r#""\07";"#, "\u{7}"),
        (r#""\101";"#, "A"),
        (r#""\400";"#, " 0"),
        (r#""\8";"#, "8"),
        (r#""\9";"#, "9"),
        (r#""\1";"#, "\u{1}"),
        (r#""\377";"#, "\u{FF}"),
        (r#""\3777";"#, "\u{FF}7"),
        (r#""\47";"#, "'"),
        (r#""\777";"#, "?7"),
        (r#""\08";"#, "\u{0}8"),
        (r#""\0";"#, "\u{0}"),
        (r#""\00";"#, "\u{0}"),
        (r#""\000";"#, "\u{0}"),
        (r#""\0000";"#, "\u{0}0"),
        (r#""\18";"#, "\u{1}8"),
        (r#""a\12b\9c";"#, "a\nb9c"),
        (r"'\101\102';", "AB"),
    ] {
        assert_eq!(value(src), expected, "sloppy {src:?}");
    }
    // Strict code (the default) rejects each at its escape.
    for (src, column) in [
        (r#""\07""#, 2),
        (r#""\101""#, 2),
        (r#""\400""#, 2),
        (r#""\8""#, 2),
        (r#""\9""#, 2),
        (r#""\08""#, 2),
        (r#""ab\1""#, 4),
        (r#"x = '\8';"#, 6),
    ] {
        rejects(src, InvalidToken, 1, column);
    }
    // A `"use strict"` directive makes the string right after it strict,
    // although that token is read before the directive is recognised ...
    sloppy_rejects(r#""use strict"; "\07""#, InvalidToken, 1, 16);
    sloppy_rejects("'use strict'\n'\\8';", InvalidToken, 2, 2);
    sloppy_rejects(
        r#"function f() { "use strict"; "\101"; }"#,
        InvalidToken,
        1,
        31,
    );
    sloppy_rejects(r#"() => { 'use strict'; 'a\1' }"#, InvalidToken, 1, 25);
    // ... also when lexing it as sloppy code fails: the strict error at
    // the escape is reported, not the sloppy one.
    sloppy_rejects(r#""use strict"; "\07"#, InvalidToken, 1, 16);
    sloppy_rejects("'use strict' /*\n*/ '\\9", InvalidToken, 2, 5);
    // Not a directive: the sloppy error stands.
    sloppy_rejects(r#"'use strict' "\07"#, UnexpectedEnd, 1, 14);
    sloppy_rejects(r#"x = 'use strict'; "\07"#, UnexpectedEnd, 1, 19);
    // The directives before it are strict code too.
    sloppy_rejects(r#""\07"; "use strict";"#, InvalidToken, 1, 2);
    sloppy_rejects(r#""a"; '\8'; 'b'; 'use strict'"#, InvalidToken, 1, 7);
    sloppy_rejects(
        r#"function f() { "\1"
            'use strict' }"#,
        InvalidToken,
        1,
        17,
    );
    // Only the code a directive applies to is strict: an enclosing or a
    // neighbouring function, a later statement of a script whose prologue
    // has ended, and a string that is not a `"use strict"` directive stay
    // sloppy.
    for src in [
        r#""\07"; x = function () { "use strict"; };"#,
        r#"function f() { "use strict"; } "\07";"#,
        r#"x = () => { 'use strict' }; '\8';"#,
        r#"1; "use strict"; "\07";"#,
        r#""use strict" + "\07";"#,
        r#""\07"; "use\x20strict"; "\8";"#,
    ] {
        sloppy_script(src);
    }
}

// ----- negative: unsupported syntax --------------------------------------------

#[test]
fn rejects_unsupported_syntax_with_positions() {
    use SyntaxErrorKind::Unsupported;
    let cases: &[(&str, u32, u32, &str)] = &[
        ("class A {}", 1, 1, "classes"),
        ("x = class {};", 1, 5, "classes"),
        ("async function f() {}", 1, 1, "async functions"),
        ("x = async x => x;", 1, 5, "async arrow"),
        ("x = async (y) => y;", 1, 5, "arrow"),
        ("await x;", 1, 1, "await"),
        ("function* g() {}", 1, 1, "generator"),
        ("for (x of y) {}", 1, 1, "for`-`of"),
        ("try {} catch ([e]) {}", 1, 15, "destructuring"),
        ("with (o) {}", 1, 1, "with"),
        ("debugger;", 1, 1, "debugger"),
        ("import x from 'y';", 1, 1, "import"),
        ("export const x = 1;", 1, 1, "export"),
        ("x = import('y');", 1, 5, "import"),
        ("x = this;", 1, 5, "this"),
        ("x = super.y;", 1, 5, "super"),
        ("delete x.y;", 1, 1, "delete"),
        ("x.y &&= 1;", 1, 5, "&&="),
        ("x = [...a];", 1, 6, "spread elements"),
        ("[a] = b;", 1, 1, "destructuring assignment"),
        ("f(...a);", 1, 3, "spread"),
        ("function f(...a) {}", 1, 12, "rest"),
        ("let [a] = b;", 1, 5, "destructuring"),
        ("let {a} = b;", 1, 5, "destructuring"),
        ("function f({a}) {}", 1, 12, "destructuring"),
        ("a ||= 1;", 1, 3, "||="),
        ("x in y;", 1, 3, "in"),
        ("x instanceof y;", 1, 3, "instanceof"),
        ("x = eval;", 1, 5, "eval"),
        ("function f() { return arguments; }", 1, 23, "arguments"),
    ];
    for &(src, line, column, needle) in cases {
        rejects_with(src, Unsupported, line, column, needle);
    }
}

// ----- negative: JavaScript early errors and grammar mismatches ---------------

#[test]
fn rejects_early_errors_and_misplaced_tokens() {
    use SyntaxErrorKind::{Invalid, UnexpectedToken};
    rejects("return 1;", Invalid, 1, 1);
    rejects("{ return; }", Invalid, 1, 3);
    rejects("function f() {} return;", Invalid, 1, 17);
    rejects("const c;", Invalid, 1, 7);
    rejects("const a = 1, b;", Invalid, 1, 14);
    rejects("function f(a, b, a) {}", Invalid, 1, 18);
    rejects("if (x) let y = 1;", Invalid, 1, 8);
    rejects("while (x) const y = 1;", Invalid, 1, 11);
    rejects("if (x) function f() {}", Invalid, 1, 8);
    rejects("1 = 2;", Invalid, 1, 1);
    rejects("a + b = c;", Invalid, 1, 1);
    rejects("f() = 1;", Invalid, 1, 1);
    rejects("(a = 1) = 2;", Invalid, 1, 1);
    // ASI does not apply on one line, or to create an empty statement.
    rejects("let a = 1 let b = 2;", UnexpectedToken, 1, 11);
    rejects("a b", UnexpectedToken, 1, 3);
    rejects("if (x)\nelse y;", UnexpectedToken, 2, 1);
    rejects("if (x) else y;", UnexpectedToken, 1, 8);
    rejects("function () {}", UnexpectedToken, 1, 10);
    rejects("x = function (a b) {};", UnexpectedToken, 1, 17);
    rejects("f(a b);", UnexpectedToken, 1, 5);
    rejects("x = (1;", UnexpectedToken, 1, 7);
    rejects("x = a ? b;", UnexpectedToken, 1, 10);
    rejects("}", UnexpectedToken, 1, 1);
    rejects("if x {}", UnexpectedToken, 1, 4);
    rejects("x.1", UnexpectedToken, 1, 2);
}

// ----- robustness ----------------------------------------------------------------

#[test]
fn deep_nesting_is_rejected_without_overflowing_the_stack() {
    let n = 20_000;
    let cases = [
        format!("x = {}1{};", "(".repeat(n), ")".repeat(n)),
        format!("x = {}1;", "!".repeat(n)),
        format!("x = {}1;", "- ".repeat(n)),
        format!("x = {}a;", "++ ".repeat(n)),
        format!("x = {}a;", "-- -".repeat(n)),
        format!("{}{}", "{".repeat(n), "}".repeat(n)),
        format!("{}x;", "if (1) ".repeat(n)),
        format!("{}x;", "while (1) ".repeat(n)),
        format!("{}x;{}", "do ".repeat(n), " while (1)".repeat(n)),
        format!("{}x;", "for (;;) ".repeat(n)),
        format!("{}x;{}", "switch (1) { case 1: ".repeat(n), "}".repeat(n)),
        format!("{}x;", "for (let i = 0; i < 1; i = i + 1) ".repeat(n)),
        format!("{}{}", "function f() {".repeat(n), "}".repeat(n)),
        format!(
            "x = {}1{};",
            "function () { return ".repeat(n),
            "}".repeat(n)
        ),
        format!("x = {}1{};", "function (a = ".repeat(n), ") {}".repeat(n)),
        format!("x = {}1{};", "(a = ".repeat(n), ") => a".repeat(n)),
        format!("x = 1{};", " + 1".repeat(n)),
        format!("x = 1{};", " ** 1".repeat(n)),
        format!("x = f{};", "()".repeat(n)),
        format!("x = a{};", ".b".repeat(n)),
        format!("x = {}1;", "a = ".repeat(n)),
        format!("x = {}1;", "a ? b : ".repeat(n)),
        format!("x = {}1{};", "f(".repeat(n), ")".repeat(n)),
        // Truncated deep input is also rejected for depth, not overflow.
        "(".repeat(n),
    ];
    for src in &cases {
        let e = err(src);
        assert_eq!(e.kind, SyntaxErrorKind::TooDeep, "{}: {e}", &src[..20]);
    }
}

/// Wrapping finished subtrees again (which the parser's recursion budget
/// does not see) is bounded by the tree-height limit.
#[test]
fn mixed_expression_depth_is_bounded() {
    let mut s = "a".to_string();
    for _ in 0..30 {
        s = format!("({s}){}", ".b".repeat(100));
    }
    let e = err(&format!("x = {s};"));
    assert_eq!(e.kind, SyntaxErrorKind::TooDeep, "{e}");

    // Statements, functions and expressions all count toward the height.
    let mut s = "1".to_string();
    for _ in 0..100 {
        s = format!("(function () {{ return {s}; }})()");
    }
    let e = err(&format!("x = {s};"));
    assert_eq!(e.kind, SyntaxErrorKind::TooDeep, "{e}");

    let script = parse_script("x = 1 + 2 * 3;").unwrap();
    // Number, `*`, `+`, `=`, statement.
    assert_eq!(script.body[0].height, 5);
    let script = parse_script("function f() { if (a) { return; } }").unwrap();
    // return, block, if, function, statement.
    assert_eq!(script.body[0].height, 5);
}

/// For each nesting construct, every depth up to the first one rejected as
/// [`SyntaxErrorKind::TooDeep`] parses and compiles (parse, then lowering)
/// within a 2 MiB stack (Rust's default for spawned threads) in this build
/// profile. A stack overflow here aborts the test binary, failing the run
/// (clj-quickjs-262: assignment chains used to overflow the lowering before
/// the parser's limit).
#[test]
fn nesting_at_the_limit_fits_a_2_mib_stack() {
    type Make = fn(usize) -> String;
    let makers: &[(&str, Make)] = &[
        ("parens", |n| {
            format!("x = {}1{};", "(".repeat(n), ")".repeat(n))
        }),
        ("blocks", |n| format!("{}{}", "{".repeat(n), "}".repeat(n))),
        ("if", |n| format!("{}x;", "if (1) ".repeat(n))),
        ("while", |n| format!("{}x;", "while (1) ".repeat(n))),
        ("do-while", |n| {
            format!("{}x;{}", "do ".repeat(n), " while (1)".repeat(n))
        }),
        ("for", |n| format!("{}x;", "for (;;) ".repeat(n))),
        ("var bodies", |n| {
            format!("{}var x = 1;", "if (1) ".repeat(n))
        }),
        ("switch", |n| {
            format!("{}x;{}", "switch (1) { case 1: ".repeat(n), "}".repeat(n))
        }),
        ("label sets", |n| {
            let labels: String = (0..n).map(|i| format!("l{i}: ")).collect();
            format!("{labels}x;")
        }),
        ("labelled loops", |n| {
            let loops: String = (0..n).map(|i| format!("l{i}: while (1) ")).collect();
            format!("{loops}break l0;")
        }),
        ("for with heads", |n| {
            format!("{}x;", "for (let i = (a, b); (i); i = i + 1) ".repeat(n))
        }),
        ("unary", |n| format!("x = {}1;", "!".repeat(n))),
        ("void", |n| format!("x = {}f();", "void ".repeat(n))),
        ("binary", |n| format!("x = 1{};", " + 1".repeat(n))),
        ("exponent", |n| format!("x = 1{};", " ** 1".repeat(n))),
        ("exponent of unary", |n| {
            format!("x = {}1{};", "2 ** -(".repeat(n), ")".repeat(n))
        }),
        ("comma", |n| format!("x = 1{};", ", 1".repeat(n))),
        ("nested commas", |n| {
            format!("x = {}1{};", "(a, ".repeat(n), ")".repeat(n))
        }),
        ("member", |n| format!("x = a{};", ".b".repeat(n))),
        ("call chain", |n| format!("x = f{};", "()".repeat(n))),
        ("nested calls", |n| {
            format!("x = {}1{};", "f(".repeat(n), ")".repeat(n))
        }),
        ("conditional", |n| format!("x = {}1;", "a ? b : ".repeat(n))),
        ("assignment", |n| format!("x = {}1;", "a = ".repeat(n))),
        ("function decls", |n| {
            format!("{}{}", "function f() {".repeat(n), "}".repeat(n))
        }),
        ("function exprs", |n| {
            format!(
                "x = {}1{};",
                "function () { return ".repeat(n),
                "}".repeat(n)
            )
        }),
        ("concise arrows", |n| format!("x = {}1;", "a => ".repeat(n))),
        ("parenthesized arrows", |n| {
            format!("x = {}1;", "(a, b) => ".repeat(n))
        }),
        ("block arrows", |n| {
            format!("x = {}1{};", "() => { return ".repeat(n), "}".repeat(n))
        }),
        ("defaults", |n| {
            format!("x = {}1{};", "function (a = ".repeat(n), ") {}".repeat(n))
        }),
        ("arrow defaults", |n| {
            format!("x = {}1{};", "(a = ".repeat(n), ") => a".repeat(n))
        }),
        ("mixed", |n| {
            format!("x = {}1{};", "a + -(".repeat(n), ")".repeat(n))
        }),
        ("updates", |n| {
            format!("x = {}++b{};", "a++ + -(".repeat(n), ")".repeat(n))
        }),
        ("object literals", |n| {
            format!("x = {}1{};", "{a: ".repeat(n), "}".repeat(n))
        }),
        ("object literal values", |n| {
            format!("x = {}1{};", "{a: 1, b: x.y = ".repeat(n), ", c}".repeat(n))
        }),
        ("property assignments", |n| {
            format!("x = {}1;", "a.b = ".repeat(n))
        }),
        ("templates", |n| {
            format!("x = {}1{};", "`${".repeat(n), "}`".repeat(n))
        }),
        ("array literals", |n| {
            format!("x = {}1{};", "[".repeat(n), "]".repeat(n))
        }),
        ("array elements", |n| {
            format!("x = {}1{};", "[1, , a[0] = ".repeat(n), ", 2]".repeat(n))
        }),
        ("index chains", |n| format!("x = a{};", "[0]".repeat(n))),
        ("nested indexes", |n| {
            format!("x = {}0{};", "a[".repeat(n), "]".repeat(n))
        }),
        ("index assignments", |n| {
            format!("x = {}1;", "a[0] = ".repeat(n))
        }),
        ("wrapped member chains", |n| {
            let mut s = "a".to_string();
            for _ in 0..n {
                s = format!("({s}).b.b.b.b.b");
            }
            format!("x = {s};")
        }),
        // Right-associative assignment chains (clj-quickjs-262).
        ("compound assignments", |n| {
            format!("x = {}1;", "a += ".repeat(n))
        }),
        ("property compound assignments", |n| {
            format!("x = {}1;", "o.b += ".repeat(n))
        }),
        ("index compound assignments", |n| {
            format!("x = {}1;", "o[k] -= ".repeat(n))
        }),
        ("computed index assignments", |n| {
            format!("x = {}1;", "o[k] = ".repeat(n))
        }),
        ("mixed assignments", |n| {
            format!("x = {}1;", "a = o.b += o[0] = a -= ".repeat(n))
        }),
        // `new` (clj-quickjs-71).
        ("new chains", |n| format!("x = {}F;", "new ".repeat(n))),
        ("nested new arguments", |n| {
            format!("x = {}1{};", "new F(".repeat(n), ")".repeat(n))
        }),
    ];
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            for (name, make) in makers {
                let mut deepest = 0;
                for n in 1..=MAX_NESTING {
                    match compile_script(name, &make(n)) {
                        Ok(_) => deepest = n,
                        Err(e) if e.kind == SyntaxErrorKind::TooDeep => break,
                        Err(e) => panic!("{name} at depth {n}: {e}"),
                    }
                }
                // Every construct allows meaningful nesting.
                assert!(deepest >= MAX_NESTING / 6, "{name}: only {deepest} levels");
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn arbitrary_input_never_panics() {
    // Deterministic pseudo-random token soup and byte soup.
    let pieces = [
        "(",
        ")",
        "{",
        "}",
        "[",
        "]",
        ";",
        ",",
        ".",
        "?",
        ":",
        "=",
        "=>",
        "+",
        "-",
        "*",
        "/",
        "!",
        "&&",
        "||",
        "===",
        "<",
        "let ",
        "const ",
        "function ",
        "return ",
        "if ",
        "else ",
        "while ",
        "x",
        "f",
        "1",
        "0x",
        "1e",
        "'s",
        "\"",
        "`",
        "${",
        "\\",
        "\n",
        "/*",
        "*/",
        "//",
        "async ",
        "class ",
        "é",
        "\u{2028}",
        "'\\u",
        "#",
        "@",
        "0.",
        "..",
        "...",
    ];
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..3000 {
        let len = (next() % 24) as usize;
        let src: String = (0..len)
            .map(|_| pieces[(next() % pieces.len() as u64) as usize])
            .collect();
        if let Err(e) = parse_script(&src) {
            assert!(
                e.span.start <= src.len() && e.span.end <= src.len(),
                "{src:?}: {e}"
            );
            assert!(src.is_char_boundary(e.span.start), "{src:?}: {e}");
        }
    }
}

// ----- grammar document agreement ---------------------------------------------

/// Every `js accept` example in docs/grammar.md parses, every
/// `js reject <Kind> <line>:<column>` example fails to parse exactly so, and
/// every `js compile-reject <Kind> <line>:<column>` example parses but fails
/// to compile exactly so.
#[test]
fn grammar_doc_examples() {
    let doc = include_str!("../../docs/grammar.md");
    let mut lines = doc.lines();
    let (mut accepted, mut rejected) = (0, 0);
    while let Some(line) = lines.next() {
        let Some(info) = line.strip_prefix("```js ") else {
            continue;
        };
        let body: Vec<&str> = lines.by_ref().take_while(|l| *l != "```").collect();
        let src = body.join("\n");
        let words: Vec<&str> = info.split_whitespace().collect();
        match words.as_slice() {
            ["accept"] => {
                parse_script(&src).unwrap_or_else(|e| panic!("doc example {src:?}: {e}"));
                accepted += 1;
            }
            ["compile-reject", kind, position] => {
                parse_script(&src).unwrap_or_else(|e| panic!("doc example {src:?}: {e}"));
                let e = super::compile_script("doc", &src).expect_err(&src);
                assert_eq!(
                    format!("{:?} {}:{}", e.kind, e.line, e.column),
                    format!("{kind} {position}"),
                    "doc example {src:?} gave {e}"
                );
                rejected += 1;
            }
            ["reject", kind, position] => {
                let e = err(&src);
                assert_eq!(
                    format!("{:?} {}:{}", e.kind, e.line, e.column),
                    format!("{kind} {position}"),
                    "doc example {src:?} gave {e}"
                );
                rejected += 1;
            }
            // Parsed as non-strict code (clj-quickjs-95).
            ["sloppy-accept"] => {
                sloppy_script(&src);
                accepted += 1;
            }
            ["sloppy-reject", kind, position] => {
                let e = parse_script_with(&src, ScriptOptions::SLOPPY).expect_err(&src);
                assert_eq!(
                    format!("{:?} {}:{}", e.kind, e.line, e.column),
                    format!("{kind} {position}"),
                    "doc example {src:?} gave {e}"
                );
                rejected += 1;
            }
            _ => panic!("malformed example fence {line:?}"),
        }
    }
    assert!(
        accepted >= 4 && rejected >= 10,
        "{accepted} accepted, {rejected} rejected"
    );
}

#[test]
fn diagnostics_display_position_and_message() {
    let e = err("let a = 1;\nlet b = /x/;");
    assert_eq!(
        e.to_string(),
        "2:9: SyntaxError: regular expression literals are not supported by this JavaScript subset"
    );
    assert_eq!(e.span.text("let a = 1;\nlet b = /x/;"), "/");
}

/// Binary operators, calls and member accesses record their operator
/// token (`op_span`), where run-time failures are reported
/// (clj-quickjs-52); the node's span still covers the whole expression.
#[test]
fn operator_tokens_are_recorded() {
    fn tokens<'a>(e: &Expr, src: &'a str, out: &mut Vec<&'a str>) {
        match &e.kind {
            ExprKind::Binary {
                left,
                right,
                op_span,
                ..
            } => {
                tokens(left, src, out);
                out.push(op_span.text(src));
                tokens(right, src, out);
            }
            ExprKind::Call {
                callee,
                args,
                op_span,
            } => {
                tokens(callee, src, out);
                out.push(op_span.text(src));
                for arg in args {
                    tokens(arg, src, out);
                }
            }
            ExprKind::Member {
                object, op_span, ..
            } => {
                tokens(object, src, out);
                out.push(op_span.text(src));
            }
            _ => {}
        }
    }
    let src = "a * b ** c . d ( 1 << 2 ) >>> f\n(g);";
    let script = parse_script(src).unwrap();
    let StmtKind::Expr(e) = &script.body[0].kind else {
        panic!("not an expression statement")
    };
    assert_eq!(e.span.text(src), "a * b ** c . d ( 1 << 2 ) >>> f\n(g)");
    let mut out = Vec::new();
    tokens(e, src, &mut out);
    assert_eq!(out, ["*", "**", ".", "(", "<<", ">>>", "("]);
}
