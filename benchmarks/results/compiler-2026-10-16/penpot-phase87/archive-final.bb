(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn])
(def here (fs/parent *file*))
(def out (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase87"))
(assert (= [1401 0 106] (:tests (edn/read-string (slurp (fs/file here "validation.edn"))))))
(doseq [pattern ["*.bb" "*.log" "*.edn" "*.patch"] file (fs/glob here pattern)]
  (fs/copy file (fs/file out (fs/file-name file)) {:replace-existing true}))
(fs/copy (fs/file here "production/candidate-identity.txt") (fs/file out "production/candidate-identity.txt") {:replace-existing true})
(spit (fs/file out "validated-source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
    (map #(str (fs/file here %))
      ["production/light-cljs" "production/main-none.js" "production/main-advanced.js" "production/render.js"
       "tests.log" "project/Cargo.toml" "project/Cargo.lock" "project/build.rs"
       "project/src/analyzer.rs" "project/src/arena.rs" "project/src/compilation_session.rs"
       "project/src/compiler_reader.rs" "project/src/dev_build.rs" "project/src/eval_compiler.rs"
       "project/src/layered.rs" "project/src/macro_host.rs" "project/docs/compiler.md" "project/docs/evaluator.md"]))))
