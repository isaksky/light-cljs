(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(spit (fs/file here "run.bb")
  (str/replace (slurp "/private/tmp/light-cljs-phase45/run.bb")
    "/private/tmp/light-cljs-phase44/light-cljs-v12"
    "/private/tmp/light-cljs-phase47/light-cljs-bindings"))
(fs/copy "/private/tmp/light-cljs-phase45/summarize.bb" (fs/file here "summarize.bb") {:replace-existing true})
(println "Prepared updated native profile; not started.")
