//! The session compiler as an action producer (docs/dev-workflow.md,
//! "Actions" and "Phase 1"): a `DevSession` compiles forms, requires and
//! loaded files into actions without a heap, and the units of those actions
//! run in any heap: a fresh QuickJS evaluator here, and a Node process running
//! the Phase 0 dev build of examples/dev through its driver.
use crate::dev_execution::run_dev;
use crate::temp_dir::TempDir;
use light_cljs::dev_session::{Action, DevSession, script};
use light_cljs::eval_session::FileLoadRequest;
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::source_provider::{FileSourceProvider, LoadRequest, SourceInput};
use light_cljs::{EvalOptions, EvalSession};
use std::{
    cell::{Cell, RefCell},
    fs,
    path::Path,
    process::Command,
    rc::Rc,
    time::Duration,
};

const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/dev");

/// One line per action: `invoke NS`, `load NS ...` (a unit's namespace per
/// unit), `ns NS`, `warnings N` or `error MESSAGE`.
fn shape(actions: &[Action]) -> Vec<String> {
    actions
        .iter()
        .map(|action| match action {
            Action::Invoke(unit) => format!("invoke {}", unit.namespace),
            Action::Load(units) => std::iter::once("load".to_owned())
                .chain(units.iter().map(|unit| unit.namespace.clone()))
                .collect::<Vec<_>>()
                .join(" "),
            Action::Namespace(name) => format!("ns {name}"),
            Action::Warnings(warnings) => std::iter::once("warnings".to_owned())
                .chain(warnings.iter().map(|warning| warning.message.clone()))
                .collect::<Vec<_>>()
                .join(" "),
            Action::Error(error) => format!("error {}", error.message),
        })
        .collect()
}

/// Apply `actions` in `engine` as a runtime would, returning the printed
/// value of the last action when it ran a unit, else "nil". The namespace
/// actions only name the current namespace, which these programs never read.
fn apply(engine: &Evaluator, actions: &[Action]) -> String {
    let mut last = None;
    for action in actions {
        match action {
            Action::Invoke(unit) => last = Some(engine.evaluate_unit(&unit.js).unwrap()),
            Action::Load(units) => {
                for unit in units {
                    last = Some(engine.evaluate_unit(&unit.js).unwrap());
                }
            }
            Action::Namespace(_) => last = None,
            Action::Warnings(_) => {}
            Action::Error(error) => panic!("{error}"),
        }
    }
    last.map_or_else(|| "nil".into(), |value| engine.print(value).unwrap())
}

fn input(text: &str) -> SourceInput {
    SourceInput::new("repl.cljs", text)
}

/// A dev session whose source provider serves `sources` (namespace, text)
/// and counts its loads.
fn dev_session(sources: &[(&str, &str)], loads: Rc<Cell<usize>>) -> DevSession {
    let sources = sources
        .iter()
        .map(|(name, text)| (name.to_string(), text.to_string()))
        .collect::<Vec<_>>();
    DevSession::new(&EvalOptions {
        source_provider: Some(Rc::new(move |request: &LoadRequest| {
            let name = String::from_utf16_lossy(request.namespace.name());
            loads.set(loads.get() + 1);
            Ok(sources
                .iter()
                .find(|(namespace, _)| *namespace == name)
                .map(|(_, text)| SourceInput::new(format!("{name}.cljs"), text)))
        })),
        ..Default::default()
    })
    .unwrap()
}

