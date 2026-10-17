(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(doseq [name ["run.bb" "validate.bb" "clean-bench.bb" "build-bench.bb"]]
  (spit (fs/file here name)
    (-> (slurp (fs/file "/private/tmp/light-cljs-phase97" name))
        (str/replace "light-cljs-v8-stack-limits" "light-cljs-prepared-deferred-units")
        (str/replace "bounded V8 stack and shared scheduler" "prepared deferred units"))))
