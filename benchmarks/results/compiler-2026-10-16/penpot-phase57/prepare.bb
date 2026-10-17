(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(fs/create-dirs project)
(p/shell "rsync" "-a" "--exclude=target" "--exclude=.git" "/private/tmp/light-cljs-phase56/project/" (str project "/"))
(def file (fs/file project "src/v8_evaluator.rs"))
(def before (slurp file))
(def old "let source =\n                include_str!(\"v8_services.js\").replace(\"__LIGHT_RUNTIME_SOURCE__\", RUNTIME_SOURCE);")
(def new "let mut runtime = RUNTIME_SOURCE.to_owned();
            if let Ok(trial) = std::env::var(\"LIGHT_V8_RUNTIME_TRIAL\") {
                let store = \"class Store extends (function (value) { return value; }) { #s; static has(value) { return (typeof value === 'object' ? value !== null : typeof value === 'function') && #s in value; } static get(value) { if ((typeof value === 'object' ? value !== null : typeof value === 'function') && #s in value) return value.#s; } static set(value, payload) { if (#s in value) value.#s = payload; else new Store(value).#s = payload; } }\";
                if trial == \"seq-stores\" || trial == \"all-stores\" {
                    for name in [\"cells\", \"views\", \"lazyValues\", \"chunks\"] {
                        let old = format!(\"const {name} = privateStore()\");
                        assert!(runtime.contains(&old));
                        runtime = runtime.replace(&old, &format!(\"const {name} = {store}\"));
                    }
                }
                if trial == \"borrowed-store\" || trial == \"all-stores\" {
                    runtime = runtime.replace(\"const borrowed = new WeakMap()\", &format!(\"const borrowed = {store}\"));
                }
                if trial == \"metadata-store\" || trial == \"all-stores\" {
                    runtime = runtime.replace(\"const metadataValues = new WeakMap()\", &format!(\"const metadataValues = {store}\"));
                }
            }
            let source = include_str!(\"v8_services.js\").replace(\"__LIGHT_RUNTIME_SOURCE__\", &runtime);")
(assert (str/includes? before old))
(spit file (str/replace before old new))
(p/shell {:dir (str project)} "cargo" "fmt")
(fs/copy "/private/tmp/light-cljs-phase56/run-bench.bb" (fs/file here "run-bench.bb") {:replace-existing true})
(spit (fs/file here "build.bb")
  "(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])\n(def here (fs/parent *file*))\n(def project (fs/file here \"project\"))\n(p/shell \"touch\" (str (fs/file project \"src/lib.rs\")) (str (fs/file project \"src/main.rs\")))\n(with-open [out (io/output-stream (fs/file here \"build.log\"))]\n (p/shell {:dir (str project) :out out :err :out :extra-env {\"CARGO_TARGET_DIR\" \"/private/tmp/light-cljs-phase43/v8-probe/target\" \"CARGO_PROFILE_RELEASE_LTO\" \"thin\" \"CARGO_PROFILE_RELEASE_CODEGEN_UNITS\" \"1\"}} \"cargo\" \"build\" \"--release\"))\n(fs/copy \"/private/tmp/light-cljs-phase43/v8-probe/target/release/light-cljs\" (fs/file here \"light-cljs-runtime-trials\") {:replace-existing true})\n")
(spit (fs/file here "run.bb")
  "(require '[babashka.fs :as fs] '[babashka.process :as p])\n(def here (fs/parent *file*))\n(doseq [trial (or (seq *command-line-args*) [\"seq-stores\" \"borrowed-store\" \"metadata-store\"])]\n (p/shell {:extra-env {\"LIGHT_V8_BINARY\" (str (fs/file here \"light-cljs-runtime-trials\")) \"LIGHT_V8_HEAP_BYTES\" \"268435456\" \"LIGHT_V8_RUNTIME_TRIAL\" trial}} \"bb\" (str (fs/file here \"run-bench.bb\")) \"app.main\" trial)\n (p/shell \"cmp\" (str (fs/file here (str trial \".js\"))) \"/private/tmp/light-cljs-phase42/production/main-none.js\"))\n")
(def bench (fs/file here "run-bench.bb"))
(spit bench (str/replace (slurp bench) "\"LIGHT_V8_HEAP_BYTES\" \"LIGHT_V8_STATS\"" "\"LIGHT_V8_RUNTIME_TRIAL\" \"LIGHT_V8_HEAP_BYTES\" \"LIGHT_V8_STATS\""))
(println "Prepared compiler-runtime-only store trials; generated application runtime remains unchanged.")
