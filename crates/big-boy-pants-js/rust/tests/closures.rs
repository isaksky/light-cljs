//! Lexical closures with captured bindings and publication checks
//! (clj-quickjs-20), through the public embedding API.
//!
//! Early VM tests: programs are hand-assembled bytecode, not JavaScript
//! source support (compiling closures from source is clj-quickjs-21). The
//! JavaScript in each doc comment is what the bytecode does. A binding that a
//! closure captures and that is assigned (or may be read before it is
//! initialized) lives in a binding cell (`NewCell`/`CellInit`/`CellGet`/
//! `CellSet`); a binding that is initialized before capture and never
//! assigned is captured by value. Objects with properties are not supported
//! yet, so an object literal of closures becomes a vector.
//!
//! `closure1` and `closure2` adapt `f` and `test_closure1` from
//! `research/quickjs/tests/test_closure.js` (QuickJS, revision 7d1c13c5d8f8;
//! see `docs/architecture.md` for the attribution). String concatenation in
//! `log` becomes a host effect whose operation names the logged variable:
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

use big_boy_pants_js::{
    Assembler, Budget, Executable, ExecutableSpec, Host, HostCall, Opcode, PathStep, Primitive,
    PublishError, Rooted, RunOutcome, Runtime, Task, TaskId, ValidationErrorKind, ValueView,
    VmError, VmErrorKind,
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

/// Run `task` one instruction at a time, on alternating OS threads, with a
/// full collection after every yield: every captured value must survive
/// while only frames and closures reach it. Returns the result and the most
/// heap objects live after any of those collections.
fn run_collecting(rt: &Runtime, task: &Task, host: &mut Recorder) -> (Rooted, usize) {
    let (mut runs, mut most_live) = (0, 0);
    loop {
        runs += 1;
        assert!(runs < 10_000, "task did not finish");
        let outcome = thread::scope(|s| {
            s.spawn(|| task.run_with(Budget::instructions(1), host))
                .join()
                .unwrap()
        })
        .unwrap();
        match outcome {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
                most_live = most_live.max(rt.heap_stats().objects);
            }
            RunOutcome::Complete(v) => return (v, most_live),
            other => panic!("{other:?}"),
        }
    }
}

/// `function () { count = count + 1; return count; }`, capturing the cell of
/// `count`.
fn increment() -> Executable {
    use Opcode::*;
    let mut asm = Assembler::new();
    asm.captures(1);
    asm.get_capture(0);
    asm.get_capture(0)
        .op(CellGet)
        .constant(num(1.0))
        .op(Add)
        .op(CellSet);
    asm.get_capture(0).op(CellGet).op(Return);
    asm.finish("increment", 0, 0).unwrap()
}

/// `function makeCounter(start) { let count = start; return function () {
/// count = count + 1; return count; }; }`
fn make_counter() -> Executable {
    use Opcode::*;
    let mut asm = Assembler::new();
    // local 0: start; local 1: the cell of `count`.
    asm.op(NewCell).set_local(1);
    asm.get_local(1).get_local(0).op(CellInit);
    asm.get_local(1).function(&increment()).op(Return);
    asm.finish("makeCounter", 1, 2).unwrap()
}

/// `function main(start) { const c = makeCounter(start); c(); c(); return
/// c(); }`
fn counter_main() -> Executable {
    use Opcode::*;
    let mut asm = Assembler::new();
    // local 0: start; local 1: c.
    asm.function(&make_counter())
        .get_local(0)
        .call(1)
        .set_local(1);
    asm.get_local(1).call(0).op(Pop);
    asm.get_local(1).call(0).op(Pop);
    asm.get_local(1).call(0).op(Return);
    asm.finish("main", 1, 2).unwrap()
}

#[test]
fn an_escaping_closure_keeps_its_binding_across_yields_and_collections() {
    let rt = Runtime::new();
    let start = rt.root(num(10.0));
    let task = rt.spawn(&counter_main(), &[&start]).unwrap();
    drop(start);
    let mut host = Recorder::default();
    // `makeCounter`'s frame is gone after its `Return`; from then on the cell
    // is reachable only through the closure in `main`'s local.
    let (result, most_live) = run_collecting(&rt, &task, &mut host);
    assert_eq!(result.view(), ValueView::Number(13.0));
    assert_eq!(most_live, 3, "makeCounter, the cell and the closure");
    drop(result);
    rt.collect().unwrap();
    assert_eq!(
        rt.heap_stats().objects,
        0,
        "finished frames release closures"
    );
}

