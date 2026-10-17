(require '[babashka.fs :as fs] '[clojure.string :as str])
(def root (fs/parent *file*))
(def prior "/private/tmp/claude-501/-Users-admin-src-light-cljs/ef390548-9edc-4e71-ba3a-a2837e664e83/scratchpad/bench")
(defn runtime [text]
  (let [start (str/index-of text "// Original bounded runtime for emitted JavaScript.")
        end (str/index-of text "\n})();" start)]
    (subs text start (+ end 6))))
(doseq [[job old new] [["render" "sub_render5.js" "render.js"] ["main-none" "main_none.js" "clean/main-none.js"] ["main-advanced" "main_advanced.js" "main-advanced.js"]]]
  (let [a (runtime (slurp (fs/file prior old))) b (runtime (slurp (fs/file root new)))]
    (prn {:job job :runtime-equal (= a b) :runtime-byte-delta (- (alength (.getBytes b "UTF-8")) (alength (.getBytes a "UTF-8")))})))
