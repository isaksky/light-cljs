(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn]
         '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def repo (fs/file "/Users/admin/src/light-cljs"))
(def project (fs/file here "project"))
(def baseline (fs/file "/private/tmp/light-cljs-phase100/project"))
(def output (fs/file here "production"))
(assert (= [1406 0 106] (:tests (edn/read-string (slurp (fs/file here "validation.edn"))))))
(assert (= 74 (:var-destructuring (edn/read-string (slurp (fs/file here "output-audit.edn"))))))
(doseq [directory ["src" "tests" "vendor" "benches"]]
  (p/shell "diff" "-qr" (str (fs/file baseline directory)) (str (fs/file repo directory))))
(doseq [file ["Cargo.toml" "Cargo.lock" "build.rs" "docs/compiler.md" "docs/evaluator.md"]]
  (p/shell "cmp" (str (fs/file baseline file)) (str (fs/file repo file))))
(def files ["src/analyzer.rs" "src/js_syntax.rs" "src/emitter.rs" "src/emitter/chunked.rs"
            "tests/compiler/macros.rs" "tests/compiler/optimization.rs" "docs/compiler.md"])
(doseq [file files]
  (fs/copy (fs/file project file) (fs/file repo file) {:replace-existing true}))
(doseq [directory ["src" "tests" "vendor" "benches"]]
  (p/shell "diff" "-qr" (str (fs/file project directory)) (str (fs/file repo directory))))
(doseq [file ["Cargo.toml" "Cargo.lock" "build.rs" "docs/compiler.md" "docs/evaluator.md"]]
  (p/shell "cmp" (str (fs/file project file)) (str (fs/file repo file))))
(p/shell {:dir (str repo)} "cargo" "fmt" "--check")
(p/shell {:dir (str repo)} "git" "diff" "--check")
(fs/create-dirs output)
(spit (fs/file output "candidate-identity.txt")
  "All source, test, vendor, bench and build inputs match the candidate that passed 1406 tests, strict Clippy, formatting and three Node workflows. Compiler documentation was updated after validation.\n")
(doseq [file ["src/lib.rs" "src/main.rs" "build.rs" "vendor/rquickjs-sys/build.rs" "vendor/rquickjs-sys/quickjs/quickjs.c"]]
  (p/shell "touch" (str (fs/file repo file))))
(with-open [out (io/output-stream (fs/file output "build.log"))]
  (p/shell {:dir (str repo) :out out :err :out} "cargo" "build" "--release"))
(fs/copy (fs/file repo "target/release/light-cljs") (fs/file output "light-cljs") {:replace-existing true})
(fs/copy (fs/file repo "src/name_index.rs") (fs/file output "name_index.rs") {:replace-existing true})
(spit (fs/file output "implementation.patch") (:out (p/shell {:dir (str repo) :out :string} "git" "diff" "--binary")))
(p/shell {:dir (str repo)} "bb" "benchmarks/penpot.bb" (str output))
(p/shell "cmp" (str (fs/file output "main-none.js")) (str (fs/file here "main-none.js")))
(doseq [label ["render" "main-none" "main-advanced"]]
  (with-open [in (io/input-stream (fs/file output (str label ".js")))
              out (io/output-stream (fs/file output (str label "-syntax.log")))]
    (p/shell {:in in :out out :err :out} "node" "--check" "--input-type=module")))
(println "Production benchmark completed; all three outputs pass explicit ESM syntax")
