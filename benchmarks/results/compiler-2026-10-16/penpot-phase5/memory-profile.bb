(require '[babashka.fs :as fs] '[clojure.string :as str])
(def destination "/private/tmp/light-cljs-phase5-profile")
(def vendor (fs/file destination "vendor"))
(when (fs/sym-link? vendor) (fs/delete vendor))
(fs/copy-tree "vendor" vendor {:replace-existing true})
(def c-file (fs/file vendor "rquickjs-sys/quickjs/quickjs.c"))
(defn edit [file before after]
  (let [text (slurp file)] (assert (str/includes? text before) before) (spit file (str/replace text before after))))
(edit c-file "    int64_t js_func_pc2line_size;\n} JSMemoryUsage_helper;"
      "    int64_t js_func_pc2line_size;\n    int64_t source_size, vardefs_size, cpool_size, closure_size;\n} JSMemoryUsage_helper;")
(edit c-file "        js_func_size += (b->arg_count + b->var_count) * sizeof(*b->vardefs);"
      "        js_func_size += (b->arg_count + b->var_count) * sizeof(*b->vardefs);\n        hp->vardefs_size += (b->arg_count + b->var_count) * sizeof(*b->vardefs);")
(edit c-file "        js_func_size += b->cpool_count * sizeof(*b->cpool);"
      "        js_func_size += b->cpool_count * sizeof(*b->cpool);\n        hp->cpool_size += b->cpool_count * sizeof(*b->cpool);")
(edit c-file "        js_func_size += b->closure_var_count * sizeof(*b->closure_var);"
      "        js_func_size += b->closure_var_count * sizeof(*b->closure_var);\n        hp->closure_size += b->closure_var_count * sizeof(*b->closure_var);")
(edit c-file "    js_func_size += b->source_len + 1;" "    js_func_size += b->source_len + 1;\n    hp->source_size += b->source_len + 1;")
(edit c-file "    s->js_func_count = mem.js_func_count;"
      "    fprintf(stderr, \"JS_FUNCTION_STORAGE source=%lld vardefs=%lld cpool=%lld closures=%lld\\n\", (long long)mem.source_size, (long long)mem.vardefs_size, (long long)mem.cpool_size, (long long)mem.closure_size);\n    s->js_func_count = mem.js_func_count;")
(spit (fs/file destination "src/profile.rs") "
#[repr(C)] #[derive(Default, Debug)] struct MallocStats { blocks: u32, used: usize, max: usize, reserved: usize }
unsafe extern \"C\" { fn malloc_zone_statistics(zone: *mut std::ffi::c_void, stats: *mut MallocStats); }
pub fn heap() { let mut stats = MallocStats::default(); unsafe { malloc_zone_statistics(std::ptr::null_mut(), &mut stats); } eprintln!(\"MALLOC {stats:?}\"); }
" :append true)
(edit (fs/file destination "src/evaluator.rs") "self.runtime.memory_usage());" "self.runtime.memory_usage()); crate::profile::heap();")
(def run (slurp "/private/tmp/light-cljs-phase3-profile/run.bb"))
(spit (fs/file destination "run.bb") (str/replace run "light-cljs-phase3-profile" "light-cljs-phase5-profile"))
(println destination)
