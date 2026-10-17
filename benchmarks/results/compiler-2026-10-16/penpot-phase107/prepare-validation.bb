(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(doseq [file ["validate.bb" "check-and-bench.bb"]]
  (spit (fs/file here file)
    (-> (slurp (str "/private/tmp/light-cljs-phase102/" file))
      (str/replace "lexical JS assignment" "guarded runtime startup")
      (str/replace "[1421 0 106]" "[1426 0 106]")
      (str/replace "light-cljs-v8-lexical-js-vars" "light-cljs-v8-guarded-startup"))))
