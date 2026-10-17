(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def before (slurp (fs/file here "compiler_roots.js")))
(defn replace-once [text before after]
  (assert (= 2 (count (str/split text (re-pattern (java.util.regex.Pattern/quote before)) -1))) before)
  (str/replace text before after))
(def after
  (-> before
      (replace-once "  let demandEpoch = 0;" "  let demandEpoch = 0, lastDemand = null;")
      (replace-once "  function invalidateDemands() { demanded.clear(); demandEpoch++; }"
                    "  function invalidateDemands() { demanded.clear(); demandEpoch++; lastDemand = null; }")
      (replace-once "      if (demanded.get(protocol)?.get(native)?.has(typeName)) return;"
                    (str "      if (lastDemand !== null && lastDemand.protocol === protocol && lastDemand.native === native && lastDemand.typeName === typeName) return;\n"
                         "      if (demanded.get(protocol)?.get(native)?.has(typeName)) { lastDemand = {protocol, native, typeName}; return; }"))
      (replace-once "        types.add(typeName);" "        types.add(typeName); lastDemand = {protocol, native, typeName};")))
(spit (fs/file here "compiler_roots-demand.js") after)
(spit (fs/file here "replay-demand.js") (replace-once (slurp (fs/file here "replay.js")) before after))
