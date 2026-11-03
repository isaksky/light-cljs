import std;
import .export_schema;

namespace program;

enum Error { None, Header, Version, Limits, Truncated, Source, Range, Reference, Operation, Branch, Trailing }
// An installed immutable image owns exact bytes. Inspection borrows ranges;
// there are no pointers, runtime values, or line/column indexes in the image.
root struct Image { bytes: u8[>..] = [], valid: bool = false }
struct Layout { source: i64, functions: i64, constants: i64, children: i64, instructions: i64, units: i64, end: i64 }
fn word(bytes: const u8[:], offset: i64) -> i64 {
    var n: i64 = 0;
    for i in 4 { n |= (bytes[offset + i] as i64) << (8 * i); }
    return n;
}
fn layout(bytes: const u8[:]) -> Layout {
    let source = 32;
    let functions = source + word(bytes, 8);
    let constants = functions + word(bytes, 12) * 52;
    let children = constants + word(bytes, 16) * 12;
    let instructions = children + word(bytes, 20) * 4;
    let units = instructions + word(bytes, 24) * 20;
    return Layout { source: source, functions: functions, constants: constants, children: children, instructions: instructions, units: units, end: units + word(bytes, 28) * 2 };
}
fn range(start: i64, length: i64, count: i64) -> bool { return start <= count && length <= count - start; }
fn boundary(source: const u8[:], offset: i64) -> bool {
    return offset <= source.len && (offset == source.len || (source[offset] & 192) != 128);
}
fn branch(bytes: const u8[:], base: i64, count: i64, target: i64) -> bool {
    var lo: i64 = 0; var hi = count;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let offset = word(bytes, base + mid * 20 + 16);
        if offset < target { lo = mid + 1; } else { hi = mid; }
    }
    return lo < count && word(bytes, base + lo * 20 + 16) == target;
}
fn validate(bytes: const u8[:]) -> Error {
    if bytes.len < 32 { return Error.Truncated; }
    if bytes[..4] != "GSP\0" { return Error.Header; }
    if word(bytes, 4) != 2 { return Error.Version; }
    if word(bytes, 8) > 8388608 || word(bytes, 12) == 0 || word(bytes, 12) > 65536 { return Error.Limits; }
    for offset in [16, 20, 24, 28] { if word(bytes, offset) > 1048576 { return Error.Limits; } }
    let l = layout(bytes);
    if l.end > bytes.len { return Error.Truncated; }
    if l.end < bytes.len { return Error.Trailing; }
    let source = bytes[l.source..l.functions];
    // Validate all UTF-8, including unreferenced source, without confusing
    // runtime UTF-16 payloads (which may contain lone surrogates) with source.
    var normalized: u8[>..] = [];
    each_utf8(source) { cp => normalized.push_utf8(cp); };
    if normalized[..] != source { return Error.Source; }
    let nf = word(bytes, 12); let nc = word(bytes, 16); let nch = word(bytes, 20); let ni = word(bytes, 24); let nu = word(bytes, 28);
    for i in nc {
        let at = l.constants + i * 12; let tag = word(bytes, at); let a = word(bytes, at + 4); let b = word(bytes, at + 8);
        if tag > 4 { return Error.Operation; }
        if tag <= 1 && (a != 0 || b != 0) { return Error.Operation; }
        if tag == 2 && (a > 1 || b != 0) { return Error.Operation; }
        if tag == 4 && !range(a, b, nu) { return Error.Reference; }
    }
    var used:       bool[>..] = []; used.resize(ni, false);
    var child_used: bool[>..] = []; child_used.resize(nf, false); child_used[0] = true;
    var depths:     i64[>..]  = []; depths.resize(nf, 0);
    for id in nf {
        let f = l.functions + id * 52;
        let start = word(bytes, f); let count = word(bytes, f + 4);
        let constants = word(bytes, f + 8); let constant_count = word(bytes, f + 12);
        let children = word(bytes, f + 16); let child_count = word(bytes, f + 20);
        let params = word(bytes, f + 24); let locals = word(bytes, f + 28); let captures = word(bytes, f + 32);
        if count == 0 || !range(start, count, ni) || !range(constants, constant_count, nc) || !range(children, child_count, nch) || !range(word(bytes, f + 36), word(bytes, f + 40), nu) { return Error.Range; }
        if params > locals || locals > 65535 || captures > 65535 || constant_count > 65536 || child_count > 65536 || word(bytes, f + 44) > 65535 { return Error.Limits; }
        let flags = word(bytes, f + 48);
        if (flags & 255) > 1 || (flags >> 8) > 1 { return Error.Operation; }
        for child in child_count {
            let target = word(bytes, l.children + (children + child) * 4);
            if target <= id || target >= nf || child_used[target] { return Error.Reference; }
            child_used[target] = true; depths[target] = depths[id] + 1;
            if depths[target] > 256 { return Error.Limits; }
        }
        let base = l.instructions + start * 20;
        var expected_offset: i64 = 0;
        for i in count {
            if used[start + i] { return Error.Range; } used[start + i] = true;
            let at = base + i * 20; let descriptor = word(bytes, at); let op = descriptor & 255;
            if export_descriptor(op) != descriptor { return Error.Operation; }
            if op == 194 && export_form_arity(word(bytes, at + 4)) < 0 { return Error.Operation; }
            let arg = word(bytes, at + 4); let kind = (descriptor >> 8) & 255; let size = descriptor >> 16;
            if word(bytes, at + 16) != expected_offset { return Error.Range; }
            expected_offset += size + 1;
            if expected_offset > 4294967295 { return Error.Limits; }
            let span_start = word(bytes, at + 8); let span_end = word(bytes, at + 12);
            if span_start > span_end || !boundary(source, span_start) || !boundary(source, span_end) { return Error.Source; }
            if size == 0 && arg != 0 || size == 2 && arg > 65535 { return Error.Limits; }
            if kind == 1 && arg >= constant_count || kind == 2 && arg >= locals || kind == 4 && arg >= child_count || kind == 5 && arg >= captures { return Error.Reference; }
            if kind == 1 && export_name_operand(op) && word(bytes, l.constants + (constants + arg) * 12) != 4 { return Error.Reference; }
            if kind == 3 && !branch(bytes, base, count, arg) { return Error.Branch; }
        }
    }
    for flag in used { if !flag { return Error.Range; } }
    for flag in child_used { if !flag { return Error.Reference; } }
    return Error.None;
}
// Validation completes before touching an existing image. Failed installation
// preserves every byte of the prior valid program.
fn install(image: Image&, bytes: const u8[:]) -> Error {
    let result = validate(bytes);
    if result != Error.None { return result; }
    image.bytes.clear(); image.bytes.append(bytes); image.valid = true;
    return Error.None;
}
