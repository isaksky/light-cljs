//! Malformed-input rejection (clj-quickjs-30): truncated sources, the
//! documented rejected forms, reserved-binding misuse and bytecode-validator
//! boundaries must all fail safely, with a positioned diagnostic and never a
//! panic. Sources here are synthetic fixtures, not light-cljs output; the
//! truncation sweep reuses the adapted QuickJS corpus fixtures (see their
//! headers and `docs/attribution.md` for the upstream notice).

use big_boy_pants_js::frontend::{
    ScriptOptions, SyntaxError, SyntaxErrorKind, compile_script_with, parse_module,
};
use big_boy_pants_js::{
    Assembler, Budget, ConstructorKind, Executable, ExecutableSpec, Host, HostCall, Opcode,
    Primitive, RunOutcome, Runtime, ValidationError, ValidationErrorKind, VmErrorKind,
};

struct NoEffects;

impl Host for NoEffects {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        Err(format!("unexpected host effect {}", call.op))
    }
}

/// Every diagnostic points inside its source, at a character boundary, with
/// a line/column that exists and a non-empty message.
fn check_diagnostic(source: &str, error: &SyntaxError) {
    assert!(
        error.span.start <= error.span.end && error.span.end <= source.len(),
        "{source:?}: span {:?} outside the source: {error}",
        error.span
    );
    assert!(
        source.is_char_boundary(error.span.start) && source.is_char_boundary(error.span.end),
        "{source:?}: span {:?} splits a character: {error}",
        error.span
    );
    let lines = source.lines().count().max(1) as u32;
    assert!(
        (1..=lines + 1).contains(&error.line),
        "{source:?}: line {} of {lines}: {error}",
        error.line
    );
    assert!(error.column >= 1, "{source:?}: {error}");
    assert!(!error.message.is_empty(), "{source:?}: {error}");
}

/// Compiles `source` (sloppy, so directive prologues decide strictness);
/// a successful compilation then runs for a bounded number of instructions.
/// Returns whether the prefix compiled.
fn compile_and_run_bounded(name: &str, source: &str) -> bool {
    match compile_script_with(name, source, ScriptOptions::SLOPPY) {
        Err(error) => {
            check_diagnostic(source, &error);
            false
        }
        Ok(executable) => {
            let runtime = Runtime::new();
            let task = runtime.spawn(&executable, &[]).unwrap();
            // Any outcome is acceptable: a prefix may complete, fail with a
            // catchable error, or exhaust its slice inside an upstream loop.
            let _ = task
                .run_with(Budget::instructions(20_000), &mut NoEffects)
                .unwrap();
            true
        }
    }
}

/// Every line-boundary prefix of each corpus fixture compiles or is rejected
/// with a positioned SyntaxError; compiled prefixes run bounded without a
/// panic. Truncation inside functions, literals, templates, classes, regexp
/// literals and comments is all exercised by the upstream sources.
#[test]
fn truncated_corpus_sources_are_rejected_or_run_safely() {
    let fixtures = [
        (
            "operators",
            include_str!("fixtures/js/quickjs_test_language_operators.js"),
        ),
        (
            "parsing",
            include_str!("fixtures/js/quickjs_test_language_parsing.js"),
        ),
        (
            "closure",
            include_str!("fixtures/js/quickjs_test_closure.js"),
        ),
        (
            "loops",
            include_str!("fixtures/js/quickjs_test_loop_statements.js"),
        ),
        (
            "builtins",
            include_str!("fixtures/js/quickjs_test_builtin_builtins.js"),
        ),
    ];
    let (mut compiled, mut rejected) = (0, 0);
    for (name, fixture) in fixtures {
        let (_, body) = fixture.split_once("*/\n").expect("license header");
        let mut end = 0;
        for line in body.split_inclusive('\n') {
            end += line.len();
            if compile_and_run_bounded(name, &body[..end]) {
                compiled += 1;
            } else {
                rejected += 1;
            }
        }
    }
    // Most prefixes end inside a function body; the rest are whole programs.
    assert!(
        rejected > compiled,
        "{rejected} rejected, {compiled} compiled"
    );
    assert!(compiled >= 50, "{compiled} prefixes compiled");
}

