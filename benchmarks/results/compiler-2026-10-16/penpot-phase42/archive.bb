(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str] '[clojure.edn :as edn])
(def here (fs/parent *file*))
(def production (fs/file here "production"))
(def evidence (fs/file "benchmarks/results/compiler-2026-10-16/penpot-phase42"))
(fs/create-dirs evidence)
(def results (mapv edn/read-string (str/split-lines (slurp (fs/file production "results.edn")))))
(assert (= 5 (count results)))
(assert (every? #(zero? (:exit %)) results))
(def counts (reduce (fn [totals [_ passed failed ignored]] (mapv + totals (mapv parse-long [passed failed ignored])))
  [0 0 0] (re-seq #"test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;"
    (slurp (fs/file production "tests.log")))))
(assert (= [1400 0 106] counts))
(def verification
  (apply str (for [job ["render" "main-none" "main-advanced"]]
    (let [file (fs/file production (str job ".js"))]
      (p/shell "cmp" (str file) (str "/private/tmp/light-cljs-phase33/production/" job ".js"))
      (p/shell {:out :string :err :string} "node" "--check" (str file))
      (str job ": complete output matches phase33 byte for byte and passes node --check.\n")))))
(spit (fs/file evidence "equivalence.txt") verification)
(doseq [name ["results.edn" "render.time.txt" "main-none.time.txt" "main-advanced.time.txt"
              "shadow-release.time.txt" "shadow-release.log" "shadow-compile.time.txt" "shadow-compile.log"
              "tests.log" "clippy.log" "build.log" "node-runtime.log" "node-helpers.log" "node-execution.log"]]
  (fs/copy (fs/file production name) (fs/file evidence name) {:replace-existing true}))
(doseq [file (concat (fs/glob here "*.bb") [(fs/file here "negative.rs") (fs/file here "negative.log") (fs/file here "negative-library.txt")])]
  (fs/copy file (fs/file evidence (fs/file-name file)) {:replace-existing true}))
(def port (fs/absolutize "research/penpot-light"))
(def files (sort-by str (filter fs/regular-file?
  (mapcat #(fs/glob (fs/file port %) "**") ["frontend/src" "common/src" "vendor" "generated" "shims"]))))
(def digest (java.security.MessageDigest/getInstance "SHA-256"))
(doseq [file files]
  (.update digest (.getBytes (str (fs/relativize (fs/absolutize ".") file)) "UTF-8"))
  (.update digest (fs/read-all-bytes file)))
(def port-hash (str (count files) " files\n" (format "%064x" (java.math.BigInteger. 1 (.digest digest))) "\n"))
(assert (= port-hash (slurp "benchmarks/results/compiler-2026-10-16/penpot-phase33/port-hash.txt")))
(spit (fs/file evidence "port-hash.txt") port-hash)
(def jobs (into {} (map (juxt :job identity) results)))
(defn seconds [job] (format "%.2f" (:wall-seconds (jobs job))))
(defn gb [job] (format "%.2f" (/ (:peak-rss-bytes (jobs job)) 1e9)))
(defn ratio [a b field] (/ (field (jobs a)) (field (jobs b))))
(def comparison
  (str "None remains **" (format "%.2f" (ratio "main-none" "shadow-compile" :wall-seconds))
    " times slower** than shadow dev at **" (format "%.0f" (* 100.0 (ratio "main-none" "shadow-compile" :peak-rss-bytes)))
    "% of its peak RSS**. Advanced is **" (format "%.2f" (ratio "main-advanced" "shadow-release" :wall-seconds))
    " times slower** than shadow release at **" (format "%.0f" (* 100.0 (ratio "main-advanced" "shadow-release" :peak-rss-bytes)))
    "% of its RSS**. The requested speed goal remains unmet."))
(def detail
  "Reader identity validation now resolves keyword/symbol names without inserting\nunused entries into executable literal tables. Actual lowering uses the same\nname-resolution helpers and still inserts its literals. Exact UTF-16 names,\nauto-resolution and generated semantic names retain their existing paths.\n\nA nine-key map previously retained 27 names and 27 strings; the regression\nnow requires nine of each. It also covers nested map/set keys, symbols and\nauto-resolved keywords in source and owned compiler forms. The frozen old\nlibrary reproduces the 27/27 negative control.\n\n")
(spit (fs/file evidence "README.md")
  (str "# Keep reader validation out of executable literal tables\n\n" detail
    "The allocation evidence is in phase41. QuickJS Function.call, native\nmembership memoization and the tested PGO configuration were rejected in\nphases38–40; they are not part of this implementation.\n\n"
    "The full suite passes **1,400 tests, zero failures, 106 ignored**. Strict\nClippy, formatting, diff checking and all three Node workflows pass.\n\n"
    "Fresh processes, cold shadow caches and one job at a time, after all\nCPU-heavy validation/build/profile work completed:\n\n"
    "| Job | Wall seconds | Peak RSS bytes | Output bytes |\n| --- | ---: | ---: | ---: |\n"
    (apply str (for [r results] (str "| " (:job r) " | " (:wall-seconds r) " | " (:peak-rss-bytes r) " | " (or (:output-bytes r) "—") " |\n")))
    "\n" comparison "\n\n"
    "The previous phase33 main-none run was 121.15 seconds / 1,172,799,488 bytes\nRSS. This phase's single main-none observation is faster but has higher peak\nRSS; it does not establish a whole-build memory reduction. The literal-table\nregression demonstrates eliminated retained entries independently of RSS.\n\n"
    "All three complete outputs match phase33 byte for byte and pass Node\nsyntax checking. The 996-file port fingerprint is unchanged. Penpot bundling\nand browser execution remain untested. Scope differences and timing/RSS\nvariability still apply: shadow main also includes render, rasterizer and\ncode-highlight; light-cljs main includes its five lazy modules.\n\n"
    "Frozen executable, tracked patch, essential untracked name-index source\nand large JavaScript outputs remain in /private/tmp/light-cljs-phase42/production.\n"))
(def report "benchmarks/penpot-investigation.md")
(let [text (str/replace (slurp report) "Subsequent diagnostics leave production at phase33."
             "Before phase42, the following diagnostics left production at phase33.")
      at (str/index-of text "Latest validated cold rerun (phase33)")]
  (assert at)
  (spit report
    (str (subs text 0 at)
      "Latest validated cold rerun (phase42): main none **" (seconds "main-none") " s / " (gb "main-none")
      " GB RSS**, main advanced **" (seconds "main-advanced") " s / " (gb "main-advanced")
      " GB**. Fresh shadow dev is **" (seconds "shadow-compile") " s / " (gb "shadow-compile")
      " GB**, release **" (seconds "shadow-release") " s / " (gb "shadow-release") " GB**.\n\n"
      detail "Full suite: **1,400 passed, zero failed, 106 ignored**. Strict Clippy,\nformatting and all three Node workflows pass. Render is **" (seconds "render") " s / " (gb "render") " GB**.\n"
      "All three complete outputs match phase33 byte for byte and pass syntax\nchecks; the port is unchanged. Penpot browser execution remains untested.\n\n"
      "Main none is faster than phase33's 121.15 s observation, but its peak RSS\nis higher than phase33's 1.17 GB. This run does not establish a whole-build\nmemory reduction; the regression checks demonstrate eliminated dead entries.\n\n"
      comparison "\n\nEvidence: `results/compiler-2026-10-16/penpot-phase42/`. Phase41's heap\nsnapshots found the discarded reader names; its live allocation totals are\nnot RSS measurements. The CLI already drops its host after analysis.\n\n"
      (str/replace-first (subs text at) "Latest validated cold rerun" "Previous validated cold rerun"))))
(spit (fs/file production "implementation.patch") (:out (p/shell {:out :string} "git" "diff" "--binary")))
(spit (fs/file evidence "source-hashes.txt")
  (str "HEAD " (str/trim (:out (p/shell {:out :string} "git" "rev-parse" "HEAD"))) "\n"
    (:out (p/shell {:out :string} "shasum" "-a" "256"
      (str (fs/file production "light-cljs")) (str (fs/file production "implementation.patch"))
      (str (fs/file production "name_index.rs")) "src/analyzer.rs" "tests/compiler/map_reader_order.rs"))))
(print verification)
(println comparison)
