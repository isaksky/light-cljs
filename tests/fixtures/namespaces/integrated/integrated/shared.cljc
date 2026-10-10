(ns integrated.shared)

(def platform #?(:cljs :javascript :clj :jvm))
(def ^:dynamic *offset* 1)

(defprotocol Scale
  (scale [this factor]))

(defrecord Entry [id values]
  Scale
  (scale [_ factor] (* factor (reduce + values))))

(deftype Window [values]
  ISeqable
  (-seq [_] (seq values))
  ICounted
  (-count [_] (count values))
  ILookup
  (-lookup [_ key] (get values key))
  (-lookup [_ key missing] (get values key missing))
  IReduce
  (-reduce [_ f] (reduce f values))
  (-reduce [_ f initial] (reduce f initial values)))

(defn total
  ([values] (total 0 values))
  ([initial values & more]
   (reduce + initial (concat values (apply concat more)))))

(defn countdown [n]
  (loop [remaining n result 0]
    (if (zero? remaining)
      result
      (recur (dec remaining) (+ result remaining)))))
