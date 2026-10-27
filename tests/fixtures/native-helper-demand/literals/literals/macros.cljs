(ns literals.macros (:require [literals.helper :as h]))
(defmacro echo [tag x] (h/emit tag x))
(defmacro fresh [] (list 'quote (h/fresh)))
