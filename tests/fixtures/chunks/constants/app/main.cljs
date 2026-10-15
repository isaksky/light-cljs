(ns app.main
  "Constants go to the chunks of their readers: startup declares the keyword
  its menu prints, each feature the constants only it reads, the chunk the
  editor and the reports share those both read, and a chunk of constants
  alone, which no namespace or definition makes, the keyword that only the
  editor and the settings read."
  (:require [light.lazy :as lazy]))

(js/console.log "menu:" (pr-str [:menu-item]))

(def editor (lazy/loadable app.editor/start))

(def reports (lazy/loadable app.reports/start))

(def settings (lazy/loadable app.settings/start))

(defn start []
  (-> (lazy/load editor)
      (.then (fn [f] (f) (lazy/load reports)))
      (.then (fn [f] (f) (lazy/load settings)))
      (.then (fn [f] (f) nil))))

start
