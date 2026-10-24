//! Nested calls, recursion and mutual recursion in explicit task-owned VM
//! frames (clj-quickjs-4), through the public embedding API.
//!
//! Early VM tests: programs are hand-assembled bytecode, not JavaScript
//! source support (compiling functions and calls from source is
//! clj-quickjs-21). The JavaScript in each doc comment is what the bytecode
//! does; functions without closures reach themselves through
//! `CurrentFunction` (a named function expression's self-binding) and reach
//! each other through arguments, since captured bindings are clj-quickjs-20.
//!
//! `fib` and `myfunc1`/`myfunc2` adapt `test_closure3` and `test_closure2`
//! from `research/quickjs/tests/test_closure.js` (QuickJS, revision
//! 7d1c13c5d8f8; see `docs/architecture.md` for the attribution):
//!
//! QuickJS is Copyright (c) 2017-2021 Fabrice Bellard and Charlie Gordon,
//! released under the MIT license: Permission is hereby granted, free of
//! charge, to any person obtaining a copy of this software and associated
//! documentation files (the "Software"), to deal in the Software without
//! restriction, including without limitation the rights to use, copy, modify,
//! merge, publish, distribute, sublicense, and/or sell copies of the
//! Software, and to permit persons to whom the Software is furnished to do
//! so, subject to the following conditions: The above copyright notice and
//! this permission notice shall be included in all copies or substantial
//! portions of the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT
//! WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE
//! WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
//! NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
//! LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF
//! CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH
//! THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use std::thread;

use clj_quickjs::{
    Assembler, Budget, Executable, Host, HostCall, Opcode, Primitive, Rooted, RunOutcome, Runtime,
    TaskId, TaskStatus, ValueView, VmError, VmErrorKind, VmLimits,
};

fn num(n: f64) -> Primitive {
    Primitive::Number(n)
}

fn complete(outcome: RunOutcome) -> Rooted {
    match outcome {
        RunOutcome::Complete(v) => v,
        other => panic!("expected Complete, got {other:?}"),
    }
}

fn failed(outcome: RunOutcome) -> VmError {
    match outcome {
        RunOutcome::Failed(e) => e,
        other => panic!("expected Failed, got {other:?}"),
    }
}

/// Run `main` with number arguments to completion and view the result.
fn run(rt: &Runtime, main: &Executable, args: &[f64]) -> ValueView {
    let args: Vec<Rooted> = args.iter().map(|&a| rt.root(num(a))).collect();
    let args: Vec<&Rooted> = args.iter().collect();
    complete(rt.spawn(main, &args).unwrap().run().unwrap()).view()
}

/// `(f) => f(arg1, …)`: an entry that calls child `f` with number arguments.
fn call_with(f: &Executable, args: &[f64]) -> Executable {
    let mut asm = Assembler::new();
    asm.function(f);
    for &a in args {
        asm.constant(num(a));
    }
    asm.call(args.len() as u16).op(Opcode::Return);
    asm.finish("main", 0, 0).unwrap()
}

/// `function add(a, b) { return a + b; }`
fn add() -> Executable {
    let mut asm = Assembler::new();
    asm.get_local(0)
        .get_local(1)
        .op(Opcode::Add)
        .op(Opcode::Return);
    asm.finish("add", 2, 2).unwrap()
}

/// `var fib1 = function fib1(n) { if (n <= 0) return 0; else if (n == 1)
/// return 1; else return fib1(n - 1) + fib1(n - 2); };` (`test_closure3`)
fn fib() -> Executable {
    use Opcode::*;
    let mut asm = Assembler::new();
    let (not_le0, not_one) = (asm.label(), asm.label());
    asm.get_local(0)
        .constant(num(0.0))
        .op(Le)
        .jump_if_false(not_le0);
    asm.constant(num(0.0)).op(Return);
    asm.bind(not_le0);
    asm.get_local(0)
        .constant(num(1.0))
        .op(LooseEq)
        .jump_if_false(not_one);
    asm.constant(num(1.0)).op(Return);
    asm.bind(not_one);
    asm.op(CurrentFunction)
        .get_local(0)
        .constant(num(1.0))
        .op(Sub)
        .call(1);
    asm.op(CurrentFunction)
        .get_local(0)
        .constant(num(2.0))
        .op(Sub)
        .call(1);
    asm.op(Add).op(Return);
    asm.finish("fib1", 1, 1).unwrap()
}

