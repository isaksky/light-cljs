(require '[babashka.fs :as fs] '[babashka.process :as p]
         '[clojure.string :as str] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def repo "/Users/admin/src/light-cljs")
(def output (fs/file repo "benchmarks/results/compiler-2026-10-16/penpot-phase45"))
(fs/create-dirs output)
(doseq [file (concat (map #(fs/file here %) ["run.bb" "run-bench.bb" "summarize.bb" "archive.bb"
                                           "result.edn" "sample.log" "pid.txt" "leaf-samples.txt"
                                           "main-thread-summary.txt"])
                    (mapcat #(fs/glob here %) ["*.time" "*.log" "*.command.edn"]))]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(with-open [input (io/input-stream (fs/file here "sample.txt"))
            compressed (java.util.zip.GZIPOutputStream. (io/output-stream (fs/file output "sample.txt.gz")))]
  (io/copy input compressed))
(doseq [job ["main" "main-counts"]]
  (p/shell "cmp" (str (fs/file here (str job ".js")))
    "/private/tmp/light-cljs-phase42/production/main-none.js"))
(spit (fs/file output "equivalence.txt")
  "Both sampled v12 output and the cached-count trial match phase42 main-none byte for byte. Explicit ESM syntax rejection is a known baseline defect documented in phase44.\n")
(def scratch (fs/file here "patch-inputs"))
(doseq [file ["src/analyzer.rs" "src/compilation_session.rs"]
        [side root] [["before" "/private/tmp/light-cljs-phase44/project"]
                     ["after" (fs/file here "project")]]]
  (let [path (fs/file scratch side file)]
    (fs/create-dirs (fs/parent path))
    (fs/copy (fs/file root file) path {:replace-existing true})))
(def delta
  (let [result (p/shell {:dir (str scratch) :out :string :err :string :continue true}
                 "git" "diff" "--no-index" "--" "before" "after")]
    (assert (= 1 (:exit result)))
    (-> (:out result) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/"))))
(spit (fs/file output "namespace-counts.patch") delta)
(spit (fs/file output "source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
          (map str [(fs/file here "light-cljs-counts")
                    "/private/tmp/light-cljs-phase44/light-cljs-v12"
                    (fs/file here "main.js") (fs/file here "main-counts.js")
                    (fs/file here "sample.txt")
                    (fs/file here "project/src/analyzer.rs")
                    (fs/file here "project/src/compilation_session.rs")]))))
(println "Archived native sample and namespace-count delta")
