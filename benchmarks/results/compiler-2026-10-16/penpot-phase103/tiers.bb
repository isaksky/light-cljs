(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn])
(def here (fs/parent *file*))
(def baseline (fs/file "/private/tmp/light-cljs-phase102"))
(assert (= [1421 0 106]
           (:tests (edn/read-string (slurp (fs/file baseline "validation.edn"))))))
(def binary (fs/file baseline "light-cljs-v8-lexical-js-vars"))
(fs/copy (fs/file baseline "run-bench.bb") (fs/file here "run-bench.bb") {:replace-existing true})
(def early-flags
  "--invocation-count-for-maglev=40 --invocation-count-for-turbofan=300 --minimum-invocations-after-ic-update=50 --invocation-count-for-maglev-with-delay=60 --invocation-count-for-osr=50 --invocation-count-for-maglev-osr=10")
(doseq [[label flags] [["early-tiers" early-flags] ["default-tiers" ""]]]
  (println "Measuring" label) (flush)
  (p/shell {:extra-env {"LIGHT_V8_BINARY" (str binary)
                        "LIGHT_V8_HEAP_BYTES" "268435456"
                        "LIGHT_V8_FLAGS" flags}}
    "bb" (str (fs/file here "run-bench.bb")) "app.main" label "--optimize" "none")
  (p/shell "cmp" (str (fs/file here (str label ".js")))
    (str (fs/file baseline "main-none.js"))))
(println "Tiering comparison completed; complete outputs unchanged")
