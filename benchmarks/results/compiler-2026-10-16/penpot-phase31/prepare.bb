(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file "/private/tmp/light-cljs-phase21/project"))
(doseq [path ["Cargo.toml" "Cargo.lock" "build.rs"]]
  (fs/copy path (fs/file project path) {:replace-existing true}))
(doseq [path ["src" "vendor" "benches"]]
  (fs/copy-tree path (fs/file project path) {:replace-existing true}))
(defn replace! [file before after]
  (let [path (fs/file project "src" file) source-text (slurp path)]
    (assert (str/includes? source-text before) (str "Missing anchor " file " " before))
    (spit path (str/replace source-text before after))))
(def record-js (-> (slurp (fs/file here "record.js")) (str/replace "{" "{{") (str/replace "}" "}}")))
(replace! "evaluator.rs" "              const bridge = rt.compilerForms;"
          (str "              const bridge = rt.compilerForms;\n" record-js))
(replace! "evaluator.rs"
          "try {{ const result = save((0, eval)(source)(rt)); hostDiagnostic = null; return result; }}"
          "try {{ const value = (0, eval)(source)(rt); evaluatedSources.push(source); if (typeof value === 'function') functionSources.set(value, source); const result = save(value); hostDiagnostic = null; return result; }}")
(replace! "evaluator.rs"
          "save(Reflect.apply(get(id), null, ids.map(get)))"
          "save(invokeRecorded(id, ids))")
(replace! "evaluator.rs"
          "                let _ = ctx.eval::<(), _>(\"__lightEvaluator.clear()\");"
          (str "                if let Ok(text) = ctx.eval::<String, _>(\"JSON.stringify(globalThis.__macroCapture ?? null)\") {\n"
               "                    if text != \"null\" { std::fs::write(\"/private/tmp/light-cljs-phase31/capture.json\", text).unwrap(); }\n"
               "                }\n"
               "                let _ = ctx.eval::<(), _>(\"__lightEvaluator.clear()\");"))
(replace! "compiler_roots.js"
          "  let frontier = Infinity, declarationFrontier = Infinity;"
          (str "  let frontier = Infinity, declarationFrontier = Infinity;\n"
               "  globalThis.__captureFrontiers = () => [frontier, declarationFrontier].map(v => v === Infinity ? null : v);\n"
               "  globalThis.__restoreCaptureFrontiers = (a, b) => { frontier = a ?? Infinity; declarationFrontier = b ?? Infinity; };"))
(replace! "compilation_session.rs" "let active = evaluator.active(ctx).map_err(|e| e.to_string())?;"
          "let active = evaluator.active(ctx.clone()).map_err(|e| e.to_string())?;")
(replace! "compilation_session.rs"
          "                Ok(crate::emitter::emit_deferred_unit(&program))"
          (str "                let source = crate::emitter::emit_deferred_unit(&program);\n"
               "                let captured: rquickjs::Object = ctx.globals().get(\"__capturedUnits\").unwrap();\n"
               "                captured.set(id.to_string(), source.clone()).unwrap();\n"
               "                Ok(source)"))
(replace! "macro_host.rs"
          "                let value = evaluator.call(function, &args)?;"
          (str "                if origin.macro_name == crate::compiler_data::Name::new(Some(\"instaparse.abnf\"), \"precompile-cljs-grammar\") {\n"
               "                    let flag = evaluator.evaluate_unit(\"rt => {globalThis.__captureNextMacro = true; return null;}\")?;\n"
               "                    evaluator.release(flag)?;\n"
               "                }\n"
               "                let value = evaluator.call(function, &args)?;"))
(fs/copy (fs/file project "src/compiler_roots.js") (fs/file here "compiler_roots.js") {:replace-existing true})
(println "Prepared isolated Instaparse macro capture")
