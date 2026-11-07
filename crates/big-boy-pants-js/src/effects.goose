import std;
import os;
import .source_vm;

namespace effects;

// Compiler-owned declarations, in source order. IDs are logical occurrences,
// not worker/session heap IDs. State and receipts never enter a VM snapshot.
// Native leaves: gensym=5, counter increment=6, counter read=7; 8 is external
// and deliberately unsupported. Resource IDs address sixteen scalar counters.
struct Grant { id: i64, owner: i64, sequence: i64, kind: i64, resource: i64 = 0 }
struct Receipt { grant: Grant, value: i64 = 0, fault: source::Fault = source::Fault.None, prefix: u16[128] = [0; 128], count: i64 = 0 }
root struct Authority {
    grants: Grant[>..] = [], receipts: Receipt[>..] = [], counters: i64[16] = [0; 16],
    next:   i64        = 0, fail_id: i64 = -1, service_ns: i64 = 0,
}
fn same(a: Grant, b: Grant) -> bool { return a.id == b.id && a.owner == b.owner && a.sequence == b.sequence && a.kind == b.kind && a.resource == b.resource; }
fn valid(grants: const Grant[:]) -> bool {
    if grants.len > 1024 { return false; }
    for g, i in grants {
        if g.id < 0 || g.owner < 0 || g.sequence < 0 || g.kind < 5 || g.kind > 7 || g.resource < 0 || g.resource >= 16 { return false; }
        var sequence: i64 = 0;
        for j in i { if grants[j].id == g.id { return false; } if grants[j].owner == g.owner { sequence += 1; } }
        if g.sequence != sequence { return false; }
    }
    return true;
}
// An exact duplicate gets its original receipt, including failure. Conflicting
// IDs or out-of-order deliveries cannot mutate state or consume a declaration.
fn commit(a: Authority&, g: Grant, prefix: const u16[:] = []) -> Receipt {
    for r in a.receipts { if r.grant.id == g.id {
        if same(r.grant, g) && prefix.len == r.count && prefix == r.prefix[..r.count] { return r; }
        return Receipt { grant: g, fault: source::Fault.EffectContract };
    } }
    if a.next >= a.grants.len || !same(a.grants[a.next], g) || prefix.len > 128 || g.resource < 0 || g.resource >= 16 || g.kind < 5 || g.kind > 7 || g.kind != 5 && prefix.len != 0 { return Receipt { grant: g, fault: source::Fault.EffectContract }; }
    var r = Receipt { grant: g, count: prefix.len }; for unit, i in prefix { r.prefix[i] = unit; }
    if g.id == a.fail_id { r.fault = source::Fault.HostEffect; }
    else {
        if (g.kind == 5 || g.kind == 6) && a.counters[g.resource] == 9007199254740991 { r.fault = source::Fault.HostEffect; }
        else { if g.kind == 5 || g.kind == 6 { a.counters[g.resource] += 1; } r.value = a.counters[g.resource]; }
    }
    a.receipts.push(r); a.next += 1; return r;
}
// Delivery is called only after run_claimed's defer has released ownership.
// The adapter may service correlated helper work during this suspension.
fn service(a: Authority&, job: source::Job&, owner: i64, sequence: i64&) -> bool {
    if job.claimed || !job.effect_pending || job.status != tiny::Status.Ready { return false; }
    let start = clock_ns(); defer { a.service_ns += clock_ns() - start; }
    var index: i64 = -1;
    for g, i in a.grants { if g.owner == owner && g.sequence == sequence { index = i; } }
    if index < 0 || a.grants[index].kind != job.effect_kind { return source::deliver_effect(job, source::Value.Undefined, source::Fault.EffectContract); }
    let g = a.grants[index]; var prefix: u16[>..] = [];
    if g.kind == 5 {
        if job.effect_count > 1 || job.effect_count == 1 && !job.effect_args[0].is_string() { return source::deliver_effect(job, source::Value.Undefined, source::Fault.EffectContract); }
        if job.effect_count == 0 { prefix.append([71, 95, 95]); }
        else { let p = job.effect_args[0]; if !source::valid(job.heap, p) || source::str_len(job.heap, p) > 128 { return source::deliver_effect(job, source::Value.Undefined, source::Fault.EffectContract); } source::append_utf16(job.heap, p, prefix); }
    } else if job.effect_count != 0 { return source::deliver_effect(job, source::Value.Undefined, source::Fault.EffectContract); }
    // This slice supports unqualified generated names only; namespace and
    // compiler naming-scope/collision services require the compiler adapter.
    for unit in prefix { if unit == 47 { return source::deliver_effect(job, source::Value.Undefined, source::Fault.EffectContract); } }
    let r = commit(a, g, prefix);
    if r.fault != source::Fault.None { return source::deliver_effect(job, source::Value.Undefined, r.fault); }
    var value = source::number(r.value);
    if g.kind == 5 {
        let suffix = str(r.value); var units: u16[>..] = []; units.append(prefix); for byte in suffix { units.push(byte); }
        value = source::allocate(job.heap, source::Node { kind: 9, value: source::string(job.heap, units), namespace_value: source::nil() });
        if job.heap.fault != source::Fault.None { return source::deliver_effect(job, source::Value.Undefined, job.heap.fault); }
    }
    sequence += 1; return source::deliver_effect(job, value);
}
