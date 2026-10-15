(ns app.editor
  "The editor feature: sorted collections, `str`, and printing through
  `enable-console-print!`."
  (:require [app.shared :as shared]))

(defn outline
  "Each document's title with its word count, in title order."
  [documents]
  (into (sorted-map) (map (juxt :title :words)) documents))

(defn start
  "Prints one line describing the documents."
  []
  (enable-console-print!)
  (let [titles (outline shared/documents)
        ids (into (sorted-set) (keys (shared/index-by :id shared/documents)))]
    (println (str "editor: " (count titles) " documents, first "
                  (ffirst titles) ", ids " ids))))
