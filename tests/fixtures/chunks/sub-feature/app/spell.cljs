(ns app.spell
  (:require [app.editor :as editor]
            [app.text :as text]))

(defn check [] (str "spelled " (text/greet) " " (fn? editor/start)))
