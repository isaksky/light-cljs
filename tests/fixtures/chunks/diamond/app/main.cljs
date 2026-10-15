(ns app.main
  "Two features share app.format, which requires app.text: both lazy roots
  and no other reach the two, so they are one chunk both features import."
  (:require [light.lazy :as lazy]))

(def editor (lazy/loadable app.editor/start))

(def reports (lazy/loadable app.reports/start))

(defn start []
  (-> (lazy/load editor)
      (.then (fn [f] (js/console.log (f "draft")) (lazy/load reports)))
      (.then (fn [f] (js/console.log (f [3 4 5])) nil))))

start
