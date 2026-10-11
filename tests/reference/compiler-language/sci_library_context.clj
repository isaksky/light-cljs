;; Complete original SCI library bodies with visible import/observation setup.
(require '[clojure.java.io :as io] '[clojure.string :as str])
(let [source (slurp "tests/reference/execution.clj")
      boundary (.indexOf source "\n(try\n  (let [[kind identity checkout output mode input")]
  (assert (pos? boundary) "Reference helper/main boundary changed")
  (load-string (subs source 0 boundary)))
(defn unhex [text]
  (assert (re-matches #"(?:[0-9a-f]{2})*" text) "Invalid hex source")
  (String. (byte-array (map #(unchecked-byte (Integer/parseInt % 16))
                           (re-seq #".." text))) "UTF-8"))
(try
  (let [[kind identity checkout output input] *command-line-args*
        output (io/file output) src (io/file output "src")
        rows (mapv (fn [line]
                     (let [[id original declarations runtime] (str/split line #"\t")]
                       [id (unhex original) (unhex declarations) (unhex runtime)]))
                   (str/split-lines (slurp input)))
        namespaces (mapv #(symbol (str "sci.library.case" %)) (range (count rows)))]
    (checked-reference kind identity checkout)
    (assert (seq rows) "Zero library sources")
    (doseq [[case-ns [id original declarations runtime]] (map vector namespaces rows)]
      (let [file (io/file src "sci" "library" (str (last (str/split (str case-ns) #"\.")) ".cljs"))]
        (io/make-parents file)
        (spit file (str "(ns " case-ns " (:require [clojure.string] [clojure.set] [clojure.walk] [clojure.edn]))\n"
                        declarations "\n(def run (fn* [observe] "
                        "(set! *print-newline* true) "
                        "(set-print-fn! (fn [text] (.write (.-stdout js/process) text))) "
                        "(set-print-err-fn! (fn [text] (.write (.-stderr js/process) text))) "
                        runtime "))\n"))))
    (let [file (io/file src "semantic" "reference.cljs")]
      (io/make-parents file)
      (spit file (str "(ns semantic.reference (:require "
                      (str/join " " (map #(str "[" % "]") namespaces)) "))\n"
                      "(set! (.-lightCases js/globalThis) (array "
                      (str/join " " (map #(str % "/run") namespaces)) "))\n"
                      "(set! (.-lightCaseMetadata js/globalThis) (array "
                      (str/join " " (map (fn [[id original]]
                                           (str "(array " (pr-str id) " " (pr-str original) ")")) rows)) "))\n"
                      inspect-value-source)))
    (checked-build src output 'semantic.reference (conj (set namespaces) 'semantic.reference))
    (checked-reference kind identity checkout))
  (finally (shutdown-agents)))
