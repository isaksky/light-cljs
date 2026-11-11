import std;
import math;
import .source_vm;
import .realm;
import .array_callbacks;
import .array_methods;
import .array_mutators;
import .function_calls;
import .coerce;

namespace source;

// Array.prototype.sort and toSorted (light-cljs-910; decision 5 of
// docs/javascript-coverage.md): a stable, resumable merge sort, as the
// Pants engine's rust/src/vm/native/array_sort.rs and QuickJS's
// js_array_sort are, with the specification's steps (ECMAScript 23.1.3.30,
// 23.1.3.34 and SortIndexedProperties, 23.1.3.30.1): the comparator check
// first (a TypeError before anything is read), ToObject(this),
// LengthOfArrayLike, the present elements collected (sort: HasProperty then
// Get; toSorted: Get of every index, a hole reading undefined), the values
// other than undefined sorted with the comparator (its result's ToNumber,
// NaN as +0) or by the code units of their ToString, and the result written:
// sort Sets the sorted values, then the undefineds, and deletes the indices
// the holes leave, so the hole count is kept; toSorted fills a new plain
// array of the same length.
//
// Both run in array_mutators.goose's frame (Native.ArrayMethod) as units
// over its memoized primitives, so every HasProperty, Get, comparator call,
// conversion, Set and delete is a step of its own, and a comparison is one
// unit (a step ends after each, whether or not it ran JavaScript). Beyond
// that frame's scratch operands:
//   callee    the run being merged from: a plain array of the collected
//             values that are not undefined, a job row no JavaScript sees
//   receiver  the run being merged into, another one
//   output    toSorted's result
//   Q         how many elements were present (the collected values and the
//             undefineds); P, R, S and Held are each stage's cursors:
//   Collect   P the next index
//   Sort      a bottom-up merge: R the run width, S the pair's start, P the
//             left run's cursor, Held the right run's; the output position
//             follows from them. A pass over, the two arrays swap and the
//             width doubles.
//   Write     P the next index to set or delete
// Equal elements keep their order: the right element is taken only when
// the comparator's result is greater than zero (the default order: when
// its string is less), and the comparator is called with (left, right).
// A dense receiver with every element present is collected, and after an
// unchanged one written back, in one step each (array_mutators.goose's
// mutator_dense).
let mutate_stage::Sort = 11;
let mutate_stage::Write = 12;

// How many elements of toSorted's result one step writes.
inline fn sort_batch() -> i64 { return 1024; }
inline fn array_sorter(b: Builtin) -> bool { return b == Builtin.ArrayPrototypeSort || b == Builtin.ArrayPrototypeToSorted; }
// Element i of scratch array a.
inline fn sort_value(heap: Heap&, a: Value, i: i64) -> Value { let v, _ = own_element(heap, a, i); return v; }

