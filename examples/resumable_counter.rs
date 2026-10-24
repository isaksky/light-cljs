//! Resumable VM example: budgeted runs, `Yielded`, and a leaf host effect,
//! running JavaScript source.
//!
//! Run with `cargo run --example resumable_counter`.
//!
//! The program is `examples/resumable_counter.js` (a synthetic fixture),
//! compiled by this crate's frontend (`frontend::compile_function`) and run by
//! its VM:
//!
//! ```js
//! function count(n) {
//!   let last;
//!   const bump = function (i) { return __rt.effect(0, i); };
//!   let i = 0;
//!   while (i < n) { last = bump(i); i = i + 1; }
//!   return last;
//! }
//! ```
//!
//! Two tasks run the same shared executable. The example plays a tiny
//! round-robin scheduler: each run gets a budget of 5 instructions, so every
//! run ends in `Yielded` until the task returns. The host counter prints one
//! line per executed `host_effect` instruction (the lowering of
//! `__rt.effect`); since a resumed run continues at the saved instruction,
//! no effect line is ever printed twice.

use std::collections::VecDeque;

use clj_quickjs::frontend::compile_function;
use clj_quickjs::{Budget, Host, HostCall, Primitive, RunOutcome, Runtime, Task, ValueView};

/// Host operation selector for "increment the counter".
const INCREMENT: u16 = 0;

/// The host side: a counter that the program can only bump through a leaf
/// effect. It never reenters the VM, touches the heap, or suspends.
#[derive(Default)]
struct CounterHost {
    count: u32,
}

impl Host for CounterHost {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        if call.op != INCREMENT {
            return Err(format!("unknown host operation {}", call.op));
        }
        self.count += 1;
        let i = match call.arg {
            Primitive::Number(n) => n.to_string(),
            other => format!("{other:?}"),
        };
        println!(
            "  effect: counter = {} ({} at i = {i})",
            self.count, call.task
        );
        Ok(Primitive::Number(f64::from(self.count)))
    }
}

fn main() {
    let exe = match compile_function(include_str!("resumable_counter.js")) {
        Ok(exe) => exe,
        Err(e) => {
            eprintln!("examples/resumable_counter.js:{e}");
            std::process::exit(2);
        }
    };

    println!(
        "JavaScript source examples/resumable_counter.js, compiled by the clj_quickjs frontend"
    );
    let rt = Runtime::new();
    let mut ready: VecDeque<(&str, Task)> = VecDeque::new();
    for (name, n) in [("A", 3.0), ("B", 2.0)] {
        let arg = rt.root(Primitive::Number(n));
        match rt.spawn(&exe, &[&arg]) {
            Ok(task) => ready.push_back((name, task)),
            Err(e) => {
                eprintln!("spawn failed: {e}");
                std::process::exit(1);
            }
        }
    }

    let mut host = CounterHost::default();
    let budget = Budget::instructions(5);
    let mut run_no = 0;
    let mut effects_expected = 0;
    while let Some((name, task)) = ready.pop_front() {
        run_no += 1;
        let before = host.count;
        let outcome = match task.run_with(budget, &mut host) {
            Ok(outcome) => outcome,
            Err(e) => {
                eprintln!("run error: {e}");
                std::process::exit(1);
            }
        };
        let effects = host.count - before;
        match outcome {
            RunOutcome::Yielded => {
                println!("run {run_no}: task {name} yielded ({effects} effect(s) this run)");
                ready.push_back((name, task));
            }
            RunOutcome::Complete(v) => {
                let shown = match v.view() {
                    ValueView::Number(n) => n.to_string(),
                    other => format!("{other:?}"),
                };
                println!("run {run_no}: task {name} completed with {shown}");
                effects_expected += match name {
                    "A" => 3,
                    _ => 2,
                };
            }
            other => {
                eprintln!("run {run_no}: task {name} ended with {other:?}");
                std::process::exit(1);
            }
        }
    }
    println!(
        "host counter = {} after {run_no} runs (expected {effects_expected}: one per effect instruction executed)",
        host.count
    );
    if host.count != effects_expected {
        eprintln!("effect count mismatch: an effect was replayed or lost");
        std::process::exit(1);
    }
}
