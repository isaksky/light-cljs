//! The session compiler as an action producer (docs/dev-workflow.md,
//! "Actions" and "Phase 1"): a `DevSession` compiles forms, requires and
//! loaded files into actions without a heap, and the units of those actions
//! run in any heap: a fresh QuickJS evaluator here, and a Node process running
//! the Phase 0 dev build of examples/dev through its driver.
use crate::dev_execution::run_dev;
use crate::temp_dir::TempDir;
use light_cljs::compiler_data::{Form, Origin, Source};
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

/// A loaded file generates names in a scope of its namespace and text
/// (light-cljs-332): one text without an `ns` form, loaded in two
/// namespaces, quotes two distinct gensyms, and loading it again in one of
/// them quotes the same one again.
#[test]
fn namespace_less_load_files_get_distinct_gensyms() {
    let dev = dev_session(
        &[(
            "lib",
            "(ns lib) (defmacro uniq [] (list 'quote (gensym \"tag\")))",
        )],
        Rc::default(),
    );
    let engine = Evaluator::new(Default::default()).unwrap();
    let file = SourceInput::new("tag.cljs", "(def tag (lib/uniq)) tag");
    let mut tags = Vec::new();
    for namespace in ["a", "b", "a"] {
        let entered = dev.source(&input(&format!("(ns {namespace} (:require [lib]))")));
        apply(&engine, &entered);
        tags.push(apply(&engine, &dev.load_file(&file)));
    }
    assert_ne!(tags[0], tags[1], "{tags:?}");
    assert_eq!(tags[0], tags[2], "{tags:?}");
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

const GOOG: &str = r#"(ns app.text
  (:require [goog.string :as gstr :refer [trim] :rename {trim strim}]
            [goog.string.format]))
