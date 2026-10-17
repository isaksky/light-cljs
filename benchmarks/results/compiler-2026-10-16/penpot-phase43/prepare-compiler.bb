(require '[babashka.fs :as fs] '[clojure.string :as str])
(def root "/Users/admin/src/light-cljs")
(def output "/private/tmp/light-cljs-phase44/project")
(assert (not (fs/exists? output)))
(fs/create-dirs output)
(doseq [directory ["src" "tests" "vendor" "benches"] :when (fs/exists? (fs/file root directory))]
  (fs/copy-tree (fs/file root directory) (fs/file output directory)))
(doseq [file ["Cargo.toml" "Cargo.lock" "build.rs" "AGENTS.md"]]
  (fs/copy (fs/file root file) (fs/file output file)))
(defn edit [file before after]
  (let [path (fs/file output file) text (slurp path)]
    (assert (str/includes? text before) file)
    (spit path (str/replace-first text before after))))
(edit "Cargo.toml" "[dependencies]" "[dependencies]\nv8 = \"=152.2.0\"")
(edit "src/lib.rs" "pub mod evaluator;" "pub mod evaluator;\nmod v8_evaluator;\nmod v8_bridge;")
(edit "src/evaluator.rs" "static NEXT_SESSION:" "pub(crate) static NEXT_SESSION:")
(edit "src/evaluator.rs" "    session: u64,\n    index: u64," "    pub(crate) session: u64,\n    pub(crate) index: u64,")
(edit "src/form_bridge.rs" "fn exact(a: &Datum, b: &Datum)" "pub(crate) fn exact(a: &Datum, b: &Datum)")
;; Keep the exact evaluator service implementation, including shared macro
;; inputs and error diagnostics, in the isolated backend.
(let [text (slurp (fs/file root "src/evaluator.rs"))
      start (str/index-of text "            const __lightEvaluator = (() => {{")
      end (str/index-of text "        \"#)))?;" start)
      body (subs text start end)]
  (spit (fs/file output "src/v8_services.js")
    (-> body
        (str/replace "{{" "{")
        (str/replace "}}" "}")
        (str/replace "{RUNTIME_SOURCE}" "__LIGHT_RUNTIME_SOURCE__")
        (str/replace "{FORM_BRIDGE_VERSION}" "5"))))
(println output)
