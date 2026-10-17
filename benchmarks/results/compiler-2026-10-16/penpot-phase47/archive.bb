(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def repo "/Users/admin/src/light-cljs")
(def output (fs/file repo "benchmarks/results/compiler-2026-10-16/penpot-phase47"))
(fs/create-dirs output)
(doseq [pattern ["*.bb" "*.log" "*.time" "*.command.edn"] file (fs/glob here pattern)]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(def scratch (fs/file here "patch-inputs"))
(doseq [file ["src/analyzer.rs" "tests/compiler/macros.rs"]
        [side root] [["before" "/private/tmp/light-cljs-phase46/project"] ["after" (fs/file here "project")]]]
  (fs/create-dirs (fs/parent (fs/file scratch side file)))
  (fs/copy (fs/file root file) (fs/file scratch side file) {:replace-existing true}))
(def delta (p/shell {:dir (str scratch) :out :string :err :string :continue true}
             "git" "diff" "--no-index" "--" "before" "after"))
(assert (= 1 (:exit delta)))
(spit (fs/file output "reader-bindings.patch")
  (-> (:out delta) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/")))
(p/shell "cmp" (str (fs/file here "main-bindings.js")) "/private/tmp/light-cljs-phase42/production/main-none.js")
(spit (fs/file output "source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
          (map #(str (fs/file here %)) ["light-cljs-bindings" "main-bindings.js" "project/src/analyzer.rs" "project/tests/compiler/macros.rs"]))))
(println "Archived relevant-namespace reader binding probe.")
