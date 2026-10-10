;   Copyright (c) Rich Hickey. All rights reserved.
;   The use and distribution terms for this software are covered by the
;   Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;   which can be found in the file epl-v10.html at the root of this distribution.
;   By using this software in any fashion, you are agreeing to be bound by
;   the terms of this license.
;   You must not remove this notice, or any other, from this software.

(ns clojure.string
  (:refer-clojure :exclude [replace reverse])
  (:require [goog.string :as gstring])
  (:import [goog.string StringBuffer]))

(defn- replace-all
  [s re replacement]
  (let [r (js/RegExp. (.-source re)
                      (cond-> "g"
                        (.-ignoreCase re) (str "i")
                        (.-multiline re) (str "m")
                        (.-unicode re) (str "u")))]
    (.replace s r replacement)))

(defn join
  "Returns a string of all elements in coll, as returned by (seq coll),
  separated by an optional separator."
  ([coll]
   (loop [sb (StringBuffer.) coll (seq coll)]
     (if-not (nil? coll)
       (recur (. sb (append (str (first coll)))) (next coll))
       ^string (.toString sb))))
  ([separator coll]
   (loop [sb (StringBuffer.) coll (seq coll)]
     (if-not (nil? coll)
       (do
         (. sb (append (str (first coll))))
         (let [coll (next coll)]
           (when-not (nil? coll)
             (. sb (append separator)))
           (recur sb coll)))
       ^string (.toString sb)))))

(defn ^string upper-case
  "Converts string to all upper-case."
  [s]
  (.toUpperCase s))
