use light_cljs::compiler_data::DatumKind;
use light_cljs::eval_session::{FileLoadRequest, FileProvider};
use light_cljs::evaluator::EvalError;
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::source_provider::{LoadRequest, SourceInput};
use light_cljs::{EvalOptions, EvalSession};
use light_cljs::{MacroHost, analyze_with_macros, emit_js, parse};
use std::{
    cell::{Cell, RefCell},
    process::Command,
    rc::Rc,
};

fn printed(session: &mut EvalSession, source: &str) -> String {
    let value = session.eval_string(source).unwrap().value;
    let result = session.print(value).unwrap();
    session.release(value).unwrap();
    result
}

#[test]
fn clocks_random_sources_and_taps_are_explicit_session_capabilities() {
    let mut session = EvalSession::new(EvalOptions {
        clock: Some(Rc::new(|| Ok(123.5))),
        random: Some(Rc::new(|| Ok(0.25))),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        printed(
            &mut session,
            "[(system-time) (rand) (rand 8) (rand-int 8) (rand-nth [:a :b :c :d]) (random-uuid)]"
        ),
        "[123.5 0.25 2 2 :b #uuid \"44444444-4444-4444-8444-444444444444\"]"
    );
    assert_eq!(
        printed(
            &mut session,
            r#"
        (def events (atom []))
        (def good (fn [x] (swap! events conj x)))
        (def bad (fn [_] (throw (js/Error. "tap failure"))))
        [(add-tap bad) (add-tap good) (add-tap good) (tap> :one)
         (remove-tap good) (tap> :two) @events]
    "#
        ),
        "[nil nil nil true nil true [:one]]"
    );
    let mut fresh = session.fresh_session().unwrap();
    assert_eq!(
        printed(&mut fresh, "[(system-time) (tap> :fresh)]"),
        "[123.5 true]"
    );
    assert_eq!(printed(&mut session, "@events"), "[:one]");
    assert_eq!(
        printed(
            &mut session,
            r#"
      (let [jobs (atom []) hits (atom []) listener (fn [x] (swap! hits conj x))]
        (add-tap listener)
        [(binding [*exec-tap-fn* (fn [task] (swap! jobs conj task) true)] (tap> :scheduled))
         @hits (do ((first @jobs)) @hits) (remove-tap listener)
         (binding [*exec-tap-fn* (fn [_] false)] (tap> :declined))])
    "#
        ),
        "[true [] [:scheduled] nil false]"
    );
}

#[test]
fn runtime_scheduler_admission_bounded_reentrancy_and_arities() {
    let checks = r#"
      let tasks = [], events = [];
      c.configureHostHooks({clock: () => 17, random: () => 0.5, tapSchedule: task => tasks.push(task), maxPendingTaps: 2});
      const listener = value => events.push(value);
      c.addTap(listener);
      assert.equal(c.tapGreater(1), true); assert.equal(c.tapGreater(2), true);
      assert.equal(c.tapGreater(3), false); assert.deepEqual(events, []);
      tasks.shift()(); c.removeTap(listener); tasks.shift()();
      assert.deepEqual(events, [1]);
      assert.equal(c.tapGreater(4), true); c.addTap(listener); tasks.shift()();
      assert.deepEqual(events, [1, 4]); c.removeTap(listener);
      assert.equal(c.systemTime(), 17); assert.equal(c.randInt(10), 5);
      assert.equal(c.prStr(c.shuffle(c.vector(1,2,3))), '[1 3 2]');
      c.configureHostHooks({tapSchedule: null, maxPendingTaps: 4});
      let calls = 0, rejected = 0;
      c.addTap(() => { calls++; if (!c.tapGreater(calls)) rejected++; });
      assert.equal(c.tapGreater(0), true); assert.equal(calls, 4); assert.equal(rejected, 1);
      for (const name of ['systemTime','addTap','removeTap','tapGreater','loadFile']) {
        assert.throws(() => c[name](1,2), /arity/);
      }
      c.configureHostHooks({clock: () => NaN, random: () => 1});
      assert.throws(() => c.systemTime(), /finite/); assert.throws(() => c.rand(), /\[0, 1\)/);
      assert.throws(() => c.configureHostHooks({maxPendingTaps: 0}), /positive/);
    "#;
    let runtime = include_str!("../src/js_runtime.js");
    let script = format!(
        "const assert = require('node:assert/strict'), rt = {runtime}, c = rt.core; {checks}"
    );
    let output = Command::new("node").args(["-e", &script]).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let expression = format!(
        r#"(() => {{
      const c = rt.core, assert = {{
        equal: (a,b) => {{ if (a !== b) throw new Error('Expected equal values: '+a+' '+b); }},
        deepEqual: (a,b) => {{ if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error('Expected equal arrays'); }},
        throws: (f, pattern) => {{ let caught = false; try {{ f(); }} catch (error) {{ caught = pattern.test(error.message); }} if (!caught) throw new Error('Expected matching exception'); }}
      }};
      {checks}
      return true;
    }})()"#
    );
    let result = evaluator.evaluate_js(&expression).unwrap();
    assert_eq!(evaluator.print(result).unwrap(), "true");
    evaluator.release(result).unwrap();
}

