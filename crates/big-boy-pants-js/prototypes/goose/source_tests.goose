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
    let arithmetic = embed_file("corpus/synthetic/arithmetic.gsp");
    let control = embed_file("corpus/synthetic/control_flow.gsp");
    let macro = embed_file("corpus/churn-churn.pants.gsp");
    execute(arithmetic, 27.5); execute(control, 7307);
    let wide = embed_file("corpus/synthetic/wide.gsp"); execute(wide, 299);
    assert(program::word(wide, 16) > 255);
    let wide_layout = program::layout(wide); assert(program::word(wide, wide_layout.functions + 28) > 255);
    let unicode = embed_file("corpus/synthetic/unicode.gsp"); assert(program::validate(unicode) == program::Error.None);
    var malformed: u8[>..] = []; malformed.append(unicode);
    let ul = program::layout(unicode);
    var emoji: i64 = -1;
    for i in program::word(unicode, 8) { if unicode[32 + i] == 240 { emoji = i; } }
    assert(emoji >= 0);
    set_word(malformed[..], ul.instructions + 8, emoji + 1);
    assert(program::validate(malformed) == program::Error.Source);
    set_word(malformed[..], ul.instructions + 8, program::word(unicode, ul.instructions + 8));
    malformed[32 + emoji] = 255; assert(program::validate(malformed) == program::Error.Source);
    var image = program::Image {}; assert(program::install(image, macro) == program::Error.None);
    var scalar_code = tiny::Program {}; assert(!scalar(image, scalar_code) && !scalar_code.valid);
    // Exhaustive truncation of a genuine source-derived macro image.
    for n in macro.len { assert(program::validate(macro[..n]) != program::Error.None); }
    var bad: u8[>..] = []; bad.append(macro);
    let original = copy(image.bytes);
    set_word(bad[..], 4, 1); assert(program::install(image, bad) == program::Error.Version);
    assert(image.valid && image.bytes[..] == original[..]);
    set_word(bad[..], 4, 2); set_word(bad[..], 12, 65537); assert(program::validate(bad) == program::Error.Limits);
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
        if kind >= 1 && kind <= 5 {
            set_word(bad[..], at + 4, 4294967295);
            assert(program::validate(bad) != program::Error.None);
            set_word(bad[..], at + 4, program::word(macro, at + 4));
        }
        if (descriptor & 255) == 194 {
            set_word(bad[..], at + 4, 65535); assert(program::validate(bad) == program::Error.Operation);
            set_word(bad[..], at + 4, program::word(macro, at + 4));
        }
    }
    assert(program::validate(bad) == program::Error.None);
    depth_limit(257, program::Error.None); depth_limit(258, program::Error.Limits);
    bad.push(0); assert(program::validate(bad) == program::Error.Trailing);
    assert(image.valid && image.bytes[..] == original[..]);
    print("source export: arithmetic=27.5, control=7307, wide=299; macro inspection; malformed/truncation/transaction checks passed");
}

fn push_word(bytes: u8[>..]&, value: i64) { for i in 4 { bytes.push(((value >> (8 * i)) & 255) as u8); } }
fn depth_limit(count: i64, expected: program::Error) {
    var bytes: u8[>..] = []; bytes.append("GSP\0");
    for n in [2, 0, count, 0, count - 1, count * 2, 0] { push_word(bytes, n); }
    for i in count {
        for n in [i * 2, 2, 0, 0, i, if i + 1 < count { 1 } else { 0 }, 0, 0, 0, 0, 0, 0, 0] { push_word(bytes, n); }
    }
    for i in (count - 1) { push_word(bytes, i + 1); }
    for i in count { for n in [2, 0, 0, 0, 0, 15, 0, 0, 0, 1] { push_word(bytes, n); } }
    assert(program::validate(bytes) == expected);
}
