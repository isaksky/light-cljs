(ns batch.ops
  (:require [batch.data :as d] [cljs.core :as c])
  (:require-macros [batch.macros :as m]))

(js/console.log "ops")
(defn changed [] (m/append-item d/original :items 3))
(defn total [m] (c/reduce c/+ 0 (:items m)))