// One unit of sort or toSorted for the frame m walks (see above).
fn sort_unit(job: Job&, m: Micro&, b: Builtin) {
    let heap .= job.heap; let copying = b == Builtin.ArrayPrototypeToSorted;
    let o = job.frames[m.at].input; let len = job.frames[m.at].argument_count;
    let compare = if job.frames[m.at].index > 0 { job.operands[m.base + mutate_slot::Args] } else { Value.Undefined };
    let stage = mutate_number(job, m.base, mutate_slot::Stage);
    if stage == mutate_stage::Setup {
        let work = allocate(heap, Node { kind: node_kind::Array });
        let aux = allocate(heap, Node { kind: node_kind::Array });
        if heap.fault != Fault.None { m.stop = true; return; }
        job.frames[m.at].callee = work; job.frames[m.at].receiver = aux;
        if copying {
            let length: f64 = len;
            let a = array_species_create(job, b, o, length);
            if heap.fault != Fault.None { m.stop = true; return; }
            job.frames[m.at].output = a;
        }
        mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Collect);
        let fast = mutator_dense(job, o, len);
        if heap.fault != Fault.None { m.stop = true; return; }
        if fast {
            var n: i64 = 0;
            for i in len {
                let v = sort_value(heap, o, i);
                if !v.is_undefined() { if !set_element(heap, work, n, v) { m.stop = true; return; } n += 1; }
            }
            mutate_put(job, m.base, mutate_slot::P, len); mutate_put(job, m.base, mutate_slot::Q, len);
        }
        return;
    }
    let work = job.frames[m.at].callee; let aux = job.frames[m.at].receiver;
    if !work.is_object() || !aux.is_object() || node(heap, work.id()).kind != node_kind::Array || node(heap, aux.id()).kind != node_kind::Array { heap.fault = Fault.InvalidState; m.stop = true; return; }
    let n: i64 = node(heap, work.id()).length;
    if stage == mutate_stage::Collect {
        let k = mutate_number(job, m.base, mutate_slot::P);
        if k >= len {
            mutate_put(job, m.base, mutate_slot::P, 0);
            if n < 2 { mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Write); return; }
            mutate_put(job, m.base, mutate_slot::R, 1); mutate_put(job, m.base, mutate_slot::S, 0); job.operands[m.base + mutate_slot::Held] = number(1);
            mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Sort);
            return;
        }
        var present = true;
        if !copying { present = micro_has(job, m, o, k); if m.stop { return; } }
        if present {
            let v = micro_get(job, m, b, o, k);
            if m.stop { return; }
            if !v.is_undefined() && !set_element(heap, work, n, v) { m.stop = true; return; }
            mutate_put(job, m.base, mutate_slot::Q, mutate_number(job, m.base, mutate_slot::Q) + 1);
        }
        mutate_put(job, m.base, mutate_slot::P, k + 1);
        micro_unit_done(job, m);
        return;
    }
    if stage == mutate_stage::Sort {
        let width = mutate_number(job, m.base, mutate_slot::R); let lo = mutate_number(job, m.base, mutate_slot::S);
        let l = mutate_number(job, m.base, mutate_slot::P); let r = job.operands[m.base + mutate_slot::Held].num() as i64;
        if width < 1 || lo < 0 || l < lo || r < l { heap.fault = Fault.InvalidState; m.stop = true; return; }
        // The pass is over: the merged runs become the source, twice as wide.
        if lo >= n {
            job.frames[m.at].callee = aux; job.frames[m.at].receiver = work;
            mutate_put(job, m.base, mutate_slot::P, 0);
            if width * 2 >= n { mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Write); return; }
            mutate_put(job, m.base, mutate_slot::R, width * 2); mutate_put(job, m.base, mutate_slot::S, 0); job.operands[m.base + mutate_slot::Held] = number(min(width * 2, n));
            return;
        }
        let mid = min(lo + width, n); let hi = min(lo + 2 * width, n);
        if l > mid || r < mid || r > hi { heap.fault = Fault.InvalidState; m.stop = true; return; }
        let out = l + r - mid;
        // A run is spent: the other's rest is copied, an element a step;
        // then the next pair.
        if l >= mid || r >= hi {
            if !charge(heap) { m.stop = true; return; }
            if l < mid { if !set_element(heap, aux, out, sort_value(heap, work, l)) { m.stop = true; return; } mutate_put(job, m.base, mutate_slot::P, l + 1); }
            else if r < hi { if !set_element(heap, aux, out, sort_value(heap, work, r)) { m.stop = true; return; } job.operands[m.base + mutate_slot::Held] = number(r + 1); }
            else { mutate_put(job, m.base, mutate_slot::S, hi); mutate_put(job, m.base, mutate_slot::P, hi); job.operands[m.base + mutate_slot::Held] = number(min(hi + width, n)); return; }
            // A copied element is a step, so a collection the growing run
            // asked for runs before the next.
            m.stop = true;
            return;
        }
        let x = sort_value(heap, work, l); let y = sort_value(heap, work, r);
        var right = false;
        if !compare.is_undefined() {
            let pair: Value[2] = [x, y];
            let v = micro_call(job, m, compare, Value.Undefined, pair[..]);
            if m.stop { return; }
            let c = micro_to_number(job, m, b, v);
            if m.stop { return; }
            right = c > 0;
        } else {
            let xs = micro_string(job, m, b, x);
            if m.stop { return; }
            let ys = micro_string(job, m, b, y);
            if m.stop { return; }
            let less = binary_value(heap, opcode::CompareLt, ys, xs);
            if heap.fault != Fault.None { m.stop = true; return; }
            right = less.flag();
        }
        if !set_element(heap, aux, out, if right { y } else { x }) { m.stop = true; return; }
        if right { job.operands[m.base + mutate_slot::Held] = number(r + 1); } else { mutate_put(job, m.base, mutate_slot::P, l + 1); }
        micro_unit_done(job, m);
        // A comparison is a step, whether or not it ran JavaScript.
        m.stop = true;
        return;
    }
    // Write.
    let j = mutate_number(job, m.base, mutate_slot::P); let present = mutate_number(job, m.base, mutate_slot::Q);
    if n > present || present > len { heap.fault = Fault.InvalidState; m.stop = true; return; }
    if copying {
        // The result is this frame's own array: no JavaScript sees the stores.
        // It is filled sort_batch() elements a step, from cursor P, so a
        // long result's growing run is collected between batches.
        let a = job.frames[m.at].output;
        if j >= len { micro_finish(job, m, a); return; }
        let end = min(j + sort_batch(), len);
        if !charge(heap, end - j) { m.stop = true; return; }
        for i in j..end { if !set_element(heap, a, i, if i < n { sort_value(heap, work, i) } else { Value.Undefined }) { m.stop = true; return; } }
        mutate_put(job, m.base, mutate_slot::P, end);
        m.stop = true;
        return;
    }
    if j == 0 && present == len && mutate_number(job, m.base, mutate_slot::Done) == 0 {
        let fast = mutator_dense(job, o, len);
        if heap.fault != Fault.None { m.stop = true; return; }
        if fast {
            for i in len { if !set_element(heap, o, i, if i < n { sort_value(heap, work, i) } else { Value.Undefined }) { m.stop = true; return; } }
            micro_finish(job, m, o);
            return;
        }
    }
    if j >= len { micro_finish(job, m, o); return; }
    if j < n { micro_set_index(job, m, b, o, j, sort_value(heap, work, j)); }
    else if j < present { micro_set_index(job, m, b, o, j, Value.Undefined); }
    else { micro_delete(job, m, b, o, j); }
    if m.stop { return; }
    mutate_put(job, m.base, mutate_slot::P, j + 1);
    micro_unit_done(job, m);
}
