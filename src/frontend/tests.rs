//! Frontend tests. Synthetic fixtures written for this project, not
//! light-cljs output and not adapted from QuickJS, except one line in
//! `hashbang_comment_only_at_start` and three scripts in
//! `arrow_functions_and_asi` (each marked there, with the QuickJS notice).

use super::ast::{
    BinaryOp, Expr, ExprKind, Function, FunctionKind, LogicalOp, Stmt, StmtKind, UnaryOp,
};
use super::lexer::{Lexer, Punct, TokenKind};
use super::{MAX_NESTING, SyntaxError, SyntaxErrorKind, line_column, parse_script};

// ----- S-expression rendering of the AST ------------------------------------

fn render_expr(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Number(n) => format!("{n}"),
        ExprKind::String(s) => format!("{s:?}"),
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::Null => "null".into(),
        ExprKind::Undefined => "undefined".into(),
        ExprKind::Identifier(name) => name.clone(),
        ExprKind::Unary { op, operand } => {
            let op = match op {
                UnaryOp::Not => "!",
                UnaryOp::Neg => "neg",
                UnaryOp::Plus => "pos",
            };
            format!("({op} {})", render_expr(operand))
        }
        ExprKind::Binary { op, left, right } => {
            let op = match op {
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
            };
            format!("({op} {} {})", render_expr(left), render_expr(right))
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
        ExprKind::Assign { target, value } => {
            format!("(= {} {})", target.name, render_expr(value))
        }
        ExprKind::Call { callee, args } => {
            let mut out = format!("(call {}", render_expr(callee));
            for arg in args {
                out.push(' ');
                out.push_str(&render_expr(arg));
            }
            out.push(')');
            out
        }
        ExprKind::Member { object, property } => {
            format!("(. {} {})", render_expr(object), property.name)
        }
        ExprKind::Function(f) => render_function(f),
        ExprKind::Sequence { left, right } => {
            format!("(, {} {})", render_expr(left), render_expr(right))
        }
    }
}

