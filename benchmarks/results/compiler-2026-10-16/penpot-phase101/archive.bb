(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def output (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase101"))
(fs/create-dirs output)
(def files ["src/analyzer.rs" "src/js_syntax.rs" "src/emitter.rs" "src/emitter/chunked.rs"
            "tests/compiler/macros.rs" "tests/compiler/optimization.rs" "docs/compiler.md"])
(def inputs (fs/file here "archive-patch-inputs"))
(doseq [[side root] [["before" "/private/tmp/light-cljs-phase100/project"]
                    ["after" (fs/file here "project")]] file files]
  (fs/create-dirs (fs/parent (fs/file inputs side file)))
  (fs/copy (fs/file root file) (fs/file inputs side file) {:replace-existing true}))
(let [result (p/shell {:dir (str inputs) :out :string :continue true} "git" "diff" "--no-index" "--" "before" "after")]
  (assert (= 1 (:exit result)))
  (spit (fs/file output "changes.patch")
    (-> (:out result) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/"))))
(doseq [pattern ["*.bb" "*.log" "*.time" "*.edn" "*.patch"] file (fs/glob here pattern)]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(spit (fs/file output "source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
    (concat (map #(str (fs/file here %)) ["light-cljs-lexical-js-vars" "main-none.js"])
            (map #(str (fs/file here "project" %)) files)))))