#[test]
fn nested_call_runs_in_a_new_frame_and_returns_to_the_caller() {
    use Opcode::*;
    // function double(x) { return add(x, x); }
    let add = add();
    let mut asm = Assembler::new();
    asm.function(&add)
        .get_local(0)
        .get_local(0)
        .call(2)
        .op(Return);
    let double = asm.finish("double", 1, 1).unwrap();
    // function main(x) { return 1 + double(x); }: the 1 stays on main's
    // operand stack, below the call, across two nested frames.
    let mut asm = Assembler::new();
    asm.constant(num(1.0))
        .function(&double)
        .get_local(0)
        .call(1)
        .op(Add)
        .op(Return);
    let main = asm.finish("main", 1, 1).unwrap();
    let rt = Runtime::new();
    assert_eq!(run(&rt, &main, &[20.0]), ValueView::Number(41.0));
    // The tree of function code is shared, not copied, by every load.
    assert!(Executable::same_code(&main.function(0).unwrap(), &double));
    assert!(Executable::same_code(&double.function(0).unwrap(), &add));
    assert!(main.function(1).is_none());
}

#[test]
fn recursion_through_the_current_function() {
    let rt = Runtime::new();
    let fib = fib();
    assert_eq!(
        run(&rt, &call_with(&fib, &[6.0]), &[]),
        ValueView::Number(8.0)
    );
    assert_eq!(
        run(&rt, &call_with(&fib, &[15.0]), &[]),
        ValueView::Number(610.0)
    );
    // The entry frame can recurse too: its function object is materialized
    // by `CurrentFunction` on first use.
    assert_eq!(
        run(&rt, &fib, &[10.0]),
        ValueView::Number(55.0),
        "fib1 as the task entry"
    );
}

#[test]
fn deep_recursion_never_uses_the_rust_stack() {
    use Opcode::*;
    // function sum(n) { return n === 0 ? 0 : n + sum(n - 1); }
    let mut asm = Assembler::new();
    let rec = asm.label();
    asm.get_local(0)
        .constant(num(0.0))
        .op(StrictEq)
        .jump_if_false(rec);
    asm.constant(num(0.0)).op(Return);
    asm.bind(rec);
    asm.get_local(0)
        .op(CurrentFunction)
        .get_local(0)
        .constant(num(1.0))
        .op(Sub)
        .call(1)
        .op(Add)
        .op(Return);
    let sum = asm.finish("sum", 1, 1).unwrap();
    let rt = Runtime::new();
    let main = call_with(&sum, &[9_000.0]);
    // 9,001 frames on a worker with a 64 KiB Rust stack: the interpreter
    // loop never recurses, so this cannot overflow it.
    let result = thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || run(&rt, &main, &[]))
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(result, ValueView::Number(9_000.0 * 9_001.0 / 2.0));
}

#[test]
fn mutual_recursion_through_function_arguments() {
    use Opcode::*;
    // function even(n, odd) { if (n === 0) return true; return odd(n - 1, even); }
    // function odd(n, even) { if (n === 0) return false; return even(n - 1, odd); }
    let parity = |name: &str, at_zero: bool| {
        let mut asm = Assembler::new();
        let rec = asm.label();
        asm.get_local(0)
            .constant(num(0.0))
            .op(StrictEq)
            .jump_if_false(rec);
        asm.constant(Primitive::Bool(at_zero)).op(Return);
        asm.bind(rec);
        asm.get_local(1)
            .get_local(0)
            .constant(num(1.0))
            .op(Sub)
            .op(CurrentFunction)
            .call(2)
            .op(Return);
        asm.finish(name, 2, 2).unwrap()
    };
    let (even, odd) = (parity("even", true), parity("odd", false));
    // function main(n) { return even(n, odd); }
    let mut asm = Assembler::new();
    asm.function(&even)
        .get_local(0)
        .function(&odd)
        .call(2)
        .op(Return);
    let main = asm.finish("main", 1, 1).unwrap();
    let rt = Runtime::new();
    assert_eq!(run(&rt, &main, &[10.0]), ValueView::Bool(true));
    assert_eq!(run(&rt, &main, &[7.0]), ValueView::Bool(false));
    assert_eq!(run(&rt, &main, &[0.0]), ValueView::Bool(true));

    // test_closure2: `myfunc1(n) { function myfunc2(n) { return myfunc1(n - 1); }
    // if (n == 0) return 0; else return myfunc2(n); }`, with myfunc2's
    // capture of myfunc1 passed as an argument instead.
    let mut asm = Assembler::new();
    asm.get_local(0)
        .get_local(1)
        .constant(num(1.0))
        .op(Sub)
        .call(1)
        .op(Return);
    let myfunc2 = asm.finish("myfunc2", 2, 2).unwrap();
    let mut asm = Assembler::new();
    let other = asm.label();
    asm.get_local(0)
        .constant(num(0.0))
        .op(LooseEq)
        .jump_if_false(other);
    asm.constant(num(0.0)).op(Return);
    asm.bind(other);
    asm.function(&myfunc2)
        .op(CurrentFunction)
        .get_local(0)
        .call(2)
        .op(Return);
    let myfunc1 = asm.finish("myfunc1", 1, 1).unwrap();
    assert_eq!(
        run(&rt, &call_with(&myfunc1, &[1.0]), &[]),
        ValueView::Number(0.0)
    );
    assert_eq!(
        run(&rt, &call_with(&myfunc1, &[25.0]), &[]),
        ValueView::Number(0.0)
    );
}

