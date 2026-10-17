(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def file (fs/file here "project/vendor/rquickjs-sys/quickjs/quickjs.c"))
(assert (str/starts-with? (str (fs/real-path file)) (str here)))
(def from "    ctx->interrupt_counter = JS_INTERRUPT_COUNTER_INIT;\n    if (rt->interrupt_handler) {")
(def to
"    /* Diagnostic sampling at interpreter safepoints, without executing JS. */
    static uint64_t light_next_sample;
    uint64_t light_now = js__hrtime_ns();
    if (light_now >= light_next_sample) {
        light_next_sample = light_now + 10000000;
        fprintf(stderr, \"QJS_SAMPLE\\t%\" PRIu64, light_now);
        JSStackFrame *light_frame = rt->current_stack_frame;
        for (int depth = 0; light_frame && depth < 24; depth++, light_frame = light_frame->prev_frame) {
            if (JS_VALUE_GET_TAG(light_frame->cur_func) != JS_TAG_OBJECT) continue;
            JSObject *light_object = JS_VALUE_GET_OBJ(light_frame->cur_func);
            if (js_class_has_bytecode(light_object->class_id)) {
                JSFunctionBytecode *light_bytecode = light_object->u.func.function_bytecode;
                char light_name[256];
                fprintf(stderr, \"\\t%s:%d\", JS_AtomGetStrRT(rt, light_name, sizeof(light_name), light_bytecode->func_name), light_bytecode->line_num);
            } else fprintf(stderr, \"\\t<native>\");
        }
        fputc('\\n', stderr);
    }
    ctx->interrupt_counter = JS_INTERRUPT_COUNTER_INIT;
    if (rt->interrupt_handler) {")
(def engine-source (slurp file))
(assert (= 1 (count (re-seq (re-pattern (java.util.regex.Pattern/quote from)) engine-source))))
(spit file (str/replace engine-source from to))
(spit (fs/file here "run.bb")
  (-> (slurp "/private/tmp/light-cljs-phase87/run.bb")
      (str/replace "light-cljs-quickjs-shared-storage" "light-cljs-quickjs-sampling")
      (str/replace "QuickJS shared storage" "QuickJS function sampling")))
