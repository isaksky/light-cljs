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
// The regular-expression section (version 7, light-cljs-932) follows the
// records at `patterns` (PatternLayout); `end` is its end.
struct Layout { source: i64, functions: i64, constants: i64, children: i64, instructions: i64, units: i64, tail: i64 = 0, sites: i64 = 0, sources: i64 = 0, records: i64 = 0, patterns: i64 = 0, end: i64 }
// The regular-expression section (GSP version 7, light-cljs-932; the
// exporter's PATTERN_WORDS documents it): at `counts` three u32 counts
// (programs, op rows, UTF-16 units), then the program rows
// (pattern_row::Words words), the op rows (5 words) and the units. A
// run-time pattern (runtime_compile.goose) is such a section alone, of one
// program whose instruction row is pattern_row::Runtime.
struct PatternLayout { counts: i64, rows: i64 = 0, ops: i64 = 0, units: i64 = 0, end: i64 }
// One compiled regular expression (light-cljs-932): the bbp Rust
// frontend's matcher program for a pattern and its flags, as the Code
// tables hold it. `op` is a literal's instruction row (`literal`), or for a
// run-time pattern the code's instruction count when it was compiled, so the
// rows stay in ascending `op` order; `flags` are regexp_flag:: bits; a
// `failure` (regexp_failure::) has no program, its message being its units.
// Otherwise `start` is its first row in the op table: `instructions`
// instructions, then `members` member rows, then `names` capture-name rows,
// one per group after group zero; `captures` counts group zero, `repeats`
// the repeat slots; `units`/`unit_count` its run of the code units (the
// failure's message or the capture names' text).
struct Pattern { op: u32, literal: bool, flags: u8, failure: u8, captures: u32, repeats: u32, start: u32, instructions: u32, members: u32, names: u32, units: u32, unit_count: u32 }
// One op row (regexp_op::, its regexp_op_flag:: bits and operands a to d;
// positions and targets are indices into its program's run).
struct PatternOp { kind: u8, flags: u8, a: u32, b: u32, c: u32, d: u32 }
fn pattern_layout(bytes: const u8[:], at: i64) -> PatternLayout {
    if at < 0 || at > bytes.len - 12 { return PatternLayout { counts: at, rows: at + 12, ops: at + 12, units: at + 12, end: at + 12 }; }
    let rows = at + 12; let ops = rows + word(bytes, at) * pattern_row::Words * 4; let units = ops + word(bytes, at + 4) * 20;
    return PatternLayout { counts: at, rows: rows, ops: ops, units: units, end: units + word(bytes, at + 8) * 2 };
}
// Program row r of a section whose bounds were checked, its unit run still
// relative to the section's units.
fn pattern_at(bytes: const u8[:], p: PatternLayout, r: i64) -> Pattern {
    let at = p.rows + r * pattern_row::Words * 4;
    return Pattern { op: word(bytes, at) as u32, literal: word(bytes, at) != pattern_row::Runtime, flags: word(bytes, at + 4) as u8, failure: word(bytes, at + 8) as u8, captures: word(bytes, at + 12) as u32, repeats: word(bytes, at + 16) as u32,
        start: word(bytes, at + 20) as u32, instructions: word(bytes, at + 24) as u32, members: word(bytes, at + 28) as u32, names: word(bytes, at + 32) as u32, units: word(bytes, at + 36) as u32, unit_count: word(bytes, at + 40) as u32 };
}
fn pattern_op(bytes: const u8[:], p: PatternLayout, i: i64) -> PatternOp {
    let at = p.ops + i * 20; let head = word(bytes, at);
    return PatternOp { kind: (head & 255) as u8, flags: ((head >> 8) & 255) as u8, a: word(bytes, at + 4) as u32, b: word(bytes, at + 8) as u32, c: word(bytes, at + 12) as u32, d: word(bytes, at + 16) as u32 };
}
// The program row of literal instruction row `op` of a validated image's
// section, or -1.
fn literal_pattern(bytes: const u8[:], p: PatternLayout, op: i64) -> i64 {
    var lo: i64 = 0; var hi = word(bytes, p.counts);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if word(bytes, p.rows + mid * pattern_row::Words * 4) < op { lo = mid + 1; } else { hi = mid; }
    }
    return if lo < word(bytes, p.counts) && word(bytes, p.rows + lo * pattern_row::Words * 4) == op { lo } else { -1 };
}
// The regexp_op_flag:: bits an op kind may carry.
fn op_flags(kind: i64) -> i64 {
    return match kind {
        regexp_op::Char, regexp_op::Backreference => regexp_op_flag::IgnoreCase,
        regexp_op::Dot => regexp_op_flag::DotAll,
        regexp_op::Class => regexp_op_flag::IgnoreCase | regexp_op_flag::Inverted,
        regexp_op::Assert => regexp_op_flag::IgnoreCase | regexp_op_flag::Multiline,
        regexp_op::RepeatHead => regexp_op_flag::Greedy,
        regexp_op::Look => regexp_op_flag::Negative,
        _ => 0,
    };
}
// Whether program p's run `ops` (its instructions, members and names) and
// its `units` of text form a program the matcher can run without a check:
// every kind, flag and operand in range, every target an instruction, every
// group and repeat slot the program's, every Class member a Range or
// Escape and every Backreference member a Capture, the names one per
// group, the instructions ending in Accept. A failure has no rows. The
// image validator (validate_patterns) and snapshot validation
// (snapshot::patterns_valid) share it.
fn pattern_valid(p: Pattern, ops: const PatternOp[:], units: const u16[:]) -> bool {
    if p.flags & regexp_flag::Unicode != 0 && p.flags & regexp_flag::UnicodeSets != 0 { return false; }
    if p.failure != regexp_failure::None {
        return p.failure <= regexp_failure::Unsupported && p.captures == 0 && p.repeats == 0 && p.instructions == 0 && p.members == 0 && p.names == 0 && ops.len == 0 && units.len == p.unit_count;
    }
    let n: i64 = p.instructions; let members: i64 = p.members; let captures: i64 = p.captures; let repeats: i64 = p.repeats;
    if n == 0 || captures == 0 || p.names != captures - 1 || ops.len != n + members + captures - 1 || units.len != p.unit_count || captures > 1048576 || repeats > 1048576 { return false; }
    if ops[n - 1].kind as i64 != regexp_op::Accept { return false; }
    for x, i in ops {
        let kind: i64 = x.kind; let a: i64 = x.a; let b: i64 = x.b; let c: i64 = x.c; let d: i64 = x.d;
        if i < n {
            if kind > regexp_op::Accept || x.flags & ~op_flags(kind) != 0 { return false; }
            let ok = match kind {
                regexp_op::Char => a <= 1114111 && b == 0 && c == 0 && d == 0,
                regexp_op::Class, regexp_op::Backreference => {
                    var fits = a >= n && b <= n + members - a && c == 0 && d == 0 && (kind == regexp_op::Class || b > 0);
                    if fits { for m in ops[a..(a + b)] { let mk: i64 = m.kind; if (if kind == regexp_op::Class { mk != regexp_op::Range && mk != regexp_op::Escape } else { mk != regexp_op::Capture }) { fits = false; } } }
                    fits
                },
                regexp_op::Assert => a <= regexp_assertion::NotWordBoundary && b == 0 && c == 0 && d == 0,
                regexp_op::Start, regexp_op::End => a >= 1 && a < captures && b == 0 && c == 0 && d == 0,
                regexp_op::Clear => a >= 1 && a <= b && b < captures && c == 0 && d == 0,
                regexp_op::Split, regexp_op::Jump, regexp_op::Look => a < n && b == 0 && c == 0 && d == 0,
                regexp_op::RepeatInit, regexp_op::RepeatBegin => a < repeats && b == 0 && c == 0 && d == 0,
                regexp_op::RepeatHead => a < repeats && b <= c && d < n && i + 1 < n,
                regexp_op::RepeatEnd => a < repeats && c < n && d == 0,
                _ => a == 0 && b == 0 && c == 0 && d == 0,
            };
            if !ok { return false; }
        } else if i < n + members {
            if x.flags != 0 || c != 0 || d != 0 { return false; }
            let ok = match kind {
                regexp_op::Range => a <= b && b <= 1114111,
                regexp_op::Escape => a <= regexp_escape::NotSpace && b == 0,
                regexp_op::Capture => a >= 1 && a < captures && b == 0,
                _ => false,
            };
            if !ok { return false; }
        } else {
            // A name: its text's range in the program's units, or none.
            if kind != regexp_op::Name || x.flags != 0 || d != 0 || c > 1 || c == 0 && (a != 0 || b != 0) || !range(a, b, units.len) { return false; }
        }
    }
    return true;
}
// The section at `at` of an image with `ni` instructions (`image`) or a
// run-time pattern (one program, pattern_row::Runtime): its counts within
// the image limits, its programs ascending by literal row and each one
// tiling the op rows and units in order and valid (pattern_valid). Whether
// every literal instruction has its program the caller checks.
fn validate_patterns(bytes: const u8[:], p: PatternLayout, ni: i64, image: bool) -> Error {
    if p.counts < 0 || p.rows > bytes.len { return Error.Truncated; }
    let np = word(bytes, p.counts); let nops = word(bytes, p.counts + 4); let nu = word(bytes, p.counts + 8);
    if np > 1048576 || nops > 1048576 || nu > 1048576 { return Error.Limits; }
    if p.end > bytes.len { return Error.Truncated; }
    if p.end < bytes.len { return Error.Trailing; }
    if !image && np != 1 { return Error.Range; }
    var ops: i64            = 0; var units: i64 = 0; var previous: i64 = -1;
    var run: PatternOp[>..] = []; var text: u16[>..] = [];
    for i in nops { if word(bytes, p.ops + i * 20) > 65535 { return Error.Operation; } }
    for r in np {
        let at = p.rows + r * pattern_row::Words * 4;
        if word(bytes, at + 4) > 255 || word(bytes, at + 8) > regexp_failure::Unsupported { return Error.Operation; }
        let x = pattern_at(bytes, p, r);
        if image && (!x.literal || x.op <= previous || x.op >= ni) || !image && x.literal { return Error.Reference; }
        previous = x.op;
        let total: i64 = x.instructions + x.members + x.names;
        if x.start != ops || x.units != units || !range(ops, total, nops) || !range(units, x.unit_count, nu) { return Error.Range; }
        run.clear(); for i in total { run.push(pattern_op(bytes, p, ops + i)); }
        text.clear(); for i in x.unit_count { text.push(unit(bytes, p.units, units + i) as u16); }
        if !pattern_valid(x, run, text) { return Error.Operation; }
        ops += total; units += x.unit_count;
    }
    if ops != nops || units != nu { return Error.Range; }
    return Error.None;
}
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
    if tail < 0 || tail > bytes.len - 12 { return Layout { source: source, functions: functions, constants: constants, children: children, instructions: instructions, units: units, tail: tail, sites: tail + 12, sources: tail + 12, records: tail + 12, patterns: tail + 12, end: tail + 12 }; }
    let sites = tail + 12; let sources = sites + word(bytes, tail) * 20; let records = sources + word(bytes, tail + 4) * 12;
    let patterns = records + word(bytes, tail + 8);
    return Layout { source: source, functions: functions, constants: constants, children: children, instructions: instructions, units: units, tail: tail, sites: sites, sources: sources, records: records, patterns: patterns, end: pattern_layout(bytes, patterns).end };
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
// An image's source bytes: UTF-8, except that a lone surrogate of the
// run-time compiler's UTF-16 input is its code point's three-byte sequence
// (generalized UTF-8, WTF-8; light-cljs-864), so a function's source text
// decodes (append_utf8) to the exact units it was compiled from. Shortest
// forms only, and no high surrogate directly followed by a low one, whose
// pair has its own four-byte form.
fn source_text(bytes: const u8[:]) -> bool {
    var remaining: i64 = 0; var cp: i64 = 0; var minimum: i64 = 0; var high = false;
    for byte in bytes {
        if remaining == 0 {
            if byte < 128 { high = false; continue; }
            if byte >= 194 && byte <= 223 { remaining = 1; cp = byte & 31; minimum = 128; }
            else if byte >= 224 && byte <= 239 { remaining = 2; cp = byte & 15; minimum = 2048; }
            else if byte >= 240 && byte <= 244 { remaining = 3; cp = byte & 7; minimum = 65536; }
            else { return false; }
        } else {
            if (byte & 192) != 128 { return false; }
            cp = cp * 64 + (byte & 63); remaining -= 1;
            if remaining == 0 {
                if cp < minimum || cp > 1114111 || high && cp >= 56320 && cp <= 57343 { return false; }
                high = cp >= 55296 && cp <= 56319;
            }
        }
    }
    return remaining == 0;
}
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
    // Version 7 (light-cljs-932): the regular-expression section
    // (PatternLayout), one program per RegExpLiteral instruction.
    if word(bytes, 4) != 7 { return Error.Version; }
    if word(bytes, 8) > 8388608 || word(bytes, 12) == 0 || word(bytes, 12) > 65536 { return Error.Limits; }
    for offset in [16, 20, 24, 28] { if word(bytes, offset) > 1048576 { return Error.Limits; } }
    let l = layout(bytes);
    if l.sites > bytes.len { return Error.Truncated; }
    if word(bytes, l.tail) > 1048576 || word(bytes, l.tail + 4) > 1048576 || word(bytes, l.tail + 8) > 8388608 { return Error.Limits; }
    if l.end > bytes.len { return Error.Truncated; }
    if l.end < bytes.len { return Error.Trailing; }
    let source = bytes[l.source..l.functions];
    // Validate all of the source, unreferenced bytes included (source_text:
    // UTF-8 with lone surrogates of run-time source as WTF-8).
    if !source_text(source) { return Error.Source; }
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
    // Regular expressions (version 7): ascending literal rows, each program
    // valid; every RegExpLiteral instruction has its row (counted below).
    let pl = pattern_layout(bytes, l.patterns);
    let patterns = validate_patterns(bytes, pl, ni, true);
    if patterns != Error.None { return patterns; }
    var literals:   i64       = 0;
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
            if op == opcode::RegExpLiteral { if literal_pattern(bytes, pl, start + i) < 0 { return Error.Reference; } literals += 1; }
        }
    }
    if literals != word(bytes, pl.counts) { return Error.Reference; }
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