/// Records every leaf effect as (task, op, argument).
#[derive(Default)]
struct Recorder {
    calls: Vec<(TaskId, u16, f64)>,
}

impl Host for Recorder {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        let Primitive::Number(n) = call.arg else {
            return Err("number expected".into());
        };
        self.calls.push((call.task, call.op, n));
        Ok(Primitive::Undefined)
    }
}

impl Recorder {
    fn of(&self, task: TaskId) -> Vec<(u16, f64)> {
        self.calls
            .iter()
            .filter(|c| c.0 == task)
            .map(|c| (c.1, c.2))
            .collect()
    }
}

/// The effects `walk(n)` performs: op 0 on the way down, op 1 on the way up.
fn walk_effects(n: u32) -> Vec<(u16, f64)> {
    let down = (0..=n).rev().map(|i| (0, f64::from(i)));
    let up = (0..=n).map(|i| (1, f64::from(i)));
    down.chain(up).collect()
}

/// ```js
/// function walk(n) {
///   const v = [n];             // a heap value held only by this frame
///   effect0(n);
///   const r = n === 0 ? 0 : walk(n - 1);
///   effect1(n);
///   return r + v[0];
/// }
/// ```
fn walk() -> Executable {
    use Opcode::*;
    let mut asm = Assembler::new();
    let (rec, done) = (asm.label(), asm.label());
    asm.get_local(0).new_vector(1).set_local(1);
    asm.get_local(0).host_effect(0).op(Pop);
    asm.get_local(0)
        .constant(num(0.0))
        .op(StrictEq)
        .jump_if_false(rec);
    asm.constant(num(0.0)).set_local(2).jump(done);
    asm.bind(rec);
    asm.op(CurrentFunction)
        .get_local(0)
        .constant(num(1.0))
        .op(Sub)
        .call(1)
        .set_local(2);
    asm.bind(done);
    asm.get_local(0).host_effect(1).op(Pop);
    asm.get_local(2)
        .get_local(1)
        .constant(num(0.0))
        .op(Nth)
        .op(Add)
        .op(Return);
    asm.finish("walk", 1, 3).unwrap()
}

/// `function main(n) { return walk(n); }`
fn walk_main(walk: &Executable) -> Executable {
    let mut asm = Assembler::new();
    asm.function(walk).get_local(0).call(1).op(Opcode::Return);
    asm.finish("main", 1, 1).unwrap()
}

#[test]
fn yield_and_collect_with_live_frames_then_resume_without_replay() {
    let rt = Runtime::new();
    let main = walk_main(&walk());
    let n = rt.root(num(4.0));
    let task = rt.spawn(&main, &[&n]).unwrap();
    drop(n);
    let mut host = Recorder::default();
    // One instruction per run, a full collection after every yield, and
    // alternating OS threads: every frame boundary is a yield point, and each
    // frame's vector is reachable only from that frame's locals.
    let (mut runs, mut most_live) = (0, 0);
    let result = loop {
        runs += 1;
        assert!(runs < 1_000, "task did not finish");
        let outcome = thread::scope(|s| {
            s.spawn(|| task.run_with(Budget::instructions(1), &mut host))
                .join()
                .unwrap()
        })
        .unwrap();
        match outcome {
            RunOutcome::Yielded => {
                assert_eq!(task.status(), TaskStatus::Ready);
                rt.collect().unwrap();
                most_live = most_live.max(rt.heap_stats().objects);
            }
            RunOutcome::Complete(v) => break v.view(),
            other => panic!("{other:?}"),
        }
    };
    // 4 + 3 + 2 + 1 + 0, read back from vectors that survived ~100
    // collections while their frames were suspended.
    assert_eq!(result, ValueView::Number(10.0));
    assert_eq!(host.of(task.id()), walk_effects(4), "each effect ran once");
    // At the deepest point: five vectors and the one function object every
    // walk frame was called through (`CurrentFunction` in a called frame is
    // its callee, not a new object).
    assert_eq!(most_live, 6);
    rt.collect().unwrap();
    assert_eq!(rt.heap_stats().objects, 0, "finished frames release values");
}

