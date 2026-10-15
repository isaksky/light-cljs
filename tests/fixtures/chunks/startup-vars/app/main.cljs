(ns app.main
  "The editor feature assigns Vars of app.state, which startup owns, with
  `set!`, `binding` and `with-redefs`; startup reads them back after the
  feature ran."
  (:require [app.state :as state]
            [light.lazy :as lazy]))

(def editor (lazy/loadable app.editor/start))

(defn show [] [state/mode state/*depth* (state/fixed-value)])

(defn start []
  (-> (lazy/load editor)
      (.then (fn [f]
               (js/console.log (pr-str (f)) (pr-str (show)) (pr-str (f)))
               nil))))

start