/// `closure2` (`test_closure1`): `function f2() { var val = 1; function
/// set(a) { val = a; } function get(a) { return val; } return [set, get]; }`
fn f2() -> Executable {
    use Opcode::*;
    let mut asm = Assembler::new();
    asm.captures(1)
        .get_capture(0)
        .get_local(0)
        .op(CellSet)
        .op(Undefined)
        .op(Return);
    let set = asm.finish("set", 1, 1).unwrap();
    let mut asm = Assembler::new();
    asm.captures(1).get_capture(0).op(CellGet).op(Return);
    let get = asm.finish("get", 1, 1).unwrap();
    let mut asm = Assembler::new();
    // local 0: the cell of `val`.
    asm.op(NewCell).set_local(0);
    asm.get_local(0).constant(num(1.0)).op(CellInit);
    asm.get_local(0).function(&set);
    asm.get_local(0).function(&get);
    asm.new_vector(2).op(Return);
    asm.finish("f2", 0, 1).unwrap()
}

#[test]
fn closures_created_together_observe_one_binding() {
    use Opcode::*;
    // const obj = f2(); const before = obj.get(); obj.set(10);
    // return before * 100 + obj.get();
    let mut asm = Assembler::new();
    asm.function(&f2()).call(0).set_local(0);
    asm.get_local(0).constant(num(1.0)).op(Nth).call(0);
    asm.constant(num(100.0)).op(Mul);
    asm.get_local(0).constant(num(0.0)).op(Nth);
    asm.constant(num(10.0)).call(1).op(Pop);
    asm.get_local(0).constant(num(1.0)).op(Nth).call(0);
    asm.op(Add).op(Return);
    let main = asm.finish("closure2", 0, 1).unwrap();
    let rt = Runtime::new();
    let task = rt.spawn(&main, &[]).unwrap();
    let (result, _) = run_collecting(&rt, &task, &mut Recorder::default());
    assert_eq!(result.view(), ValueView::Number(110.0));
}

/// `function rec(n, k) { if (n == 0) return k() + k(); let c = n;
/// return rec(n - 1, function () { c = c + 10; return c + k(); }); }`
fn rec() -> Executable {
    use Opcode::*;
    // The closure captures the cell of `c` and the value of `k`.
    let mut asm = Assembler::new();
    asm.captures(2);
    asm.get_capture(0);
    asm.get_capture(0)
        .op(CellGet)
        .constant(num(10.0))
        .op(Add)
        .op(CellSet);
    asm.get_capture(0).op(CellGet);
    asm.get_capture(1).call(0).op(Add).op(Return);
    let step = asm.finish("step", 0, 0).unwrap();
    let mut asm = Assembler::new();
    let recurse = asm.label();
    // local 0: n; local 1: k; local 2: the cell of `c`.
    asm.get_local(0)
        .constant(num(0.0))
        .op(LooseEq)
        .jump_if_false(recurse);
    asm.get_local(1)
        .call(0)
        .get_local(1)
        .call(0)
        .op(Add)
        .op(Return);
    asm.bind(recurse);
    asm.op(NewCell).set_local(2);
    asm.get_local(2).get_local(0).op(CellInit);
    asm.op(CurrentFunction)
        .get_local(0)
        .constant(num(1.0))
        .op(Sub);
    asm.get_local(2).get_local(1).function(&step);
    asm.call(2).op(Return);
    asm.finish("rec", 2, 3).unwrap()
}