#[test]
fn host_hooks_have_macro_application_phase_witnesses() {
    let source = r#"
      (defn probe []
        (let [events (atom []) accepted (atom [])
              listener (fn step [n] (swap! events conj n)
                         (when (< n 3) (swap! accepted conj (tap> (inc n)))))]
          (add-tap listener)
          (add-tap listener)
          (let [initial (tap> 0)]
            (remove-tap listener)
            (tap> :removed)
             [(number? (system-time)) initial @events @accepted
             (try (load-file "denied.cljs") false (catch :default _ true))
             (binding [*exec-tap-fn* (fn [_] false)] (tap> :declined))])))
      (defmacro phase-snapshot [] (probe))
      [(phase-snapshot) (probe)]
    "#;
    let program =
        analyze_with_macros(source, &parse(source).unwrap(), &mut MacroHost::new()).unwrap();
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let result = evaluator.evaluate(&program).unwrap();
    let expected = "[[true true [0 1 2 3] [true true true] true false] [true true [0 1 2 3] [true true true] true false]]";
    assert_eq!(evaluator.print(result).unwrap(), expected);
    evaluator.release(result).unwrap();
    let output = Command::new("node")
        .args([
            "-e",
            &format!("console.log(String({}));", emit_js(&program)),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), expected);
    let mut denied = EvalSession::new(EvalOptions {
        clock: Some(Rc::new(|| Err(EvalError("clock denied".into())))),
        ..Default::default()
    })
    .unwrap();
    assert!(
        denied
            .eval_string("(system-time)")
            .unwrap_err()
            .to_string()
            .contains("clock denied")
    );
    assert_eq!(printed(&mut denied, "42"), "42");
}

fn provider(requests: Rc<RefCell<Vec<FileLoadRequest>>>) -> FileProvider {
    Rc::new(move |request| {
        requests.borrow_mut().push(request.clone());
        Ok(match request.path.as_str() {
            "tools.cljc" => Some(SourceInput::new(
                "capability/tools.cljc",
                "(ns tools) #?(:cljs (def factor 2) :clj invalid) (defn twice [x] (* factor x)) (defmacro doubled [x] (twice x)) (twice 21)",
            )),
            "app.cljs" => Some(SourceInput::new(
                "capability/app.cljs",
                "(ns app (:require [tools :as t])) (t/twice 21)",
            )),
            "bad.cljs" => Some(SourceInput::new(
                "capability/bad.cljs",
                "(def retained 7)\nmissing",
            )),
            "throw.cljs" => Some(SourceInput::new(
                "capability/throw.cljs",
                "\n(throw (ex-info \"file failed\" {:loaded true}))",
            )),
            "identity.cljs" => Some(SourceInput::new(
                "capability/identity.cljs",
                "(throw original-error)",
            )),
            "scalar.cljs" => Some(SourceInput::new("capability/scalar.cljs", "\n(throw nil)")),
            "recursive.cljs" => Some(SourceInput::new(
                "capability/recursive.cljs",
                "(load-file \"recursive.cljs\")",
            )),
            "disguised.cljs" => Some(SourceInput::new("retired.clj", "42")),
            _ => None,
        })
    })
}

