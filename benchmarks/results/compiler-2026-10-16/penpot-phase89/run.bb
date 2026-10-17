(require '[babashka.fs :as fs] '[babashka.process :as p]
         '[clojure.java.io :as io] '[clojure.edn :as edn])
(def here (fs/parent *file*))
(def command (edn/read-string (slurp "/private/tmp/light-cljs-phase88/main-none.command.edn")))
(spit (fs/file here "main-none.command.edn") (pr-str {:command command :environment {"LIGHT_QJS_NAMESPACE_CACHE" "0"}}))
(with-open [out (io/output-stream (fs/file here "main-none.js"))
            err (io/output-stream (fs/file here "main-none.time"))]
  (apply p/shell {:out out :err err :extra-env {"LIGHT_QJS_NAMESPACE_CACHE" "0"}}
         "/usr/bin/time" "-l" command))
(p/shell "cmp" (str (fs/file here "main-none.js")) "/private/tmp/light-cljs-phase42/production/main-none.js")
(println "Uncached detached-registration control completed.")
