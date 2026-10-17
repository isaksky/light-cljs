(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(with-open [out (io/output-stream (fs/file here "build.log"))]
  (p/shell {:dir "/private/tmp/light-cljs-phase21/project" :out out :err :out
            :extra-env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase13-storage-profile/target"}}
           "cargo" "build" "--release"))
(fs/copy "/private/tmp/light-cljs-phase13-storage-profile/target/release/light-cljs"
         (fs/file here "light-cljs") {:replace-existing true})
(def roots (mapcat #(vector "--source-root" (str "/Users/admin/src/light-cljs/research/penpot-light/" %))
                  ["frontend/src" "common/src" "vendor" "generated" "shims"]))
(with-open [out (io/output-stream (fs/file here "instaparse.js"))
            err (io/output-stream (fs/file here "capture.log"))]
  (apply p/shell {:out out :err err}
    (concat ["/usr/bin/time" "-l" (str (fs/file here "light-cljs"))
             "--compile" "--esm" "--macro-expansions" "100000000" "--macro-nodes" "4000000000"
             "--macro-timeout-ms" "3600000" "--macro-memory-limit" "4000000000"
             "--macro-poll-budget" "1000000000000"] roots ["--entry" "instaparse.abnf" "--optimize" "none"])))
