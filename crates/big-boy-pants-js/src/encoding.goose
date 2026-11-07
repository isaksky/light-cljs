import std;
import .vm;

// Migration helper for the historical synthetic test notation ONLY. Actual
// exported files never pass through this byte-sized fixture assembler.
fn load8(p: tiny::Program&, old: const u8[:], constants: const tiny::Value[:], locals: i64) -> bool {
    var offsets: i64[>..] = []; offsets.resize(old.len + 1, -1);
    var code:    u8[>..]  = [];
    var pos = 0;
    var size = 0;
    while pos < old.len {
        let op = old[pos]; offsets[pos] = size;
        size += tiny::width(op);
        pos += if tiny::width(op) > 1 { 2 } else { 1 };
    }
    assert(pos == old.len);
    offsets[old.len] = size;
    pos = 0;
    while pos < old.len {
        let op = old[pos]; var arg: i64 = 0;
        if tiny::width(op) > 1 {
            arg = old[pos + 1];
            if op == opcode::Jump || op == opcode::JumpIfFalse { assert(arg < offsets.len && offsets[arg] >= 0); arg = offsets[arg]; }
            pos += 1;
        }
        assert(tiny::emit(code, op, arg)); pos += 1;
    }
    return tiny::load(p, code, constants, locals);
}
