(ns bench.macro (:require [bench.macros :as m]))
(reduce + 0 (m/data))
