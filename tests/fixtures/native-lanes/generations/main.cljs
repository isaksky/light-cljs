(ns generations.main (:require [generations.a] [generations.b] [generations.macros :refer [trace note]]))
(note :main)
(js/console.log (pr-str (trace)))
