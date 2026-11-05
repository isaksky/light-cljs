import std;
import .source_vm;

namespace heap_program;

// One VM heap per thread program (light-cljs-794; docs/javascript-coverage.md,
// decision 2): a test `main` builds no `source::Heap` and runs each function
// that builds one as its own thread program, one after another. The spawn
// copies main's session-identity counter into the worker; `finish` hands the
// worker's counter back and `join` adopts it, so identities stay unique
// across the sequence exactly as they were when the functions shared a program.
struct Identity { next: i64 }
fn finish() { assert(qput(Identity { next: source::next_identity })); }
fn join(thread: i64) { assert(thread_wait(thread)); let i = qget<Identity>(); source::next_identity = i.next; }