/// Compiles and runs a complete seed program, which must finish with a
/// number, so every later prefix of it is reachable by the sweep.
fn seed_completes(name: &str, program: &str) {
    let executable = compile_script_with(name, program, ScriptOptions::SLOPPY)
        .unwrap_or_else(|error| panic!("{name} seed must compile: {error}"));
    let runtime = Runtime::new();
    let task = runtime.spawn(&executable, &[]).unwrap();
    match task
        .run_with(Budget::instructions(100_000), &mut NoEffects)
        .unwrap()
    {
        RunOutcome::Complete(value) => {
            assert!(
                matches!(value.view(), big_boy_pants_js::ValueView::Number(_)),
                "{name} seed completed with {:?}",
                value.view()
            );
        }
        other => panic!("{name} seed must complete: {other:?}"),
    }
}

/// Every character-boundary prefix of two complete, fully supported programs
/// (one strict, one sloppy) is rejected at the end of input with a kind a
/// truncation can produce, or compiles and runs bounded. Because the seeds
/// use no rejected form, no prefix may stop early at an `Unsupported` token:
/// the sweep reaches every later construct (generator, async function,
/// labelled loops, switch, regexp literal, catch pattern, `with`, legacy
/// octals).
#[test]
fn truncated_program_at_every_character_is_safe() {
    let strict = "'use strict';\n\
        let total = 0x1F + 0b11 - 0o7 + 1.5e1 + Number(10n ** 2n);\n\
        const text = `a${total}b` + \"\\u{1F600}\\x41\" + 'q\\'r';\n\
        class Point { constructor(x = 1, [y] = [2]) { this.x = x; this.y = y; } get sum() { return this.x + this.y; } static of(x, y) { return new Point(x, [y]); } }\n\
        function* gen() { try { yield 1; } finally { total++; } }\n\
        for (const v of gen()) { total += v + Point.of(1, 2).sum; }\n\
        async function later(p) { await p; return p?.q; }\n\
        label: for (let k in { a: 1, b: 2 }) { if (k === 'a') continue label; else break; }\n\
        switch (total) { case 1: total--; break; default: total = /re[g]+x\\//giu.test(text) ? 1 : 2; }\n\
        try { throw new RangeError('r'); } catch ({ message }) { total += message.length; }\n\
        total;\n";
    let sloppy = "var total = 017 + 08 + \"\\101\".length;\n\
        with ({ depth: 2 }) { total += depth; }\n\
        function f(a, b) { return a + b + arguments.length; }\n\
        total += f(1, 2);\n\
        label: do { total--; if (total > 40) continue label; } while (total > 40);\n\
        total;\n";
    for (name, program) in [("strict", strict), ("sloppy", sloppy)] {
        seed_completes(name, program);
        let (mut compiled, mut at_end, mut rejected) = (0, 0, 0);
        for end in (1..program.len()).filter(|&end| program.is_char_boundary(end)) {
            let prefix = &program[..end];
            match compile_script_with(name, prefix, ScriptOptions::SLOPPY) {
                Err(error) => {
                    check_diagnostic(prefix, &error);
                    rejected += 1;
                    if error.kind == SyntaxErrorKind::UnexpectedEnd {
                        at_end += 1;
                    }
                    // A cut can leave a token or statement incomplete
                    // (`UnexpectedEnd`, `InvalidToken`, `UnknownToken`), an
                    // early error (`const c` without initializer, `Invalid`)
                    // or a token out of place (`UnexpectedToken`); it never
                    // reaches a rejected form or the nesting limit.
                    assert!(
                        matches!(
                            error.kind,
                            SyntaxErrorKind::UnexpectedEnd
                                | SyntaxErrorKind::InvalidToken
                                | SyntaxErrorKind::UnknownToken
                                | SyntaxErrorKind::Invalid
                                | SyntaxErrorKind::UnexpectedToken
                        ),
                        "{name} {prefix:?}: {error}"
                    );
                }
                Ok(executable) => {
                    compiled += 1;
                    let runtime = Runtime::new();
                    let task = runtime.spawn(&executable, &[]).unwrap();
                    let _ = task
                        .run_with(Budget::instructions(5_000), &mut NoEffects)
                        .unwrap();
                }
            }
        }
        // Statement boundaries compile; most cuts end inside a statement.
        assert!(compiled >= 3, "{name}: {compiled} prefixes compiled");
        assert!(
            at_end * 2 > rejected,
            "{name}: {at_end} of {rejected} rejections at the end of input"
        );
    }
}

