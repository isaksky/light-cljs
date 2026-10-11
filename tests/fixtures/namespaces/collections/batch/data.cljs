(ns batch.data (:require [batch.macros :as m]))

(def original (m/initial-data))
(def words '(batch/data x))
(def tags #{:a :b})
(def here ::local)
