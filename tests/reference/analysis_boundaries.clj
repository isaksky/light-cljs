;; Negative/reference-permissiveness probes, loaded by execution.clj's analysis mode.
;; Analyze, emit and syntax-check; never execute rejected/subset-boundary programs.
(require '[cljs.env :as env]
         '[cljs.compiler :as comp])

(defn boundary-attempt [stage f]
  (try {:value (f)}
       (catch Throwable error
         {:category stage
          :detail (ex-message (or (ex-cause error) error))})))

(defn boundary-result [source analysis-env mode]
  (let [read-result (boundary-attempt "reader-error"
                      #(doall (ana/forms-seq* (java.io.StringReader. source)
                                             (when (= mode "cljc") "boundary.cljc"))))
        analyzed (if (:category read-result) read-result
                   (boundary-attempt "analysis-error"
                     #(mapv (fn [form] (ana/analyze analysis-env form)) (:value read-result))))
        emitted (if (:category analyzed) analyzed
                  (boundary-attempt "emit-error"
                    #(str/join "\n" (map comp/emit-str (:value analyzed)))))]
    (if (:category emitted)
      emitted
      ;; Missing/crashed Node is infrastructure failure, never a passing rejection.
      (let [{:keys [exit err]}
            (shell/sh (or (System/getenv "NODE") "node") "--check"
                      :in (str "function boundarySyntaxCheck() {\n"
                               (if (= :expr (:context analysis-env))
                                 (str "return (\n" (:value emitted) "\n);")
                                 (:value emitted)) "\n}"))]
        (cond
          (zero? exit) {:category "accepted"}
          (and (= exit 1) (str/includes? err "SyntaxError"))
          {:category "javascript-error" :detail err}
          :else (throw (ex-info "Boundary JavaScript syntax checker failed" {:exit exit :stderr err})))))))

(defn check-analysis-boundaries [output input]
  (let [baseline (env/default-compiler-env {:optimizations :none :static-fns false})
        rows (->> (str/split-lines (slurp input))
                  (remove #(or (str/blank? %) (str/starts-with? % "#"))))]
    (env/with-compiler-env baseline
      (comp/with-core-cljs nil (fn [])))
    (let [results
          (for [row rows
                :let [[id mode before rejected after] (str/split row #"\t" -1)
                      source (str before rejected after)
                      warnings (atom [])
                      result
                      (env/with-compiler-env (atom @baseline)
                        (binding [ana/*cljs-ns* 'cljs.user
                                  ana/*cljs-static-fns* false
                                  ;; Validate headers independently of filesystem loading.
                                  ana/*analyze-deps* false
                                  ana/*cljs-warning-handlers*
                                  [(fn [type _ _]
                                     (when (type ana/*cljs-warnings*)
                                       (swap! warnings conj (name type))))]]
                          (let [analysis-env (cond-> (assoc (ana/empty-env) :def-emits-var false)
                                               (#{"expr" "cljc"} mode) (assoc :context :expr))
                                effective {:optimizations (:optimizations (ana/compiler-options))
                                           :static-fns ana/*cljs-static-fns*
                                           :def-emits-var (:def-emits-var analysis-env)}]
                            (assert (= expected-settings effective))
                            (boundary-result source analysis-env mode))))
                      category (if (and (= (:category result) "accepted") (seq @warnings))
                                 "warning" (:category result))]]
            (do
              (assert (and id mode rejected) (str "Invalid boundary row: " row))
              (str id "\t" category "\t" (str/join "," (sort (distinct @warnings)))
                   "\t" (str/replace (or (:detail result) "") #"[\r\n\t]+" " "))))]
      (spit (io/file output "analysis.tsv") (str (str/join "\n" results) "\n")))
    (println "REFERENCE SETTINGS" (pr-str expected-settings))))
