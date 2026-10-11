;   Copyright (c) Rich Hickey. All rights reserved.
;   The use and distribution terms for this software are covered by the
;   Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;   which can be found in the file epl-v10.html at the root of this distribution.
;   By using this software in any fashion, you are agreeing to be bound by
;   the terms of this license.
;   You must not remove this notice, or any other, from this software.

;; Adapted from pinned ClojureScript core_test.cljs; EPL-1.0.
(ns cljs.core-test)
;; ObjMap storage representation is outside this dialect; preserve nominal hierarchy operations.
(def ObjMap PersistentArrayMap)



(defn run [observe]
(do
  (do
    (derive ::rect ::shape)
    (derive ::square ::rect)
    (assert (= #{:cljs.core-test/shape} (parents ::rect)))
    (assert (= #{:cljs.core-test/rect :cljs.core-test/shape} (ancestors ::square)))
    (assert (= #{:cljs.core-test/rect :cljs.core-test/square} (descendants ::shape)))
    (assert (true? (isa? 42 42)))
    (assert (true? (isa? ::square ::shape)))
    (derive ObjMap ::collection)
    (derive cljs.core.PersistentHashSet ::collection)
    (assert (true? (isa? ObjMap ::collection)))
    (assert (true? (isa? cljs.core.PersistentHashSet ::collection)))
    (assert (false? (isa? cljs.core.IndexedSeq ::collection)))
    (isa? js/String js/Object)
    (assert (true? (isa? [::square ::rect] [::shape ::shape])))
    ;; ?? (ancestors java.util.ArrayList)
    ;; ?? isa? based dispatch tests
    ))
true)

run
