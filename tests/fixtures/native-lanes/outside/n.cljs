(ns outside.n (:require [outside.reg :as r])) (defmacro bump [] (swap! (:k @r/table) inc))
