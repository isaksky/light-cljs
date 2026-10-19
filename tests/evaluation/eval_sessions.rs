use light_cljs::{EvalEngine, EvalOptions};

/// The engine this copy of the suite runs its sessions on. main.rs includes
/// the file once per engine: `eval_sessions` runs on the default engine (V8
/// where the build has it), and `eval_sessions_quickjs` on QuickJS.
fn engine() -> EvalEngine {
    if module_path!().ends_with("_quickjs") {
        EvalEngine::QuickJs
    } else {
        EvalEngine::default()
    }
}
/// A session on [`engine`], whatever engine its options name.
struct EvalSession(light_cljs::EvalSession);
impl EvalSession {
    fn new(mut options: EvalOptions) -> Result<Self, light_cljs::EvalDiagnostic> {
        options.backend = engine();
        light_cljs::EvalSession::new(options).map(Self)
    }
}
impl std::ops::Deref for EvalSession {
    type Target = light_cljs::EvalSession;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for EvalSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl std::fmt::Debug for EvalSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EvalSession({})", self.backend().name())
    }
}

/// A heap limit the session's own allocations reach soon: QuickJS starts in
/// well under 4 MB, and V8 refuses a limit under 16 MB at startup.
fn small_heap() -> usize {
    if engine() == EvalEngine::QuickJs {
        4 * 1024 * 1024
    } else {
        32 * 1024 * 1024
    }
}
fn printed(s: &mut light_cljs::EvalSession, source: &str) -> String {
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

/// Rust text cannot hold a lone surrogate: `string`, a host function's
/// `string` and the raw output callback reject one; printing escapes it.
#[test]
fn rust_string_conversion_rejects_lone_surrogates() {
    use light_cljs::{HostArity, HostFunction};
    use std::rc::Rc;
    let mut s = EvalSession::new(EvalOptions {
        functions: vec![HostFunction {
            name: "host/text".into(),
            arity: HostArity::Exact(1),
            callback: Rc::new(|call, args| {
                call.string(args[0])?;
                Ok(args[0])
            }),
        }],
        output: Some(Rc::new(|_| Ok(()))),
        ..Default::default()
    })
    .unwrap();
    let lone = s
        .eval_string("(str \"a\" (js/String.fromCharCode 0xD800))")
        .unwrap()
        .value;
    assert!(s.string(lone).is_err());
    assert_eq!(s.print(lone).unwrap(), "\"a\\ud800\"");
    let pair = s.eval_string("(str \"\\uD83D\\uDE00\")").unwrap().value;
    assert_eq!(s.string(pair).unwrap(), "\u{1F600}");
    assert!(
        s.eval_string("(host/text (js/String.fromCharCode 0xDC00))")
            .is_err()
    );
    assert!(
        s.eval_string("(print (js/String.fromCharCode 0xD800))")
            .is_err()
    );
    assert_eq!(printed(&mut s, "(host/text \"ok\")"), "\"ok\"");
}
/// A host function that panics ends the session, even when the language
/// catches the error, until `reset`.
#[test]
fn host_callback_panics_are_terminal_until_reset() {
    use light_cljs::{HostArity, HostFunction};
    use std::rc::Rc;
    let mut s = EvalSession::new(EvalOptions {
        functions: vec![HostFunction {
            name: "host/boom".into(),
            arity: HostArity::Exact(0),
            callback: Rc::new(|_, _| panic!("boom")),
        }],
        ..Default::default()
    })
    .unwrap();
    let error = s
        .eval_string("(try (host/boom) (catch :default _ :caught))")
        .unwrap_err();
    assert!(
        error.message.contains("invalidated") || error.message.contains("panicked"),
        "{error}"
    );
    assert!(s.eval_string("42").is_err());
    s.reset().unwrap();
    assert_eq!(printed(&mut s, "42"), "42");
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
            memory_limit: small_heap(),
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

#[test]
fn contextual_anonymous_reader_data_and_failures_reuse_the_session() {
    let mut s = EvalSession::new(EvalOptions::default()).unwrap();
    assert_eq!(
        printed(
            &mut s,
            "(let [f '#(%0 %-1)] [(count (second f)) (count (nth f 2))])"
        ),
        "[2 2]"
    );
    for (text, expected) in [
        ("#(%0)", "[0 1 false]"),
        ("#(%-1 %&)", "[2 2 true]"),
        ("#(%-2 %-2)", "[0 2 true]"),
        ("#(%2 %0 %-1 %&)", "[4 4 false]"),
    ] {
        let source = format!(
            r##"(let [f (read-string {text:?}) body (nth f 2)] [(count (second f)) (count body) (= (first body) (second body))])"##
        );
        assert_eq!(printed(&mut s, &source), expected);
        if text == "#(%2 %0 %-1 %&)" {
            assert_eq!(
                printed(
                    &mut s,
                    r##"(let [f (read-string "#(%2 %0 %-1 %&)")] (= (nth (nth f 2) 2) (last (nth f 2)) (last (second f))))"##
                ),
                "true"
            );
        }
    }
    assert_eq!(
        printed(
            &mut s,
            r##"(let [f (read-string "#(quote [%0 %-1 %&])") body (second (nth f 2))] [(count (second f)) (= (second body) (nth body 2) (last (second f)))])"##
        ),
        "[2 true]"
    );
    assert_eq!(
        printed(
            &mut s,
            r##"(let [f (read-string "#(`[%0 %-1 %&])")] (count (second f)))"##
        ),
        "2"
    );
    for bad in ["#(#())", "#(%3/2)", "#(%foo)"] {
        assert_eq!(
            printed(
                &mut s,
                &format!("(try (read-string {bad:?}) :failed (catch :default _ :caught))")
            ),
            ":caught"
        );
        assert_eq!(
            printed(
                &mut s,
                r##"(let [f (read-string "#(%-1 %&)")] (= (last (second f)) (first (nth f 2)) (second (nth f 2))))"##
            ),
            "true"
        );
    }
    assert!(s.eval_string("(#(%0) 1)").is_err());
    assert_eq!(printed(&mut s, "(#(vector %-1 %&) 3 4)"), "[(3 4) (3 4)]");
}

/// The session runs pending promise jobs after each top-level form
/// (light-cljs-295), as an event loop runs them after a script: `.then`
/// reactions, and the rest of an `^:async` body after an `await`, have run
/// before the next form, but not inside the form that queued them, and not
/// between the forms of a namespace the session loads.
#[test]
fn promise_jobs_run_after_each_top_level_form() {
    use light_cljs::source_provider::{LoadRequest, SourceInput};
    use std::rc::Rc;
    let mut s = EvalSession::new(EvalOptions {
        source_provider: Some(Rc::new(|r: &LoadRequest| {
            Ok(
                (String::from_utf16_lossy(r.namespace.name()) == "app.jobs").then(|| {
                    SourceInput::new(
                        "app/jobs.cljs",
                        "(ns app.jobs)\n\
                         (def log (atom []))\n\
                         (.then (js/Promise.resolve) #(swap! log conj :job))\n\
                         (swap! log conj :loaded)\n",
                    )
                }),
            )
        })),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        printed(
            &mut s,
            "(def r (atom [])) (def v (atom nil))
             (defn ^:async f [x] (swap! r conj :before) (swap! r conj (inc (await x))) :done)
             (do (.then (f (js/Promise.resolve 1)) #(reset! v %)) [@r @v])"
        ),
        "[[:before] nil]"
    );
    assert_eq!(printed(&mut s, "[@r @v]"), "[[:before 2] :done]");
    // A rejection reaches its handler; a later form of the same source sees
    // what the reactions of an earlier one did.
    assert_eq!(
        printed(
            &mut s,
            "(.catch (f (js/Promise.reject (js/Error. \"no\"))) #(reset! v (ex-message %))) @v"
        ),
        "\"no\""
    );
    assert_eq!(
        printed(
            &mut s,
            "(def log (atom [])) (.then (js/Promise.resolve) #(swap! log conj 1)) (swap! log conj 2)"
        ),
        "[1 2]"
    );
    assert_eq!(
        printed(&mut s, "(require 'app.jobs) @app.jobs/log"),
        "[:loaded :job]"
    );
    // A failed form's jobs run too, before the next form.
    assert!(
        s.eval_string("(.then (js/Promise.resolve) #(swap! log conj 3)) (throw (js/Error. \"x\"))")
            .is_err()
    );
    assert!(
        s.eval_string(
            "(do (.then (js/Promise.resolve) #(swap! log conj 4)) (throw (js/Error. \"x\")))"
        )
        .is_err()
    );
    assert_eq!(printed(&mut s, "@log"), "[1 2 3 4]");
}

/// Promise jobs run under the operation's deadline, poll budget and memory
/// limit, which end a job that loops, an endless chain of jobs and a job
/// that allocates without bound as they end any other evaluation.
#[test]
fn promise_jobs_stay_within_the_session_limits() {
    use light_cljs::evaluator::EvaluatorOptions;
    for (engine, source, expected) in [
        (
            EvaluatorOptions {
                timeout: std::time::Duration::from_millis(200),
                poll_budget: u64::MAX,
                ..Default::default()
            },
            "(.then (js/Promise.resolve) (fn [_] (try (loop [] (recur)) (catch :default _ 0))))",
            "deadline exceeded",
        ),
        (
            EvaluatorOptions {
                timeout: std::time::Duration::from_millis(200),
                poll_budget: u64::MAX,
                ..Default::default()
            },
            "(defn ^:async f [] (await nil) (loop [] (recur))) (.catch (f) (fn [_] 0))",
            "deadline exceeded",
        ),
        (
            EvaluatorOptions {
                poll_budget: 2000,
                ..Default::default()
            },
            "(defn again [_] (.then (js/Promise.resolve) again)) (again nil)",
            "poll budget exhausted",
        ),
        (
            EvaluatorOptions {
                memory_limit: small_heap(),
                ..Default::default()
            },
            "(def a #js []) (defn grow [_] (.push a (.fill (js/Array. 4096) 1)) (.then (js/Promise.resolve) grow)) (grow nil)",
            "memory",
        ),
    ] {
        let mut s = EvalSession::new(EvalOptions {
            engine,
            ..Default::default()
        })
        .unwrap();
        let error = s.eval_string(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{source}: {error}");
        assert!(s.eval_string("42").is_err(), "{source}");
        s.reset().unwrap();
        assert_eq!(printed(&mut s, "42"), "42");
    }
}

/// A file `load_file` loads runs as one script, its jobs after its last form
/// and before the method returns; one the language `load-file` loads runs
/// inside its form, whose jobs run after the whole form.
#[test]
fn promise_jobs_run_after_a_loaded_file() {
    use light_cljs::source_provider::SourceInput;
    use std::rc::Rc;
    let mut s = EvalSession::new(EvalOptions {
        file_provider: Some(Rc::new(|_: &light_cljs::eval_session::FileLoadRequest| {
            Ok(Some(SourceInput::new(
                "jobs.cljs",
                "(.then (js/Promise.resolve) #(swap! log conj :job))\n\
                 (swap! log conj :loaded)\n",
            )))
        })),
        ..Default::default()
    })
    .unwrap();
    printed(&mut s, "(def log (atom []))");
    let value = s.load_file("jobs.cljs").unwrap().value;
    s.release(value).unwrap();
    assert_eq!(printed(&mut s, "@log"), "[:loaded :job]");
    assert_eq!(
        printed(
            &mut s,
            "(reset! log []) (do (load-file \"jobs.cljs\") @log)"
        ),
        "[:loaded]"
    );
    assert_eq!(printed(&mut s, "@log"), "[:loaded :job]");
}

/// `run_jobs` budgets the engine's stack from its own Rust frame, as every
/// other outer operation does, so a caller deeper than the last operation
/// does not shrink the stack its jobs get.
#[test]
fn promise_jobs_refresh_the_stack_boundary() {
    use light_cljs::EvalDiagnostic;
    use light_cljs::source_provider::SourceInput;
    let mut s = EvalSession::new(EvalOptions {
        engine: light_cljs::evaluator::EvaluatorOptions {
            stack_limit: 256 * 1024,
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap();
    printed(&mut s, "(def r (atom nil))");
    let forms = s
        .read_forms(&SourceInput::new(
            "<jobs>.cljs",
            "(.then (js/Promise.resolve) #(reset! r :ran))",
        ))
        .unwrap();
    let analyzed = s.analyze(&forms[0]).unwrap();
    let value = s.evaluate(&analyzed).unwrap().value;
    s.release(value).unwrap();
    #[inline(never)]
    fn deeper(s: &EvalSession, depth: usize) -> Result<(), EvalDiagnostic> {
        let pad = std::hint::black_box([0u8; 16 * 1024]);
        let result = if depth == 0 {
            s.run_jobs()
        } else {
            deeper(s, depth - 1)
        };
        std::hint::black_box(&pad);
        result
    }
    deeper(&s, 32).unwrap();
    assert_eq!(printed(&mut s, "@r"), ":ran");
}

/// An exception that leaves a job (a `queueMicrotask` callback's; a `.then`
/// reaction's rejects its promise instead) is the form's error, with its
/// value and data, and the remaining jobs still run. A failed form keeps its
/// own exception while its jobs run, even when they evaluate.
#[test]
fn promise_job_exceptions_keep_their_values() {
    let mut s = EvalSession::new(EvalOptions::default()).unwrap();
    printed(&mut s, "(def log (atom []))");
    let error = s
        .eval_string(
            "(do (js/queueMicrotask #(throw (ex-info \"job\" {:tag :kept})))
                 (js/queueMicrotask #(throw (ex-info \"later\" {})))
                 (.then (js/Promise.resolve) #(swap! log conj :ran))
                 :value)
             (swap! log conj :next-form)",
        )
        .unwrap_err();
    assert!(error.message.contains("job"), "{error}");
    let data = s.value(&error.data.expect("owned ex-data")).unwrap();
    assert_eq!(s.print(data).unwrap(), "{:tag :kept}");
    let thrown = s.last_exception().unwrap();
    let data = s.lookup("cljs.core/ex-data").unwrap();
    let tag = s.invoke(data, &[thrown]).unwrap();
    assert_eq!(s.print(tag).unwrap(), "{:tag :kept}");
    assert_eq!(printed(&mut s, "@log"), "[:ran]");
    let error = s
        .eval_string(
            "(do (.then (js/Promise.resolve) (fn [_] (eval '(swap! log conj :evaluated))))
                 (js/queueMicrotask #(throw (ex-info \"job\" {})))
                 (throw (ex-info \"original\" {:tag :form})))",
        )
        .unwrap_err();
    assert!(error.message.contains("original"), "{error}");
    let thrown = s.last_exception().unwrap();
    let tag = s.invoke(data, &[thrown]).unwrap();
    assert_eq!(s.print(tag).unwrap(), "{:tag :form}");
    assert_eq!(printed(&mut s, "@log"), "[:ran :evaluated]");
}

/// A promise left rejected with no handler attached once the jobs after a
/// form run out is reported once, after the form, in order; one that a job
/// handles later in the same drain, or an `await` or `.catch` handles, is
/// not. Under `Fail` the first is the form's error, with its reason as the
/// last exception, unless the form or a job failed first.
#[test]
fn unhandled_promise_rejections_are_reported_after_the_jobs() {
    use light_cljs::UnhandledRejections;
    let report = |reason: &str| format!("unhandled promise rejection: {reason}");
    let define = "(def log (atom []))
        (defn ^:async boom [] (await nil) (throw (ex-info \"boom\" {:a 1})))
        (deftype Loud [] IPrintWithWriter (-pr-writer [_ w _] (eval nil) (-write w \"#loud\")))";
    let mut s = EvalSession::new(EvalOptions::default()).unwrap();
    printed(&mut s, define);
    assert_eq!(printed(&mut s, "(boom) :value"), ":value");
    assert_eq!(
        s.take_unhandled_rejections(),
        [report("#error {:message \"boom\", :data {:a 1}}")]
    );
    assert!(s.take_unhandled_rejections().is_empty());
    printed(
        &mut s,
        "(let [p (js/Promise.reject 1)]
           (.then (js/Promise.resolve) (fn [_] (.catch p #(swap! log conj %)))))
         (defn ^:async caught [] (try (await (boom)) (catch :default e (swap! log conj (ex-message e)))))
         (caught)
         (.catch (boom) #(swap! log conj (:a (ex-data %))))",
    );
    assert!(s.take_unhandled_rejections().is_empty());
    assert_eq!(printed(&mut s, "@log"), "[1 \"boom\" 1]");
    // No job needs to run; a handler a later form attaches does not withdraw
    // a report, and a failed form's rejections are reported too.
    printed(
        &mut s,
        "(def p (js/Promise.reject :first)) (do (js/Promise.reject :second) nil) (.catch p identity)",
    );
    assert_eq!(
        s.take_unhandled_rejections(),
        [report(":first"), report(":second")]
    );
    assert!(
        s.eval_string("(do (js/Promise.reject :failed) (throw (js/Error. \"x\")))")
            .is_err()
    );
    assert_eq!(s.take_unhandled_rejections(), [report(":failed")]);
    // Printing a reason keeps the last exception, even when its printer
    // evaluates, and a reason that does not print at all is still reported.
    assert!(
        s.eval_string("(do (js/Promise.reject (Loud.)) (throw (ex-info \"form\" {:tag :form})))")
            .is_err()
    );
    assert_eq!(s.take_unhandled_rejections(), [report("#loud")]);
    let thrown = s.last_exception().unwrap();
    let ex_data = s.lookup("cljs.core/ex-data").unwrap();
    let data = s.invoke(ex_data, &[thrown]).unwrap();
    assert_eq!(s.print(data).unwrap(), "{:tag :form}");
    assert_eq!(
        printed(
            &mut s,
            "(do (js/Promise.reject (js* \"new Proxy({}, {get() {throw new Error('no access')}})\"))
                 (js/Promise.reject :second)
                 :ok)"
        ),
        ":ok"
    );
    assert_eq!(
        s.take_unhandled_rejections(),
        [report("<unprintable reason>"), report(":second")]
    );
    // An embedder that evaluates a form itself reports them with `run_jobs`.
    let forms = s
        .read_forms(&light_cljs::source_provider::SourceInput::new(
            "<jobs>.cljs",
            "(boom)",
        ))
        .unwrap();
    let analyzed = s.analyze(&forms[0]).unwrap();
    let value = s.evaluate(&analyzed).unwrap().value;
    s.release(value).unwrap();
    assert!(s.take_unhandled_rejections().is_empty());
    s.run_jobs().unwrap();
    assert_eq!(
        s.take_unhandled_rejections(),
        [report("#error {:message \"boom\", :data {:a 1}}")]
    );

    let mut s = EvalSession::new(EvalOptions {
        unhandled_rejections: UnhandledRejections::Fail,
        ..Default::default()
    })
    .unwrap();
    printed(&mut s, define);
    let error = s
        .eval_string("(boom) (swap! log conj :next-form)")
        .unwrap_err();
    assert_eq!(
        error.message,
        report("#error {:message \"boom\", :data {:a 1}}")
    );
    let data = s.value(&error.data.expect("owned ex-data")).unwrap();
    assert_eq!(s.print(data).unwrap(), "{:a 1}");
    let thrown = s.last_exception().unwrap();
    let ex_data = s.lookup("cljs.core/ex-data").unwrap();
    let data = s.invoke(ex_data, &[thrown]).unwrap();
    assert_eq!(s.print(data).unwrap(), "{:a 1}");
    assert_eq!(printed(&mut s, "@log"), "[]");
    assert_eq!(
        printed(&mut s, "(.catch (boom) #(swap! log conj :caught)) @log"),
        "[:caught]"
    );
    let error = s.eval_string("(js/Promise.reject (Loud.))").unwrap_err();
    assert_eq!(error.message, report("#loud"));
    let thrown = s.last_exception().unwrap();
    let loud = s.eval_string("#(instance? Loud %)").unwrap().value;
    let is_loud = s.invoke(loud, &[thrown]).unwrap();
    assert_eq!(s.print(is_loud).unwrap(), "true");
    // Those are dropped unprinted: an endless reason would never print.
    let error = s
        .eval_string("(do (js/Promise.reject (range)) (throw (ex-info \"form\" {})))")
        .unwrap_err();
    assert!(error.message.contains("form"), "{error}");
    let error = s
        .eval_string(
            "(do (js/Promise.reject (range)) (js/queueMicrotask #(throw (ex-info \"job\" {}))) nil)",
        )
        .unwrap_err();
    assert!(error.message.contains("job"), "{error}");
    assert_eq!(printed(&mut s, ":clean"), ":clean");
    assert!(s.take_unhandled_rejections().is_empty());
}
#[test]
fn macroexpand_seed_preserves_pending_reader_declarations() {
    // Expansions the macro session does not run (compiler seeds, nonmacro
    // calls) must not consume a declaration's registration, or a later
    // expansion would not see it declared.
    let mut s = EvalSession::new(EvalOptions::default()).unwrap();
    assert_eq!(
        printed(
            &mut s,
            "(defmacro probe [] (list 'quote (second (read-string \"`map\")))) (def map 1) nil"
        ),
        "nil"
    );
    assert!(printed(&mut s, "(macroexpand '(let [x 1] x))").contains("x 1"));
    assert_eq!(printed(&mut s, "(macroexpand '(+ 1 2))"), "(+ 1 2)");
    assert_eq!(printed(&mut s, "(probe)"), "cljs.user/map");
}
/// A session's macros read its build configuration, as `--watch --config`
/// gives the watch's session.
#[test]
fn session_macros_read_the_build_configuration() {
    let mut configuration = light_cljs::BuildConfiguration::new();
    configuration.insert("app/flavor", ":dev").unwrap();
    configuration.insert("app/ports", "[80 443]").unwrap();
    let mut s = EvalSession::new(EvalOptions {
        configuration,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        printed(
            &mut s,
            "(require '[light.build :as b]) \
             (defmacro setting [key] (list 'quote (b/config key))) \
             [(setting :app/flavor) (setting :app/ports) (setting :app/unset)]"
        ),
        "[:dev [80 443] nil]"
    );
}

/// `light.build` serves a session's macros for its whole life with its
/// source provider's resources: a `./` path resolves against the current
/// namespace's directory, in an evaluation and in `macroexpand`.
#[test]
fn session_macros_read_resources_relative_to_the_current_namespace() {
    let root = crate::temp_dir::TempDir::new("eval-build-resource");
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::write(root.join("app/greeting.txt"), "hello from app").unwrap();
    std::fs::write(root.join("greeting.txt"), "hello from the root").unwrap();
    std::fs::write(
        root.join("app/macros.cljs"),
        "(ns app.macros (:require [light.build :as b]))\n\
         (defmacro greeting [] (b/resource \"./greeting.txt\"))",
    )
    .unwrap();
    let mut s = EvalSession::new(EvalOptions {
        source_provider: Some(std::rc::Rc::new(
            light_cljs::source_provider::FileSourceProvider::new(vec![root.path().clone()]),
        )),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        printed(
            &mut s,
            "(ns app.core (:require [app.macros :refer [greeting]])) (greeting)"
        ),
        "\"hello from app\""
    );
    assert_eq!(
        printed(&mut s, "(macroexpand '(greeting))"),
        "\"hello from app\""
    );
    assert_eq!(
        printed(
            &mut s,
            "(ns greeter (:require [app.macros :refer [greeting]])) (greeting)"
        ),
        "\"hello from the root\""
    );
}
/// A runtime that ran a session's units has the reader context and every
/// definition of each namespace as when each `def`'s unit registered the
/// whole namespace, while the units register only the definitions they add
/// (light-cljs-366): syntax-quote of `read-string` qualifies each definition
/// to its namespace, and a core name once the namespace defines it, through
/// an `ns` form that changes the namespace's aliases, a unit that fails and
/// a definition of another namespace.
#[test]
fn runtime_reader_sees_every_definition_a_unit_adds() {
    let mut s = EvalSession::new(EvalOptions::default()).unwrap();
    for n in 0..40 {
        printed(&mut s, &format!("(def v{n} {n})"));
    }
    assert_eq!(
        printed(
            &mut s,
            "[(read-string \"`v0\") (read-string \"`v39\") (read-string \"`inc\")]"
        ),
        "[(quote cljs.user/v0) (quote cljs.user/v39) (quote cljs.core/inc)]"
    );
    printed(&mut s, "(def inc 5)");
    assert!(s.eval_string("(js* \"(\")").is_err());
    printed(&mut s, "(def after-failure 1)");
    printed(&mut s, "(ns app.other) (def o 1)");
    printed(
        &mut s,
        "(ns cljs.user (:require [app.other :as-alias other]))",
    );
    printed(&mut s, "(def after-ns 2)");
    printed(
        &mut s,
        "(in-ns 'app.other) (def later 3) (in-ns 'cljs.user)",
    );
    assert_eq!(
        printed(
            &mut s,
            "[(read-string \"`inc\") (read-string \"`after-failure\") (read-string \"`after-ns\") \
             (read-string \"`v39\") (read-string \"::other/k\") (read-string \"`dec\") \
             (binding [*ns* (find-ns 'app.other)] [(read-string \"`o\") (read-string \"`later\")])]"
        ),
        "[(quote cljs.user/inc) (quote cljs.user/after-failure) (quote cljs.user/after-ns) \
         (quote cljs.user/v39) :app.other/k (quote cljs.core/dec) \
         [(quote app.other/o) (quote app.other/later)]]"
    );
}
/// An analyzed form's unit registers its namespaces in full, so evaluated
/// after later units replaced the namespace's aliases it reads with the
/// aliases it was compiled with, as before light-cljs-366; and a runtime
/// `eval` that defines a name while a read is in progress, whose unit is
/// such a unit too, leaves the bindings that read qualifies with as they
/// were when it began.
#[test]
fn late_and_nested_units_keep_their_reader_contexts() {
    use light_cljs::compiler_data::Name;
    use light_cljs::source_provider::SourceInput;
    let mut s = EvalSession::new(EvalOptions {
        data_readers: vec![(
            Name::new(None, "late"),
            "(fn [x] (eval '(def inc 7)) x)".into(),
        )],
        ..Default::default()
    })
    .unwrap();
    printed(&mut s, "(ns cljs.user (:require [app.old :as-alias old]))");
    printed(&mut s, "(def a 1)");
    let form = s
        .read_forms(&SourceInput::new(
            "late.cljs",
            "(do (def delayed 1) (read-string \"::old/k\"))",
        ))
        .unwrap()
        .remove(0);
    let analyzed = s.analyze(&form).unwrap();
    printed(&mut s, "(ns cljs.user (:require [app.new :as-alias new]))");
    printed(&mut s, "(def b 2)");
    let value = s.evaluate(&analyzed).unwrap().value;
    assert_eq!(s.print(value).unwrap(), ":app.old/k");
    s.release(value).unwrap();
    assert_eq!(
        printed(
            &mut s,
            "[(read-string \"[`inc #late 1 `inc]\") (read-string \"`inc\")]"
        ),
        "[[(quote cljs.core/inc) 1 (quote cljs.core/inc)] (quote cljs.user/inc)]"
    );
}
