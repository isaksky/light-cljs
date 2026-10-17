(require '[clojure.java.io :as io] '[clojure.string :as str] '[babashka.fs :as fs])
(def here (fs/parent *file*))
(defn category [stack]
  (cond
    (str/includes? stack "reader_identity") :reader-identity
    (str/includes? stack "program_fragment6append") :fragment-append
    (re-find #"5arena.*[Cc]lone" stack) :arena-clone
    (re-find #"7layered|7Layered" stack) :layered-maps
    (re-find #"\) (?:js_|JS_)[^ |]+" stack) :quickjs
    :else :other))
(doseq [phase ["before" "after"]]
  (with-open [r (io/reader (fs/file here (str phase "-heap.txt")))]
    (let [rows (keep #(when-let [[_ calls bytes stack] (re-matches #"(\d+) calls? for (\d+) bytes: (.*)" %)]
                       {:calls (parse-long calls) :bytes (parse-long bytes) :stack stack}) (line-seq r))
          result (reduce (fn [out {:keys [calls bytes stack]}]
                           (let [kind (category stack)]
                             (-> out
                                 (update-in [:total :calls] (fnil + 0) calls)
                                 (update-in [:total :bytes] (fnil + 0) bytes)
                                 (update-in [:categories kind :calls] (fnil + 0) calls)
                                 (update-in [:categories kind :bytes] (fnil + 0) bytes)))) {} rows)]
      (spit (fs/file here (str phase "-summary.edn")) (pr-str result))
      (println phase (pr-str result)))))
