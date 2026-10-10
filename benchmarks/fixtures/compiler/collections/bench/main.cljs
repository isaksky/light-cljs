(ns bench.main (:require [bench.data :as d]))
(reduce + 0 d/items)
