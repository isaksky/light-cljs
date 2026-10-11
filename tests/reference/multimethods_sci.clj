;; Portable multimethod source probes from pinned SCI, EPL-1.0.
(require '[clojure.edn :as edn]
         '[clojure.java.shell :as shell]
         '[clojure.string :as str]
         '[sci.core :as sci])
(let [[checkout fixture] *command-line-args*
      revision "5fe89056384418be50d232b4278344cb3a1a526b"
      path "test/sci/multimethods_test.cljc"
      head (shell/sh "git" "-C" checkout "rev-parse" "HEAD")
      pinned (shell/sh "git" "-C" checkout "show" (str revision ":" path))]
  (assert (and (zero? (:exit head)) (= revision (str/trim (:out head)))))
  (assert (and (zero? (:exit pinned)) (= (:out pinned) (slurp (str checkout "/" path)))))
  (let [cases (->> (str/split-lines (slurp fixture))
                  (filter #(str/starts-with? % "sci-"))
                  (map #(str/split % #"\t" -1)))]
    (assert (= 5 (count cases)))
    (doseq [[id source expected] cases]
      (let [actual (sci/eval-string source)]
        (assert (= (edn/read-string expected) actual)
                (str id ": expected " expected ", got " (pr-str actual)))))
    (println "Matched" (count cases) "portable multimethod observations against SCI" revision)))

(shutdown-agents)
