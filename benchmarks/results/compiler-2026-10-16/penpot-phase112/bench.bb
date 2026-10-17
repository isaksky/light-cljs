(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def result (fs/file here "bench"))
(p/shell {:extra-env {"LIGHT_CLJS_BINARY" (str (fs/file here "light-cljs-v8-integrated"))}}
  "bb" "/Users/admin/src/light-cljs/benchmarks/penpot.bb" (str result))
(doseq [label ["render" "main-none" "main-advanced"]]
  (p/shell "cmp" (str (fs/file result (str label ".js"))) (str "/private/tmp/light-cljs-phase111/" label ".js"))
  (with-open [in (io/input-stream (fs/file result (str label ".js")))
              out (io/output-stream (fs/file here (str label "-syntax.log")))]
    (p/shell {:in in :out out :err :out} "node" "--check" "--input-type=module")))
(println "All five cold benchmarks complete; compiler outputs unchanged and valid ESM")
