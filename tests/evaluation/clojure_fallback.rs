//! `--eval` and `--repl` resolve a `clojure.X` require that no source
//! provides to `cljs.X`, a source root's or a bundled library's, as
//! `--compile` does (tests/compiler/clojure_fallback.rs): in `ns`, `ns*` and
//! `require`, with both qualifiers naming it. An explicit `clojure.X`
//! source still wins.
use crate::temp_dir::TempDir;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

/// The standard output of `light-cljs ARGS --source-root ROOT` given
/// `input`, which must succeed without diagnostics.
fn run(root: &Path, args: &[&str], input: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .env("PATH", "")
        .args(args)
        .arg("--source-root")
        .arg(root)
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
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success() && stderr.is_empty(), "{stderr}");
    String::from_utf8(output.stdout).unwrap()
}

const MAIN: &str = "(ns app.main (:require [clojure.reader :as r :refer [read-string]] [clojure.foo :as f :refer [v]]))
[(r/read-string \"{:a 1}\") (read-string \"[2]\") (clojure.reader/read-string \":k\") (cljs.reader/read-string \"3\")
 f/v v clojure.foo/v cljs.foo/v]";

#[cfg(light_v8)]
#[test]
fn eval_falls_back_from_clojure_to_bundled_and_source_cljs_namespaces() {
    let dir = TempDir::new("eval-clojure-fallback");
    let root = dir.join("src");
    write(&root, "cljs/foo.cljs", "(ns cljs.foo) (def v 41)");
    let main = dir.join("main.cljs");
    fs::write(&main, MAIN).unwrap();
    assert_eq!(
        run(&root, &["--eval", main.to_str().unwrap()], ""),
        "[{:a 1} [2] :k 3 41 41 41 41]\n"
    );
    // A quoted `require` and an `ns*` take the same fallback, which may
    // require the library again under the same alias.
    assert_eq!(
        run(
            &root,
            &[
                "--eval",
                "--text",
                "(require '[clojure.reader :as r] '[clojure.foo :refer [v]])
                 (ns* (:require '[clojure.reader :as r2] '[clojure.reader :as r]))
                 [(r/read-string \"{:b 2}\") (r2/read-string \"4\") (r/read-string \"6\") (clojure.reader/read-string \"5\") v clojure.foo/v]",
            ],
            "",
        ),
        "[{:b 2} 4 6 5 41 41]\n"
    );
    // An explicit `clojure.foo` source replaces `cljs.foo`.
    write(&root, "clojure/foo.cljs", "(ns clojure.foo) (def v 7)");
    assert_eq!(
        run(
            &root,
            &[
                "--eval",
                "--text",
                "(ns app (:require [clojure.foo :as f])) [f/v clojure.foo/v]",
            ],
            "",
        ),
        "[7 7]\n"
    );
}

#[cfg(light_v8)]
#[test]
fn repl_falls_back_from_clojure_to_bundled_and_source_cljs_namespaces() {
    let dir = TempDir::new("repl-clojure-fallback");
    let root = dir.join("src");
    write(&root, "cljs/foo.cljs", "(ns cljs.foo) (def v 41)");
    assert_eq!(
        run(
            &root,
            &["--repl"],
            "(require '[clojure.reader :as r] '[clojure.foo :refer [v]])\n\
             [(r/read-string \"{:b 2}\") (clojure.reader/read-string \"3\") (cljs.reader/read-string \"4\") v]\n\
             (ns x (:require [clojure.foo :as ff]))\n\
             [ff/v clojure.foo/v cljs.foo/v]\n",
        ),
        "nil\n[{:b 2} 3 4 41]\nnil\n[41 41 41]\n"
    );
    // An explicit `clojure.reader` source replaces the bundled `cljs.reader`.
    write(
        &root,
        "clojure/reader.cljs",
        "(ns clojure.reader) (defn read-string [s] (str \"own \" s))",
    );
    assert_eq!(
        run(
            &root,
            &["--repl"],
            "(require '[clojure.reader :as r])\n[(r/read-string \"1\") (clojure.reader/read-string \"2\")]\n",
        ),
        "nil\n[\"own 1\" \"own 2\"]\n"
    );
}
