(ns app.editor
  (:require [app.main :as main]
            [app.text :as text]))

(defn start [] (str (main/ready) " " text/greeting))
