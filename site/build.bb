#!/usr/bin/env bb
;; Assemble and serve the GitHub Pages site: the landing page in site/ plus the
;; browser playground at playground/.
;;
;;   bb site/build.bb build                     build the playground, then
;;                                              assemble site/dist
;;   bb site/build.bb build --reuse-playground  assemble site/dist from the
;;                                              existing playground/dist
;;   bb site/build.bb serve [PORT]              serve site/dist as GitHub Pages
;;                                              does, under /light-cljs/ on
;;                                              http://localhost:8788
;;
;; The deployment (.github/workflows/pages.yml) builds and checks the
;; playground first, then runs `build --reuse-playground` and publishes
;; site/dist. Every link in the site is relative, so the same files work at
;; https://isaksky.github.io/light-cljs/ and locally.
(ns site
  (:require [babashka.fs :as fs]
            [babashka.process :as p]
            [clojure.string :as str]))

(def site-dir (-> *file* fs/parent fs/absolutize fs/normalize))
(def repo-dir (fs/parent site-dir))
(def dist-dir (fs/path site-dir "dist"))
(def playground-dist (fs/path repo-dir "playground" "dist"))
(def static-files ["index.html" "style.css"])
;; The path GitHub Pages serves the project site under.
(def base-path "/light-cljs/")

(defn fail [& message]
  (binding [*out* *err*] (println (apply str message)))
  (System/exit 1))

(defn build [reuse-playground?]
  (when-not reuse-playground?
    (let [{:keys [exit]} (p/shell {:dir (str repo-dir) :continue true}
                                  "bb" "playground/build.bb" "build")]
      (when-not (zero? exit) (fail "playground build failed with exit " exit))))
  (when-not (fs/exists? (fs/path playground-dist "pkg" "light_cljs_playground_bg.wasm"))
    (fail "playground/dist is not built; run `bb playground/build.bb build` first"))
  (fs/delete-tree dist-dir)
  (fs/create-dirs dist-dir)
  (doseq [file static-files]
    (fs/copy (fs/path site-dir file) (fs/path dist-dir file)))
  (fs/copy-tree playground-dist (fs/path dist-dir "playground"))
  (let [files (filter fs/regular-file? (file-seq (fs/file dist-dir)))]
    (println (format "assembled %s: %d files, %.1f MB" dist-dir (count files)
                     (/ (reduce + (map fs/size files)) 1e6)))))

(def content-types
  {"html" "text/html; charset=utf-8"
   "js" "text/javascript; charset=utf-8"
   "css" "text/css; charset=utf-8"
   "wasm" "application/wasm"
   "json" "application/json"
   "map" "application/json"})

(defn not-found [uri]
  {:status 404 :headers {"Content-Type" "text/plain"} :body (str "not found: " uri)})

(defn redirect [location]
  {:status 301 :headers {"Location" location} :body ""})

(defn serve [port]
  (when-not (fs/exists? (fs/path dist-dir "index.html"))
    (fail "site/dist is not built; run `bb site/build.bb build` first"))
  (let [root (fs/canonicalize dist-dir)
        handler
        (fn [{:keys [uri]}]
          (cond
            (= uri "/") (redirect base-path)
            (= uri (subs base-path 0 (dec (count base-path)))) (redirect base-path)
            (not (str/starts-with? uri base-path)) (not-found uri)
            :else
            (let [file (fs/canonicalize (fs/path root (subs uri (count base-path))))]
              (cond
                ;; Compare path components, so a sibling such as dist-backup
                ;; is outside the root.
                (not (fs/starts-with? file root)) (not-found uri)
                ;; Like GitHub Pages: a directory redirects to its slash form,
                ;; whose index.html the relative links resolve against.
                (and (fs/directory? file) (not (str/ends-with? uri "/")))
                (redirect (str uri "/"))
                :else
                (let [file (if (fs/directory? file) (fs/path file "index.html") file)]
                  (if (fs/regular-file? file)
                    {:status 200
                     :headers {"Content-Type" (get content-types (fs/extension file) "application/octet-stream")
                               "Cache-Control" "no-cache"}
                     :body (fs/file file)}
                    (not-found uri)))))))]
    (require 'org.httpkit.server)
    ((resolve 'org.httpkit.server/run-server) handler {:port port})
    (println (str "serving " root " at http://localhost:" port base-path " (Ctrl-C to stop)"))
    @(promise)))

(let [[command & args] *command-line-args*]
  (case command
    "build" (case (vec args)
              [] (build false)
              ["--reuse-playground"] (build true)
              (fail "usage: bb site/build.bb build [--reuse-playground]"))
    "serve" (serve (if-let [port (first args)] (parse-long port) 8788))
    (fail "usage: bb site/build.bb build [--reuse-playground] | serve [PORT]")))
