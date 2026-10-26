(ns siblings.main (:require [siblings.a :as a] [siblings.b :as b] [siblings.c :as c] [siblings.macros :refer [next-value]]))
(js/console.log (pr-str [a/v1 a/v2 a/v3 b/v1 b/v2 b/v3 c/v1 c/v2 c/v3 (next-value) (+ (a/pure) (b/pure) (c/pure))]))