#[test]
fn closures_created_inside_recursion_capture_each_activation() {
    use Opcode::*;
    // return rec(3, function () { return 0; });
    let mut asm = Assembler::new();
    asm.constant(num(0.0)).op(Return);
    let zero = asm.finish("zero", 0, 0).unwrap();
    let mut asm = Assembler::new();
    asm.function(&rec())
        .constant(num(3.0))
        .function(&zero)
        .call(2)
        .op(Return);
    let main = asm.finish("main", 0, 0).unwrap();
    let rt = Runtime::new();
    let task = rt.spawn(&main, &[]).unwrap();
    let (result, most_live) = run_collecting(&rt, &task, &mut Recorder::default());
    // First k(): (1+10) + (2+10) + (3+10) + 0 = 36. Each activation's `c`
    // is its own binding, and the second k() sees the first one's writes:
    // 21 + 22 + 23 + 0 = 66.
    assert_eq!(result.view(), ValueView::Number(102.0));
    // rec, zero, and per level a cell and a closure, all kept alive only by
    // suspended frames and other closures.
    assert_eq!(most_live, 2 + 3 * 2);
    drop(result);
    rt.collect().unwrap();
    assert_eq!(rt.heap_stats().objects, 0);
}

/// `closure1` (`f` in `test_closure.js`), with `var x` as `const x`: every
/// captured binding is initialized before capture and never assigned, so it
/// is captured by value and `g` is an immutable closure.
///
/// ```js
/// function f(a, b, c) {
///     const x = 10;
///     log("a=" + a);
///     function g(d) {
///         function h() { log("d=" + d); log("x=" + x); }
///         log("b=" + b); log("c=" + c);
///         h();
///     }
///     g(4);
///     return g;
/// }
/// ```
fn closure1_f() -> Executable {
    use Opcode::*;
    const A: u16 = 1;
    const B: u16 = 2;
    const C: u16 = 3;
    const D: u16 = 4;
    const X: u16 = 5;
    // h captures [d, x].
    let mut asm = Assembler::new();
    asm.captures(2);
    asm.get_capture(0).host_effect(D).op(Pop);
    asm.get_capture(1).host_effect(X).op(Pop);
    asm.op(Undefined).op(Return);
    let h = asm.finish("h", 0, 0).unwrap();
    // g(d) captures [b, c, x] and passes x on to h.
    let mut asm = Assembler::new();
    asm.captures(3);
    asm.get_capture(0).host_effect(B).op(Pop);
    asm.get_capture(1).host_effect(C).op(Pop);
    asm.get_local(0).get_capture(2).function(&h).call(0).op(Pop);
    asm.op(Undefined).op(Return);
    let g = asm.finish("g", 1, 1).unwrap();
    // f(a, b, c); local 3: x.
    let mut asm = Assembler::new();
    asm.constant(num(10.0)).set_local(3);
    asm.get_local(0).host_effect(A).op(Pop);
    asm.get_local(1).get_local(2).get_local(3).function(&g);
    asm.op(Dup).constant(num(4.0)).call(1).op(Pop);
    asm.op(Return);
    asm.finish("f", 3, 4).unwrap()
}

#[test]
fn an_immutable_closure_is_published_and_called_from_another_task() {
    // var g1 = f(1, 2, 3);
    let mut asm = Assembler::new();
    asm.function(&closure1_f());
    asm.constant(num(1.0))
        .constant(num(2.0))
        .constant(num(3.0))
        .call(3)
        .op(Opcode::Return);
    let main = asm.finish("closure1", 0, 0).unwrap();
    let rt = Runtime::new();
    let mut host = Recorder::default();
    let a = rt.spawn(&main, &[]).unwrap();
    let g1 = complete(a.run_with(Budget::UNLIMITED, &mut host).unwrap());
    rt.publish(&g1)
        .expect("a closure over immutable values is shareable");
    // g1(5), in another task, with its own frames.
    let mut asm = Assembler::new();
    asm.get_local(0)
        .constant(num(5.0))
        .call(1)
        .op(Opcode::Return);
    let apply = asm.finish("apply", 1, 1).unwrap();
    let b = rt.spawn(&apply, &[&g1]).unwrap();
    drop(g1);
    let (result, _) = run_collecting(&rt, &b, &mut host);
    assert_eq!(result.view(), ValueView::Undefined);
    // "a=1,b=2,c=3,d=4,x=10,b=2,c=3,d=5,x=10," (closure1).
    let log: Vec<(u16, f64)> = [a.id(), b.id()].iter().flat_map(|&t| host.of(t)).collect();
    let expected = [
        (1, 1.0),
        (2, 2.0),
        (3, 3.0),
        (4, 4.0),
        (5, 10.0),
        (2, 2.0),
        (3, 3.0),
        (4, 5.0),
        (5, 10.0),
    ];
    assert_eq!(log, expected);
}

