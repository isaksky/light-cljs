;   Copyright (c) Rich Hickey. All rights reserved.
;   The use and distribution terms for this software are covered by the
;   Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;   which can be found in the file epl-v10.html at the root of this distribution.
;   By using this software in any fashion, you are agreeing to be bound by
;   the terms of this license.
;   You must not remove this notice, or any other, from this software.

; Adapted from ClojureScript a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e.
; Native macro host only; emits compiled JavaScript sequence operations.
(clojure.core/defmacro sequence-for [seq-exprs body-expr]
  (assert (and (vector? seq-exprs) (even? (count seq-exprs))) "for requires an even binding vector")
  (let [groups (reduce (fn [groups [k v]]
                        (if (keyword? k)
                          (conj (pop groups) (conj (peek groups) [k v]))
                          (conj groups [k v])))
                [] (partition 2 seq-exprs))
        emit-bind
        (fn emit-bind [[[bind expr & mods] & [[_ next-expr] :as more]]]
          (let [iter (gensym "iter") s (gensym "seq") i (gensym "index") b (gensym "buffer")
                nested (when more (emit-bind more))
                base (if more
                       `(let [nested# ~nested inner# (seq (nested# ~next-expr))]
                          (if inner# (concat inner# (~iter (rest ~s))) (recur (rest ~s))))
                       `(cons ~body-expr (~iter (rest ~s))))
                wrap (fn [body [k v]]
                       (cond (= k :let) `(let ~v ~body)
                             (= k :while) `(when ~v ~body)
                             (= k :when) `(if ~v ~body (recur (rest ~s)))
                             :else (throw (ex-info (str "Invalid 'for' keyword " k) {}))))
                normal (reduce wrap base (reduce conj () mods))
                chunk-base `(do (clojure.core/chunk-append ~b ~body-expr) (recur (inc ~i)))
                wrap-chunk (fn [body [k v]]
                             (cond (= k :let) `(let ~v ~body)
                                   (= k :while) `(when ~v ~body)
                                   (= k :when) `(if ~v ~body (recur (inc ~i)))
                                   :else (throw (ex-info (str "Invalid 'for' keyword " k) {}))))
                chunk-body (reduce wrap-chunk chunk-base (reduce conj () mods))]
            ;; Build child expansions before quoting the containing loop. This
            ;; keeps native macro evaluation depth independent of emitted depth.
            (if more
              `(fn ~iter [~s]
                 (lazy-seq (loop [~s ~s] (when-first [~bind ~s] ~normal))))
              `(fn ~iter [~s]
                 (lazy-seq
                   (loop [~s ~s]
                     (when-let [~s (seq ~s)]
                       (if (clojure.core/chunked-seq? ~s)
                         (let [c# (clojure.core/chunk-first ~s)
                               size# (count c#) ~b (clojure.core/chunk-buffer size#)]
                           (if (boolean
                                 (loop [~i 0]
                                   (if (< ~i size#)
                                     (let [~bind (-nth c# ~i)] ~chunk-body)
                                     true)))
                             (clojure.core/chunk-cons (clojure.core/chunk ~b)
                               (~iter (clojure.core/chunk-rest ~s)))
                             (clojure.core/chunk-cons (clojure.core/chunk ~b) nil)))
                         (let [~bind (first ~s)] ~normal)))))))))
        expansion (emit-bind groups)]
    `(let [iter# ~expansion] (iter# ~(second seq-exprs)))))

(clojure.core/defmacro sequence-doseq
  "Repeatedly executes body (presumably for side-effects) with
  bindings and filtering as provided by \"for\".  Does not retain
  the head of the sequence. Returns nil."
  [seq-exprs & body]
  (assert (and (vector? seq-exprs) (even? (count seq-exprs))) "doseq requires an even binding vector")
  (clojure.core/let [err (clojure.core/fn [& msg] (throw (ex-info (apply clojure.core/str msg) {})))
             step (clojure.core/fn step [recform exprs]
                    (clojure.core/if-not exprs
                      [true `(do ~@body nil)]
                      (clojure.core/let [k (first exprs)
                                 v (second exprs)

                                 seqsym (gensym "seq__")
                                 recform (if (clojure.core/keyword? k) recform `(recur (next ~seqsym) nil 0 0))
                                 steppair (step recform (nnext exprs))
                                 needrec (steppair 0)
                                 subform (steppair 1)]
                        (clojure.core/cond
                          (= k :let) [needrec `(let ~v ~subform)]
                          (= k :while) [false `(when ~v
                                                 ~subform
                                                 ~@(clojure.core/when needrec [recform]))]
                          (= k :when) [false `(if ~v
                                                (do
                                                  ~subform
                                                  ~@(clojure.core/when needrec [recform]))
                                                ~recform)]
                          (clojure.core/keyword? k) (err "Invalid 'doseq' keyword" k)
                          :else (clojure.core/let [chunksym (with-meta (gensym "chunk__")
                                                      {:tag 'not-native})
                                           countsym (gensym "count__")
                                           isym     (gensym "i__")
                                           recform-chunk  `(recur ~seqsym ~chunksym ~countsym (inc ~isym))
                                           steppair-chunk (step recform-chunk (nnext exprs))
                                           subform-chunk  (steppair-chunk 1)]
                                  [true `(loop [~seqsym   (seq ~v)
                                                ~chunksym nil
                                                ~countsym 0
                                                ~isym     0]
                                           (if (boolean (< ~isym ~countsym))
                                             (let [~k (-nth ~chunksym ~isym)]
                                               ~subform-chunk
                                               ~@(clojure.core/when needrec [recform-chunk]))
                                             (when-let [~seqsym (seq ~seqsym)]
                                               (if (clojure.core/chunked-seq? ~seqsym)
                                                 (let [c# (clojure.core/chunk-first ~seqsym)]
                                                   (recur (clojure.core/chunk-rest ~seqsym) c#
                                                     (count c#) 0))
                                                 (let [~k (first ~seqsym)]
                                                   ~subform
                                                   ~@(clojure.core/when needrec [recform]))))))])))))]
    (nth (step nil (seq seq-exprs)) 1)))