/// Module-only rejected forms (`docs/grammar.md`, "Rejected forms") fail in
/// the module goal with their documented kinds; `await using` at module top
/// level is a SyntaxError of either documented kind.
#[test]
fn rejected_module_forms_fail_with_documented_kinds() {
    use SyntaxErrorKind::{Invalid, Unsupported};
    let rows = [
        ("import {'x' as x} from 's';", Unsupported),
        ("let x; export {x as 'name'};", Unsupported),
        ("import 's' with {};", Unsupported),
        ("import {a} from 's' with { type: 'json' };", Unsupported),
        ("x = import.meta;", Unsupported),
        ("await 1;", Unsupported),
        ("import('s');", Unsupported),
        ("export let a; a ??= b || c ?? d;", Invalid),
        ("export const c;", Invalid),
    ];
    for (source, kind) in rows {
        let error = parse_module(source).expect_err(source);
        check_diagnostic(source, &error);
        assert_eq!(error.kind, kind, "{source:?}: {error}");
    }
    // Logical assignment is accepted since clj-quickjs-314.
    parse_module("export let a; a ??= 1;").unwrap();
    // Object spread is accepted since clj-quickjs-315.
    parse_module("export const x = 1; x = {...o};").unwrap();
    let error = parse_module("await using x = r;").expect_err("await using");
    check_diagnostic("await using x = r;", &error);
    assert!(
        matches!(error.kind, Invalid | Unsupported),
        "await using: {error}"
    );
}

/// Sources over the nesting budget are rejected as `TooDeep` through
/// compilation, including truncated deep input, without overflowing the stack.
#[test]
fn overdeep_sources_fail_with_too_deep() {
    let n = 20_000;
    for source in [
        format!("x = {}1{};", "(".repeat(n), ")".repeat(n)),
        format!("{}{}", "{".repeat(n), "}".repeat(n)),
        format!("{}x;", "if (1) ".repeat(n)),
        format!("x = {}1{};", "[".repeat(n), "]".repeat(n)),
        format!("x = 1{};", " + 1".repeat(n)),
        "(".repeat(n),
    ] {
        let error = compile_script_with("deep", &source, ScriptOptions::SLOPPY)
            .expect_err("over-deep source must be rejected");
        check_diagnostic(&source, &error);
        assert_eq!(
            error.kind,
            SyntaxErrorKind::TooDeep,
            "{}: {error}",
            &source[..20]
        );
    }
}

