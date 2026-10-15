(ns app.state)

(def mode :plain)

(def ^:dynamic *depth* 0)

(def fixed 1)

(defn depth [] *depth*)

(defn fixed-value [] fixed)
