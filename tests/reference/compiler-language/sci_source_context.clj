;; Reference-only JVM macro declarations and original user namespace/source context.
;; No .clj input is supplied to light-cljs. Original forms are analyzed in order.
(ns sci.source-context)
(require '[clojure.java.io :as io] '[clojure.string :as str]
         '[cljs.env :as env] '[cljs.compiler :as comp])
(let [source (slurp "tests/reference/execution.clj")
      boundary (.indexOf source "\n(try\n  (let [[kind identity checkout output mode input")]
  (assert (pos? boundary) "Reference helper/main boundary changed")
  (load-string (subs source 0 boundary)))

(defn unhex [text]
  (String. (byte-array (map #(unchecked-byte (Integer/parseInt % 16))
                           (re-seq #".." text))) "UTF-8"))

(defn original-forms [source]
  (binding [ana/*cljs-ns* 'user]
    (doall (ana/forms-seq* (java.io.StringReader. source) "sci-original.cljs"))))

(defn case-js [source form-sources baseline]
  ;; The reference host namespace is a per-case macro environment, separate
  ;; from the application user namespace. JVM observation effects stay explicit.
  (when (find-ns 'user) (remove-ns 'user))
  (create-ns 'user)
  (let [phase-effects (atom [])
        macro-names (keep (fn [[kind text]]
                            (when (= kind "macro")
                              (second (binding [*ns* (find-ns 'user)] (read-string text)))))
                         form-sources)]
    (binding [*ns* (find-ns 'user)]
      (clojure.core/refer 'clojure.core)
      (intern *ns* 'observe (fn [value] (swap! phase-effects conj value) value)))
    (env/with-compiler-env (atom @baseline)
      (binding [ana/*cljs-ns* 'user ana/*cljs-static-fns* false
                ana/*unchecked-if* false ana/*unchecked-arrays* false
                ana/*cljs-file* "sci-original.cljs"]
        (swap! env/*compiler* assoc-in [::ana/namespaces 'user] {:name 'user :defs {}})
        (swap! env/*compiler* assoc-in [::ana/namespaces 'user :use-macros]
               (zipmap macro-names (repeat 'user)))
        (swap! env/*compiler* assoc-in [::ana/namespaces 'user :require-macros]
               {'user 'user})
        (let [forms (original-forms source)
              effective (assoc (ana/empty-env) :def-emits-var false)
              emitted (atom [])]
          (assert (= (count forms) (count form-sources)) "Original top-level forms changed")
          ;; Register the actual application observation fixture without moving
          ;; or wrapping the original body or changing its source coordinates.
          (ana/analyze (assoc effective :context :statement) '(def observe nil))
          (doseq [form (original-forms "(set! *print-newline* true) (set-print-fn! (fn [text] (.write (.-stdout js/process) text))) (set-print-err-fn! (fn [text] (.write (.-stderr js/process) text)))")]
            (swap! emitted conj (comp/emit-str (ana/analyze (assoc (ana/empty-env) :context :statement :def-emits-var false) form))))
          (doseq [[index [form [kind raw]]] (map-indexed vector (map vector forms form-sources))]
            (if (= kind "macro")
              (binding [*ns* (find-ns 'user)] (eval (read-string raw)))
              (do
                ;; An ordinary root referenced by a subsequent macro has its
                ;; own reference host root. This is explicit phase setup; a
                ;; failed host evaluation remains a diagnostic, never a pass.
                (when (= kind "host-root")
                  (binding [*ns* (find-ns 'user)] (eval (read-string raw))))
                (let [context (if (= index (dec (count forms))) :return :statement)
                      analyzed (ana/analyze (assoc (ana/empty-env) :context context :def-emits-var false) form)]
                  (swap! emitted conj (comp/emit-str analyzed))))))
          {:javascript (str "function(observe) { user.observe = observe;\n"
                            (str/join "\n" @emitted) "\n}")
           :phase-effects (pr-str @phase-effects)})))))

(try
  (let [[kind identity checkout output input] *command-line-args*
        output (io/file output)
        src (io/file output "src")
        baseline (env/default-compiler-env {:optimizations :none :static-fns false})
        _ (checked-reference kind identity checkout)
        _ (env/with-compiler-env baseline (comp/with-core-cljs nil (fn [])))
        rows (for [line (str/split-lines (slurp input)) :when (not (str/blank? line))]
               (let [[id source forms] (str/split line #"\t")]
                 [id (unhex source) (read-string (unhex forms))]))
        results (mapv (fn [[id source forms]]
                        (try (assoc (case-js source forms baseline) :id id :source source :status "compiled")
                             (catch Throwable error {:id id :source source :status "rejected"
                               :detail (str/replace (ex-message (or (ex-cause error) error)) #"[\r\n\t]+" " ")}))) rows)]
    (io/make-parents (io/file src "user.cljs"))
    (spit (io/file src "user.cljs") "(ns user) (def context-marker true)")
    (io/make-parents (io/file src "semantic" "reference.cljs"))
    (spit (io/file src "semantic" "reference.cljs")
          (str "(ns semantic.reference (:require [user]))\n" inspect-value-source))
    (checked-build src output 'semantic.reference #{'user 'semantic.reference})
    (spit (io/file output "contexts.js")
          (str "globalThis.lightCases = [\n"
               (str/join ",\n" (map #(or (:javascript %) "null") results)) "];\n"
               "globalThis.lightCaseMetadata = ["
               (str/join "," (map #(str "[" (pr-str (:id %)) "," (pr-str (:source %)) "]") results)) "];\n"))
    (doseq [result results :when (= (:status result) "compiled")]
      (let [path (io/file output "case-js" (str (:id result) ".js"))]
        (io/make-parents path)
        (spit path (str "globalThis.lightCases = [" (:javascript result) "];\n"
                        "globalThis.lightCaseMetadata = [[" (pr-str (:id result)) ","
                        (pr-str (:source result)) "]];\n"))))
    (spit (io/file output "contexts.tsv")
          (str/join "\n" (map #(str (:id %) "\t" (:status %) "\t"
                                      (or (:detail %) "") "\t" (or (:phase-effects %) "")) results)))
    (checked-reference kind identity checkout))
  (finally (shutdown-agents)))
