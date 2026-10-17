(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def file (fs/file here "project/src/macro_host.rs"))
(def original (slurp file))
(def profiler
"// Diagnostic only: separate nested macro and bridge-stage timing stacks.
mod macro_timing {
    use std::{cell::RefCell, collections::HashMap, time::Instant};
    #[derive(Default)]
    struct Group {
        stack: Vec<u128>,
        rows: HashMap<String, (u64, u128, u128)>,
    }
    #[derive(Default)]
    struct Profile { groups: [Group; 2] }
    impl Drop for Profile {
        fn drop(&mut self) {
            for (index, group) in self.groups.iter().enumerate() {
                let mut rows: Vec<_> = group.rows.iter().collect();
                rows.sort_by_key(|(_, row)| std::cmp::Reverse(row.2));
                for (label, (count, total, own)) in rows {
                    eprintln!(\"MACRO_TIMING\\t{}\\t{}\\t{}\\t{}\\t{}\", index, label, count, total, own);
                }
            }
        }
    }
    thread_local! { static PROFILE: RefCell<Profile> = RefCell::new(Profile::default()); }
    pub(super) struct Guard { group: usize, label: String, start: Instant }
    pub(super) fn begin(group: usize, label: impl Into<String>) -> Guard {
        PROFILE.with(|p| p.borrow_mut().groups[group].stack.push(0));
        Guard { group, label: label.into(), start: Instant::now() }
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            let duration = self.start.elapsed().as_nanos();
            PROFILE.with(|p| {
                let mut p = p.borrow_mut();
                let group = &mut p.groups[self.group];
                let children = group.stack.pop().unwrap();
                if let Some(parent) = group.stack.last_mut() { *parent += duration; }
                let entry = group.rows.entry(self.label.clone()).or_default();
                entry.0 += 1;
                entry.1 += duration;
                entry.2 += duration.saturating_sub(children);
            });
        }
    }
}
")
(defn once [s from to]
  (assert (= 1 (count (re-seq (re-pattern (java.util.regex.Pattern/quote from)) s))) (str "Expected one: " from))
  (str/replace s from to))
(def result
  (reduce (fn [s [from to]] (once s from to))
    (str original "\n" profiler)
    [["        origin: crate::compiler_data::ExpansionOrigin,\n    ) -> Result<Form, String> {"
      "        origin: crate::compiler_data::ExpansionOrigin,\n    ) -> Result<Form, String> {\n        let _macro_timing = macro_timing::begin(0, format!(\"{}/{}\", String::from_utf16_lossy(origin.macro_name.namespace().unwrap_or(&[])), String::from_utf16_lossy(origin.macro_name.name())));"]
     ["                args.push(evaluator.import_macro_form(call, limits)?);"
      "                { let _timer = macro_timing::begin(1, \"import_form\"); args.push(evaluator.import_macro_form(call, limits)?); }"]
     ["                args.push(evaluator.import_environment(environment, limits)?);"
      "                { let _timer = macro_timing::begin(1, \"import_environment\"); args.push(evaluator.import_environment(environment, limits)?); }"]
     ["                args.extend(evaluator.macro_arguments(args[0], call, limits)?);"
      "                { let _timer = macro_timing::begin(1, \"macro_arguments\"); args.extend(evaluator.macro_arguments(args[0], call, limits)?); }"]
     ["                let value = evaluator.call(function, &args)?;"
      "                let value = { let _timer = macro_timing::begin(1, \"call\"); evaluator.call(function, &args)? }; "]
     ["                evaluator.export_form(value, call, origin.clone(), limits)"
      "                { let _timer = macro_timing::begin(1, \"export_form\"); evaluator.export_form(value, call, origin.clone(), limits) }"]
     ["            if let Some(value) = result {"
      "            let _release_timer = macro_timing::begin(1, \"release\");\n            if let Some(value) = result {"]]))
(spit file result)
(spit (fs/file here "run.bb")
  (-> (slurp "/private/tmp/light-cljs-phase87/run.bb")
      (str/replace "light-cljs-quickjs-shared-storage" "light-cljs-macro-timing")
      (str/replace "QuickJS shared storage" "QuickJS macro timing")))
