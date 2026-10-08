;; Reproduce the JVM reader's character classes, including its UTF-16-char API.
;; Run from the project root with:
;; clojure -Sdeps '{:paths ["research/clojurescript/src/main/clojure"]}' -M tests/reference/reader_classes.clj
(require '[cljs.vendor.clojure.tools.reader.impl.utils :as utils])

(defn ranges [pred]
  (let [points (filter #(pred (char %)) (range 65536))]
    (loop [remaining (next points) start (first points) end (first points) result []]
      (if-let [p (first remaining)]
        (if (= p (inc end))
          (recur (next remaining) start p result)
          (recur (next remaining) p p (conj result [start end])))
        (conj result [start end])))))

(doseq [[label pred] [[:whitespace utils/whitespace?] [:digit utils/numeric?]]]
  (println label)
  (doseq [[start end] (ranges pred)]
    (println (format "U+%04X..U+%04X" start end))))
