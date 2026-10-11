//! Direct phase-contract witnesses; pinned upstream conformance is separate.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{MacroHost, analyze_with_macros, emit_js, parse};
use std::{cell::RefCell, process::Command, rc::Rc};

fn both(host: &mut MacroHost, source: &str, expected: &str, effects: &[&str]) {
    host.disable_native_evaluation();
    let program = analyze_with_macros(source, &parse(source).unwrap(), host).unwrap();
    let trace = Rc::new(RefCell::new(Vec::new()));
    let sink = trace.clone();
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    evaluator
        .install_callback("connectBridge", move |_, event| {
            sink.borrow_mut().push(event);
            Ok("connection".into())
        })
        .unwrap();
    evaluator
        .evaluate_js("globalThis.connect = () => connectBridge('connect')")
        .unwrap();
    let value = evaluator.evaluate(&program).unwrap();
    assert_eq!(evaluator.print(value).unwrap(), expected);
    assert_eq!(
        trace
            .borrow()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        effects
    );
    let js = format!(
        "const effects=[]; globalThis.connect=()=>{{effects.push('connect');return 'connection'}}; console.log(String({})); console.log(effects.join(','));",
        emit_js(&program)
    );
    let output = Command::new("node").args(["-e", &js]).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{expected}\n{}\n", effects.join(","))
    );
}

#[test]
fn acceptance_programs_execute_in_independent_heaps_with_fresh_and_reused_hosts() {
    let mut reused = MacroHost::new();
    for _ in 0..2 {
        for host in [&mut reused, &mut MacroHost::new()] {
            both(
                host,
                "(def factor (+ 1 1)) (defn twice [x] (* factor x)) (def connection (js/connect)) (defmacro doubled [x] (twice x)) [(doubled 21) (twice 21)]",
                "[42 42]",
                &["connect"],
            );
            both(
                host,
                "(def hits (atom 0)) (defn next-id [] (swap! hits inc)) (defmacro id [] (next-id)) [(id) (id) @hits]",
                "[1 2 0]",
                &[],
            );
            both(
                host,
                "(def hits (atom 0)) (defn next-id [] (swap! hits inc)) (swap! hits inc) (defmacro id [] (next-id)) [(id) (id) @hits]",
                "[1 2 1]",
                &[],
            );
        }
    }
}

#[test]
fn quoted_js_macro_literals_retain_explicit_descriptors() {
    // Required form ABI witness: quoted #js descriptors survive the compiled
    // bridge, while arbitrary host objects remain invalid expansion results.
    both(
        &mut MacroHost::new(),
        "(defmacro m [] '(let [out #js []] out)) (alength (m))",
        "0",
        &[],
    );
}

#[test]
fn legacy_entrypoints_are_fenced_in_a_child_process() {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "legacy_execution_fence_probe", "--nocapture"])
        .env("LIGHT_CLJS_DISABLE_NATIVE_EVAL", "1")
        .env("LIGHT_CLJS_NATIVE_FENCE_PROBE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn legacy_execution_fence_probe() {
    // The parent test always executes this branch in an isolated process: no
    // mutation of process-global environment in the concurrent test harness.
    if std::env::var_os("LIGHT_CLJS_NATIVE_FENCE_PROBE").is_none() {
        return;
    }
    use light_cljs::interpreter::{Context, builtin_definitions};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let mut c = Context::new();
    let source = c.source("fenced.clj", "42");
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    let analyzed = c.analyze_form(&form).unwrap();
    let nil = c.nil();
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            light_cljs::interpreter::eval_string("42").unwrap();
        }))
        .is_err()
    );
    assert!(catch_unwind(AssertUnwindSafe(|| c.eval_string("42"))).is_err());
    assert!(catch_unwind(AssertUnwindSafe(|| c.eval_source(&source))).is_err());
    assert!(catch_unwind(AssertUnwindSafe(|| c.eval_form(&form))).is_err());
    assert!(catch_unwind(AssertUnwindSafe(|| c.eval_analyzed(&analyzed))).is_err());
    assert!(catch_unwind(AssertUnwindSafe(|| c.invoke(&nil, &[]))).is_err());
    assert!(
        catch_unwind(AssertUnwindSafe(
            || c.invoke_native(&builtin_definitions()[0], &[])
        ))
        .is_err()
    );
}
