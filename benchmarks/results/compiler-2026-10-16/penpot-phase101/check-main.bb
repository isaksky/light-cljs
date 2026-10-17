(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def output (fs/file here "main-none.js"))
(with-open [in (io/input-stream output)
            out (io/output-stream (fs/file here "main-syntax.log"))]
  (p/shell {:in in :out out :err :out} "node" "--check" "--input-type=module"))
(let [result (p/shell {:out :string :err :string :continue true}
               "diff" "-u" "/private/tmp/light-cljs-phase100/production/main-none.js" (str output))]
  (assert (= 1 (:exit result)))
  (spit (fs/file here "main-output.patch") (:out result)))
(println "Main output passes explicit ESM syntax; changes saved for review")
