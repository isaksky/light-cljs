(require '[cheshire.core :as json] '[babashka.fs :as fs])
(def here (fs/parent *file*))
(let [heap (json/parse-string (slurp (fs/file here "render-v9.heapsnapshot")) true)
      fields (get-in heap [:snapshot :meta :node_fields])
      types (get-in heap [:snapshot :meta :node_types 0])
      strings (:strings heap)
      nodes (mapv vec (partition (count fields) (:nodes heap)))]
  (assert (= fields ["type" "name" "id" "self_size" "edge_count" "detachedness"]))
  (println fields)
  (prn (sort-by val > (reduce (fn [m n] (update m (types (n 0)) (fnil + 0) (n 3))) {} nodes)))
  (doseq [node (take 20 (sort-by #(nth % 3) > nodes))]
    (let [name (strings (node 1))]
      (prn {:type (types (node 0)) :bytes (node 3) :length (count name)
            :name (subs name 0 (min 100 (count name)))}))))
