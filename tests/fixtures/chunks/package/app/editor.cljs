(ns app.editor
  (:require ["stub-package" :as stub :refer [twice]]))

(defn start [x] (str (twice x) " " (.-label stub)))
