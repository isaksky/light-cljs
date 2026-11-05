#!/usr/bin/env bb
(require '[babashka.fs :as fs] '[babashka.process :as p]
         '[clojure.edn :as edn] '[clojure.string :as str] '[cheshire.core :as json])
(import '[java.util.concurrent TimeUnit])
(def directory (fs/parent (fs/canonicalize *file*)))
(def root (fs/parent directory))
(def repository (fs/canonicalize (fs/path directory "../../..")))
(def goose (or (System/getenv "GOOSE") "goose"))
(doseq [tool [goose "bb" "cargo" "node"]]
  (when-not (fs/which tool) (throw (ex-info (str "Missing required tool: " tool) {}))))
(assert (empty? *command-line-args*) "Usage: bb correctness.bb")
(def temporary (fs/create-temp-dir {:dir (str directory) :prefix ".goose-correctness-"}))
(def environment (assoc (into {} (System/getenv)) "CARGO_BUILD_JOBS" "3" "LIGHT_CLJS_V8_CACHE" "require"
                        "CARGO_TARGET_DIR" (str (fs/path repository "target"))
                        "GOOSE_CORRECTNESS_INPUTS" (str temporary)))
;; Remember descendants while the parent lives; reparenting on failure must not
;; make a compiler session disappear from cleanup. Deadlines are watchdogs only.
(defn run! [seconds capture? & command]
  (println "+" (str/join " " command)) (flush)
  (let [proc (p/process (vec command) {:dir (str repository) :env environment
                                     :out (if capture? :string :inherit) :err :inherit})
        handle (.toHandle (:proc proc)) owned (atom {(.pid handle) handle})
        until (+ (System/nanoTime) (* seconds 1000000000))]
    (try
      (loop []
        (with-open [descendants (.descendants handle)]
          (doseq [child (.toArray descendants)] (swap! owned assoc (.pid child) child)))
        (let [result (deref proc 20 ::waiting)]
          (cond
            (not= result ::waiting) (do (assert (zero? (:exit result)) (str "Infrastructure/command failure: " command " exit=" (:exit result))) (:out result))
            (> (System/nanoTime) until) (throw (ex-info (str "Correctness watchdog expired after " seconds "s: " command) {}))
            :else (recur))))
      (finally
        (doseq [child (reverse (vals @owned)) :when (.isAlive child)] (.destroyForcibly child))
        (doseq [child (vals @owned) :when (.isAlive child)] (.get (.onExit child) 5 TimeUnit/SECONDS))
        (assert (every? #(not (.isAlive %)) (vals @owned)) "Owned process survived cleanup")))))
;; v1: closed numeric expressions, distinct binder IDs, depth <= 3, <= 40
;; expression nodes. Alpha changes binders/references together, and identity
;; applies its argument once. Observations are ordered writes to one named slot.
(def seeds [0 1 17 42 99 255 1024 24301])
(defn generated [seed]
  (let [state (atom (long seed)) next-id (atom 0)
        choose (fn [n] (swap! state #(bit-and 4294967295 (+ (* 1664525 %) 1013904223))) (mod @state n))]
    (letfn [(expr [depth scope]
              (if (zero? depth)
                (if (and (seq scope) (zero? (choose 2))) [:local (nth scope (choose (count scope)))] [:literal (str (- (choose 17) 8))])
                (case (choose 4)
                  0 [:add (expr (dec depth) scope) (expr (dec depth) scope)]
                  1 (let [id (swap! next-id inc)] [:let id (expr (dec depth) scope) (expr (dec depth) (conj scope id))])
                  2 [:if (expr (dec depth) scope) (expr (dec depth) scope) (expr (dec depth) scope)]
                  [:observe (expr (dec depth) scope)])))
            (emit [tree alpha?]
              (let [[op a b c] tree name #(str (if alpha? "renamed" "local") %)]
                (case op :literal a :local (name a)
                  :add (str "(" (emit a alpha?) "+" (emit b alpha?) ")")
                  :let (str "((" (name a) ")=>" (emit c alpha?) ")(" (emit b alpha?) ")")
                  :if (str "(" (emit a alpha?) "?" (emit b alpha?) ":" (emit c alpha?) ")")
                  :observe (str "observe(" (emit a alpha?) ")"))))]
      (let [tree (expr 3 [])]
        (for [variant [:original :alpha :identity]
              :let [body (emit tree (= variant :alpha)) body (if (= variant :identity) (str "(once=>once)(" body ")") body)]]
          {:id (str "seed-" seed "-" (name variant)) :seed seed :version 1 :variant variant
           :source (str "function generated(){function observe(value){globalThis.observed=value;return value;}return " body ";}")})))))
(def cases (vec (concat (mapcat generated seeds)
                       (map-indexed (fn [i body] {:id (str "typed-" i) :version 1 :source (str "function typed(){" body "}")})
                         ["return -0;" "return 0/0;" "return 1/0;" "return -1/0;" "return 'é😀\\ud800';"
                          "return '\\u00e9\\ud83d\\ude00\\ud800';" "return '';" "return undefined;" "return null;"
                          "globalThis.observed=1;globalThis.observed=-0;return 7;"
                          "globalThis.observed=1;globalThis.observed=2;return null.x;"
                          "globalThis.observed=1;return missing;" "return true;" "throw -0;" "throw 0/0;" "throw 1/0;" "throw 'é😀\\ud800';"
                          "throw undefined;" "throw null;" "throw false;" "globalThis.observed=2;throw 7;"]))))
(def reference-js
  "const fs=require('fs'),vm=require('vm');const rows=JSON.parse(fs.readFileSync(process.argv[1],'utf8'));const canonical=v=>{if(v===undefined)return 'undefined';if(v===null)return 'null';if(typeof v==='number')return 'number:'+(Object.is(v,-0)?'-0':String(v));if(typeof v==='string')return 'utf16:['+Array.from({length:v.length},(_,i)=>v.charCodeAt(i)).join(',')+']';if(typeof v==='boolean')return 'boolean:'+v;throw Error('unsupported outcome')};console.log(JSON.stringify(rows.map(row=>{const effects=[],stdout=[];const context={console:{log:(...v)=>stdout.push(v.join(' '))}};Object.defineProperty(context,'observed',{set:v=>effects.push(v)});let status='returned',raw;try{raw=vm.runInNewContext('('+row.source+')()',context,{timeout:1000});}catch(e){status='thrown';raw=e;}let value;if(status==='thrown'&&require('util').types.isNativeError(raw)){if(!['TypeError','ReferenceError'].includes(raw.name))throw raw;value='error:'+raw.name;}else value=canonical(raw);return {id:row.id,status,value,effects:effects.map(canonical),stdout:stdout.join('')};})))")
(defn read-one! [text]
  (with-open [reader (java.io.PushbackReader. (java.io.StringReader. text))]
    (let [value (edn/read {:eof ::eof} reader)]
      (assert (and (not= value ::eof) (= ::eof (edn/read {:eof ::eof} reader))) "Malformed/extra protocol output")
      value)))
(defn value! [value]
  (assert (string? value) "Missing typed value")
  (cond (str/starts-with? value "number:")
        (let [text (subs value 7) n (case text "NaN" Double/NaN ("Infinity" "inf") Double/POSITIVE_INFINITY ("-Infinity" "-inf") Double/NEGATIVE_INFINITY (Double/parseDouble text))]
          [:number (Double/doubleToLongBits n)])
        (str/starts-with? value "utf16:")
        (let [units (read-one! (subs value 6))]
          (assert (and (vector? units) (every? #(and (integer? %) (<= 0 % 65535)) units)) "Malformed UTF16 outcome")
          [:utf16 units])
        (#{"undefined" "null" "boolean:true" "boolean:false" "error:TypeError" "error:ReferenceError"} value) value
        :else (throw (ex-info "Unsupported/malformed typed outcome" {:value value}))))
(defn outcomes! [rows]
  (assert (= (count rows) (count cases)) "Missing/duplicate outcomes")
  (assert (= (mapv :id cases) (mapv :id rows)) "Missing/duplicate/reordered outcome IDs")
  (mapv (fn [{:keys [id status value effects stdout]}]
          (assert (#{"returned" "thrown"} status) "Missing/invalid status")
          (assert (and (vector? effects) (string? stdout)) "Missing effects/stdout")
          [id status (value! value) (mapv value! effects) stdout]) rows))
(defn compare! [expected actual] (assert (= (outcomes! expected) (outcomes! actual)) "Typed outcome/effect/stdout mismatch"))
(try
  (doseq [row cases] (spit (str (fs/path temporary (str (:id row) ".js"))) (:source row)))
  (spit (str (fs/path temporary "inputs.json")) (json/generate-string cases))
  (println "Generated manifest v1, seeds" seeds "rows" (count cases) "(retained with source on failure)")
  (println "LAYER source capture/export and independent V8/Pants expectations")
  (run! 900 false "bb" (str (fs/path directory "regenerate.bb")) "--check")
  (run! 300 false "cargo" "test" "--locked" "-p" "big-boy-pants-js" "--lib" "frontend::goose::serial_tests::generated_sources_for_goose" "--" "--ignored" "--exact")
  (let [reference (json/parse-string (run! 30 true "node" "-e" reference-js (str (fs/path temporary "inputs.json"))) true)]
    (compare! reference reference)
    (doseq [bad ["" "{} {}" "{:status"]]
      (assert (try (read-one! bad) false (catch Throwable _ true)) "Malformed protocol passed"))
    (doseq [mutate [#(subvec % 1) #(assoc % 1 (first %)) #(vec (reverse %)) #(assoc-in % [0 :status] "thrown")
                    #(assoc-in % [0 :value] "number:999") #(update-in % [34 :effects] (comp vec reverse))
                    #(assoc-in % [0 :stdout] "unexpected") #(assoc-in % [0 :effects] nil)]]
      (assert (try (compare! reference (mutate reference)) false (catch Throwable _ true)) "Comparator accepted corrupt outcome"))
    (doseq [mode [["--debug"] []]]
      (apply run! 180 false goose "build" (str (fs/path root "src/execute.goose")) (concat mode ["-o" (str (fs/path temporary "execute"))]))
      (let [actual (mapv (fn [row]
                           (let [result (read-one! (run! 10 true (str (fs/path temporary "execute")) (str (fs/path temporary (str (:id row) ".gsp"))) "--outcome"))]
                             (assert (= #{:status :value :effects :stdout} (set (keys result))) "Malformed outcome record")
                             (assoc result :id (:id row) :status (name (:status result))))) cases)]
        (compare! reference actual)
        (doseq [rows (partition 3 (take 24 actual))] (assert (apply = (map #(dissoc % :id) rows)) "Generated metamorphic variant changed semantics"))
        (println "LAYER generated + typed V8/Goose outcomes" mode "PASS" (count actual)))))
  (println "LAYER serial source-derived macros: owned forms/provenance/identity/counts")
  (run! 300 false "bb" (str (fs/path directory "macros.bb")))
  (println "LAYER actual compiler adapter: V8/Pants/Goose workers 1/2/4/8")
  (run! 900 false "bb" (str (fs/path directory "compiler.bb")))
  (run! 300 false "cargo" "test" "--locked" "--no-default-features" "--features" "quickjs,goose-macros" "--lib" "goose_compiler_bridge")
  (fs/delete-tree temporary)
  (println "Bounded correctness PASS; all owned processes reaped")
  (catch Throwable error
    (binding [*out* *err*] (println "FAIL: reproduction inputs/seeds/version/source retained at" (str temporary)))
    (throw error))
  (finally
    (fs/delete-if-exists (fs/path temporary "execute"))
    (fs/delete-tree (fs/path temporary "execute.dSYM"))))
