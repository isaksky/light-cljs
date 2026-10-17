(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(spit (fs/file here "profile.bb")
  (str/replace (slurp "/private/tmp/light-cljs-phase48/run.bb")
    "/private/tmp/light-cljs-phase47/light-cljs-bindings"
    "/private/tmp/light-cljs-phase50/light-cljs-mimalloc"))
(doseq [file ["summarize.bb"]]
  (fs/copy (fs/file "/private/tmp/light-cljs-phase48" file) (fs/file here file) {:replace-existing true}))
(fs/copy "/private/tmp/light-cljs-phase50/run-bench.bb" (fs/file here "run-bench.bb") {:replace-existing true})
(spit (fs/file here "heaps.bb")
  "(require '[babashka.fs :as fs] '[babashka.process :as p])\n(def here (fs/parent *file*))\n(doseq [[size label] [[134217728 \"main-128\"] [67108864 \"main-64\"]]]\n  (p/shell {:extra-env {\"LIGHT_V8_BINARY\" \"/private/tmp/light-cljs-phase50/light-cljs-mimalloc\" \"LIGHT_V8_HEAP_BYTES\" (str size)}}\n    \"bb\" (str (fs/file here \"run-bench.bb\")) \"app.main\" label)\n  (p/shell \"cmp\" (str (fs/file here (str label \".js\"))) \"/private/tmp/light-cljs-phase42/production/main-none.js\"))\n")
(println "Prepared profile and smaller heap trials, not started.")
