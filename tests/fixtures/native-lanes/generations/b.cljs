(ns generations.b (:require [generations.macros :refer [note]]))
(defn f [] (note :b1))
(defmacro note [x] (swap! generations.macros/log conj [:b x]) nil)
(defn g [] (note :b2))
