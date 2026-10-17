(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def evaluator-source (slurp "src/evaluator.rs"))
(def start (str/index-of evaluator-source "fn protocol_arity_cache_does_not_inherit_array_properties()"))
(def test-source (subs evaluator-source start (str/index-of evaluator-source "    #[test]" start)))
(def expression (second (re-find #"(?s)r#\"(.*?)\"#" test-source)))
(assert expression)
(spit (fs/file here "arity-probe.js")
  (str "(() => { try { return JSON.stringify(" expression "); }"
       " catch (error) { return JSON.stringify({error: error.message}); } })()\n"))
(doseq [[name runtime expected]
        [["original" "/private/tmp/light-cljs-phase23/before.js" "30\n"]
         ["array-cache" "/private/tmp/light-cljs-phase23/after.js"
          "{\"error\":\"No protocol method ILookup/-lookup defined for this value\"}\n"]
         ["fixed-cache" "src/js_runtime.js" "30\n"]]]
  (let [out (:out (p/shell {:out :string}
                    "/private/tmp/light-cljs-phase22/probe" runtime
                    (str (fs/file here "arity-probe.js"))))]
    (spit (fs/file here (str "arity-" name ".log")) out)
    (assert (= expected out) (str name ": " out))
    (println name (str/trim out))))
