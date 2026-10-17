(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn]
         '[clojure.string :as str] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def repo (fs/file "/Users/admin/src/light-cljs"))
(def project (fs/file here "project"))
(def baseline (fs/file "/private/tmp/light-cljs-phase87/project"))
(def output (fs/file here "production"))
(assert (= [1403 0 106] (:tests (edn/read-string (slurp (fs/file here "validation.edn"))))))
(doseq [directory ["src" "tests" "vendor" "benches"]]
  (p/shell "diff" "-qr" (str (fs/file baseline directory)) (str (fs/file repo directory))))
(doseq [file ["Cargo.toml" "Cargo.lock" "build.rs" "docs/evaluator.md"]]
  (p/shell "cmp" (str (fs/file baseline file)) (str (fs/file repo file))))
(def files ["src/analyzer.rs" "src/emitter.rs" "src/compilation_session.rs" "src/compiler_bootstrap.rs" "src/evaluator.rs"])
(doseq [file files]
  (fs/copy (fs/file project file) (fs/file repo file) {:replace-existing true}))
(def documentation (str/replace (slurp (fs/file repo "docs/evaluator.md"))
  "mutation. Completed units release their boxed definition snapshot and macro host;\ntheir retained program tables shed spare capacity without changing IDs or values.\nCompile-time emission uses the ordinary emitter with\nroot access hooks and never invokes an application optimizer. Ready root reads\nstay in JS. Completed or failed roots release their initializer closures and\nretain their value or error; Rust records that emission occurred without keeping\na duplicate source string. Unrequested initializers drain in source order; standalone runtime"
  "mutation. Completed units release their boxed definition snapshot and macro host.\nAfter application assembly consumes a unit's IR, the session prepares its source\nand retains only that source and immutable reader-registration tables; it holds\nthe analyzed program weakly so an active reentrant owner can finish using it.\nPreparing source executes no user code. Namespace registration and initialization\nremain lazy, and compilation-use emission counts still record actual engine demand.\nCompile-time emission uses the ordinary emitter with root access hooks and never\ninvokes an application optimizer. Ready root reads stay in JS. Completed or failed\nroots release their initializer closures and retain their value or error. Prepared\nsource remains available for compilation reentry. Unrequested initializers drain\nin source order; standalone runtime"))
(assert (not= documentation (slurp (fs/file repo "docs/evaluator.md"))))
(spit (fs/file repo "docs/evaluator.md") documentation)
(spit (fs/file project "docs/evaluator.md") documentation)
(doseq [directory ["src" "tests" "vendor" "benches"]]
  (p/shell "diff" "-qr" (str (fs/file project directory)) (str (fs/file repo directory))))
(doseq [file ["Cargo.toml" "Cargo.lock" "build.rs" "docs/evaluator.md"]]
  (p/shell "cmp" (str (fs/file project file)) (str (fs/file repo file))))
(p/shell {:dir (str repo)} "cargo" "fmt" "--check")
(p/shell {:dir (str repo)} "git" "diff" "--check")
(fs/create-dirs output)
(spit (fs/file output "candidate-identity.txt")
  "All compiler, runtime, test, vendor, bench and build inputs match the candidate that passed 1403 tests, strict Clippy, formatting and three Node workflows. The evaluator documentation was updated after validation.\n")
(doseq [file ["src/lib.rs" "src/main.rs" "build.rs" "vendor/rquickjs-sys/build.rs" "vendor/rquickjs-sys/quickjs/quickjs.c"]]
  (p/shell "touch" (str (fs/file repo file))))
(with-open [out (io/output-stream (fs/file output "build.log"))]
  (p/shell {:dir (str repo) :out out :err :out} "cargo" "build" "--release"))
(fs/copy (fs/file repo "target/release/light-cljs") (fs/file output "light-cljs") {:replace-existing true})
(fs/copy (fs/file repo "src/name_index.rs") (fs/file output "name_index.rs") {:replace-existing true})
(spit (fs/file output "implementation.patch") (:out (p/shell {:dir (str repo) :out :string} "git" "diff" "--binary")))
(p/shell {:dir (str repo)} "bb" "benchmarks/penpot.bb" (str output))
(doseq [label ["render" "main-none" "main-advanced"]]
  (p/shell "cmp" (str (fs/file output (str label ".js"))) (str "/private/tmp/light-cljs-phase42/production/" label ".js")))
