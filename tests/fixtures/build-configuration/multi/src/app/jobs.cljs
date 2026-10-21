(ns app.jobs
  "What only the worker requires.")

(defn run
  "The result of `n` jobs."
  [n]
  (* n 21))
