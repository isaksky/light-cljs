(require '[clojure.string :as str])
;; Attribute each leaf sample to its innermost compiler phase. Nested macro
;; execution can reenter analysis, so inclusive phase counts overlap.
(def phases [["environment-import" "18import_environment"]
             ["form-import" "11import_data"]
             ["form-export" "11export_data"]
             ["macro-call" "ActiveEvaluator4call"]
             ["javascript-unit-evaluation" "13evaluate_unit"]
             ["arity-and-expression-warnings" "collect_expression_warnings"]
             ["environment-construction" "20compiler_environment"]
             ["reader-environment" "18reader_environment"]
             ["reader-view" "18update_reader_view"]
             ["deferred-analysis" "13analyze_input"]
             ["macro-expansion-other" "17expand_javascript"]
             ["namespace-analysis" "NamespaceAnalysis7analyze"]])
(def totals (atom {}))
(defn finish [stack]
  (let [{:keys [n children phase]} (peek stack)]
    (swap! totals update phase (fnil + 0) (- n children))
    (pop stack)))
(loop [lines (->> (str/split-lines (slurp (first *command-line-args*)))
                 (drop-while #(not= "Call graph:" %))
                 (take-while #(not (str/starts-with? % "Total number in stack"))))
       stack []]
  (if-let [line (first lines)]
    (let [m (re-matcher #"(\d+) (.*)  \(in " line)]
      (if (.find m)
        (let [depth (.start m 1) n (parse-long (.group m 1)) name (.group m 2)
              stack (loop [s stack] (if (and (seq s) (>= (:depth (peek s)) depth)) (recur (finish s)) s))
              phase (or (some (fn [[p needle]] (when (str/includes? name needle) p)) phases)
                        (:phase (peek stack)) "other")
              stack (if (seq stack) (update-in stack [(dec (count stack)) :children] + n) stack)]
          (recur (next lines) (conj stack {:depth depth :n n :children 0 :phase phase})))
        (recur (next lines) stack)))
    (loop [s stack] (when (seq s) (recur (finish s))))))
(def samples (reduce + (vals @totals)))
(println "Samples:" samples)
(doseq [[phase n] (sort-by val > @totals)]
  (println phase n (format "%.2f%%" (* 100.0 (/ n samples)))))
