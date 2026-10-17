(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def runtime-text (slurp "src/js_runtime.js"))
(def anchor "  function recordFieldIndex(Type, key) { return Type.fieldNames.findIndex(name => equalPair(key, keyword(null, name))); }")
(assert (str/includes? runtime-text anchor))
(def updated
  (str/replace runtime-text anchor
    (str anchor "\n  function recordKeywordFieldIndex(Type, key) { const data = keywords.get(key); return data ? Type.fieldNames.indexOf(data.spelling) : -1; }")))
(def updated
  (reduce (fn [s [begin end]]
            (let [a (str/index-of s begin) b (str/index-of s end a)]
              (assert (and a b))
              (str (subs s 0 a) (str/replace (subs s a b) "recordFieldIndex" "recordKeywordFieldIndex") (subs s b))))
          updated [["  //@unit recordMethods.lookup" "  //@unit recordMethods.count"]
                   ["  //@unit recordMethods.contains" "  //@unit recordMethods.dissoc"]]))
(spit (fs/file here "runtime-record.js") updated)
