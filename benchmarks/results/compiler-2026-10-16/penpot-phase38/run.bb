(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project "/private/tmp/light-cljs-phase21/project")
(def target "/private/tmp/light-cljs-phase13-storage-profile/target")
(doseq [variant ["before" "after"]]
  (println "Building" variant) (flush)
  (fs/copy (fs/file here (str variant ".c")) (fs/file project "vendor/rquickjs-sys/quickjs/quickjs.c") {:replace-existing true})
  (with-open [out (io/output-stream (fs/file here (str variant "-build.log")))]
    (p/shell {:dir project :extra-env {"CARGO_TARGET_DIR" target} :out out :err :out}
      "cargo" "build" "--release" "--example" "replay_probe"))
  (fs/copy (fs/file target "release/examples/replay_probe") (fs/file here (str "probe-" variant)) {:replace-existing true}))
(doseq [variant ["before" "after" "before" "after"]
        :let [suffix (str (System/currentTimeMillis))]]
  (println "Replaying" variant suffix) (flush)
  (with-open [out (io/output-stream (fs/file here (str variant "-" suffix ".log")))]
    (p/shell {:out out :err :out} "/usr/bin/time" "-l" (str (fs/file here (str "probe-" variant)))
      (str (fs/file here "runtime.js")) "/private/tmp/light-cljs-phase31/replay.js")))
