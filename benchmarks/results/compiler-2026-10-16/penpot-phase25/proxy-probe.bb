(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def evaluator-source (slurp "src/evaluator.rs"))
(def start (str/index-of evaluator-source "fn frontier_replay_includes_an_earlier_bundle_initialized_by_a_proxy_patch()"))
(def test-source (subs evaluator-source start (str/index-of evaluator-source "    #[test]" start)))
(def expression (second (re-find #"(?s)evaluate_js\(r#\"(.*?)\"#\)" test-source)))
(assert expression)
(spit (fs/file here "proxy-probe.js")
  (str "const fs = require('node:fs');\n"
       "const rt = (0, eval)(fs.readFileSync('src/js_runtime.js', 'utf8'));\n"
       "globalThis.__lightRuntimeForCompiler = rt;\n"
       "(0, eval)(fs.readFileSync(process.argv[2], 'utf8'));\n"
       "console.log(" expression ");\n"))
(doseq [[name path expected] [["original" "/private/tmp/light-cljs-phase24/before.js" "[true,3,1,2,3]\n"]
                             ["naive-index" "/private/tmp/light-cljs-phase24/after.js" "[false,3,1,2,3]\n"]
                             ["fixed-index" "src/compiler_roots.js" "[true,3,1,2,3]\n"]]]
  (let [out (:out (p/shell {:out :string} "node" (str (fs/file here "proxy-probe.js")) path))]
    (spit (fs/file here (str "proxy-" name ".log")) out)
    (assert (= expected out) (str name ": " out))
    (println name (str/trim out))))
