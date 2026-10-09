;; SCI reference transport. Only run through regenerate.py, never for execution
;; of the native interpreter. Adapted fixture attribution is in NOTICE.md.
(require '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[sci.core :as sci])

(def revision "5fe89056384418be50d232b4278344cb3a1a526b")
(def options {:features #{:clj}})
(def limit 10000)

(defn utf16 [^String s]
  (str "u" (apply str (map #(format "%04x" (int %)) s))))

(defn tagged [tag xs]
  (str "(" tag (when (seq xs) " ") (str/join " " xs) ")"))

(declare encode)

(defn encode-items [xs depth]
  (let [xs (vec (take (inc limit) xs))]
    (when (> (count xs) limit)
      (throw (ex-info "Reference serialization item limit" {})))
    (mapv #(encode % (inc depth)) xs)))

(defn encode
  ([v] (encode v 0))
  ([v depth]
   (when (> depth 128)
     (throw (ex-info "Reference serialization depth limit" {})))
   (cond
     (nil? v) "nil"
     (boolean? v) (tagged "bool" [(str v)])
     (string? v) (tagged "string" [(utf16 v)])
     (char? v) (tagged "char" [(format "%04x" (int v))])
     (symbol? v) (tagged "symbol" [(if-let [n (namespace v)] (utf16 n) "-") (utf16 (name v))])
     (keyword? v) (tagged "keyword" [(if-let [n (namespace v)] (utf16 n) "-") (utf16 (name v))])
     (instance? Double v) (tagged "float" [(format "%016x" (Double/doubleToLongBits v))])
     (instance? clojure.lang.BigInt v) (tagged "bigint" [(str v)])
     (instance? java.math.BigInteger v) (tagged "bigint" [(str v)])
     (instance? java.math.BigDecimal v) (tagged "decimal" [(str (.unscaledValue ^java.math.BigDecimal v)) (str (.scale ^java.math.BigDecimal v))])
     (integer? v) (tagged "int" [(str v)])
     (ratio? v) (let [n (numerator v) d (denominator v)]
                  (when-not (and (<= Long/MIN_VALUE n Long/MAX_VALUE) (<= 1 d Long/MAX_VALUE))
                    (throw (ex-info "Ratio components exceed v1 transport bounds" {})))
                  (tagged "ratio" [(str n) (str d)]))
     (vector? v) (tagged "vector" (encode-items v depth))
     (map? v) (do
                (when (> (count v) limit) (throw (ex-info "Reference map limit" {})))
                (tagged "map" (sort (map (fn [[k x]] (tagged "entry" [(encode k (inc depth)) (encode x (inc depth))])) v))))
     (set? v) (tagged "set" (sort (encode-items v depth)))
     (list? v) (tagged "list" (encode-items v depth))
     (seq? v) (tagged "seq" (encode-items v depth))
     :else (throw (ex-info "Unsupported reference transport value" {:class (str (class v))})))))

(defn error-outcome [^Throwable e]
  (let [data (ex-data e)
        macro-value? (some #(re-find #"^Can't take value of a macro:" (or (ex-message %) "")) (take-while some? (iterate ex-cause e)))
        splice-reader? (= "unquote-splice not in list" (ex-message e))
        unknown-tag? (boolean (re-matches #"No reader function for tag .+" (or (ex-message e) "")))
        phase (cond (or splice-reader? unknown-tag?) "read" macro-value? "analysis" :else (case (:phase data) "parse" "read" "analysis" "analysis" nil "execution"
                    (throw (ex-info "Unknown SCI error phase" {:data data} e))))
        causes (take-while some? (iterate ex-cause e))
        messages (str/join "\n" (map ex-message causes))
        category (cond
                   macro-value? "invalid-form"
                   (re-find #"(?m)^(if-let|if-some|when-let|when-some) requires (a vector for its binding|exactly 2 forms in binding vector)$" messages) "invalid-form"
                   (= phase "read") "reader"
                   (and (= phase "analysis") (str/includes? messages "Unable to resolve symbol")) "unresolved-symbol"
                   (and (= phase "analysis") (re-find #"(?m)^Wrong number of args \([0-9]+\) passed to: :.+$" messages)) "invalid-form"
                   (and (= phase "analysis") (re-find #"Too (few|many) arguments to if" messages)) "invalid-form"
                   (and (= phase "analysis") (re-find #"Wrong number of args .* passed to quote" messages)) "invalid-form"
                   (and (= phase "analysis")
                        (re-find #"(?m)^(Parameter declaration missing\.|Parameter declaration should be a vector|Can't have more than 1 variadic overload|Can't have fixed arity function with more params than variadic function)$" messages)) "invalid-form"
                   (and (= phase "analysis")
                        (re-find #"(?m)^(Can only recur from tail position|Cannot recur across try|Mismatched argument count to recur, expected: [0-9]+ args, got: [0-9]+)$" messages)) "invalid-form"
                   (and (= phase "analysis")
                        (re-find #"(?m)^(Too many arguments to throw, throw expects a single Throwable instance|Unable to resolve classname: .*)$" messages)) "invalid-form"
                   (and (= phase "execution") (str/includes? messages "Duplicate key")) "duplicate-key"
                   (and (= phase "execution")
                        (re-find #"(?m)^Cannot call .* with [0-9]+ arguments$" messages)) "arity"
                   (some #(instance? clojure.lang.ArityException %) causes) "arity"
                   (some #(instance? IndexOutOfBoundsException %) causes) "bounds"
                   (and (= phase "execution") (re-find #"(?m)^Unable to convert: class .* to Object\[\]$" messages)) "type"
                   (and (= phase "execution") (re-find #"(?m)^Can't pop empty (vector|list)$" messages)) "type"
                   (some #(or (instance? ArithmeticException %) (instance? NumberFormatException %)) causes) "arithmetic"
                   (some #(or (instance? ClassCastException %) (instance? IllegalArgumentException %) (instance? UnsupportedOperationException %)) causes) "type"
                   (some #(= :reference/throw (:kind (ex-data %))) causes) "exception-info"
                   :else (throw (ex-info "Unclassified reference exception" {:phase phase :data data} e)))]
    (tagged "error" [phase category])))

(def load-sources
  (into {} (for [line (str/split-lines (slurp (io/file (.getParent (io/file *file*)) "load-sources.tsv")))
                 :when (and (not (str/blank? line)) (not (str/starts-with? line "#")))
                 :let [[name file source] (str/split line #"\t" -1)]]
             [(symbol name) {:file file :source source}])))

(defn run-case [[id profile source]]
  (binding [*out* *err*] (println "CASE START" id source) (flush))
  (let [trace (atom [])
        out (java.io.StringWriter.)
        err (java.io.StringWriter.)
        reader-options (case (if (str/starts-with? id "sci-context-reader-tag-syntax-error-") "reader-tags" profile)
                         "loading" {:load-fn (fn [{:keys [namespace]}] (get load-sources namespace))}
                         "reader-bb" {:features #{:bb}}
                         "reader-tags" {:readers {'t/tag inc 'x/str str
                                                  't/data (fn [v] (swap! trace conj v) v)}}
                         "reader-fallback" {:readers (fn [tag] (fn [v] [tag v]))}
                         {})
        ;; A new context per row prevents definitions and effects leaking.
        ctx (sci/init (assoc (merge options reader-options) :namespaces
                             {'user (merge {'observe (fn [v] (swap! trace conj v) v)}
                                           (when (= profile "state-validators")
                                             {'set-validator! set-validator! 'get-validator get-validator}))}))
        fields (sci/binding [sci/out out sci/err err sci/read-eval false]
                 (let [result (try {:value (sci/eval-string* ctx source)}
                                   (catch Exception e {:error e}))
                       ;; Encoding errors are driver failures, outside the SCI catch.
                       outcome (if-let [e (:error result)] (error-outcome e)
                                   (tagged "return" [(encode (:value result))]))]
                   [id outcome (encode @trace) (encode (str out)) (encode (str err))]))]
    (println (str/join "\t" fields))
    (flush)
    (binding [*out* *err*] (println "CASE END" id) (flush))))

(let [[checkout input metadata] *command-line-args*
      root (.getCanonicalFile (io/file checkout))]
  (assert (and checkout input metadata (= 3 (count *command-line-args*))) "Use regenerate.py")
  (assert (= "1.12.4" (clojure-version)) "Clojure runtime version mismatch")
  (doseq [path ["sci/core.cljc" "sci/impl/parser.cljc" "sci/impl/analyzer.cljc"
                "sci/impl/interpreter.cljc" "sci/impl/namespaces.cljc" "sci/impl/types.cljc"]]
    (let [resource (io/resource path)]
      (assert (and resource (= "file" (.getProtocol resource))
                   (= (.getCanonicalFile (io/file root "src" path))
                      (.getCanonicalFile (io/file resource))))
              (str "SCI resource did not load from selected checkout: " path " -> " resource))))
  (spit metadata
        (str "format\tsci-outcomes-v1\nrevision\t" revision
             "\nclojure\t" (clojure-version)
             "\njava\t" (System/getProperty "java.runtime.version")
             "\njava-vendor\t" (System/getProperty "java.vendor")
             "\noptions\t{:features #{:clj}, :read-eval false, :user [observe], :out :captured, :err :captured}\n"))
  (let [rows (->> (str/split-lines (slurp input))
                  (remove #(or (str/blank? %) (str/starts-with? % "#")))
                  (mapv #(str/split % #"\t" -1)))]
    (assert (seq rows) "No reference cases")
    (assert (= (count rows) (count (set (map first rows)))) "Duplicate reference case IDs")
    (doseq [row rows]
      (assert (= 7 (count row)) (str "Expected seven fixture fields: " (first row)))
      (run-case row))))
(shutdown-agents)
