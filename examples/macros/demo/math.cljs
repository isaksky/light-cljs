(ns demo.math (:require [demo.macros :as m :refer [twice]]))
(defn scale [x] (m/twice (twice x)))
