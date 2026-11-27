//! The bundled `cljs.pprint`, required as `cljs.pprint` and as `clojure.pprint`:
//! `pprint`, `write`, `write-out`, `cl-format`, `formatter`, `formatter-out`,
//! `print-table`, `code-dispatch`, custom dispatch built from
//! `pprint-logical-block` / `print-length-loop` / `pprint-newline` /
//! `pprint-indent`, and public utilities across `--compile` (`none`, `prepare`,
//! `advanced`), cached builds, `--eval`, `--repl`, macro-time `cl-format`, and
//! `--source-root` override (light-cljs-1169).
use crate::temp_dir::TempDir;
use light_cljs::compiler::analyze_namespace_entry_with_dependencies;
use light_cljs::dependency_manifest::{RequireForm, RequireTarget, SourceOrigin};
use light_cljs::{MacroHost, analyzer::AnalysisOptions};
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

fn stderr_on_err(output: Output) -> String {
    assert!(!output.status.success(), "unexpected success");
    String::from_utf8(output.stderr).unwrap()
}

fn write_file(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

fn run_node(js: &str) -> String {
    crate::canonical_json::node_stdout(js)
}

/// A program exercising the full `cljs.pprint` / `clojure.pprint` surface with
/// deterministic output, asserting exact formatted strings.
const PPRINT_SURFACE_MAIN: &str = r#"(ns app.main
  (:require [cljs.pprint :as pp
             :refer [pprint write write-out cl-format print-table
                     pprint-newline pprint-indent pprint-tab
                     set-pprint-dispatch get-pretty-writer fresh-line
                     float? char-code simple-dispatch code-dispatch
                     pprint-logical-block print-length-loop
                     formatter formatter-out with-pprint-dispatch]]
            [clojure.pprint :as cpp]))

(enable-console-print!)

(defrecord Point [x y])

(defn- make-collector []
  (let [chunks (atom [])]
    {:chunks chunks
     :writer (reify IWriter
               (-write [this s] (swap! chunks conj s) this)
               (-flush [_] nil))}))

(def ^:private output-buf (atom []))

(defn- emit [label s]
  (swap! output-buf conj (str "=== " label " ===\n" s
                              (when-not (or (empty? s) (= "\n" (subs s (dec (count s)))))
                                "\n"))))

;; 1. pprint of core data types, collections, records, queues, JS arrays/objects, and wide wrapping
(emit "types"
      (with-out-str
        (pprint (sorted-map :a 1
                            :b [2 3]
                            :c '(4 5)
                            :d (sorted-set 6 7)
                            :e (->Point 8 9)
                            :f (into cljs.core.PersistentQueue.EMPTY [10 11])
                            :g #js [12 13]
                            :h #js {:k 14}
                            :i 'sym
                            :j "hi"
                            :k nil))))

