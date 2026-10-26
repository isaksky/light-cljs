(ns demand.failure (:require [demand.macros :refer [demand-failure]]))
(defn left [] (demand-failure 40))
(defn right [] (demand-failure 40))
