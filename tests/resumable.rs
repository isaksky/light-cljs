//! Budgeted runs through the public embedding API.
//!
//! Early VM tests: programs are hand-assembled bytecode, not JavaScript
//! source support. The test plays the embedder and drives runs on threads it
//! owns; the runtime owns no threads.

use std::thread;

use clj_quickjs::{
    Assembler, Budget, Executable, Host, HostCall, Opcode, Primitive, RunOutcome, Runtime, TaskId,
    ValueView,
};

/// `function f(n) { let i = 0, last; while (i < n) { last = effect(i); i = i + 1; } return last; }`
fn effect_loop() -> Executable {
    let mut asm = Assembler::new();
    let (top, done) = (asm.label(), asm.label());
    asm.constant(Primitive::Number(0.0)).set_local(1);
    asm.op(Opcode::Undefined).set_local(2);
    asm.bind(top);
    asm.get_local(1).get_local(0).op(Opcode::Lt);
    asm.jump_if_false(done);
    asm.get_local(1).host_effect(0).set_local(2);
    asm.get_local(1)
        .constant(Primitive::Number(1.0))
        .op(Opcode::Add)
        .set_local(1);
    asm.jump(top);
    asm.bind(done);
    asm.get_local(2).op(Opcode::Return);
    asm.finish("effect-loop", 1, 3).unwrap()
}

/// Counts effects and records which OS thread performed each one.
#[derive(Default)]
struct Counter {
    calls: Vec<(TaskId, Primitive, thread::ThreadId)>,
}

impl Host for Counter {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        self.calls
            .push((call.task, call.arg, thread::current().id()));
        Ok(Primitive::Number(self.calls.len() as f64))
    }
}

#[test]
fn yielded_task_resumes_on_another_thread_without_replaying_effects() {
    let rt = Runtime::new();
    let n = rt.root(Primitive::Number(3.0));
    let task = rt.spawn(&effect_loop(), &[&n]).unwrap();
    let mut host = Counter::default();

    // Alternate between two embedder threads, one bounded run each, until done.
    let mut runs = 0;
    let result = loop {
        runs += 1;
        assert!(runs < 50, "task did not finish");
        let outcome = thread::scope(|s| {
            s.spawn(|| task.run_with(Budget::instructions(9), &mut host))
                .join()
                .unwrap()
        })
        .unwrap();
        match outcome {
            RunOutcome::Yielded => continue,
            RunOutcome::Complete(v) => break v.view(),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(result, ValueView::Number(3.0));
    // 4 + 12 * 3 + 6 = 46 instructions in runs of 9.
    assert_eq!(runs, 6);
    let args: Vec<Primitive> = host.calls.iter().map(|c| c.1.clone()).collect();
    assert_eq!(
        args,
        [0.0, 1.0, 2.0].map(Primitive::Number),
        "each effect ran exactly once"
    );
    assert!(host.calls.iter().all(|c| c.0 == task.id()));
    // Effects were performed by different OS threads: the task migrated.
    assert_ne!(host.calls[0].2, host.calls[2].2);
}
