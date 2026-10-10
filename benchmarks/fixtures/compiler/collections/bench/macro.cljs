(ns bench.macro (:require-macros [bench.macros :as m]))
(reduce + 0 (m/data))
