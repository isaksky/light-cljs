(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def script
  (-> (slurp "/private/tmp/light-cljs-phase48/archive.bb")
      (str/replace "penpot-phase48" "penpot-phase53")
      (str/replace "/private/tmp/light-cljs-phase47/light-cljs-bindings" "/private/tmp/light-cljs-phase50/light-cljs-mimalloc")
      (str/replace "[\"*.bb\" \"*.log\" \"*.edn\" \"*.txt\"]" "[\"*.bb\" \"*.log\" \"*.edn\" \"*.txt\" \"*.time\"]")))
(spit (fs/file here "archive-profile.bb") script)
(load-file (str (fs/file here "archive-profile.bb")))
