(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(load-file (str (fs/file here "summarize.bb")))
(def repo "/Users/admin/src/light-cljs")
(def output (fs/file repo "benchmarks/results/compiler-2026-10-16/penpot-phase74"))
(fs/create-dirs output)
(def total-samples (reduce + (vals @totals)))
(def allocator-samples (reduce + (map val (filter #(str/includes? (key %) "(in libsystem_malloc.dylib)") @totals))))
(spit (fs/file here "categories.edn")
  (pr-str {:main-thread-exclusive total-samples :system-malloc-exclusive allocator-samples
           :malloc-fraction (/ (double allocator-samples) total-samples)
           :name-new-inclusive (into {} (filter #(str/includes? (key %) "13compiler_dataNtB5_4Name3new") @inclusive))}))
(doseq [pattern ["*.bb" "*.log" "*.edn" "*.txt"] file (fs/glob here pattern)
        :when (not= "sample.txt" (fs/file-name file))]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(with-open [input (io/input-stream (fs/file here "sample.txt"))
            compressed (java.util.zip.GZIPOutputStream. (io/output-stream (fs/file output "sample.txt.gz")))]
  (io/copy input compressed))
(p/shell "cmp" (str (fs/file here "main.js")) "/private/tmp/light-cljs-phase42/production/main-none.js")
(spit (fs/file output "source-hashes.txt")
  (:out (p/shell {:out :string} "shasum" "-a" "256"
          "/private/tmp/light-cljs-phase71/light-cljs-direct-roots"
          (str (fs/file here "main.js")) (str (fs/file here "sample.txt")))))
(println (slurp (fs/file here "categories.edn")))
