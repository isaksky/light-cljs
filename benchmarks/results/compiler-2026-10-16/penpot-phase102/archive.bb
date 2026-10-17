(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn])
(def here (fs/parent *file*))
(def output (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase102"))
(assert (= [1421 0 106] (:tests (edn/read-string (slurp (fs/file here "validation.edn"))))))
(fs/create-dirs output)
(doseq [pattern ["*.bb" "*.log" "*.time" "*.edn" "*.patch"] file (fs/glob here pattern)]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(doseq [label ["render" "main-none" "main-advanced"]]
  (p/shell "cmp" (str (fs/file here (str label ".js")))
    (str "/private/tmp/light-cljs-phase101/production/" label ".js")))
(def files ["src/analyzer.rs" "src/js_syntax.rs" "src/emitter.rs" "src/emitter/chunked.rs"
            "tests/compiler/macros.rs" "tests/compiler/optimization.rs" "docs/compiler.md"
            "src/v8_evaluator.rs" "src/v8_watchdog.rs" "src/v8_stack.rs"])
(spit (fs/file output "source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
    (concat (map #(str (fs/file here %)) ["light-cljs-v8-lexical-js-vars" "render.js"
                                         "main-none.js" "main-advanced.js"])
            (map #(str (fs/file here "project" %)) files)))))
(println "V8 candidate evidence archived; all three outputs match corrected production")
