(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str] '[clojure.edn :as edn])
(def here (fs/parent *file*))
(def heap (fs/file here "heap"))
(def evidence (fs/file "benchmarks/results/compiler-2026-10-16/penpot-phase42"))
(def destination (fs/file evidence "heap"))
(fs/create-dirs destination)
(p/shell "cmp" (str (fs/file heap "render.js")) (str (fs/file here "production/render.js")))
(p/shell "node" "--check" (str (fs/file heap "render.js")))
(doseq [file (concat (fs/glob heap "*.bb") (fs/glob heap "*.log") (fs/glob heap "*-summary.edn") [(fs/file heap "result.edn")])]
  (fs/copy file (fs/file destination (fs/file-name file)) {:replace-existing true}))
(doseq [file (fs/glob here "*.bb")]
  (fs/copy file (fs/file evidence (fs/file-name file)) {:replace-existing true}))
(defn summary [root phase] (edn/read-string (slurp (fs/file root (str phase "-summary.edn")))))
(def baseline "/private/tmp/light-cljs-phase41")
(doseq [phase ["before" "after"]]
  (assert (zero? (get-in (summary heap phase) [:categories :reader-identity :bytes] 0))))
(assert (= (get-in (summary baseline "before") [:categories :quickjs])
           (get-in (summary heap "before") [:categories :quickjs])))
(def note
  "The separate live-allocation comparison of render confirms reduced retained\ndata: 312,381,712 to 290,492,640 bytes after analysis (7.0% less), and\n104,108,000 to 94,240,688 bytes after releasing the macro session (9.5% less).\nThe reader-identity allocation category disappears; the classified QuickJS\nallocation total is unchanged. These are live allocator sizes, not RSS.\nThe instrumented output also matches the validated output exactly.\n")
(spit (fs/file destination "README.md")
  (str "# Compare live render allocations after the reader-identity fix\n\n" note
    "\n| Snapshot | Old records | New records | Old bytes | New bytes |\n| --- | ---: | ---: | ---: | ---: |\n"
    (apply str (for [phase ["before" "after"] :let [old (:total (summary baseline phase)) new (:total (summary heap phase))]]
      (str "| " phase " | " (:calls old) " | " (:calls new) " | " (:bytes old) " | " (:bytes new) " |\n")))
    "\nUses the same macOS live-stack diagnostic as phase41, after all clean\nbenchmarks completed. Its time/footprint is not a benchmark. Raw snapshots\nand frozen diagnostic executable remain under /private/tmp/light-cljs-phase42/heap.\nThe classifier's priority and ownership limitations remain those of phase41.\n"))
(spit (fs/file destination "equivalence.txt") "Complete instrumented render output matches validated phase42 output and passes node --check.\n")
(spit (fs/file destination "source-hashes.txt")
  (:out (p/shell {:out :string} "shasum" "-a" "256"
    (str (fs/file heap "light-cljs")) (str (fs/file heap "before-heap.txt")) (str (fs/file heap "after-heap.txt"))
    (str (fs/file baseline "light-cljs")) (str (fs/file baseline "before-heap.txt")) (str (fs/file baseline "after-heap.txt")))))
(spit (fs/file evidence "README.md") (str "\n" note
  "\nThe archived prepare.bb now requires LIGHT_BASELINE_LIBRARY explicitly.\nThe original negative-control library path was in a reused target directory;\nthat directory was subsequently rebuilt. Its already linked negative executable\nremains frozen under /private/tmp/light-cljs-phase42 and is fingerprinted.\n") :append true)
(def report "benchmarks/penpot-investigation.md")
(let [text (slurp report) anchor "None remains **3.86 times slower**"]
  (assert (str/includes? text anchor))
  (spit report (str/replace-first text anchor (str note "\n" anchor))))
(spit (fs/file here "production/implementation.patch") (:out (p/shell {:out :string} "git" "diff" "--binary")))
(spit (fs/file evidence "source-hashes.txt")
  (str "HEAD " (str/trim (:out (p/shell {:out :string} "git" "rev-parse" "HEAD"))) "\n"
    (:out (p/shell {:out :string} "shasum" "-a" "256"
      (str (fs/file here "production/light-cljs")) (str (fs/file here "production/implementation.patch"))
      (str (fs/file here "production/name_index.rs")) (str (fs/file here "negative"))
      "src/analyzer.rs" "tests/compiler/map_reader_order.rs"))))
(println note)
