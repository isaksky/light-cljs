(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def output (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase55"))
(fs/create-dirs output)
(doseq [pattern ["*.bb" "*.log" "*.time" "*.edn"] file (fs/glob here pattern)]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(def scratch (fs/file here "patch-inputs"))
(doseq [[side root] [["before" "/private/tmp/light-cljs-phase54/project"] ["after" (fs/file here "project")]]
        file ["src/analyzer.rs" "src/macro_host.rs" "src/compilation_session.rs" "src/eval_compiler.rs"]]
  (fs/create-dirs (fs/parent (fs/file scratch side file)))
  (fs/copy (fs/file root file) (fs/file scratch side file) {:replace-existing true}))
(def delta (p/shell {:dir (str scratch) :out :string :err :string :continue true}
             "git" "diff" "--no-index" "--" "before" "after"))
(assert (= 1 (:exit delta)))
(spit (fs/file output "namespace-ids.patch")
  (-> (:out delta) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/")))
(spit (fs/file output "source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
          (map #(str (fs/file here %)) ["light-cljs-namespace-ids" "main-namespace-ids.js" "stale-target-light-cljs-namespace-ids"
                                      "project/src/analyzer.rs" "project/src/macro_host.rs" "project/src/compilation_session.rs" "project/src/eval_compiler.rs"]))))
(println "Archived namespace member IDs trial and invalid stale-artifact measurement.")
