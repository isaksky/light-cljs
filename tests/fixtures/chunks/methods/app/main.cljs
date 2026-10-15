(ns app.main
  "A loadable of app.shapes, a namespace that only adds methods to startup's
  multimethod: loading it runs the methods and resolves to nil."
  (:require [light.lazy :as lazy]))

(defmulti area :shape)

(defmethod area :default [_] :unknown)

(def shapes (lazy/loadable app.shapes))

(defn start []
  (-> (lazy/load shapes)
      (.then (fn [v]
               (js/console.log (pr-str v) (area {:shape :square :side 3})
                               (pr-str (area {:shape :circle})))
               nil))))

start