(ns* (:require '[goog.array :as garray]))
(require '[goog.object :as gobj :refer [containsKey]])
(pr-str [(gstr/format "%s!" 1) (strim " a ") (goog.string/trim " b ")
         (garray/last #js [1 2]) (containsKey #js {:k 1} "k") (gobj/get #js {:k 4} "k")])
"#;
const GOOG_VALUE: &str = r#"["1!" "a" "b" 2 true 4]"#;
/// A stand-in for the Closure Library functions [`GOOG`] calls.
const GOOG_HOST: &str = "globalThis.goog = {string: {format: (f, x) => f.replace('%s', x), \
                         trim: s => s.trim()}, array: {last: a => a[a.length - 1]}, \
                         object: {containsKey: (o, k) => k in o, get: (o, k) => o[k]}}";

/// A `goog.*` library that the source provider does not provide is a
/// JavaScript host namespace in an `ns`, an `ns*` and a `require`, as in
/// `--compile`: the session asks the provider for it once and loads
/// nothing, and the units read the host's functions through the alias, the
/// renamed refer, a refer and the namespace's own name, as the compiled
/// namespace does.
#[test]
fn an_unprovided_goog_library_is_a_host_namespace() {
    let dir = TempDir::new("dev-goog");
    fs::create_dir_all(dir.join("app")).unwrap();
    fs::write(dir.join("app/text.cljs"), GOOG).unwrap();
    let compiled = light_cljs::compile_namespace("app.text", &[dir.to_path_buf()]).unwrap();
    let engine = Evaluator::new(Default::default()).unwrap();
    engine.evaluate_js(GOOG_HOST).unwrap();
    let value = engine.evaluate_js(&compiled).unwrap();
    assert_eq!(engine.string(value).unwrap(), GOOG_VALUE);

    let files = FileSourceProvider::new(vec![dir.to_path_buf()]);
    let asked = Rc::new(RefCell::new(Vec::new()));
    let provider = {
        let asked = asked.clone();
        Rc::new(move |request: &LoadRequest| {
            asked
                .borrow_mut()
                .push(String::from_utf16_lossy(request.namespace.name()));
            light_cljs::source_provider::SourceProvider::load(&files, request)
        })
    };
    let dev = DevSession::new(&EvalOptions {
        source_provider: Some(provider.clone()),
        ..Default::default()
    })
    .unwrap();
    let actions = dev.load_file(&SourceInput::new("app/text.cljs", GOOG));
    assert_eq!(
        shape(&actions),
        ["ns app.text", "ns app.text", "ns app.text", "load app.text"]
    );
    assert_eq!(
        *asked.borrow(),
        [
            "goog.string",
            "goog.string.format",
            "goog.array",
            "goog.object"
        ]
    );
    assert!(dev.requires("app.text").is_empty());
    let engine = Evaluator::new(Default::default()).unwrap();
    engine.evaluate_js(GOOG_HOST).unwrap();
    assert_eq!(apply(&engine, &actions), format!("{GOOG_VALUE:?}"));

    let mut session = EvalSession::new(EvalOptions {
        source_provider: Some(provider),
        ..Default::default()
    })
    .unwrap();
    session
        .eval_string(&format!("(js* {GOOG_HOST:?})"))
        .unwrap();
    let value = session
        .eval_source(&SourceInput::new("app/text.cljs", GOOG))
        .unwrap()
        .value;
    assert_eq!(session.string(value).unwrap(), GOOG_VALUE);
}

/// The session asks the provider whether it provides a `goog.*` library
/// with the request a load makes, which names the requesting file and the
/// reload: a provider that serves the source only to `app/client.cljs`, or
/// only to a reload, has it loaded in an `ns` and in a `require`, and its
/// Vars are used, not host paths.
#[test]
fn goog_source_resolution_preserves_request_metadata() {
    let dev = DevSession::new(&EvalOptions {
        source_provider: Some(Rc::new(|request: &LoadRequest| {
            let name = String::from_utf16_lossy(request.namespace.name());
            let served = match name.as_str() {
                "goog.custom" => request.requesting_file.as_deref() == Some("app/client.cljs"),
                "goog.again" => request.reload != light_cljs::source_provider::Reload::None,
                _ => false,
            };
            Ok(served.then(|| {
                SourceInput::new(format!("{name}.cljs"), format!("(ns {name}) (def value 7)"))
            }))
        })),
        ..Default::default()
    })
    .unwrap();
    let client = "(ns app.client (:require [goog.custom :as g]))\n\
                  (require '[goog.again :as a] :reload)\n\
                  (+ g/value a/value)";
    let actions = dev.load_file(&SourceInput::new("app/client.cljs", client));
    let engine = Evaluator::new(Default::default()).unwrap();
    assert_eq!(apply(&engine, &actions), "14");
    assert_eq!(dev.requires("app.client"), ["goog.custom", "goog.again"]);
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

/// The name metadata, docstring and attribute map of an `ns` form reach the
/// macro environment, as under `--compile`
/// (`namespace_metadata_and_docstrings_reach_the_macro_environment_and_refresh_on_reuse`
/// in tests/compiler/macros.rs), and an `ns` form without them forgets them;
/// metadata on the `ns` form itself is not a clause.
#[test]
fn ns_metadata_reaches_the_macro_environment_and_refreshes() {
    let dev = dev_session(
        &[(
            "app.macros",
            "(ns app.macros) (defmacro read-meta [] \
             (str (:foo (meta (:name (:ns &env)))) \"/\" (:doc (:ns &env))))",
        )],
        Rc::default(),
    );
    let engine = Evaluator::new(Default::default()).unwrap();
    let read = |source: &str| apply(&engine, &dev.source(&input(source)));
    assert_eq!(
        read(
            "(ns ^{:foo old} app.main \"doc\" {:foo new} \
             (:require [app.macros :refer [read-meta]])) (read-meta)"
        ),
        "\"new/doc\""
    );
    assert_eq!(
        read("(ns app.main (:require [app.macros :refer [read-meta]])) (read-meta)"),
        "\"/\""
    );
    let tagged = dev.source(&input("^:tagged (ns app.tagged)"));
    assert_eq!(shape(&tagged), ["ns app.tagged"]);
    assert_eq!(
        read("^:tagged (ns* (:require '[app.macros :as m])) (m/read-meta)"),
        "\"/\""
    );
}

/// A directive built from data, not read from the source text its span
/// covers, is read from its data: the namespace it declares is entered, an
/// error in it is located at the whole form, and a span over other text or
/// over nothing changes neither.
#[test]
fn supplied_ns_forms_use_their_data() {
    let mut session = EvalSession::new(EvalOptions::default()).unwrap();
    let data = session
        .read_forms(&input("(ns app.actual (:require [clojure.string :as s]))"))
        .unwrap()
        .remove(0)
        .data();
    let elsewhere = Source::new("other.cljs", "(ns app.other)");
    let nowhere = Source::new("<supplied>", "");
    for (span, origin) in [
        (elsewhere.span(0..14).unwrap(), Origin::SuppliedData),
        (elsewhere.span(0..14).unwrap(), Origin::Source),
        (nowhere.span(0..0).unwrap(), Origin::SuppliedData),
    ] {
        let form = Form::from_data(data.clone(), span, origin);
        session.eval_form(&form).unwrap();
        assert_eq!(session.namespace(), "app.actual");
        let value = session.eval_string("(s/upper-case \"x\")").unwrap().value;
        assert_eq!(session.print(value).unwrap(), "\"X\"");
        session.eval_string("(in-ns 'cljs.user)").unwrap();
    }
    let bad = session
        .read_forms(&input("(ns app.bad (:unless []))"))
        .unwrap()
        .remove(0)
        .data();
    let form = Form::from_data(bad, elsewhere.span(0..14).unwrap(), Origin::SuppliedData);
    let error = session.eval_form(&form).unwrap_err();
    assert_eq!(
        error.to_string(),
        "other.cljs:1:1: unsupported namespace clause"
    );
}

/// Warning mode applies to `ns*` as to `ns`: `[lib :as js]` is the warning
/// `--compile --warnings` gives, where strict analysis rejects it.
#[test]
fn nsstar_honors_warning_mode_for_a_reserved_alias() {
    let dev = dev_session(&[], Rc::default());
    let actions = dev.source(&input(
        "(ns app.user) (ns* (:require '[clojure.string :as js])) true",
    ));
    assert_eq!(
        shape(&actions)[..3],
        [
            "ns app.user",
            "warnings In [clojure.string :as js], the alias name js is reserved for JavaScript \
         interop [js-used-as-alias]",
            "ns app.user",
        ]
    );
    assert!(
        matches!(actions.last(), Some(Action::Invoke(_))),
        "{:?}",
        shape(&actions)
    );
    dev.set_warning_mode(false);
    let strict = dev.source(&input("(ns* (:require '[clojure.string :as js]))"));
    assert!(
        matches!(strict.last(), Some(Action::Error(error))
            if error.message.contains("js is reserved")),
        "{:?}",
        shape(&strict)
    );
}

/// The runtime's `load-file` hands the warnings of the file's forms to
/// `EvalOptions::warnings`, located, before each form runs; a receiver that
/// fails fails the load there, so the forms after the warning never run.
#[test]
fn runtime_load_file_delivers_warnings_and_stops_on_receiver_error() {
    let file = "(swap! effects conj :before)\n(defn f [] (later 1))\n(swap! effects conj :after)";
    for fail in [false, false, true] {
        let warnings = Rc::new(RefCell::new(Vec::new()));
        let received = warnings.clone();
        let mut session = EvalSession::new(EvalOptions {
            warning_mode: true,
            warnings: Some(Rc::new(move |warning| {
                received.borrow_mut().push(warning.to_string());
                if fail {
                    Err(light_cljs::evaluator::EvalError("receiver failed".into()))
                } else {
                    Ok(())
                }
            })),
            file_provider: Some(Rc::new(move |_: &FileLoadRequest| {
                Ok(Some(SourceInput::new("warn.cljs", file)))
            })),
            ..Default::default()
        })
        .unwrap();
        session.eval_string("(def effects (atom []))").unwrap();
        let loaded = session.eval_string("(load-file \"warn.cljs\")");
        assert_eq!(loaded.is_err(), fail);
        if fail {
            assert!(
                loaded.unwrap_err().to_string().contains("receiver failed"),
                "the receiver's error is the load's"
            );
        }
        assert_eq!(
            *warnings.borrow(),
            ["warn.cljs:2:13: Use of undeclared Var cljs.user/later [undeclared-var]"]
        );
        let effects = session.eval_string("@effects").unwrap().value;
        assert_eq!(
            session.print(effects).unwrap(),
            if fail {
                "[:before]"
            } else {
                "[:before :after]"
            }
        );
    }
}

/// The namespaces a unit registers with the runtime, in order.
fn registered(unit: &light_cljs::dev_session::Unit) -> Vec<&str> {
    unit.js
        .split("registerReaderNamespace(\"")
        .skip(1)
        .map(|rest| &rest[..rest.find('"').unwrap()])
        .collect()
}

/// A unit registers the namespaces whose reader context or definitions
/// changed since the session's last unit, which is every namespace the
/// session knows for its first (light-cljs-301), so a form in a namespace
/// that defines nothing new registers none.
#[test]
fn a_unit_registers_the_namespaces_that_changed() {
    let dev = DevSession::new(&EvalOptions::default()).unwrap();
    let actions = dev.source(&input(
        "(def x 1) (+ x 1) (def y 2) (ns app.other (:require [cljs.user :as-alias u])) \
         (def z ::u/k) (in-ns 'cljs.user) [x y app.other/z]",
    ));
    assert_eq!(
        shape(&actions),
        [
            "invoke cljs.user",
            "invoke cljs.user",
            "invoke cljs.user",
            "ns app.other",
            "invoke app.other",
            "ns cljs.user",
            "invoke cljs.user"
        ]
    );
    let units = actions
        .iter()
        .filter_map(|action| match action {
            Action::Invoke(unit) => Some(registered(unit)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        units,
        [
            vec!["cljs.user"],
            vec![],
            vec!["cljs.user"],
            vec!["app.other"],
            vec![]
        ]
    );
    assert_eq!(
        apply(&Evaluator::new(Default::default()).unwrap(), &actions),
        "[1 2 :cljs.user/k]"
    );
}

/// The unit of a session's `def` registers only the definition it adds, not
/// every definition before it (light-cljs-366): the unit of the 2,000th
/// `def` is as large as that of the 100th, and a runtime that ran every unit
/// has all of the namespace's definitions, as when each unit registered the
/// whole namespace.
#[test]
fn a_def_unit_does_not_grow_with_the_definitions_before_it() {
    let dev = DevSession::new(&EvalOptions::default()).unwrap();
    let engine = Evaluator::new(Default::default()).unwrap();
    let mut sizes = Vec::new();
    for n in 1..=2000 {
        let actions = dev.source(&input(&format!("(def v{n:04} (vec (range 50)))")));
        let [Action::Invoke(unit)] = &actions[..] else {
            panic!("{:?}", shape(&actions));
        };
        assert_eq!(registered(unit), ["cljs.user"]);
        sizes.push(unit.js.len());
        apply(&engine, &actions);
    }
    assert_eq!(sizes[99], sizes[1999]);
    // The first unit registers the whole namespace, as before.
    assert!(sizes[0] > sizes[1], "{} {}", sizes[0], sizes[1]);
    let actions = dev.source(&input(
        "[(read-string \"`v0001\") (read-string \"`v1000\") (read-string \"`v2000\") \
          (read-string \"`inc\")]",
    ));
    assert_eq!(
        apply(&engine, &actions),
        "[(quote cljs.user/v0001) (quote cljs.user/v1000) (quote cljs.user/v2000) \
         (quote cljs.core/inc)]"
    );
}

/// Reflection in a session sees every namespace as it did when each unit
/// registered every namespace: `find-ns`, `ns-publics`, `resolve`, and
/// `read-string` with the aliases and definitions of a namespace other than
/// the current one, also of a namespace that only a host binding declared.
#[test]
fn reflection_sees_every_namespace_of_the_session() {
    let mut session = EvalSession::new(EvalOptions::default()).unwrap();
    fn printed(session: &mut EvalSession, source: &str) -> String {
        let value = session.eval_string(source).unwrap().value;
        session.print(value).unwrap()
    }
    let s = &mut session;
    printed(
        s,
        "(ns app.a (:require [app.lib :as-alias lib])) (defn f [] 1) (def g 2)",
    );
    printed(s, "(ns app.b) (def h 3)");
    let value = s.eval_string("42").unwrap().value;
    s.bind("app.c/v", value).unwrap();
    assert_eq!(
        printed(
            s,
            "[(str *ns*) (binding [*ns* (find-ns 'app.a)] \
              [(read-string \"::lib/k\") (read-string \"`f\") (read-string \"`h\")]) \
             (sort (keys (ns-publics 'app.a))) (resolve 'app.a/g) app.c/v \
             (binding [*ns* (find-ns 'app.c)] (read-string \"`v\"))]"
        ),
        "[\"app.b\" [:app.lib/k (quote app.a/f) (quote app.a/h)] (f g) #'app.a/g 42 (quote app.c/v)]"
    );
    // An analyzed form's unit that runs after later units registered its
    // namespace again leaves the next unit to register what they did.
    let form = s.read_forms(&input("(def early 1)")).unwrap().remove(0);
    let analyzed = s.analyze(&form).unwrap();
    printed(s, "(def map 2)");
    s.evaluate(&analyzed).unwrap();
    assert_eq!(
        printed(s, "[(read-string \"`map\") (read-string \"`early\")]"),
        "[(quote app.b/map) (quote app.b/early)]"
    );
    // A unit the heap cannot even parse registered nothing: the next unit
    // registers what it would have.
    printed(s, "(ns app.d (:require [app.lib :as-alias lib]))");
    assert!(s.eval_string("(js* \"(\")").is_err());
    assert_eq!(printed(s, "(read-string \"::lib/k\")"), ":app.lib/k");
}

/// Loading a namespace whose macro calls a helper the namespace declares
/// first and defines after the macro's use, as cuerdas.core's `unsafe-concat`
/// calls its `repeat` in Penpot: the macro sees the whole file's definitions,
/// as upstream's Clojure load of the file gives it, in a `DevSession` (the
/// session of a watch without `--cache-dir`) and in an `EvalSession`, and the
/// file's units still run in order.
#[test]
fn a_loaded_macro_calls_a_later_definition_of_its_namespace() {
    // `app.loose` also uses an undeclared Var, a warning in warning mode.
    let source = |namespace: &str, extra: &str| {
        format!(
            "(ns {namespace} (:refer-clojure :exclude [repeat]))\n\
             (declare repeat)\n\
             (defmacro stars [& xs] (repeat \"*\" (count xs)))\n\
             (defn three [] (stars 1 2 3))\n\
             (def early (three)){extra}\n\
             (defn repeat [s n] (apply str (clojure.core/repeat n s)))\n\
             (defn four [] (str (stars 1 2 3 4) (repeat \"-\" 2)))"
        )
    };
    let text = source("app.text", "");
    let loose = source("app.loose", "\n(defn bad [] (nowhere))");
    let served = text.clone();
    let options = EvalOptions {
        source_provider: Some(Rc::new(move |request: &LoadRequest| {
            let name = String::from_utf16_lossy(request.namespace.name());
            Ok(match name.as_str() {
                "app.text" => Some(SourceInput::new("app/text.cljc", served.clone())),
                "app.loose" => Some(SourceInput::new("app/loose.cljc", loose.clone())),
                _ => None,
            })
        })),
        ..Default::default()
    };
    let dev = DevSession::new(&options).unwrap();
    let engine = Evaluator::new(Default::default()).unwrap();
    let require = "(require 'app.text) [app.text/early (app.text/four)]";
    let actions = dev.source(&input(require));
    assert!(
        !matches!(actions.last(), Some(Action::Error(_))),
        "{:?}",
        shape(&actions)
    );
    assert_eq!(apply(&engine, &actions), r#"["***" "****--"]"#);
    // The form's warning comes once, from its own turn, not from the forms
    // defined ahead.
    let actions = dev.source(&input("(require 'app.loose) (app.loose/four)"));
    let warnings = shape(&actions)
        .into_iter()
        .filter(|action| action.starts_with("warnings"))
        .collect::<Vec<_>>();
    assert_eq!(
        warnings,
        ["warnings Use of undeclared Var app.loose/nowhere [undeclared-var]"]
    );
    assert_eq!(apply(&engine, &actions), r#""****--""#);
    // A helper the file uses before defining it without declaring it, which
    // warning mode allows: the use fails, and the form is analyzed again.
    let later = dev.load_namespace_file(
        "app.later",
        &SourceInput::new(
            "app/later.cljc",
            "(ns app.later) (defmacro k [] (g)) (def w (k)) (defn g [] 3)",
        ),
    );
    assert!(
        !matches!(later.last(), Some(Action::Error(_))),
        "{:?}",
        shape(&later)
    );
    apply(&engine, &later);
    assert_eq!(apply(&engine, &dev.source(&input("app.later/w"))), "3");
    // Loading the file again, as a watch step does, sees the definitions too.
    let reload = dev.load_namespace_file("app.text", &SourceInput::new("app/text.cljc", text));
    assert!(
        !matches!(reload.last(), Some(Action::Error(_))),
        "{:?}",
        shape(&reload)
    );
    apply(&engine, &reload);
    assert_eq!(
        apply(&engine, &dev.source(&input("(app.text/three)"))),
        r#""***""#
    );
    // A definition the file never makes is still unavailable to the macro.
    let missing = dev.load_namespace_file(
        "app.text",
        &SourceInput::new(
            "app/text.cljc",
            "(ns app.text) (declare gone) (defmacro m [] (gone)) (defn f [] (m)) (def x 1)",
        ),
    );
    assert!(
        matches!(missing.last(), Some(Action::Error(error))
            if error.message.contains("unavailable definition: app.text/gone")),
        "{:?}",
        shape(&missing)
    );

    for warning_mode in [true, false] {
        let mut session = EvalSession::new(EvalOptions {
            warning_mode,
            ..options.clone()
        })
        .unwrap();
        let value = session.eval_source(&input(require)).unwrap().value;
        assert_eq!(session.print(value).unwrap(), r#"["***" "****--"]"#);
    }
}

/// Defining a file's later helpers ahead of their turn keeps the load's
/// source order otherwise: a macro the file redefines later is not the one
/// an earlier use expands, a macro use expands once, and a data reader runs
/// once for each occurrence, and a syntax quote read ahead qualifies a name
/// defined before it, in warning mode and not.
#[test]
fn defining_ahead_keeps_macro_generations_expansions_and_reads_in_order() {
    let text = "(ns app.demo)\n\
                (def counter (atom 0))\n\
                (declare f)\n\
                (defmacro m [] (swap! counter inc) (f) @counter)\n\
                (def x (m))\n\
                (defn f [] nil)\n\
                (defmacro m [] 2)\n\
                (def y (m))\n\
                (def z #tick nil)\n\
                (def map 7)\n\
                (def quoted `map)";
    for warning_mode in [true, false] {
        let mut session = EvalSession::new(EvalOptions {
            warning_mode,
            data_readers: vec![(
                light_cljs::compiler_data::Name::new(None, "tick"),
                "(let [n (atom 0)] (fn [_] (swap! n inc)))".into(),
            )],
            source_provider: Some(Rc::new(move |request: &LoadRequest| {
                let name = String::from_utf16_lossy(request.namespace.name());
                Ok((name == "app.demo").then(|| SourceInput::new("app/demo.cljc", text)))
            })),
            ..Default::default()
        })
        .unwrap();
        let value = session
            .eval_source(&input(
                "(require 'app.demo) [app.demo/x app.demo/y app.demo/z app.demo/quoted]",
            ))
            .unwrap()
            .value;
        assert_eq!(
            session.print(value).unwrap(),
            "[1 2 1 app.demo/map]",
            "{warning_mode}"
        );
    }
}
