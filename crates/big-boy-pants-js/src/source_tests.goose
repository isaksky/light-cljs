import std;
import .program;
import .scalar;
import .vm;

fn set_word(bytes: u8[:], at: i64, value: i64) { for i in 4 { bytes[at + i] = ((value >> (8 * i)) & 255) as u8; } }
fn execute(bytes: const u8[:], expected: f64) {
    var image = program::Image {}; assert(program::install(image, bytes) == program::Error.None);
    var code = tiny::Program {}; assert(scalar(image, code));
    var job = tiny::Job { program: code }; tiny::initialize(job, []);
    var session = tiny::Session {};
    assert(tiny::run(session, job, -1) == tiny::Outcome.Complete);
    assert(tiny::equal(job.result, tiny::Value.Number { value: expected }));
}
fn main() {
    let arithmetic = embed_file("../corpus/synthetic/arithmetic.gsp");
    let control = embed_file("../corpus/synthetic/control_flow.gsp");
    let macro = embed_file("../corpus/churn-churn.pants.gsp");
    execute(arithmetic, 27.5); execute(control, 7307);
    let wide = embed_file("../corpus/synthetic/wide.gsp"); execute(wide, 299);
    assert(program::word(wide, 16) > 255);
    let wide_layout = program::layout(wide); assert(program::word(wide, wide_layout.functions + 28) > 255);
    let unicode = embed_file("../corpus/synthetic/unicode.gsp"); assert(program::validate(unicode) == program::Error.None);
    var malformed: u8[>..] = []; malformed.append(unicode);
    let ul = program::layout(unicode);
    var emoji: i64 = -1;
    for i in program::word(unicode, 8) { if unicode[32 + i] == 240 { emoji = i; } }
    assert(emoji >= 0);
    set_word(malformed[..], ul.instructions + 8, emoji + 1);
    assert(program::validate(malformed) == program::Error.Source);
    set_word(malformed[..], ul.instructions + 8, program::word(unicode, ul.instructions + 8));
    malformed[32 + emoji] = 255; assert(program::validate(malformed) == program::Error.Source);
    // Source bytes (light-cljs-864): a lone surrogate of run-time source is
    // its code point's WTF-8 sequence, beside a genuine U+FFFD or a low one
    // before a high one; never a split pair, an overlong form or a cut-off
    // sequence.
    let lone: u8[] = [97, 237, 160, 128, 239, 191, 189, 237, 176, 128, 237, 160, 128, 32, 237, 176, 128];
    assert(program::source_text(lone));
    let pair:     u8[] = [237, 160, 128, 237, 176, 128]; assert(!program::source_text(pair));
    let overlong: u8[] = [224, 128, 128]; assert(!program::source_text(overlong));
    let cut:      u8[] = [237, 160]; assert(!program::source_text(cut));
    var image = program::Image {}; assert(program::install(image, macro) == program::Error.None);
    var scalar_code = tiny::Program {}; assert(!scalar(image, scalar_code) && !scalar_code.valid);
    // Exhaustive truncation of a genuine source-derived macro image.
    for n in macro.len { assert(program::validate(macro[..n]) != program::Error.None); }
    var bad: u8[>..] = []; bad.append(macro);
    let original = copy(image.bytes);
    set_word(bad[..], 4, 1); assert(program::install(image, bad) == program::Error.Version);
    assert(image.valid && image.bytes[..] == original[..]);
    // Version 2 predates the direct-eval site bit (light-cljs-823), version 3
    // the function source text (light-cljs-853), version 4 the simple
    // parameter list bit (light-cljs-854), version 5 the eval-site section
    // (light-cljs-858).
    set_word(bad[..], 4, 2); assert(program::validate(bad) == program::Error.Version);
    set_word(bad[..], 4, 3); assert(program::validate(bad) == program::Error.Version);
    set_word(bad[..], 4, 4); assert(program::validate(bad) == program::Error.Version);
    set_word(bad[..], 4, 5); assert(program::validate(bad) == program::Error.Version);
    set_word(bad[..], 4, 6); set_word(bad[..], 12, 65537); assert(program::validate(bad) == program::Error.Limits);
    set_word(bad[..], 12, program::word(macro, 12));
    let l = program::layout(macro);
    set_word(bad[..], l.functions, 4294967295); assert(program::validate(bad) == program::Error.Range);
    set_word(bad[..], l.functions, program::word(macro, l.functions));
    set_word(bad[..], l.children, 4294967295); assert(program::validate(bad) == program::Error.Reference);
    set_word(bad[..], l.children, program::word(macro, l.children));
    set_word(bad[..], l.instructions, 255); assert(program::validate(bad) == program::Error.Operation);
    set_word(bad[..], l.instructions, program::word(macro, l.instructions));
    set_word(bad[..], l.instructions + 12, 4294967295); assert(program::validate(bad) == program::Error.Source);
    set_word(bad[..], l.instructions + 12, program::word(macro, l.instructions + 12));
    for i in program::word(macro, 24) {
        let at = l.instructions + i * 20; let descriptor = program::word(macro, at); let kind = (descriptor >> 8) & 255;
        if kind >= operand::Const && kind <= operand::Capture {
            set_word(bad[..], at + 4, 4294967295);
            assert(program::validate(bad) != program::Error.None);
            set_word(bad[..], at + 4, program::word(macro, at + 4));
        }
        if (descriptor & 255) == opcode::FormOp {
            set_word(bad[..], at + 4, 65535); assert(program::validate(bad) == program::Error.Operation);
            set_word(bad[..], at + 4, program::word(macro, at + 4));
        }
        // The direct-eval site bit (24) is valid only on the four call
        // opcodes, and only with the call's eval-site row (light-cljs-858),
        // which this image has none of; bits 25..31 never are.
        set_word(bad[..], at, descriptor | 16777216);
        assert(program::validate(bad) == (if export_direct_eval_call(descriptor & 255) { program::Error.Reference } else { program::Error.Operation }));
        set_word(bad[..], at, descriptor | 33554432); assert(program::validate(bad) == program::Error.Operation);
        set_word(bad[..], at, descriptor);
    }
    // Function source text (light-cljs-853): flags bit 18 with a non-empty
    // byte range on UTF-8 boundaries, or neither.
    var texts: i64 = 0;
    for i in program::word(macro, 12) {
        let at = l.functions + i * function_row::Bytes; let flags = program::word(macro, at + 48);
        let start = program::word(macro, at + 52); let end = program::word(macro, at + 56);
        if (flags & function_row::SourceText) == 0 {
            set_word(bad[..], at + 56, 1); assert(program::validate(bad) == program::Error.Source); set_word(bad[..], at + 56, 0);
            continue;
        }
        texts += 1;
        set_word(bad[..], at + 48, flags & ~function_row::SourceText); assert(program::validate(bad) == program::Error.Source); set_word(bad[..], at + 48, flags);
        // A simple parameter list, bit 19 (light-cljs-854), is either way
        // valid; bit 20 is unassigned.
        set_word(bad[..], at + 48, flags ^ function_row::SimpleParameters); assert(program::validate(bad) == program::Error.None);
        set_word(bad[..], at + 48, flags | (function_row::SimpleParameters * 2)); assert(program::validate(bad) == program::Error.Operation); set_word(bad[..], at + 48, flags);
        set_word(bad[..], at + 56, start); assert(program::validate(bad) == program::Error.Source);
        set_word(bad[..], at + 56, program::word(macro, 8) + 1); assert(program::validate(bad) == program::Error.Source); set_word(bad[..], at + 56, end);
    }
    assert(texts > 0);
    var split: u8[>..] = []; split.append(unicode);
    for i in program::word(unicode, 12) {
        let at = ul.functions + i * function_row::Bytes;
        if (program::word(unicode, at + 48) & function_row::SourceText) == 0 || program::word(unicode, at + 52) > emoji || program::word(unicode, at + 56) <= emoji { continue; }
        set_word(split[..], at + 56, emoji + 1); assert(program::validate(split) == program::Error.Source); set_word(split[..], at + 56, program::word(unicode, at + 56));
        texts = -1;
    }
    assert(texts == -1 && program::validate(split) == program::Error.None);
    assert(program::validate(bad) == program::Error.None);
    depth_limit(257, program::Error.None); depth_limit(258, program::Error.Limits);
    bad.push(0); assert(program::validate(bad) == program::Error.Trailing);
    assert(image.valid && image.bytes[..] == original[..]);
    // Even child records outside every function range must contain valid IDs.
    assert(program::word(arithmetic, 20) == 0);
    let arithmetic_layout = program::layout(arithmetic);
    var orphan: u8[>..] = []; orphan.append(arithmetic[..arithmetic_layout.children]);
    push_word(orphan, 4294967295); orphan.append(arithmetic[arithmetic_layout.instructions..]);
    set_word(orphan[..], 20, 1);
    for target in [program::word(arithmetic, 12), 4294967295] {
        set_word(orphan[..], arithmetic_layout.children, target);
        assert(program::validate(orphan) == program::Error.Reference);
        assert(program::install(image, orphan) == program::Error.Reference);
        assert(image.valid && image.bytes[..] == original[..]);
    }
    eval_sites(embed_file("../corpus/serial/direct-eval-reached.gsp"));
    print("source export: arithmetic=27.5, control=7307, wide=299; macro inspection; malformed/truncation/transaction/eval-site checks passed");
}
// The eval-site section (GSP version 6, light-cljs-858) of an image with one
// direct eval: each malformed site, source or record range is refused.
fn eval_sites(good: const u8[:]) {
    assert(program::validate(good) == program::Error.None);
    for n in good.len { assert(program::validate(good[..n]) != program::Error.None); }
    let l = program::layout(good); assert(program::word(good, l.tail) == 1);
    let count = program::word(good, l.sites + 8); assert(count >= 1);
    var bad: u8[>..] = []; bad.append(good);
    // The site names an unmarked instruction, or none.
    let op = program::word(good, l.sites);
    for n in [op - 1, program::word(good, 24)] { set_word(bad[..], l.sites, n); assert(program::validate(bad) == program::Error.Reference); }
    set_word(bad[..], l.sites, op);
    // Source and record ranges tile their tables; a record is non-empty.
    set_word(bad[..], l.sites + 4, 1); assert(program::validate(bad) == program::Error.Range); set_word(bad[..], l.sites + 4, 0);
    set_word(bad[..], l.sites + 8, count + 1); assert(program::validate(bad) == program::Error.Range); set_word(bad[..], l.sites + 8, count);
    let length = program::word(good, l.sites + 16);
    set_word(bad[..], l.sites + 16, 0); assert(program::validate(bad) == program::Error.Range); set_word(bad[..], l.sites + 16, length);
    // Kinds and roles: an unknown kind, an index on a non-indexed kind, a
    // second receiver, an unknown role bit, a local past the function's.
    var receiver: i64 = -1;
    for j in count {
        let at = l.sources + j * 12;
        if (program::word(good, at + 8) & 255) == eval_role::Receiver { receiver = at; }
        set_word(bad[..], at, eval_source::NewTarget + 1); assert(program::validate(bad) == program::Error.Operation); set_word(bad[..], at, program::word(good, at));
        set_word(bad[..], at + 8, program::word(good, at + 8) | 512); assert(program::validate(bad) == program::Error.Operation); set_word(bad[..], at + 8, program::word(good, at + 8));
        if program::word(good, at) == eval_source::Local {
            set_word(bad[..], at + 4, 65535); assert(program::validate(bad) == program::Error.Reference); set_word(bad[..], at + 4, program::word(good, at + 4));
        }
    }
    assert(receiver >= 0);
    set_word(bad[..], receiver, eval_source::This); set_word(bad[..], receiver + 4, 1); assert(program::validate(bad) == program::Error.Operation);
    set_word(bad[..], receiver, program::word(good, receiver)); set_word(bad[..], receiver + 4, program::word(good, receiver + 4));
    set_word(bad[..], receiver + 8, eval_role::Binding); assert(program::validate(bad) == program::Error.Operation);
    set_word(bad[..], receiver + 8, program::word(good, receiver + 8)); assert(program::validate(bad) == program::Error.None);
    // The marked call without its site row: a Reference error.
    var bare: u8[>..] = []; bare.append(good[..l.tail]); for n in [0, 0, 0] { push_word(bare, n); }
    assert(program::validate(bare) == program::Error.Reference);
}

fn push_word(bytes: u8[>..]&, value: i64) { for i in 4 { bytes.push(((value >> (8 * i)) & 255) as u8); } }
fn depth_limit(count: i64, expected: program::Error) {
    var bytes: u8[>..] = []; bytes.append("GSP\0");
    for n in [6, 0, count, 0, count - 1, count * 2, 0] { push_word(bytes, n); }
    for i in count {
        for n in [i * 2, 2, 0, 0, i, if i + 1 < count { 1 } else { 0 }, 0, 0, 0, 0, 0, 0, 0, 0, 0] { push_word(bytes, n); }
    }
    for i in (count - 1) { push_word(bytes, i + 1); }
    for i in count { for n in [export_descriptor(opcode::Undefined), 0, 0, 0, 0, export_descriptor(opcode::Return), 0, 0, 0, 1] { push_word(bytes, n); } }
    for n in [0, 0, 0] { push_word(bytes, n); }
    assert(program::validate(bytes) == expected);
}
