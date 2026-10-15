use crate::temp_dir::TempDir;
use light_cljs::source_provider::{FileSourceProvider, SourceInput};
use light_cljs::{EvalOptions, EvalSession};
use std::{path::PathBuf, rc::Rc};

fn session(root: PathBuf) -> EvalSession {
    EvalSession::new(EvalOptions {
        source_provider: Some(Rc::new(FileSourceProvider::new(vec![root]))),
        ..Default::default()
    })
    .unwrap()
}
fn printed(session: &mut EvalSession, source: &str) -> String {
    let value = session.eval_string(source).unwrap().value;
    let text = session.print(value).unwrap();
    session.release(value).unwrap();
    text
}
fn original(session: &mut EvalSession) {
    let result = session
        .eval_source(&SourceInput::new(
            "tests/fixtures/compiler-nsstar/syntax/nsstar.cljs",
            include_str!("../fixtures/compiler-nsstar/syntax/nsstar.cljs"),
        ))
        .unwrap();
    assert_eq!(session.string(result.value).unwrap(), "true");
    assert_eq!(session.print(result.value).unwrap(), "\"true\"");
    assert_eq!(result.namespace, "syntax.nsstar");
    session.release(result.value).unwrap();
    assert_eq!(
        printed(session, "[::text/name (text/join \"-\" [1 2])]"),
        "[:clojure.string/name \"1-2\"]"
    );
}
#[test]
fn unchanged_complete_nsstar_source_executes_in_fresh_and_reused_sessions() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/compiler-nsstar");
    let mut reused = session(root);
    for _ in 0..2 {
        original(&mut reused);
        original(&mut reused.fresh_session().unwrap());
    }
    printed(&mut reused, "(ns isolated)");
    assert!(reused.eval_string("text/join").is_err());
    assert!(reused.eval_string("::text/name").is_err());
    original(&mut reused);
}

#[test]
fn malformed_nsstar_is_located_and_failed_imports_do_not_publish_aliases() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/compiler-nsstar");
    let mut reused = session(root);
    for (body, expected) in [
        (
            "(ns* (:require [clojure.string :as leaked]))",
            "must be quoted",
        ),
        (
            "(ns* (:require '[missing.library :as leaked]))",
            "missing namespace",
        ),
        ("(ns* (:require '[clojure.string :as 1]))", ":as alias"),
        (
            "(ns* (:require '[clojure.string :as leaked :refer [absent]]))",
            "public Var or macro",
        ),
        (
            "(ns* (:require '[clojure.string :as leaked :as other]))",
            "duplicate require option",
        ),
        (
            "(ns* (:require '[clojure.string :rename {join combine}]))",
            "not referred",
        ),
        (
            "(fn [] (ns* (:require '[clojure.string :as leaked])))",
            "top-level",
        ),
        ("(ns* other)", "literal namespace directive"),
        (
            "(ns* (:require 'clojure.string) (:require 'clojure.set))",
            "one namespace directive",
        ),
        ("(ns* (:require-macros 'syntax.jvm))", "retired"),
    ] {
        let error = reused
            .eval_source(&SourceInput::new(
                "invalid.cljs",
                format!("(ns errors)\n{body}"),
            ))
            .unwrap_err();
        assert!(error.message.contains(expected), "{body}: {error}");
        let location = error.location.unwrap();
        assert_eq!(location.source().name(), "invalid.cljs");
        assert_eq!(
            location
                .source()
                .line_column(location.bytes().start)
                .unwrap()
                .0,
            2
        );
        assert!(reused.eval_string("leaked/join").is_err(), "{body}");
        assert!(reused.eval_string("::leaked/name").is_err(), "{body}");
        original(&mut reused);
    }
    printed(
        &mut reused,
        "(ns errors) (ns* (:require '[clojure.string :as kept]))",
    );
    let error = reused
        .eval_string("(ns* (:require '[clojure.set :as kept]))")
        .unwrap_err();
    assert!(
        error.message.contains("conflicting namespace alias"),
        "{error}"
    );
    assert_eq!(printed(&mut reused, "(kept/join \"-\" [1 2])"), "\"1-2\"");
    assert_eq!(
        printed(
            &mut reused,
            "(ns* (:require '[missing.alias :as-alias only])) ::only/name"
        ),
        ":missing.alias/name"
    );
    assert!(reused.eval_string("only/value").is_err());
}

