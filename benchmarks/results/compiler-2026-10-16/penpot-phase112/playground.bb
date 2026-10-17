(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(doseq [step ["build" "check"]]
  (println "WebAssembly playground" step) (flush)
  (with-open [out (io/output-stream (fs/file here (str "playground-" step ".log")))]
    (p/shell {:dir (str (fs/file here "project")) :out out :err :out}
      "bb" "playground/build.bb" step)))
