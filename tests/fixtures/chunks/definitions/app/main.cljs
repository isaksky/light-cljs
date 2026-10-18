(ns app.main
  "Startup reads one definition of app.shared; at advanced the others move
  toward the features that read them: `fmt` and the multimethod, which both
  features read, to their shared chunk, and the editor's helper and protocol
  to the editor's chunk. Each feature's method of the multimethod stays in
  the feature's chunk."
  (:require [app.shared :as shared]
            [light.lazy :as lazy]))

(js/console.log (shared/title "menu"))

(def editor (lazy/loadable app.editor/start))

(def reports (lazy/loadable app.reports/start))

(defn start []
  (-> (lazy/load editor)
      (.then (fn [f] (js/console.log (f)) (lazy/load reports)))
      (.then (fn [f] (js/console.log (f)) nil))))

start
