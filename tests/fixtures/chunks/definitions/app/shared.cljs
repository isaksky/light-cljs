(ns app.shared)

(defn title [s] (str "== " s " =="))

(defn fmt [label values]
  (str label ": " (apply str (interpose ", " (map #(if (number? %) (* 10 %) %) values)))))

(defn outline [words]
  (str (count words) " words, first " (first words) ", last " (last words)))

(defprotocol Shape
  (area [shape]))

(defmulti render :kind)