/// A REPL form is an `Invoke` of its unit in the current namespace, and the
/// session's analysis of a form is the same action.
#[test]
fn a_form_is_an_invoke_of_its_unit() {
    let dev = DevSession::new(&EvalOptions::default()).unwrap();
    let actions = dev.source(&input("(def x 41) (inc x)"));
    assert_eq!(shape(&actions), ["invoke cljs.user", "invoke cljs.user"]);
    let Action::Invoke(unit) = &actions[1] else {
        unreachable!()
    };
    assert!(unit.js.starts_with("(__lightInteractiveRuntime) => "));
    assert!(
        unit.js.ends_with("\n//# sourceURL=cljs.user.js\n"),
        "{}",
        unit.js
    );
    assert_eq!(unit.location.bytes(), 11..18);
    let map = unit.source_map.as_deref().unwrap();
    assert!(
        map.starts_with(r#"{"version":3,"file":"cljs.user.js","sources":["repl.cljs"],"#),
        "{map}"
    );
    assert_eq!(
        apply(&Evaluator::new(Default::default()).unwrap(), &actions),
        "42"
    );

    let mut session = EvalSession::new(EvalOptions::default()).unwrap();
    let form = session.read_forms(&input("(+ 1 2)")).unwrap().remove(0);
    let analyzed = session.analyze(&form).unwrap();
    assert!(matches!(analyzed.action(), Action::Invoke(unit) if unit.namespace == "cljs.user"));
    let value = session.evaluate(&analyzed).unwrap().value;
    assert_eq!(session.print(value).unwrap(), "3");
}

/// A `require` enters each namespace it loads, dependencies first, loads its
/// forms' units and returns to the requiring namespace; a namespace loads
/// once.
#[test]
fn a_require_loads_each_namespace_once_in_dependency_order() {
    let loads = Rc::new(Cell::new(0));
    let dev = dev_session(
        &[
            ("a", "(ns a) (def x 1)"),
            (
                "b",
                "(ns b (:require [a :as a])) (def y (inc a/x)) (def z y)",
            ),
        ],
        loads.clone(),
    );
    let engine = Evaluator::new(Default::default()).unwrap();
    let actions = dev.source(&input("(require 'b) b/z"));
    // The session analyzes in warning mode, which warns of single-segment
    // namespaces, as `--compile --warnings` does.
    assert_eq!(
        shape(&actions),
        [
            "ns cljs.user",
            "warnings b is a single segment namespace [single-segment-namespace]",
            "ns b",
            "warnings a is a single segment namespace [single-segment-namespace]",
            "ns a",
            "load a",
            "ns b",
            "load b b",
            "ns cljs.user",
            "invoke cljs.user"
        ]
    );
    assert_eq!(apply(&engine, &actions), "2");
    assert_eq!(loads.get(), 2);
    let again = dev.source(&input("(require 'b 'a)"));
    assert_eq!(shape(&again), ["ns cljs.user"]);
    assert_eq!(apply(&engine, &again), "nil");
    assert_eq!(loads.get(), 2);
    assert_eq!(dev.namespace(), "cljs.user");
    // `in-ns` changes the namespace, and the next form is compiled in it.
    let entered = dev.source(&input("(in-ns 'b) z"));
    assert_eq!(shape(&entered), ["ns b", "invoke b"]);
    assert_eq!(apply(&engine, &entered), "2");
}

/// `require :reload` compiles the source the provider returns at that time;
/// a plain `require` of a loaded namespace loads nothing.
#[test]
fn require_reload_compiles_the_source_the_provider_returns_now() {
    let text = Rc::new(RefCell::new("(ns lib) (defn greet [] \"one\")".to_owned()));
    let source = text.clone();
    let dev = DevSession::new(&EvalOptions {
        source_provider: Some(Rc::new(move |request: &LoadRequest| {
            assert_eq!(String::from_utf16_lossy(request.namespace.name()), "lib");
            Ok(Some(SourceInput::new("lib.cljs", source.borrow().clone())))
        })),
        ..Default::default()
    })
    .unwrap();
    let engine = Evaluator::new(Default::default()).unwrap();
    let first = dev.source(&input("(require 'lib) (lib/greet)"));
    assert_eq!(
        shape(&first),
        [
            "ns cljs.user",
            "warnings lib is a single segment namespace [single-segment-namespace]",
            "ns lib",
            "load lib",
            "ns cljs.user",
            "invoke cljs.user"
        ]
    );
    assert_eq!(apply(&engine, &first), "\"one\"");
    *text.borrow_mut() = "(ns lib) (defn greet [] \"two\")".into();
    let loaded = dev.source(&input("(require 'lib) (lib/greet)"));
    assert_eq!(shape(&loaded), ["ns cljs.user", "invoke cljs.user"]);
    assert_eq!(apply(&engine, &loaded), "\"one\"");
    let reloaded = dev.source(&input("(require 'lib :reload) (lib/greet)"));
    assert_eq!(shape(&reloaded), shape(&first));
    let [_, _, _, Action::Load(old), ..] = first.as_slice() else {
        unreachable!()
    };
    let [_, _, _, Action::Load(new), ..] = reloaded.as_slice() else {
        unreachable!()
    };
    assert_ne!(old[0].js, new[0].js);
    assert_eq!(apply(&engine, &reloaded), "\"two\"");
}

/// A loaded file is one `Load` of its forms' units, with the namespaces it
/// requires before them; the session stays in the file's namespace, and the
/// file's value is its last form's.
#[test]
fn load_file_loads_its_forms_and_stays_in_its_namespace() {
    let dev = dev_session(&[("a", "(ns a) (def x 1)")], Rc::default());
    let actions = dev.load_file(&SourceInput::new(
        "tool.cljs",
        "(ns tool (:require [a :as a])) (def z (* 2 a/x)) z",
    ));
    assert_eq!(
        shape(&actions),
        [
            "warnings tool is a single segment namespace [single-segment-namespace]",
            "ns tool",
            "warnings a is a single segment namespace [single-segment-namespace]",
            "ns a",
            "load a",
            "ns tool",
            "load tool tool"
        ]
    );
    assert_eq!(dev.namespace(), "tool");
    let engine = Evaluator::new(Default::default()).unwrap();
    assert_eq!(apply(&engine, &actions), "2");
    // A file whose last form is a directive has the value nil.
    let directive = dev.load_file(&SourceInput::new("other.cljs", "(def w 3) (in-ns 'a)"));
    assert_eq!(shape(&directive), ["load tool", "ns a"]);
    assert_eq!(apply(&engine, &directive), "nil");
}

/// A form that does not compile ends the actions with its located error,
/// after the actions of the forms before it; nothing after it is compiled,
/// and the session goes on.
#[test]
fn a_failing_form_after_a_successful_one_ends_with_its_error() {
    let dev = dev_session(&[], Rc::default());
    // The strict analysis an `EvalSession` has by default.
    dev.set_warning_mode(false);
    let engine = Evaluator::new(Default::default()).unwrap();
    let actions = dev.source(&input("(def ok 1)\n(undeclared ok)\n(def never 2)"));
    // Strict analysis warns of the undeclared Var, as `--compile` does,
    // before it fails.
    assert_eq!(
        shape(&actions),
        [
            "invoke cljs.user",
            "warnings Use of undeclared Var cljs.user/undeclared [undeclared-var]",
            "error unresolved symbol: undeclared"
        ]
    );
    let Some(Action::Error(error)) = actions.last() else {
        unreachable!()
    };
    let location = error.location.as_ref().unwrap();
    assert_eq!(
        location.source().line_column(location.bytes().start),
        Some((2, 2))
    );
    apply(&engine, &actions[..1]);
    assert_eq!(apply(&engine, &dev.source(&input("ok"))), "1");
    assert!(matches!(
        dev.source(&input("never")).last(),
        Some(Action::Error(error)) if error.message == "unresolved symbol: never"
    ));
    // A require of a missing namespace fails at the require.
    assert_eq!(
        shape(&dev.source(&input("(require 'absent)"))),
        ["ns cljs.user", "error missing namespace absent"]
    );
    // A package require without a binding warns before its namespace action.
    assert_eq!(
        shape(&dev.source(&input("(require '[\"react\"])"))),
        [
            "warnings requiring JavaScript package \"react\" without :as, :refer or :default \
             has no effect; the host loads packages [package-require-no-effect]",
            "ns cljs.user"
        ]
    );
}

/// The actions of a `require` compiled without a heap from an edited
/// examples/dev apply to the running Phase 0 dev build of the original with
/// the hook protocol: the next render runs the new code, and the `defonce`
/// state keeps its clicks. Each unit runs in its namespace, and the REPL form
/// after the `require` in the requiring one.
#[test]
fn units_without_a_heap_reload_a_running_dev_build_in_node() {
    let dir = TempDir::new("dev-session");
    let out = dir.join("dev");
    let status = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--compile", "--entry", "app.main", "--source-root", EXAMPLE])
        .arg("--reloadable")
        .arg("--out-dir")
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success());
    let root = dir.join("src");
    fs::create_dir_all(root.join("app")).unwrap();
    for name in ["state", "macros", "view", "main"] {
        let file = format!("app/{name}.cljs");
        let text = fs::read_to_string(Path::new(EXAMPLE).join(&file)).unwrap();
        let edited = text.replace(
            "(js/console.log (describe card))",
            "(js/console.log \"render:\" (describe card))",
        );
        assert_eq!(edited != text, name == "view");
        fs::write(root.join(&file), edited).unwrap();
    }
    fs::write(
        root.join("app/probe.cljs"),
        "(ns app.probe)\n(js/console.log \"probe\" (str *ns*))\n",
    )
    .unwrap();
    let dev = DevSession::new(&EvalOptions {
        source_provider: Some(Rc::new(FileSourceProvider::new(vec![root]))),
        ..Default::default()
    })
    .unwrap();
    let actions = dev.source(&input(
        "(require 'app.view 'app.probe) (js/console.log \"after\" (str *ns*))",
    ));
    assert_eq!(
        shape(&actions),
        [
            "ns cljs.user",
            "ns app.view",
            "ns app.macros",
            "load app.macros app.macros",
            "ns app.view",
            "ns app.state",
            "load app.state app.state",
            "ns app.view",
            "load app.view app.view app.view app.view",
            "ns cljs.user",
            "ns app.probe",
            "load app.probe",
            "ns cljs.user",
            "invoke cljs.user"
        ]
    );
    let file = dir.join("actions.js");
    fs::write(&file, script(&actions)).unwrap();
    assert_eq!(
        run_dev(&out, true, &[file]).unwrap(),
        r#"{"id":"program","outcome":{"status":"returned","value":{"$value":"undefined"}},"trace":[],"stdout":"start\nclicks: [1]\nreloading\nprobe app.probe\nafter cljs.user\nrender: clicks: [2]\n"}"#
    );
    // An error is the script's exception: nothing after it was compiled.
    dev.set_warning_mode(false);
    let failing = dev.source(&input("(js/console.log \"before\") (undeclared)"));
    assert!(
        script(&failing)
            .ends_with("throw new Error(\"repl.cljs:1:28: unresolved symbol: undeclared\");\n"),
        "{}",
        script(&failing)
    );
}

