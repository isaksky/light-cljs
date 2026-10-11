(ns math-helpers)

(def greeting #?(:clj "Hello from native Rust" :cljs "Hello from compiled JavaScript"))

(defn square [x]
  (* x x))
