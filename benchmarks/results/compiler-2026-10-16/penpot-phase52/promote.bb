(require '[babashka.fs :as fs] '[babashka.process :as p]
         '[clojure.java.io :as io] '[clojure.edn :as edn] '[clojure.string :as str])
(def here (fs/parent *file*))
(def repo (fs/file "/Users/admin/src/light-cljs"))
(def project (fs/file here "project"))
(def output (fs/file here "production"))
(assert (= [1401 0 106] (:tests (edn/read-string (slurp (fs/file here "validation.edn"))))))
(assert (= "0" (str/trim (slurp (fs/file here "linux-results/exit-code.txt")))))
(def changed ["Cargo.toml" "Cargo.lock" "THIRD_PARTY_NOTICES.md" "src/main.rs" "src/emitter.rs"])
(def scratch (fs/file here "patch-inputs"))
(doseq [[side root] [["before" repo] ["after" project]] file changed]
  (let [target (fs/file scratch side file)]
    (fs/create-dirs (fs/parent target))
    (fs/copy (fs/file root file) target {:replace-existing true})))
(def delta (p/shell {:dir (str scratch) :out :string :err :string :continue true}
             "git" "diff" "--no-index" "--" "before" "after"))
(assert (= 1 (:exit delta)))
(def patch (fs/file here "quickjs-allocator-quotes.patch"))
(spit patch (-> (:out delta) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/")))
(p/shell {:dir (str repo)} "git" "apply" "--check" (str patch))
(p/shell {:dir (str repo)} "git" "apply" (str patch))
;; All production compiler/runtime/test/vendor inputs must match the full-suite candidate.
(doseq [directory ["src" "tests" "vendor" "benches"]]
  (p/shell "diff" "-qr" (str (fs/file project directory)) (str (fs/file repo directory))))
(doseq [file changed] (p/shell "cmp" (str (fs/file project file)) (str (fs/file repo file))))
(fs/create-dirs output)
(spit (fs/file output "candidate-identity.txt") "All src, tests, vendor and benches files and changed manifests/notices match the candidate that passed 1401 tests, strict Clippy and three Node workflows.\n")
(p/shell {:dir (str repo)} "cargo" "fmt" "--check")
(p/shell {:dir (str repo)} "git" "diff" "--check")
(with-open [out (io/output-stream (fs/file output "build.log"))]
  (p/shell {:dir (str repo) :out out :err :out} "cargo" "build" "--release"))
(fs/copy (fs/file repo "target/release/light-cljs") (fs/file output "light-cljs") {:replace-existing true})
(fs/copy (fs/file repo "src/name_index.rs") (fs/file output "name_index.rs") {:replace-existing true})
(spit (fs/file output "implementation.patch") (:out (p/shell {:dir (str repo) :out :string} "git" "diff" "--binary")))
(p/shell {:dir (str repo)} "bb" "benchmarks/penpot.bb" (str output))
