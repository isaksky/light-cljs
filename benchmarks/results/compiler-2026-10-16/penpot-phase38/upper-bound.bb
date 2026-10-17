(require '[babashka.fs :as fs] '[clojure.string :as str] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project "/private/tmp/light-cljs-phase21/project")
(def target "/private/tmp/light-cljs-phase13-storage-profile/target")
(def baseline (slurp (fs/file here "before.c")))
(def declaration "static JSValue js_call_c_function(JSContext *ctx, JSValueConst func_obj,")
(def at (str/index-of baseline declaration))
(def declared (str (subs baseline 0 at)
  "static JSValue js_function_call(JSContext *ctx, JSValueConst this_val,\n                                int argc, JSValueConst *argv);\n"
  (subs baseline at)))
(def begin (str/index-of declared "/* argv[] is modified if (flags & JS_CALL_FLAG_COPY_ARGV) = 0. */"))
(def finish (str/index-of declared "JSValue JS_Call(JSContext" begin))
(assert (and begin finish))
(def body (subs declared begin finish))
(def updated (-> body
  (str/replace "    if (js_poll_interrupts(caller_ctx))\n" " restart_direct_call:\n    if (js_poll_interrupts(caller_ctx))\n")
  (str/replace "    p = JS_VALUE_GET_OBJ(func_obj);\n    if (unlikely(p->class_id != JS_CLASS_BYTECODE_FUNCTION)) {"
    "    p = JS_VALUE_GET_OBJ(func_obj);\n    if (p->class_id == JS_CLASS_C_FUNCTION &&\n        p->u.cfunc.cproto == JS_CFUNC_generic &&\n        p->u.cfunc.c_function.generic == js_function_call &&\n        !(flags & JS_CALL_FLAG_CONSTRUCTOR)) {\n        /* Upper-bound diagnostic only: removes observable native stack frames. */\n        caller_ctx = p->u.cfunc.realm;\n        func_obj = this_obj;\n        this_obj = argc > 0 ? argv[0] : JS_UNDEFINED;\n        if (argc > 0) { argc--; argv++; }\n        goto restart_direct_call;\n    }\n    if (unlikely(p->class_id != JS_CLASS_BYTECODE_FUNCTION)) {")))
(assert (not= body updated))
(def candidate (str (subs declared 0 begin) updated (subs declared finish)))
(spit (fs/file here "upper-bound.c") candidate)
(spit (fs/file project "vendor/rquickjs-sys/quickjs/quickjs.c") candidate)
(println "Building deliberately nonproduction upper-bound diagnostic") (flush)
(with-open [out (io/output-stream (fs/file here "upper-bound-build.log"))]
  (p/shell {:dir project :extra-env {"CARGO_TARGET_DIR" target} :out out :err :out}
    "cargo" "build" "--release" "--example" "replay_probe"))
(fs/copy (fs/file target "release/examples/replay_probe") (fs/file here "probe-upper-bound") {:replace-existing true})
(doseq [variant ["before" "after" "upper-bound"]]
  (with-open [out (io/output-stream (fs/file here (str variant "-semantics.log")))]
    (let [result @(p/process [(str (fs/file here (str "probe-" variant)))
                             (str (fs/file here "runtime.js")) (str (fs/file here "semantics.js"))]
                            {:out out :err :out})]
      (println "Semantic probe" variant "exit" (:exit result)))))
(doseq [variant ["upper-bound" "before"]]
  (println "Replaying final" variant) (flush)
  (with-open [out (io/output-stream (fs/file here (str variant "-final.log")))]
    (p/shell {:out out :err :out} "/usr/bin/time" "-l" (str (fs/file here (str "probe-" variant)))
      (str (fs/file here "runtime.js")) "/private/tmp/light-cljs-phase31/replay.js")))
