(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(p/shell "touch" (str (fs/file project "src/lib.rs")) (str (fs/file project "src/main.rs")))
(with-open [out (io/output-stream (fs/file here "build.log"))]
  (p/shell {:dir (str project) :out out :err :out
            :extra-env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase43/v8-probe/target"
                        "CARGO_PROFILE_RELEASE_LTO" "thin" "CARGO_PROFILE_RELEASE_CODEGEN_UNITS" "1"}}
    "cargo" "build" "--release"))
(fs/copy "/private/tmp/light-cljs-phase43/v8-probe/target/release/light-cljs" (fs/file here "light-cljs-callbacks") {:replace-existing true})
(fs/copy "/private/tmp/light-cljs-phase56/run-bench.bb" (fs/file here "run-bench.bb") {:replace-existing true})
(doseq [[entry label options baseline]
        [["app.render" "render" ["--optimize" "none"] "render"]
         ["app.main" "main-none" ["--optimize" "none"] "main-none"]
         ["app.main" "main-advanced" ["--optimize" "advanced" "--pseudo-names" "--elide-asserts"] "main-advanced"]]]
  (apply p/shell {:extra-env {"LIGHT_V8_BINARY" (str (fs/file here "light-cljs-callbacks")) "LIGHT_V8_HEAP_BYTES" "268435456"}}
    "bb" (str (fs/file here "run-bench.bb")) entry label options)
  (p/shell "cmp" (str (fs/file here (str label ".js"))) (str "/private/tmp/light-cljs-phase42/production/" baseline ".js")))
