(ns npm.missing
  (:require [host.packages] [clojure.string :as str]
            ["left-pad" :as left-pad] ["react" :as r]))

(defn failure [f]
  (try (f) nil (catch :default e e)))

(defn names-package-and-registry? [error package]
  (and (instance? js/ReferenceError error)
       (str/includes? (.-message error) (str "\"" package "\""))
       (str/includes? (.-message error) "globalThis.lightCljsPackages")))

(defn run [observe]
  (assert (names-package-and-registry? (failure #(left-pad/default "x" 3)) "left-pad"))
  (let [registry js/lightCljsPackages]
    (set! js/lightCljsPackages nil)
    (try
      (assert (names-package-and-registry? (failure #(r/createElement "div")) "react"))
      (finally (set! js/lightCljsPackages registry))))
  (assert (= [:element "div"] (r/createElement "div")))
  true)
run
