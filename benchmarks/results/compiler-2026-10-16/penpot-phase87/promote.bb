(require '[babashka.fs :as fs] '[babashka.process :as p]
         '[clojure.java.io :as io] '[clojure.edn :as edn] '[clojure.string :as str])
(def here (fs/parent *file*))
(def repo (fs/file "/Users/admin/src/light-cljs"))
(def project (fs/file here "project"))
(def baseline (fs/file "/private/tmp/light-cljs-phase72/project"))
(def output (fs/file here "production"))
(assert (= [1401 0 106] (:tests (edn/read-string (slurp (fs/file here "validation.edn"))))))
(doseq [directory ["src" "tests" "vendor" "benches"]]
  (p/shell "diff" "-qr" (str (fs/file baseline directory)) (str (fs/file repo directory))))
(doseq [file ["Cargo.toml" "Cargo.lock" "build.rs"]]
  (p/shell "cmp" (str (fs/file baseline file)) (str (fs/file repo file))))
(def changed ["src/analyzer.rs" "src/arena.rs" "src/compilation_session.rs"
              "src/compiler_reader.rs" "src/dev_build.rs" "src/eval_compiler.rs"
              "src/layered.rs" "src/macro_host.rs" "docs/compiler.md" "docs/evaluator.md"])
(def reader-note "\nThe analyzer's private reader view resolves declared binding spellings only\nwhen syntax quotation reads them, using the current definition snapshot's name\nindex. It does not rebuild binding-kind tables by scanning earlier expressions.\nThe public `NamespaceView` still exposes its complete binding introspection.\n")
(assert (= (slurp (fs/file repo "docs/compiler.md"))
           (str/replace (slurp (fs/file project "docs/compiler.md")) reader-note "")))
(def storage-note "the assembled program grows linearly with its definitions. Namespace names use\nthe same frozen arena, and namespace entry maps share bounded write layers.\nMacro metadata frames share both their base and their pending write layer until\nmutation. Completed units release their boxed definition snapshot and macro host;\ntheir retained program tables shed spare capacity without changing IDs or values.\nCompile-time emission uses the ordinary emitter with")
(assert (= (slurp (fs/file repo "docs/evaluator.md"))
           (str/replace (slurp (fs/file project "docs/evaluator.md")) storage-note
                        "the assembled program grows linearly with its definitions. Compile-time emission uses the ordinary emitter with")))
(def scratch (fs/file here "promotion-patch-inputs"))
(doseq [[side root] [["before" repo] ["after" project]] file changed]
  (let [target (fs/file scratch side file)]
    (fs/create-dirs (fs/parent target))
    (fs/copy (fs/file root file) target {:replace-existing true})))
(def delta (p/shell {:dir (str scratch) :out :string :err :string :continue true}
                   "git" "diff" "--no-index" "--" "before" "after"))
(assert (= 1 (:exit delta)))
(def patch (fs/file here "shared-storage.patch"))
(spit patch (-> (:out delta) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/")))
(p/shell {:dir (str repo)} "git" "apply" "--check" (str patch))
(p/shell {:dir (str repo)} "git" "apply" (str patch))
(doseq [directory ["src" "tests" "vendor" "benches"]]
  (p/shell "diff" "-qr" (str (fs/file project directory)) (str (fs/file repo directory))))
(doseq [file (concat ["Cargo.toml" "Cargo.lock" "build.rs"] (filter #(str/starts-with? % "docs/") changed))]
  (p/shell "cmp" (str (fs/file project file)) (str (fs/file repo file))))
(fs/create-dirs output)
(spit (fs/file output "candidate-identity.txt") "All source, tests, vendor, benches, manifests, build.rs and changed documentation match the candidate that passed 1401 tests, strict Clippy, formatting and three Node workflows.\n")
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
