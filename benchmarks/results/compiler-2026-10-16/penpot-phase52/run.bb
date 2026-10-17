(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(with-open [out (io/output-stream (fs/file here "build.log"))]
  (p/shell {:dir (str project) :out out :err :out
            :extra-env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase43/v8-probe/target"}}
    "cargo" "build" "--release"))
(def binary (fs/file here "light-cljs-quickjs-mimalloc"))
(fs/copy "/private/tmp/light-cljs-phase43/v8-probe/target/release/light-cljs" binary {:replace-existing true})
(def roots (mapcat #(vector "--source-root" (str "/Users/admin/src/light-cljs/research/penpot-light/" %))
                  ["frontend/src" "common/src" "vendor" "generated" "shims"]))
(def command (vec (concat [(str binary) "--compile" "--esm"
                          "--macro-expansions" "100000000" "--macro-nodes" "4000000000"
                          "--macro-timeout-ms" "3600000" "--macro-memory-limit" "4000000000"
                          "--macro-poll-budget" "1000000000000"]
                    roots ["--entry" "app.main" "--optimize" "none"])))
(spit (fs/file here "main-quickjs-mimalloc.command.edn") (pr-str command))
(with-open [out (io/output-stream (fs/file here "main-quickjs-mimalloc.js"))
            err (io/output-stream (fs/file here "main-quickjs-mimalloc.time"))]
  (apply p/shell {:out out :err err} "/usr/bin/time" "-l" command))
(p/shell "cmp" (str (fs/file here "main-quickjs-mimalloc.js")) "/private/tmp/light-cljs-phase42/production/main-none.js")
(println "QuickJS allocator and compact registration trial completed.")
