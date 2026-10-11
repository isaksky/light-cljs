(ns extensions.impl (:require [extensions.protocol :as p] [cljs.core :as c]))
(c/extend-protocol p/P
  p/T (run ([this] (.-value this)) ([this x] (+ (.-value this) x)))
  p/R (run ([this] (:value this)) ([this x] (+ (:value this) x)))
  string (run ([this] this) ([this x] (str this x))))
(defn change! [] (extend-type p/T p/P (run [_] 12)))
