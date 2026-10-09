(ns report.metrics)

(defmacro with-base [base & body]
  `(let [~'base ~base] ~@body))

(defn totals [rows]
  (reduce (fn [acc {:keys [region amount active]}]
            (if active (update acc region (fn [old] (+ (or old 0) amount))) acc))
          {} rows))

(defn checksum [totals]
  (with-base 3
    (reduce + (map (fn [[region amount]] (+ base (* (inc region) amount))) totals))))