#[test]
fn closures_over_mutable_bindings_are_not_published() {
    let rt = Runtime::new();
    // [set, get] of f2: both capture the same cell.
    let mut asm = Assembler::new();
    asm.function(&f2()).call(0).op(Opcode::Return);
    let make = asm.finish("make", 0, 0).unwrap();
    let task = rt.spawn(&make, &[]).unwrap();
    let pair = complete(task.run().unwrap());
    let err = rt.publish(&pair).unwrap_err();
    let PublishError::NotShareable { path, found, owner } = &err else {
        panic!("{err:?}")
    };
    assert_eq!((*found, *owner), ("binding cell", Some(task.id())));
    assert_eq!(path.steps(), [PathStep::Index(0), PathStep::Capture(0)]);
    assert_eq!(
        err.to_string(),
        format!(
            "cannot publish: value[0].capture(0) is a task-owned binding cell \
             (owned by {}), which is not shareable",
            task.id()
        )
    );

    // Transitively: an immutable closure capturing that closure is rejected
    // too, and one capturing a persistent vector and a number is not.
    let mut asm = Assembler::new();
    asm.captures(2).op(Opcode::Undefined).op(Opcode::Return);
    let holder = asm.finish("holder", 0, 0).unwrap();
    let mut asm = Assembler::new();
    asm.get_local(0).get_local(1).function(&holder);
    asm.op(Opcode::Return);
    let wrap = asm.finish("wrap", 2, 2).unwrap();
    let wrap_in =
        |a: &Rooted, b: &Rooted| complete(rt.spawn(&wrap, &[a, b]).unwrap().run().unwrap());
    let set = {
        let mut asm = Assembler::new();
        asm.get_local(0)
            .constant(num(0.0))
            .op(Opcode::Nth)
            .op(Opcode::Return);
        let first = asm.finish("first", 1, 1).unwrap();
        complete(rt.spawn(&first, &[&pair]).unwrap().run().unwrap())
    };
    let one = rt.root(num(1.0));
    let tainted = wrap_in(&one, &set);
    let err = rt.publish(&tainted).unwrap_err();
    assert!(
        err.to_string().starts_with(
            "cannot publish: value.capture(1).capture(0) is a task-owned binding cell"
        ),
        "{err}"
    );
    let mut asm = Assembler::new();
    asm.constant(num(1.0)).constant(num(2.0)).new_vector(2);
    asm.op(Opcode::Return);
    let vector = asm.finish("vector", 0, 0).unwrap();
    let vector = complete(rt.spawn(&vector, &[]).unwrap().run().unwrap());
    let clean = wrap_in(&vector, &one);
    rt.publish(&clean)
        .expect("only immutable shareable captures");
    // A plain (mutable) object captured by value is rejected as well.
    let obj = rt.scope(|s| {
        let o = s.new_object();
        s.persist(o)
    });
    let err = rt.publish(&wrap_in(&obj, &one)).unwrap_err();
    assert!(
        err.to_string()
            .starts_with("cannot publish: value.capture(0) is a task-owned object"),
        "{err}"
    );
}