#[test]
fn tasks_running_the_same_function_have_independent_frames() {
    let rt = Runtime::new();
    let walk = walk();
    let main = walk_main(&walk);
    let (n4, n6) = (rt.root(num(4.0)), rt.root(num(6.0)));
    let a = rt.spawn(&main, &[&n4]).unwrap();
    let b = rt.spawn(&main, &[&n6]).unwrap();
    // One loaded copy of each function, whichever task runs it.
    assert!(Executable::same_code(a.executable(), b.executable()));
    assert!(Executable::same_code(&main.function(0).unwrap(), &walk));
    let mut host = Recorder::default();
    let (mut ra, mut rb) = (None, None);
    let mut runs = 0;
    while ra.is_none() || rb.is_none() {
        runs += 1;
        assert!(runs < 1_000, "tasks did not finish");
        // Interleave with different budgets so the two tasks are at
        // different depths and positions of the same code.
        for (task, slot, budget) in [(&a, &mut ra, 3), (&b, &mut rb, 5)] {
            if slot.is_some() {
                continue;
            }
            match task.run_with(Budget::instructions(budget), &mut host) {
                Ok(RunOutcome::Yielded) => {}
                Ok(RunOutcome::Complete(v)) => *slot = Some(v.view()),
                other => panic!("{other:?}"),
            }
        }
        rt.collect().unwrap();
    }
    assert_eq!(ra, Some(ValueView::Number(10.0)));
    assert_eq!(rb, Some(ValueView::Number(21.0)));
    assert_eq!(host.of(a.id()), walk_effects(4));
    assert_eq!(host.of(b.id()), walk_effects(6));
}

#[test]
fn a_function_object_is_shareable_and_callable_from_another_task() {
    let rt = Runtime::new();
    let add = add();
    // Task A returns a function object.
    let mut asm = Assembler::new();
    asm.function(&add).op(Opcode::Return);
    let make = asm.finish("make", 0, 0).unwrap();
    let f = complete(rt.spawn(&make, &[]).unwrap().run().unwrap());
    assert_eq!(f.view(), ValueView::Object);
    rt.publish(&f)
        .expect("a function without captures is shareable");
    // Task B calls it with its own frames.
    let mut asm = Assembler::new();
    asm.get_local(0)
        .get_local(1)
        .get_local(2)
        .call(2)
        .op(Opcode::Return);
    let apply = asm.finish("apply", 3, 3).unwrap();
    let (x, y) = (rt.root(num(2.0)), rt.root(num(40.0)));
    let r = complete(rt.spawn(&apply, &[&f, &x, &y]).unwrap().run().unwrap());
    assert_eq!(r.view(), ValueView::Number(42.0));
}

#[test]
fn function_objects_have_identity() {
    use Opcode::*;
    let add = add();
    let rt = Runtime::new();
    // Each `Function` evaluation is a new object, like a function expression.
    let mut asm = Assembler::new();
    asm.function(&add).function(&add).op(StrictEq).op(Return);
    let two = asm.finish("two", 0, 0).unwrap();
    assert_eq!(run(&rt, &two, &[]), ValueView::Bool(false));
    // The entry frame's function object is allocated once and kept by the
    // frame, across a yield and a collection.
    let mut asm = Assembler::new();
    asm.op(CurrentFunction).op(Pop).op(CurrentFunction);
    asm.op(CurrentFunction).op(StrictEq).op(Return);
    let same = asm.finish("same", 0, 0).unwrap();
    let rt = Runtime::new();
    let task = rt.spawn(&same, &[]).unwrap();
    let mut host = Recorder::default();
    assert!(matches!(
        task.run_with(Budget::instructions(2), &mut host).unwrap(),
        RunOutcome::Yielded
    ));
    assert_eq!(rt.collect().unwrap().reclaimed, 0);
    assert_eq!(rt.heap_stats().objects, 1, "held only by the frame");
    let r = complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap());
    assert_eq!(r.view(), ValueView::Bool(true));
}

