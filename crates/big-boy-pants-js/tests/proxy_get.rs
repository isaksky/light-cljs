use big_boy_pants_js::frontend::compile_script;
use big_boy_pants_js::{
    Budget, Completion, Host, HostCall, Primitive, RunOutcome, Runtime, ValueView,
};

#[test]
fn get_traps_follow_ecmascript() {
    let code = compile_script("proxy-get", include_str!("fixtures/js/proxy_get.js")).unwrap();
    let rt = Runtime::new();
    let result = rt.spawn(&code, &[]).unwrap().run().unwrap();
    assert!(
        matches!(result, RunOutcome::Complete(ref v) if v.view() == ValueView::Bool(true)),
        "{result:?}"
    );
}

#[test]
fn builtin_reads_follow_ecmascript() {
    let code = compile_script(
        "proxy-builtin-reads",
        include_str!("fixtures/js/proxy_builtin_reads.js"),
    )
    .unwrap();
    let rt = Runtime::new();
    let result = rt.spawn(&code, &[]).unwrap().run().unwrap();
    assert!(
        matches!(result, RunOutcome::Complete(ref v) if v.view() == ValueView::Bool(true)),
        "{result:?}"
    );
}

struct NoEffects;
impl Host for NoEffects {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        Err(format!("unexpected host effect {}", call.op))
    }
}

#[test]
fn proxy_array_length_is_observable() {
    // The same Node-checked table also suspends in every get trap and ToLength
    // callback. Exact read logs detect replay; collection checks saved receivers
    // and conversion results, and task migration leaves no Rust frame alive.
    let source = include_str!("fixtures/js/proxy_builtin_reads.js").replace(
        "function pause() {}",
        "let pauses=0; function pause() {__rt.need(1800 + pauses++, 1);}",
    );
    let rt = Runtime::new();
    let code = compile_script("proxy-array-length-waits", &source).unwrap();
    let mut task = rt.spawn(&code, &[]).unwrap();
    let mut waits = 0;
    let mut yields = 0;
    for _ in 0..20000 {
        match task
            .run_with(Budget::instructions(7), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Yielded => {
                yields += 1;
                rt.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waits += 1;
                rt.collect().unwrap();
                let request = task.take_producer_requests().pop().unwrap();
                let value = rt.root(Primitive::Undefined);
                task = rt
                    .complete(request.key(), request.producer(), Completion::Ready(&value))
                    .unwrap()
                    .into_runnable()
                    .pop()
                    .unwrap();
                task = std::thread::spawn(move || {
                    let outcome = task
                        .run_with(Budget::instructions(1), &mut NoEffects)
                        .unwrap();
                    assert!(matches!(outcome, RunOutcome::Yielded));
                    task
                })
                .join()
                .unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                assert_eq!(waits, 113); // Same pause count in Node/V8.
                assert!(yields > waits);
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("Proxy builtin reads exceeded instruction bound");
}
