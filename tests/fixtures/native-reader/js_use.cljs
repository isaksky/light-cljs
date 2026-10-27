(ns js-use (:require [reader-macros :refer [js-probe]]))
(let [[array object] (js-probe)]
  (js/console.log (.charCodeAt (.-value (aget array 0)) 0)
                  (.join (js/Object.keys object) ",")))
