//! End-to-end synthetic compiler-host scenario (`docs/design.md` §13),
//! running JavaScript source on two executor workers.
//!
//! Run with `cargo run --example e2e -- [success|failure|cancel|companion]`
//! (default `success`):
//!
//! * `success`: two consumers wait for a helper a producer task compiles,
//!   then both call it and return subtrees of the shared input.
//! * `failure`: the producer task fails; the publication of that failure
//!   wakes both consumers, whose `catch` blocks observe it.
//! * `cancel`: the consumer that claimed the producer request is cancelled
//!   while parked (its `finally` block runs); the producer and the other
//!   consumer still complete.
//! * `companion`: one task reaches the helper from inside a native fold's
//!   JavaScript callback, with a live captured local and two pending
//!   `finally` blocks, suspends there and completes after migrating.
//!
//! No external compiler or C engine is involved: the fixtures `consumer.js`,
//! `producer.js`, `work.js` and `companion.js` next to this file are
//! synthetic (not light-cljs output) and are compiled and run by this crate.
//! The scenarios are in `scenario.rs` (module docs list the steps);
//! `tests/e2e.rs` runs the same code and asserts what each proves. This
//! example prints what happened: the host's trace, the executor's event
//! record and the identity checks.

mod scenario;

use std::process::ExitCode;

use big_boy_pants_js::exec::{Event, EventKind, OutcomeKind};

use scenario::{ConsumerEnd, PublishedAs, Trace, Variant};

fn main() -> ExitCode {
    let arg = std::env::args().nth(1);
    let ok = match arg.as_deref() {
        Some("companion") => companion(),
        other => match other.unwrap_or("success").parse::<Variant>() {
            Ok(variant) => consumers(variant),
            Err(e) => {
                eprintln!("{e}; expected success, failure, cancel or companion");
                return ExitCode::FAILURE;
            }
        },
    };
    match ok {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("scenario failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn print_trace(trace: &[Trace], events: &[Event]) {
    println!("\nhost trace:");
    for t in trace {
        println!("  {t}");
    }

    println!("\nexecutor events:");
    for e in events {
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
}

fn consumers(variant: Variant) -> Result<bool, String> {
    let report = scenario::run_variant(variant)?;
    println!(
        "variant {}: world {}, dependency {}",
        variant.name(),
        report.world,
        report.key
    );
    for (i, thread) in report.worker_threads.iter().enumerate() {
        println!("worker {i}: OS thread {thread:?}");
    }
    print_trace(&report.trace, &report.events);

    let mut ok = true;
    for c in &report.consumers {
        let ConsumerEnd::Returned(result) = &c.end else {
            let others_done = report
                .consumers
                .iter()
                .filter(|o| o.task != c.task)
                .all(|o| !o.cancelled());
            println!(
                "consumer {} (tag {}) was cancelled while parked; the producer and the \
                 other consumer completed: {others_done}",
                c.task, c.spec.tag
            );
            ok &= others_done;
            continue;
        };
        match report.published {
            PublishedAs::Helper => {
                let same = result.same_object(&c.expected);
                let input = result.same_object(&report.form);
                ok &= same;
                println!(
                    "consumer {} (tag {}, depth {}): returned the input's depth-{} subtree \
                     with its identity: {same} (the whole input: {input})",
                    c.task, c.spec.tag, c.spec.depth, c.spec.depth
                );
            }
            PublishedAs::Failure => {
                let kind = result.error().map(|e| e.kind());
                let cause = result
                    .error_cause()
                    .is_some_and(|cause| cause.same_object(&report.published_value));
                ok &= kind.is_some() && cause;
                println!(
                    "consumer {} (tag {}): caught {kind:?}; its cause is the published \
                     failure value: {cause}",
                    c.task, c.spec.tag
                );
            }
        }
    }
    Ok(ok)
}

fn companion() -> Result<bool, String> {
    let report = scenario::run_companion()?;
    println!(
        "companion: world {}, dependency {}",
        report.world, report.key
    );
    print_trace(&report.trace, &report.events);
    let same: Vec<bool> = report.rt.scope(|s| {
        let result = s.local(&report.result).unwrap();
        report
            .expected
            .iter()
            .enumerate()
            .map(|(i, expected)| {
                let picked = s.nth(result, i).unwrap();
                s.same_object(picked, s.local(expected).unwrap())
            })
            .collect()
    });
    println!(
        "companion {}: the fold returned the form's body and inner subtrees with their \
         identity: {same:?}",
        report.task
    );
    Ok(same.iter().all(|&s| s))
}
