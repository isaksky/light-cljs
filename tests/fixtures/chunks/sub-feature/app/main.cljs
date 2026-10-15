(ns app.main
  "The editor feature loads a spelling sub-feature, which requires the
  editor back: the editor's chunk is one both lazy roots reach, and the
  sub-feature's chunk imports it."
  (:require [light.lazy :as lazy]))

(def editor (lazy/loadable app.editor/start))

(defn start []
  (-> (lazy/load editor)
      (.then (fn [f] (f)))
      (.then (fn [text] (js/console.log text) nil))))

start
