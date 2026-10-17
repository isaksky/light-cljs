(require '[babashka.fs :as fs] '[clojure.string :as str])
(def destination "/private/tmp/light-cljs-phase5-profile")
(fs/create-dirs destination)
(doseq [file ["Cargo.toml" "Cargo.lock" "build.rs"]] (fs/copy file (fs/path destination file) {:replace-existing true}))
(fs/copy-tree "src" (fs/file destination "src") {:replace-existing true})
(doseq [dir ["vendor" "benches"]]
  (let [target (fs/path destination dir)] (when-not (fs/exists? target) (fs/create-sym-link target (fs/absolutize dir)))))
(defn edit [file before after]
  (let [path (fs/file destination "src" file) text (slurp path)]
    (assert (str/includes? text before) (str "Missing " file " " before))
    (spit path (str/replace text before after))))
(fs/copy (fs/file (fs/parent *file*) "profile.rs") (fs/file destination "src/profile.rs") {:replace-existing true})
(spit (fs/file destination "src/profile.rs") "
thread_local! { static NAMES:RefCell<std::collections::HashMap<String, &'static str>>=RefCell::new(Default::default()); }
pub fn named_scope(prefix: &str, name: &crate::compiler_data::Name)->Guard {
 let label=format!(\"{prefix}/{}/{}\", String::from_utf16_lossy(name.namespace().unwrap_or(&[])), String::from_utf16_lossy(name.name()));
 let name=NAMES.with(|names| { let mut names=names.borrow_mut(); *names.entry(label.clone()).or_insert_with(|| Box::leak(label.into_boxed_str())) }); scope(name)
}
" :append true)
(edit "lib.rs" "pub mod analyzer;" "pub(crate) mod profile;\npub mod analyzer;")
(edit "compiler.rs" "    loader.visit(entry)?;" "    let _profile = crate::profile::scope(\"analysis-other\");\n    loader.visit(entry)?;")
(edit "compiler.rs" "    Ok((program, files))" "    crate::profile::report();\n    if let Some(bootstrap) = macros.bootstrap.clone().or_else(|| macros.session_handle().and_then(|s| s.bootstrap())) { bootstrap.evaluator.profile_memory(); }\n    Ok((program, files))")
(edit "evaluator.rs" "    pub fn collect_garbage(&self) {" "    pub(crate) fn profile_memory(&self) { eprintln!(\"JS_MEMORY_BEFORE {:?}\", self.runtime.memory_usage()); self.collect_garbage(); eprintln!(\"JS_MEMORY_AFTER {:?}\", self.runtime.memory_usage()); }\n    pub fn collect_garbage(&self) {")
(edit "emitter.rs" "fn emit_unit_mode(program: &Program, demand_roots: bool) -> String {" "fn emit_unit_mode(program: &Program, demand_roots: bool) -> String {\n    let _profile = crate::profile::scope(\"unit-emission\");")
(edit "emitter.rs" "    ) -> String {\n        let program = self.program;\n        let mut registration" "    ) -> String {\n        let _profile = crate::profile::scope(\"namespace-registration\");\n        let program = self.program;\n        let mut registration")
(edit "evaluator.rs" "    pub fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {" "    pub fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {\n        let _profile = crate::profile::scope(\"unit-evaluation\");")
(edit "macro_host.rs" "                args.push(evaluator.import_form(call, limits)?);" "                {let _profile=crate::profile::scope(\"form-import\"); args.push(evaluator.import_form(call, limits)?);}")
(edit "macro_host.rs" "                args.push(evaluator.import_environment(environment, limits)?);" "                {let _profile=crate::profile::scope(\"environment-import\"); args.push(evaluator.import_environment(environment, limits)?);}")
(edit "macro_host.rs" "                    args.push(evaluator.import_form(argument, limits)?);" "                    let _profile=crate::profile::scope(\"argument-import\");\n                    args.push(evaluator.import_form(argument, limits)?);")
(edit "macro_host.rs" "                let value = evaluator.call(function, &args)?;" "                let value = {let _profile=crate::profile::named_scope(\"macro-call\", &origin.macro_name); evaluator.call(function, &args)?};")
(edit "macro_host.rs" "                evaluator.export_form(value, call, origin.clone(), limits)" "                {let _profile=crate::profile::scope(\"form-export\"); evaluator.export_form(value, call, origin.clone(), limits)}")
(edit "analyzer.rs" "    fn collect_expression_warnings(&mut self) {" "    fn collect_expression_warnings(&mut self) {\n        let _profile = crate::profile::scope(\"warnings\");")
(edit "macro_host.rs" "        let imports = self.imports.get(namespace).cloned().unwrap_or_default();" "        let _profile = crate::profile::scope(\"build-environment\");\n        let imports = self.imports.get(namespace).cloned().unwrap_or_default();")
(edit "macro_host.rs" "        let c = crate::compiler_data::DataBuilder;\n        let string = |s: &str|" "        let _profile = crate::profile::scope(\"build-reader-environment\");\n        let c = crate::compiler_data::DataBuilder;\n        let string = |s: &str|")
(edit "macro_host.rs" "cache.as_ref().filter(|c| c.locals == locals) {" "cache.as_ref().filter(|c| c.locals == locals) {\n                    crate::profile::count(\"local-cache-hit\", 1);")
(edit "macro_host.rs" "*cache = Some(LocalEnvironment {" "crate::profile::count(\"local-cache-miss\", 1);\n                    *cache = Some(LocalEnvironment {")
(edit "macro_host.rs" "cache.as_ref().filter(|c| c.key == context_key) {" "cache.as_ref().filter(|c| c.key == context_key) {\n                crate::profile::count(\"reader-cache-hit\", 1);")
(edit "macro_host.rs" "*cache = Some(ReaderEnvironment {" "crate::profile::count(\"reader-cache-miss\", 1);\n                *cache = Some(ReaderEnvironment {")
(edit "form_bridge.rs" "                remaining -= nodes;" "                crate::profile::count(\"environment-cached-nodes\", nodes as u64);\n                remaining -= nodes;")
(println destination)
(edit "evaluator.rs" "        Ok(session)" "        session.enter(|ctx| { let start=std::time::Instant::now(); ctx.globals().set(\"__lightProfileClock\", Function::new(ctx.clone(), move || start.elapsed().as_secs_f64())?) })?;\n        Ok(session)")
(edit "evaluator.rs" "pub(crate) fn profile_memory(&self) {" "pub(crate) fn profile_memory(&self) { let id=self.evaluate_js(\"JSON.stringify(globalThis.__lightReaderPreparation)\").unwrap(); eprintln!(\"READER_PREPARATION {}\", self.string(id).unwrap()); self.release(id).unwrap();")
(edit "js_runtime.js" "  function withMacroReaderContext(fn, args) {" "  function withMacroReaderContext(fn, args) {\n    const profileStart = __lightProfileClock();")
(edit "js_runtime.js" "    try { return Reflect.apply(fn, null, args); }\n    finally { ns.readerContext = previous; coreCells.currentNs = saved; }"
      "    const stats = globalThis.__lightReaderPreparation ??= {calls: 0, seconds: 0}; stats.calls++; stats.seconds += __lightProfileClock() - profileStart;\n    try { return Reflect.apply(fn, null, args); }\n    finally { ns.readerContext = previous; coreCells.currentNs = saved; }")
(edit "compiler.rs" "    crate::profile::report();" "    eprintln!(\"PROGRAM exprs={} expr-size={} globals={} locals={} strings={} units={} namespaces={}\", program.exprs.len(), std::mem::size_of::<crate::analyzer::Expr>(), program.globals.len(), program.locals.len(), program.strings.len(), program.compilation_uses.len(), program.namespaces.len());\n    crate::profile::report();")
