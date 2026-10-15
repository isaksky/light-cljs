(ns app.main
  "Startup of the code-splitting example: logs the menu when it loads, and the
  host calls `start` (the default export with --esm) to load the features."
  (:require [app.shared :as shared]
            [light.lazy :as lazy]))

(js/console.log "menu:" (shared/label :editor) "|" (shared/label :reports))

(def editor (lazy/loadable app.editor/start))

(def reports (lazy/loadable app.reports/start))

(defn start
  "Loads and runs the editor feature, then the reports feature; each logs its
  result as one line. Returns a promise of nil."
  []
  (-> (lazy/load editor)
      (.then (fn [start-editor] (start-editor) (lazy/load reports)))
      (.then (fn [start-reports] (start-reports) nil))))

start
