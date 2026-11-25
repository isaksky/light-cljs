#!/usr/bin/env bb
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(defn load-average []
  (-> (process/shell {:out :string} "sysctl" "-n" "vm.loadavg")
      :out str/trim (str/replace #"[{}]" "") str/trim (str/split #"\s+")
      first parse-double))

(defn peak-bytes [timing]
  (some-> (re-find #"(?m)^\s*(\d+)\s+maximum resident set size" timing) second parse-long))

(defn elapsed [start]
  (/ (- (System/nanoTime) start) 1e9))

(defn measure
  ([log-dir id cwd argv] (measure log-dir id cwd argv {}))
  ([log-dir id cwd argv {:keys [input output allow-failure? env]}]
   (let [out (or output (fs/path log-dir (str id ".out")))
         err (fs/path log-dir (str id ".err"))
         opts (cond-> {:dir (str cwd), :out (fs/file out), :err (fs/file err)
                       :continue true}
                input (assoc :in (fs/file input))
                env (assoc :extra-env env))
         load (load-average)
         start (System/nanoTime)
         {:keys [exit]} (apply process/shell opts "/usr/bin/time" "-l" (map str argv))
         seconds (elapsed start)
         memory (peak-bytes (slurp (fs/file err)))]
     (when (and (not allow-failure?) (not (zero? exit)))
       (throw (ex-info (str id " failed, see " err) {:exit exit :argv argv})))
     {:seconds seconds :peak-bytes memory :load load :exit exit})))

(defn save! [log-dir row sample]
  (spit (fs/file log-dir (str row ".edn")) (str (pr-str sample) "\n") :append true)
  (prn sample))

(defn alternated [runs]
  (for [run (range 1 (inc runs))
        side (if (odd? run) [:master :branch] [:branch :master])]
    [run side]))

(defn release-targets [master branch]
  {:master (fs/path master "target"), :branch (fs/path branch "target/release-comparison")})

(defn binary [target]
  (fs/path target "release/light-cljs"))

(defn build! [log-dir master branch runs]
  (let [roots {:master master :branch branch}
        targets (release-targets master branch)]
    (doseq [[run side] (alternated runs)
            :let [root (roots side)
                  target (targets side)
                  id (str "build-" (name side) "-" run)
                  command ["cargo" "build" "--release" "--target-dir" (str target)]]]
      (when (fs/exists? target)
        (fs/delete-tree target))
      (save! log-dir "build" (assoc (measure log-dir (str id "-clean") root command)
                              :run run :side side :workload :clean
                              :bytes (fs/size (binary target))))
      (process/shell "touch" (str (fs/path root "src/main.rs")))
      (save! log-dir "build" (assoc (measure log-dir (str id "-touch") root command)
                              :run run :side side :workload :touch)))))

(defn rust-fixtures! [dir branch]
  (let [source (fs/path dir "extract_ratchet.rs")
        extractor (fs/path dir "extract-ratchet")
        fixture-module (str (fs/path branch "tests/support/ratchet_fixtures.rs"))]
    (spit (fs/file source)
          (str "#[path = " (pr-str fixture-module) "]
mod fixtures;
use std::{env, fs, path::Path};

fn main() -> std::io::Result<()> {
    let root = env::args_os().nth(1).unwrap();
    let root = Path::new(&root);
    fs::create_dir_all(root)?;
    for f in fixtures::FIXTURES {
        fs::write(root.join(format!(\"{}.cljs\", f.name)), f.source)?;
        println!(\"{}\", f.name);
    }
    for f in fixtures::NAMESPACE_FIXTURES {
        let source_root = root.join(f.name);
        for (path, source) in f.files {
            let path = source_root.join(path);
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, source)?;
        }
        println!(\"{} {}\", f.name, f.entry);
    }
    Ok(())
}
"))
    (process/shell "rustc" "--edition=2024" "-A" "dead_code" "-o" (str extractor) (str source))
    (->> (process/shell {:out :string} (str extractor) (str (fs/path dir "ratchet-sources")))
         :out str/split-lines (map #(str/split % #" ")))))

(defn compare-files! [log-dir workload load master branch]
  (let [result (process/shell {:continue true} "cmp" "-s" (str master) (str branch))]
    (when (> (:exit result) 1)
      (throw (ex-info "Could not compare outputs" {:master master :branch branch})))
    (save! log-dir "output" {:workload workload :identical (zero? (:exit result)) :load load
                             :master-bytes (fs/size master) :branch-bytes (fs/size branch)})))

(defn output! [log-dir master branch]
  (let [dir (fs/create-dirs (fs/path log-dir "output"))
        sources (fs/path dir "ratchet-sources")
        fixtures (rust-fixtures! dir branch)
        targets (release-targets master branch)]
    (doseq [[fixture entry] fixtures
            level ["none" "prepare" "advanced"]
            :let [id (str fixture "-" level)
                  load (load-average)
                  outputs (into {} (for [side [:master :branch]]
                                     [side (fs/path dir (str id "-" (name side) ".js"))]))]]
      (doseq [side [:master :branch]
              :let [input (if entry
                            ["--entry" entry "--source-root" (fs/path sources fixture) "--no-cache"]
                            [(fs/path sources (str fixture ".cljs"))])]]
        (apply process/shell {:out (fs/file (outputs side)) :dir (str dir)}
               (str (binary (targets side))) "--compile"
               (map str (concat input ["--optimize" level]))))
      (compare-files! log-dir id load (outputs :master) (outputs :branch)))
    (let [project (fs/path branch "research/todomvc-port")
          load (load-average)]
      (doseq [side [:master :branch]]
        (process/shell {:dir (str project)} (str (binary (targets side)))
                       "release" "client" "--no-cache"
                       "--out-dir" (str (fs/path dir (str "todomvc-" (name side))))))
      (let [master-js (fs/path dir "todomvc-master/todomvc.core.js")
            branch-js (fs/path dir "todomvc-branch/todomvc.core.js")]
        (compare-files! log-dir "todomvc-advanced" load master-js branch-js)))))

(defn session! [log-dir master branch runs]
  (let [dir (fs/create-dirs (fs/path log-dir "sessions"))
        targets (release-targets master branch)
        macro-script (fs/path dir "macros.cljs")
        repl-input (fs/path dir "repl.cljs")
        limits ["--timeout-ms" "120000" "--poll-budget" "1000000000000"]]
    (spit (fs/file macro-script)
          (str "(defmacro expand [x] `(let [n# ~x] (+ n# n# 1)))\n"
               "(reduce + ["
               (str/join "\n" (map #(str "(expand " % ")") (range 400))) "])\n"))
    (spit (fs/file repl-input)
          (apply str (for [i (range 3000)]
                       (str "(def v" i " (vec (range 50)))\n"
                            "(reduce + (map inc v" i "))\n"))))
    (doseq [[workload args input lines last-result]
            [[:startup ["--eval" "--text" "(+ 1 2)"] nil 1 "3"]
             [:libraries ["--eval" "--text"
                          "(require 'clojure.string 'clojure.set 'clojure.walk 'clojure.edn 'cljs.reader) :loaded"]
              nil 1 ":loaded"]
             [:macros (concat ["--eval"] limits [macro-script]) nil 1 "160000"]
             [:repl (into ["--repl"] limits) repl-input 6000 "1275"]]
            [run side] (alternated runs)
            :let [id (str (name workload) "-" (name side) "-" run)
                  out (fs/path dir (str id ".out"))
                  sample (measure dir id dir (cons (binary (targets side)) args)
                                  {:input input :output out})
                  results (str/split-lines (slurp (fs/file out)))]]
      (when-not (and (= lines (count results)) (= last-result (last results)))
        (throw (ex-info "Unexpected session result" {:output (str out)})))
      (save! log-dir "sessions" (assoc sample :run run :side side :workload workload)))))

(defn reader-driver! [dir target side]
  (let [source (fs/path dir "reader.rs")
        driver (fs/path dir (str "readers-" (name side)))
        release (fs/path target "release")
        native (concat [(fs/path release "gn_out/obj")]
                       (filter fs/directory? (fs/glob release "build/*/out")))]
    (spit (fs/file source)
          "use light_cljs::{MacroHost, compiler::compile_namespace_with_macros, compiler_data::Name};
use std::{env, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env::args_os().nth(1).expect(\"source root\"));
    let mut host = MacroHost::new();
    host.register_compiler_data_reader(Name::new(Some(\"bench\"), \"named\"), \"(fn [x] (inc x))\")?;
    host.register_compiler_default_data_reader(\"(fn [tag x] (if (= tag 'bench/fallback) (+ x 2) nil))\")?;
    print!(\"{}\", compile_namespace_with_macros(\"bench.reader\", &[root], &mut host)?);
    Ok(())
}
")
    (apply process/shell "rustc" "--edition=2024" "-C" "opt-level=3"
           "-C" "lto=thin" "-C" "codegen-units=1"
           "--extern" (str "light_cljs=" (fs/path release "liblight_cljs.rlib"))
           "-L" (str "dependency=" (fs/path release "deps"))
           "-o" (str driver) (str source)
           (mapcat #(vector "-L" (str "native=" %)) native))
    driver))

(defn readers! [log-dir master branch runs]
  (let [dir (fs/create-dirs (fs/path log-dir "readers"))
        drivers (into {} (map (fn [[side target]] [side (reader-driver! dir target side)])
                              (release-targets master branch)))]
    (doseq [[workload pairs] [[:small 16] [:scaled 3000]]
            :let [root (fs/path dir (name workload))]]
      (fs/create-dirs (fs/path root "bench"))
      (spit (fs/file root "bench/reader.cljs")
            (str "(ns bench.reader)\n(reduce + [\n"
                 (apply str (for [i (range pairs)]
                              (str "#bench/named " i " #bench/fallback " i "\n")))
                 "])\n"))
      (doseq [[run side] (alternated runs)
              :let [id (str "readers-" (name workload) "-" (name side) "-" run)
                    output (fs/path dir (str id ".js"))
                    sample (measure dir id dir [(drivers side) root] {:output output})
                    result (process/shell {:out :string} "node" "-e"
                                          "console.log(eval(require('node:fs').readFileSync(process.argv[1], 'utf8')))"
                                          (str output))]]
        (when-not (= (str (* pairs (+ pairs 2))) (str/trim (:out result)))
          (throw (ex-info "Unexpected data-reader result" {:filename (str output)})))
        (save! log-dir "readers" (assoc sample :run run :side side :workload workload))))))

(defn compile! [log-dir master branch project build-id runs]
  (let [targets (release-targets master branch)
        name (fs/file-name project)
        output (fs/path log-dir "compiled" name)]
    (doseq [[run side] (alternated runs)
            jobs ["1" "default"]
            :let [id (str name "-jobs-" jobs "-" (clojure.core/name side) "-" run)
                  cmd [(binary (targets side)) "release" build-id "--no-cache" "--out-dir" output]]]
      (fs/delete-tree output)
      (save! log-dir "compile"
             (assoc (measure log-dir id project (cond-> cmd (= jobs "1") (into ["--jobs" jobs])))
                    :run run :side side :workload (str name " / " jobs))))))

(defn await-line [reader log pattern]
  (let [matched (future
                  (loop []
                    (when-let [line (.readLine reader)]
                      (.write log (str line "\n"))
                      (if (re-find pattern line) line (recur)))))
        line (deref matched 120000 nil)]
    (or line (throw (ex-info "Watch ended or timed out before its status line" {:pattern pattern})))))

(defn stop-watch! [watch]
  (process/shell {:continue true} "pkill" "-TERM" "-P" (str (.pid (:proc watch)))))

(defn watch-sample! [log-dir id cwd cmd edit!]
  (let [log (fs/path log-dir (str id ".err"))
        load (load-average)
        start (System/nanoTime)
        watch (apply process/process {:dir (str cwd), :err :pipe
                                      :out (fs/file log-dir (str id ".out"))}
                     "/usr/bin/time" "-l" (map str cmd))]
    (try
      (with-open [reader (io/reader (:err watch)), err (io/writer (fs/file log))]
        (await-line reader err #"; watching every ")
        (let [seconds (if edit!
                        (do (edit!)
                            (let [line (await-line reader err #"^light-cljs: recompiled ")]
                              (or (some-> (re-matches #"light-cljs: recompiled todomvc\.views in ([\d.]+) ms" line)
                                          second parse-double (/ 1000.0))
                                  (throw (ex-info "Unexpected watch step" {:line line})))))
                        (elapsed start))]
          (stop-watch! watch)
          (let [timing (str/join "\n" (line-seq reader))]
            (.write err timing)
            @watch
            {:seconds seconds :peak-bytes (peak-bytes timing) :load load})))
      (finally
        (when (.isAlive (:proc watch))
          (stop-watch! watch)
          @watch)))))

(def macro-limits
  ["--macro-expansions" "100000000" "--macro-nodes" "4000000000"
   "--macro-timeout-ms" "3600000" "--macro-memory-limit" "4000000000"
   "--macro-poll-budget" "1000000000000"])

(defn watch! [log-dir master branch runs]
  (let [dir (fs/create-dirs (fs/path log-dir "watch"))
        targets (release-targets master branch)
        port (fs/path branch "research/todomvc-port")]
    (doseq [[run side] (alternated runs)
            :let [id (str (name side) "-" run)
                  project (fs/path dir id)
                  roots (mapcat #(vector "--source-root" (fs/path project %)) ["src" "vendor" "compat"])
                  cmd (concat [(binary (targets side)) "--watch" "--entry" "todomvc.core"
                               "--out-dir" (fs/path project "out") "--cache-dir" (fs/path project "cache")
                               "--poll-ms" "20" "--stats"] roots macro-limits)
                  edited (fs/file project "src/todomvc/views.cljs")]]
      (fs/delete-tree project)
      (fs/copy-tree port project)
      (doseq [workload [:start :unchanged :edit]
              :let [edit! (when (= workload :edit)
                            #(spit edited (str/replace (slurp edited)
                                                      "Double-click to edit a todo"
                                                      "Double-click to edit this todo")))]]
        (save! log-dir "watch" (assoc (watch-sample! dir (str id "-" (name workload)) project cmd edit!)
                                   :run run :side side :workload workload))))))

(defn integer-stat [report stat]
  (some (fn [line]
          (when (str/starts-with? line (str stat " "))
            (parse-long (subs line (inc (count stat))))))
        (str/split-lines report)))

(defn cache! [log-dir master branch runs]
  (let [dir (fs/create-dirs (fs/path log-dir "cache"))
        src (fs/create-dirs (fs/path dir "src/app"))
        targets (release-targets master branch)]
    (spit (fs/file src "macros.cljs") "(ns app.macros) (defmacro answer [n] `(+ ~n 3))")
    (spit (fs/file src "main.cljs")
          "(ns app.main (:require [app.macros :refer [answer]])) (js/console.log (answer 39))")
    (doseq [[run side] (alternated runs)
            :let [project (fs/path dir (str (name side) "-" run))
                  bin (binary (targets side))]]
      (fs/delete-tree project)
      (fs/create-dirs project)
      (doseq [[mode phase cache-name hits]
              [[:clean :cold "clean" 0] [:clean :warm "clean" 2]
               [:watch :cold "watch" 0] [:watch :warm "watch" 2]
               [:clean :cross "watch" 0] [:watch :cross "clean" 0]]
              :let [id (str (name mode) "-" (name phase))
                    common ["--entry" "app.main" "--source-root" (fs/path dir "src")
                            "--out-dir" (fs/path project (str "out-" (name mode)))
                            "--cache-dir" (fs/path project cache-name) "--stats"]
                    sample (case mode
                             :clean (measure project id project
                                             (into [bin "--compile" "--reloadable"] common))
                             :watch (watch-sample! project id project
                                                   (into [bin "--watch"] common) nil))
                    report (slurp (fs/file project (str id ".err")))
                    actual-hits (integer-stat report "cache.hits")]]
        (when-not (= hits actual-hits)
          (throw (ex-info "Unexpected namespace cache hits" {:dir (str project) :step id
                                                            :expected hits :actual actual-hits})))
        (save! log-dir "cache"
               (assoc sample :run run :side side :workload id :hits actual-hits
                      :misses (integer-stat report "cache.misses")
                      :macro-calls (integer-stat report "macro-calls")
                      :output-reused (integer-stat report "cache.output.reused")))))))

(defn suite! [log-dir root side features]
  (let [id (str "suite-" side)
        cmd (concat ["cargo" "nextest" "run" "--no-fail-fast"] features)
        sample (measure log-dir id root cmd {:allow-failure? true
                                              :env {"LIGHT_CLJS_V8_CACHE" "require"}})]
    (save! log-dir "suite" (assoc sample :workload (keyword side)))
    (System/exit (:exit sample))))

(defn median [xs]
  (let [v (vec (sort xs))
        n (count v)
        middle (quot n 2)]
    (if (odd? n)
      (v middle)
      (/ (+ (v (dec middle)) (v middle)) 2.0))))

(defn figures [xs digits]
  (let [number-format (str "%." digits "f")
        format-string (str number-format " [" number-format "–" number-format "]")]
    (apply format format-string (map double [(median xs) (apply min xs) (apply max xs)]))))

(defn summarize! [path]
  (let [samples (map edn/read-string (remove str/blank? (str/split-lines (slurp path))))]
    (doseq [[[workload side] rows] (sort-by key (group-by (juxt :workload :side) samples))]
      (println workload side (count rows)
               "s" (figures (map :seconds rows) 3)
               "MiB" (figures (map #(/ (:peak-bytes %) 1048576.0) rows) 2)
               "load" (figures (map :load rows) 2)
               (when (every? :bytes rows) (str "bytes " (figures (map :bytes rows) 0)))))))

(let [[mode log-dir master branch & options] *command-line-args*
      runs (some-> (first options) parse-long)]
  (when-not (= mode "summary")
    (fs/create-dirs log-dir))
  (case mode
    "build" (build! log-dir master branch runs)
    "output" (output! log-dir master branch)
    "sessions" (session! log-dir master branch runs)
    "readers" (readers! log-dir master branch runs)
    "compile" (let [[project build-id runs] options]
                (compile! log-dir master branch project build-id (parse-long runs)))
    "watch" (watch! log-dir master branch runs)
    "cache" (cache! log-dir master branch runs)
    "suite" (suite! log-dir master branch options)
    "summary" (summarize! log-dir)
    (throw (ex-info "Expected build|output|sessions|readers|watch|cache LOG_DIR MASTER_WORKTREE BRANCH_WORKTREE [RUNS], compile LOG_DIR MASTER_WORKTREE BRANCH_WORKTREE PROJECT BUILD RUNS, suite LOG_DIR WORKTREE LABEL [FEATURE_FLAGS], or summary FILE"
                    {:mode mode}))))
