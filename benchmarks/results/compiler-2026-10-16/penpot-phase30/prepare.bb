(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file "/private/tmp/light-cljs-phase21/project"))
(doseq [path ["Cargo.toml" "Cargo.lock" "build.rs"]]
  (fs/copy path (fs/file project path) {:replace-existing true}))
(doseq [path ["src" "vendor" "benches"]]
  (fs/copy-tree path (fs/file project path) {:replace-existing true}))
(def root-source (slurp "src/compiler_roots.js"))
(spit (fs/file here "before.js") root-source)
(spit (fs/file project "src/compiler_roots.js")
  (str/replace root-source "get(_, name) { return force(name); },"
    "get(_, name) { const counts = globalThis.__rootReadCounts ??= new Map(); counts.set(name, (counts.get(name) ?? 0) + 1); return force(name); },"))
(def evaluator-path (fs/file project "src/evaluator.rs"))
(def evaluator-source (slurp evaluator-path))
(def anchor "                let _ = ctx.eval::<(), _>(\"__lightEvaluator.clear()\");")
(assert (str/includes? evaluator-source anchor))
(spit evaluator-path
  (str/replace evaluator-source anchor
    (str "                if let Ok(text) = ctx.eval::<String, _>(\"JSON.stringify([...globalThis.__rootReadCounts ?? []].sort((a,b) => b[1] - a[1]))\") { if text != \"[]\" { eprintln!(\"ROOT_READS {}\", text); } }\n" anchor)))
(spit (fs/file here "probe.js")
  (str "(() => { globalThis.__lightRuntimeForCompiler = rt;\n" root-source ";\n"
       "globalThis.__lightCompileUnit = () => 'rt => 42';\n"
       "__lightRegisterUnit('bench/value', 0, false, 'bench', null);\n"
       "if (__lightCompilerRoots['bench/value'] !== 42) throw Error('initial value');\n"
       "let total = 0; const start = Date.now();\n"
       "for (let i = 0; i < 1000000; i++) total += __lightCompilerRoots['bench/value'];\n"
       "if (total !== 42000000) throw Error('incorrect sum');\n"
       "return JSON.stringify({reads: 1000000, milliseconds: Date.now() - start}); })()\n"))
(println "Prepared isolated compiler-root read counter")