(emit "wide-margin"
      (binding [pp/*print-right-margin* 20]
        (with-out-str
          (pprint (sorted-map 1 (sorted-map 12345 123456)
                              3 (sorted-map 4 5 6 7))))))

(emit "length-and-level"
      (with-out-str
        (binding [*print-length* 2]
          (pprint [1 2 3 4])
          (pprint '(a b c)))
        (binding [*print-level* 2]
          (pprint [1 [2 [3 [4]]]]))))

(emit "miser-width"
      (with-pprint-dispatch code-dispatch
        (let [form '(do (if cond-a then-val else-val))
              with-miser (binding [pp/*print-right-margin* 22
                                   pp/*print-miser-width* 20]
                           (with-out-str (pprint form)))
              without-miser (binding [pp/*print-right-margin* 22
                                      pp/*print-miser-width* nil]
                              (with-out-str (pprint form)))]
          (when (= with-miser without-miser)
            (throw (js/Error. "expected *print-miser-width* to change output")))
          (str "miser=20:\n" with-miser "miser=nil:\n" without-miser))))

(emit "namespace-maps"
      (with-out-str
        (binding [*print-namespace-maps* true]
          (pprint {:user/a 1 :user/b 2}))
        (binding [*print-namespace-maps* false]
          (pprint {:user/a 1 :user/b 2}))))

;; 2. write with :stream nil and keyword options, plus pprint to explicit writer
(emit "write-options"
      (str (write [1 2 3 4] :stream nil :length 2) "\n"
           (write [1 [2 [3]]] :stream nil :level 2) "\n"
           (write '(a b c d) :stream nil :pretty false) "\n"
           (write 'foo.bar/baz :stream nil :dispatch code-dispatch :suppress-namespaces true) "\n"
           (write (sorted-set 123 456 789) :stream nil :right-margin 8) "\n"))

(emit "explicit-writer"
      (let [{:keys [chunks writer]} (make-collector)]
        (pprint {:k [1 2]} writer)
        (apply str @chunks)))

;; 3. cl-format directives, destinations (nil, true, writer), formatter & formatter-out
(emit "cl-format-directives"
      (str (cl-format nil "~a | ~s | ~d | ~r | ~:r | ~@r" "hi" "hi" 42 15 3 14) "\n"
           (cl-format nil "~,2f | ~,2e | ~$" 3.14159 1234.5 7.5) "\n"
           (cl-format nil "~{~a~^, ~}" [1 2 3]) "\n"
           (cl-format nil "~[zero~;one~;two~:;many~] ~[zero~;one~;two~:;many~]" 1 5) "\n"
           (cl-format nil "~(HeLLo WoRLd~) | ~:(hello world~) | ~@(hello WORLD~) | ~:@(hello world~)") "\n"
           (cl-format nil "~<{~;~@{~w~^ ~_~}~;}~:>" '[a b c]) "\n"
           (cl-format nil "ab~5tc~%de~&fg") "\n"))

(emit "cl-format-streams"
      (let [wcol (make-collector)
            from-true (with-out-str (cl-format true "true:~d" 9))
            fmt (formatter "~a=~:r")
            fmt-out (formatter-out "[~a/~d]")
            out-col (make-collector)]
        (cl-format (:writer wcol) "writer:~a" "ok")
        (binding [*out* (:writer out-col)]
          (fmt-out "k" 7))
        (str from-true "\n"
             (apply str @(:chunks wcol)) "\n"
             (fmt nil "pos" 2) "\n"
             (apply str @(:chunks out-col)) "\n")))

;; 4. print-table with and without explicit keys
(emit "print-table"
      (with-out-str
        (print-table [:b :a] [{:a 1 :b 20} {:a 300 :b 4}])
        (print-table [{:x "aa" :y 1} {:x "b" :y 22}])))

;; 5. code-dispatch on defn, let, cond, if, ns, #(), Atom, Delay, PersistentQueue
(emit "code-dispatch"
      (with-pprint-dispatch code-dispatch
        (let [a (atom {:v 1})
              d (delay 99)
              q (into cljs.core.PersistentQueue.EMPTY [1 2 3])]
          (str (write '(defn greet [name]
                         (let [msg (str "hello " name)]
                           (if (seq name)
                             (cond
                               (= name "alice") :greeting-alice
                               (= name "bob") :greeting-bob
                               :else msg)
                             :empty-name)))
                      :stream nil
                      :right-margin 50) "\n"
               (write '(ns demo.core (:require [clojure.string :as s])) :stream nil) "\n"
               (write '#(+ % %2) :stream nil) "\n"
               (write a :stream nil) " / same=" (= (write a :stream nil) (write a :stream nil)) "\n"
               (write d :stream nil) " -> " (do @d (write d :stream nil)) "\n"
               (write q :stream nil) "\n"))))

;; 6. Custom dispatch with pprint-logical-block, print-length-loop (recur inside when),
;;    pprint-newline, pprint-indent, plus set-pprint-dispatch
(defn- custom-vec-dispatch [x]
  (if (vector? x)
    (pprint-logical-block :prefix "<<" :suffix ">>"
      (pprint-indent :block 1)
      (print-length-loop [s (seq x)]
        (when s
          (write-out (first s))
          (when (next s)
            (-write *out* "; ")
            (pprint-newline :linear)
            (recur (next s))))))
    (simple-dispatch x)))

(emit "custom-dispatch"
      (let [prev pp/*print-pprint-dispatch*]
        (try
          (set-pprint-dispatch custom-vec-dispatch)
          (str (binding [*print-length* 3]
                 (write [10 20 30 40 50] :stream nil)) "\n"
               (binding [pp/*print-right-margin* 12]
                 (write ["alpha" "beta" "gamma"] :stream nil)) "\n")
          (finally
            (set-pprint-dispatch prev)))))

;; 7. Utilities, pprint-tab error, get-pretty-writer + fresh-line, and clojure.pprint qualifiers
(emit "utilities-and-fallback"
      (let [{:keys [chunks writer]} (make-collector)
            pw (get-pretty-writer writer)
            tab-err (try
                      (binding [*out* pw] (pprint-tab :line 1 1))
                      :no-error
                      (catch js/Error e (.-message e)))]
        (binding [*out* pw]
          (-write *out* "first")
          (fresh-line)
          (fresh-line)
          (-write *out* "second")
          (-flush *out*))
        (pr-str
          {:floats [(float? 1.5) (float? 1) (float? js/NaN) (float? js/Infinity) (float? "1.5")]
           :codes [(char-code \A) (char-code 66)]
           :tab tab-err
           :fresh (apply str @chunks)
           :qualifiers [(cpp/cl-format nil "~a" :alias)
                        (clojure.pprint/cl-format nil "~a" :clojure-qual)
                        (cljs.pprint/cl-format nil "~a" :cljs-qual)]})))

(js/process.stdout.write (apply str @output-buf))
"#;

const EXPECTED_PPRINT_SURFACE: &str = r#"=== types ===
{:a 1,
 :b [2 3],
 :c (4 5),
 :d #{6 7},
 :e {:x 8, :y 9},
 :f #queue [10 11],
 :g #js [12 13], 
 :h #js {:k 14}, 
 :i sym, 
 :j "hi", 
 :k nil}
=== wide-margin ===
{1 {12345 123456},
 3 {4 5, 6 7}}
=== length-and-level ===
[1 2 ...]
(a b ...)
[1 [2 #]]
=== miser-width ===
miser=20:
(do
  (if
    cond-a
    then-val
    else-val))
miser=nil:
(do
  (if cond-a
    then-val
    else-val))
=== namespace-maps ===
#:user{:a 1, :b 2}
{:user/a 1, :user/b 2}
=== write-options ===
[1 2 ...]
[1 [2 #]]
(a b c d)
baz
#{123
  456
  789}
=== explicit-writer ===
{:k [1 2]}
=== cl-format-directives ===
hi | "hi" | 42 | fifteen | third | XIV
3.14 | 1.23E+3 | 7.50
1, 2, 3
one many
hello world | Hello World | Hello world | HELLO WORLD
{a b c}
ab   c
de
fg
=== cl-format-streams ===
true:9
writer:ok
pos=second
[k/7]
=== print-table ===

| :b |  :a |
|----+-----|
| 20 |   1 |
|  4 | 300 |

| :x | :y |
|----+----|
| aa |  1 |
|  b | 22 |
=== code-dispatch ===
(defn greet [name]
  (let [msg (str "hello " name)]
    (if (seq name)
      (cond
        (= name "alice") :greeting-alice
        (= name "bob") :greeting-bob
        :else msg) 
      :empty-name)))
(ns demo.core
  (:require [clojure.string :as s]))
#(+ %1 %2)
#<Atom@1: {:v 1}> / same=true
#<Delay@2: :not-delivered> -> #<Delay@2: 99>
<-(1 2 3)-<
=== custom-dispatch ===
<<10; 20; 30; ...>>
<<"alpha";
   "beta";
   "gamma">>
=== utilities-and-fallback ===
{:floats [true false false false false], :codes [65 66], :tab "pprint-tab is not yet implemented", :fresh "first\nsecond", :qualifiers [":alias" ":clojure-qual" ":cljs-qual"]}
"#;

#[test]
fn compile_pprint_surface_across_optimization_levels() {
    let dir = TempDir::new("cljs-pprint-compile");
    let root = dir.join("src");
    write_file(&root, "app/main.cljs", PPRINT_SURFACE_MAIN);
    let root_text = root.to_str().unwrap();
    for level in ["none", "prepare", "advanced"] {
        let output = light_cljs(
            dir.path(),
            &[
                "--warnings",
                "--compile",
                "--optimize",
                level,
                "--entry",
                "app.main",
                "--source-root",
                root_text,
                "--no-cache",
            ],
            "",
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.is_empty(),
            "--optimize {level} emitted unexpected warnings/stderr:\n{stderr}"
        );
        let js = stdout(output);
        if level == "advanced" {
            assert!(
                !js.contains("definitionVar"),
                "--optimize advanced emitted unexpected definitionVar from cljs.pprint"
            );
        }
        assert_eq!(
            run_node(&js),
            EXPECTED_PPRINT_SURFACE,
            "--optimize {level} output mismatch"
        );
    }
}

#[test]
fn clojure_pprint_fallback_and_private_surface() {
    let dir = TempDir::new("cljs-pprint-fallback");
    let root = dir.join("src");
    write_file(
        &root,
        "app/main.cljs",
        "(ns app.main (:require [clojure.pprint :as pp]))\n\
         (js/console.log (pp/cl-format nil \"~a\" 1) (clojure.pprint/cl-format nil \"~a\" 2) (cljs.pprint/cl-format nil \"~a\" 3))",
    );
    let (_, _, manifest) = analyze_namespace_entry_with_dependencies(
        "app.main",
        std::slice::from_ref(&root),
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap();
    let app = manifest.record(manifest.id("app.main").unwrap());
    let req = app
        .requires
        .iter()
        .find(|r| r.written == "clojure.pprint")
        .unwrap();
    assert_eq!(req.form, RequireForm::ClojureRewrite);
    let pprint_id = manifest.id("cljs.pprint").unwrap();
    assert_eq!(req.target, RequireTarget::Namespace(pprint_id));
    assert_eq!(
        manifest.record(pprint_id).source.origin,
        SourceOrigin::Bundled
    );
    assert!(manifest.id("clojure.pprint").is_none());

    // Upstream's record-defining helper macro must not be exported as cljs.pprint/deftype,
    // and private helpers not expanded into by public macros must stay private.
    for (bad_refer, expected_err) in [
        ("deftype", "cljs.pprint/deftype"),
        ("level-exceeded", "cljs.pprint/level-exceeded is private"),
        ("start-block", "cljs.pprint/start-block is private"),
        (
            "with-pretty-writer",
            "cljs.pprint/with-pretty-writer is private",
        ),
        ("getf", "cljs.pprint/getf is private"),
        ("setf", "cljs.pprint/setf is private"),
    ] {
        write_file(
            &root,
            "app/bad.cljs",
            &format!("(ns app.bad (:require [cljs.pprint :refer [{bad_refer}]]))"),
        );
        let err = stderr_on_err(light_cljs(
            dir.path(),
            &[
                "--compile",
                "--entry",
                "app.bad",
                "--source-root",
                root.to_str().unwrap(),
                "--no-cache",
            ],
            "",
        ));
        assert!(
            err.contains(expected_err),
            "refer {bad_refer}: expected {expected_err:?} in {err}"
        );
    }
}

fn stats_value(stderr: &str, key: &str) -> String {
    stderr
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key} ")))
        .unwrap_or_else(|| panic!("no {key} in {stderr}"))
        .to_owned()
}

#[test]
fn cached_build_and_macro_time_cl_format() {
    let dir = TempDir::new("cljs-pprint-cache");
    let root = dir.join("src");
    write_file(
        &root,
        "app/macros.cljs",
        "(ns app.macros (:require [cljs.pprint :as pp]))\n\
         (defmacro formatted-const [label n]\n\
           (pp/cl-format nil \"~a:~:r\" label n))",
    );
    write_file(
        &root,
        "app/main.cljs",
        "(ns app.main\n\
           (:require [app.macros :refer [formatted-const]]\n\
                     [cljs.pprint :as pp :refer [pprint]]))\n\
         (enable-console-print!)\n\
         (println (formatted-const \"rank\" 3))\n\
         (binding [pp/*print-right-margin* 15]\n\
           (pprint (sorted-map :a 1 :b [2 3 4])))",
    );
    let root_text = root.to_str().unwrap();
    // `pprint` appends a trailing newline before passing the string to
    // `*print-fn*`, and `enable-console-print!` routes `*print-fn*` to
    // `console.log`, which adds its own newline.
    let expected = "rank:third\n{:a 1,\n :b [2 3 4]}\n\n";
    let uncached = stdout(light_cljs(
        dir.path(),
        &[
            "--compile",
            "--entry",
            "app.main",
            "--source-root",
            root_text,
            "--no-cache",
        ],
        "",
    ));
    assert_eq!(run_node(&uncached), expected);

    let cache = dir.join("cache");
    let cached = || {
        let output = light_cljs(
            dir.path(),
            &[
                "--compile",
                "--entry",
                "app.main",
                "--source-root",
                root_text,
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
    // Cold cache builds and stores clojure.string, cljs.pprint, app.macros, app.main.
    let (first_js, first_installed) = cached();
    assert_eq!(first_installed, "0");
    assert_eq!(first_js, uncached);
    // Warm cache installs all four namespaces without re-running cljs.pprint's macros.
    let (second_js, second_installed) = cached();
    assert_eq!(second_installed, "4");
    assert_eq!(second_js, uncached);

    // Editing app.main re-expands formatted-const using cached cljs.pprint and app.macros.
    write_file(
        &root,
        "app/main.cljs",
        "(ns app.main\n\
           (:require [app.macros :refer [formatted-const]]\n\
                     [cljs.pprint :as pp :refer [pprint]]))\n\
         (enable-console-print!)\n\
         (println (formatted-const \"item\" 2))\n\
         (pprint {:ok true})",
    );
    let (third_js, third_installed) = cached();
    assert_eq!(third_installed, "3");
    assert_eq!(run_node(&third_js), "item:second\n{:ok true}\n\n");
}

#[test]
fn eval_and_repl_pprint_and_pp() {
    let dir = TempDir::new("cljs-pprint-eval-repl");
    let eval_code = "(require '[cljs.pprint :as pp :refer [pprint cl-format print-table with-pprint-dispatch code-dispatch]]\n\
                              '[clojure.pprint :as cpp])\n\
        [(with-out-str (binding [pp/*print-right-margin* 14] (pprint {:a 1 :b [2 3]})))\n\
         (cpp/cl-format nil \"~a is ~:r\" \"Bob\" 3)\n\
         (with-out-str (print-table [:k :v] [{:k :x :v 1}]))\n\
         (with-out-str (with-pprint-dispatch code-dispatch (pprint '(defn f [x] (inc x)))))]";
    assert_eq!(
        stdout(light_cljs(dir.path(), &["--eval", "--text", eval_code], "")),
        "[\"{:a 1,\\n :b [2 3]}\\n\" \"Bob is third\" \"\\n| :k | :v |\\n|----+----|\\n| :x |  1 |\\n\" \"(defn f [x] (inc x))\\n\"]\n"
    );

    let repl_input = "(require '[cljs.pprint :as pp :refer [pp cl-format]] '[clojure.pprint :as cpp])\n\
                      (sorted-map :alpha [1 2 3] :beta [4 5 6])\n\
                      (binding [pp/*print-right-margin* 20] (pp))\n\
                      (cpp/cl-format nil \"~@r\" 9)\n";
    let repl_out = light_cljs(dir.path(), &["--repl"], repl_input);
    assert!(
        repl_out.stderr.is_empty(),
        "unexpected REPL stderr:\n{}",
        String::from_utf8_lossy(&repl_out.stderr)
    );
    assert_eq!(
        stdout(repl_out),
        "nil\n\
         {:alpha [1 2 3], :beta [4 5 6]}\n\
         {:alpha [1 2 3],\n \
         :beta [4 5 6]}\n\
         nil\n\
         \"IX\"\n"
    );
}

#[test]
fn an_explicit_source_root_overrides_bundled_cljs_pprint() {
    let dir = TempDir::new("cljs-pprint-override");
    let root = dir.join("src");
    write_file(
        &root,
        "cljs/pprint.cljs",
        "(ns cljs.pprint)\n\
         (defn pprint [x] (println (str \"user-pprint:\" x)))\n\
         (defn cl-format [_ fmt & args] (str \"user-fmt:\" fmt \":\" (count args)))\n\
         (defmacro pp [] `(pprint *1))",
    );
    let expr = "[(pp/cl-format nil \"~a\" 1 2) (clojure.pprint/cl-format nil \"~d\" 3) (with-out-str (pp/pprint :ok))]";
    write_file(
        &root,
        "app/main.cljs",
        &format!(
            "(ns app.main (:require [cljs.pprint :as pp] [clojure.pprint]))\n\
             (js/console.log (pr-str {expr}))"
        ),
    );
    let root_text = root.to_str().unwrap();
    let expected = "[\"user-fmt:~a:2\" \"user-fmt:~d:1\" \"user-pprint::ok\\n\"]\n";
    let js = stdout(light_cljs(
        dir.path(),
        &[
            "--compile",
            "--entry",
            "app.main",
            "--source-root",
            root_text,
            "--no-cache",
        ],
        "",
    ));
    assert_eq!(run_node(&js), expected);

    for mode in ["--eval", "--repl"] {
        let input = format!("(require '[cljs.pprint :as pp] '[clojure.pprint])\n{expr}\n");
        let expected_mode = if mode == "--eval" {
            expected.to_owned()
        } else {
            format!("nil\n{expected}")
        };
        assert_eq!(
            stdout(light_cljs(
                dir.path(),
                &[mode, "--source-root", root_text],
                &input
            )),
            expected_mode,
            "{mode}"
        );
    }
}

/// light-cljs-1171: `pprint-set`, `pprint-array` and `pprint-pqueue` were
/// rewritten without `formatter-out`; verify their output matches the original
/// `cl-format` format strings across empty collections, wrapping margins,
/// `*print-length*` and `*print-level*` at every optimization level.
#[test]
fn pprint_set_array_and_pqueue_match_original_format_strings() {
    let dir = TempDir::new("cljs-pprint-inlined-printers");
    let root = dir.join("src");
    write_file(
        &root,
        "app/main.cljs",
        r#"(ns app.main
             (:require [cljs.pprint :as pp :refer [write cl-format]]))
           (defn fmt-via [fmt x opts]
             (let [f (fn [o]
                       (if (or (set? o) (array? o) (instance? PersistentQueue o))
                         (cl-format *out* fmt o)
                         (pp/simple-dispatch o)))]
               (apply write x :stream nil :dispatch f opts)))
           (defn fmt-fn [f x opts]
             (let [d (fn [o]
                       (if (or (set? o) (array? o) (instance? PersistentQueue o))
                         (f o)
                         (pp/simple-dispatch o)))]
               (apply write x :stream nil :dispatch d opts)))
           (def cases
             [["set" pp/pprint-set "~<#{~;~@{~w~^ ~:_~}~;}~:>"
               [#{} (sorted-set 1) (sorted-set 1 2 3)
                (sorted-set 100 200 300 400 500)
                (sorted-set [1 2] [3 4])]]
              ["array" @#'pp/pprint-array "~<[~;~@{~w~^, ~:_~}~;]~:>"
               [#js [] #js [1] #js [1 2 3] #js [100 200 300 400 500]]]
              ["pqueue" @#'pp/pprint-pqueue "~<<-(~;~@{~w~^ ~_~}~;)-<~:>"
               [cljs.core.PersistentQueue.EMPTY
                (into cljs.core.PersistentQueue.EMPTY [1])
                (into cljs.core.PersistentQueue.EMPTY [1 2 3])
                (into cljs.core.PersistentQueue.EMPTY [100 200 300 400 500])]]])
           (def opt-sets
             [[] [:right-margin 8] [:right-margin 12]
              [:length 0] [:length 1] [:length 2]
              [:level 0] [:level 1] [:level 2]])
           (let [mismatches
                 (for [[label f fmt vals] cases
                       v vals
                       opts opt-sets
                       :let [a (fmt-fn f v opts)
                             b (fmt-via fmt v opts)]
                       :when (not= a b)]
                   [label (pr-str v) opts a b])]
             (js/console.log (count mismatches)))"#,
    );
    let root_text = root.to_str().unwrap();
    for level in ["none", "prepare", "advanced"] {
        let js = stdout(light_cljs(
            dir.path(),
            &[
                "--compile",
                "--optimize",
                level,
                "--entry",
                "app.main",
                "--source-root",
                root_text,
                "--no-cache",
            ],
            "",
        ));
        assert_eq!(run_node(&js), "0\n", "at {level}");
    }
}
