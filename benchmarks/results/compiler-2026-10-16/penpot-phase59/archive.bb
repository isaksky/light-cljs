(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def output (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase59"))
(fs/create-dirs output)
(doseq [pattern ["*.bb" "*.log" "*.edn"] file (fs/glob here pattern)]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(def scratch (fs/file here "patch-inputs"))
(doseq [[side root] [["before" "/private/tmp/light-cljs-phase58/project"] ["after" (fs/file here "project")]]
        file ["src/lib.rs" "src/v8_allocator.rs" "src/v8_evaluator.rs"]
        :when (fs/exists? (fs/file root file))]
  (fs/create-dirs (fs/parent (fs/file scratch side file)))
  (fs/copy (fs/file root file) (fs/file scratch side file) {:replace-existing true}))
(def delta (p/shell {:dir (str scratch) :out :string :err :string :continue true}
             "git" "diff" "--no-index" "--" "before" "after"))
(assert (= 1 (:exit delta)))
(spit (fs/file output "buffer-budget-incomplete.patch")
  (-> (:out delta) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/")
      (str/replace "a/after/" "a/")))
(spit (fs/file output "source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
          (map #(str (fs/file here %)) ["project/src/lib.rs" "project/src/v8_allocator.rs" "project/src/v8_evaluator.rs"]))))
(println "Archived incomplete buffer guard and the failing growable SharedArrayBuffer regression. Not validated or promoted.")
