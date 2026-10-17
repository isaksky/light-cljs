(require '[babashka.fs :as fs] '[babashka.process :as p])
(def here (fs/parent *file*))
(def out (fs/file here "stack-comparison"))
(fs/create-dirs out)
(fs/copy (fs/file here "run-bench.bb") (fs/file out "run-bench.bb") {:replace-existing true})
(doseq [[label binary] [["scheduler-main" "/private/tmp/light-cljs-phase96/light-cljs-shared-watchdog"]
                        ["bounded-main" (str (fs/file here "light-cljs-v8-stack-limits"))]]]
  (p/shell {:extra-env {"LIGHT_V8_BINARY" binary "LIGHT_V8_HEAP_BYTES" "268435456"}}
    "bb" (str (fs/file out "run-bench.bb")) "app.main" label "--optimize" "none")
  (p/shell "cmp" (str (fs/file out (str label ".js"))) "/private/tmp/light-cljs-phase42/production/main-none.js"))
