#!/usr/bin/env bb
;; Build and serve the browser playground.
;;
;;   bb playground/build.bb build   compile playground/crate to wasm, run
;;                                  wasm-bindgen and assemble playground/dist
;;   bb playground/build.bb check   run playground/check.mjs (Node) against dist
;;   bb playground/build.bb serve   serve playground/dist on http://localhost:8787
;;   bb playground/build.bb         build, then serve
;;
;; Needs: a Rust toolchain with the wasm32-unknown-unknown target, a clang that
;; can emit wasm32 objects (Homebrew llvm on macOS; Apple's clang cannot), and
;; the wasm-bindgen CLI at the version playground/crate/Cargo.toml pins.
;; See playground/README.md.
;;
;; Toolchain discovery, each overridable through the environment:
;;   LIGHT_CLJS_CARGO   the cargo command (default: `rustup run stable cargo`
;;                      against ~/.rustup-wasm when that isolated install
;;                      exists, else `cargo`)
;;   CC_wasm32_unknown_unknown / AR_wasm32_unknown_unknown
;;                      the C compiler and archiver for QuickJS (default:
;;                      Homebrew llvm's clang and llvm-ar when installed)
;;   WASM_BINDGEN       the wasm-bindgen binary (default: on PATH)
(ns build
  (:require [babashka.fs :as fs]
            [babashka.process :as p]
            [clojure.string :as str]))

(def playground-dir (-> *file* fs/parent fs/absolutize fs/normalize))
(def crate-dir (fs/path playground-dir "crate"))
(def dist-dir (fs/path playground-dir "dist"))
(def wasm-name "light_cljs_playground")
(def static-files ["index.html" "app.js" "worker.js" "style.css"])

(defn fail [& message]
  (binding [*out* *err*] (println (apply str message)))
  (System/exit 1))

(defn brew-prefix [formula]
  (let [{:keys [exit out]} (try (p/sh ["brew" "--prefix" formula]) (catch Exception _ {:exit 1}))]
    (when (zero? exit) (str/trim out))))

(defn first-existing [paths]
  (some #(when (and % (fs/exists? %)) (str %)) paths))

(def isolated-rustup-home (str (fs/path (fs/home) ".rustup-wasm")))

(defn linker-env
  "wasm32 links with the toolchain's own `rust-lld`, which on macOS loads
  `libLLVM.dylib` through an rpath that rustup's layout does not satisfy
  (observed with Rust 1.99): name the toolchain's lib directory outright."
  [sysroot]
  (if (str/includes? (System/getProperty "os.name") "Mac")
    {"DYLD_LIBRARY_PATH" (str sysroot "/lib")}
    {}))

(defn cargo-command []
  (if-let [custom (System/getenv "LIGHT_CLJS_CARGO")]
    {:command (str/split custom #"\s+") :env {}}
    (let [rustup (or (fs/which "rustup")
                     (first-existing [(some-> (brew-prefix "rustup") (str "/bin/rustup"))]))]
      (if (and rustup (fs/directory? isolated-rustup-home))
        ;; An isolated install has no proxies on PATH, so cargo would find
        ;; another rustc: name the toolchain's own binaries outright.
        (let [env {"RUSTUP_HOME" isolated-rustup-home
                   "CARGO_HOME" (str (fs/path isolated-rustup-home "cargo"))}
              {:keys [exit out]} (p/sh {:extra-env env :continue true} (str rustup) "run" "stable" "rustc" "--print" "sysroot")
              sysroot (when (zero? exit) (str/trim out))]
          (when-not sysroot
            (fail isolated-rustup-home " exists but has no stable toolchain; see playground/README.md"))
          {:command [(str sysroot "/bin/cargo")]
           :env (merge env {"RUSTC" (str sysroot "/bin/rustc")} (linker-env sysroot))})
        ;; A regular rustup install (or a plain cargo with the target added).
        (let [{:keys [exit out]} (p/sh ["rustc" "--print" "sysroot"])]
          {:command ["cargo"]
           :env (if (zero? exit) (linker-env (str/trim out)) {})})))))

(defn c-toolchain-env []
  (let [llvm (brew-prefix "llvm")]
    {"CC_wasm32_unknown_unknown"
     (or (System/getenv "CC_wasm32_unknown_unknown")
         (first-existing [(some-> llvm (str "/bin/clang"))])
         "clang")
     "AR_wasm32_unknown_unknown"
     (or (System/getenv "AR_wasm32_unknown_unknown")
         (first-existing [(some-> llvm (str "/bin/llvm-ar"))])
         "llvm-ar")}))

(defn pinned-wasm-bindgen []
  (let [manifest (slurp (str (fs/path crate-dir "Cargo.toml")))]
    (or (second (re-find #"wasm-bindgen\s*=\s*\"=([0-9.]+)\"" manifest))
        (fail "playground/crate/Cargo.toml must pin wasm-bindgen = \"=VERSION\""))))

(defn wasm-bindgen-binary []
  (let [binary (or (System/getenv "WASM_BINDGEN") (some-> (fs/which "wasm-bindgen") str)
                   (fail "wasm-bindgen is not on PATH (brew install wasm-bindgen, or cargo install wasm-bindgen-cli --version " (pinned-wasm-bindgen) ")"))
        pinned (pinned-wasm-bindgen)
        installed (-> (p/sh [binary "--version"]) :out str/trim (str/split #"\s+") last)]
    (when (not= installed pinned)
      (fail "wasm-bindgen " installed " is installed but the crate pins " pinned
            "; install that CLI version or change the pin in playground/crate/Cargo.toml"))
    binary))

(defn run! [{:keys [command env dir]}]
  (println ">" (str/join " " command))
  (let [result (apply p/shell {:dir (str dir) :extra-env env :continue true} command)]
    (when-not (zero? (:exit result))
      (fail "command failed with exit " (:exit result)))))

(defn build []
  (let [{:keys [command env]} (cargo-command)
        env (merge env (c-toolchain-env))
        wasm-bindgen (wasm-bindgen-binary)]
    (println "C compiler for QuickJS:" (get env "CC_wasm32_unknown_unknown"))
    (run! {:command (into command ["build" "--release" "--target" "wasm32-unknown-unknown"])
           :env env :dir crate-dir})
    (let [wasm (fs/path crate-dir "target" "wasm32-unknown-unknown" "release" (str wasm-name ".wasm"))
          pkg (fs/path dist-dir "pkg")]
      (fs/create-dirs pkg)
      (run! {:command [wasm-bindgen "--target" "web" "--no-typescript" "--out-dir" (str pkg)
                       "--out-name" wasm-name (str wasm)]
             :env {} :dir playground-dir})
      (when-let [wasm-opt (fs/which "wasm-opt")]
        (let [bg (str (fs/path pkg (str wasm-name "_bg.wasm")))]
          (run! {:command [(str wasm-opt) "-O2" "--enable-bulk-memory" "--enable-nontrapping-float-to-int" "-o" bg bg]
                 :env {} :dir playground-dir})))
      (doseq [file static-files]
        (fs/copy (fs/path playground-dir file) (fs/path dist-dir file) {:replace-existing true}))
      (let [bg (fs/path pkg (str wasm-name "_bg.wasm"))]
        (println (format "built %s (%.1f MB) into %s" wasm-name (/ (fs/size bg) 1e6) dist-dir))))))

(def content-types
  {"html" "text/html; charset=utf-8"
   "js" "text/javascript; charset=utf-8"
   "css" "text/css; charset=utf-8"
   "wasm" "application/wasm"
   "json" "application/json"
   "map" "application/json"})

(defn serve [port]
  (when-not (fs/exists? (fs/path dist-dir "index.html"))
    (fail "playground/dist is not built; run `bb playground/build.bb build` first"))
  (let [root (fs/canonicalize dist-dir)
        handler (fn [{:keys [uri]}]
                  (let [relative (if (= uri "/") "index.html" (subs uri 1))
                        file (fs/path root relative)]
                    (if (and (fs/regular-file? file)
                             (str/starts-with? (str (fs/canonicalize file)) (str root)))
                      {:status 200
                       :headers {"Content-Type" (get content-types (fs/extension file) "application/octet-stream")
                                 "Cache-Control" "no-cache"}
                       :body (fs/file file)}
                      {:status 404 :headers {"Content-Type" "text/plain"} :body (str "not found: " uri)})))]
    (require 'org.httpkit.server)
    ((resolve 'org.httpkit.server/run-server) handler {:port port})
    (println (str "serving " root " at http://localhost:" port "/ (Ctrl-C to stop)"))
    @(promise)))

(defn check []
  (run! {:command ["node" (str (fs/path playground-dir "check.mjs"))] :env {} :dir playground-dir}))

(let [[command port] *command-line-args*
      port (if port (parse-long port) 8787)]
  (case command
    "build" (build)
    "check" (check)
    "serve" (serve port)
    nil (do (build) (serve port))
    (fail "usage: bb playground/build.bb [build|check|serve [PORT]]")))
