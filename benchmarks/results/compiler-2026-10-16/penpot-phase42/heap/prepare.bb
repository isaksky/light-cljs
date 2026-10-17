(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file "/private/tmp/light-cljs-phase21/project"))
(doseq [path ["Cargo.toml" "Cargo.lock" "build.rs"]]
  (fs/copy path (fs/file project path) {:replace-existing true}))
(doseq [path ["src" "vendor" "benches" "tests" "examples"]]
  (fs/copy-tree path (fs/file project path) {:replace-existing true}))
(def file (fs/file project "src/compiler.rs"))
(def before "    let files = loader\n        .ordered\n        .into_iter()")
(def inserted "    if let Ok(directory) = std::env::var(\"LIGHT_HEAP_PROFILE\") {\n        for phase in [\"before\", \"after\"] {\n            if phase == \"after\" { *macros = MacroHost::default(); }\n            let ready = std::path::Path::new(&directory).join(format!(\"{phase}.ready\"));\n            let resume = std::path::Path::new(&directory).join(format!(\"{phase}.resume\"));\n            std::fs::write(ready, \"ready\\n\").unwrap();\n            while !resume.exists() { std::thread::sleep(std::time::Duration::from_millis(20)); }\n        }\n    }\n")
(let [text (slurp file)]
  (assert (str/includes? text before))
  (spit file (str/replace text before (str inserted before))))
(println "Prepared live-heap diagnostic; production unchanged")
