;; Exact original SCI sources read as .cljc with the real :cljs reader feature.
(ns sci.cljc-transport-after185)
(require '[clojure.java.io :as io] '[clojure.string :as str]
         '[cljs.env :as env] '[cljs.compiler :as comp])
(let [source (slurp "tests/reference/execution.clj")
      boundary (.indexOf source "\n(try\n  (let [[kind identity checkout output mode input")]
  (assert (pos? boundary)) (load-string (subs source 0 boundary)))
(defn unhex [s] (String. (byte-array (map #(unchecked-byte (Integer/parseInt % 16)) (re-seq #".." s))) "UTF-8"))
(def encoder (slurp "tests/reference/compiler-language/sci-cljc-transport-after185-encoder.cljs"))
(defn case-js [id original baseline]
  (let [source (str "(let [original-value (do " original ")] " (str/replace encoder "ORIGINAL_ID" (pr-str id)) ")")]
  (env/with-compiler-env (atom @baseline)
    (binding [ana/*cljs-ns* 'user ana/*cljs-static-fns* false
              ana/*unchecked-if* false ana/*unchecked-arrays* false
              ana/*cljs-file* "sci-original.cljc"]
      (swap! env/*compiler* assoc-in [::ana/namespaces 'user] {:name 'user :defs {}})
      (let [forms (doall (ana/forms-seq* (java.io.StringReader. source) "sci-original.cljc"))
            effective (assoc (ana/empty-env) :def-emits-var false)
            _ (assert (= {:optimizations :none :static-fns false :def-emits-var false}
                         {:optimizations (:optimizations (ana/compiler-options))
                          :static-fns ana/*cljs-static-fns*
                          :def-emits-var (:def-emits-var effective)})
                      "Actual original .cljc analysis settings drift")
            _ (ana/analyze (assoc effective :context :statement) '(def observe nil))
            emitted (map-indexed
                      (fn [index form]
                        (comp/emit-str (ana/analyze
                          (assoc effective :context (if (= index (dec (count forms))) :return :statement)) form))) forms)]
        (str "function(observe) { user.observe=observe;\n"
             (if (seq forms) (str/join "\n" emitted) "return null;") "\n}"))))))
(try
  (let [[kind identity checkout output input] *command-line-args*
        output (io/file output) src (io/file output "src")
        baseline (env/default-compiler-env {:optimizations :none :static-fns false})
        _ (checked-reference kind identity checkout)
        _ (env/with-compiler-env baseline (comp/with-core-cljs nil (fn [])))
        rows (mapv (fn [line] (let [[id raw] (str/split line #"\t")] [id (unhex raw)])) (str/split-lines (slurp input)))
        results (mapv (fn [[id source]]
                  (try {:id id :source source :status "compiled" :js (case-js id source baseline)}
                    (catch Throwable error {:id id :source source :status "rejected"
                      :detail (str/replace (ex-message (or (ex-cause error) error)) #"[\r\n\t]+" " ")}))) rows)]
    (io/make-parents (io/file src "user.cljs")) (spit (io/file src "user.cljs") "(ns user) (def marker true)")
    (io/make-parents (io/file src "semantic/reference.cljs"))
    (spit (io/file src "semantic/reference.cljs") (str "(ns semantic.reference (:require [user]))\n" inspect-value-source))
    (checked-build src output 'semantic.reference #{'user 'semantic.reference})
    (doseq [result results]
      (let [folder (io/file output "original-sources" (:id result))]
        (io/make-parents (io/file folder "sci-original.cljc"))
        (spit (io/file folder "sci-original.cljc") (:source result))
        (spit (io/file folder (if (= "compiled" (:status result)) "compiled.js" "rejection.txt"))
          (or (:js result) (:detail result)))))
    (spit (io/file output "contexts.js")
      (str "globalThis.lightCases=[" (str/join "," (map #(or (:js %) "null") results)) "];\n"
           "globalThis.lightCaseMetadata=[" (str/join "," (map #(str "[" (pr-str (:id %)) "," (pr-str (:source %)) "]") results)) "];\n"))
    (spit (io/file output "contexts.tsv")
      (str/join "\n" (map #(str (:id %) "\t" (:status %) "\t" (:detail %)) results)))
    (checked-reference kind identity checkout))
  (finally (shutdown-agents)))
