(ns reader-use (:require [reader-macros :refer [probe callback-probe]]))
(js/console.log (pr-str (probe)))
(js/console.log (pr-str (callback-probe)))
