(ns batch.main
  (:require [batch.data :as d] [batch.ops :as o] [batch.macros :as m]))

(js/console.log "main")
(defn report []
  [(o/total (o/changed)) (first d/words)
   (contains? d/tags :a) (namespace d/here)])

(defn run [observe]
  (observe (report))
  (observe (m/literal-data))
  (observe (= (m/literal-data)
              '[:generated batch/data {:nested (x [1 2])} #{:a :b}]))
  (let [changed (m/append-item (do (observe "map") d/original)
                               (do (observe "key") :items)
                               (do (observe "item") 3))]
    (observe (= changed (assoc d/original :items (conj (:items d/original) 3))))
    [(get d/original :items) (get changed :items)
     (get changed (vector 1 2)) (= '(1 2) [1 2])]))

run
