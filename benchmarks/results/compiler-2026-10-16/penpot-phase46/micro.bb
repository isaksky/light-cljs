(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def source-file (str (fs/file here "names.rs")))
(with-open [out (io/output-stream (fs/file here "micro.log"))]
  (p/shell {:out out :err :out} "rustc" "-O" source-file "-o" (str (fs/file here "names")))
  (doseq [mode ["old" "candidate" "old" "candidate"]]
    (p/shell {:out out :err :out} (str (fs/file here "names")) mode))
  (p/shell {:out out :err :out} "rustc" "-O" "--cfg" "count_allocations" source-file
    "-o" (str (fs/file here "names-count")))
  (doseq [mode ["old" "candidate"]]
    (p/shell {:out out :err :out} (str (fs/file here "names-count")) mode)))
(println (slurp (fs/file here "micro.log")))
