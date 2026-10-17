(require '[babashka.fs :as fs] '[babashka.process :as p])
(def here (fs/parent *file*))
(doseq [[label binary] [["candidate-repeat1" (fs/file here "light-cljs-v8-buffer-budget")]
                        ["baseline-repeat1" "/private/tmp/light-cljs-phase108/light-cljs-v8-source-baseline"]
                        ["candidate-repeat2" (fs/file here "light-cljs-v8-buffer-budget")]
                        ["baseline-repeat2" "/private/tmp/light-cljs-phase108/light-cljs-v8-source-baseline"]]]
  (p/shell {:extra-env {"LIGHT_V8_BINARY" (str binary) "LIGHT_V8_HEAP_BYTES" "268435456"}}
    "bb" (str (fs/file here "run-bench.bb")) "app.main" label "--optimize" "none")
  (p/shell "cmp" (str (fs/file here "main-none.js")) (str (fs/file here (str label ".js")))))