#[test]
fn task_local_environments_do_not_interfere() {
    let rt = Runtime::new();
    let main = counter_main();
    let (s10, s100) = (rt.root(num(10.0)), rt.root(num(100.0)));
    let a = rt.spawn(&main, &[&s10]).unwrap();
    let b = rt.spawn(&main, &[&s100]).unwrap();
    let mut host = Recorder::default();
    let (mut ra, mut rb) = (None, None);
    let mut runs = 0;
    // Interleaved with different budgets and a collection after each round:
    // the same code, each task with its own cells.
    while ra.is_none() || rb.is_none() {
        runs += 1;
        assert!(runs < 1_000, "tasks did not finish");
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
    assert_eq!(ra, Some(ValueView::Number(13.0)));
    assert_eq!(rb, Some(ValueView::Number(103.0)));
}

#[test]
fn another_task_cannot_use_a_closure_over_mutable_bindings() {
    let rt = Runtime::new();
    let mut asm = Assembler::new();
    asm.function(&make_counter())
        .constant(num(0.0))
        .call(1)
        .op(Opcode::Return);
    let make = asm.finish("make", 0, 0).unwrap();
    let owner = rt.spawn(&make, &[]).unwrap();
    let counter = complete(owner.run().unwrap());
    assert!(rt.publish(&counter).is_err());
    // Handed over without the check (spawn does not check arguments), the
    // closure still cannot reach the other task's binding.
    let mut asm = Assembler::new();
    asm.get_local(0).call(0).op(Opcode::Return);
    let call = asm.finish("call", 1, 1).unwrap();
    let thief = rt.spawn(&call, &[&counter]).unwrap();
    let err = failed(thief.run().unwrap());
    assert_eq!(
        err.kind(),
        VmErrorKind::ForeignBinding {
            owner: owner.id(),
            caller: thief.id(),
        }
    );
    assert_eq!(err.diagnostic().executable.as_ref(), "increment");

    // Writes are refused too: `set` of f2 (`cell_set`), and a closure that
    // initializes a binding it captured uninitialized (`cell_init`).
    let mut asm = Assembler::new();
    asm.function(&f2()).call(0).op(Opcode::Return);
    let make_pair = asm.finish("make", 0, 0).unwrap();
    let pair_owner = rt.spawn(&make_pair, &[]).unwrap();
    let pair = complete(pair_owner.run().unwrap());
    let mut asm = Assembler::new();
    asm.captures(1)
        .get_capture(0)
        .constant(num(1.0))
        .op(Opcode::CellInit)
        .op(Opcode::Undefined)
        .op(Opcode::Return);
    let init = asm.finish("init", 0, 0).unwrap();
    let mut asm = Assembler::new();
    asm.op(Opcode::NewCell).function(&init).op(Opcode::Return);
    let make_init = asm.finish("make", 0, 0).unwrap();
    let init_owner = rt.spawn(&make_init, &[]).unwrap();
    let init = complete(init_owner.run().unwrap());
    // set(5) through the pair, or init().
    let mut asm = Assembler::new();
    asm.get_local(0)
        .constant(num(0.0))
        .op(Opcode::Nth)
        .constant(num(5.0))
        .call(1)
        .op(Opcode::Return);
    let call_set = asm.finish("call_set", 1, 1).unwrap();
    for (exe, arg, owner, at) in [
        (&call_set, &pair, &pair_owner, "set"),
        (&call, &init, &init_owner, "init"),
    ] {
        let thief = rt.spawn(exe, &[arg]).unwrap();
        let err = failed(thief.run().unwrap());
        assert_eq!(
            err.kind(),
            VmErrorKind::ForeignBinding {
                owner: owner.id(),
                caller: thief.id(),
            },
            "{err}"
        );
        assert_eq!(err.diagnostic().executable.as_ref(), at);
    }
}

#[test]
fn a_cycle_between_a_closure_and_its_environment_is_reclaimed() {
    use Opcode::*;
    // let f = function () { return f; };
    let mut asm = Assembler::new();
    asm.captures(1).get_capture(0).op(CellGet).op(Return);
    let self_ref = asm.finish("f", 0, 0).unwrap();
    let make = |then_return: bool| {
        let mut asm = Assembler::new();
        asm.op(NewCell).set_local(0);
        asm.get_local(0)
            .get_local(0)
            .function(&self_ref)
            .op(CellInit);
        if then_return {
            // return f;
            asm.get_local(0).op(CellGet).op(Return);
        } else {
            // return f() === f;
            asm.get_local(0).op(CellGet).call(0);
            asm.get_local(0).op(CellGet).op(StrictEq).op(Return);
        }
        asm.finish("main", 0, 1).unwrap()
    };
    let rt = Runtime::new();
    // Alive while a frame reaches it, across yields and collections.
    let task = rt.spawn(&make(false), &[]).unwrap();
    let (result, most_live) = run_collecting(&rt, &task, &mut Recorder::default());
    assert_eq!(result.view(), ValueView::Bool(true));
    assert_eq!(most_live, 2, "the cell and the closure");
    let stats = rt.collect().unwrap();
    assert_eq!(stats.reclaimed, 2, "the finished task's cycle");
    assert_eq!(rt.heap_stats().objects, 0);
    // Alive while a root reaches it, reclaimed once the root is dropped.
    let task = rt.spawn(&make(true), &[]).unwrap();
    let f = complete(task.run().unwrap());
    drop(task);
    assert_eq!(rt.collect().unwrap().reclaimed, 0);
    assert_eq!(rt.heap_stats().objects, 2);
    let weak = f.downgrade();
    drop(f);
    assert_eq!(rt.collect().unwrap().reclaimed, 2);
    assert_eq!(rt.heap_stats().objects, 0);
    assert!(!weak.is_live());
}

#[test]
fn an_uninitialized_binding_is_a_reference_error() {
    use Opcode::*;
    // const g = function () { return x; }; g(); let x = 1;
    let mut asm = Assembler::new();
    asm.captures(1).get_capture(0).op(CellGet).op(Return);
    let g = asm.finish("g", 0, 0).unwrap();
    let mut asm = Assembler::new();
    asm.op(NewCell).set_local(0);
    asm.get_local(0).function(&g).call(0).op(Pop);
    asm.get_local(0).constant(num(1.0)).op(CellInit);
    asm.op(Undefined).op(Return);
    let read_early = asm.finish("main", 0, 1).unwrap();
    // x = 2; let x = 1;
    let mut asm = Assembler::new();
    asm.op(NewCell).set_local(0);
    asm.get_local(0).constant(num(2.0)).op(CellSet);
    asm.op(Undefined).op(Return);
    let write_early = asm.finish("main", 0, 1).unwrap();
    let rt = Runtime::new();
    for (exe, at) in [(read_early, "g"), (write_early, "main")] {
        let err = failed(rt.spawn(&exe, &[]).unwrap().run().unwrap());
        assert_eq!(err.kind(), VmErrorKind::ReferenceError, "{err}");
        assert_eq!(err.diagnostic().executable.as_ref(), at);
    }
    // Cell instructions reject values that are not cells.
    let mut asm = Assembler::new();
    asm.constant(num(1.0)).op(CellGet).op(Return);
    let not_a_cell = asm.finish("main", 0, 0).unwrap();
    let err = failed(rt.spawn(&not_a_cell, &[]).unwrap().run().unwrap());
    assert_eq!(err.kind(), VmErrorKind::UnsupportedOperand, "{err}");
}

#[test]
fn captures_are_declared_and_checked_at_load_time() {
    use Opcode::*;
    // A capture index past the declaration.
    let mut asm = Assembler::new();
    asm.captures(1).get_capture(1).op(Return);
    assert_eq!(
        asm.finish("bad", 0, 0).unwrap_err().kind,
        ValidationErrorKind::CaptureOutOfRange { index: 1, count: 1 }
    );
    // `Function` pops the captures its child declares.
    let mut asm = Assembler::new();
    asm.captures(2).op(Undefined).op(Return);
    let child = asm.finish("child", 0, 0).unwrap();
    let mut asm = Assembler::new();
    asm.constant(num(1.0)).function(&child).op(Return);
    assert!(matches!(
        asm.finish("short", 0, 0).unwrap_err().kind,
        ValidationErrorKind::StackUnderflow {
            opcode: Opcode::Function,
            depth: 1
        }
    ));
    // The declaration round-trips through the byte format.
    let spec: ExecutableSpec = child.to_spec();
    assert_eq!(spec.captures, 2);
    let reloaded = Executable::load(spec).unwrap();
    let mut asm = Assembler::new();
    asm.constant(num(1.0))
        .constant(num(2.0))
        .function(&reloaded);
    asm.op(Return);
    assert!(asm.finish("ok", 0, 0).is_ok());
    // Closure code spawned directly has no environment to read from.
    let mut asm = Assembler::new();
    asm.captures(1).get_capture(0).op(Return);
    let body = asm.finish("body", 0, 0).unwrap();
    let rt = Runtime::new();
    let err = failed(rt.spawn(&body, &[]).unwrap().run().unwrap());
    assert_eq!(err.kind(), VmErrorKind::Internal, "{err}");
}
