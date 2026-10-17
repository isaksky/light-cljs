(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def command (assoc (edn/read-string (slurp (fs/file here "main-none.command.edn"))) 0
               "/private/tmp/light-cljs-phase87/production/light-cljs"))
(spit (fs/file here "baseline-main.command.edn") (pr-str command))
(with-open [out (io/output-stream (fs/file here "baseline-main.js"))
            err (io/output-stream (fs/file here "baseline-main.time"))]
  (apply p/shell {:out out :err err} "/usr/bin/time" "-l" command))
(p/shell "cmp" (str (fs/file here "baseline-main.js")) (str (fs/file here "main-none.js")))
(println "Frozen QuickJS production baseline completed")
