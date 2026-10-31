(ns extensions.ext (:require [extensions.p :as p]))
(def secret "extension")
(extend-type p/T p/P (value [_] secret) Object (toString [_] secret))
(defmethod p/choose :x [_] secret)
