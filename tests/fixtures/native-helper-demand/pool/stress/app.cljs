(ns stress.app (:require [stress.a :as a] [stress.b :as b] [stress.c :as c]))
(js/console.log (a/f5) (b/f5) (c/f5) (pr-str (a/f3)) (pr-str (b/f3)) (pr-str (c/f3)))
