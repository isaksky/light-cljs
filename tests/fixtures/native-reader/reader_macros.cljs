(ns reader-macros (:require [cljs.reader :as reader] [reader-helper :as helper]))
(defmacro probe []
  (list 'quote [(reader/read-string "42")
                (reader/read-string {:readers {'pair vec}} "#pair (1 2)")]))

(defmacro missing-readers-probe [nil-options empty-options eof-options]
  (list 'quote [(reader/read-string nil-options "1")
                (reader/read-string empty-options "2")
                (reader/read-string eof-options "")
                (reader/read nil-options (reader/string-push-back-reader "3"))
                (reader/read empty-options (reader/string-push-back-reader "4"))
                (reader/read eof-options (reader/string-push-back-reader ""))]))

(defmacro callback-probe []
  (let [seen (js/Array.)
        parsed (reader/read-string
                {:readers {'wrap (fn [x]
                                   (.push seen "before")
                                   (let [v (helper/decorate x)]
                                     (.push seen "after") v))}}
                "#wrap [7]")]
    (list 'quote [parsed (meta parsed) (vec seen)])))

(defmacro js-probe []
  (list 'quote [(reader/read-string "#js [#js {:value \"\\uD800\"}]")
                (reader/read-string "#js {\"10\" 10 \"2\" 2 :z 3}")]))
