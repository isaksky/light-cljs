(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def out (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase90"))
(fs/create-dirs out)
(doseq [file (concat (mapcat #(fs/glob here %) ["*.bb" "*.log" "*.edn"])
                    (map #(fs/file here %) ["main-thread-summary.txt" "inclusive-samples.txt"]))]
  (fs/copy file (fs/file out (fs/file-name file)) {:replace-existing true}))
(with-open [compressed (io/output-stream (fs/file out "sample.txt.gz"))]
  (p/shell {:out compressed} "gzip" "-c" (str (fs/file here "sample.txt"))))
(p/shell "cmp" (str (fs/file here "main.js")) "/private/tmp/light-cljs-phase42/production/main-none.js")
(spit (fs/file out "source-hashes.txt") (:out (p/shell {:out :string} "shasum" "-a" "256"
  "/private/tmp/light-cljs-phase87/production/light-cljs"
  (str (fs/file here "main.js")) (str (fs/file here "sample.txt")))))
