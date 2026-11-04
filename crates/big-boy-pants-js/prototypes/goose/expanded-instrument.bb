;; Loaded only by expanded-benchmark.bb --instrument. Generate private copies;
;; clean measurements always compile the unmodified compiler_session.goose.
(def instrument-dir (fs/path target "instrumented"))
(fs/create-dirs instrument-dir)
(doseq [pattern ["*.goose" "*.build" "*.h"] file (fs/glob directory pattern)]
  (fs/copy file (fs/path instrument-dir (fs/file-name file)) {:replace-existing true}))
(def activity-c "
/* Separate measurement build: append flat interval/queue records. */
#include <fcntl.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
static void goose_bench_interval(gs_i64 id, gs_i64 chunk, gs_i64 mode,
                                 gs_i64 phase, gs_i64 begin, gs_i64 end) {
    const char *path = getenv(\"GOOSE_BENCH_ACTIVITY\");
    if (!path) return;
    char text[320];
    int length = snprintf(text, sizeof text,
        \"{:kind :interval :id %lld :chunk %lld :mode %lld :phase %lld :start-ns %lld :end-ns %lld}\\n\",
        (long long)id, (long long)chunk, (long long)mode, (long long)phase,
        (long long)begin, (long long)end);
    int fd = open(path, O_WRONLY | O_CREAT | O_APPEND, 0600);
    if (fd < 0 || length < 0 || (size_t)length >= sizeof text) abort();\n    if (write(fd, text, (size_t)length) != length || close(fd) != 0) abort();
}
static void goose_bench_queue(gs_i64 now, gs_i64 queued, gs_i64 running,
                              gs_i64 waiting, gs_i64 ready, gs_i64 macros) {
    const char *path = getenv(\"GOOSE_BENCH_ACTIVITY\");
    if (!path) return;
    char text[320];
    int length = snprintf(text, sizeof text,
        \"{:kind :native-queue :time-ns %lld :queued %lld :running %lld :waiting %lld :ready-waiting %lld :running-macros %lld}\\n\",
        (long long)now, (long long)queued, (long long)running,
        (long long)waiting, (long long)ready, (long long)macros);
    int fd = open(path, O_WRONLY | O_CREAT | O_APPEND, 0600);
    if (fd < 0 || length < 0 || (size_t)length >= sizeof text) abort();\n    if (write(fd, text, (size_t)length) != length || close(fd) != 0) abort();
}
")
(spit (str (fs/path instrument-dir "measurement.h")) activity-c :append true)
(def instrument-source (fs/path instrument-dir "compiler_session.goose"))
(defn replace-required [s before after]
  (assert (str/includes? s before) (str "Instrumentation anchor missing: " before))
  (str/replace s before after))
(spit (str instrument-source)
  (-> (slurp (str instrument-source))
      (replace-required "extern fn peak_memory()" "extern fn interval(id: i64, chunk: i64, mode: i64, phase: i64, begin: i64, end: i64) = \"goose_bench_interval\";\nextern fn queue_observation(now: i64, queued: i64, running: i64, waiting: i64, ready: i64, macros: i64) = \"goose_bench_queue\";\nextern fn peak_memory()")
      (replace-required "let tracing = instrumented || deterministic || cleanup_test || (arguments.len == 4 && arguments[3] == \"readiness-test\");" "let tracing = true;")
      (replace-required "assert(outcome != tiny::Outcome.Failed);" "if outcome == tiny::Outcome.Failed { print(\"ERROR \", id, \" serial-\", job.fault, \" work=\", heap.work, \" nodes=\", heap.nodes.len, \" values=\", heap.values.len, \" steps=\", job.steps); flush_stdout(); } assert(outcome != tiny::Outcome.Failed);")
      (replace-required "let worker_started = clock_ns();" "let worker_started = clock_ns(); defer { interval(id, checkpoint, mode, 0, worker_started, clock_ns()); }")
      (replace-required "reset_ns += clock_ns() - resetting;" "reset_ns += clock_ns() - resetting; interval(work.id, work.checkpoint, work.mode, 4, resetting, clock_ns());")
      (replace-required "assert(snapshot::restore(saved.heap, code, heap, bindings, identity, sc)); let rt = bindings[0].value;" "assert(snapshot::restore(saved.heap, code, heap, bindings, identity, sc)); interval(id, checkpoint, mode, 2, worker_started, clock_ns()); let rt = bindings[0].value;")
      (replace-required "let state = save(job, rt, call, control," "interval(id, checkpoint, mode, 1, execution_started, clock_ns()); let saving = clock_ns(); let state = save(job, rt, call, control,")
      (replace-required "state.storage = saved.storage;" "state.storage = saved.storage; interval(id, checkpoint, mode, 3, saving, clock_ns());")
      (replace-required "let exporting = clock_ns(); let execution_ns = exporting - execution_started;" "let exporting = clock_ns(); interval(id, checkpoint, mode, 1, execution_started, exporting); let execution_ns = exporting - execution_started;")
      (replace-required "while !closing && received < queue.len && running.len < limit"
        "var pending_waiters: i64 = 0; var ready_waiters: i64 = 0; var running_macros: i64 = 0; for r in running { if r.key == -1 { running_macros += 1; } } for w in waiting { if !w.active { continue; } pending_waiters += 1; for p in produced { if w.key == p.key { ready_waiters += 1; break; } } } queue_observation(clock_ns(), queue.len - received, running.len, pending_waiters, ready_waiters, running_macros);\n        while !closing && received < queue.len && running.len < limit")))
(command env "goose" "format" (str instrument-source))
