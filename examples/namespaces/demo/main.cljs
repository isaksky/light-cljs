(ns demo.main
  (:require [demo.math :as m]))
(def twice
  (fn* [f x]
    (f (f x))))
(js/console.log
  (twice m/root 256))
