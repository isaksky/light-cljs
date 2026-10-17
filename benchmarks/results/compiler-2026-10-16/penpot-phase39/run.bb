(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project "/private/tmp/light-cljs-phase21/project")
(def target "/private/tmp/light-cljs-phase13-storage-profile/target")
(with-open [out (io/output-stream (fs/file here "build.log"))]
  (p/shell {:dir project :extra-env {"CARGO_TARGET_DIR" target} :out out :err :out}
    "cargo" "build" "--release" "--example" "replay_probe" "--bin" "light-cljs"))
(doseq [[src dst] [["release/examples/replay_probe" "probe"] ["release/light-cljs" "light-cljs"]]]
  (fs/copy (fs/file target src) (fs/file here dst) {:replace-existing true}))
(println "Replaying captured grammar") (flush)
(with-open [out (io/output-stream (fs/file here "replay.log"))]
  (p/shell {:extra-env {"LIGHT_PROFILE_INTERRUPTS" "1"} :out out :err :out}
    (str (fs/file here "probe")) (str (fs/file here "runtime.js")) "/private/tmp/light-cljs-phase31/replay.js"))
(let [rows (for [[_ top incl at] (re-seq #"JS_SAMPLE top=(\d+) inclusive=(\d+) ([^\n]+)" (slurp (fs/file here "replay.log")))]
             {:top (parse-long top) :inclusive (parse-long incl) :at at})]
  (spit (fs/file here "replay-summary.edn")
    (pr-str {:top (take 70 (sort-by :top > rows)) :inclusive (take 70 (sort-by :inclusive > rows))})))
(println "Replay profile complete")
