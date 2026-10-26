(ns failure.main (:require [failure.a :as a] [failure.b :as b] [failure.macros :refer [probe]]))
(js/console.log a/x a/y b/x (probe))
