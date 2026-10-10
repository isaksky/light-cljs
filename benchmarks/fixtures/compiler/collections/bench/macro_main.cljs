(ns bench.macro-main (:require [bench.macro-data :as d]))
(reduce + 0 d/items)
