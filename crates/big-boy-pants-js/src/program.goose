import std;
import .export_schema;

namespace program;

enum Error { None, Header, Version, Limits, Truncated, Source, Range, Reference, Operation, Branch, Trailing }
// An installed immutable image owns exact bytes. Inspection borrows ranges;
// there are no pointers, runtime values, or line/column indexes in the image.
root struct Image { bytes: u8[>..] = [], valid: bool = false }
// The eval-site section (version 6, light-cljs-858) follows the UTF-16 pool
// at `tail`: three u32 counts (sites, sources, record bytes), then the site
// rows (5 words: the call's instruction row, source start and count, record
// byte start and length), the source rows (3 words: an eval_source:: kind, a
// local or capture index, a role word whose bits 0..7 are an eval_role:: and
// whose eval_role::Cell bit marks a binding cell) and the records' bytes.
struct Layout { source: i64, functions: i64, constants: i64, children: i64, instructions: i64, units: i64, tail: i64 = 0, sites: i64 = 0, sources: i64 = 0, records: i64 = 0, end: i64 }
fn word(bytes: const u8[:], offset: i64) -> i64 {
    var n: i64 = 0;
    for i in 4 { n |= (bytes[offset + i] as i64) << (8 * i); }
    return n;
}
fn unit(bytes: const u8[:], units: i64, index: i64) -> i64 { return bytes[units + index * 2] | ((bytes[units + index * 2 + 1] as i64) << 8); }
fn layout(bytes: const u8[:]) -> Layout {
    let source = 32;
    let functions = source + word(bytes, 8);
    let constants = functions + word(bytes, 12) * function_row::Bytes;
    let children = constants + word(bytes, 16) * 12;
    let instructions = children + word(bytes, 20) * 4;
    let units = instructions + word(bytes, 24) * 20;
    let tail = units + word(bytes, 28) * 2;
    // Bytes too short for the section's counts end past the image.
    if tail < 0 || tail > bytes.len - 12 { return Layout { source: source, functions: functions, constants: constants, children: children, instructions: instructions, units: units, tail: tail, sites: tail + 12, sources: tail + 12, records: tail + 12, end: tail + 12 }; }
    let sites = tail + 12; let sources = sites + word(bytes, tail) * 20; let records = sources + word(bytes, tail + 4) * 12;
    return Layout { source: source, functions: functions, constants: constants, children: children, instructions: instructions, units: units, tail: tail, sites: sites, sources: sources, records: records, end: records + word(bytes, tail + 8) };
}
// The eval site whose call is instruction row `op` of a validated image, or -1.
fn site_row(bytes: const u8[:], l: Layout, op: i64) -> i64 {
    var lo: i64 = 0; var hi = word(bytes, l.tail);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if word(bytes, l.sites + mid * 20) < op { lo = mid + 1; } else { hi = mid; }
    }
    return if lo < word(bytes, l.tail) && word(bytes, l.sites + lo * 20) == op { lo } else { -1 };
}
// One site's source rows: every kind an eval_source::, an index only for a
// local or a capture, roles in capture order (bindings, the receiver once,
// new.target, environments, super home, super fields; eval_role::), the cell
// bit only on a binding or the super fields.
fn site_sources_valid(bytes: const u8[:], l: Layout, start: i64, count: i64) -> bool {
    var last: i64 = -1; var receivers: i64 = 0;
    for j in count {
        let at = l.sources + (start + j) * 12; let kind = word(bytes, at); let index = word(bytes, at + 4); let role_word = word(bytes, at + 8);
        let role = role_word & 255; let cell = (role_word & eval_role::Cell) != 0;
        if kind > eval_source::NewTarget || (role_word & ~(255 | eval_role::Cell)) != 0 || role > eval_role::SuperFields || role < last { return false; }
        if role == last && role != eval_role::Binding && role != eval_role::Environment { return false; }
        if cell && role != eval_role::Binding && role != eval_role::SuperFields { return false; }
        if kind != eval_source::Local && kind != eval_source::Capture && index != 0 || index > 65535 { return false; }
        if role == eval_role::Receiver { receivers += 1; }
        last = role;
    }
    return receivers == 1;
}
fn range(start: i64, length: i64, count: i64) -> bool { return start <= count && length <= count - start; }
// ThrowKind and built-in global selectors are 0..count; other category-6
// operands (counts, form operations) have no selector table here.
fn selector(op: i64, arg: i64) -> bool { let count = export_selector_count(op); return count < 0 || arg < count; }
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
    // Version 3 (light-cljs-823): descriptor bit 24 marks a direct-eval call.
    // Version 4 (light-cljs-853): function flags bit 18 marks source text,
    // whose byte range is the function row's words 13 and 14.
    // Version 5 (light-cljs-854): function flags bit 19 marks a simple
    // parameter list (function_row::SimpleParameters).
    // Version 6 (light-cljs-858): the eval-site section (Layout).
    if word(bytes, 4) != 6 { return Error.Version; }
    if word(bytes, 8) > 8388608 || word(bytes, 12) == 0 || word(bytes, 12) > 65536 { return Error.Limits; }
    for offset in [16, 20, 24, 28] { if word(bytes, offset) > 1048576 { return Error.Limits; } }
    let l = layout(bytes);
    if l.sites > bytes.len { return Error.Truncated; }
    if word(bytes, l.tail) > 1048576 || word(bytes, l.tail + 4) > 1048576 || word(bytes, l.tail + 8) > 8388608 { return Error.Limits; }
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
        // The tags are constant::Undefined..String and Symbol..BigInt; the gap
        // between String and Symbol is the VM's heap-reference value tag.
        if tag > constant::String && tag < constant::Symbol || tag > constant::BigInt { return Error.Operation; }
        if (tag == constant::Undefined || tag == constant::Null) && (a != 0 || b != 0) { return Error.Operation; }
        if tag == constant::Bool && (a > 1 || b != 0) { return Error.Operation; }
        if tag == constant::String && !range(a, b, nu) { return Error.Reference; }
        if tag == constant::Symbol && (export_well_known_symbol(a).len == 0 || b != 0) { return Error.Operation; }
        if tag == constant::BigInt {
            // Sign unit (0 or 1), then little-endian 16-bit digits with no
            // high zero digit; zero is the single unit 0.
            if b == 0 || !range(a, b, nu) { return Error.Reference; }
            let sign = unit(bytes, l.units, a); let high = unit(bytes, l.units, a + b - 1);
            if sign > 1 || b > 1 && high == 0 || b == 1 && sign != 0 { return Error.Operation; }
        }
    }
    // Pool entries can be outside all function ranges; validate those IDs too.
    for i in nch { if word(bytes, l.children + i * 4) >= nf { return Error.Reference; } }
    // Eval sites: ascending call rows, one per marked call (checked with the
    // instructions below); source and record ranges tile their tables in
    // order, each record non-empty and each site's capture count a u16.
    let ns = word(bytes, l.tail); var sources: i64 = 0; var records: i64 = 0; var previous: i64 = -1;
    for s in ns {
        let at = l.sites + s * 20; let op = word(bytes, at);
        if op <= previous || op >= ni { return Error.Reference; }
        if word(bytes, at + 4) != sources || word(bytes, at + 12) != records || word(bytes, at + 16) == 0 { return Error.Range; }
        let count = word(bytes, at + 8);
        if count > 65535 || !range(sources, count, word(bytes, l.tail + 4)) || !range(records, word(bytes, at + 16), word(bytes, l.tail + 8)) { return Error.Range; }
        if !site_sources_valid(bytes, l, sources, count) { return Error.Operation; }
        previous = op; sources += count; records += word(bytes, at + 16);
    }
    if sources != word(bytes, l.tail + 4) || records != word(bytes, l.tail + 8) { return Error.Range; }
    var used:       bool[>..] = []; used.resize(ni, false);
    var child_used: bool[>..] = []; child_used.resize(nf, false); child_used[0] = true;
    var depths:     i64[>..]  = []; depths.resize(nf, 0);
    for id in nf {
        let f = l.functions + id * function_row::Bytes;
        let start = word(bytes, f); let count = word(bytes, f + 4);
        let constants = word(bytes, f + 8); let constant_count = word(bytes, f + 12);
        let children = word(bytes, f + 16); let child_count = word(bytes, f + 20);
        let params = word(bytes, f + 24); let locals = word(bytes, f + 28); let captures = word(bytes, f + 32);
        if count == 0 || !range(start, count, ni) || !range(constants, constant_count, nc) || !range(children, child_count, nch) || !range(word(bytes, f + 36), word(bytes, f + 40), nu) { return Error.Range; }
        if params > locals || locals > 65535 || captures > 65535 || constant_count > 65536 || child_count > 65536 || word(bytes, f + 44) > 65535 { return Error.Limits; }
        let flags = word(bytes, f + 48);
        // Strict bit 0; constructor kind bits 8..15 (none, ordinary, base,
        // derived, default derived class); generator bit 16, async bit 17;
        // source text bit 18 (function_row::SourceText); a simple parameter
        // list bit 19 (function_row::SimpleParameters).
        if (flags & 255) > 1 || ((flags >> 8) & 255) > constructor::DefaultDerivedClass || (flags >> 16) > 15 { return Error.Operation; }
        // The source text: a non-empty byte range of the image's source on
        // UTF-8 boundaries, or both words 0 without the flag.
        let text_start = word(bytes, f + 52); let text_end = word(bytes, f + 56);
        if (flags & function_row::SourceText) == 0 { if text_start != 0 || text_end != 0 { return Error.Source; } }
        else if text_start >= text_end || !boundary(source, text_start) || !boundary(source, text_end) { return Error.Source; }
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
            let at = base + i * 20; let site = word(bytes, at) >> 24; let descriptor = word(bytes, at) & 16777215; let op = descriptor & 255;
            // Bits 0..23 are the registry descriptor; bit 24 marks the call of
            // a direct-eval site (only export_direct_eval_call's opcodes);
            // bits 25..31 are zero.
            if export_descriptor(op) != descriptor || site > 1 || site == 1 && !export_direct_eval_call(op) { return Error.Operation; }
            // A marked call has its site row, and only a marked call; the
            // site's local and capture sources are this function's.
            let row = if ns > 0 { site_row(bytes, l, start + i) } else { -1 };
            if (row >= 0) != (site == 1) { return Error.Reference; }
            if row >= 0 {
                let first = word(bytes, l.sites + row * 20 + 4);
                for j in word(bytes, l.sites + row * 20 + 8) {
                    let src = l.sources + (first + j) * 12; let kind = word(bytes, src); let index = word(bytes, src + 4);
                    if kind == eval_source::Local && index >= locals || kind == eval_source::Capture && index >= captures { return Error.Reference; }
                }
            }
            if op == opcode::FormOp && export_form_arity(word(bytes, at + 4)) < 0 { return Error.Operation; }
            let arg = word(bytes, at + 4); let kind = (descriptor >> 8) & 255; let size = descriptor >> 16;
            if kind == operand::Immediate && !selector(op, arg) { return Error.Operation; }
            if word(bytes, at + 16) != expected_offset { return Error.Range; }
            expected_offset += size + 1;
            if expected_offset > 4294967295 { return Error.Limits; }
            let span_start = word(bytes, at + 8); let span_end = word(bytes, at + 12);
            if span_start > span_end || !boundary(source, span_start) || !boundary(source, span_end) { return Error.Source; }
            if size == 0 && arg != 0 || size == 2 && arg > 65535 { return Error.Limits; }
            if kind == operand::Const && arg >= constant_count || kind == operand::Local && arg >= locals || kind == operand::Function && arg >= child_count || kind == operand::Capture && arg >= captures { return Error.Reference; }
            if kind == operand::Const && export_name_operand(op) && word(bytes, l.constants + (constants + arg) * 12) != constant::String { return Error.Reference; }
            if kind == operand::Target && !branch(bytes, base, count, arg) { return Error.Branch; }
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
