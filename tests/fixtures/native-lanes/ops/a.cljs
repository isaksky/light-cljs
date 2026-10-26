(ns ops.a (:require [ops.macros :refer [step reentrant]]))
(def x (step :a))
(def y (reentrant))
