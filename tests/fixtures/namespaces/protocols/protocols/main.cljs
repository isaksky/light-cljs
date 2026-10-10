(ns protocols.main (:require [protocols.alpha :as a :refer [make-alpha]] [protocols.beta :as b] [cljs.core :as c]))
(let [v (make-alpha 7) saved a/run w (b/make-beta)]
  (js/console.log (pr-str [(saved v) (apply saved v [3]) (protocols.alpha/run w) (b/run w)]))
  (js/console.log (pr-str [(c/satisfies? a/P v) (c/implements? a/P w) (satisfies? a/Marker v) (satisfies? b/P v)]))
  (js/console.log (pr-str (a/run (with-meta v {`a/run (fn [_] :meta)}))))
  (js/console.log (:doc (meta #'a/run)) (name (:protocol (meta #'a/run)))))
(extend-type string a/P (run ([_] 12) ([_ x] x)))
(js/console.log (a/run "x") (implements? a/P "x") (satisfies? a/P "x"))
