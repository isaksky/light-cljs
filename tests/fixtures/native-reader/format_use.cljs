(ns format-use (:require [cuerdas.core :as str]))
(let [a "A" b "B"]
  (js/console.log (str/ffmt "url(%1): %2/%1 %% %" a b a b)
                  (str/ffmt "plain")
                  (str/ffmt "%10/%1" "a" "b" "c" "d" "e" "f" "g" "h" "i" "j")))
