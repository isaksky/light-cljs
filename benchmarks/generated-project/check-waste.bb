#!/usr/bin/env bb
(ns generated-project.check-waste
  (:require [babashka.fs :as fs]
            [clojure.edn :as edn]))

;; What an optimizer may do with each kind of marked payload:
;;   :removable     unreferenced definitions; an advanced build should drop them
;;   :best-effort   legal to drop, but only with arity or property analysis
;;   :retained      a load-time side effect on live state (defmethod); kept
;;   :compile-time  only in macro bodies; never emitted
;;   :never-loaded  outside the require graph; always an error if present

(defn scan [manifest path]
  (let [files (if (fs/directory? path)
                (sort (filter #(#{"js" "mjs" "cjs"} (fs/extension %)) (fs/glob path "**")))
                [path])
        _ (when (empty? files) (throw (ex-info "No JavaScript files found" {:path path})))
        ;; Scan one file at a time; don't retain hundreds of MB of dev output.
        found (reduce (fn [seen file]
                        (into seen (re-seq #"GENERATED_WASTE_[A-Z0-9_]+_END" (slurp (str file)))))
                      #{} files)]
    {:path (str path) :bytes (reduce + (map fs/size files))
     :survivors (filterv #(contains? found (:marker %)) (:waste-markers manifest))}))

(defn main [args]
  (let [strict? (some #{"--strict"} args)
        paths (remove #{"--strict"} args)]
    (when (empty? paths)
      (throw (ex-info "Usage: bb check-waste.bb [--strict] JS-FILE-OR-DIRECTORY ..." {})))
    (let [manifest (edn/read-string (slurp (fs/file (fs/parent (fs/absolutize *file*)) "manifest.edn")))
          totals (frequencies (map :category (:waste-markers manifest)))
          results (mapv #(scan manifest %) paths)]
      (doseq [{:keys [path bytes survivors]} results]
        (println (format "%s: %d bytes, %d/%d waste markers retained"
                         path bytes (count survivors) (count (:waste-markers manifest))))
        (let [kept (frequencies (map :category survivors))]
          (doseq [[category {:keys [expect]}] (sort-by key (into {} (map (juxt :category identity) (:waste-markers manifest))))]
            (println (format "  %-14s %-13s %d/%d" (name category) (name expect) (get kept category 0) (totals category)))))
        (when (some #(= :never-loaded (:expect %)) survivors)
          (println "  ERROR: a namespace outside the entry graph is present"))
        (when (and strict? (some #(= :removable (:expect %)) survivors))
          (println "  STRICT: removable markers survived")))
      (when (or (some (fn [r] (some #(= :never-loaded (:expect %)) (:survivors r))) results)
                (and strict? (some (fn [r] (some #(= :removable (:expect %)) (:survivors r))) results)))
        (System/exit 1)))))

(when (= (fs/absolutize *file*)
         (some-> (System/getProperty "babashka.file") fs/absolutize))
  (main *command-line-args*))
