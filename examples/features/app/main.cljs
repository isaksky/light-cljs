(ns app.main
  "Startup of the code-splitting example: logs the menu when it loads, and the
  host calls `start` (the default export with --esm) to run the features."
  (:require [app.shared :as shared]
            [app.editor :as editor]
            [app.reports :as reports]))

(js/console.log "menu:" (shared/label :editor) "|" (shared/label :reports))

(defn start
  "Runs the editor feature, then the reports feature; each logs its result as
  one line."
  []
  ;; Becomes (lazy/loadable app.editor/start) once light.lazy lands.
  (editor/start)
  ;; Becomes (lazy/loadable app.reports/start) once light.lazy lands.
  (reports/start)
  nil)

start
