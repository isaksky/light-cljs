(ns reader-macros (:require [cljs.reader :as reader] [reader-helper :as helper]))
(defmacro probe []
  (list 'quote [(reader/read-string "42")
                (reader/read-string {:readers {'pair vec}} "#pair (1 2)")]))

(defmacro callback-probe []
  (let [seen (js/Array.)
        parsed (reader/read-string
                {:readers {'wrap (fn [x]
                                   (.push seen "before")
                                   (let [v (helper/decorate x)]
                                     (.push seen "after") v))}}
                "#wrap [7]")]
    (list 'quote [parsed (meta parsed) (vec seen)])))
