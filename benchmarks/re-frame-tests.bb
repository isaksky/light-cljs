#!/usr/bin/env bb
;; Runs re-frame's own ClojureScript test suite (research/re-frame/test) compiled by
;; shadow-cljs and by light-cljs, in the same headless Chromium, and prints a
;; pass/fail table per test namespace and every test that does not pass:
;;   bb benchmarks/re-frame-tests.bb [--light BIN] [--runs N] [--port DIR] [--work DIR]
;;                                   [--re-frame DIR] [--porter DIR] [--timeout-ms N]
;; --light   light-cljs binary (default target/release/light-cljs)
;; --runs    runs per build (default 3); a test passes for a build only if it passes in every run
;; --port    the light-cljs port of the suite (default research/shadow-credibility/re-frame-test-port).
;;           Made with the porter when missing, then the edits below are applied; an existing
;;           port is used as it is, so delete it to port again.
;; --work    build directory (default a temporary directory, deleted afterwards)
;; Builds: shadow-cljs 3.5.3 :browser-test (re-frame's own :ns-regexp, `compile`); light-cljs
;; `compile` (dev build) and `release` of the port, each bundled for the page by esbuild.
;; Needs the Clojure CLI, Java, Node and npm (react, playwright-core and esbuild are installed in
;; the work directory; Playwright's Chromium must already be in its browser cache).
(ns re-frame-tests
  (:require [babashka.fs :as fs]
            [babashka.process :as process]
            [cheshire.core :as json]
            [clojure.pprint :as pprint]
            [clojure.string :as str]
            [rewrite-clj.node :as node]
            [rewrite-clj.parser :as parser]))

(def repo (fs/normalize (fs/path (fs/parent (fs/absolutize *file*)) "..")))

(def versions {:shadow-cljs "3.5.3" :reagent "1.2.0" :react "18.2.0" :playwright-core "1.63.0" :esbuild "0.25.12"})

(defn sh! [dir & cmd]
  (let [r (apply process/sh {:dir (str dir)} cmd)]
    (when-not (zero? (:exit r))
      (throw (ex-info (str "Command failed: " (str/join " " cmd))
                      {:out (subs (:out r) (max 0 (- (count (:out r)) 3000)))
                       :err (subs (:err r) (max 0 (- (count (:err r)) 3000)))})))
    r))

(defn log [& xs] (apply println xs) (flush))

;;; The project both compilers build

(defn test-namespaces
  "re-frame's ClojureScript test namespaces, as its :browser-test build's :ns-regexp selects them."
  [test-dir]
  (->> (fs/glob test-dir "**.{cljs,cljc}")
       (map #(-> (str (fs/relativize test-dir %)) (str/replace #"\.clj[sc]$" "") (str/replace "/" ".") (str/replace "_" "-")))
       (filter #(re-matches #"re-frame\..*-test" %))
       sort))

(defn runner-source
  "A cljs.test runner, compiled by both compilers, that leaves one result per test var in
  globalThis.reFrameTestResults and sets globalThis.reFrameTestsDone when the run ends."
  [nss]
  (str "(ns re-frame-tests.runner
  (:require [cljs.test :as t]
" (str/join "\n" (map #(str "            [" % "]") nss)) "))

(def results (atom []))

(defn- publish! []
  (set! (.-reFrameTestResults js/globalThis) (js/JSON.stringify (clj->js @results))))

(defn- var-name [v]
  (let [m (meta v)] (str (:ns m) \"/\" (:name m))))

(defn- record! [m]
  (swap! results (fn [rs] (if (seq rs) (update-in rs [(dec (count rs)) (:type m)] inc) rs)))
  (publish!))

(defn- default! [m]
  (when-let [f (get-method t/report [::t/default (:type m)])] (f m)))

(defmethod t/report [::results :begin-test-var] [m]
  (swap! results conj {:test (var-name (:var m)) :pass 0 :fail 0 :error 0})
  (publish!))
(defmethod t/report [::results :pass] [m] (t/inc-report-counter! :pass) (record! m))
(defmethod t/report [::results :fail] [m] (t/inc-report-counter! :fail) (record! m) (default! m))
(defmethod t/report [::results :error] [m] (t/inc-report-counter! :error) (record! m) (default! m))
(defmethod t/report [::results :begin-test-ns] [m] (default! m))
(defmethod t/report [::results :summary] [m] (default! m))
(defmethod t/report [::results :end-run-tests] [_]
  (publish!)
  (set! (.-reFrameTestsDone js/globalThis) true))

(defn ^:export init []
  (enable-console-print!)
  (t/run-tests (t/empty-env ::results)
" (str/join "\n" (map #(str "               '" %) nss)) "))

(defn ^:export start [])
;; The light-cljs page calls this; shadow's :browser-test page calls init itself.
(set! (.-reFrameTestsInit js/globalThis) init)
(defn ^:export stop [done] (done))
"))

(def driver
  "node drive.mjs ROOT PAGE TIMEOUT_MS: serves ROOT at http://re-frame-tests.local/, opens PAGE in
  headless Chromium and prints {results, done, console, pageErrors} as JSON."
  "import { chromium } from 'playwright-core';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
const [root, pageName, timeoutArg] = process.argv.slice(2);
const types = { '.html': 'text/html', '.js': 'text/javascript', '.map': 'application/json' };
const browser = await chromium.launch({ headless: true });
const out = { results: [], done: false, console: [], pageErrors: [] };
try {
  const page = await browser.newPage();
  page.on('console', (m) => out.console.push(m.text()));
  page.on('pageerror', (e) => out.pageErrors.push(String((e && e.stack) || e)));
  await page.route('http://re-frame-tests.local/**', async (route) => {
    const rel = decodeURIComponent(new URL(route.request().url()).pathname).replace(/^\\/+/, '');
    try {
      await route.fulfill({ status: 200, body: await readFile(path.join(root, rel)), contentType: types[path.extname(rel)] || 'application/octet-stream' });
    } catch { await route.fulfill({ status: 404, body: 'not found' }); }
  });
  await page.goto('http://re-frame-tests.local/' + pageName);
  try { await page.waitForFunction(() => globalThis.reFrameTestsDone === true, null, { timeout: Number(timeoutArg) }); out.done = true; }
  catch { out.done = false; }
  out.results = JSON.parse(await page.evaluate(() => globalThis.reFrameTestResults || '[]'));
} finally {
  await browser.close();
}
process.stdout.write(JSON.stringify(out) + '\\n');
")

(defn write-project! [project re-frame porter nss]
  (fs/create-dirs (fs/path project "runner" "re_frame_tests"))
  (fs/copy-tree (fs/path re-frame "src") (fs/path project "src"))
  (fs/copy-tree (fs/path re-frame "test") (fs/path project "test"))
  (spit (str (fs/path project "runner" "re_frame_tests" "runner.cljs")) (runner-source nss))
  (spit (str (fs/path project "drive.mjs")) driver)
  (spit (str (fs/path project "package.json"))
        (json/generate-string {:private true
                               :dependencies {"react" (:react versions) "react-dom" (:react versions)
                                              "playwright-core" (:playwright-core versions) "esbuild" (:esbuild versions)}}))
  (spit (str (fs/path project "deps.edn"))
        (pr-str {:paths ["runner" "src" "test"]
                 :deps {'thheller/shadow-cljs {:mvn/version (:shadow-cljs versions)} 'reagent/reagent {:mvn/version (:reagent versions)}
                        'net.cgrand/macrovich {:mvn/version "0.2.1"} 'org.clojure/tools.logging {:mvn/version "1.2.4"}}
                 :aliases {:shadow {:main-opts ["-m" "shadow.cljs.devtools.cli"]}
                           :port {:extra-deps {'light-cljs/port {:local/root (str porter)}}}}}))
  ;; :test is re-frame's own :browser-test build (its :ns-regexp; binaryage/devtools preloads
  ;; dropped) with the runner above; :port is the same program as a :browser build for the porter.
  (spit (str (fs/path project "shadow-cljs.edn"))
        (pr-str {:deps {:aliases [:shadow]}
                 :builds {:test {:target :browser-test :test-dir "out/shadow" :ns-regexp "re-frame\\..*-test$"
                                 :runner-ns 're-frame-tests.runner :compiler-options {:pretty-print true}}
                          :port {:target :browser :output-dir "out/port"
                                 :modules {:main {:init-fn 're-frame-tests.runner/init}}}}})))

;;; Port edits

(defn replace-form
  "TEXT with its top-level (defmacro NAME ...) replaced by REPLACEMENT."
  [text sym replacement]
  (let [forms (:children (parser/parse-string-all text))
        hit? #(let [s (node/string %)] (str/starts-with? s (str "(defmacro " sym "\n")))
        n (count (filter hit? forms))]
    (when-not (= 1 n) (throw (ex-info (str "defmacro " sym " found " n " times") {})))
    (apply str (map #(if (hit? %) replacement (node/string %)) forms))))

(defn replace-once [text from to]
  (let [n (count (re-seq (re-pattern (java.util.regex.Pattern/quote from)) text))]
    (when-not (= 1 n) (throw (ex-info (str "Edit target found " n " times") {:from from})))
    (str/replace text from to)))

(def cljs-test-edits
  "light-cljs's cljs.test is ClojureScript's own (cljs/test.cljs with the macros of cljs/test.cljc,
  from the jar shadow-cljs uses), with these edits. The porter's catalogue only has a stub."
  [{:in :runtime :from "(:require-macros [clojure.template :as temp]\n                   [cljs.test :as test])\n  "
    :to "" :cause "light-cljs retired :require-macros: the macros are defined in the namespace itself."}
   {:in :macros :from "(:fn-var (ana-api/resolve menv x))"
    :to "(not (contains? (:locals menv) x))\n       (contains? '#{= not= == < > <= >= identical? true? false? nil? some? empty? contains? map? vector? set? string? keyword? number? fn? ifn? coll? seq? sequential? every? not-any? re-find re-matches distinct? zero? pos? neg?} x)"
    :cause "light-cljs macros cannot resolve a computed symbol, so `is` treats only these cljs.core functions as predicates; any other form is asserted as a value. Pass and fail are the same; only a failure's :actual is less detailed."}
   {:in :macros :from "(let [{:keys [output-dir]} (ana-api/get-options)]\n    (or output-dir \"out\"))"
    :to "\"out\"" :cause "light-cljs macros have no analyzer options; the default output directory is used (only for stack-trace file names)."}
   {:in :macros :from "(when ana/*load-tests*" :to "(when true" :cause "light-cljs has no *load-tests* switch: tests always load."}
   {:in :macros :from "`(run-tests (empty-env) '~ana/*cljs-ns*))" :to "`(run-tests (empty-env) '~(-> &env :ns :name)))"
    :cause "The current namespace is &env's :ns :name in light-cljs."}
   {:in :macros :from "`(run-tests ~env-or-ns '~ana/*cljs-ns*)))" :to "`(run-tests ~env-or-ns '~(-> &env :ns :name))))"
    :cause "The current namespace is &env's :ns :name in light-cljs."}
   {:in :macros :from "(throw (#?(:clj Exception. :cljs js/Error.) \"The number" :to "(throw (js/Error. \"The number"
    :cause "A reader conditional cannot appear in a .cljs file."}
   {:in :macros :from "(#?(:clj Exception. :cljs js/Error.) \"First argument" :to "(js/Error. \"First argument"
    :cause "A reader conditional cannot appear in a .cljs file."}
   {:in :macros :form 'run-all-tests
    :to "(defmacro run-all-tests\n  ([& _] (throw (js/Error. \"run-all-tests is not in this port: name the namespaces with run-tests\"))))"
    :cause "light-cljs macros cannot list every analyzed namespace; the suite names its namespaces with run-tests."}
   {:in :macros :form 'test-all-vars-block
    :to "(defmacro test-all-vars-block
  ([[quote ns]]
   `(let [env# (get-current-env)
          interns# (ns-interns '~ns)]
      (concat
       [(fn []
          (when (nil? env#)
            (set-env! (empty-env)))
          (when-let [v# (get interns# '~'cljs-test-once-fixtures)]
            (update-current-env! [:once-fixtures] assoc '~ns (deref v#)))
          (when-let [v# (get interns# '~'cljs-test-each-fixtures)]
            (update-current-env! [:each-fixtures] assoc '~ns (deref v#))))]
       (test-vars-block
        (->> (vals interns#)
             (filter (comp :test meta))
             (sort-by (comp :line meta))))
       [(fn []
          (when (nil? env#)
            (clear-env!)))]))))"
    :cause "Upstream reads the namespace's test vars and fixtures from the analyzer while expanding; light-cljs macros cannot, so the expansion reads them at run time with ns-interns, in the same :line order."}
   {:in :macros :form 'test-ns-block
    :to "(defmacro test-ns-block
  ([env [quote ns :as form]]
   (assert (and (= quote 'quote) (symbol? ns)) \"Argument to test-ns must be a quoted symbol\")
   `[(fn []
       (set-env! ~env)
       (do-report {:type :begin-test-ns, :ns ~form})
       (if-let [hook# (get (ns-interns ~form) '~'test-ns-hook)]
         (hook#)
         (block (test-all-vars-block ~form))))
     (fn []
       (do-report {:type :end-test-ns, :ns ~form}))]))"
    :cause "test-ns-hook is looked up at run time with ns-interns, as for test-all-vars-block; the namespace-exists assertion is dropped (requiring it already checks that)."}
   {:in :macros :form 'run-test
    :to "(defmacro run-test
  [test-symbol]
  `(let [env# (get-current-env)]
     (run-block
      (concat
       [(fn [] (when (nil? env#) (set-env! (empty-env))))]
       (test-vars-block [(var ~test-symbol)])
       [(fn [] (when (nil? env#) (clear-env!)))]))))"
    :cause "light-cljs macros cannot resolve the test's namespace, so run-test applies no fixtures. The suite does not call it."}])

(defn cljs-test-source
  "cljs/test.cljs with the macros of cljs/test.cljc and cljs-test-edits, from ClojureScript's JAR."
  [jar]
  (let [entry (fn [e] (with-open [z (java.util.zip.ZipFile. (str jar))] (slurp (.getInputStream z (.getEntry z e)))))
        apply-edits (fn [text in]
                      (reduce (fn [t {:keys [from to form] :as e}]
                                (if (= in (:in e)) (if form (replace-form t form to) (replace-once t from to)) t))
                              text cljs-test-edits))
        runtime (apply-edits (entry "cljs/test.cljs") :runtime)
        macros (apply-edits (entry "cljs/test.cljc") :macros)
        split-ns (fn [text] (let [[ns-form & more] (remove #(#{:whitespace :newline :comment} (node/tag %))
                                                           (:children (parser/parse-string-all text)))
                                  ns-text (node/string ns-form)
                                  at (+ (str/index-of text ns-text) (count ns-text))]
                              [(subs text 0 at) (subs text at)]))
        [runtime-head runtime-body] (split-ns runtime)
        [_ macros-body] (split-ns macros)]
    ;; Macros first: light-cljs rejects a macro call before the macro's definition, and the
    ;; runtime's js-filename calls cljs-output-dir.
    (str runtime-head "\n\n;;; The macros of cljs/test.cljc (light-cljs defines macros in the namespace itself)\n"
         macros-body "\n\n;;; cljs/test.cljs\n" runtime-body)))

(def spliced-metadata
  "The porter splices shadow-cljs's expansion of re-frame.core-instrumented's dispatch-sync and
  subscribe macros (they read &env and *file*) but prints it without the reader metadata of the
  call's literal vector, ^:my-flag here, which the test checks survives the macro. Restored.")

(def port-edits
  "Edits to the porter's output other than cljs.test, each with its cause."
  [{:file "compat/goog.cljs" :from "(goog-define DEBUG false)" :to "(goog-define DEBUG true)"
    :cause "Closure's goog.DEBUG defaults to true and shadow-cljs's compile keeps it; the porter's shim defaults it to false, which would turn off re-frame's debug-only checks and traces that the tests exercise. A light-cljs dev build takes no --define, so the default is changed (both light-cljs builds use it)."}
   {:file "src/re_frame/interceptor_test.cljs" :from "[re-frame.interceptor :refer [context get-coeffect"
    :to "[re-frame.interceptor :refer [get-coeffect"
    :cause "light-cljs rejects a :refer of a private Var (re-frame.interceptor/context is defn-), as Clojure does; ClojureScript accepts it and warns only where the Var is used. The test names it through the alias instead (next edit)."}
   {:file "src/re_frame/interceptor_test.cljs" :from "[re-frame.registrar :as registrar]))\n"
    :to "[re-frame.registrar :as registrar]))\n\n(def ^:private context interceptor/context) ; port: was :refer [context], see edits.edn\n"
    :cause "Pairs with the previous edit: the test's calls to context reach the private Var through the alias, which light-cljs allows."}
   {:file "src/re_frame/trace.cljc" :from "#?(:cljs (goog-define trace-enabled? false)"
    :to "#?(:cljs (def ^boolean trace-enabled? false) ; port: was goog-define, see edits.edn"
    :cause "The tests set! re-frame.trace/trace-enabled?, a goog-define. light-cljs treats a goog-define Var as a constant and rejects set! of it at every level (docs/compiler.md), as the Closure Compiler rejects reassigning a @define in optimized builds; only shadow-cljs's unoptimized compile allows it. Its default is unchanged."}
   {:file "src/re_frame/dispatch_source_meta_test.cljs" :from "[:dispatch-test/recv-merge]]" :to "^:my-flag [:dispatch-test/recv-merge]]"
    :cause spliced-metadata}
   {:file "src/re_frame/dispatch_source_meta_test.cljs" :from "[:dispatch-test/recv-prod]]" :to "^:my-flag [:dispatch-test/recv-prod]]"
    :cause spliced-metadata}
   {:file "src/re_frame/subscribe_source_meta_test.cljs" :from "v__L81_1__auto__ [:sub-test/echo]" :to "v__L81_1__auto__ ^:my-flag [:sub-test/echo]"
    :cause spliced-metadata}
   {:file "src/re_frame/subscribe_source_meta_test.cljs" :from "[:sub-test/prod]]" :to "^:my-flag [:sub-test/prod]]"
    :cause spliced-metadata}
   {:file "compat/goog/async/nextTick.cljs" :from nil
    :to "(ns goog.async.nextTick\n  \"re-frame.interop reads goog.async.nextTick as a JavaScript global.\")\n\n(when-not (exists? js/goog) (set! (.-goog js/globalThis) #js {}))\n(when-not (.-async js/goog) (set! (.-async js/goog) #js {}))\n(set! (.-nextTick (.-async js/goog)) (fn [f] (js/setTimeout f 0)))\n"
    :cause "re-frame.interop reads goog.async.nextTick as a JavaScript global and light-cljs has no Closure Library (a non-goal); re-frame's router uses it for queued dispatches. Same shim as benchmarks/generated-project/re-frame-libs.bb."}])

(defn clojurescript-jar [project]
  (or (some #(when (re-find #"org/clojure/clojurescript/.*\.jar$" %) %)
            (str/split (str/trim (:out (sh! project "clojure" "-Spath"))) #":"))
      (throw (ex-info "No ClojureScript jar on the project's classpath" {}))))

(defn edit-port!
  "Applies the edits to the porter's output in PORT and lists them, with their causes, in edits.edn."
  [port jar porter]
  (spit (str (fs/path port "compat" "cljs" "test.cljs")) (cljs-test-source jar))
  (doseq [{:keys [file from to]} port-edits
          :let [f (fs/path port file)]]
    (if from
      (spit (str f) (replace-once (slurp (str f)) from to))
      (do (fs/create-dirs (fs/parent f)) (spit (str f) to))))
  (spit (str (fs/path port "edits.edn"))
        (with-out-str
          (pprint/pprint
           {:made-by "bb benchmarks/re-frame-tests.bb"
            :porter (str porter)
            :cljs-test {:from (str jar) :edits (mapv #(-> % (dissoc :to :from) (update :in name)) cljs-test-edits)}
            :edits (mapv #(-> % (dissoc :to :from) (assoc :kind (if (:from %) :replace :add))) port-edits)}))))

(defn make-port! [project port porter]
  (log "porting with" (str porter))
  (sh! project "clojure" "-M:shadow:port" "run" "light-cljs.port/main" ":port" "../port")
  (let [made (fs/path (fs/parent project) "port")]
    (fs/create-dirs port)
    ;; light-cljs.edn is the porter's, build :port; its :output-dir points into the project the
    ;; porter ran in, so the builds below give --out-dir.
    (doseq [f ["src" "vendor" "compat" "light-cljs.edn" "port-report.edn" "port-map.edn"]]
      (if (fs/directory? (fs/path made f))
        (fs/copy-tree (fs/path made f) (fs/path port f))
        (fs/copy (fs/path made f) (fs/path port f))))
    (edit-port! port (clojurescript-jar project) porter)
    (log "wrote" (str port))))

;;; Builds and runs

(defn esbuild! [project dir source]
  (let [main (fs/path dir "main.mjs")]
    (spit (str main) source)
    (sh! project "node_modules/.bin/esbuild" (str main) "--bundle" "--format=iife"
         (str "--outfile=" (fs/path dir "bundle.js")) "--log-level=warning")))

(defn page! [dir & scripts]
  (spit (str (fs/path dir "index.html"))
        (str "<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body>"
             (apply str (map #(if (str/ends-with? % ".js") (str "<script src=\"" % "\"></script>") (str "<script>" % "</script>")) scripts))
             "</body></html>")))

(defn build-light!
  "Builds the port with light-cljs MODE (compile or release) and writes the page that runs it;
  returns the directory the page is in. Each page calls the runner's init (through the global
  the runner sets) once the program has loaded, as shadow's index.html does."
  [project port light mode]
  (let [out (fs/path project "out" (str "light-" mode))]
    (fs/delete-tree out)
    ;; release takes --warnings, the ClojureScript warning-compatible analysis a dev build always
    ;; uses: re-frame.core-instrumented's defmacros, whose call sites the porter spliced, still name
    ;; helpers that exist only under :clj, and strict analysis rejects them as unresolved.
    (apply sh! port (str light) mode "port" "--out-dir" (str out) "--no-cache"
           (when (= mode "release") ["--warnings"]))
    (if (= mode "compile")
      ;; The dev build is classic scripts that read React from globalThis.lightCljsPackages, which
      ;; a bundle made by esbuild provides first, as docs/evaluation.md's dev page does.
      (do (esbuild! project out "import * as react from 'react';\nglobalThis.lightCljsPackages = { react };\n")
          (page! out "bundle.js" "index.js" "globalThis.__lightDevLoaded.then(() => globalThis.reFrameTestsInit());"))
      ;; The release build is ES modules importing react; esbuild bundles them for the page.
      (do (esbuild! project out "import './re-frame-tests.runner.js';\n")
          (page! out "bundle.js" "globalThis.reFrameTestsInit();")))
    (str out)))

(defn run-page! [project root page timeout-ms]
  (let [r (json/parse-string (:out (sh! project "node" "drive.mjs" (str root) page (str timeout-ms))) true)]
    (assoc r :by-test (into {} (map (juxt :test identity) (:results r))))))

(defn status [r]
  (cond (nil? r) :not-run (pos? (:error r)) :error (pos? (:fail r)) :fail :else :pass))

(defn summarise
  "One status per test for a build over its runs: the status if every run agrees, else :flaky."
  [runs tests]
  (into {} (for [t tests :let [ss (distinct (map #(status (get (:by-test %) t)) runs))]]
             [t (if (= 1 (count ss)) (first ss) :flaky)])))

(defn main [args]
  (let [opts (loop [[a & more] args o {}]
               (case a
                 nil o
                 ("--light" "--runs" "--port" "--work" "--re-frame" "--porter" "--timeout-ms")
                 (recur (rest more) (assoc o (keyword (subs a 2)) (first more)))
                 (throw (ex-info (str "Unknown argument " a) {}))))
        light (fs/absolutize (or (:light opts) (fs/path repo "target" "release" "light-cljs")))
        runs (parse-long (or (:runs opts) "3"))
        timeout-ms (parse-long (or (:timeout-ms opts) "60000"))
        re-frame (fs/absolutize (or (:re-frame opts) (fs/path repo "research" "re-frame")))
        porter (fs/absolutize (or (:porter opts) (fs/path repo "research" "shadow-credibility" "shadow-port-tool-971")))
        port (fs/absolutize (or (:port opts) (fs/path repo "research" "shadow-credibility" "re-frame-test-port")))
        work (if (:work opts) (fs/absolutize (:work opts)) (fs/create-temp-dir {:prefix "re-frame-tests-"}))
        project (fs/path work "project")
        nss (test-namespaces (fs/path re-frame "test"))]
    (try
      (when (fs/exists? project) (throw (ex-info (str project " exists; remove it first") {})))
      (log "re-frame" (str/trim (:out (sh! re-frame "git" "rev-parse" "HEAD"))) "-" (count nss) "test namespaces")
      (write-project! project re-frame porter nss)
      (log "npm install") (sh! project "npm" "install" "--no-audit" "--no-fund")
      (log "shadow-cljs compile test") (sh! project "clojure" "-M:shadow" "compile" "test")
      (when-not (fs/exists? port) (make-port! project port porter))
      (let [builds [[:shadow (str (fs/path project "out" "shadow"))]
                    [:light-compile (do (log "light-cljs compile test") (build-light! project port light "compile"))]
                    [:light-release (do (log "light-cljs release test") (build-light! project port light "release"))]]
            results (reduce (fn [acc i]
                              (reduce (fn [acc [k root]]
                                        (log "run" (inc i) (name k))
                                        (update acc k (fnil conj []) (run-page! project root "index.html" timeout-ms)))
                                      acc builds))
                            {} (range runs))
            tests (sort (distinct (mapcat #(keys (:by-test %)) (mapcat val results))))
            table (into {} (map (fn [[k rs]] [k (summarise rs tests)]) results))
            ks (map first builds)]
        (println)
        (println "| Namespace | Tests |" (str/join " | " (map name ks)) "|")
        (println "|---|---:|" (str/join "|" (repeat (count ks) "---:")) "|")
        (doseq [[ns ts] (sort (group-by #(first (str/split % #"/")) tests))]
          (println "|" ns "|" (count ts) "|"
                   (str/join " | " (map (fn [k] (count (filter #(= :pass (get-in table [k %])) ts))) ks)) "|"))
        (println "| total |" (count tests) "|"
                 (str/join " | " (map (fn [k] (count (filter #(= :pass (get-in table [k %])) tests))) ks)) "|")
        ;; Assertions reported (pass + fail + error), per run: equal counts show no build skipped any.
        (println "| assertions per run | |"
                 (str/join " | " (map (fn [k] (str/join ", " (map (fn [r] (reduce + (map #(+ (:pass %) (:fail %) (:error %)) (:results r))))
                                                                   (get results k))))
                                      ks)) "|")
        (println)
        (doseq [t tests :when (some #(not= :pass (get-in table [% t])) ks)]
          (println t (into {} (map (fn [k] [k (get-in table [k t])]) ks))))
        (doseq [[k rs] results, [i r] (map-indexed vector rs)
                :when (or (not (:done r)) (seq (:pageErrors r)))]
          (println k "run" (inc i) "done:" (:done r) "page errors:" (pr-str (map #(subs % 0 (min 300 (count %))) (:pageErrors r)))))
        (spit (str (fs/path work "results.edn")) (pr-str {:table table :results results})))
      (finally
        (when-not (:work opts) (fs/delete-tree work))))))

(when (= (fs/absolutize *file*)
         (some-> (System/getProperty "babashka.file") fs/absolutize))
  (main *command-line-args*))
