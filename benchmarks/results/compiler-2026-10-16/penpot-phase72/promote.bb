(require '[babashka.fs :as fs] '[babashka.process :as p]
         '[clojure.java.io :as io] '[clojure.edn :as edn] '[clojure.string :as str])
(def here (fs/parent *file*))
(def repo (fs/file "/Users/admin/src/light-cljs"))
(def project (fs/file here "project"))
(def output (fs/file here "production"))
(assert (= [1400 0 106] (:tests (edn/read-string (slurp (fs/file here "validation.edn"))))))
(def changed ["src/emitter.rs" "src/terse.rs" "tests/evaluation/evaluator.rs"])
(def scratch (fs/file here "patch-inputs"))
(doseq [[side root] [["before" "/private/tmp/light-cljs-phase52/project"] ["after" project]] file changed]
  (let [target (fs/file scratch side file)]
    (fs/create-dirs (fs/parent target))
    (fs/copy (fs/file root file) target {:replace-existing true})))
(def delta (p/shell {:dir (str scratch) :out :string :err :string :continue true}
             "git" "diff" "--no-index" "--" "before" "after"))
(assert (= 1 (:exit delta)))
(def patch (fs/file here "unit-hygiene.patch"))
(spit patch (-> (:out delta) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/")))
(p/shell {:dir (str repo)} "git" "apply" "--check" (str patch))
(p/shell {:dir (str repo)} "git" "apply" (str patch))
(doseq [directory ["src" "tests" "vendor" "benches"]]
  (p/shell "diff" "-qr" (str (fs/file project directory)) (str (fs/file repo directory))))
(doseq [file ["Cargo.toml" "Cargo.lock" "build.rs"]]
  (p/shell "cmp" (str (fs/file project file)) (str (fs/file repo file))))
(fs/create-dirs output)
(spit (fs/file output "candidate-identity.txt") "All src, tests, vendor, benches, manifests and build.rs match the candidate that passed 1400 tests, strict Clippy, formatting and three Node workflows.\n")
(p/shell {:dir (str repo)} "cargo" "fmt" "--check")
(p/shell {:dir (str repo)} "git" "diff" "--check")
(p/shell "touch" (str (fs/file repo "src/lib.rs")) (str (fs/file repo "src/main.rs")) (str (fs/file repo "build.rs")))
(with-open [out (io/output-stream (fs/file output "build.log"))]
  (p/shell {:dir (str repo) :out out :err :out} "cargo" "build" "--release"))
(fs/copy (fs/file repo "target/release/light-cljs") (fs/file output "light-cljs") {:replace-existing true})
(fs/copy (fs/file repo "src/name_index.rs") (fs/file output "name_index.rs") {:replace-existing true})
(spit (fs/file output "implementation.patch") (:out (p/shell {:dir (str repo) :out :string} "git" "diff" "--binary")))
(p/shell {:dir (str repo)} "bb" "benchmarks/penpot.bb" (str output))
(doseq [label ["render" "main-none" "main-advanced"]]
  (p/shell "cmp" (str (fs/file output (str label ".js"))) (str "/private/tmp/light-cljs-phase42/production/" label ".js")))
(println "Production outputs byte-identical to phase42; known main ESM defect unchanged.")
