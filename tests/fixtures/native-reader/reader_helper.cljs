(ns reader-helper)
(defn decorate [x] (with-meta x {:tagged true}))
