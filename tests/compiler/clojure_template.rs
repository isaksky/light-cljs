//! The bundled `clojure.template`: `apply-template` and `do-template` called
//! directly and from user macros, in `--compile` builds (uncached and cached),
//! `--eval` and `--repl`, and overridden by an explicit `--source-root`.
use crate::temp_dir::TempDir;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

fn light_cljs(dir: &Path, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

/// A program whose user macros build on both parts of the library, and whose
/// entry also uses them directly, through `:refer` and an alias.
fn template_program(root: &Path) {
    write(
        root,
        "app/macros.cljs",
        "(ns app.macros (:require [clojure.template :as t]))
         (defmacro swapped [& vs]
           `(vector ~@(map (fn [v] (t/apply-template '[a b] '[b a] v)) (partition 2 vs))))
         (defmacro logged-sums [& vs]
           `(t/do-template [x y] (swap! app.main/log conj (+ x y)) ~@vs))",
    );
    write(
        root,
        "app/main.cljs",
        "(ns app.main
           (:require [clojure.template :as t :refer [do-template]]
                     [app.macros :refer [swapped logged-sums]]))
         (def log (atom []))
         (do-template [x y] (swap! log conj [y x]) 1 2 3 4)
         (logged-sums 10 20 30 40)
         (pr-str [@log (swapped 1 2 3 4)
                  (t/apply-template '[k v] '{k [v v]} [:a 1])
                  (macroexpand '(clojure.template/do-template [x y] (+ y x) 2 4 3 5))])",
    );
}

const TEMPLATE_RESULT: &str = "[[[2 1] [4 3] 30 70] [[2 1] [4 3]] {:a [1 1]} (do (+ 4 2) (+ 5 3))]";

fn run_js(js: &str) -> String {
    crate::canonical_json::node_stdout(&format!("console.log({js});"))
        .trim_end()
        .to_owned()
}

fn stats_value(stderr: &str, key: &str) -> String {
    stderr
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key} ")))
        .unwrap_or_else(|| panic!("no {key} in {stderr}"))
        .to_owned()
}

#[test]
fn compile_runs_templates_directly_and_in_user_macros_uncached_and_cached() {
    let dir = TempDir::new("clojure-template-compile");
    let root = dir.join("src");
    template_program(&root);
    let root = root.to_str().unwrap();
    let uncached = stdout(light_cljs(
        dir.path(),
        &[
            "--compile",
            "--entry",
            "app.main",
            "--source-root",
            root,
            "--no-cache",
        ],
        "",
    ));
    assert_eq!(run_js(&uncached), TEMPLATE_RESULT);
    let cache = dir.join("cache");
    let cached = || {
        let output = light_cljs(
            dir.path(),
            &[
                "--compile",
                "--entry",
                "app.main",
                "--source-root",
                root,
                "--cache-dir",
                cache.to_str().unwrap(),
                "--stats",
            ],
            "",
        );
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        (
            stdout(output),
            stats_value(&stderr, "cache.install.namespaces"),
        )
    };
    // The first build stores every namespace, clojure.template included; the
    // second installs them all instead of expanding the templates again.
    assert_eq!(cached(), (uncached.clone(), "0".into()));
    assert_eq!(cached(), (uncached, "4".into()));
    // A changed entry that reaches the template only through a user macro
    // expands it again in a cached build.
    write(
        Path::new(root),
        "app/main.cljs",
        "(ns app.main (:require [app.macros :refer [swapped]])) (pr-str (swapped :x :y))",
    );
    assert_eq!(run_js(&cached().0), "[[:y :x]]");
}

#[test]
fn eval_and_repl_require_the_bundled_template() {
    let dir = TempDir::new("clojure-template-eval");
    let direct = "(require '[clojure.template :as t])
        [(t/apply-template '[x y] '(+ x y [x]) [1 2])
         (let [n (atom 0)] (t/do-template [a b] (swap! n + (* a b)) 1 2 3 4) @n)
         (macroexpand '(t/do-template [x y] (+ y x) 2 4 3 5))
         (try (t/apply-template '(x) 'x [1]) :accepted (catch :default _ :rejected))]";
    assert_eq!(
        stdout(light_cljs(dir.path(), &["--eval", "--text", direct], "")),
        "[(+ 1 2 [1]) 14 (do (+ 4 2) (+ 5 3)) :rejected]\n"
    );
    assert_eq!(
        stdout(light_cljs(
            dir.path(),
            &["--repl"],
            "(require '[clojure.template :refer [do-template apply-template]])\n\
             (do-template [n v] (def n v) one 1 two 2)\n\
             [one two (apply-template '[x] '(* x x) [3])]\n",
        )),
        "nil\n2\n[1 2 (* 3 3)]\n"
    );
    // User macros built on the template, from a source root, in both modes.
    let root = dir.join("src");
    template_program(&root);
    let root = root.to_str().unwrap();
    assert_eq!(
        stdout(light_cljs(
            dir.path(),
            &[
                "--eval",
                "--source-root",
                root,
                "--text",
                "(require 'app.main) @app.main/log",
            ],
            "",
        )),
        "[[2 1] [4 3] 30 70]\n"
    );
    assert_eq!(
        stdout(light_cljs(
            dir.path(),
            &["--repl", "--source-root", root],
            "(require '[app.macros :refer [swapped]])\n\
             (swapped 1 2 3 4)\n",
        )),
        "nil\n[[2 1] [4 3]]\n"
    );
}

#[test]
fn an_explicit_source_root_overrides_the_bundled_template() {
    let dir = TempDir::new("clojure-template-override");
    let root = dir.join("src");
    write(
        &root,
        "clojure/template.cljs",
        "(ns clojure.template)
         (defn apply-template [argv expr values] [:user argv expr values])
         (defmacro do-template [argv expr & values] [:user-macro (count values)])",
    );
    let program = "[(t/apply-template '[x] 'x [1]) (t/do-template [x] x 1 2 3)]";
    write(
        &root,
        "app/main.cljs",
        &format!("(ns app.main (:require [clojure.template :as t])) (pr-str {program})"),
    );
    let root = root.to_str().unwrap();
    let expected = "[[:user [x] x [1]] [:user-macro 3]]";
    let js = stdout(light_cljs(
        dir.path(),
        &[
            "--compile",
            "--entry",
            "app.main",
            "--source-root",
            root,
            "--no-cache",
        ],
        "",
    ));
    assert_eq!(run_js(&js), expected);
    for mode in ["--eval", "--repl"] {
        assert_eq!(
            stdout(light_cljs(
                dir.path(),
                &[mode, "--source-root", root],
                &format!("(require '[clojure.template :as t])\n{program}\n"),
            )),
            if mode == "--eval" {
                format!("{expected}\n")
            } else {
                format!("nil\n{expected}\n")
            },
            "{mode}"
        );
    }
}
