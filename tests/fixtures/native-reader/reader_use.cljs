(ns reader-use (:require [reader-macros :refer [probe]]))
(js/console.log (pr-str (probe)))
