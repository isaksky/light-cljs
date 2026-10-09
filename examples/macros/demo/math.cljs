(ns demo.math (:require-macros [demo.macros :as m :refer [twice]]))
(defn scale [x] (m/twice (twice x)))
