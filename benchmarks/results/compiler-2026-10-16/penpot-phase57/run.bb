(require '[babashka.fs :as fs] '[babashka.process :as p])
(def here (fs/parent *file*))
(doseq [trial (or (seq *command-line-args*) ["seq-stores" "borrowed-store" "metadata-store"])]
 (p/shell {:extra-env {"LIGHT_V8_BINARY" (str (fs/file here "light-cljs-runtime-trials")) "LIGHT_V8_HEAP_BYTES" "268435456" "LIGHT_V8_RUNTIME_TRIAL" trial}} "bb" (str (fs/file here "run-bench.bb")) "app.main" trial)
 (p/shell "cmp" (str (fs/file here (str trial ".js"))) "/private/tmp/light-cljs-phase42/production/main-none.js"))
