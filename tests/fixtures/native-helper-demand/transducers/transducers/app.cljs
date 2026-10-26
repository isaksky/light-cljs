(ns transducers.app (:require [transducers.a :as a] [transducers.b :as b]))
(js/console.log (pr-str a/result) (pr-str b/result))
