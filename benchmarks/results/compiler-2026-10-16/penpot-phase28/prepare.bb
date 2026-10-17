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
(replace! "lib.rs" "mod compiler_bootstrap;" "mod compiler_bootstrap;\nmod perf_phase28;")
(spit (fs/file project "src/perf_phase28.rs")
"use std::{cell::RefCell, collections::BTreeMap, time::{Duration, Instant}};
#[derive(Default)] struct Stats(BTreeMap<&'static str, (usize, Duration)>);
impl Drop for Stats { fn drop(&mut self) { for (name, (count, time)) in &self.0 { eprintln!(\"PHASE28 {} {} {:.6}\", name, count, time.as_secs_f64()); } } }
thread_local! { static STATS: RefCell<Stats> = RefCell::new(Stats::default()); }
pub(crate) struct Timer(&'static str, Instant);
pub(crate) fn timer(name: &'static str) -> Timer { Timer(name, Instant::now()) }
impl Drop for Timer { fn drop(&mut self) { let duration = self.1.elapsed(); STATS.with(|s| { let mut s = s.borrow_mut(); let item = s.0.entry(self.0).or_default(); item.0 += 1; item.1 += duration; }); } }
pub(crate) fn measure<T>(name: &'static str, f: impl FnOnce() -> T) -> T { let _timer = timer(name); f() }
")
(doseq [[before after]
        [["args.push(evaluator.import_form(call, limits)?);"
          "args.push(crate::perf_phase28::measure(\"form-import\", || evaluator.import_form(call, limits))?);"]
         ["args.push(evaluator.import_environment(environment, limits)?);"
          "args.push(crate::perf_phase28::measure(\"environment-import\", || evaluator.import_environment(environment, limits))?);"]
         ["args.push(evaluator.import_form(argument, limits)?);"
          "args.push(crate::perf_phase28::measure(\"argument-import\", || evaluator.import_form(argument, limits))?);"]
         ["let value = evaluator.call(function, &args)?;"
          "let value = crate::perf_phase28::measure(\"macro-call\", || evaluator.call(function, &args))?;"]
         ["evaluator.export_form(value, call, origin.clone(), limits)"
          "crate::perf_phase28::measure(\"result-export\", || evaluator.export_form(value, call, origin.clone(), limits))"]]]
  (replace! "macro_host.rs" before after))
(replace! "compiler_bootstrap.rs"
          "&& elidable_environment(env, &mut remaining, 128, &engine.control())"
          "&& crate::perf_phase28::measure(\"environment-proof\", || elidable_environment(env, &mut remaining, 128, &engine.control()))")
(replace! "program_fragment.rs"
          "    let exprs = Ids::new(shared.exprs, target.exprs.len());"
          "    let _timer = crate::perf_phase28::timer(\"fragment-append\");\n    let exprs = Ids::new(shared.exprs, target.exprs.len());")
(replace! "emitter.rs"
          "fn emit_unit_mode(program: &Program, demand_roots: bool) -> String {"
          "fn emit_unit_mode(program: &Program, demand_roots: bool) -> String {\n    let _timer = crate::perf_phase28::timer(\"unit-emission\");")
(replace! "emitter.rs"
          "    let raw = program\n        .strings"
          "    let raw_timer = crate::perf_phase28::timer(\"unit-name-scan\");\n    let raw = program\n        .strings")
(replace! "emitter.rs"
          "        .collect::<String>();\n    let mut index = 0;"
          "        .collect::<String>();\n    drop(raw_timer);\n    let mut index = 0;")
(println "Prepared isolated diagnostic source" project)
