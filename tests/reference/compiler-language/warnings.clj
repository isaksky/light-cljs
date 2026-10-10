;; Keep execution.clj responsible for identity, base options, and phase checks.
;; Capture every public warning in order, including warnings before exceptions.
(require '[cljs.analyzer :as warning-ana]
         '[cljs.env :as warning-env]
         '[clojure.edn :as warning-edn]
         '[clojure.java.io :as warning-io]
         '[clojure.string :as warning-str])

(def ^:dynamic *warning-probe* nil)

(let [[_ _ _ output mode input settings-path] *command-line-args*
      _ (assert (= mode "analysis"))
      settings (warning-edn/read-string (slurp settings-path))
      rows (mapv #(warning-str/split % #"\t" -1) (warning-str/split-lines (slurp input)))
      form-ids (reduce (fn [result [id reader-mode before source after]]
                         (warning-env/with-compiler-env (warning-env/default-compiler-env)
                           (reduce (fn [result form]
                                     (assert (or (nil? (get result form)) (= id (get result form)))
                                             "Warning probes must have distinguishable top-level forms")
                                     (assoc result form id))
                                   result (doall (warning-ana/forms-seq*
                                                    (java.io.StringReader. (str before source after))
                                                    (when (= reader-mode "cljc") "boundary.cljc"))))))
                       {} rows)
      warnings (atom {})
      observed-settings (atom {})
      analyze warning-ana/analyze]
  (with-redefs [warning-ana/analyze
                (fn [& args]
                  (if-let [id (and (nil? *warning-probe*) (false? (:def-emits-var (first args))) (get form-ids (second args)))]
                    (let [config (get settings id)
                          _ (assert (= #{:checked-arrays :static-fns} (set (keys config))))
                          _ (assert (every? boolean? (vals config)))
                          previous (:options @warning-env/*compiler*)
                          handlers warning-ana/*cljs-warning-handlers*]
                      (try
                        (swap! warning-env/*compiler* assoc-in [:options :static-fns] (:static-fns config))
                        (binding [*warning-probe* id
                                  warning-ana/*unchecked-if* false
                                  warning-ana/*cljs-static-fns* (:static-fns config)
                                  warning-ana/*cljs-warning-handlers*
                                  [(fn [kind env info]
                                     (when (get warning-ana/*cljs-warnings* kind)
                                       (swap! warnings update id (fnil conj [])
                                              [(name kind) (warning-ana/error-message kind info)]))
                                     (doseq [handler handlers] (handler kind env info)))]
                                  warning-ana/*checked-arrays* (if (:checked-arrays config) :warn false)]
                          (let [effective {:optimizations (:optimizations (warning-ana/compiler-options))
                                           :static-fns warning-ana/*cljs-static-fns*
                                           :checked-arrays (= :warn warning-ana/*checked-arrays*)
                                           :def-emits-var (:def-emits-var (first args))}]
                            (assert (= (merge {:optimizations :none :def-emits-var false} config) effective))
                            (swap! observed-settings assoc id effective)
                            (apply analyze (assoc (first args) :ns (warning-ana/get-namespace warning-ana/*cljs-ns*)) (rest args))))
                        (finally (swap! warning-env/*compiler* assoc :options previous))))
                    (apply analyze args)))]
    (load-file "tests/reference/execution.clj"))
  (assert (= (set (map first rows)) (set (keys @observed-settings))) "Every warning probe must observe its effective settings")
  (spit (warning-io/file output "warnings.tsv")
        (apply str (for [[id] rows]
                     (str id "\t[" (warning-str/join "," (for [[kind message] (get @warnings id [])]
                                                     (do (assert (string? message))
                                                         (str "[" (pr-str kind) "," (pr-str message) "]")))) "]\n"))))
  (spit (warning-io/file output "warning-settings.tsv")
        (apply str (for [[id] rows
                         :let [config (get @observed-settings id)]]
                     (str id "\t" (:static-fns config) "\t" (:checked-arrays config) "\n")))))
