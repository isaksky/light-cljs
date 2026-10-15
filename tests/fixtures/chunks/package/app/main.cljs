(ns app.main
  "Only the editor feature requires the JavaScript package stub-package, so
  only the editor's chunk imports it."
  (:require [light.lazy :as lazy]))

(def editor (lazy/loadable app.editor/start))

(defn start []
  (-> (lazy/load editor) (.then (fn [f] (js/console.log (f 2)) nil))))

start
