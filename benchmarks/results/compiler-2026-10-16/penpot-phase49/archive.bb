(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def output (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase49"))
(fs/create-dirs output)
(doseq [pattern ["*.bb" "*.log" "*.time" "*.command.edn"] file (fs/glob here pattern)]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(def scratch (fs/file here "patch-inputs"))
(doseq [[side root] [["before" "/private/tmp/light-cljs-phase47/project"] ["after" (fs/file here "project")]]]
  (fs/create-dirs (fs/file scratch side "src"))
  (fs/copy (fs/file root "src/emitter.rs") (fs/file scratch side "src/emitter.rs") {:replace-existing true}))
(def delta (p/shell {:dir (str scratch) :out :string :err :string :continue true}
             "git" "diff" "--no-index" "--" "before" "after"))
(assert (= 1 (:exit delta)))
(spit (fs/file output "scan.patch")
  (-> (:out delta) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/")))
(spit (fs/file output "source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
          (map #(str (fs/file here %)) ["light-cljs-scan" "main-scan.js" "project/src/emitter.rs"]))))
(println "Archived scan prefilter trial.")
