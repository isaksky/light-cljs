(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(doseq [name ["before" "after"]]
  (println "Replaying" name) (flush)
  (with-open [out (io/output-stream (fs/file here (str name ".log")))]
    (p/shell {:out out :err :out} "/usr/bin/time" "-l" "/private/tmp/light-cljs-phase22/probe"
      (str (fs/file here (str "runtime-" name ".js"))) "/private/tmp/light-cljs-phase31/replay.js")))
