(ns app.editor
  (:require [app.text :as text]
            [light.lazy :as lazy]))

(def spelling (lazy/loadable app.spell/check))

(defn start []
  (.then (lazy/load spelling) (fn [check] (str (text/greet) ", " (check)))))
