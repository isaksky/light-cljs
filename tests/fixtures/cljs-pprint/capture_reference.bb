#!/usr/bin/env bb
;; Capture or verify the cljs.pprint differential corpus against Maven
;; ClojureScript 1.12.134 running under Node.
;;
;; Usage:
;;   bb tests/fixtures/cljs-pprint/capture_reference.bb          # --check (default)
;;   bb tests/fixtures/cljs-pprint/capture_reference.bb --check  # verify TSV matches 1.12.134
;;   bb tests/fixtures/cljs-pprint/capture_reference.bb --update # rewrite TSV expected_json column

(require '[babashka.fs :as fs]
         '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.string :as str])

(def script-dir (fs/parent *file*))
(def corpus-path (fs/path script-dir "differential-corpus.tsv"))
(def cljs-version "1.12.134")
(def result-prefix "__PPRINT_DIFF_RESULT__ ")

(defn parse-corpus [text]
  (let [lines (str/split-lines text)]
    {:lines lines
     :cases (into []
                  (keep-indexed
                   (fn [idx line]
                     (when-not (or (str/blank? line) (str/starts-with? line "#"))
                       (let [parts (str/split line #"\t" -1)]
                         (when (not= 3 (count parts))
                           (throw (ex-info (str "Expected 3 columns at line " (inc idx))
                                           {:line line})))
                         {:line-idx idx
                          :id (nth parts 0)
                          :expr (nth parts 1)
                          :expected (json/parse-string (nth parts 2))}))))
                  lines)}))

(defn build-reference-eval-source [cases]
  (let [entries (map (fn [{:keys [id expr]}]
                       (str "(js/console.log " (pr-str result-prefix)
                            " (js/JSON.stringify #js [" (pr-str id) " " expr "]))"))
                     cases)]
    (str "(require '[cljs.pprint :as pp :include-macros true] '[clojure.string :as str])\n"
         "(defrecord SampleRec [x y])\n"
         "(defn normalize-ref [s] (str/replace s #\"#<[^@]*@\\d+:\" \"#<REF@ID:\"))\n"
         (str/join "\n" entries) "\n")))

(defn run-upstream [cases]
  (let [src (build-reference-eval-source cases)
        deps (str "{:deps {org.clojure/clojurescript {:mvn/version \"" cljs-version "\"}}}")
        {:keys [exit out err]}
        (proc/sh ["clojure" "-Sdeps" deps "-M" "-m" "cljs.main" "-re" "node" "-e" src])]
    (when-not (zero? exit)
      (binding [*out* *err*]
        (println "ClojureScript" cljs-version "reference run failed with exit" exit)
        (println err))
      (System/exit exit))
    (let [results (into {}
                        (keep (fn [line]
                                (when (str/starts-with? line result-prefix)
                                  (let [payload (str/trim (subs line (count result-prefix)))
                                        [id actual] (json/parse-string payload)]
                                    [id actual]))))
                        (str/split-lines out))]
      (when (not= (count results) (count cases))
        (binding [*out* *err*]
          (println "Expected" (count cases) "results, got" (count results))
          (println out)
          (println err))
        (System/exit 1))
      results)))

(defn -main [& args]
  (let [mode (case (first args)
               nil :check
               "--check" :check
               "--update" :update
               (do
                 (binding [*out* *err*]
                   (println "Usage: bb capture_reference.bb [--check|--update]"))
                 (System/exit 2)))
        {:keys [lines cases]} (parse-corpus (slurp (str corpus-path)))
        actuals (run-upstream cases)]
    (case mode
      :update
      (let [by-idx (into {} (map (juxt :line-idx identity)) cases)
            new-lines (map-indexed
                       (fn [idx line]
                         (if-let [{:keys [id expr]} (get by-idx idx)]
                           (str id "\t" expr "\t" (json/generate-string (get actuals id)))
                           line))
                       lines)]
        (spit (str corpus-path) (str (str/join "\n" new-lines) "\n"))
        (println "Updated" (count cases) "cases from ClojureScript" cljs-version))

      :check
      (let [mismatches (filter (fn [{:keys [id expected]}]
                                 (not= expected (get actuals id)))
                               cases)]
        (if (empty? mismatches)
          (println "OK:" (count cases) "differential corpus cases match ClojureScript" cljs-version)
          (do
            (binding [*out* *err*]
              (println "FAIL:" (count mismatches) "mismatches against ClojureScript" cljs-version)
              (doseq [{:keys [id expected]} mismatches]
                (println "  case:" id)
                (println "    committed:" (pr-str expected))
                (println "    upstream: " (pr-str (get actuals id)))))
            (System/exit 1)))))))

(apply -main *command-line-args*)
