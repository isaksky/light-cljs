;   Copyright (c) Rich Hickey. All rights reserved.
;   The use and distribution terms for this software are covered by the
;   Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;   which can be found in the file epl-v10.html at the root of this distribution.
;   By using this software in any fashion, you are agreeing to be bound by
;   the terms of this license.
;   You must not remove this notice, or any other, from this software.

(ns foo.core
  (:refer-clojure :rename {when always map core-map}))

;; Renaming excludes both original names, allowing these ordinary own Vars.
(defn when [] :own-when)
(defn map [] :own-map)

(defn run [observe]
  (assert (= [:own-when :own-map] [(when) (map)]))
  (let [events (atom [])]
    (assert (= :selected (always true (swap! events conj :ran) :selected)))
    (assert (nil? (always false (swap! events conj :unexpected))))
    (assert (= [:ran] @events)))
  (assert (= (always true 42) (cljs.core/when true 42)))
  (assert (identical? core-map cljs.core/map))
  (assert (= [2 3 4] (vec (core-map inc [1 2 3]))))
  true)
run