fn render_function(f: &Function) -> String {
    let params: Vec<&str> = f.params.iter().map(|p| p.name.as_str()).collect();
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

fn render_stmt(s: &Stmt) -> String {
    match &s.kind {
        StmtKind::FunctionDecl(f) => render_function(f),
        StmtKind::Lexical(decl) => {
            let keyword = match decl.kind {
                super::ast::LexicalKind::Let => "let",
                super::ast::LexicalKind::Const => "const",
            };
            let mut out = format!("({keyword}");
            for d in &decl.declarators {
                match &d.init {
                    Some(init) => {
                        out.push_str(&format!(" ({} {})", d.name.name, render_expr(init)))
                    }
                    None => out.push_str(&format!(" ({})", d.name.name)),
                }
            }
            out.push(')');
            out
        }
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

#[test]
fn unary_binds_tighter_than_binary_and_looser_than_calls() {
    assert_eq!(ast("-x * y;"), "(* (neg x) y)");
    assert_eq!(ast("!a + -b * +c;"), "(+ (! a) (* (neg b) (pos c)))");
    assert_eq!(ast("!!f(x).y;"), "(! (! (. (call f x) y)))");
    assert_eq!(ast("- -1;"), "(neg (neg 1))");
    assert_eq!(ast("a - -b;"), "(- a (neg b))");
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

// ----- positive: statements --------------------------------------------------

#[test]
fn declarations() {
    assert_eq!(ast("let a;"), "(let (a))");
    assert_eq!(ast("let a, b = 1, c;"), "(let (a) (b 1) (c))");
    assert_eq!(ast("const d = 2, e = d;"), "(const (d 2) (e d))");
    assert_eq!(ast("let $_x9 = async;"), "(let ($_x9 async))");
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
fn unicode_string_escapes_decode_scalars() {
    let src = r"x = 'A😀éz';";
    let script = parse_script(src).unwrap();
    let StmtKind::Expr(Expr {
        kind: ExprKind::Assign { value, .. },
        ..
    }) = &script.body[0].kind
    else {
        panic!()
    };
    // The surrogate pair combines into one scalar value; both escapes are
    // consumed and the following characters are kept.
    assert_eq!(value.kind, ExprKind::String("A😀éz".into()));
    assert_eq!(value.span.text(src), r"'A😀éz'");
    // A high surrogate followed by a non-surrogate escape is not combined.
    rejects_with(
        r"'\uD83DA'",
        SyntaxErrorKind::Unsupported,
        1,
        2,
        "surrogate",
    );
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
    rejects_with("(a = 1) => 1", Unsupported, 1, 4, "default parameter");
    rejects_with("([a]) => 1", Unsupported, 1, 2, "destructuring");
    rejects_with("({ a }) => 1", Unsupported, 1, 2, "destructuring");
    rejects("(eval) => 1", ReservedWord, 1, 2);
    rejects("arguments => 1", ReservedWord, 1, 1);
    rejects("undefined => 1", ReservedWord, 1, 1);
    rejects("yield => 1", ReservedWord, 1, 1);
    // No own `this`/`arguments` in the subset, inside arrows too.
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
    // Maximal munch: these are single unsupported tokens.
    rejects_with("a>>>=b", Unsupported, 1, 2, ">>>=");
    rejects_with("a ** b", Unsupported, 1, 3, "**");
    rejects_with("a ?? b", Unsupported, 1, 3, "??");
    rejects_with("a?.b", Unsupported, 1, 2, "optional chaining");
    rejects_with("a--b", Unsupported, 1, 2, "increment");
    // HTML-like comments are not comments here.
    rejects_with("a <!-- b", Unsupported, 1, 5, "increment");
    // `++` after a line break begins the next statement.
    rejects_with("x = a\n++b", Unsupported, 2, 1, "increment");
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
    rejects_with("'\\uD800'", Unsupported, 1, 2, "surrogate");
    rejects_with("'\\uDC00\\uD800'", Unsupported, 1, 2, "surrogate");
    rejects("'abc\ndef'", InvalidToken, 1, 1);
    // Characters and constructs that are not tokens of the subset.
    rejects_with("x = `t`;", Unsupported, 1, 5, "template");
    rejects("x = @y;", UnknownToken, 1, 5);
    // A hashbang is a comment only at the start of the source.
    assert_eq!(ast("#!/usr/bin/env node"), "");
    rejects("x;\n#!/usr/bin/env node", UnknownToken, 2, 1);
    rejects("x.#y", UnknownToken, 1, 3);
    rejects("let é = 1;", UnknownToken, 1, 5);
    rejects_with("let \\u0061 = 1;", Unsupported, 1, 5, "escape");
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
        ("var x = 1;", 1, 1, "var"),
        ("for (;;) {}", 1, 1, "for"),
        ("do {} while (x);", 1, 1, "do"),
        ("while (x) { break; }", 1, 13, "break"),
        ("while (x) continue;", 1, 11, "continue"),
        ("switch (x) {}", 1, 1, "switch"),
        ("try {} catch ([e]) {}", 1, 15, "destructuring"),
        ("label: x;", 1, 1, "labelled"),
        ("with (o) {}", 1, 1, "with"),
        ("debugger;", 1, 1, "debugger"),
        ("import x from 'y';", 1, 1, "import"),
        ("export const x = 1;", 1, 1, "export"),
        ("x = import('y');", 1, 5, "import"),
        ("x = new F();", 1, 5, "new"),
        ("x = this;", 1, 5, "this"),
        ("x = super.y;", 1, 5, "super"),
        ("x = typeof y;", 1, 5, "typeof"),
        ("void 0;", 1, 1, "void"),
        ("delete x.y;", 1, 1, "delete"),
        ("x = ~y;", 1, 5, "~"),
        ("++x;", 1, 1, "increment"),
        ("x = {};", 1, 5, "object literals"),
        ("x = [1];", 1, 5, "array literals"),
        ("a[0];", 1, 2, "computed member"),
        ("f(...a);", 1, 3, "spread"),
        ("function f(...a) {}", 1, 12, "rest"),
        ("function f(a = 1) {}", 1, 14, "default parameter"),
        ("let [a] = b;", 1, 5, "destructuring"),
        ("let {a} = b;", 1, 5, "destructuring"),
        ("function f({a}) {}", 1, 12, "destructuring"),
        ("a += 1;", 1, 3, "+="),
        ("a ||= 1;", 1, 3, "||="),
        ("a.b = 1;", 1, 1, "assignment to properties"),
        ("x in y;", 1, 3, "in"),
        ("x instanceof y;", 1, 3, "instanceof"),
        ("a & b | c ^ d;", 1, 3, "&"),
        ("a << 1;", 1, 3, "<<"),
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
        format!("{}{}", "{".repeat(n), "}".repeat(n)),
        format!("{}x;", "if (1) ".repeat(n)),
        format!("{}x;", "while (1) ".repeat(n)),
        format!("{}{}", "function f() {".repeat(n), "}".repeat(n)),
        format!(
            "x = {}1{};",
            "function () { return ".repeat(n),
            "}".repeat(n)
        ),
        format!("x = 1{};", " + 1".repeat(n)),
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

/// For each nesting construct, the deepest accepted input parses within a
/// 2 MiB stack (Rust's default for spawned threads) in this build profile.
/// A stack overflow here aborts the test binary, failing the run.
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
        ("unary", |n| format!("x = {}1;", "!".repeat(n))),
        ("binary", |n| format!("x = 1{};", " + 1".repeat(n))),
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
        ("mixed", |n| {
            format!("x = {}1{};", "a + -(".repeat(n), ")".repeat(n))
        }),
        ("wrapped member chains", |n| {
            let mut s = "a".to_string();
            for _ in 0..n {
                s = format!("({s}).b.b.b.b.b");
            }
            format!("x = {s};")
        }),
    ];
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            for (name, make) in makers {
                let mut deepest = 0;
                for n in 1..=MAX_NESTING {
                    match parse_script(&make(n)) {
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
