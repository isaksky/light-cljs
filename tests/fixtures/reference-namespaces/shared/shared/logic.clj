(ns shared.logic (:require [shared.helper :as h]))
(defmacro add [x] `(cljs.core/+ ~x ~(h/offset)))
(defmacro keyword-name [x] (str x))
