(require '[babashka.fs :as fs] '[clojure.string :as str])
(def root (fs/parent *file*))
(def prior "/private/tmp/claude-501/-Users-admin-src-light-cljs/ef390548-9edc-4e71-ba3a-a2837e664e83/scratchpad/bench")
(doseq [[stem old new] [["main-none" "main_none.js" "clean/main-none.js"] ["main-advanced" "main_advanced.js" "main-advanced.js"]]
        [version path] [["old" (fs/file prior old)] ["new" (fs/file root new)]]]
  (let [text (slurp path) start (str/index-of text "// Original bounded runtime for emitted JavaScript.")
        end (str/index-of text "\n})();" start)]
    (spit (fs/file root (str stem "-" version ".body.js"))
          (str (subs text 0 start) "<embedded-runtime>" (subs text (+ end 6))))))
