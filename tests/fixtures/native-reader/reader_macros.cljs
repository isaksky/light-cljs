(ns reader-macros (:require [cljs.reader :as reader]))
(defmacro probe []
  (list 'quote [(reader/read-string "42")
                (reader/read-string {:readers {'pair vec}} "#pair (1 2)")]))
