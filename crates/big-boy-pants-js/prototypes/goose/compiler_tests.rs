//! Lifecycle fixtures complement the actual .cljs compiler tests in capture.rs.
use super::*;
use crate::compiler_data::{DatumKind, Name, Origin, Source};

fn runner() -> PathBuf {
    std::env::var_os("GOOSE_COMPILER_RUNNER")
        .expect("run compiler.bb")
        .into()
}
fn form() -> Form {
    let source = Source::new("retained.cljs", "nil");
    Form::from_data(
        Datum::new(DatumKind::Nil),
        source.span(0..3).unwrap(),
        Origin::Source,
    )
}
fn origin(form: &Form) -> ExpansionOrigin {
    ExpansionOrigin {
        macro_name: Name::new(Some("fixture"), "macro"),
        call: form.span(),
        definition: Some(form.span()),
        helpers: vec![],
    }
}
fn inputs(e: &GooseEvaluator, source: &str) -> (ValueId, Vec<ValueId>, Form) {
    let form = form();
    let function = e.evaluate_unit(source).unwrap();
    let call = e.import_form(&form, Default::default()).unwrap();
    let env = e
        .import_data(&Datum::new(DatumKind::Nil), Default::default())
        .unwrap();
    (function, vec![call, env], form)
}
fn cleaned(s: &GooseSession) {
    assert!(s.process.lock().unwrap().is_none());
    assert!(s.readers.lock().unwrap().is_empty());
    assert!(s.pending.lock().unwrap().is_empty());
    assert!(!s.directory.exists());
}
#[cfg(unix)]
fn fake(body: &str) -> Arc<GooseSession> {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::temp_dir().join(format!(
        "goose-protocol-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, format!("#!/bin/sh\nprintf 'READY 1 1\\n'\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let session = GooseSession::start(&path, 1);
    std::fs::remove_file(path).unwrap();
    session.unwrap()
}
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_handles_are_owner_scoped() {
    let session = GooseSession::start(&runner(), 1).unwrap();
    let a = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
    let b = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
    let x = a.import_form(&form(), Default::default()).unwrap();
    let y = b.import_form(&form(), Default::default()).unwrap();
    assert_eq!(x.index, y.index);
    assert!(b.release(x).unwrap_err().0.contains("foreign"));
    a.release(x).unwrap();
    assert!(a.release(x).unwrap_err().0.contains("released"));
    let second = GooseSession::start(&runner(), 1).unwrap();
    let c = GooseEvaluator::new(second.clone(), &Default::default()).unwrap();
    let z = c.import_form(&form(), Default::default()).unwrap();
    assert_eq!(z.index, x.index);
    assert!(c.release(x).is_err());
    assert!(a.release(x).unwrap_err().0.contains("released"));
    let mut owner = crate::MacroHost::new();
    owner.use_goose_source_macros(session.clone()).unwrap();
    assert!(
        crate::MacroHost::new()
            .use_goose_source_macros(session.clone())
            .is_err()
    );
    crate::compile_with_macros("(defmacro m [] 1) (m)", &mut owner).unwrap();
    assert!(
        crate::compile_with_macros("(defmacro m [] 2) (m)", &mut owner).is_err(),
        "a new compilation must not reuse old unit identities"
    );
    session.close();
    assert!(a.import_form(&form(), Default::default()).is_err());
    b.release(y).unwrap();
    second.close();
    cleaned(&session);
    cleaned(&second);
}
#[cfg(unix)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_exit_and_malformed_replies_cleanup() {
    for (body, malformed_graph) in [
        ("read id\nexit 0", false),
        ("read id\nprintf 'malformed\\n'\nread stop", false),
        ("read id\nprintf 'DONE 999\\n'\nread stop", false),
        (
            "read id\nprintf x > \"$1/$id.out\"\nprintf 'DONE %s\\n' \"$id\"\nread stop",
            true,
        ),
    ] {
        let session = fake(body);
        let evaluator = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let (function, args, form) = inputs(&evaluator, "rt => (form, env) => 1");
        let result = evaluator.call(function, &args);
        if malformed_graph {
            let value = result.unwrap();
            assert!(
                evaluator
                    .export_form(value, &form, origin(&form), Default::default())
                    .is_err()
            );
        } else {
            assert!(result.is_err());
        }
        cleaned(&session);
    }
}
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_worker_failure_and_owned_diagnostic() {
    let session = GooseSession::start(&runner(), 1).unwrap();
    let mut host = crate::MacroHost::new();
    host.use_goose_source_macros(session.clone()).unwrap();
    let source = "(defmacro bad [] (nth [1] 10))\n(bad)";
    let error = crate::analyzer::analyze_named_with_macro_warnings(
        "negative.cljs",
        source,
        &crate::parse(source).unwrap(),
        &mut host,
        Default::default(),
        &mut Vec::new(),
    )
    .unwrap_err();
    session.close();
    drop(host);
    cleaned(&session);
    let message = error.to_string();
    assert!(message.contains("negative.cljs:2:1"), "{message}");
    assert!(
        message.contains("definition negative.cljs:1:1"),
        "{message}"
    );
    assert!(
        message.contains("function=") && message.contains("emitted-span="),
        "{message}"
    );
    // Independent reference: compare the supported origin and call location.
    let reference = crate::analyzer::analyze_named_with_macro_warnings(
        "negative.cljs",
        source,
        &crate::parse(source).unwrap(),
        &mut crate::MacroHost::new(),
        Default::default(),
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(
        reference.contains("negative.cljs:2:1")
            && reference.contains("definition negative.cljs:1:1"),
        "{reference}"
    );
}
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_helper_failure_keeps_instruction_context() {
    let session = GooseSession::start(&runner(), 1).unwrap();
    let mut host = crate::MacroHost::new();
    host.use_goose_source_macros(session.clone()).unwrap();
    let source = "(def broken (nth [1] 10))\n(defmacro bad [] broken)\n(bad)";
    let error = crate::analyzer::analyze_named_with_macro_warnings(
        "helper-negative.cljs",
        source,
        &crate::parse(source).unwrap(),
        &mut host,
        Default::default(),
        &mut Vec::new(),
    )
    .unwrap_err();
    session.close();
    drop(host);
    cleaned(&session);
    let message = error.to_string();
    assert!(message.contains("helper-negative.cljs:3:1"), "{message}");
    assert!(
        message.contains("definition helper-negative.cljs:2:1"),
        "{message}"
    );
    assert!(message.contains("TypeError function="), "{message}");
    for field in ["function=", "pc=", "emitted-span="] {
        let value = message.split_once(field).expect(&message).1;
        assert!(value.starts_with(|c: char| c.is_ascii_digit()), "{message}");
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_caught_runtime_errors_preserve_identity() {
    for body in ["null.x;", "let x = 1; x();"] {
        let source = format!(
            "(defmacro caught [] (js* \"(() => {{try {{{body}}} catch(e) {{return e === null;}}}})()\")) (caught)"
        );
        let reference = crate::compile_with_macros(&source, &mut crate::MacroHost::new()).unwrap();
        assert!(reference.contains("false"), "{reference}");
        let session = GooseSession::start(&runner(), 1).unwrap();
        let mut host = crate::MacroHost::new();
        host.use_goose_source_macros(session.clone()).unwrap();
        assert_eq!(
            crate::compile_with_macros(&source, &mut host).unwrap(),
            reference
        );
        session.close();
        cleaned(&session);
    }
    // Explicitly thrown primitive values keep their ordinary JS semantics.
    let source = "(defmacro caught [] (js* \"(() => {try {throw null;} catch(e) {return e === null;}})()\")) (caught)";
    let reference = crate::compile_with_macros(source, &mut crate::MacroHost::new()).unwrap();
    let session = GooseSession::start(&runner(), 1).unwrap();
    let mut host = crate::MacroHost::new();
    host.use_goose_source_macros(session.clone()).unwrap();
    assert_eq!(
        crate::compile_with_macros(source, &mut host).unwrap(),
        reference
    );
    session.close();
    cleaned(&session);
    // Error properties/prototypes are explicitly outside the bounded surface.
    let source = "(defmacro caught [] (js* \"(() => {try {null.x;} catch(e) {return e.name;}})()\")) (caught)";
    assert!(crate::compile_with_macros(source, &mut crate::MacroHost::new()).is_ok());
    let session = GooseSession::start(&runner(), 1).unwrap();
    let mut host = crate::MacroHost::new();
    host.use_goose_source_macros(session.clone()).unwrap();
    let error = crate::compile_with_macros(source, &mut host)
        .unwrap_err()
        .to_string();
    assert!(error.contains("UnsupportedOperand"), "{error}");
    cleaned(&session);
}
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cancel_and_deadline_cleanup() {
    for cancel in [false, true] {
        let session = GooseSession::start(&runner(), 1).unwrap();
        let options = EvaluatorOptions {
            timeout: if cancel {
                Duration::from_secs(2)
            } else {
                Duration::from_millis(30)
            },
            ..Default::default()
        };
        let evaluator = GooseEvaluator::new(session.clone(), &options).unwrap();
        let (function, args, _) = inputs(
            &evaluator,
            "rt => (form, env) => __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/value')",
        );
        evaluator
            .register_compiler_root("fixture/value", Some(0), true, "fixture.cljs", None)
            .unwrap();
        evaluator.install_helper(Rc::new(|_, _| {
            thread::sleep(Duration::from_millis(60));
            Ok("rt => __rt.formVector(1)".into())
        }));
        let control = evaluator.control();
        let s = session.clone();
        let cancelling = cancel.then(|| {
            thread::spawn(move || {
                let until = Instant::now() + Duration::from_secs(1);
                while !s.trace().iter().any(|line| line.starts_with("NEED "))
                    && Instant::now() < until
                {
                    thread::sleep(Duration::from_millis(1));
                }
                control.cancel();
            })
        });
        let error = evaluator.call(function, &args).unwrap_err().to_string();
        if let Some(t) = cancelling {
            t.join().unwrap();
        }
        assert!(
            error.contains(if cancel { "cancelled" } else { "deadline" }),
            "{error}"
        );
        cleaned(&session);
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_rejects_shared_writes_before_mutation() {
    for source in [
        "(defmacro effect [] (js* \"(globalThis.gooseForbidden = 1)\") 1) (effect)",
        "(defmacro effect [] (js* \"(typeof globalThis.Array === 'undefined' ? 1 : 2)\")) (effect)",
    ] {
        let session = GooseSession::start(&runner(), 1).unwrap();
        let mut host = crate::MacroHost::new();
        host.use_goose_source_macros(session.clone()).unwrap();
        let error = crate::compile_with_macros(source, &mut host)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("NativeBoundary") && error.contains("writes=0"),
            "{error}"
        );
        cleaned(&session);
        assert!(
            crate::compile_with_macros(source, &mut crate::MacroHost::new()).is_ok(),
            "V8 separately supports this explicitly rejected Goose job"
        );
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_rejects_root_reads_that_bypass_the_compiler_intrinsic() {
    let source = "(def value 1) (defmacro bypass [] (js* \"globalThis.__lightCompilerRoots['cljs.user/value']\")) (bypass)";
    let session = GooseSession::start(&runner(), 1).unwrap();
    let mut host = crate::MacroHost::new();
    host.use_goose_source_macros(session.clone()).unwrap();
    let error = crate::compile_with_macros(source, &mut host)
        .unwrap_err()
        .to_string();
    assert!(error.contains("DependencyNeed"), "{error}");
    cleaned(&session);
    assert!(crate::compile_with_macros(source, &mut crate::MacroHost::new()).is_ok());
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_injected_worker_abort_cleans_entire_session() {
    // A deliberately truncated real worker packet causes a native assertion,
    // distinct from a user-program TypeError. No fake protocol process here.
    for workers in [1, 2, 4, 8] {
        let session = GooseSession::start(&runner(), workers).unwrap();
        session
            .export(&session.directory.join("1.gsp"), "rt => (form, env) => 1")
            .unwrap();
        std::fs::write(session.directory.join("1.gfb"), b"GFB1").unwrap();
        let (tx, rx) = mpsc::channel();
        session.pending.lock().unwrap().insert(1, tx);
        {
            let mut process = session.process.lock().unwrap();
            let input = &mut process.as_mut().unwrap().input;
            writeln!(input, "1").unwrap();
            input.flush().unwrap();
        }
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            match rx.recv_timeout(Duration::from_millis(2)) {
                Ok(Event::Failure(message)) => {
                    assert!(message.contains("exited"), "{message}");
                    break;
                }
                Ok(Event::Start | Event::Resumed) => {}
                // The runtime may leave its input thread blocked after a
                // worker abort. The host's deadline retires the whole process;
                // it must never be reported as a thrown-program outcome.
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if String::from_utf8_lossy(&session.stderr.lock().unwrap())
                        .contains("assert failed")
                    {
                        break;
                    }
                    assert!(
                        Instant::now() < until,
                        "native worker did not report the injected abort"
                    );
                }
                other => panic!("native abort must be an infrastructure failure: {other:?}"),
            }
        }
        session.close();
        cleaned(&session);
        let stderr = session.stderr.lock().unwrap();
        let stderr = String::from_utf8_lossy(&stderr);
        assert!(
            stderr.contains("assert failed"),
            "worker abort was not injected: {stderr}"
        );
    }
}
