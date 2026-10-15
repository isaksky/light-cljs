//! The session compiler as an action producer (docs/dev-workflow.md,
//! "Actions" and "Phase 1"): a `DevSession` compiles forms, requires and
//! loaded files into actions without a heap, and the units of those actions
//! run in any heap: a fresh QuickJS evaluator here, and a Node process running
//! the Phase 0 dev build of examples/dev through its driver.
use crate::dev_execution::run_dev;
use crate::temp_dir::TempDir;
use light_cljs::dev_session::{Action, DevSession, define_script};
use light_cljs::evaluator::Evaluator;
use light_cljs::source_provider::{FileSourceProvider, LoadRequest, SourceInput};
use light_cljs::{EvalOptions, EvalSession};
use std::{
    cell::{Cell, RefCell},
    fs,
    path::Path,
    process::Command,
    rc::Rc,
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
    assert_eq!(unit.location.bytes(), 11..18);
    assert!(unit.source_map.is_none());
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
    assert_eq!(
        shape(&actions),
        [
            "ns cljs.user",
            "ns b",
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
    let [_, _, Action::Load(old), ..] = first.as_slice() else {
        unreachable!()
    };
    let [_, _, Action::Load(new), ..] = reloaded.as_slice() else {
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
        ["ns tool", "ns a", "load a", "ns tool", "load tool tool"]
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

/// The units of a `require` compiled without a heap from an edited
/// examples/dev apply to the running Phase 0 dev build of the original with
/// the hook protocol: the next render runs the new code, and the `defonce`
/// state keeps its clicks.
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
    let dev = DevSession::new(&EvalOptions {
        source_provider: Some(Rc::new(FileSourceProvider::new(vec![root]))),
        ..Default::default()
    })
    .unwrap();
    let actions = dev.source(&input("(require 'app.view)"));
    assert_eq!(
        shape(&actions)
            .into_iter()
            .filter(|action| action.starts_with("load"))
            .collect::<Vec<_>>(),
        [
            "load app.macros app.macros",
            "load app.state app.state",
            "load app.view app.view app.view app.view"
        ]
    );
    let units = actions
        .iter()
        .flat_map(|action| match action {
            Action::Load(units) => units.clone(),
            _ => vec![],
        })
        .collect::<Vec<_>>();
    let script = dir.join("units.js");
    fs::write(&script, define_script(&units)).unwrap();
    assert_eq!(
        run_dev(&out, true, &[script]).unwrap(),
        r#"{"id":"program","outcome":{"status":"returned","value":{"$value":"undefined"}},"trace":[],"stdout":"start\nclicks: [1]\nreloading\nrender: clicks: [2]\n"}"#
    );
}