#[test]
fn load_file_uses_one_compiler_and_heap_with_macros_namespaces_and_require() {
    let requests = Rc::new(RefCell::new(Vec::new()));
    let namespace_loads = Rc::new(Cell::new(0));
    let loads = namespace_loads.clone();
    let mut session = EvalSession::new(EvalOptions {
        file_provider: Some(provider(requests.clone())),
        source_provider: Some(Rc::new(move |_: &LoadRequest| {
            loads.set(loads.get() + 1);
            Ok(Some(SourceInput::new(
                "dependency/tools.cljs",
                "(ns tools) (defn twice [x] (* x 2))",
            )))
        })),
        ..Default::default()
    })
    .unwrap();
    let result = session.load_file("tools.cljc").unwrap();
    assert_eq!(session.number(result.value).unwrap(), 42.0);
    session.release(result.value).unwrap();
    assert_eq!(session.namespace(), "tools");
    assert_eq!(printed(&mut session, "(ns-name *ns*)"), "tools");
    assert_eq!(
        printed(&mut session, "[(doubled 21) (twice 21)]"),
        "[42 42]"
    );
    assert_eq!(printed(&mut session, "(load-file \"app.cljs\")"), "42");
    assert_eq!(session.namespace(), "app");
    assert_eq!(printed(&mut session, "(ns-name *ns*)"), "app");
    assert_eq!(namespace_loads.get(), 1);
    assert_eq!(
        printed(&mut session, "(require '[tools :as t]) (t/twice 5)"),
        "10"
    );
    assert_eq!(namespace_loads.get(), 1);
    assert_eq!(requests.borrow()[1].requesting_namespace, "tools");
}

#[test]
fn file_diagnostics_history_recovery_and_capability_denial_are_stable() {
    let requests = Rc::new(RefCell::new(Vec::new()));
    let mut session = EvalSession::new(EvalOptions {
        file_provider: Some(provider(requests.clone())),
        ..Default::default()
    })
    .unwrap();
    let error = session.eval_string("(load-file \"bad.cljs\")").unwrap_err();
    assert!(
        error.to_string().contains("capability/bad.cljs:2:1"),
        "{error}"
    );
    assert_eq!(printed(&mut session, "retained"), "7");
    let error = session
        .eval_string("(load-file \"throw.cljs\")")
        .unwrap_err();
    assert!(
        error.to_string().contains("capability/throw.cljs:2:1"),
        "{error}"
    );
    assert!(
        error.data.is_some(),
        "loaded ex-info data survives the host bridge"
    );
    let exception = session.last_exception().unwrap();
    assert!(session.print(exception).unwrap().contains("file failed"));
    session.release(exception).unwrap();
    assert_eq!(
        printed(
            &mut session,
            "(try (load-file \"throw.cljs\") (catch :default e (ex-data e)))"
        ),
        "{:loaded true}"
    );
    assert_eq!(
        printed(
            &mut session,
            "(def original-error (ex-info \"identity\" {:same true})) (try (load-file \"identity.cljs\") (catch :default e (identical? e original-error)))"
        ),
        "true"
    );
    assert_eq!(
        printed(
            &mut session,
            "(try (eval '(throw original-error)) (catch :default e (identical? e original-error)))"
        ),
        "true"
    );
    let scalar = session
        .eval_string("(load-file \"scalar.cljs\")")
        .unwrap_err();
    assert!(
        scalar.to_string().contains("capability/scalar.cljs:2:1"),
        "{scalar}"
    );
    assert_eq!(
        printed(
            &mut session,
            "(try (load-file \"scalar.cljs\") (catch :default e (nil? e)))"
        ),
        "true"
    );
    assert_eq!(printed(&mut session, "(+ retained 1)"), "8");
    assert!(
        session
            .eval_string("(load-file \"recursive.cljs\")")
            .unwrap_err()
            .to_string()
            .contains("file load cycle")
    );
    assert_eq!(printed(&mut session, "42"), "42");
    let calls = requests.borrow().len();
    assert!(
        session
            .load_file("native.clj")
            .unwrap_err()
            .to_string()
            .contains("retired")
    );
    assert_eq!(requests.borrow().len(), calls);
    assert!(
        session
            .load_file("disguised.cljs")
            .unwrap_err()
            .to_string()
            .contains("retired")
    );
    let mut denied = EvalSession::new(EvalOptions::default()).unwrap();
    assert!(
        denied
            .eval_string("(load-file \"anything.cljs\")")
            .unwrap_err()
            .to_string()
            .contains("explicit file provider")
    );
    let value = session.eval_string("42").unwrap().value;
    assert!(matches!(
        session.data(value).unwrap().kind(),
        DatumKind::Double(42.0)
    ));
    session.release(value).unwrap();
    let mut failed = EvalSession::new(EvalOptions {
        file_provider: Some(Rc::new(|_| Err(EvalError("denied by provider".into())))),
        ..Default::default()
    })
    .unwrap();
    assert!(
        failed
            .load_file("private.cljs")
            .unwrap_err()
            .to_string()
            .contains("denied by provider")
    );
}
