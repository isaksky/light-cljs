#!/usr/bin/env bb
;; The edit-to-reload loop of `light-cljs watch` against `shadow-cljs watch`,
;; for benchmarks/watch-vs-shadow.md.
;;
;;   bb benchmarks/watch-vs-shadow.bb prepare APP
;;   bb benchmarks/watch-vs-shadow.bb run APP [--tools shadow,light] [--edits N]
;;                                           [--raw FILE] [--log FILE]
;;                                           [--timeout-s S] [--light BIN]
;;                                           [--pause-jitter-ms MS]
;;   bb benchmarks/watch-vs-shadow.bb report RAW [RAW ...]
;;
;; APP is todomvc (re-frame's example, research/re-frame/examples/todomvc for
;; shadow-cljs and its port research/todomvc-port for light-cljs, as
;; benchmarks/re-frame.bb sets them up) or re-frame-200k (a copy of the
;; generated re-frame app research/shadow-credibility/projects/
;; re-frame-200k-seed42 in research/watch-vs-shadow/re-frame-200k, which
;; prepare makes). research/ is gitignored; see the shadow-comparison-
;; credibility project notes for a worktree's copies.
;;
;; prepare writes each tool's dev page: for shadow-cljs a :browser build
;; (TodoMVC has one; the generated app gets a `:watch` build of the same
;; sources and a page that calls workload.main/main), for light-cljs an
;; index.html in the watch's out dir that loads React from an esbuild bundle
;; into globalThis.lightCljsPackages, then index.js, then calls the entry's
;; value (docs/evaluation.md, "JavaScript packages in dev").
;;
;; run starts one tool's watch at a time (nothing cached: shadow's
;; .shadow-cljs/builds and light-cljs's .light-cljs removed first), each tool
;; with its default file watcher and polling interval, waits for its first
;; build, then runs benchmarks/watch-vs-shadow.mjs, which opens the page in
;; headless Chromium through the tool's own dev server and reload client and
;; saves every scenario's edits: one warm-up and N timed edits (default 5),
;; the same text for both tools, with a pause of a second after each (plus a
;; random share of --pause-jitter-ms, so that saves do not all land at the
;; same point of a watch's poll period). Meanwhile the watch's process tree's
;; resident memory is sampled every 250 ms. Prints, per scenario and tool,
;; the median and range of the three intervals from the edit's file
;; modification time (output written, reload message received, new code
;; running in the page), the tool's own step time from its log, the tree's
;; user and system CPU time per edit and the load average; then startup time,
;; the tree's CPU time until its first build and peak memory. --tools gives
;; the order; run the two orders on alternate invocations, each with --raw,
;; and `report` pools their edits.
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[cheshire.core :as json]
         '[clojure.string :as str])

(def repo (fs/parent (fs/parent (fs/absolutize *file*))))
(def research (fs/path repo "research"))
(def npm-tools (fs/path research "npm-tools"))
(def esbuild (str (fs/path npm-tools "node_modules/.bin/esbuild")))

(defn option [args flag default]
  (or (second (drop-while #(not= flag %) args)) default))

(defn light-binary [args]
  (str (fs/absolutize (or (option args "--light" nil) (System/getenv "LIGHT_CLJS_BINARY")
                          (fs/path repo "target/release/light-cljs")))))

(defn sh! [opts & cmd]
  (let [r (apply process/sh (merge {:err :string :out :string} opts) (map str cmd))]
    (when-not (zero? (:exit r))
      (throw (ex-info (str "failed: " (str/join " " cmd) "\n" (:err r) (:out r)) {})))
    r))

;; ---------------------------------------------------------------------------
;; apps and scenarios

(def todomvc-dir (fs/path research "re-frame/examples/todomvc"))
(def todomvc-port (fs/path research "todomvc-port"))
(def generated-source (fs/path research "shadow-credibility/projects/re-frame-200k-seed42"))
(def generated-dir (fs/path research "watch-vs-shadow/re-frame-200k"))

;; Each scenario edits one string (or number) of one file, the same text for
;; both tools; FILE maps tool to path. The probe reads the page text (:dom),
;; calls the function the edit changes (:call), or, for re-frame.loggers's
;; set-loggers!, whose message shows only when its assertion fails, reads its
;; source text (:def). See benchmarks/watch-vs-shadow.mjs.
(def apps
  {"todomvc"
   {:shadow {:dir todomvc-dir
             :cmd ["npx" "shadow-cljs" "watch" "client"]
             :build "client"
             :out (fs/path todomvc-dir "resources/public/js")}
    :light {:dir todomvc-port
            :build "client"
            :out (fs/path todomvc-dir "out/light-watch")}
    :ready "document.querySelector('h1')?.textContent === 'todos'"
    :scenarios
    [{:name "view (leaf): todomvc.views"
      :file {:shadow (fs/path todomvc-dir "src/todomvc/views.cljs")
             :light (fs/path todomvc-port "src/todomvc/views.cljs")}
      :from "[:h1 \"todos\"]" :to #(str "[:h1 \"todos " % "\"]")
      :probe {:dom true :original "todos"}}
     {:name "widely required: re-frame.loggers"
      :file {:shadow (fs/path research "re-frame/src/re_frame/loggers.cljc")
             :light (fs/path todomvc-port "src/re_frame/loggers.cljc")}
      :from "\"Unknown keys in new-loggers\""
      :to #(str "\"Unknown keys in new-loggers " % "\"")
      :probe {:def "re-frame.loggers/set-loggers!" :original "Unknown keys in new-loggers"}}]}

   "re-frame-200k"
   {:shadow {:dir generated-dir
             :cmd ["clojure" "-M:shadow" "watch" "watch"]
             :build "watch"
             :out (fs/path generated-dir "out/shadow-watch")}
    :light {:dir generated-dir
            :build "bench"
            :out (fs/path generated-dir "out/light-watch")}
    :ready "!!document.querySelector('#app-root header')"
    :scenarios
    [{:name "view (leaf): app.baga.views"
      :file (fs/path generated-dir "src/app/baga/views.cljs")
      :from "[:h2 \"Baga\"]" :to #(str "[:h2 \"Baga " % "\"]")
      :probe {:call "app.baga.views/root" :args [] :original "Baga"}}
     {:name "widely required: workload.common"
      :file (fs/path generated-dir "src/workload/common.cljs")
      :from "\"GENERATED_WASTE_C_COMMON1_END\""
      :to #(str "\"GENERATED_WASTE_C_COMMON1_END " % "\"")
      :probe {:call "workload.common/geinklarvou" :args [1] :original "GENERATED_WASTE_C_COMMON1_END"}}
     {:name "macro: workload.sobis/hainfoulvei"
      :file (fs/path generated-dir "macros/workload/sobis.cljc")
      :from "(+ 11 (do ~@body))" :to #(str "(+ " % " (do ~@body))") :numeric true
      ;; dokgem(x) is (workload.common/norm (+ 11 body)), norm being mod 1000003.
      :probe {:call "workload.breisrei/dokgem" :args [3] :offset 11 :modulus 1000003}}]}})

;; ---------------------------------------------------------------------------
;; prepare

(defn packages-bundle
  "React, react-dom and react-dom/client in one development-mode script that
  sets globalThis.lightCljsPackages, built from NODE-MODULES-DIR's packages."
  [node-modules-dir out]
  (let [entry (fs/path node-modules-dir "light-watch-packages.mjs")]
    (spit (fs/file entry)
          (str "import * as react from \"react\";\n"
               "import * as reactDom from \"react-dom\";\n"
               "import * as reactDomClient from \"react-dom/client\";\n"
               "globalThis.lightCljsPackages = { \"react\": react, \"react-dom\": reactDom, \"react-dom/client\": reactDomClient };\n"))
    (sh! {:dir (str node-modules-dir)} esbuild (str entry) "--bundle" "--format=iife"
         "--define:process.env.NODE_ENV=\"development\"" (str "--outfile=" out) "--log-level=warning")
    (fs/delete entry)))

(def light-loader
  (str "<script src=\"packages.js\"></script>\n"
       "<script src=\"index.js\"></script>\n"
       "<script>globalThis.__lightDevLoaded.then((start) => start());</script>\n"))

(defn prepare [app]
  (case app
    "todomvc"
    (let [out (get-in apps ["todomvc" :light :out])
          html (slurp (fs/file todomvc-dir "resources/public/index.html"))]
      (when-not (fs/exists? (fs/path todomvc-dir "node_modules"))
        (throw (ex-info "run bb benchmarks/re-frame.bb setup first" {})))
      (fs/create-dirs out)
      (fs/copy (fs/path todomvc-dir "resources/public/todos.css") out {:replace-existing true})
      (packages-bundle todomvc-dir (fs/path out "packages.js"))
      (spit (fs/file out "index.html") (str/replace html "<script src=\"js/client.js\"></script>\n" light-loader)))

    "re-frame-200k"
    (let [dir generated-dir]
      (fs/delete-tree dir)
      (fs/create-dirs (fs/parent dir))
      (fs/copy-tree generated-source dir)
      (doseq [d [".cpcache" ".shadow-cljs" "out"]] (fs/delete-tree (fs/path dir d)))
      ;; A browser build of the same sources for shadow-cljs's watch; the
      ;; project's own :bench build is a :node-script.
      (spit (fs/file dir "shadow-cljs.edn")
            (pr-str {:deps {:aliases [:shadow]}
                     :dev-http {9681 "out/shadow-watch"}
                     :builds {:watch {:target :browser
                                      :output-dir "out/shadow-watch/js"
                                      :asset-path "/js"
                                      :modules {:main {:init-fn 'workload.main/main}}
                                      :compiler-options {:warnings {:invalid-arithmetic false}}}}}))
      (fs/create-dirs (fs/path dir "out/shadow-watch"))
      (spit (fs/file dir "out/shadow-watch/index.html")
            "<!doctype html>\n<html><head><meta charset=\"utf-8\"></head><body>\n<script src=\"/js/main.js\"></script>\n</body></html>\n")
      (fs/create-dirs (fs/path dir "out/light-watch"))
      (packages-bundle dir (fs/path dir "out/light-watch/packages.js"))
      (spit (fs/file dir "out/light-watch/index.html")
            (str "<!doctype html>\n<html><head><meta charset=\"utf-8\"></head><body>\n" light-loader "</body></html>\n")))))

;; ---------------------------------------------------------------------------
;; run

(defn load-average []
  (-> (sh! {} "sysctl" "-n" "vm.loadavg") :out str/trim (str/split #"\s+") second parse-double))

(defn ps-rows []
  (for [l (str/split-lines (:out (sh! {} "ps" "-A" "-o" "pid=,ppid=,rss=")))
        :let [[pid ppid rss] (str/split (str/trim l) #"\s+")]]
    [(parse-long pid) (parse-long ppid) (parse-long rss)]))

(defn tree
  "PID and its descendants: {pid rss-bytes}."
  [pid]
  (let [rows (ps-rows)
        children (group-by second rows)
        rss (into {} (map (fn [[p _ r]] [p (* 1024 r)]) rows))]
    (loop [todo [pid] acc {}]
      (if-let [[p & more] (seq todo)]
        (recur (concat more (map first (children p))) (assoc acc p (rss p 0)))
        acc))))

(defn cpu-seconds
  "ps's [[dd-]hh:]mm:ss.ss as seconds."
  [text]
  (let [[days clock] (if (str/includes? text "-") (str/split text #"-") ["0" text])]
    (+ (* 86400 (parse-long days))
       (reduce (fn [acc x] (+ (* acc 60) (parse-double x))) 0 (str/split clock #":")))))

(defn tree-cpu
  "User and system CPU seconds of PID and its descendants: [user system]."
  [pid]
  (let [rows (for [l (str/split-lines (:out (sh! {} "ps" "-A" "-o" "pid=,ppid=,utime=,stime=")))]
               (str/split (str/trim l) #"\s+"))
        pids (set (keys (tree pid)))]
    (reduce (fn [[u s] [p _ pu ps]]
              (if (pids (parse-long p)) [(+ u (cpu-seconds pu)) (+ s (cpu-seconds ps))] [u s]))
            [0.0 0.0] rows)))

(defn now-ms [] (/ (System/nanoTime) 1e6))
(defn epoch-ms [] (double (System/currentTimeMillis)))

(defn start-watch
  "Start TOOL's watch for APP: {:proc :lines (atom [[epoch-ms line]]) :samples
  (atom [[epoch-ms bytes]]) :start epoch-ms}."
  [app tool binary]
  (let [{:keys [dir cmd build out]} (get-in apps [app tool])
        cmd (case tool
              :shadow cmd
              :light [binary "watch" build "--out-dir" (str out) "--serve" "0" "--no-nrepl"])
        _ (case tool
            :shadow (fs/delete-tree (fs/path dir ".shadow-cljs/builds" build))
            :light (fs/delete-tree (fs/path dir ".light-cljs")))
        lines (atom [])
        samples (atom [])
        start (epoch-ms)
        proc (apply process/process {:dir (str dir) :out :stream :err :stream} (map str cmd))
        pump (fn [stream]
               (future (with-open [r (java.io.BufferedReader. (java.io.InputStreamReader. stream))]
                         (doseq [l (line-seq r)] (swap! lines conj [(epoch-ms) l])))))
        pid (.pid ^Process (:proc proc))
        running (atom true)]
    (pump (:out proc))
    (pump (:err proc))
    (future (while @running
              (swap! samples conj [(epoch-ms) (reduce + (vals (tree pid)))])
              (Thread/sleep 250)))
    {:proc proc :pid pid :lines lines :samples samples :start start :running running}))

(defn stop-watch [{:keys [pid running]}]
  (reset! running false)
  (doseq [p (keys (tree pid))] (process/sh "kill" "-TERM" (str p)))
  (Thread/sleep 2000)
  (doseq [p (keys (tree pid))] (process/sh "kill" "-KILL" (str p))))

(def ready-pattern {:shadow #"Build completed\." :light #"watching every"})
(def url-pattern {:shadow #":dev-http (http://\S+) serving"
                  :light #"serving .* at (http://\S+?)/?$"})

(defn wait-line [{:keys [lines proc]} pattern what timeout-ms]
  (let [deadline (+ (now-ms) timeout-ms)]
    (loop []
      (if-let [hit (some (fn [[t l]] (when-let [m (re-find pattern l)] [t m])) @lines)]
        hit
        (do (when (or (> (now-ms) deadline) (not (process/alive? proc)))
              (throw (ex-info (str "no " what "; output:\n" (str/join "\n" (map second (take-last 40 @lines)))) {})))
            (Thread/sleep 20)
            (recur))))))

(defn step-lines
  "The tool's own log lines for each finished step: [[epoch-ms ms namespaces]]."
  [tool lines]
  (keep (fn [[t l]]
          (case tool
            :light (when-let [[_ nss ms] (re-find #"recompiled (.*) in ([\d.]+) ms" l)]
                     [t (parse-double ms) (count (str/split nss #", "))])
            :shadow (when-let [[_ _ compiled s] (re-find #"Build completed\. \((\d+) files, (\d+) compiled, \d+ warnings, ([\d.]+)s\)" l)]
                      [t (* 1000 (parse-double s)) (parse-long compiled)])))
        lines))

(defn scenario-config [app tool sc edits]
  (let [file (str (if (map? (:file sc)) (get-in sc [:file tool]) (:file sc)))
        text (slurp file)
        _ (when-not (= 1 (count (re-seq (re-pattern (java.util.regex.Pattern/quote (:from sc))) text)))
            (throw (ex-info (str file " does not contain " (:from sc) " exactly once") {})))
        tokens (for [k (range (inc edits))]
                 (if (:numeric sc) (str (+ 972001 k)) (str "w972e" (inc k))))]
    {:name (:name sc) :file file :text text
     :edits (map #(str/replace text (:from sc) ((:to sc) %)) tokens)
     :tokens tokens :probe (:probe sc)}))

(defn run-tool [app tool binary {:keys [edits timeout-s log pause-jitter-ms]}]
  (let [{:keys [ready scenarios]} (apps app)
        out (get-in apps [app tool :out])
        ;; Read every source before the watch starts, so that whatever
        ;; happens below, the finally clause can put them back.
        configs (mapv #(scenario-config app tool % edits) scenarios)
        load0 (load-average)
        w (start-watch app tool binary)]
    (try
      (let [[ready-at _] (wait-line w (ready-pattern tool) "first build" 1800000)
            ;; The tree's CPU time up to its first build, read as soon as
            ;; the line shows (within the 20 ms the wait polls).
            startup-cpu (tree-cpu (:pid w))
            [_ [_ url]] (wait-line w (url-pattern tool) "server address" 60000)
            cfg (fs/create-temp-file {:prefix "watch-vs-shadow" :suffix ".json"})
            driver-start (epoch-ms)
            _ (spit (fs/file cfg)
                    (json/generate-string
                     {:tool (name tool) :url url :events (str (str/replace url #"/$" "") "/events")
                      :npmTools (str npm-tools) :outDir (str out) :rootPid (:pid w)
                      :ready ready :timeoutMs (* 1000 timeout-s) :pauseMs 1000
                      :pauseJitterMs (or pause-jitter-ms 0)
                      :scenarios configs}))
            r (process/sh {:err :inherit :out :string} "node" (str (fs/path repo "benchmarks/watch-vs-shadow.mjs")) (str cfg))
            _ (fs/delete cfg)
            _ (when-not (zero? (:exit r)) (throw (ex-info "the browser driver failed" {})))
            result (json/parse-string (:out r) true)
            edits-start (+ driver-start (:pageLoadMs result))
            samples @(:samples w)
            steps (step-lines tool @(:lines w))
            with-step (fn [e] (let [[t ms n] (first (filter #(>= (first %) (:mtime e)) steps))]
                                (assoc e :step-ms ms :step-namespaces n :step-logged t)))]
        {:tool tool :load-before load0
         :startup-ms (- ready-at (:start w))
         :startup-cpu startup-cpu
         :page-load-ms (:pageLoadMs result)
         :startup-peak (apply max 0 (map second (filter #(<= (first %) ready-at) samples)))
         :steady-peak (apply max 0 (map second (filter #(>= (first %) edits-start) samples)))
         :errors (:errors result)
         :edits (mapv with-step (:edits result))})
      (catch Exception e
        (binding [*out* *err*]
          (println "watch output, last 40 lines:")
          (run! #(println (second %)) (take-last 40 @(:lines w))))
        (throw e))
      (finally
        (stop-watch w)
        (when log
          (spit log (str/join (for [[t l] @(:lines w)] (format "%.1f %s %s\n" t (name tool) l))) :append true))
        (doseq [{:keys [file text]} configs
                :when (not= text (slurp file))]
          (spit file text)
          (binding [*out* *err*] (println "restored" file)))))))

(defn median [xs] (let [s (vec (sort xs)) n (count s)]
                    (if (odd? n) (s (quot n 2)) (/ (+ (s (dec (quot n 2))) (s (quot n 2))) 2.0))))

(defn summary [xs fmt]
  (if (some nil? xs)
    "-"
    (str (format fmt (double (median xs))) " (" (format fmt (double (apply min xs))) "-"
         (format fmt (double (apply max xs))) ")")))

(def tool-label {:shadow "shadow-cljs" :light "light-cljs"})

(defn print-results
  "Tables for RUNS (from one or more invocations): each scenario's edits of
  each tool pooled across its runs, warm-up edits left out."
  [app binary runs]
  (let [tools (distinct (map :tool runs))
        timed (fn [tool sc] (for [run runs :when (= tool (:tool run))
                                  e (:edits run) :when (and (= sc (:scenario e)) (pos? (:i e)))]
                              e))
        scenarios (distinct (map :scenario (mapcat :edits runs)))
        diff (fn [es a b] (when (every? #(and (a %) (b %)) es) (map #(- (a %) (b %)) es)))]
    (println (str "\n### " app "\n"))
    (println (str "light-cljs " (fs/file-name binary) "; " (quot (count runs) (count tools)) " watch sessions per tool; "
                  (count (timed (first tools) (first scenarios)))
                  " timed edits per scenario and tool, each session's first edit of a scenario a warm-up."
                  " Milliseconds from the edit's file modification time: median (range).\n"))
    (println "| Scenario | Tool | Output written | Reload message | New code running | Tool's own step time | Namespaces or files compiled | CPU user + system per edit, s | Load average |")
    (println "|---|---|---:|---:|---:|---:|---:|---:|---:|")
    (doseq [sc scenarios tool tools :let [es (timed tool sc)]]
      (println (format "| %s | %s | %s | %s | %s | %s | %s | %s | %s |" sc (tool-label tool)
                       (summary (map :output es) "%.0f") (summary (map :message es) "%.0f")
                       (summary (map :running es) "%.0f") (summary (map :step-ms es) "%.0f")
                       (summary (map :step-namespaces es) "%.0f")
                       (summary (map #(+ (:cpuUser %) (:cpuSys %)) es) "%.2f")
                       (summary (map :load es) "%.1f"))))
    (println "\nWhere the interval to new code running goes, medians in milliseconds: noticing the edit and"
             "everything around the tool's own step (output written minus step time), the step, writing to"
             "the push (reload message minus output written), and the page loading the code (running minus message).\n")
    (println "| Scenario | Tool | Noticing and other | Step | Output to message | Message to running |")
    (println "|---|---|---:|---:|---:|---:|")
    (doseq [sc scenarios tool tools :let [es (timed tool sc)
                                          m #(if (seq %) (format "%.0f" (double (median %))) "-")]]
      (println (format "| %s | %s | %s | %s | %s | %s |" sc (tool-label tool)
                       (m (diff es :output :step-ms)) (m (keep :step-ms es))
                       (m (diff es :message :output)) (m (diff es :running :message)))))
    (println "\n| Tool | Startup to first build, s | CPU user + system during startup, s | Page load in Chromium, s | Peak memory during startup, MB | Peak memory while editing, MB | Load average before |")
    (println "|---|---:|---:|---:|---:|---:|---:|")
    (doseq [run runs :let [[user sys] (:startup-cpu run)]]
      (println (format "| %s | %.1f | %s | %.1f | %.0f | %.0f | %.1f |" (tool-label (:tool run))
                       (/ (:startup-ms run) 1000.0)
                       (if user (format "%.1f + %.1f" user sys) "-")
                       (/ (:page-load-ms run) 1000.0)
                       (/ (:startup-peak run) 1e6) (/ (:steady-peak run) 1e6) (:load-before run))))
    (doseq [run runs e (distinct (:errors run))]
      (println (str "\n- " (tool-label (:tool run)) ": " e)))))

(let [[command app & args] *command-line-args*]
  (case command
    "prepare" (do (prepare app) (println "prepared" app))
    "run" (let [binary (light-binary args)
                tools (map keyword (str/split (option args "--tools" "shadow,light") #","))
                opts {:edits (parse-long (option args "--edits" "5"))
                      :timeout-s (parse-long (option args "--timeout-s" "900"))
                      :log (option args "--log" nil)
                      :pause-jitter-ms (parse-long (option args "--pause-jitter-ms" "0"))}
                runs (vec (for [t tools]
                            (do (binding [*out* *err*] (println "==" app (name t)))
                                (run-tool app t binary opts))))]
            (when-let [raw (option args "--raw" nil)]
              (spit raw (pr-str {:app app :binary binary :runs runs})))
            (print-results app binary runs))
    "report" (let [raws (map (comp read-string slurp) (cons app (take-while #(not (str/starts-with? % "--")) args)))]
               (print-results (:app (first raws)) (:binary (first raws)) (vec (mapcat :runs raws))))
    (do (println "usage: bb benchmarks/watch-vs-shadow.bb prepare APP | run APP [--tools shadow,light] [--edits N] [--raw FILE] [--log FILE] [--timeout-s S] [--light BIN] [--pause-jitter-ms MS] | report RAW...")
        (System/exit 1))))
