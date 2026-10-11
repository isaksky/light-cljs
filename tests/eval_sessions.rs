use light_cljs::{EvalOptions, EvalSession};
fn printed(s: &mut EvalSession, source: &str) -> String {
    let v = s.eval_string(source).unwrap().value;
    let text = s.print(v).unwrap();
    s.release(v).unwrap();
    text
}
#[test]
fn incremental_application_roots_and_macro_roots_are_independent() {
    let mut s = EvalSession::new(EvalOptions::default()).unwrap();
    assert_eq!(
        printed(
            &mut s,
            "(def a (atom 0)) (defn next-id [] (swap! a inc)) (defmacro id [] (next-id)) [(id) (id) @a]"
        ),
        "[1 2 0]"
    );
    assert_eq!(printed(&mut s, "(next-id) @a"), "1");
    assert_eq!(
        printed(&mut s, "(def saved next-id) (def a (atom 10)) (saved)"),
        "11"
    );
    let mut fresh = s.fresh_session().unwrap();
    assert!(fresh.eval_string("a").is_err());
    assert_eq!(printed(&mut fresh, "(+ 1 2)"), "3");
}
#[test]
fn language_errors_keep_effects_and_restore_scopes() {
    let mut s = EvalSession::new(EvalOptions::default()).unwrap();
    printed(&mut s, "(def a (atom 0)) (def ^:dynamic x 1)");
    assert!(
        s.eval_string("(binding [x 2] (swap! a inc) (throw (js/Error. \"boom\")))")
            .unwrap_err()
            .to_string()
            .contains("boom")
    );
    assert_eq!(printed(&mut s, "[@a x]"), "[1 1]");
    assert!(s.eval_string("(swap! a inc) missing").is_err());
    assert_eq!(printed(&mut s, "@a"), "2");
}

