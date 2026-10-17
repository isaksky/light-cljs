(require '[babashka.fs :as fs] '[clojure.string :as str] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def baseline (slurp (fs/file here "runtime-baseline.js")))
(def no-box (str/replace baseline "Object(value) === value" "(typeof value === \"object\" ? value !== null : typeof value === \"function\")"))
(assert (= 2 (count (re-seq #"Object\(value\) === value" baseline))))
(spit (fs/file here "runtime-no-box.js") no-box)
(def slots (-> (slurp (fs/file here "runtime-collections.js"))
               (str/replace #"      const mask = protocolMasks.get\('[^']+'\);\n      Type.prototype\[mask\[0\]\] = \(Type.prototype\[mask\[0\]\] \|\| 0\) \| mask\[1\];\n" "")
               (str/replace "Type === Cons &&" "(Type === Cons || Type === NodeSeqType) &&")))
(spit (fs/file here "runtime-slots.js") slots)
(spit (fs/file here "forged.js") "JSON.stringify([rt.core.isSeq(Object.create(rt.core.List.prototype)), rt.core.isCounted(Object.create(rt.core.List.prototype))])")
(doseq [name ["baseline" "collections" "slots"]]
  (let [result (p/shell {:out :string :err :string} "/private/tmp/light-cljs-phase22/probe"
          (str (fs/file here (str "runtime-" name ".js"))) (str (fs/file here "forged.js")))]
    (spit (fs/file here (str "forged-" name ".log")) (:out result))
    (println name (str/trim (:out result)))))
(doseq [name ["no-box" "slots"]]
  (println "Replaying" name) (flush)
  (with-open [out (io/output-stream (fs/file here (str name ".log")))]
    (p/shell {:out out :err :out} "/usr/bin/time" "-l" "/private/tmp/light-cljs-phase22/probe"
      (str (fs/file here (str "runtime-" name ".js"))) (str (fs/file here "replay.js")))))
