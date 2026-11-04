import std;
import os;
import .source_vm;
import .export_schema;
import .program;

namespace snapshot;

// This is a private copy, not immutable artifact publication. All components
// are flat owned arrays, and every graph edge is an integer/session pair.
// Allocator 1 is the current append-only arena: its complete metadata is the
// fourteen end cursors. There are deliberately no fictional free lists.
struct Binding { id: i64, generation: i64, value: source::Value }
struct Header {
    version: i64     = 2, allocator: i64 = 1, base: i64 = 0, checkpoint: i64 = 1,
    ends:    i64[14] = [0; 14], limits: source::Limits = source::Limits {},
    work:    i64     = 0, global: source::Value = source::Value {}, call_method: source::Value = source::Value {},
    boot:    i64     = 0, initializers: i64 = 0, producers: i64 = 0,
}
struct Snapshot {
    header:     Header         = Header {},
    code_bytes: u8[]           = [], functions: source::Function[] = [], code_constants: source::Value[] = [],
    children:   i64[]          = [], ops: source::Op[] = [], code_units: u16[] = [],
    nodes:      source::Node[] = [], values: source::Value[] = [], properties: source::Property[] = [],
    units:      u16[]          = [], constants: source::Value[] = [], bytes: u8[] = [], sources: source::SourceRange[] = [], bindings: Binding[] = [],
}
// A borrowing counterpart lets capture validate before allocating a copy.
struct View {
    header:     Header,
    code_bytes: const u8[:], functions: const source::Function[:], code_constants: const source::Value[:],
    children:   const i64[:], ops: const source::Op[:], code_units: const u16[:],
    nodes:      const source::Node[:], values: const source::Value[:], properties: const source::Property[:],
    units:      const u16[:], constants: const source::Value[:], bytes: const u8[:], sources: const source::SourceRange[:], bindings: const Binding[:],
}
enum Error { None, Boundary, Version, Allocator, Identity, Length, Link, Code, Unsupported, Bytes, Work, Destination }
struct Control {
    bytes:          i64 = 67108864, work: i64 = 10000000, visited: i64 = 0, error: Error = Error.None,
    snapshot_bytes: i64 = 0, copied_bytes: i64 = 0, validation_peak: i64 = 0, capture_ns: i64 = 0, restore_ns: i64 = 0,
}
fn reject(c: Control&, error: Error) -> bool { if c.error == Error.None { c.error = error; } return false; }
fn poll(c: Control&, n: i64 = 1) -> bool {
    if c.error != Error.None { return false; }
    if n < 0 || n > c.work - c.visited { return reject(c, Error.Work); }
    c.visited += n; return true;
}
fn range(start: i64, count: i64, len: i64) -> bool { return start >= 0 && count >= 0 && start <= len && count <= len - start; }
fn ends<T>(s: const T&) -> i64[14] {
    return [s.code_bytes.len, s.functions.len, s.code_constants.len, s.children.len, s.ops.len, s.code_units.len,
            s.nodes.len, s.values.len, s.properties.len, s.units.len, s.constants.len, s.bytes.len, s.sources.len, s.bindings.len];
}
fn payload_bytes<T>(s: const T&) -> i64 {
    return bytes_of(s.code_bytes).len + bytes_of(s.functions).len + bytes_of(s.code_constants).len + bytes_of(s.children).len + bytes_of(s.ops).len + bytes_of(s.code_units).len
         + bytes_of(s.nodes).len + bytes_of(s.values).len + bytes_of(s.properties).len + bytes_of(s.units).len + bytes_of(s.constants).len + bytes_of(s.bytes).len + bytes_of(s.sources).len + bytes_of(s.bindings).len;
}
fn footprint<T>(s: const T&) -> i64 { let empty: Snapshot[] = [Snapshot {}]; return bytes_of(empty).len + payload_bytes(s); }
fn value<T>(s: const T&, v: source::Value, code: bool = false) -> bool {
    if v.tag < 0 || v.tag > (if code { 4 } else { 5 }) || v.tag == 2 && v.a != 0 && v.a != 1 { return false; }
    if v.tag >= 4 {
        if v.session != (if code { 0 } else { s.header.base }) { return false; }
        if v.tag == 4 { return range(v.a, v.b, if code { s.code_units.len } else { s.units.len }); }
        return range(v.a, 1, s.nodes.len);
    }
    return v.session == 0;
}
fn code_valid<T>(s: const T&, c: Control&) -> bool {
    if s.functions.len == 0 || s.functions.len > 65536 || s.code_bytes.len > 8388608 { return reject(c, Error.Code); }
    for v in s.code_constants { if !poll(c) { return false; } if !value(s, v, true) { return reject(c, Error.Code); } }
    for id in s.children { if !poll(c) { return false; } if !range(id, 1, s.functions.len) { return reject(c, Error.Code); } }
    if !utf8(s.code_bytes) { return reject(c, Error.Code); }
    if s.ops.len > c.bytes - footprint(s) { return reject(c, Error.Bytes); }
    c.validation_peak = max(c.validation_peak, s.ops.len);
    var used: bool[>..] = []; used.resize(s.ops.len, false);
    var next: i64       = 0;
    for f in s.functions {
        if !poll(c) { return false; }
        if f.count <= 0 || !range(f.start, f.count, s.ops.len) || !range(f.constants, 0, s.code_constants.len) || !range(f.children, 0, s.children.len) || f.params < 0 || f.params > f.locals || f.locals > 65535 || f.captures < 0 || f.captures > 65535 { return reject(c, Error.Code); }
        if s.ops[f.start].depth != 0 { return reject(c, Error.Code); }
        var offset: i64 = 0;
        for pc in f.start..(f.start + f.count) {
            if !poll(c) { return false; }
            if used[pc] { return reject(c, Error.Code); } used[pc] = true;
            let op = s.ops[pc]; let descriptor = export_descriptor(op.code);
            if descriptor < 0 || op.arg < 0 || op.offset != offset || op.start < 0 || op.end < op.start || !range(op.start, op.end - op.start, s.code_bytes.len) || op.depth < -1 || op.depth > 65536 { return reject(c, Error.Code); }
            if !program::boundary(s.code_bytes, op.start) || !program::boundary(s.code_bytes, op.end) { return reject(c, Error.Code); }
            let kind = (descriptor >> 8) & 255; let width = descriptor >> 16; offset += width + 1;
            if width == 0 && op.arg != 0 || width == 2 && op.arg > 65535 { return reject(c, Error.Code); }
            if kind == 1 && !range(op.arg, 1, s.code_constants.len - f.constants) || kind == 2 && op.arg >= f.locals || kind == 4 && !range(op.arg, 1, s.children.len - f.children) || kind == 5 && op.arg >= f.captures { return reject(c, Error.Code); }
            if kind == 1 && export_name_operand(op.code) && s.code_constants[f.constants + op.arg].tag != 4 { return reject(c, Error.Code); }
            if kind == 3 && !range(op.arg - f.start, 1, f.count) || op.code == 194 && export_form_arity(op.arg) < 0 { return reject(c, Error.Code); }
            // Check stack flow without allocating another decoded Code owner.
            let pop, push = effect(s, f, op);
            if pop < 0 || push < 0 { return reject(c, Error.Code); }
            if op.depth >= 0 {
                if op.depth < pop || op.depth - pop + push > 65536 { return reject(c, Error.Code); }
                if op.code != 15 && op.code != 55 {
                    for edge in (if op.code == 14 || op.code == 56 { 2 } else { 1 }) {
                        let target = if (op.code == 13 || op.code == 14 || op.code == 56) && edge == 0 { op.arg } else { pc + 1 };
                        let extra = if op.code == 56 && edge == 0 { 1 } else { 0 };
                        if !range(target - f.start, 1, f.count) || s.ops[target].depth != op.depth - pop + push + extra { return reject(c, Error.Code); }
                    }
                }
            }
        }
        next += f.count;
    }
    return if next == s.ops.len { true } else { reject(c, Error.Code) };
}
fn utf8(bytes: const u8[:]) -> bool {
    var remaining: i64 = 0; var cp: i64 = 0; var minimum: i64 = 0;
    for byte in bytes {
        if remaining == 0 {
            if byte < 128 { continue; }
            if byte >= 194 && byte <= 223 { remaining = 1; cp = byte & 31; minimum = 128; }
            else if byte >= 224 && byte <= 239 { remaining = 2; cp = byte & 15; minimum = 2048; }
            else if byte >= 240 && byte <= 244 { remaining = 3; cp = byte & 7; minimum = 65536; }
            else { return false; }
        } else {
            if (byte & 192) != 128 { return false; }
            cp = cp * 64 + (byte & 63); remaining -= 1;
            if remaining == 0 && (cp < minimum || cp > 1114111 || cp >= 55296 && cp <= 57343) { return false; }
        }
    }
    return remaining == 0;
}
// Same explicit effect table as source::stack_effect; its only data-dependent
// case (closure captures) is resolved after validating the child link above.
fn effect<T>(s: const T&, f: source::Function, op: source::Op) -> i64, i64 {
    if op.code == 43 { return s.functions[s.children[f.children + op.arg]].captures, 1; }
    // The other cases do not read Code. This empty owner avoids duplicating
    // the opcode contract and does not copy any source/constant table.
    var empty = source::Code {};
    return source::stack_effect(empty, f, op);
}
fn validate<T>(s: const T&, c: Control&) -> bool {
    if !poll(c) { return false; }
    if s.header.version != 2 { return reject(c, Error.Version); }
    if s.header.allocator != 1 { return reject(c, Error.Allocator); }
    if s.header.base <= 0 || s.header.base > 2147483647 || s.header.checkpoint <= 0 || s.header.checkpoint > 2147483647 || s.header.checkpoint == s.header.base { return reject(c, Error.Identity); }
    let lengths = ends(s);
    if lengths != s.header.ends { return reject(c, Error.Length); }
    for n in lengths { if n < 0 || n > 8388608 { return reject(c, Error.Length); } }
    c.snapshot_bytes = footprint(s);
    if c.snapshot_bytes > c.bytes { return reject(c, Error.Bytes); }
    if !poll(c, s.code_bytes.len + s.code_units.len + s.units.len + s.bytes.len) { return false; }
    let l = s.header.limits;
    if l.nodes < 0 || l.values < 0 || l.units < 0 || l.properties < 0 || l.work < 0 || l.steps < 0 || l.frames < 0 || l.stack < 0 || s.header.work < 0 || s.header.work > l.work || s.nodes.len > l.nodes || s.values.len > l.values || s.units.len > l.units || s.constants.len > l.values || s.properties.len > l.properties || s.header.boot < 0 || s.header.boot > 1 || s.header.initializers < 0 || s.header.producers < 0 { return reject(c, Error.Allocator); }
    if !code_valid(s, c) { return false; }
    if !value(s, s.header.global) || s.header.global.tag != 5 || s.nodes[s.header.global.a].kind != 0 { return reject(c, Error.Link); }
    if !value(s, s.header.call_method) || s.header.call_method.tag != 5 || s.nodes[s.header.call_method.a].kind != 4 || s.nodes[s.header.call_method.a].function != 14 { return reject(c, Error.Link); }
    for v in s.values { if !poll(c) { return false; } if !value(s, v) { return reject(c, Error.Link); } }
    if s.constants.len != s.code_constants.len { return reject(c, Error.Length); }
    for v, i in s.constants {
        if !poll(c) { return false; }
        var original = s.code_constants[i]; if original.tag == 4 { original.session = s.header.base; }
        if !value(s, v) { return reject(c, Error.Link); }
        // Live compiler sessions append code after runtime strings exist.
        // String constants keep exact UTF-16 contents, but their runtime range
        // need not have the same offset as the immutable code's string pool.
        if original.tag == 4 {
            if !poll(c, v.b) { return false; }
            if v.tag != 4 || v.b != original.b || s.units[v.a..(v.a + v.b)] != s.code_units[original.a..(original.a + original.b)] { return reject(c, Error.Link); }
        } else if bytes_of([v]) != bytes_of([original]) { return reject(c, Error.Link); }
    }
    for p, i in s.properties {
        if !poll(c) { return false; }
        // put prepends a new record; backward links guarantee termination.
        if p.next < -1 || p.next >= i || !value(s, p.key) || p.key.tag != 4 || !value(s, p.value) { return reject(c, Error.Link); }
    }
    for r in s.sources {
        if !range(r.start, r.count, s.bytes.len) { return reject(c, Error.Link); }
        if !poll(c, r.count + 1) { return false; }
        if !utf8(s.bytes[r.start..(r.start + r.count)]) { return reject(c, Error.Link); }
    }
    for n in s.nodes {
        if !poll(c) { return false; }
        if n.kind < 0 || n.kind > 14 { return reject(c, Error.Unsupported); }
        if n.head < -1 || n.head >= s.properties.len || n.length < 0 || !value(s, n.value) || !value(s, n.namespace_value) || !value(s, n.metadata) || n.source < -1 || n.source >= s.sources.len || n.origin < -1 { return reject(c, Error.Link); }
        if n.source >= 0 && (n.span_start < 0 || n.span_end < n.span_start || !range(n.span_start, n.span_end - n.span_start, s.sources[n.source].count)) { return reject(c, Error.Link); }
        if n.source >= 0 {
            let r = s.sources[n.source]; let text = s.bytes[r.start..(r.start + r.count)];
            if !program::boundary(text, n.span_start) || !program::boundary(text, n.span_end) { return reject(c, Error.Link); }
        }
        if n.kind == 2 && (!range(n.function, 1, s.functions.len) || n.count != s.functions[n.function].captures) { return reject(c, Error.Link); }
        if n.kind == 2 || n.kind >= 5 && n.kind <= 8 || n.kind == 12 || n.kind == 14 { if !range(n.start, n.count, s.values.len) || n.kind == 7 && n.count % 2 != 0 { return reject(c, Error.Link); } }
        if n.kind == 4 && (n.function < 0 || n.function > 15 || n.function == 9) { return reject(c, Error.Unsupported); }
        if (n.kind == 9 || n.kind == 10) && (n.value.tag != 4 || n.namespace_value.tag != 1 && n.namespace_value.tag != 4) || n.kind == 11 && n.value.tag > 4 { return reject(c, Error.Link); }
        if n.metadata.tag != 0 && n.metadata.tag != 1 && (n.metadata.tag != 5 || s.nodes[n.metadata.a].kind != 7) { return reject(c, Error.Link); }
    }
    var previous: i64 = -1;
    for b in s.bindings {
        if !poll(c) { return false; }
        if b.id <= previous || b.generation <= 0 || !value(s, b.value) { return reject(c, Error.Identity); } previous = b.id;
    }
    return true;
}
fn borrow(heap: const source::Heap&, bindings: const Binding[:], checkpoint: i64) -> View {
    let code .= heap.code;
    var v = View { header: Header { base: heap.identity, checkpoint: checkpoint, limits: heap.limits, work: heap.work, global: heap.global, call_method: heap.call_method, boot: heap.boot_count, initializers: heap.initializer_count, producers: heap.producer_count },
        code_bytes: code.source_bytes, functions: code.functions, code_constants: code.constants, children: code.children, ops: code.ops, code_units: code.units,
        nodes: heap.nodes, values: heap.values, properties: heap.properties, units: heap.units, constants: heap.constants, bytes: heap.source_bytes, sources: heap.source_ranges, bindings: bindings };
    v.header.ends = ends(v); return v;
}
fn capture(heap: const source::Heap&, bindings: const Binding[:], c: Control&, checkpoint: i64 = 0) -> Snapshot {
    let started = clock_ns(); defer { c.capture_ns += clock_ns() - started; }
    if heap.live_jobs != 0 || heap.fault != source::Fault.None || !heap.code.valid { reject(c, Error.Boundary); return Snapshot {}; }
    var s = borrow(heap, bindings, if heap.identity == 1 { 2 } else { 1 });
    if !validate(s, c) { return Snapshot {}; }
    // Snapshot identities and private arena identities share the coordinator's
    // non-reusing namespace. The authority itself is never in the image.
    // A live compiler worker receives its checkpoint from the serial admission
    // authority. Its copied global counter must never allocate session IDs.
    s.header.checkpoint = if checkpoint == 0 { source::reserve_identity() } else { checkpoint };
    if s.header.checkpoint <= 0 || s.header.checkpoint > 2147483647 || s.header.checkpoint == heap.identity { reject(c, Error.Identity); return Snapshot {}; }
    c.copied_bytes += footprint(s);
    return Snapshot { header: s.header, code_bytes: s.code_bytes, functions: s.functions, code_constants: s.code_constants, children: s.children, ops: s.ops, code_units: s.code_units,
        nodes: s.nodes, values: s.values, properties: s.properties, units: s.units, constants: s.constants, bytes: s.bytes, sources: s.sources, bindings: s.bindings };
}
fn relocated(v: source::Value, identity: i64) -> source::Value { var out = v; if out.tag >= 4 { out.session = identity; } return out; }
// Reset only after the dispatch's job has retired. This reestablishes every
// restore destination invariant; restore still validates the entire image and
// requires a fresh identity. No references/roots may cross this boundary.
fn reset(code: source::Code&, heap: source::Heap&, bindings: Binding[>..]&) -> bool {
    if !(heap.code .== code) || heap.live_jobs != 0 { return false; }
    bindings.clear();
    heap.nodes.clear(); heap.values.clear(); heap.properties.clear(); heap.units.clear(); heap.constants.clear(); heap.source_bytes.clear(); heap.source_ranges.clear();
    code.source_bytes.clear(); code.functions.clear(); code.constants.clear(); code.children.clear(); code.ops.clear(); code.units.clear();
    code.valid = false; code.error = tiny::LoadError.None; code.error_pc = -1;
    heap.identity = 0; heap.limits = source::Limits {}; heap.work = 0; heap.fault = source::Fault.None; heap.global = source::Value {}; heap.call_method = source::Value {};
    heap.boot_count = 0; heap.initializer_count = 0; heap.producer_count = 0;
    return true;
}
fn restore(s: const Snapshot&, code: source::Code&, heap: source::Heap&, bindings: Binding[>..]&, identity: i64, c: Control&) -> bool {
    let started = clock_ns(); defer { c.restore_ns += clock_ns() - started; }
    if !(heap.code .== code) || heap.identity != 0 || heap.live_jobs != 0 || heap.nodes.len != 0 || heap.values.len != 0 || heap.properties.len != 0 || heap.units.len != 0 || heap.constants.len != 0 || heap.source_bytes.len != 0 || heap.source_ranges.len != 0 || code.valid || code.source_bytes.len != 0 || code.functions.len != 0 || code.constants.len != 0 || code.children.len != 0 || code.ops.len != 0 || code.units.len != 0 || bindings.len != 0 { return reject(c, Error.Destination); }
    if identity <= 0 || identity > 2147483647 || identity == s.header.base || identity == s.header.checkpoint { return reject(c, Error.Identity); }
    if !validate(s, c) { return false; }
    // No initialization, producer, publication cache, claim or waiter code.
    code.source_bytes.append(s.code_bytes); code.functions.append(s.functions); code.constants.append(s.code_constants); code.children.append(s.children); code.ops.append(s.ops); code.units.append(s.code_units); code.valid = true;
    heap.identity = identity; heap.limits = s.header.limits; heap.work = s.header.work; heap.global = relocated(s.header.global, identity); heap.call_method = relocated(s.header.call_method, identity);
    heap.boot_count = s.header.boot; heap.initializer_count = s.header.initializers; heap.producer_count = s.header.producers;
    for n in s.nodes { var out = n; out.value = relocated(n.value, identity); out.namespace_value = relocated(n.namespace_value, identity); out.metadata = relocated(n.metadata, identity); heap.nodes.push(out); }
    for v in s.values { heap.values.push(relocated(v, identity)); }
    for p in s.properties { heap.properties.push(source::Property { key: relocated(p.key, identity), value: relocated(p.value, identity), next: p.next }); }
    heap.units.append(s.units); for v in s.constants { heap.constants.push(relocated(v, identity)); }
    heap.source_bytes.append(s.bytes); heap.source_ranges.append(s.sources);
    for b in s.bindings { bindings.push(Binding { id: b.id, generation: b.generation, value: relocated(b.value, identity) }); }
    c.copied_bytes += payload_bytes(s); return true;
}
// Optional local codec: Goose's checked flat image format plus our semantic
// version/allocator checks. This is compiler-ABI-specific, not a wire standard.
fn encode(s: const Snapshot&, out: u8[>..]&, c: Control&) -> bool {
    if !validate(s, c) { return false; }
    let before = out.len; let records: Snapshot[] = [copy(s)]; to_bytes(records, out);
    // [s] is an intermediate full copy, followed by the serialization copy.
    c.copied_bytes += footprint(s) + out.len - before; return true;
}
fn decode(bytes: const u8[:], c: Control&) -> Snapshot[>..] {
    if bytes.len > c.bytes { reject(c, Error.Bytes); return []; }
    if !poll(c, bytes.len) { return []; }
    var records, ok = from_bytes<Snapshot[>..]>(bytes);
    if !ok || records.len != 1 { reject(c, Error.Length); return []; }
    c.copied_bytes += bytes_of(records).len;
    for s in records { if !validate(s, c) { return []; } }
    c.copied_bytes += bytes_of(records).len; return copy(records);
}
// Admission accounting for a finite batch, not a scheduling system. Reserve
// before spawn, release only after thread_wait has released private storage.
struct Resources { limit: i64 = 268435456, workers: i64 = 2, active: i64 = 0, retained: i64 = 0, peak: i64 = 0 }
fn reserve(r: Resources&, bytes: i64) -> bool {
    if bytes < 0 || r.active >= r.workers || bytes > r.limit - r.retained { return false; }
    r.active += 1; r.retained += bytes; r.peak = max(r.peak, r.retained); return true;
}
fn release(r: Resources&, bytes: i64) { assert(r.active > 0 && bytes >= 0 && bytes <= r.retained); r.active -= 1; r.retained -= bytes; }
