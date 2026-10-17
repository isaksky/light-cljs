(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io]
         '[clojure.string :as str])
(def here (fs/parent *file*))
(p/shell "bb" (str (fs/file here "validate.bb")))
(def counts (reduce #(mapv + %1 %2) [0 0 0]
  (map #(mapv parse-long (rest %))
    (re-seq #"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored"
      (slurp (fs/file here "tests-v8-default.log"))))))
(assert (= counts [1426 0 106]) counts)
(spit (fs/file here "validation.edn")
  (pr-str {:tests counts :clippy :passed :format :passed :node-workflows 3}))
(spit (fs/file here "build-only.bb")
  (str/replace (slurp "/private/tmp/light-cljs-phase99/build-only.bb")
    "light-cljs-prepared-deferred-units" "light-cljs-v8-guarded-startup"))
(p/shell "bb" (str (fs/file here "build-only.bb")))
(doseq [[entry label options]
        [["app.render" "render" ["--optimize" "none"]]
         ["app.main" "main-none" ["--optimize" "none"]]
         ["app.main" "main-advanced" ["--optimize" "advanced" "--pseudo-names" "--elide-asserts"]]]]
  (apply p/shell {:extra-env {"LIGHT_V8_BINARY" (str (fs/file here "light-cljs-v8-guarded-startup"))
                            "LIGHT_V8_HEAP_BYTES" "268435456"}}
    "bb" (str (fs/file here "run-bench.bb")) entry label options)
  (p/shell "cmp" (str (fs/file here (str label ".js")))
    (str "/private/tmp/light-cljs-phase101/production/" label ".js"))
  (with-open [in (io/input-stream (fs/file here (str label ".js")))
              out (io/output-stream (fs/file here (str label "-syntax.log")))]
    (p/shell {:in in :out out :err :out} "node" "--check" "--input-type=module")))
(println "V8 validation and all three clean builds passed, identical to production")
