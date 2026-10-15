(ns app.shared)

(defn title [s] (str "== " s " =="))

(defn outline [words]
  (str (count words) " words, first " (first words) ", last " (last words)))
