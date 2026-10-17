(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(doseq [file ["check.bb" "validate.bb" "run.bb" "clean-bench.bb" "build-bench.bb"]]
  (spit (fs/file here file)
    (-> (slurp (fs/file "/private/tmp/light-cljs-phase96" file))
        (str/replace "shared interrupt scheduler" "bounded V8 stack and shared scheduler")
        (str/replace "light-cljs-shared-watchdog" "light-cljs-v8-stack-limits"))))