#[test]
fn source_macros_referrals_imports_and_ordered_effects_keep_the_declared_owner() {
    let root = TempDir::new("eval-nsstar");
    std::fs::create_dir_all(root.join("syntax")).unwrap();
    std::fs::write(root.join("syntax/library.cljs"),
        "(ns syntax.library) (def value 7) (defmacro snapshot [] [value (str (get-in &env [:ns :name]))])").unwrap();
    std::fs::write(
        root.join("syntax/library.clj"),
        "(ns syntax.library) (defmacro snapshot [] false)",
    )
    .unwrap();
    let reused = EvalSession::new(EvalOptions {
        source_provider: Some(Rc::new(FileSourceProvider::new(vec![root.to_path_buf()]))),
        file_provider: Some(Rc::new(|_| {
            Ok(Some(SourceInput::new(
                "callback.cljs",
                "(ns* (:require '[clojure.string :as callback])) (callback/join \"-\" [3 4])",
            )))
        })),
        ..Default::default()
    })
    .unwrap();
    let source = r#"(ns syntax.owner)
(def effects (atom []))
(do (swap! effects conj :before)
    (ns* (:require '[syntax.library :as l :refer [snapshot] :rename {snapshot capture}]))
    (ns* (:require '[clojure.string :refer [join] :rename {join combine}]))
    (swap! effects conj :after))
(ns* (:import '[Host Widget]))
(ns* (:refer-global :only '[Date]))
(set! js/Host (js-obj "Widget" (js* "(function Widget(x) { this.x = x; })")))
(def owned l/value)
(pr-str [(= [7 [7 "syntax.owner"] [7 "syntax.owner"] "1-2"]
             [syntax.owner/owned (capture) (l/snapshot) (combine "-" [1 2])])
         @effects (= 7 (.-x (Widget. 7))) (identical? Date js/Date)
         (= ['Widget Widget] (find (ns-imports 'syntax.owner) 'Widget))])"#;
    std::fs::write(root.join("syntax/owner.cljs"), source).unwrap();
    let javascript =
        light_cljs::compile_namespace("syntax.owner", std::slice::from_ref(root.path())).unwrap();
    let engine = light_cljs::evaluator::Evaluator::new(Default::default()).unwrap();
    let value = engine.evaluate_js(&javascript).unwrap();
    assert_eq!(
        engine.string(value).unwrap(),
        "[true [:before :after] true true true]"
    );
    engine.release(value).unwrap();
    let output = std::process::Command::new("node")
        .args(["-e", &format!("console.log({javascript});")])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[true [:before :after] true true true]\n"
    );
    for mut current in [reused.fresh_session().unwrap(), reused] {
        assert_eq!(
            printed(&mut current, source),
            "\"[true [:before :after] true true true]\""
        );
        // Reapplying the same source referrals preserves their resolved identities.
        assert_eq!(
            printed(
                &mut current,
                "(ns* (:require '[syntax.library :as l :refer [snapshot] :rename {snapshot capture}])) (ns* (:require '[clojure.string :refer [join] :rename {join combine}])) [(capture) (combine \"-\" [1 2]) @effects]"
            ),
            "[[7 \"syntax.owner\"] \"1-2\" [:before :after]]"
        );
        assert_eq!(current.namespace(), "syntax.owner");
        assert!(current.eval_string("l/snapshot").is_err());
        // The callback path also loads raw source directives using this compiler.
        assert_eq!(
            printed(&mut current, "(load-file \"callback.cljs\")"),
            "\"3-4\""
        );
        assert_eq!(
            printed(
                &mut current,
                "(ns ordinary (:require [clojure.string :as text])) (text/upper-case \"a\")"
            ),
            "\"A\""
        );
    }
}
