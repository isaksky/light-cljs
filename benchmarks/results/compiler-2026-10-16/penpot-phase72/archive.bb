(require '[babashka.fs :as fs] '[babashka.process :as p])
(def here (fs/parent *file*))
(def out (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase72"))
(fs/create-dirs out)
(doseq [pattern ["*.bb" "*.log" "*.time" "*.edn" "*.patch"] file (fs/glob here pattern)]
  (fs/copy file (fs/file out (fs/file-name file)) {:replace-existing true}))
(p/shell "cmp" (str (fs/file here "main-none.js")) "/private/tmp/light-cljs-phase42/production/main-none.js")
(spit (fs/file out "source-hashes.txt") (:out (apply p/shell {:out :string} "shasum" "-a" "256"
  (map #(str (fs/file here %)) ["light-cljs-quickjs-unit-hygiene" "main-none.js" "project/Cargo.toml" "project/Cargo.lock"
    "project/src/emitter.rs" "project/src/terse.rs" "project/tests/evaluation/evaluator.rs"]))))
