;; This Source Code Form is subject to the terms of the Mozilla Public
;; License, v. 2.0. If a copy of the MPL was not distributed with this
;; file, You can obtain one at http://mozilla.org/MPL/2.0/.
;;
;; Copyright (c) KALEIDOS SUBSIDIARY SL
;;
;; Reduced admission fixture from Penpot app.common.record.
;; The macro declaration is verbatim; unrelated definitions are omitted.

(ns admission.record (:refer-clojure :exclude [defrecord]))
(declare emit-impl-js emit-impl-jvm property-symbol generate-field-access)

(defmacro defrecord
  [rsym fields & impls]
  (let [param   (gensym "param-")
        ks      (map keyword fields)
        fields' (mapv #(with-meta % nil) fields)
        nsname  (if (:ns &env)
                  (-> &env :ns :name)
                  (str *ns*))
        ident   (str "#" nsname "." (name rsym))]

    `(do
       (deftype ~rsym ~(into fields ['$meta '$extmap '$hash])
         ~@(if (:ns &env)
             (emit-impl-js rsym fields')
             (emit-impl-jvm rsym fields'))
         ~@impls

         ~@(when (:ns &env)
             ['cljs.core/IPrintWithWriter
              `(~'-pr-writer [~'this writer# opts#]
                             (let [pr-pair# (fn [keyval#]
                                              (cljs.core/pr-sequential-writer writer# (~'js* "cljs.core.pr_writer")
                                                                              "" " " "" opts# keyval#))]
                               (cljs.core/pr-sequential-writer
                                writer# pr-pair# ~(str ident "{") ", " "}" opts#
                                (concat [~@(for [f fields']
                                             `(vector ~(keyword f) (. ~'this ~(property-symbol f))))]
                                        (. ~'this ~'-$extmap)))))]))

       ~@(when-not (:ns &env)
           [`(defmethod print-method ~rsym [o# ^java.io.Writer w#]
               (.write w# ~(str "#" nsname "." (name rsym)))
               (print-method (into {} o#) w#))])

       (defn ~(with-meta (symbol (str "pos->" rsym))
                (assoc (meta rsym) :factory :positional))
         [~@fields']
         (new ~rsym ~@(conj fields nil nil nil)))

       (defn ~(with-meta (symbol (str 'map-> rsym))
                (assoc (meta rsym) :factory :map))
         [~param]
         (let [exclude# #{~@ks}
               extmap#  (reduce-kv (fn [acc# k# v#]
                                     (if (contains? exclude# k#)
                                       acc#
                                       (assoc acc# k# v#)))
                                   {}
                                   ~param)]
           (new ~rsym
                ~@(for [k ks]
                    `(get ~param ~k))
                nil
                (not-empty extmap#)
                nil)))
       ~rsym)))
