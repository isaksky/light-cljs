(ns types.main (:require [types.alpha :as a] [types.beta :as b]))
(let [old a/original new (a/UserBox. 2) beta (b/UserBox. 3) via-saved (a/saved-factory 4)
      saved a/read-value]
  (assert (= [1 [:new 2] [:beta 3] [:new 4]]
             [(saved old) (saved new) (apply saved [beta]) (saved via-saved)]))
  (assert (instance? a/saved-constructor old))
  (assert (not (instance? a/UserBox old)))
  (assert (instance? a/UserBox via-saved))
  (assert (not (identical? a/UserBox b/UserBox)))
  (assert (identical? (type old) a/saved-constructor))
  (assert (= 8 (a/write-value old 8)))
  (assert (= 10 (apply old [2])))
  (js/console.log "types-ok"))
