;; Copyright (c) Andrey Antukh <niwi@niwi.nz>
;; All rights reserved.
;;
;; Redistribution and use in source and binary forms, with or without
;; modification, are permitted provided that the following conditions are met:
;;
;; * Redistributions of source code must retain the above copyright notice, this
;;   list of conditions and the following disclaimer.
;;
;; * Redistributions in binary form must reproduce the above copyright notice,
;;   this list of conditions and the following disclaimer in the documentation
;;   and/or other materials provided with the distribution.
;;
;; THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
;; AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
;; IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
;; DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
;; FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
;; DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
;; SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
;; CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
;; OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
;; OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

;; Reduced Penpot ffmt reproduction. concat, interpolate-ffmt and ffmt
;; are verbatim; the external repeat helper uses string repetition.

(ns cuerdas.core (:refer-clojure :exclude [repeat concat]) (:require [cljs.core :as c] [clojure.string :as str] [cljs.reader :as edn]))
;; The port's cuerdas/repeat repeats a string (s, n), unlike core/repeat.
(defn repeat [s n] (.repeat s n))
(defn- make-concat-xform
     [safe?]
     (comp (partition-by string?)
           (filter some?)
           (mapcat (fn [part]
                     (if (string? (first part))
                       [(apply c/str part)]
                       (if safe?
                         (map (fn [o] (list 'js* "(~{} ?? \"\")" o)) part)
                         (map (fn [o] (list 'js* "(~{})" o)) part)))))))
(defmacro concat
  "A macro variant of the clojure.core/str function that performs
  considerably faster string concatenation operation on CLJS (on
  JVM/CLJ it only applies basic simplification and then relies on the
  `clojure.core/str`)."
  [& params]
  (if (:ns &env)
    (let [xform  (make-concat-xform true)
          params (into [] xform params)
          stmpl  (reduce c/str "\"\"" (repeat "+~{}" (count params)))]
      (cons 'js* (cons stmpl params)))
    (cons `c/str params)))

(defn- interpolate-ffmt
     [s params]
     (loop [items  (->> (re-seq #"([^\%]+)*(\%{1,2}(\d+)?)?" s)
                        (remove (fn [[full seg]] (and (nil? seg) (not full)))))
            result []
            index  0]
       ;; (prn "interpolate-ffmt" items)
       (if-let [[full segment var? sidx] (first items)]
         (cond
           (and var? (= "%%" var?))
           (recur (rest items)
                  (conj result (str/replace full "%%" "%"))
                  index)

           (and var? sidx)
           (let [cidx (dec (edn/read-string sidx))]
             (recur (rest items)
                    (-> result
                        (conj segment)
                        (conj (nth params cidx)))
                    (inc index)))

           var?
           (recur (rest items)
                  (-> result
                      (conj segment)
                      (conj (nth params index)))
                  (inc index))

           :else
           (recur (rest items)
                  (conj result segment)
                  (inc index)))

         (remove nil? result))))

(defmacro ffmt
  "Alternative (to `istr`) string formating macro, that performs simple
  string formating on the compile time (this means that the string
  should be known at compile time). Internally it uses the fast string
  concatenation mechanism implemented in the `concat` macro.

  If you don't need the peculiarities of the `istr` macro, this macro
  should be prefered.

  It works with two basic forms: sequencial and indexed. Let seen an
  example:

    (str/ffmt \"url(%)\" my-url) ; sequential
    (str/ffmt \"url(%1)\" my-url) ; indexed
  "
  [s & params]
  (cons 'cuerdas.core/concat (interpolate-ffmt s (vec params))))
