(ns app.main
  "The editor looks namespaces up, so the program reaches reflection: at
  advanced every top-level form stays in its namespace's chunk, and the
  helper of app.shared that only the editor reads stays in startup."
  (:require [app.shared :as shared]
            [light.lazy :as lazy]))

(js/console.log (shared/title "menu"))

(def editor (lazy/loadable app.editor/start))

(defn start []
  (-> (lazy/load editor)
      (.then (fn [f] (js/console.log (pr-str (f))) nil))))

start
