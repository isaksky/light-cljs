(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def archive (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase112"))
(doseq [label ["render" "main-none" "main-advanced"]]
  (p/shell "cmp" (str (fs/file here "production-bench" (str label ".js")))
    (str (fs/file here "bench" (str label ".js"))))
  (with-open [in (io/input-stream (fs/file here "production-bench" (str label ".js")))
              out (io/output-stream (fs/file here (str "production-" label "-syntax.log")))]
    (p/shell {:in in :out out :err :out} "node" "--check" "--input-type=module")))
(fs/create-dirs (fs/file archive "production-bench"))
(doseq [pattern ["*.edn" "*.txt"] file (fs/glob (fs/file here "production-bench") pattern)]
  (fs/copy file (fs/file archive "production-bench" (fs/file-name file)) {:replace-existing true}))
(doseq [file (fs/glob here "production-*-syntax.log")]
  (fs/copy file (fs/file archive (fs/file-name file)) {:replace-existing true}))
(fs/copy *file* (fs/file archive "confirm-production.bb") {:replace-existing true})
(spit (fs/file archive "production-artifact-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256" "/Users/admin/src/light-cljs/target/release/light-cljs"
    (map #(str (fs/file here "production-bench" (str % ".js"))) ["render" "main-none" "main-advanced"]))))
(p/shell "bb" (str (fs/file here "verify-promoted.bb")))
(println "Production outputs match the validated candidate byte-for-byte and pass ESM syntax checks")
