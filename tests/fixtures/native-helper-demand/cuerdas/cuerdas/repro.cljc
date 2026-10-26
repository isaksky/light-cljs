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

;; Reduced transducer reproduction. make-concat-xform and unsafe-concat
;; are verbatim; the external cuerdas/repeat helper uses string repetition.

(ns cuerdas.repro (:refer-clojure :exclude [repeat]) (:require [cljs.core :as c]))
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
(defmacro unsafe-concat
  "Unsafe variant of `concat`. Does not add checks on unknown symbols."
  [& params]
  (if (:ns &env)
    (let [xform  (make-concat-xform false)
          params (into [] xform params)
          stmpl  (reduce c/str "\"\"" (repeat "+~{}" (count params)))]
      (cons 'js* (cons stmpl params)))
    (cons `c/str params)))
(let [x "X"]
  (js/console.log (unsafe-concat "a" "b" x "c" "d")
                  (unsafe-concat) (unsafe-concat "a" "b")))
