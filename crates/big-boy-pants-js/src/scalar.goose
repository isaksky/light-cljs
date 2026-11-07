import std;
import binary;
import .vm;
import .program;

// Execution is deliberately separate from general export/inspection coverage.
// Macro functions/calls/forms export and inspect, but cannot enter this scalar
// adapter. Unsupported operations return false before installing tiny code.
fn scalar(image: const program::Image&, output: tiny::Program&) -> bool {
    if !image.valid { return false; }
    let bytes = image.bytes[..]; let l = program::layout(bytes);
    if program::word(bytes, 12) != 1 || program::word(bytes, l.functions + 20) != 0 || program::word(bytes, l.functions + 32) != 0 { return false; }
    var constants: tiny::Value[>..] = [];
    for i in program::word(bytes, 16) {
        let at = l.constants + i * 12; let tag = program::word(bytes, at);
        let value: tiny::Value = match tag {
            0 => tiny::Value.Undefined,
            1 => tiny::Value.Null,
            2 => tiny::Value.Bool { value: program::word(bytes, at + 4) == 1 },
            3 => { let n, ok = binary::read_f64le(bytes, at + 4); assert(ok); tiny::Value.Number { value: n } },
            _ => { return false; },
        };
        constants.push(value);
    }
    var code: u8[>..] = [];
    for i in program::word(bytes, 24) {
        let at = l.instructions + i * 20; let op = program::word(bytes, at) & 255;
        // Original HostEffect is a host selector (and has an operand); tiny's
        // counter is synthetic and must never alias it during source loading.
        if op == opcode::NewObject || op == opcode::HostEffect || !tiny::known(op) { return false; }
        if !tiny::emit(code, op, program::word(bytes, at + 4)) { return false; }
    }
    return tiny::load(output, code, constants, program::word(bytes, l.functions + 28));
}
