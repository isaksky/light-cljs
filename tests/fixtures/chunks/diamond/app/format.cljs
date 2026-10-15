(ns app.format
  (:require [app.text :as text]))

(defn line [label value] (str label ": " (text/shout value)))
