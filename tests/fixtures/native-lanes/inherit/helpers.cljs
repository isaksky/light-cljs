(ns inherit.helpers (:require [inherit.macros :as m]))
(def snapshot (do (swap! m/counter + 100) @m/counter))
(defmacro snap [] snapshot)
