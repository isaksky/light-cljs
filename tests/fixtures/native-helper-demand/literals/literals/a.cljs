(ns literals.a (:require [literals.macros :refer [echo]]))
(def value (echo :a ^{:tag :a} [#js [#js {:value 1}]]))
