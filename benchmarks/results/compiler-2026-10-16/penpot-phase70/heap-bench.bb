(require '[babashka.fs :as fs] '[babashka.process :as p])
(def here (fs/parent *file*))
(def out (fs/file here "heaps"))
(fs/create-dirs out)
(fs/copy (fs/file here "run-bench.bb") (fs/file out "run-bench.bb") {:replace-existing true})
(def result (p/shell {:continue true :extra-env {"LIGHT_V8_BINARY" (str (fs/file here "light-cljs-unit-hygiene")) "LIGHT_V8_HEAP_BYTES" "134217728"}}
  "bb" (str (fs/file out "run-bench.bb")) "app.main" "main-128"))
(spit (fs/file out "status.edn") (pr-str {:heap-bytes 134217728 :exit (:exit result)}))
(when (zero? (:exit result)) (p/shell "cmp" (str (fs/file out "main-128.js")) "/private/tmp/light-cljs-phase42/production/main-none.js"))
