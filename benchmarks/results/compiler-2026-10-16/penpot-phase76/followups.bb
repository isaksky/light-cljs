(require '[babashka.process :as p])
(doseq [[label script] [["Clean validated reader benchmarks" "/private/tmp/light-cljs-phase76/clean-bench.bb"]
                        ["Rust allocation diagnostic" "/private/tmp/light-cljs-phase77/run.bb"]
                        ["Shared layer semantic tests" "/private/tmp/light-cljs-phase78/check.bb"]
                        ["Shared layer benchmark" "/private/tmp/light-cljs-phase78/run.bb"]]]
  (println label) (flush)
  (p/shell "bb" script))