/// The documented rejected forms (`docs/grammar.md`, "Rejected forms") fail
/// at compilation with the documented error kind. Each row is a complete
/// script; all rows are exercised
/// through `compile_script_with`, not the parser alone.
#[test]
fn rejected_forms_fail_with_documented_kinds() {
    const SLOPPY: ScriptOptions = ScriptOptions::SLOPPY;
    const STRICT: ScriptOptions = ScriptOptions::STRICT;
    use SyntaxErrorKind::{Invalid, ReservedWord, UnexpectedToken, Unsupported};
    let rows: &[(&str, ScriptOptions, SyntaxErrorKind)] = &[
        // using declarations
        ("using x = r;", STRICT, Invalid),
        ("using x = r;", SLOPPY, Invalid),
        ("switch (1) { case 1: using y = r; }", STRICT, Invalid),
        ("{ using x = r, y; }", STRICT, Invalid),
        ("for (using x in o) {}", STRICT, Invalid),
        ("for (using [x] of a) {}", STRICT, Invalid),
        ("for (using x = r; ;) {}", STRICT, Unsupported),
        (
            "async function f() { for (await using x = a; ;) {} }",
            STRICT,
            Invalid,
        ),
        ("await using x = r;", STRICT, Unsupported),
        // statements outside the subset
        ("for (a.b of o) {}", STRICT, Unsupported),
        ("debugger;", STRICT, Unsupported),
        ("'use strict'; with (o) {}", SLOPPY, Invalid),
        ("class A { static { return; } }", STRICT, Invalid),
        ("class A { m() { return this.#x; } }", STRICT, Invalid),
        ("import x from 'y';", STRICT, Unsupported),
        ("export const x = 1;", STRICT, Unsupported),
        ("x = import('y');", STRICT, Unsupported),
        ("x = import.meta;", STRICT, Unsupported),
        ("() => new.target", STRICT, Invalid),
        ("'use strict'; for (var k = 1 in o) {}", SLOPPY, Invalid),
        ("for (let k = 1 in o) {}", SLOPPY, Invalid),
        ("for (var k = 1 of o) {}", SLOPPY, Invalid),
        // expressions outside the subset
        ("a?.b = 1;", STRICT, Invalid),
        ("a?.b++;", STRICT, Invalid),
        ("new a?.b();", STRICT, Invalid),
        ("x = {[k]};", STRICT, UnexpectedToken),
        ("x = 1__000;", STRICT, SyntaxErrorKind::InvalidToken),
        ("x = a ?? b || c;", STRICT, Invalid),
        ("a &&= b ?? c && d;", STRICT, Invalid),
        ("a ||= b || c ?? d;", STRICT, Invalid),
        ("a ??= b && c ?? d;", STRICT, Invalid),
        ("a.b &&= b ?? c || d;", STRICT, Invalid),
        ("x = super.y;", STRICT, Invalid),
        ("super();", STRICT, Invalid),
        ("x = super?.y;", STRICT, Invalid),
        ("delete x;", STRICT, Invalid),
        ("x = {if};", STRICT, ReservedWord),
        ("x = {1};", STRICT, UnexpectedToken),
        ("({a = 1});", STRICT, Invalid),
        ("x = {__proto__: 1, __proto__: 2};", STRICT, Invalid),
        ("x = {__proto__: 1, \"__proto__\": 2};", STRICT, Invalid),
        ("1 + x => x;", STRICT, UnexpectedToken),
        ("() => {}();", STRICT, UnexpectedToken),
        ("x = a\n=> a;", STRICT, UnexpectedToken),
        ("x = -a ** b;", STRICT, Invalid),
        // reserved words and the reserved intrinsic binding
        ("let if = 1;", STRICT, ReservedWord),
        ("undefined += 1;", STRICT, ReservedWord),
        ("NaN++;", STRICT, ReservedWord),
        ("--Infinity;", STRICT, ReservedWord),
        ("let undefined = 1;", STRICT, ReservedWord),
        ("var NaN;", SLOPPY, ReservedWord),
        ("eval = 1;", STRICT, ReservedWord),
        ("arguments = 1;", STRICT, ReservedWord),
        ("let eval;", STRICT, ReservedWord),
        ("function f(eval) { 'use strict'; }", SLOPPY, ReservedWord),
        (
            "function arguments() { 'use strict'; }",
            SLOPPY,
            ReservedWord,
        ),
        ("__rt[x];", STRICT, ReservedWord),
        ("__rt.count = f;", STRICT, ReservedWord),
        ("__rt.count++;", STRICT, ReservedWord),
        ("delete __rt.count;", STRICT, ReservedWord),
        ("let __rt = 1;", STRICT, ReservedWord),
        ("var __rt;", SLOPPY, ReservedWord),
        ("function __rt() {}", STRICT, ReservedWord),
        ("function f(__rt) {}", STRICT, ReservedWord),
        ("__rt = 1;", STRICT, ReservedWord),
        ("x = {__rt};", STRICT, ReservedWord),
        ("class __rt {}", STRICT, ReservedWord),
        ("try {} catch (__rt) {}", STRICT, ReservedWord),
        ("for (const __rt of a) {}", STRICT, ReservedWord),
        // early errors
        ("return 1;", STRICT, Invalid),
        ("break;", STRICT, Invalid),
        ("continue;", STRICT, Invalid),
        ("while (1) { function f() { break; } }", STRICT, Invalid),
        ("switch (x) { default: ; default: ; }", STRICT, Invalid),
        ("const c;", STRICT, Invalid),
        ("function f(a, a) {}", STRICT, Invalid),
        ("(a, a) => 1;", STRICT, Invalid),
        ("if (x) function f() {}", STRICT, Invalid),
        ("while (x) let y = 1;", STRICT, Invalid),
        ("l: l: ;", STRICT, Invalid),
        ("l: { continue l; }", STRICT, Invalid),
        ("while (1) { break l; }", STRICT, Invalid),
        ("for (const c; ;) {}", STRICT, Invalid),
        ("for (let a, b in o) {}", STRICT, Invalid),
        ("for (1 in o) {}", STRICT, Invalid),
        ("1 = 2;", STRICT, Invalid),
        ("1 += 2;", STRICT, Invalid),
        ("f() += 1;", STRICT, Invalid),
        ("1++;", STRICT, Invalid),
        ("f()++;", STRICT, Invalid),
        ("++a++;", STRICT, Invalid),
        ("throw\n1;", STRICT, Invalid),
        ("try {} catch (e) { let e; }", STRICT, Invalid),
        ("try {} catch (e) { function e() {} }", STRICT, Invalid),
    ];
    let mut mismatches = Vec::new();
    for &(source, options, kind) in rows {
        match compile_script_with("rejected", source, options) {
            Ok(_) => mismatches.push(format!("{source:?}: compiled, expected {kind:?}")),
            Err(error) => {
                check_diagnostic(source, &error);
                if error.kind != kind {
                    mismatches.push(format!("{source:?}: {error}, expected {kind:?}"));
                }
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    // Private declarations execute; undeclared references still fail early (-291).
    compile_script_with("accepted-private", "class A { #x; }", STRICT).unwrap();
    // Rest parameters, object spread and pattern heads execute (-315).
    for source in [
        "function f(...a) {}",
        "for (let [a] of o) {}",
        "for (let [a] in o) {}",
        "x = {...o};",
    ] {
        compile_script_with("accepted-rest-spread", source, STRICT).expect(source);
    }
    // The former unsupported rows now compile; keep explicit acceptance
    // checks alongside their replacement mixing errors (clj-quickjs-314).
    for source in [
        "x = a ?? b;",
        "a &&= 1;",
        "a ||= 1;",
        "a ??= 1;",
        "a.b &&= 1;",
    ] {
        compile_script_with("accepted-logical", source, STRICT).unwrap();
    }
}

/// Hand-built bytecode: the validator's error for `code` with the given
/// constant pool and local count.
fn load(
    code: Vec<u8>,
    constants: Vec<Primitive>,
    locals: u16,
) -> Result<Executable, ValidationError> {
    Executable::load(ExecutableSpec {
        name: "malformed".into(),
        params: 0,
        function_length: None,
        function_name: None,
        locals,
        code,
        constants,
        functions: vec![],
        captures: 0,
        strict: true,
        constructor_kind: ConstructorKind::None,
        eval_sites: Vec::new(),
    })
}

fn rejected(code: Vec<u8>, constants: Vec<Primitive>, locals: u16) -> (u32, ValidationErrorKind) {
    let error = load(code, constants, locals).expect_err("malformed bytecode must not load");
    (error.offset, error.kind)
}

/// Early VM test with hand-built bytecode, not JavaScript source support:
/// branch and handler targets must be instruction boundaries, also when they
/// point backwards or at the end of the code, and branch operands must be
/// complete.
#[test]
fn bytecode_branch_and_handler_targets_are_bounded() {
    use ValidationErrorKind::{BadBranchTarget, TruncatedOperand};
    let (c, j, jf, pc, undef, ret) = (
        Opcode::Const as u8,
        Opcode::Jump as u8,
        Opcode::JumpIfFalse as u8,
        Opcode::PushCatch as u8,
        Opcode::Undefined as u8,
        Opcode::Return as u8,
    );
    // Backward target inside the operand of the preceding Const.
    assert_eq!(
        rejected(vec![c, 0, 0, j, 1, 0, 0, 0, ret], vec![Primitive::Null], 0),
        (3, BadBranchTarget { target: 1 })
    );
    // Target exactly one past the end of the code.
    assert_eq!(
        rejected(vec![undef, j, 7, 0, 0, 0, ret], vec![], 0),
        (1, BadBranchTarget { target: 7 })
    );
    // A conditional branch into its own operand.
    assert_eq!(
        rejected(vec![undef, jf, 3, 0, 0, 0, undef, ret], vec![], 0),
        (1, BadBranchTarget { target: 3 })
    );
    // A catch handler target inside the PushCatch operand.
    assert_eq!(
        rejected(vec![pc, 2, 0, 0, 0, undef, ret], vec![], 0),
        (0, BadBranchTarget { target: 2 })
    );
    // Branch operands cut short, with and without a following byte.
    assert_eq!(
        rejected(vec![j, 0, 0], vec![], 0),
        (
            0,
            TruncatedOperand {
                opcode: Opcode::Jump
            }
        )
    );
    assert_eq!(
        rejected(vec![undef, pc, 9], vec![], 0),
        (
            1,
            TruncatedOperand {
                opcode: Opcode::PushCatch
            }
        )
    );
}

/// Early VM test with hand-built bytecode: operand-stack effects are checked
/// along loop back edges and handler entries, and oversized call operands
/// underflow instead of reading past the stack.
#[test]
fn bytecode_stack_effects_are_checked_on_back_edges_and_handlers() {
    use ValidationErrorKind::{StackDepthMismatch, StackUnderflow};
    let (jf, pc, undef, pop, call, cur, ret) = (
        Opcode::JumpIfFalse as u8,
        Opcode::PushCatch as u8,
        Opcode::Undefined as u8,
        Opcode::Pop as u8,
        Opcode::Call as u8,
        Opcode::CurrentFunction as u8,
        Opcode::Return as u8,
    );
    // L: Undefined; Undefined; JumpIfFalse L; Return -- the back edge
    // reaches L one deeper than L's first visit.
    let (offset, kind) = rejected(vec![undef, undef, jf, 0, 0, 0, 0, ret], vec![], 0);
    assert_eq!(
        kind,
        StackDepthMismatch {
            expected: 0,
            found: 1
        }
    );
    assert!(offset <= 7, "offset {offset}");
    // PushCatch L; Undefined; Undefined; L: Pop; Return -- fall-through
    // reaches the catch target at depth 2, the handler entry at depth 1.
    let (_, kind) = rejected(vec![pc, 7, 0, 0, 0, undef, undef, pop, ret], vec![], 0);
    assert!(
        matches!(kind, StackDepthMismatch { .. }),
        "expected a depth mismatch, got {kind:?}"
    );
    // A consistent catch target loads: the handler entry depth is
    // PushCatch's depth plus the exception value.
    load(vec![pc, 7, 0, 0, 0, undef, ret, ret], vec![], 0).unwrap();
    // Call with the maximum argument count on a two-value stack.
    assert_eq!(
        rejected(vec![cur, undef, call, 0xFF, 0xFF, ret], vec![], 0),
        (
            2,
            StackUnderflow {
                opcode: Opcode::Call,
                depth: 2
            }
        )
    );
    // Dup3 needs three values.
    assert_eq!(
        rejected(vec![undef, undef, Opcode::Dup3 as u8, ret], vec![], 0),
        (
            2,
            StackUnderflow {
                opcode: Opcode::Dup3,
                depth: 2
            }
        )
    );
}

/// Early VM test with hand-built bytecode: operand indices are checked
/// against the declared tables, including an empty capture table and a local
/// count of zero.
#[test]
fn bytecode_operand_indices_are_bounded() {
    use ValidationErrorKind::{CaptureOutOfRange, ConstantOutOfRange, LocalOutOfRange};
    let (gc, gl, sl, c, undef, ret) = (
        Opcode::GetCapture as u8,
        Opcode::GetLocal as u8,
        Opcode::SetLocal as u8,
        Opcode::Const as u8,
        Opcode::Undefined as u8,
        Opcode::Return as u8,
    );
    assert_eq!(
        rejected(vec![gc, 0, 0, ret], vec![], 0),
        (0, CaptureOutOfRange { index: 0, count: 0 })
    );
    assert_eq!(
        rejected(vec![gl, 0, 0, ret], vec![], 0),
        (0, LocalOutOfRange { index: 0, count: 0 })
    );
    assert_eq!(
        rejected(vec![undef, sl, 0xFF, 0xFF, undef, ret], vec![], 1),
        (
            1,
            LocalOutOfRange {
                index: 0xFFFF,
                count: 1
            }
        )
    );
    assert_eq!(
        rejected(vec![c, 0xFF, 0xFF, ret], vec![Primitive::Null], 0),
        (
            0,
            ConstantOutOfRange {
                index: 0xFFFF,
                count: 1
            }
        )
    );
    // An empty constant pool rejects even index 0.
    assert_eq!(
        rejected(vec![c, 0, 0, ret], vec![], 0),
        (0, ConstantOutOfRange { index: 0, count: 0 })
    );
}

/// Early VM test with hand-assembled code: a handler instruction that
/// validates but does not match the run-time handler stack fails the task
/// with a controlled `Internal` error instead of panicking.
#[test]
fn mismatched_runtime_handler_instructions_fail_the_task() {
    let runtime = Runtime::new();
    for opcode in [Opcode::PopCatch, Opcode::PopFinally, Opcode::EndFinally] {
        let mut asm = Assembler::new();
        asm.constant(Primitive::Number(1.0))
            .op(opcode)
            .op(Opcode::Return);
        let executable = asm.finish("unmatched", 0, 0).unwrap();
        let outcome = runtime.spawn(&executable, &[]).unwrap().run().unwrap();
        match outcome {
            RunOutcome::Failed(error) => {
                assert_eq!(error.kind(), VmErrorKind::Internal, "{opcode:?}: {error}")
            }
            other => panic!("{opcode:?}: expected a controlled failure, got {other:?}"),
        }
    }
}
