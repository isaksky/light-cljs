;; This Source Code Form is subject to the terms of the Mozilla Public
;; License, v. 2.0. If a copy of the MPL was not distributed with this
;; file, You can obtain one at http://mozilla.org/MPL/2.0/.
;;
;; Copyright (c) Andrey Antukh <niwi@niwi.nz>
;;
;; Reduced admission fixture from promesa.util in the Penpot port.
;; The macro declaration is verbatim; unrelated definitions are omitted.

(ns admission.util (:refer-clojure :exclude [with-open]))

(defmacro with-open
  [bindings & body]
  {:pre [(vector? bindings)
         (even? (count bindings))
         (pos? (count bindings))]}

  (when (:ns &env)
    (throw (ex-info "cljs not supported on with-dispatch! macro" {})))

  (reduce (fn [acc bindings]
            `(let ~(vec bindings)
               (try
                 ~acc
                 (finally
                   (let [target# ~(first bindings)]
                     (if (instance? java.lang.AutoCloseable target#)
                       (.close ^java.lang.AutoCloseable target#)
                       (pt/-close target#)))))))
          `(do ~@body)
          (reverse (partition 2 bindings))))