#[test]
fn a_callee_reachable_only_from_its_frame_survives_collection() {
    use Opcode::*;
    // function inner() { return inner; } (after one no-op instruction)
    let mut asm = Assembler::new();
    asm.op(Undefined).op(Pop).op(CurrentFunction).op(Return);
    let inner = asm.finish("inner", 0, 0).unwrap();
    let main = call_with(&inner, &[]);
    let rt = Runtime::new();
    let task = rt.spawn(&main, &[]).unwrap();
    let mut host = Recorder::default();
    // `Function` and `Call` run; the callee is now off every operand stack.
    assert!(matches!(
        task.run_with(Budget::instructions(2), &mut host).unwrap(),
        RunOutcome::Yielded
    ));
    let stats = rt.collect().unwrap();
    assert_eq!(stats.reclaimed, 0);
    assert_eq!(rt.heap_stats().objects, 1);
    let f = complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap());
    assert_eq!(f.view(), ValueView::Object);
    // The returned object is the one that was called: calling it again
    // returns itself.
    let mut asm = Assembler::new();
    asm.get_local(0)
        .call(0)
        .get_local(0)
        .op(StrictEq)
        .op(Return);
    let check = asm.finish("check", 1, 1).unwrap();
    let r = complete(rt.spawn(&check, &[&f]).unwrap().run().unwrap());
    assert_eq!(r.view(), ValueView::Bool(true));
}

#[test]
fn missing_arguments_are_undefined_and_extra_ones_are_dropped() {
    use Opcode::*;
    // function second(a, b) { return b; }
    let mut asm = Assembler::new();
    asm.get_local(1).op(Return);
    let second = asm.finish("second", 2, 2).unwrap();
    // function hidden(a) { let c; return c; }: an extra argument must not
    // leak into a non-parameter local.
    let mut asm = Assembler::new();
    asm.get_local(1).op(Return);
    let hidden = asm.finish("hidden", 1, 2).unwrap();
    let rt = Runtime::new();
    assert_eq!(
        run(&rt, &call_with(&second, &[1.0]), &[]),
        ValueView::Undefined
    );
    assert_eq!(
        run(&rt, &call_with(&second, &[]), &[]),
        ValueView::Undefined
    );
    assert_eq!(
        run(&rt, &call_with(&second, &[1.0, 2.0, 3.0]), &[]),
        ValueView::Number(2.0)
    );
    assert_eq!(
        run(&rt, &call_with(&hidden, &[1.0, 2.0]), &[]),
        ValueView::Undefined
    );
    // The caller's operands below the callee are untouched: 100 + second(1, 2, 3).
    let mut asm = Assembler::new();
    asm.constant(num(100.0))
        .function(&second)
        .constant(num(1.0))
        .constant(num(2.0))
        .constant(num(3.0))
        .call(3)
        .op(Add)
        .op(Return);
    let main = asm.finish("main", 0, 0).unwrap();
    assert_eq!(run(&rt, &main, &[]), ValueView::Number(102.0));
}

#[test]
fn calling_a_non_function_is_a_type_error() {
    use Opcode::*;
    let rt = Runtime::new();
    type PushCallee = fn(&mut Assembler);
    let cases: [(&str, PushCallee); 7] = [
        ("undefined", |a| {
            a.op(Undefined);
        }),
        ("boolean", |a| {
            a.constant(Primitive::Bool(true));
        }),
        ("null", |a| {
            a.constant(Primitive::Null);
        }),
        ("number", |a| {
            a.constant(num(5.0));
        }),
        ("object", |a| {
            a.op(NewObject);
        }),
        ("vector", |a| {
            a.new_vector(0);
        }),
        ("vector builder", |a| {
            a.op(NewVectorBuilder);
        }),
    ];
    for (name, push_callee) in cases {
        let mut asm = Assembler::new();
        push_callee(&mut asm);
        asm.constant(num(1.0)).call(1).op(Return);
        let exe = asm.finish("main", 0, 0).unwrap();
        let err = failed(rt.spawn(&exe, &[]).unwrap().run().unwrap());
        assert_eq!(err.kind(), VmErrorKind::TypeError, "{name}");
        assert_eq!(err.message(), format!("{name} is not a function"));
        assert_eq!(err.diagnostic().opcode, Some(Opcode::Call));
        assert!(err.diagnostic().call_chain.is_empty());
    }
}

