(ns literals.b (:require [literals.macros :refer [echo]]))
(def value (echo :b ^{:tag :b} [#js {:value #js [2]}]))
