import std;
import .source_vm;

namespace source;

// Tagged templates (light-cljs-862): GetTemplateObject (ECMAScript
// 13.2.8.4), with the Pants engine's semantics (rust/src/vm/dispatch.rs,
// template_object) and QuickJS's attributes.
//
// TemplateObject n (111): cooked1 raw1 .. cookedn rawn → template. The
// Rust compiler (rust/src/frontend/lower.rs, tagged_template) pushes each
// quasi's cooked string (undefined for an invalid escape) and raw string
// before the substitutions, so the instruction runs before any of them. The
// result is the site's template object: a frozen array of the cooked
// strings whose own `raw` (neither writable, enumerable nor configurable)
// is a frozen array of the raw strings, both inheriting from
// %Array.prototype%. The first evaluation of a site makes it; every later
// one returns the same object.
//
// The per-site cache (decision 3 of docs/javascript-coverage.md) is
// job-local: Job.templates, one TemplateSite a site the job evaluated,
// naming the site by its TemplateObject row of Code.ops and the object by
// its node row. Code rows are never reused within a job, so a row is a
// site: eval code and Function bodies compiled at run time
// (runtime_compile.goose) append fresh rows, so each compilation's sites are
// new ones, as the Pants engine keys its registry by the compiled code and
// QuickJS by the parsed function; a function loaded from a cell or a
// callable publication installs its code at new rows too. The objects are
// rows the job allocated, so:
//   - a budget cut keeps the list with the job (it is job state, like the
//     write log); a saved continuation (compiler_session.goose, Saved)
//     carries it with the heap rows it names, which a resume restores at
//     the same rows;
//   - in-job reclamation (reclaim.goose) marks every cached object as a
//     root and remaps its row, so a site keeps its object across a
//     collection even when nothing else holds it;
//   - the worker's reset (reset) clears the list with the rows it names;
//     a job therefore never sees another job's template objects, the same
//     as a fresh worker, and an object that left a job in a publication is
//     not the one the same site makes in a later job (the tables keep
//     nothing across jobs but the image).
// The cache is a short list searched from its newest entry: a job
// evaluates few distinct sites, and a loop's repeated site is usually the
// newest.
struct TemplateSite { op: u32, object: u32 }

// Frozen arrays. No Object.freeze yet (realm.goose lists it pending), so
// the template object and its `raw` are the only frozen objects: an array
// node with `immutable` set (it marks nothing else on an array). Its
// elements are enumerable data properties that are neither writable nor
// configurable, its `length` is neither writable nor configurable, its named
// properties keep the attributes they were made with, and it is not
// extensible (sealed, function_calls.goose). Assignments to it are refused
// (a TypeError in strict code), deletes of its own properties are refused,
// definitions succeed only when they change nothing, and a built-in that
// writes into it (CreateDataPropertyOrThrow, a length update) throws
// TypeError. dense() keeps it off the element fast paths.
checkfree fn frozen(heap: const Heap&, o: i64) -> bool { let n .= node(heap, o); return n.kind == node_kind::Array && n.immutable; }

// Run TemplateObject with `count` quasis for the instruction at row `at`
// of the running frame.
fn template_object(job: Job&, at: i64, count: i64) {
    let heap .= job.heap;
    let base = job.operands.len - 2 * count;
    if count <= 0 || base < job.frames[job.frames.len - 1].operand_base { heap.fault = Fault.InvalidState; return; }
    var i = job.templates.len;
    while i > 0 {
        i -= 1;
        if job.templates[i].op == at {
            job.operands.resize(base); job.operands.push(object(job.templates[i].object)); return;
        }
    }
    // The parts are the compiler's string constants: a cooked string or
    // undefined, then a raw string. Hand-made bytecode with anything else
    // fails before any row is added.
    var cooked: Value[>..] = []; var raw: Value[>..] = [];
    for k in count {
        let c = job.operands[base + 2 * k]; let r = job.operands[base + 2 * k + 1];
        if !(c.is_string() || c.is_undefined()) || !r.is_string() || !valid(heap, c) || !valid(heap, r) { heap.fault = Fault.InvalidState; return; }
        cooked.push(c); raw.push(r);
    }
    if !nodes_fit(heap, 2) || !values_fit(heap, 2 * count) { memory(heap); return; }
    if !charge(heap, 2 * count) { return; }
    let raw_start = add_values(heap, raw);
    if raw_start < 0 { return; }
    let raw_array = allocate(heap, Node { kind: node_kind::Array, start: raw_start as u32, count: count as u32, length: count as u32, immutable: true });
    if !raw_array.is_object() { return; }
    let cooked_start = add_values(heap, cooked);
    if cooked_start < 0 { return; }
    let template = allocate(heap, Node { kind: node_kind::Array, start: cooked_start as u32, count: count as u32, length: count as u32 });
    if !template.is_object() { return; }
    let k = key_named(heap, "raw");
    if heap.fault != Fault.None || !add_own(heap, template.id(), k, raw_array, 0) { if heap.fault == Fault.None { memory(heap); } return; }
    node_mut(heap, template.id()).immutable = true;
    job.templates.push(TemplateSite { op: at as u32, object: template.id() });
    job.operands.resize(base); job.operands.push(template);
}
// A definition of own property k of frozen array o (define_property): it
// succeeds only when it changes nothing (ValidateAndApplyPropertyDescriptor
// over a non-configurable property; a new key is refused, since o is not
// extensible).
fn frozen_define(heap: Heap&, o: i64, k: Value, value: Value, attrs: u8, half: i64) -> bool {
    let p = own_entry(heap, object(o), k);
    if heap.fault != Fault.None || !p.found { return false; }
    return (half == half::Data || half == half::Complete && !is_accessor(value)) && !is_accessor(p.value) && attrs == p.attrs && same_value(heap, p.value, value);
}
