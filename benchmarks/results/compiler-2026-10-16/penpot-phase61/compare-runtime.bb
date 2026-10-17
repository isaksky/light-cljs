(require '[babashka.fs :as fs] '[clojure.string :as str])
(def phase (or (first *command-line-args*) "61"))
(def here (fs/file (str "/private/tmp/light-cljs-phase" phase)))
(defn bridge [path]
  (let [source (slurp path) start (str/index-of source "  function compilerFormBridge() {") end (str/index-of source "  //@unit inspect" start)]
    (subs source start end)))
(def old (bridge (fs/file "/private/tmp/light-cljs-phase58/project/src/js_runtime.js")))
(def new (bridge (fs/file here "project/src/js_runtime.js")))
(def output (slurp (fs/file here "main-none.js")))
(assert (str/includes? output new))
(assert (= 1 (count (re-seq (re-pattern (java.util.regex.Pattern/quote new)) output))))
(def normalized (str/replace output new old))
(assert (= normalized (slurp "/private/tmp/light-cljs-phase42/production/main-none.js")))
(println "Only the expected embedded compilerFormBridge source changed; all other output is byte-identical to phase42.")