#[test]
fn a_failure_reports_the_call_chain() {
    use Opcode::*;
    // function inner(x) { return x + 1; } called with an object.
    let mut asm = Assembler::new();
    asm.get_local(0).constant(num(1.0)).op(Add).op(Return);
    let inner = asm.finish("inner", 1, 1).unwrap();
    // function middle() { return inner({}); }
    let mut asm = Assembler::new();
    asm.function(&inner).op(NewObject).call(1).op(Return);
    let middle = asm.finish("middle", 0, 0).unwrap();
    // function main() { undefined; return middle(); }
    let mut asm = Assembler::new();
    asm.op(Undefined)
        .op(Pop)
        .function(&middle)
        .call(0)
        .op(Return);
    let main = asm.finish("main", 0, 0).unwrap();
    let rt = Runtime::new();
    let task = rt.spawn(&main, &[]).unwrap();
    let err = failed(task.run().unwrap());
    assert_eq!(err.kind(), VmErrorKind::UnsupportedOperand);
    let d = err.diagnostic();
    assert_eq!((&*d.executable, d.instruction), ("inner", 2));
    assert_eq!(d.opcode, Some(Opcode::Add));
    assert_eq!((d.task, d.world), (task.id(), rt.world()));
    let chain: Vec<(&str, usize, Option<u32>)> = d
        .call_chain
        .iter()
        .map(|c| (&*c.executable, c.instruction, c.byte_offset))
        .collect();
    // middle's `call` is instruction 2 at byte 4; main's is instruction 3 at byte 5.
    assert_eq!(chain, [("middle", 2, Some(4)), ("main", 3, Some(5))]);
    let text = err.to_string();
    assert!(text.contains("in inner at instruction 2"), "{text}");
    assert!(
        text.contains("called from middle at instruction 2"),
        "{text}"
    );
    assert!(text.contains("called from main at instruction 3"), "{text}");
    assert_eq!(task.status(), TaskStatus::Finished);
}

/// `function forever() { return forever(); }`
fn forever(locals: u16) -> Executable {
    let mut asm = Assembler::new();
    asm.op(Opcode::CurrentFunction).call(0).op(Opcode::Return);
    asm.finish("forever", 0, locals).unwrap()
}

#[test]
fn exceeding_the_call_depth_bound_fails_the_task() {
    let limits = VmLimits {
        max_call_depth: 50,
        ..VmLimits::default()
    };
    let rt = Runtime::with_limits(limits);
    let task = rt.spawn(&forever(0), &[]).unwrap();
    let err = failed(task.run().unwrap());
    assert_eq!(err.kind(), VmErrorKind::StackOverflow);
    assert_eq!(err.diagnostic().opcode, Some(Opcode::Call));
    // The 50th frame's call is refused: 49 callers plus the failing frame.
    assert_eq!(err.diagnostic().call_chain.len(), 49);
    assert!(err.message().contains("50 frames"), "{}", err.message());
    assert_eq!(task.status(), TaskStatus::Finished);
    let text = err.to_string();
    assert!(text.contains("… 33 more callers"), "{text}");

    // A depth of 1 allows no calls at all.
    let rt = Runtime::with_limits(VmLimits {
        max_call_depth: 1,
        ..VmLimits::default()
    });
    let err = failed(rt.spawn(&forever(0), &[]).unwrap().run().unwrap());
    assert_eq!(err.kind(), VmErrorKind::StackOverflow);
    assert!(err.diagnostic().call_chain.is_empty());
}

#[test]
fn unbounded_recursion_under_default_limits_fails_without_a_rust_stack_overflow() {
    let rt = Runtime::new();
    let task = rt.spawn(&forever(0), &[]).unwrap();
    let err = thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || failed(task.run().unwrap()))
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(err.kind(), VmErrorKind::StackOverflow);
    assert_eq!(
        err.diagnostic().call_chain.len(),
        VmLimits::DEFAULT.max_call_depth - 1
    );
}

#[test]
fn exceeding_the_stack_value_bound_fails_the_task() {
    // Each frame of `forever` reserves 1,000 locals.
    let rt = Runtime::with_limits(VmLimits {
        max_call_depth: 1_000,
        max_stack_values: 10_000,
    });
    let err = failed(rt.spawn(&forever(1_000), &[]).unwrap().run().unwrap());
    assert_eq!(err.kind(), VmErrorKind::StackOverflow);
    // The entry frame plus 8 called frames fit (9,000 locals and the
    // operand slot of each); the 10th frame would not.
    assert_eq!(err.diagnostic().call_chain.len(), 8);
    assert!(err.message().contains("10000"), "{}", err.message());
}
