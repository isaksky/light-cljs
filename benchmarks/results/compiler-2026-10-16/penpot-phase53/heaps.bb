(require '[babashka.fs :as fs] '[babashka.process :as p])
(def here (fs/parent *file*))
(doseq [[size label] [[134217728 "main-128"] [67108864 "main-64"]]]
  (p/shell {:extra-env {"LIGHT_V8_BINARY" "/private/tmp/light-cljs-phase50/light-cljs-mimalloc" "LIGHT_V8_HEAP_BYTES" (str size)}}
    "bb" (str (fs/file here "run-bench.bb")) "app.main" label)
  (p/shell "cmp" (str (fs/file here (str label ".js"))) "/private/tmp/light-cljs-phase42/production/main-none.js"))
