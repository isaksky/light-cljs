;; First-party reader facade; algorithms and provider state belong to canonical JS.
(ns cljs.reader)
(def ^:dynamic *tag-table* (atom (compiler-reader-initial-tag-table)))
(def ^:dynamic *default-data-reader-fn* (atom nil))
(defn parse-and-validate-timestamp [s] (compiler-reader-parse-timestamp-fields s))
(defn parse-timestamp [s] (compiler-reader-parse-timestamp s))
(defn ^:private string-options [opts]
  (assoc (merge {:default @*default-data-reader-fn*} opts)
         :readers (merge @*tag-table* (:readers opts))))
(defn ^:private stream-options [opts]
  (assoc (merge opts {:default @*default-data-reader-fn*})
         :readers (merge @*tag-table* (:readers opts))))
(defn read-string
  ([s] (compiler-reader-read-string {:readers @*tag-table* :default @*default-data-reader-fn* :eof nil} s))
  ([opts s] (compiler-reader-read-string (string-options opts) s)))
(defn read
  ([] (compiler-reader-read {:readers @*tag-table* :default @*default-data-reader-fn* :eof nil} (compiler-reader-current-input)))
  ([rdr] (compiler-reader-read {:readers @*tag-table* :default @*default-data-reader-fn* :eof nil} rdr))
  ([opts rdr] (compiler-reader-read (stream-options opts) rdr))
  ([rdr eof-error? eof opts] (compiler-reader-read rdr eof-error? eof (stream-options opts))))
(defn string-push-back-reader ([s] (compiler-reader-string-reader s)) ([s name] (compiler-reader-string-reader s name)))
(defn read-line ([] (compiler-reader-read-line)) ([rdr] (compiler-reader-read-line rdr)))
(defn input-provider [name f] (compiler-reader-input-provider name f))
(defn close [rdr] (compiler-reader-close rdr))
(defn register-tag-parser! [tag f]
  (let [previous (get @*tag-table* tag)]
    (compiler-reader-register-tag-parser tag f)
    (swap! *tag-table* assoc tag f) previous))
(defn deregister-tag-parser! [tag]
  (let [previous (get @*tag-table* tag)]
    (compiler-reader-deregister-tag-parser tag)
    (swap! *tag-table* dissoc tag) previous))
(defn register-default-tag-parser! [f]
  (let [previous @*default-data-reader-fn*]
    (compiler-reader-register-default-tag-parser f)
    (reset! *default-data-reader-fn* f) previous))
(defn deregister-default-tag-parser! []
  (let [previous @*default-data-reader-fn*]
    (compiler-reader-deregister-default-tag-parser)
    (reset! *default-data-reader-fn* nil) previous))
