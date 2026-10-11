(ns ^{:foo old} shared.main "namespace doc" {:foo new}
  (:refer-clojure :exclude [+] :rename {count size when always})
  (:require [shared.logic :as logic :refer [value add] :rename {add plus}]
            [shared.portable :as portable :refer [number]]
            [shared.sugar :as sugar :refer [twice choose] :rename {choose chosen}]
            [shared.choice :as choice]
            [clojure.example :as example :refer [answer] :rename {answer imported-answer}]
            [clojure.core :refer [when] :rename {when another-when}])
  (:use [shared.leaf :only [value] :rename {value leaf-value}]))
(def + (fn* [a b] a))
(def shared.main/own 42)
(js/console.log "main")
(js/console.log value (plus 4) (logic/add 6) (portable/number) (number)
                (sugar/twice 3) (twice 4) (chosen 9))
(js/console.log (+ 5 2) (cljs.core/+ 5 2) (size [1 2 3]) (always true 42)
                own shared.main/own choice/value imported-answer example/answer clojure.example/answer cljs.example/answer)
(js/console.log (= ::logic/k :shared.logic/k) (= `imported-answer 'cljs.example/answer)
                (= `size 'cljs.core/count) (= `+ 'shared.main/+)
                (sugar/metadata-name) (logic/keyword-name ::portable/key) (sugar/metadata-doc) (another-when true leaf-value))
