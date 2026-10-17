(require '[babashka.fs :as fs] '[babashka.process :as p])
(def here (fs/parent *file*))
(def output (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase113"))
(fs/create-dirs output)
(doseq [pattern ["*.bb" "*.log" "*.edn" "*.time"] file (fs/glob here pattern)]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(let [result (p/shell {:out :string :continue true} "diff" "-u"
               "--label" "a/src/js_runtime.js" "--label" "b/src/js_runtime.js"
               "/private/tmp/light-cljs-phase112/project/src/js_runtime.js"
               (str (fs/file here "project/src/js_runtime.js")))]
  (assert (= 1 (:exit result)))
  (spit (fs/file output "candidate.patch") (:out result)))
(let [result (p/shell {:out :string :continue true} "diff" "-qr"
               "/private/tmp/light-cljs-phase112/project/src" (str (fs/file here "project/src")))]
  (assert (= 1 (:exit result)))
  (assert (= 1 (count (clojure.string/split-lines (:out result)))))
  (assert (clojure.string/includes? (:out result) "js_runtime.js"))
  (spit (fs/file output "source-difference.log") (:out result)))
(spit (fs/file output "artifact-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
    (map #(str (fs/file here %)) ["light-cljs-specialized-stores" "baseline-1.js" "baseline-2.js" "candidate-1.js" "candidate-2.js"]))))
(println "Rejected specialization experiment archived")
