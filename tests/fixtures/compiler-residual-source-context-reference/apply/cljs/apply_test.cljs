;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns cljs.apply-test)
(defn fn-returning-this
  [x]
  (this-as this
    this))
(def meta-f (with-meta (fn [& a] a) {}))
(defn run [] (do (do (assert (= 1 (apply js/parseInt ["1"]))) (assert (= 1 (apply js/parseInt "1" nil))) (assert (= 1 (apply js/parseInt "1" []))) (assert (= 15 (apply js/parseInt "F" [16]))) (let [source-context-value__ (identical? fn-returning-this (apply fn-returning-this [0])) source-context-message__ "apply should supply the this object to be the function itself"] (assert source-context-value__ source-context-message__)) true) (do (assert (= 1 (apply #{1} [1]))) (assert (= nil (apply #{1} [2]))) (assert (= 1 (apply #{1} 1 [2]))) (assert (= 2 (apply #{} 1 [2]))) (let [source-context-value__ (try (apply #{} []) false (catch js/Error sequence-error true)) source-context-message__ "We should still get wrong arity errors"] (assert source-context-value__ source-context-message__)) true) (do (assert (= nil (apply meta-f []))) (let [source-context-value__ (= '(1)) source-context-message__ (apply meta-f [1])] (assert source-context-value__ source-context-message__)) (let [source-context-value__ (= '(1)) source-context-message__ (apply meta-f 1 [])] (assert source-context-value__ source-context-message__)) (let [source-context-value__ (= '(1 2)) source-context-message__ (apply meta-f 1 2 [])] (assert source-context-value__ source-context-message__)) (let [source-context-value__ (= '(1 2 3)) source-context-message__ (apply meta-f 1 2 3 [])] (assert source-context-value__ source-context-message__)) (let [source-context-value__ (= '(1 2 3 4)) source-context-message__ (apply meta-f 1 2 3 4 [])] (assert source-context-value__ source-context-message__)) (let [source-context-value__ (= '(1 2 3 4 5)) source-context-message__ (apply meta-f 1 2 3 4 5 [])] (assert source-context-value__ source-context-message__)) (let [source-context-value__ (= (range 1 8)) source-context-message__ (apply meta-f 1 2 3 4 5 [6 7])] (assert source-context-value__ source-context-message__)) (let [source-context-value__ (= (range 21) (apply meta-f (range 21))) source-context-message__ "Should properly call the last IFn arity with 20 args with last being a seq"] (assert source-context-value__ source-context-message__)) (let [source-context-value__ (= (range 22) (apply meta-f (range 22))) source-context-message__ "Should properly call the last IFn arity with 20 args with last being a seq"] (assert source-context-value__ source-context-message__)) (let [source-context-value__ (= (range 22) (.apply meta-f nil (to-array (range 22)))) source-context-message__ ".apply should also handle >20 arguments"] (assert source-context-value__ source-context-message__)) (let [ctor #(.apply meta-f nil (js-arguments))] 
    (assert (= '(1 2 3) (.apply ctor nil #js [1 2 3])))
    (assert (= (range 30) (.apply ctor nil (to-array (range 30)))))) true) (do (assert (= 2 (apply (fn ([a] a) ([a b] b)) 1 [2]))) (assert (= 1 (apply (fn ([a] a) ([a b] b)) 1 []))) (assert (= 1 (apply (fn ([a] a) ([a b] b)) 1 nil))) (assert (try (apply (fn ([a] a) ([a b] b)) 1 2 3 nil) false (catch js/Error sequence-error true))) (assert (try (apply (fn ([a b] a)
                                 ([a b c] a)) [1]) false (catch js/Error sequence-error true))) true) (do (doseq [f [(fn [& r] r)
             (fn [a & r] (list* a r))
             (fn [a b & r] (list* a b r))
             (fn [a b c & r] (list* a b c r))
             (fn [a b c d & r] (list* a b c d r))
             (fn [a b c d e & r] (list* a b c d e r))]]
    (assert (= (range 10) (apply f (range 10))))
    (assert (= (range 10) (apply f 0 (range 1 10))))
    (assert (= (range 10) (apply f 0 1 (range 2 10))))
    (assert (= (range 10) (apply f 0 1 2 (range 3 10))))
    (assert (= (range 10) (apply f 0 1 2 3 (range 4 10))))
    (assert (= (range 10) (apply f 0 1 2 3 4 (range 5 10))))
    (assert (= (range 10) (apply f 0 1 2 3 4 5 (range 6 10))))) (let [source-context-value__ (nil? (apply (fn [a & b] b) [1])) source-context-message__ "rest should stay nil"] (assert source-context-value__ source-context-message__)) (let [source-context-value__ (nil? (apply (fn [a & b] b) 1 [])) source-context-message__ "rest should be nil'd"] (assert source-context-value__ source-context-message__)) (let [source-context-value__ (= '(2) (apply (fn [a & b] b) 1 [2])) source-context-message__ "rest should be nil'd"] (assert source-context-value__ source-context-message__)) (let [source-context-value__ (= (range 30)
         (apply (fn [_ _ _ _ _ a b c d e _ _ _ _ _ f g h i j k _ _ _ _ _ & R]
                  (let [a (array)]
                    (copy-arguments a)
                    (concat (.slice a 0 26) R)))
                (range 30))) source-context-message__ "Variadic function are not limited to 20 params"] (assert source-context-value__ source-context-message__)) true) (do (doseq [f [(fn ([]) ([& r] r))
             (fn ([a]) ([a & r] (list* a r)))
             (fn ([a]) ([a b & r] (list* a b r)))
             (fn ([a]) ([a b c & r] (list* a b c r)))
             (fn ([a]) ([a b c d & r] (list* a b c d r)))
             (fn ([a]) ([a b c d e & r] (list* a b c d e r)))]]
    (assert (= (range 10) (apply f (range 10))))
    (assert (= (range 10) (apply f 0 (range 1 10))))
    (assert (= (range 10) (apply f 0 1 (range 2 10))))
    (assert (= (range 10) (apply f 0 1 2 (range 3 10))))
    (assert (= (range 10) (apply f 0 1 2 3 (range 4 10))))
    (assert (= (range 10) (apply f 0 1 2 3 4 (range 5 10))))
    (assert (= (range 10) (apply f 0 1 2 3 4 5 (range 6 10))))) (assert (= 1 (apply (fn ([a] a) ([a & b] b)) [1]))) (assert (= '(2) (apply (fn ([a] a) ([a & b] b)) 1 [2]))) (assert (= 1 (apply (fn ([a] a) ([a & b] b)) 1 []))) (assert (= 1 (apply (fn ([a] a) ([a & b] b)) [1]))) (assert (= '(2 3 4 5 6) (apply (fn ([a] a) ([a & b] b)) 1 2 3 4 5 [6]))) (assert (= '(2 3 4 5) (apply (fn ([a] a) ([a & b] b)) 1 2 3 4 5 []))) (let [source-context-value__ (= (range 30)
         (apply (fn ([a])
                  ([_ _ _ _ _ a b c d e _ _ _ _ _ f g h i j k _ _ _ _ _ & R]
                   (let [a (array)]
                     (copy-arguments a)
                     (concat (.slice a 0 26) R))))
                (range 30))) source-context-message__ "Variadic function are not limited to 20 params"] (assert source-context-value__ source-context-message__)) true)))
