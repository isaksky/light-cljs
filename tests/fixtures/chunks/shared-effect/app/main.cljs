(ns app.main
  "Two features share app.helper, whose top-level effects run once: at
  startup in the single file, when the first feature loads as chunks, before
  either feature calls the helper."
  (:require [light.lazy :as lazy]))

(def editor (lazy/loadable app.editor/start))

(def reports (lazy/loadable app.reports/start))

(defn start []
  (-> (lazy/load editor)
      (.then (fn [f] (js/console.log (f)) (lazy/load reports)))
      (.then (fn [f] (js/console.log (f)) nil))))

start
