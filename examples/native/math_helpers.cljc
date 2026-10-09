(ns math-helpers)

(def greeting #?(:clj "Hello from native Rust" :cljs "JavaScript"))

(defn square [x]
  (* x x))
