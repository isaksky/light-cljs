#!/usr/bin/env bb
(require '[babashka.fs :as fs]
         '[babashka.process :as p]
         '[clojure.edn :as edn]
         '[clojure.pprint :as pp]
         '[clojure.string :as str])
(import '[java.security MessageDigest]
        '[java.nio ByteBuffer]
        '[java.nio.charset StandardCharsets CodingErrorAction])

(def directory (fs/parent (fs/canonicalize *file*)))
(def repository (fs/path directory "../../../.."))
(def check? (= ["--check"] (vec *command-line-args*)))
(assert (or check? (empty? *command-line-args*)) "Usage: bb regenerate.bb [--check]")
(defn run! [& args]
  (println "+" (str/join " " args)) (flush)
  (apply p/shell {:dir (str repository)
                  :env (cond-> (assoc (dissoc (into {} (System/getenv)) "GOOSE_REGENERATE")
                                     "CARGO_BUILD_JOBS" "3" "LIGHT_CLJS_V8_CACHE" "require")
                         (not check?) (assoc "GOOSE_REGENERATE" "1"))} args))
(defn id! [id n] (assert (and (integer? id) (<= 0 id) (< id n) (<= id 4294967295)) (str "Invalid ID " id " / " n)))
(defn range! [[start len] n]
  (assert (and (integer? start) (integer? len) (<= 0 start n) (<= 0 len (- n start))) "Invalid payload range"))
(defn utf8! [bytes]
  (str (.decode (doto (.newDecoder StandardCharsets/UTF_8)
                  (.onMalformedInput CodingErrorAction/REPORT)
                  (.onUnmappableCharacter CodingErrorAction/REPORT))
                (ByteBuffer/wrap (byte-array (map unchecked-byte bytes))))))
(defn span! [sources source [start end]]
  (id! source (count sources))
  (let [bytes (:utf8 (sources source)) n (count bytes)
        boundary? #(or (= % n) (not= 128 (bit-and 192 (bytes %))))]
    (assert (and (<= 0 start end n) (boundary? start) (boundary? end)) "Invalid source byte span")))
(defn forms! [{:keys [root nodes children]} sources data]
  (id! root (count nodes))
  (doseq [node nodes]
    (span! sources (:source node) (:span node))
    (doseq [key [:metadata :payload] :let [id (key node)] :when (some? id)] (id! id (count nodes)))
    (if (= :scalar (:kind node)) (id! (:datum node) (count (:nodes data)))
        (let [[start len :as r] (:children node)] (range! r (count children))
          (doseq [id (subvec children start (+ start len))] (id! id (count nodes)))))))
(defn jobs! [path]
  (let [{:keys [version sources data definitions jobs helper-factories declared-effects]} (edn/read-string (slurp (str path)))
        {:keys [nodes children units]} data]
    (assert (= 2 version)) (assert (<= (count nodes) 100000))
    (doseq [source sources] (utf8! (:utf8 source)))
    (doseq [unit units] (assert (<= 0 unit 65535)))
    (doseq [node nodes]
      (doseq [key [:metadata :payload] :let [id (key node)] :when (some? id)] (id! id (count nodes)))
      (doseq [key [:units :name :namespace] :let [r (key node)] :when r] (range! r (count units)))
      (when-let [[start len :as r] (:children node)]
        (range! r (count children)) (doseq [id (subvec children start (+ start len))] (id! id (count nodes)))))
    (doseq [definition definitions]
      (span! sources (:source definition) (:span definition))
      (assert (fs/exists? (fs/path directory "corpus" (:factory definition)))))
    (doseq [[index job] (map-indexed vector jobs)]
      (assert (= index (:id job)))
      (doseq [key [:call :environment :expected-expansion]] (id! (key job) (count nodes)))
      (forms! (:input-forms job) sources data) (forms! (:expected-output-forms job) sources data)
      (doseq [id (:arguments job)] (id! id (count (get-in job [:input-forms :nodes]))))
      (doseq [key [:name :namespace] :let [r (get-in job [:macro key])] :when r] (range! r (count units))))
    (assert (<= 0 helper-factories 16))
    (assert (every? #{:compiler-gensym} declared-effects))))

(run! "cargo" "test" "--locked" "--features" "bbp-macros" "--lib"
      "compiler_bootstrap::goose_capture::" "--" "--test-threads=1")
(run! "cargo" "test" "--locked" "-p" "big-boy-pants-js" "--lib" "frontend::goose::" "--" "--test-threads=3")
(doseq [path (sort (fs/glob (fs/path directory "corpus") "*.jobs.edn"))] (jobs! path))
(let [paths (sort-by str (remove #(= "manifest.edn" (str (fs/file-name %)))
                               (fs/glob (fs/path directory "corpus") "**" {:hidden true})))
      sha (fn [path] (format "%064x" (BigInteger. 1 (.digest (MessageDigest/getInstance "SHA-256") (fs/read-all-bytes path)))))
      inputs ["src/compiler_bootstrap.rs" "src/emitter.rs" "src/macro_host.rs" "src/js_runtime.js" "src/bbp_runtime.js" "src/bbp_reader.js"
              "crates/big-boy-pants-js/src/frontend.rs" "crates/big-boy-pants-js/src/frontend/lower.rs"
              "crates/big-boy-pants-js/src/vm/instr.rs"
              "crates/big-boy-pants-js/prototypes/goose/export.rs"
              "crates/big-boy-pants-js/prototypes/goose/capture.rs"
              "crates/big-boy-pants-js/prototypes/goose/regenerate.bb"]
      manifest (array-map :version 2 :compiler-base-revision "49b178ecbf739b276eb93a9d350facad09e50505"
                          :historical-synthetic-revision "7ba4bd6fd5b08d37bc7c51d980632f004697ff6d" :regenerate "bb crates/big-boy-pants-js/prototypes/goose/regenerate.bb"
                          :source-sha256 (into (sorted-map) (for [path inputs] [path (sha (fs/path repository path))]))
                          :artifact-sha256 (into (sorted-map) (for [path paths :when (fs/regular-file? path)]
                                                              [(str (fs/relativize directory path)) (sha path)])))
      text (with-out-str (pp/pprint manifest)) path (fs/path directory "corpus/manifest.edn")]
  (if check? (assert (= text (slurp (str path))) "Corpus manifest differs; regenerate it") (spit (str path) text)))
(run! "bb" (str (fs/path directory "smoke.bb")))
(println "Source-derived corpus, independent V8/Pants references, flat metadata, hashes, and Goose decode verified")
