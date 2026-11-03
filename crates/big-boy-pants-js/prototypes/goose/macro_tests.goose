import std;
import .program;
import .source_vm;
import .vm;

fn decode(code: source::Code&, bytes: const u8[:]) {
    var image = program::Image {}; assert(program::install(image, bytes) == program::Error.None); assert(source::load(code, image));
}
fn invoke(heap: source::Heap&, callee: source::Value, args: const source::Value[:]) -> source::Value {
    var job = source::Job { heap: heap }; assert(source::start(job, callee, args));
    let outcome = source::run(job, -1);
    if outcome != tiny::Outcome.Complete { print("failed ", job.fault, " f=", job.fault_function, " pc=", job.fault_pc); }
    assert(outcome == tiny::Outcome.Complete); return job.result;
}
fn macro_factory(heap: source::Heap&, rt: source::Value) -> source::Value {
    let factory = invoke(heap, source::closure(heap, 0, []), []);
    let result = invoke(heap, factory, [rt]); heap.initializer_count += 1; return result;
}
fn churn() {
    var code = source::Code {}; decode(code, embed_file("corpus/churn-churn.pants.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); let rt = source::boot_runtime(heap);
    let macro = macro_factory(heap, rt);
    for n in [0, 16, 100] { let result = invoke(heap, macro, [source::nil(), source::nil(), source::number(n)]); assert(result.tag == 3 && result.number == (if n == 0 { 0 } else if n == 16 { 168 } else { 5250 })); }
    assert(heap.boot_count == 1 && heap.initializer_count == 1); print("real emitted churn: 0/16/100 passed, boot=1 initializer=1");
}
fn closure() {
    var code = source::Code {}; decode(code, embed_file("corpus/closure-captured.pants.gsp"));
    var heap = source::Heap { code: code }; assert(source::initialize(heap)); let rt = source::boot_runtime(heap); let macro = macro_factory(heap, rt);
    let input = source::form_collection(heap, 6, [source::number(1), source::number(2), source::number(3)]);
    for base in [7, 10] {
        let result = invoke(heap, macro, [source::nil(), source::nil(), source::number(base), input]);
        let out = source::form_nth(heap, result, 1); assert(source::form_count(heap, out) == 3);
        for i in 3 { let pair = source::form_nth(heap, out, i); assert(source::form_nth(heap, pair, 0).number == i + 1 && source::form_nth(heap, pair, 1).number == base + 4 + i); }
    }
    print("real emitted captured closure/mapv passed");
}
fn main() { churn(); closure(); }
