;; Supplement the existing checked analysis mode with the actual exception class
;; caught by the upstream assertions. execution.clj still owns identity checks,
;; compiler options, phase attribution and the complete ordered observation TSV.
(require '[cljs.analyzer :as diagnostic-ana]
         '[cljs.env :as diagnostic-env]
         '[clojure.java.io :as diagnostic-io]
         '[clojure.string :as diagnostic-str])

(let [[_ _ _ output mode input] *command-line-args*
      _ (assert (= mode "analysis"))
      probes (mapv (fn [line]
                     (let [[id _ before source after] (diagnostic-str/split line #"\t" -1)
                           forms (diagnostic-env/with-compiler-env (diagnostic-env/default-compiler-env)
                                   (doall (diagnostic-ana/forms-seq*
                                            (java.io.StringReader. (str before source after)))))]
                       (assert (seq forms) "Diagnostic probes require nonempty complete source forms")
                       [id (vec forms)]))
                   (diagnostic-str/split-lines (slurp input)))
      forms (set (mapcat second probes))
      exceptions (atom {})
      analyze diagnostic-ana/analyze]
  (with-redefs [diagnostic-ana/analyze
                (fn [& args]
                  (try (apply analyze args)
                       (catch Throwable error
                         (when (forms (second args))
                           (swap! exceptions update (second args) (fnil conj #{})
                                  (.getName (class error))))
                         (throw error))))]
    (load-file "tests/reference/execution.clj"))
  (diagnostic-io/make-parents (diagnostic-io/file output "exception-classes.tsv"))
  (spit (diagnostic-io/file output "exception-classes.tsv")
        (apply str (for [[id source-forms] probes
                         :let [failed (filter #(seq (get @exceptions %)) source-forms)
                               classes (set (mapcat #(get @exceptions % #{}) failed))]]
                     (do (assert (<= (count failed) 1) "Multiple failing top-level diagnostic forms")
                         (assert (<= (count classes) 1) "Ambiguous diagnostic exception class")
                         (str id "\t" (first classes) "\n"))))))