/// A standalone session begins a new deadline and poll budget at each call,
/// so its macro heap outlives an idle period longer than the deadline.
#[test]
fn a_standalone_session_survives_idle_time_between_compilations() {
    let dev = DevSession::new(&EvalOptions {
        engine: EvaluatorOptions {
            timeout: Duration::from_millis(3000),
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap();
    let engine = Evaluator::new(Default::default()).unwrap();
    let defined = dev.source(&input(
        "(defmacro twice [x] `(* 2 ~x)) (defn half [x] (/ x 2)) (twice 21)",
    ));
    assert_eq!(apply(&engine, &defined), "42");
    std::thread::sleep(Duration::from_millis(3200));
    assert_eq!(
        apply(&engine, &dev.source(&input("(half (twice 21))"))),
        "21"
    );
}

/// While the session's heap interprets a load, a unit that throws stops it:
/// the definitions after it are never compiled, the requiring namespace is
/// current again, the exception keeps its data, and the namespace loads
/// again at the next `require`; through the runtime's `load-file` too.
#[test]
fn a_unit_that_throws_while_a_namespace_loads_stops_the_load_until_a_retry() {
    let sources = Rc::new(RefCell::new(
        "(ns lib) (def before 1) (throw (ex-info \"boom\" {:at 1})) (def after 2)".to_owned(),
    ));
    let source = sources.clone();
    let mut session = EvalSession::new(EvalOptions {
        source_provider: Some(Rc::new(move |request: &LoadRequest| {
            let name = String::from_utf16_lossy(request.namespace.name());
            Ok(Some(SourceInput::new(
                format!("{name}.cljs"),
                source.borrow().replace("lib", &name),
            )))
        })),
        file_provider: Some(Rc::new(|_: &FileLoadRequest| {
            Ok(Some(SourceInput::new(
                "load.cljs",
                "(require 'other) :loaded",
            )))
        })),
        ..Default::default()
    })
    .unwrap();
    fn printed(session: &mut EvalSession, source: &str) -> String {
        let value = session.eval_string(source).unwrap().value;
        session.print(value).unwrap()
    }
    let s = &mut session;
    assert_eq!(printed(s, "(ns app) :entered"), ":entered");
    let error = s.eval_string("(require 'lib)").unwrap_err();
    assert!(
        error.to_string().starts_with("lib.cljs:1:25: Error: boom"),
        "{error}"
    );
    let data = s.value(&error.data.unwrap()).unwrap();
    assert_eq!(s.print(data).unwrap(), "{:at 1}");
    assert_eq!(s.namespace(), "app");
    let bindings = s.bindings();
    assert!(bindings.contains(&"lib/before".to_owned()), "{bindings:?}");
    assert!(!bindings.contains(&"lib/after".to_owned()), "{bindings:?}");
    assert_eq!(printed(s, "(str *ns*)"), "\"app\"");
    // The runtime's `load-file` loads through the same compiler.
    assert_eq!(
        printed(
            s,
            "(try (load-file \"load.cljs\") (catch :default e [(ex-message e) (ex-data e)]))"
        ),
        "[\"boom\" {:at 1}]"
    );
    assert!(!s.bindings().contains(&"other/after".to_owned()));
    assert_eq!(printed(s, "(str *ns*)"), "\"app\"");
    *sources.borrow_mut() = "(ns lib) (def before 1) (def after 2)".into();
    assert_eq!(
        printed(s, "(require 'lib) [lib/before lib/after (str *ns*)]"),
        "[1 2 \"app\"]"
    );
    assert_eq!(printed(s, "(load-file \"load.cljs\")"), ":loaded");
    assert_eq!(printed(s, "[other/after (str *ns*)]"), "[2 \"app\"]");
}

/// A namespace whose `ns` form uses every clause `--compile` accepts: name
/// metadata, a docstring, an attribute map, `:refer-clojure` with `:exclude`
/// and `:rename`, `:require` with `:as`, `:refer`, `:rename`, `:as-alias`
/// and package strings (with `:as`, `:refer`, `:rename`, `:default`, and one
/// that binds nothing), `:use` with `:only` and `:rename`, `:import`,
/// `:require-global` and `:refer-global`.
const EVERY_CLAUSE: &str = r#"(ns ^{:added "1"} app.every
  "Every namespace clause."
  {:author "light-cljs"}
  (:refer-clojure :exclude [update] :rename {inc plus-one})
  (:require [app.lib :as lib :refer [twice] :rename {twice double}]
            [app.only :as-alias only]
            "pkg"
            ["pkg" :as pkg :refer [pkgFn] :rename {pkgFn pkg-fn} :default pkg-default])
  (:use [app.lib :only [half] :rename {half halve}])
  (:import [Host Widget])
  (:require-global [Math :as m :refer [max] :rename {max biggest}])
  (:refer-global :only [Date] :rename {Date D}))
(defn update [x] (str "own " x))
(set! js/Host (js-obj "Widget" (js* "(function Widget(x) { this.x = x; })")))
(pr-str [(plus-one 1) (update 1) (lib/twice 2) (double 3) (halve 8) ::only/key
         (pkg-fn 1) (pkg-default) (.-tag pkg) (biggest 1 5) (m/min 1 5)
         (.-x (Widget. 7)) (identical? D js/Date)])
"#;
const EVERY_CLAUSE_LIB: &str = "(ns app.lib) (defn twice [x] (* 2 x)) (defn half [x] (/ x 2))";
const EVERY_CLAUSE_VALUE: &str = r#"[2 "own 1" 4 6 4 :app.only/key 11 "dflt" "pkg" 5 1 7 true]"#;
const PACKAGES: &str = "globalThis.lightCljsPackages = {pkg: {tag: 'pkg', pkgFn: x => x + 10, \
                        default: () => 'dflt'}}";

/// The units of an `ns` with every clause, compiled by a `DevSession`, print
/// what `--compile` output of the same namespace prints, and so does an
/// `EvalSession`; the only warning is the package that binds nothing.
#[test]
fn an_ns_form_with_every_clause_behaves_as_in_compile() {
    let dir = TempDir::new("dev-every");
    fs::create_dir_all(dir.join("app")).unwrap();
    fs::write(dir.join("app/every.cljs"), EVERY_CLAUSE).unwrap();
    fs::write(dir.join("app/lib.cljs"), EVERY_CLAUSE_LIB).unwrap();
    let compiled = light_cljs::compile_namespace("app.every", &[dir.to_path_buf()]).unwrap();
    let engine = Evaluator::new(Default::default()).unwrap();
    engine.evaluate_js(PACKAGES).unwrap();
    let value = engine.evaluate_js(&compiled).unwrap();
    assert_eq!(engine.string(value).unwrap(), EVERY_CLAUSE_VALUE);

    let provider = Rc::new(FileSourceProvider::new(vec![dir.to_path_buf()]));
    let dev = DevSession::new(&EvalOptions {
        source_provider: Some(provider.clone()),
        ..Default::default()
    })
    .unwrap();
    let actions = dev.load_file(&SourceInput::new("app/every.cljs", EVERY_CLAUSE));
    assert_eq!(
        shape(&actions),
        [
            "warnings requiring JavaScript package \"pkg\" without :as, :refer or :default has \
             no effect; the host loads packages [package-require-no-effect]",
            "ns app.every",
            "ns app.lib",
            "load app.lib app.lib",
            "ns app.every",
            "load app.every app.every app.every"
        ]
    );
    let engine = Evaluator::new(Default::default()).unwrap();
    engine.evaluate_js(PACKAGES).unwrap();
    assert_eq!(apply(&engine, &actions), format!("{EVERY_CLAUSE_VALUE:?}"));

    let mut session = EvalSession::new(EvalOptions {
        source_provider: Some(provider),
        ..Default::default()
    })
    .unwrap();
    session.eval_string(&format!("(js* {PACKAGES:?})")).unwrap();
    let value = session
        .eval_source(&SourceInput::new("app/every.cljs", EVERY_CLAUSE))
        .unwrap()
        .value;
    assert_eq!(session.string(value).unwrap(), EVERY_CLAUSE_VALUE);
}

/// An `ns` form declares all of its namespace's requires, as upstream's
/// does, so evaluating it again (a reloaded file whose `ns` changed) may
/// point an alias at another namespace; its clauses are checked as
/// `--compile` checks them, reader conditionals of a `.cljc` source
/// selected, and a name need not be ASCII, as `in-ns` takes it.
#[test]
fn an_ns_form_replaces_its_namespace_requires() {
    let dev = dev_session(
        &[
            ("app.one", "(ns app.one) (def value 1)"),
            ("app.two", "(ns app.two) (def value 2)"),
        ],
        Rc::default(),
    );
    let engine = Evaluator::new(Default::default()).unwrap();
    let first = dev.source(&input("(ns app.user (:require [app.one :as v])) v/value"));
    assert_eq!(apply(&engine, &first), "1");
    let second = dev.source(&input("(ns app.user (:require [app.two :as v])) v/value"));
    assert_eq!(apply(&engine, &second), "2");
    // An alias the new `ns` form drops is gone: `v` names no namespace.
    let third = dev.source(&input("(ns app.user) v/value"));
    assert!(
        shape(&third)[1].starts_with("warnings No such namespace: v,"),
        "{:?}",
        shape(&third)
    );
    let cljc = dev.source(&SourceInput::new(
        "app/cond.cljc",
        "(ns app.cond (:require #?(:cljs [app.two :as v] :clj [app.one :as v]))) v/value",
    ));
    assert_eq!(apply(&engine, &cljc), "2");
    for (source, message, at) in [
        (
            "(ns app.bad (:unless []))",
            "unsupported namespace clause",
            (1, 13),
        ),
        (
            "(ns app.bad (:require app.one) (:require app.two))",
            "only one :require clause is allowed per namespace",
            (1, 32),
        ),
        (
            "(ns app.bad (:require-macros [app.one]))",
            ":require-macros is retired; import macros and functions with ordinary :require \
             from .cljs/.cljc sources",
            (1, 13),
        ),
        (
            "(ns \"app\")",
            "ns requires a namespace symbol; js is reserved",
            (1, 5),
        ),
    ] {
        let actions = dev.source(&input(source));
        let Some(Action::Error(error)) = actions.last() else {
            panic!("{source}: {:?}", shape(&actions))
        };
        assert_eq!(error.message, message, "{source}");
        let location = error.location.as_ref().unwrap();
        assert_eq!(
            location.source().line_column(location.bytes().start),
            Some(at),
            "{source}"
        );
    }
    let unicode = dev.source(&input("(ns café) ::x"));
    assert_eq!(apply(&engine, &unicode), ":café/x");
}

/// The reload spellings: `[lib :reload true]`, `[lib :reload-all true]`, the
/// flags `:reload` and `:reload-all` anywhere among the arguments (first
/// too). Each compiles the text the source provider returns at that time,
/// which changes between the loads; `:reload-all` loads again, once, every
/// namespace the library requires, directly or not.
#[test]
fn reload_spellings_compile_the_text_the_provider_returns_now() {
    let texts = Rc::new(RefCell::new(vec![
        ("app.base", "(ns app.base) (def value 1)".to_owned()),
        (
            "app.left",
            "(ns app.left (:require [app.base :as b])) (def value b/value)".to_owned(),
        ),
        (
            "app.right",
            "(ns app.right (:require [app.base :as b])) (def value b/value)".to_owned(),
        ),
        (
            "app.lib",
            "(ns app.lib (:require app.left app.right)) \
             (def value [:one app.left/value app.right/value])"
                .to_owned(),
        ),
    ]));
    let loads = Rc::new(RefCell::new(Vec::new()));
    let (sources, loaded) = (texts.clone(), loads.clone());
    let dev = DevSession::new(&EvalOptions {
        source_provider: Some(Rc::new(move |request: &LoadRequest| {
            let name = String::from_utf16_lossy(request.namespace.name());
            loaded.borrow_mut().push(name.clone());
            Ok(sources
                .borrow()
                .iter()
                .find(|(namespace, _)| *namespace == name)
                .map(|(_, text)| SourceInput::new(format!("{name}.cljs"), text.clone())))
        })),
        ..Default::default()
    })
    .unwrap();
    let edit = |name: &str, from: &str, to: &str| {
        let mut texts = texts.borrow_mut();
        let text = &mut texts.iter_mut().find(|(n, _)| *n == name).unwrap().1;
        *text = text.replace(from, to);
    };
    let engine = Evaluator::new(Default::default()).unwrap();
    let run = |source: &str| {
        loads.borrow_mut().clear();
        let actions = dev.source(&input(source));
        (apply(&engine, &actions), loads.borrow().join(" "))
    };
    assert_eq!(
        run("(require 'app.lib) app.lib/value"),
        (
            "[:one 1 1]".into(),
            "app.lib app.left app.base app.right".into()
        )
    );
    edit("app.lib", ":one", ":two");
    edit("app.base", "1", "2");
    assert_eq!(
        run("(require '[app.lib :reload true]) app.lib/value"),
        ("[:two 1 1]".into(), "app.lib".into())
    );
    assert_eq!(
        run("(require '[app.lib :reload-all true]) app.lib/value"),
        (
            "[:two 2 2]".into(),
            "app.lib app.left app.base app.right".into()
        )
    );
    edit("app.lib", ":two", ":three");
    assert_eq!(
        run("(require :reload 'app.lib) app.lib/value"),
        ("[:three 2 2]".into(), "app.lib".into())
    );
    edit("app.base", "2", "3");
    assert_eq!(
        run("(require 'app.lib :reload-all) app.lib/value"),
        (
            "[:three 3 3]".into(),
            "app.lib app.left app.base app.right".into()
        )
    );
    // A false option and a plain require of a loaded namespace load nothing.
    assert_eq!(
        run("(require '[app.lib :reload false] 'app.left) app.lib/value"),
        ("[:three 3 3]".into(), String::new())
    );
    let actions = dev.source(&input("(require '[app.lib :reload :yes])"));
    assert!(
        matches!(actions.last(), Some(Action::Error(error))
            if error.message == ":reload requires true or false"),
        "{:?}",
        shape(&actions)
    );
}

/// Warning mode, the `DevSession` default: a form that uses a Var defined
/// later, as a file saved before the one defining it may, compiles with a
/// warning and runs as upstream's would, where strict analysis rejects it
/// and the reload fails. An `EvalSession` turns it on with
/// `EvalOptions::warning_mode` and receives the warnings, located, through
/// `EvalOptions::warnings`, before the form runs.
#[test]
fn warning_mode_compiles_uses_of_vars_defined_later() {
    let texts = Rc::new(RefCell::new(vec![
        ("app.state", "(ns app.state) (def clicks 1)".to_owned()),
        (
            "app.view",
            "(ns app.view (:require [app.state :as s])) (defn render [] s/clicks)".to_owned(),
        ),
    ]));
    let sources = texts.clone();
    let options = EvalOptions {
        source_provider: Some(Rc::new(move |request: &LoadRequest| {
            let name = String::from_utf16_lossy(request.namespace.name());
            Ok(sources
                .borrow()
                .iter()
                .find(|(namespace, _)| *namespace == name)
                .map(|(_, text)| SourceInput::new(format!("{name}.cljs"), text.clone())))
        })),
        ..Default::default()
    };
    let dev = DevSession::new(&options).unwrap();
    assert!(dev.warning_mode());
    let engine = Evaluator::new(Default::default()).unwrap();
    assert_eq!(
        apply(
            &engine,
            &dev.source(&input("(require 'app.view) (app.view/render)"))
        ),
        "1"
    );
    // The view is saved first, using a function the state does not define yet.
    texts.borrow_mut()[1].1 =
        "(ns app.view (:require [app.state :as s])) (defn render [] (s/total))".into();
    let reload = dev.source(&input("(require 'app.view :reload)"));
    assert_eq!(
        shape(&reload),
        [
            "ns cljs.user",
            "ns app.view",
            "warnings Use of undeclared Var app.state/total [undeclared-var]",
            "load app.view",
            "ns cljs.user"
        ]
    );
    let Action::Warnings(warnings) = &reload[2] else {
        unreachable!()
    };
    let location = warnings[0].location.as_ref().unwrap();
    assert_eq!(location.source().name(), "app.view.cljs");
    assert_eq!(
        location.source().line_column(location.bytes().start),
        Some((1, 61))
    );
    apply(&engine, &reload);
    texts.borrow_mut()[0].1 = "(ns app.state) (def clicks 2) (defn total [] (* 10 clicks))".into();
    assert_eq!(
        apply(
            &engine,
            &dev.source(&input("(require 'app.state :reload) (app.view/render)"))
        ),
        "20"
    );
    // Strict analysis rejects the use instead, and loads nothing after it.
    dev.set_warning_mode(false);
    texts.borrow_mut()[1].1 =
        "(ns app.view (:require [app.state :as s])) (defn render [] (s/missing))".into();
    let strict = dev.source(&input("(require 'app.view :reload)"));
    assert!(
        matches!(strict.last(), Some(Action::Error(error))
            if error.message.contains("unresolved symbol: s/missing")),
        "{:?}",
        shape(&strict)
    );

    let warnings = Rc::new(RefCell::new(Vec::new()));
    let received = warnings.clone();
    let mut session = EvalSession::new(EvalOptions {
        warning_mode: true,
        warnings: Some(Rc::new(move |warning| {
            received.borrow_mut().push(warning.to_string());
            Ok(())
        })),
        ..options.clone()
    })
    .unwrap();
    let value = session
        .eval_source(&input("(defn f [] (g 1))\n(defn g [x] (inc x))\n(f)"))
        .unwrap()
        .value;
    assert_eq!(session.print(value).unwrap(), "2");
    assert_eq!(
        *warnings.borrow(),
        ["repl.cljs:1:13: Use of undeclared Var cljs.user/g [undeclared-var]"]
    );
    let mut strict = EvalSession::new(options).unwrap();
    assert!(strict.eval_string("(defn f [] (g 1))").is_err());
}

/// Each unit carries a source map of its text to the form's source, named
/// as the dev build's file of its namespace, which the unit's `sourceURL`
/// names too, so a runtime that evaluates it can map what it runs.
#[test]
fn units_carry_source_maps_of_their_forms() {
    let source = "(ns app.maps)\n\n(defn check [x]\n  (js/Math.max x 4242))\n";
    let dev = DevSession::new(&EvalOptions::default()).unwrap();
    let actions = dev.load_file(&SourceInput::new("src/app/maps.cljs", source));
    let Some(Action::Load(units)) = actions.last() else {
        panic!("{:?}", shape(&actions))
    };
    let unit = &units[0];
    assert!(
        unit.js.ends_with("\n//# sourceURL=app.maps.js\n"),
        "{}",
        unit.js
    );
    let map = unit.source_map.as_deref().unwrap();
    assert!(
        map.starts_with(r#"{"version":3,"file":"app.maps.js","sources":["src/app/maps.cljs"],"#),
        "{map}"
    );
    // Node's own reader of source maps finds the literal at its source line
    // and column.
    let dir = TempDir::new("dev-unit-map");
    fs::write(dir.join("unit.js"), &unit.js).unwrap();
    fs::write(dir.join("unit.js.map"), map).unwrap();
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg("-e")
        .arg(
            "const fs = require('node:fs'); const {SourceMap} = require('node:module');
             const [js, map] = process.argv.slice(1).map(f => fs.readFileSync(f, 'utf8'));
             const before = js.slice(0, js.indexOf('4242')).split('\\n');
             const entry = new SourceMap(JSON.parse(map))
               .findEntry(before.length - 1, before.at(-1).length);
             console.log(entry.originalSource, entry.originalLine + 1, entry.originalColumn + 1);",
        )
        .arg(dir.join("unit.js"))
        .arg(dir.join("unit.js.map"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "src/app/maps.cljs 4 18\n"
    );
}
