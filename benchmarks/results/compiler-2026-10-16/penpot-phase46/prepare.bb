(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(fs/create-dirs project)
(p/shell "rsync" "-a" "--exclude=target" "--exclude=.git"
  "/private/tmp/light-cljs-phase45/project/" (str project "/"))
(def path (fs/file project "src/compiler_data.rs"))
(def original (slurp path))
(def old "namespace: namespace.map(|s| s.encode_utf16().collect()),\n            name: name.encode_utf16().collect(),")
(def replacement "namespace: namespace.map(name_utf16),\n            name: name_utf16(name),")
(assert (str/includes? original old))
(def helper "// ASCII's exact-size byte iterator lets Rc allocate the final slice directly.\n// EncodeUtf16's unknown exact length otherwise requires a temporary Vec.\nfn name_utf16(text: &str) -> Rc<[u16]> {\n    if text.is_ascii() {\n        text.bytes().map(u16::from).collect()\n    } else {\n        text.encode_utf16().collect()\n    }\n}\n\n")
(spit path (-> original (str/replace old replacement)
              (str/replace "impl Name {" (str helper "impl Name {"))))
(fs/copy "/private/tmp/light-cljs-phase45/run-bench.bb" (fs/file here "run-bench.bb") {:replace-existing true})
(p/shell {:dir (str project)} "cargo" "fmt")
(with-open [out (java.io.FileOutputStream. (fs/file here "build.log"))]
  (p/shell {:dir (str project) :out out :err :out
            :extra-env {"CARGO_TARGET_DIR" "/private/tmp/light-cljs-phase43/v8-probe/target"}}
    "cargo" "build" "--release"))
(fs/copy "/private/tmp/light-cljs-phase43/v8-probe/target/release/light-cljs"
  (fs/file here "light-cljs-names") {:replace-existing true})
(p/shell {:extra-env {"LIGHT_V8_BINARY" (str (fs/file here "light-cljs-names"))
                      "LIGHT_V8_HEAP_BYTES" "268435456"}}
  "bb" (str (fs/file here "run-bench.bb")) "app.main" "main-names")
