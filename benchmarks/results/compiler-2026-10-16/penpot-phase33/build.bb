(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(with-open [out (io/output-stream (fs/file here "build.log"))]
  (p/shell {:dir "/private/tmp/light-cljs-phase21/project" :out out :err :out
            :extra-env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase13-storage-profile/target"}}
           "cargo" "build" "--release"))
(fs/copy "/private/tmp/light-cljs-phase13-storage-profile/target/release/light-cljs"
         (fs/file here "light-cljs") {:replace-existing true})
