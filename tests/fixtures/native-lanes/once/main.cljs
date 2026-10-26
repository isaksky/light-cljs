(ns once.main (:require [once.a] [once.b] [once.macros :refer [register snapshot]]))
(register :main)
(js/console.log (pr-str (snapshot)))
