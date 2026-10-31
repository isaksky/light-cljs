(ns extensions.main (:require [extensions.ext] [extensions.a :as a] [extensions.b :as b] [extensions.c :as c]))
(js/console.log (pr-str [a/result b/result c/result]))
