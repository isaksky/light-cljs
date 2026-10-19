#!/usr/bin/env bb
;; Usage: bb vendor/v8/ci-cache.bb CACHE-DIR TARGET-DIR
;; After a build, keeps in the V8 cache only the entries that build used or
;; published, so CI saves and hands on one engine, and prints their names.
;; Fails when the build named no complete entry.
(require '[babashka.fs :as fs] '[clojure.string :as str])
(let [[cache target] *command-line-args*
      _ (when-not (and cache target)
          (binding [*out* *err*] (println "usage: bb vendor/v8/ci-cache.bb CACHE-DIR TARGET-DIR"))
          (System/exit 2))
      ;; build.rs prints one of these lines into its build-script output.
      pattern #"(?m)^(?:using the cached patched V8 engine |cargo:warning=building the patched V8 from source for )(.+)$"
      used (->> (fs/glob target "*/build/v8-*/output")
                (mapcat #(map second (re-seq pattern (slurp (fs/file %)))))
                (map #(str (fs/file-name %)))
                distinct sort vec)]
  (when (empty? used)
    (binding [*out* *err*] (println "no V8 cache entry is named under" (str target "/*/build/v8-*/output")))
    (System/exit 1))
  (doseq [name used]
    (when-not (fs/exists? (fs/file cache name "key.txt"))
      (binding [*out* *err*] (println "the V8 cache entry" name "is missing from" cache))
      (System/exit 1)))
  (let [keep (set (mapcat (fn [name] [name (str name ".lock")]) used))]
    (doseq [path (fs/list-dir cache)
            :when (not (keep (str (fs/file-name path))))]
      (binding [*out* *err*] (println "removing stale" (str path)))
      (if (fs/directory? path) (fs/delete-tree path) (fs/delete path))))
  (println (str/join "\n" used)))
