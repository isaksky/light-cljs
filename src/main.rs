//! Thin binary over the `clj_quickjs` library: compile a JavaScript script of
//! the documented subset (`docs/grammar.md`) and run it as one task.
//!
//! * `cargo run` runs the bundled demo, `examples/demo.js` (a documented
//!   synthetic fixture).
//! * `cargo run -- path/to/script.js` runs that file instead.
//!
//! The completion value is printed; a string exactly, as a quoted literal
//! that writes a lone surrogate as `\u{d800}` (`JsString`'s `Debug`). Host
//! effects (`__rt.effect(op, x)`) print `effect op: x` (a string lossily,
//! U+FFFD for a lone surrogate) and return `x`. Compilation errors exit with status 2, run
//! failures with status 1. No qjsc or C engine is involved: the script is
//! compiled by `clj_quickjs::frontend` and executed by its VM.

use clj_quickjs::frontend::compile_script;
use clj_quickjs::{Budget, Host, HostCall, Primitive, RunOutcome, Runtime, ValueView};

/// Prints each host effect and returns its argument.
struct PrintingHost;

impl Host for PrintingHost {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        let shown = match &call.arg {
            Primitive::Number(n) => n.to_string(),
            Primitive::String(s) => s.to_string(),
            other => format!("{other:?}"),
        };
        println!("effect {}: {shown}", call.op);
        Ok(call.arg)
    }
}

fn main() {
    let (name, source) = match std::env::args().nth(1) {
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(source) => (path, source),
            Err(e) => {
                eprintln!("{path}: {e}");
                std::process::exit(2);
            }
        },
        None => (
            "examples/demo.js".to_string(),
            include_str!("../examples/demo.js").to_string(),
        ),
    };
    let exe = match compile_script(&name, &source) {
        Ok(exe) => exe,
        Err(e) => {
            eprintln!("{name}:{e}");
            std::process::exit(2);
        }
    };
    let rt = Runtime::new();
    let outcome = rt
        .spawn(&exe, &[])
        .map_err(|e| e.to_string())
        .and_then(|t| {
            t.run_with(Budget::UNLIMITED, &mut PrintingHost)
                .map_err(|e| e.to_string())
        });
    match outcome {
        Ok(RunOutcome::Complete(v)) => match v.view() {
            ValueView::Number(n) => println!("{name} completed with {n}"),
            ValueView::BigInt => println!("{name} completed with {}n", v.bigint().unwrap()),
            ValueView::String => match v.string() {
                Some(s) => println!("{name} completed with {s:?}"),
                None => println!("{name} completed with a string"),
            },
            other => println!("{name} completed with {other:?}"),
        },
        Ok(RunOutcome::Failed(e)) => {
            // The error's display includes the source position.
            eprintln!("{name}: {e}");
            std::process::exit(1);
        }
        Ok(other) => {
            eprintln!("{name}: {other:?}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("run error: {e}");
            std::process::exit(1);
        }
    }
}
