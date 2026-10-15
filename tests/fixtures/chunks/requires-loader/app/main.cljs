(ns app.main
  "The editor feature requires app.main, the namespace that loads it, and
  reads startup's definitions through imports. app.util, which app.main
  requires, is a resident target: its loadable imports nothing."
  (:require [app.shared :as shared]
            [app.util :as util]
            [light.lazy :as lazy]))

(def editor (light.lazy/loadable app.editor/start))

(def twice (light.lazy/loadable app.util/twice))

(defn ready [] (shared/label :main))

(defn start []
  (-> (lazy/load twice)
      (.then (fn [f] (js/console.log (f "ab")) (lazy/load editor)))
      (.then (fn [f] (js/console.log (f)) nil))))

start
