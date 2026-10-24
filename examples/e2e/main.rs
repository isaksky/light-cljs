//! End-to-end synthetic compiler-host success scenario (`docs/design.md`
//! §13), running JavaScript source on two executor workers.
//!
//! Run with `cargo run --example e2e`. No external compiler or C engine is
//! involved: the fixtures `consumer.js`, `producer.js` and `work.js` next to
//! this file are synthetic (not light-cljs output) and are compiled and run
//! by this crate. The scenario itself is in `scenario.rs` (module docs list
//! the steps); `tests/e2e.rs` runs the same scenario and asserts what it
//! proves. This example prints what happened: the host's trace, the
//! executor's event record and the identity checks.

mod scenario;

use std::process::ExitCode;

use clj_quickjs::exec::{EventKind, OutcomeKind};

fn main() -> ExitCode {
    let report = match scenario::run() {
        Ok(report) => report,
        Err(e) => {
            eprintln!("scenario failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    println!("world {}, dependency {}", report.world, report.key);
    for (i, thread) in report.worker_threads.iter().enumerate() {
        println!("worker {i}: OS thread {thread:?}");
    }

    println!("\nhost trace:");
    for t in &report.trace {
        println!("  {t}");
    }

    println!("\nexecutor events:");
    for e in &report.events {
        if matches!(
            e.kind,
            EventKind::WorkerStarted { .. } | EventKind::WorkerStopped
        ) {
            continue;
        }
        let worker = e.worker.map_or("host".to_string(), |w| w.to_string());
        let task = e.task.map_or(String::new(), |t| format!(" {t}"));
        let kind = match &e.kind {
            EventKind::Collected(c) => format!(
                "Collected #{} (requester {:?}, {} live, {} reclaimed)",
                c.number, c.requester, c.stats.live, c.stats.reclaimed
            ),
            EventKind::Released(OutcomeKind::Waiting(key)) => format!("Released(Waiting({key}))"),
            other => format!("{other:?}"),
        };
        println!("  {:>3} {worker}{task}: {kind}", e.seq);
    }

    println!();
    let mut ok = true;
    for c in &report.consumers {
        let same = c.result.same_object(&c.expected);
        let input = c.result.same_object(&report.form);
        ok &= same;
        println!(
            "consumer {} (tag {}, depth {}): returned the input's depth-{} subtree with its \
             identity: {same} (the whole input: {input})",
            c.task, c.spec.tag, c.spec.depth, c.spec.depth
        );
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
