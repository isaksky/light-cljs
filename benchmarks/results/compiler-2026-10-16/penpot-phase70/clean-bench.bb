(require '[babashka.fs :as fs] '[babashka.process :as p])
(def here (fs/parent *file*))
(def out (fs/file here "clean"))
(fs/create-dirs out)
(fs/copy (fs/file here "run-bench.bb") (fs/file out "run-bench.bb") {:replace-existing true})
(doseq [[entry label options]
        [["app.render" "render" ["--optimize" "none"]]
         ["app.main" "main-none" ["--optimize" "none"]]
         ["app.main" "main-advanced" ["--optimize" "advanced" "--pseudo-names" "--elide-asserts"]]]]
  (apply p/shell {:extra-env {"LIGHT_V8_BINARY" (str (fs/file here "light-cljs-unit-hygiene")) "LIGHT_V8_HEAP_BYTES" "268435456"}}
    "bb" (str (fs/file out "run-bench.bb")) entry label options)
  (p/shell "cmp" (str (fs/file out (str label ".js"))) (str "/private/tmp/light-cljs-phase42/production/" label ".js")))