#[test]
fn typed_callbacks_reenter_and_preserve_owned_errors() {
    use light_cljs::compiler_data::{Datum, DatumKind};
    use light_cljs::{HostArity, HostFunction};
    use std::rc::Rc;
    let functions = vec![
        HostFunction {
            name: "host/inc".into(),
            arity: HostArity::Exact(1),
            callback: Rc::new(|call, args| {
                call.value(&Datum::new(DatumKind::Double(call.number(args[0])? + 1.0)))
            }),
        },
        HostFunction {
            name: "host/apply".into(),
            arity: HostArity::Exact(2),
            callback: Rc::new(|call, args| call.invoke(args[0], &args[1..])),
        },
        HostFunction {
            name: "host/fail".into(),
            arity: HostArity::Exact(0),
            callback: Rc::new(|_, _| Err(light_cljs::evaluator::EvalError("host failure".into()))),
        },
    ];
    let mut s = EvalSession::new(EvalOptions {
        functions,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        printed(&mut s, "(host/apply (fn [x] (host/inc x)) 41)"),
        "42"
    );
    assert!(
        s.eval_string("(host/inc)")
            .unwrap_err()
            .to_string()
            .contains("arity")
    );
    let error = s
        .eval_source(&light_cljs::source_provider::SourceInput::new(
            "failure.cljs",
            "(host/fail)",
        ))
        .unwrap_err();
    assert!(error.to_string().contains("failure.cljs:1:1"));
    assert!(error.to_string().contains("host failure"));
    assert_eq!(printed(&mut s, "42"), "42");
    drop(s);
    assert!(error.to_string().contains("host failure"));
}
#[test]
fn runtime_eval_and_macroexpand_use_the_compiler() {
    let mut s = EvalSession::new(EvalOptions::default()).unwrap();
    assert_eq!(printed(&mut s, "(eval '(def answer 42)) answer"), "42");
    assert_eq!(printed(&mut s, "(eval '(+ answer 1))"), "43");
    assert_eq!(
        printed(
            &mut s,
            "(defmacro m [x] (list '+ x 1)) (macroexpand '(m 41))"
        ),
        "(+ 41 1)"
    );
    assert_eq!(printed(&mut s, "(eval '(m 41))"), "42");
}
#[test]
fn namespaces_sources_and_reader_aliases_are_incremental() {
    use light_cljs::source_provider::{LoadRequest, SourceInput};
    use std::rc::Rc;
    let mut s=EvalSession::new(EvalOptions {source_provider:Some(Rc::new(|r:&LoadRequest|Ok(match String::from_utf16_lossy(r.namespace.name()).as_str() {
 "tools"=>Some(SourceInput::new("tools.cljc","(ns tools) #?(:cljs (def factor 2) :clj invalid) (defn twice [x] (* factor x)) (defmacro doubled [x] (twice x))")),_=>None}))),..Default::default()}).unwrap();
    assert_eq!(
        printed(
            &mut s,
            "(ns app (:require [tools :as t :refer [doubled]])) [(doubled 21) (t/doubled 21) (t/twice 21) ::t/name]"
        ),
        "[42 42 42 :tools/name]"
    );
    assert_eq!(s.namespace(), "app");
}
#[test]
fn output_input_arguments_and_print_scopes() {
    use std::{cell::RefCell, rc::Rc};
    let output = Rc::new(RefCell::new(String::new()));
    let sink = output.clone();
    let chunks = Rc::new(RefCell::new(vec!["hello\n42".to_owned()].into_iter()));
    let mut s = EvalSession::new(EvalOptions {
        output: Some(Rc::new(move |text| {
            sink.borrow_mut().push_str(text);
            Ok(())
        })),
        input: Some(Rc::new(move |_control| Ok(chunks.borrow_mut().next()))),
        arguments: vec!["one".into(), "two".into()],
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        printed(
            &mut s,
            "(println :hello) [(read-line) (read) *command-line-args* (with-out-str (println \"captured\"))]"
        ),
        "[\"hello\" 42 (\"one\" \"two\") \"captured\\n\"]"
    );
    assert_eq!(&*output.borrow(), ":hello\n");
}
#[test]
fn handles_are_local_and_reset_invalidates_old_ids() {
    let mut a = EvalSession::new(EvalOptions::default()).unwrap();
    let b = a.fresh_session().unwrap();
    let id = a.eval_string("(atom 1)").unwrap().value;
    assert!(b.print(id).unwrap_err().to_string().contains("another"));
    a.reset().unwrap();
    assert!(a.print(id).is_err());
    let retained = a.eval_string("42").unwrap().value;
    drop(a);
    assert!(b.print(retained).is_err());
}
#[test]
fn cancellation_and_limits_require_reset_but_host_compilation_recovers() {
    let mut s = EvalSession::new(EvalOptions::default()).unwrap();
    s.control().cancel();
    assert!(
        s.eval_string("42")
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
    s.reset().unwrap();
    assert_eq!(printed(&mut s, "42"), "42");
    let mut s = EvalSession::new(EvalOptions {
        engine: light_cljs::evaluator::EvaluatorOptions {
            poll_budget: 2000,
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap();
    assert!(
        s.eval_string("(try (loop [] (recur)) (catch :default e 42))")
            .is_err()
    );
    assert!(s.eval_string("42").is_err());
    s.reset().unwrap();
    assert_eq!(printed(&mut s, "42"), "42");
    let mut host = light_cljs::MacroHost::new();
    for _ in 0..2 {
        let source = "(def hits (atom 0)) (defn next-id [] (swap! hits inc)) (defmacro id [] (next-id)) [(id) (id) @hits]";
        let program =
            light_cljs::analyze_with_macros(source, &light_cljs::parse(source).unwrap(), &mut host)
                .unwrap();
        let engine = host.new_evaluator().unwrap();
        assert_eq!(
            engine.print(engine.evaluate(&program).unwrap()).unwrap(),
            "[1 2 0]"
        );
    }
}
#[test]
fn canonical_libraries_transducers_and_number_semantics() {
    let mut s = EvalSession::new(EvalOptions::default()).unwrap();
    assert_eq!(
        printed(
            &mut s,
            "(transduce (comp (map inc) (filter even?)) + 0 [1 2 3])"
        ),
        "6"
    );
    assert_eq!(
        printed(
            &mut s,
            "(ns app (:require [clojure.string :as str] [clojure.set :as set] [clojure.walk :as walk] [cljs.reader :as reader])) [(str/upper-case \"hi\") (set/union #{1} #{2}) (walk/postwalk (fn [x] (if (number? x) (inc x) x)) [1 2]) (reader/read-string \"{:a 3}\")]"
        ),
        "[\"HI\" #{1 2} [2 3] {:a 3}]"
    );
    assert_eq!(
        printed(&mut s, "[(/ 1 2) (+ 9007199254740992 1)]"),
        "[0.5 9007199254740992]"
    );
}
#[test]
fn configured_data_readers_analysis_and_host_bindings() {
    use light_cljs::compiler_data::{Datum, DatumKind, Name};
    use light_cljs::source_provider::SourceInput;
    let mut s = EvalSession::new(EvalOptions {
        data_readers: vec![(Name::new(None, "twice"), "(fn [x] (* 2 x))".into())],
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        printed(&mut s, "[#twice 21 (read-string \"#twice 21\")]"),
        "[42 42]"
    );
    let form = s
        .read_forms(&SourceInput::new("analysis.cljs", "(+ 40 2)"))
        .unwrap()
        .remove(0);
    let analyzed = s.analyze(&form).unwrap();
    let v = s.evaluate(&analyzed).unwrap().value;
    assert_eq!(s.number(v).unwrap(), 42.0);
    s.release(v).unwrap();
    let other = s.fresh_session().unwrap();
    assert!(other.evaluate(&analyzed).is_err());
    printed(&mut s, "(def x 1)");
    let two = s.value(&Datum::new(DatumKind::Double(2.0))).unwrap();
    let r = s.with_bindings(&[("x".into(), two)], |s| {
        assert_eq!(printed(s, "x"), "2");
        s.eval_string("(throw (js/Error. \"scope\"))")
    });
    assert!(r.is_err());
    assert_eq!(printed(&mut s, "x"), "1");
    assert!(s.bindings().contains(&"cljs.user/x".into()));
}
#[test]
fn compiler_policy_checks_aliases_and_runtime_eval() {
    use light_cljs::compiler_data::Name;
    let mut s = EvalSession::new(EvalOptions {
        policy: light_cljs::eval_session::NamePolicy {
            deny: [Name::new(None, "+")].into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap();
    assert!(
        s.eval_string("(+ 1 2)")
            .unwrap_err()
            .to_string()
            .contains("policy")
    );
    assert!(
        s.eval_string("(eval '(+ 1 2))")
            .unwrap_err()
            .to_string()
            .contains("policy")
    );
    assert_eq!(printed(&mut s, "(- 4 2)"), "2");
}
#[test]
fn failed_macro_compile_can_recover_in_same_session() {
    let mut s = EvalSession::new(EvalOptions::default()).unwrap();
    printed(
        &mut s,
        "(defmacro bad [] (throw (js/Error. \"macro failed\")))",
    );
    assert!(s.eval_string("(bad)").is_err());
    assert_eq!(printed(&mut s, "(defmacro good [] 42) (good)"), "42");
}
#[test]
fn session_drop_releases_callback_captures_and_cyclic_application_roots() {
    use light_cljs::{HostArity, HostFunction};
    use std::rc::Rc;
    let marker = Rc::new(());
    for _ in 0..8 {
        let captured = marker.clone();
        let mut s = EvalSession::new(EvalOptions {
            functions: vec![HostFunction {
                name: "host/identity".into(),
                arity: HostArity::Exact(1),
                callback: Rc::new(move |_, args| {
                    let _ = &captured;
                    Ok(args[0])
                }),
            }],
            ..Default::default()
        })
        .unwrap();
        let id = s
            .eval_string("(def cycle (atom nil)) (reset! cycle cycle) (host/identity 42)")
            .unwrap()
            .value;
        assert_eq!(s.number(id).unwrap(), 42.0);
        drop(s);
        assert_eq!(Rc::strong_count(&marker), 1);
    }
}
#[test]
fn source_output_limits_and_oom_are_explicit() {
    use std::rc::Rc;
    let mut source = EvalSession::new(EvalOptions {
        max_source_bytes: 8,
        ..Default::default()
    })
    .unwrap();
    assert!(
        source
            .eval_string("(+ 100 200)")
            .unwrap_err()
            .to_string()
            .contains("source byte")
    );
    assert_eq!(printed(&mut source, "42"), "42");
    let mut output = EvalSession::new(EvalOptions {
        max_output_bytes: 4,
        output: Some(Rc::new(|_| Ok(()))),
        ..Default::default()
    })
    .unwrap();
    assert!(
        output
            .eval_string("(print \"12345\")")
            .unwrap_err()
            .to_string()
            .contains("output byte")
    );
    assert_eq!(printed(&mut output, "42"), "42");
    let mut memory = EvalSession::new(EvalOptions {
        engine: light_cljs::evaluator::EvaluatorOptions {
            memory_limit: 4 * 1024 * 1024,
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap();
    let error=memory.eval_string("(try (js* \"(()=>{const a=[]; while(true) a.push(new Array(4096).fill(1));})()\") (catch :default e 42))").unwrap_err();
    assert!(error.to_string().contains("memory"), "{error}");
    assert!(memory.eval_string("42").is_err());
    memory.reset().unwrap();
    assert_eq!(printed(&mut memory, "42"), "42");
}
#[test]
fn protocol_tables_are_session_local() {
    let mut a = EvalSession::new(EvalOptions::default()).unwrap();
    assert_eq!(
        printed(
            &mut a,
            "(defprotocol P (f [x])) (extend-type number P (f [x] (+ x 1))) (f 41)"
        ),
        "42"
    );
    let mut b = a.fresh_session().unwrap();
    assert!(b.eval_string("(defprotocol P (f [x])) (f 41)").is_err());
    assert_eq!(printed(&mut a, "(f 41)"), "42");
}
#[test]
fn embedding_eval_reference_cases_keep_portable_expected_results() {
    for (source, expected) in [
        ("(eval '(+ 1 2))", "3"),
        ("(let [x 9] (eval '(let [x 4] x)))", "4"),
        ("(def x 7) (let [x 9] (eval 'x))", "7"),
        ("((eval '(fn [x] (inc x))) 4)", "5"),
        ("(eval '(def x 10)) x", "10"),
        ("(eval '(do (defmacro m [] 8) (m)))", "8"),
        ("(eval (read-string \"(+ 2 3)\"))", "5"),
        ("(eval 42)", "42"),
        ("(def a (atom 0)) (eval '(swap! a inc)) @a", "1"),
        ("(def x 1) (def x 2) x", "2"),
    ] {
        let mut s = EvalSession::new(EvalOptions::default()).unwrap();
        assert_eq!(printed(&mut s, source), expected, "{source}");
    }
    let mut s = EvalSession::new(EvalOptions::default()).unwrap();
    // Native ratio division threw; canonical JavaScript Number division does not.
    assert_eq!(
        printed(&mut s, "(try (eval '(/ 1 0)) (catch :default e 9))"),
        "##Inf"
    );
}
#[test]
fn default_reader_and_assertion_options() {
    let mut s = EvalSession::new(EvalOptions {
        default_data_reader: Some("(fn [tag value] [(str tag) value])".into()),
        assertions: false,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        printed(
            &mut s,
            "[#unknown 42 (read-string \"#unknown 42\") (assert false)]"
        ),
        "[[\"unknown\" 42] [\"unknown\" 42] nil]"
    );
}
#[test]
fn idle_time_does_not_expire_retained_values() {
    let mut s = EvalSession::new(EvalOptions {
        engine: light_cljs::evaluator::EvaluatorOptions {
            timeout: std::time::Duration::from_millis(250),
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap();
    let value = s.eval_string("42").unwrap().value;
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(s.number(value).unwrap(), 42.0);
    assert_eq!(printed(&mut s, "(+ 1 2)"), "3");
}
#[test]
fn exception_data_survives_the_session() {
    let mut s = EvalSession::new(EvalOptions::default()).unwrap();
    let error = s
        .eval_string("(throw (ex-info \"details\" {:answer 42}))")
        .unwrap_err();
    let data = error.data.clone().expect("owned ex-data");
    drop(s);
    let fresh = EvalSession::new(EvalOptions::default()).unwrap();
    let value = fresh.value(&data).unwrap();
    assert_eq!(fresh.print(value).unwrap(), "{:answer 42}");
    assert!(error.to_string().contains("details"));
}
#[test]
fn name_policy_preserves_local_and_global_shadowing() {
    use light_cljs::compiler_data::Name;
    let mut s = EvalSession::new(EvalOptions {
        policy: light_cljs::eval_session::NamePolicy {
            deny: [Name::new(None, "when")].into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap();
    assert_eq!(printed(&mut s, "(let [when (fn [x] x)] (when 42))"), "42");
    assert_eq!(printed(&mut s, "(def when (fn [x] x)) (when 42)"), "42");
}
