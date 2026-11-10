import std;
import .program;
import .export_schema;

namespace source;

// The backtracking matcher (light-cljs-933; decision 5 of
// docs/javascript-coverage.md; the Pants engine's rust/src/regexp/matcher.rs
// `Machine::run` and `exec_budgeted`, whose semantics and work accounting it
// ports; QuickJS's lre_exec is a semantic reference only).
//
// It runs a compiled program (light-cljs-932: program::Pattern, its op rows
// in Code.pattern_ops, the Pants engine's own matcher instructions) over a
// string's run as it is stored, Latin-1 or UTF-16 (`search` is generic over
// the code unit), from a start position, and leaves the groups' offsets in
// the Matcher. A search is a leaf: no JavaScript runs inside it and it
// keeps nothing (decision 3), so it is a plain Goose function whose scratch,
// the captures, repeat slots, undo trail and backtrack stack, is the
// caller's local Matcher; no host recursion follows the pattern's or the
// input's depth.
//
// Positions are UTF-16 code-unit offsets. Without the `u` or `v` flag a
// character is a code unit; with either it is a code point, a surrogate
// pair read as one and a lone surrogate as itself, and a start inside a
// pair moves back to the pair's lead unit. Assertions classify the code
// units beside the position (line terminators and word characters are all
// in the Basic Multilingual Plane, so either half of a pair is neither).
//
// Work: one step per instruction, candidate position, capture or repeat
// write, undo, saved choice and choice popped, class member scanned and
// backreference unit compared, plus the group and repeat slots at the
// start and the groups (five more each with `d`) at a match, against an
// allowance of at most max_match_steps() for the whole search
// (MAX_MATCH_STEPS). Running out is match_status::Limit: the caller's
// RangeError "regular expression step limit exceeded" (QuickJS keeps
// going: the oracles disagree, and Goose follows the Pants engine), or its
// work limit when the job's remaining work was the smaller allowance.
//
// Case-insensitive matching needs Canonicalize (light-cljs-936): a Char,
// Class or Backreference with regexp_op_flag::IgnoreCase that runs ends the
// search with match_status::IgnoreCase, which the caller fails by name. A
// word-boundary assertion needs none: its word characters gain only U+017F
// and U+212A under `i` with `u` or `v` (WordCharacters, ECMAScript 22.2.2.9.3).

// A search's outcome.
let match_status::None = 0;
let match_status::Matched = 1;
let match_status::Limit = 2;
let match_status::IgnoreCase = 3;
// A program whose instructions break the matcher's invariants (a LookEnd
// with no Look before it, an End before its Start): the image validator
// checks operands only, so a malformed program ends here, never in a
// wrong answer.
let match_status::Invalid = 4;
// A backtrack entry's kind: an ordinary choice, or the boundary a
// lookahead's Look saved (positive or negative).
let choice_kind::Choice: u8 = 0;
let choice_kind::Positive: u8 = 1;
let choice_kind::Negative: u8 = 2;
// The Pants engine's allowance for one search (MAX_MATCH_STEPS).
inline fn max_match_steps() -> i64 { return 1000000; }

// Resume at `pc` and `pos` with the undo trail cut to `trail`.
struct Choice { pc: u32, pos: u32, trail: u32, kind: u8 }
// A group's (or, `repeat`, a repeat slot's) pair before a write: a
// group's start and end (-1 for unmatched), a slot's count and start.
struct Undo { index: u32, repeat: bool = false, a: i64, b: i64 }
// A search's scratch, local to the call that owns it: group g's offsets
// at caps[2g] and caps[2g + 1] (-1 when unmatched; a group whose Start ran
// holds an empty range at its start until its End), repeat slot r's count
// and start at reps[2r] and reps[2r + 1]; `used` the work spent of
// `allowance`.
root struct Matcher { caps: i64[>..] = [], reps: i64[>..] = [], trail: Undo[>..] = [], stack: Choice[>..] = [], used: i64 = 0, allowance: i64 = 0 }

inline fn spend(m: Matcher&, n: i64) -> bool { m.used += n; return m.used <= m.allowance; }
// Undo the trail down to `to` entries.
fn rewind(m: Matcher&, to: i64) -> bool {
    while m.trail.len > to {
        if !spend(m, 1) { return false; }
        let u = m.trail.pop(); let i: i64 = u.index;
        if u.repeat { m.reps[2 * i] = u.a; m.reps[2 * i + 1] = u.b; } else { m.caps[2 * i] = u.a; m.caps[2 * i + 1] = u.b; }
    }
    return true;
}
fn set_group(m: Matcher&, g: i64, start: i64, end: i64) -> bool {
    if !spend(m, 1) { return false; }
    m.trail.push(Undo { index: g as u32, a: m.caps[2 * g], b: m.caps[2 * g + 1] });
    m.caps[2 * g] = start; m.caps[2 * g + 1] = end;
    return true;
}
fn set_repeat(m: Matcher&, r: i64, count: i64, start: i64) -> bool {
    if !spend(m, 1) { return false; }
    m.trail.push(Undo { index: r as u32, repeat: true, a: m.reps[2 * r], b: m.reps[2 * r + 1] });
    m.reps[2 * r] = count; m.reps[2 * r + 1] = start;
    return true;
}
fn save(m: Matcher&, pc: i64, pos: i64, kind: u8) -> bool {
    if !spend(m, 1) { return false; }
    m.stack.push(Choice { pc: pc as u32, pos: pos as u32, trail: m.trail.len as u32, kind: kind });
    return true;
}

inline fn lead_surrogate(c: i64) -> bool { return c >= 55296 && c <= 56319; }
inline fn trail_surrogate(c: i64) -> bool { return c >= 56320 && c <= 57343; }
// The character at i (< input.len) and its width in code units: a code
// unit, or in Unicode mode a surrogate pair's code point.
inline fn character<T>(input: const T[:], i: i64, unicode: bool) -> i64, i64 {
    let c: i64 = input[i];
    if unicode && lead_surrogate(c) && i + 1 < input.len {
        let d: i64 = input[i + 1];
        if trail_surrogate(d) { return 65536 + ((c - 55296) << 10) + d - 56320, 2; }
    }
    return c, 1;
}
// AdvanceStringIndex: one character, or one unit at the end.
fn advance_index<T>(input: const T[:], i: i64, unicode: bool) -> i64 {
    if i >= input.len { return i + 1; }
    let c, w = character(input, i, unicode);
    return i + w;
}
inline fn line_terminator(c: i64) -> bool { return c == 10 || c == 13 || c == 8232 || c == 8233; }
// IsWordChar of a code unit (-1 for none): WordCharacters, with U+017F
// and U+212A under `i` in Unicode mode (`folded`).
inline fn word_unit(c: i64, folded: bool) -> bool { return c >= 48 && c <= 57 || c >= 65 && c <= 90 || c == 95 || c >= 97 && c <= 122 || folded && (c == 383 || c == 8490); }
fn space_char(c: i64) -> bool {
    return line_terminator(c) || c == 9 || c == 11 || c == 12 || c == 32 || c == 160 || c == 5760 || c >= 8192 && c <= 8202 || c == 8239 || c == 8287 || c == 12288 || c == 65279;
}
fn class_escape(escape: i64, c: i64) -> bool {
    return match escape {
        regexp_escape::Digit => c >= 48 && c <= 57,
        regexp_escape::NotDigit => !(c >= 48 && c <= 57),
        regexp_escape::Word => word_unit(c, false),
        regexp_escape::NotWord => !word_unit(c, false),
        regexp_escape::Space => space_char(c),
        _ => !space_char(c),
    };
}

// Search `input` with program p (its run `ops`: instructions, then member
// rows) from code unit `from`, every candidate position after it unless
// p is sticky, within m.allowance: a match_status::, with a match's groups
// in m.caps (group zero the whole match) and the work spent in m.used.
// `from` past the input is no match.
fn search<T>(m: Matcher&, ops: const program::PatternOp[:], p: program::Pattern, input: const T[:], from: i64) -> i64 {
    let unicode = p.flags & (regexp_flag::Unicode | regexp_flag::UnicodeSets) != 0;
    let sticky = p.flags & regexp_flag::Sticky != 0;
    let len = input.len;
    m.used = 0;
    if from > len { return match_status::None; }
    var index = from;
    if unicode && index > 0 && index < len && trail_surrogate(input[index]) && lead_surrogate(input[index - 1]) { index -= 1; }
    let groups: i64 = p.captures; let slots: i64 = p.repeats;
    if !spend(m, groups) || !spend(m, slots) { return match_status::Limit; }
    m.caps.clear(); m.caps.resize(2 * groups, -1);
    m.reps.clear(); m.reps.resize(2 * slots, 0);
    m.trail.clear(); m.stack.clear();
    loop {
        if !spend(m, 1) { return match_status::Limit; }
        var pc: i64 = 0; var pos = index;
        var matched = false;
        loop {
            if !spend(m, 1) { return match_status::Limit; }
            let op = ops[pc]; let kind: i64 = op.kind;
            pc += 1;
            var ok = true;
            match kind {
                regexp_op::Char => {
                    if op.flags & regexp_op_flag::IgnoreCase != 0 { return match_status::IgnoreCase; }
                    ok = false;
                    if pos < len {
                        let c, w = character(input, pos, unicode);
                        if c == op.a { pos += w; ok = true; }
                    }
                },
                regexp_op::Dot => {
                    ok = false;
                    if pos < len {
                        let c, w = character(input, pos, unicode);
                        if op.flags & regexp_op_flag::DotAll != 0 || !line_terminator(c) { pos += w; ok = true; }
                    }
                },
                regexp_op::Class => {
                    if op.flags & regexp_op_flag::IgnoreCase != 0 { return match_status::IgnoreCase; }
                    ok = false;
                    if pos < len {
                        let c, w = character(input, pos, unicode);
                        let first: i64 = op.a; let count: i64 = op.b;
                        var found = false;
                        for member in ops[first..(first + count)] {
                            if !spend(m, 1) { return match_status::Limit; }
                            let mk: i64 = member.kind;
                            found = if mk == regexp_op::Range { c >= member.a && c <= member.b } else { class_escape(member.a, c) };
                            if found { break; }
                        }
                        if found != (op.flags & regexp_op_flag::Inverted != 0) { pos += w; ok = true; }
                    }
                },
                regexp_op::Assert => {
                    let a: i64 = op.a;
                    let folded = op.flags & regexp_op_flag::IgnoreCase != 0 && unicode;
                    let previous: i64 = if pos > 0 { input[pos - 1] as i64 } else { -1 };
                    let next:     i64 = if pos < len { input[pos] as i64 } else { -1 };
                    let multiline = op.flags & regexp_op_flag::Multiline != 0;
                    ok = match a {
                        regexp_assertion::Start => pos == 0 || multiline && line_terminator(previous),
                        regexp_assertion::End => pos == len || multiline && line_terminator(next),
                        regexp_assertion::WordBoundary => word_unit(previous, folded) != word_unit(next, folded),
                        _ => word_unit(previous, folded) == word_unit(next, folded),
                    };
                },
                regexp_op::Backreference => {
                    if op.flags & regexp_op_flag::IgnoreCase != 0 { return match_status::IgnoreCase; }
                    let first: i64 = op.a; let count: i64 = op.b;
                    var start: i64 = -1; var end: i64 = -1;
                    for member in ops[first..(first + count)] {
                        if !spend(m, 1) { return match_status::Limit; }
                        let g: i64 = member.a;
                        if m.caps[2 * g] >= 0 { start = m.caps[2 * g]; end = m.caps[2 * g + 1]; break; }
                    }
                    var i = start;
                    while i < end {
                        if !spend(m, 1) { return match_status::Limit; }
                        let expected, width = character(input, i, unicode);
                        if pos >= len { ok = false; break; }
                        let actual, actual_width = character(input, pos, unicode);
                        if actual != expected { ok = false; break; }
                        pos += actual_width; i += width;
                    }
                },
                regexp_op::Start => { if !set_group(m, op.a, pos, pos) { return match_status::Limit; } },
                regexp_op::End => {
                    let g: i64 = op.a;
                    if m.caps[2 * g] < 0 { return match_status::Invalid; }
                    if !set_group(m, g, m.caps[2 * g], pos) { return match_status::Limit; }
                },
                regexp_op::Clear => {
                    let last: i64 = op.b;
                    var g: i64 = op.a;
                    while g <= last { if !set_group(m, g, -1, -1) { return match_status::Limit; } g += 1; }
                },
                regexp_op::Split => { if !save(m, op.a, pos, choice_kind::Choice) { return match_status::Limit; } },
                regexp_op::Jump => { pc = op.a; },
                regexp_op::RepeatInit => { if !set_repeat(m, op.a, 0, 0) { return match_status::Limit; } },
                regexp_op::RepeatHead => {
                    let slot: i64 = op.a;
                    let count = m.reps[2 * slot];
                    if op.c != regexp_repeat::Unbounded && count >= op.c { pc = op.d; }
                    else if count < op.b {}
                    else if op.flags & regexp_op_flag::Greedy != 0 { if !save(m, op.d, pos, choice_kind::Choice) { return match_status::Limit; } }
                    else { if !save(m, pc, pos, choice_kind::Choice) { return match_status::Limit; } pc = op.d; }
                },
                regexp_op::RepeatBegin => {
                    let slot: i64 = op.a;
                    if !set_repeat(m, slot, m.reps[2 * slot], pos) { return match_status::Limit; }
                },
                regexp_op::RepeatEnd => {
                    // RepeatMatcher rejects an empty iteration once the
                    // minimum is met; failing here lets an alternative
                    // inside the atom backtrack into a nonempty one.
                    let slot: i64 = op.a;
                    let count = m.reps[2 * slot];
                    if count >= op.b && m.reps[2 * slot + 1] == pos { ok = false; }
                    else {
                        if !set_repeat(m, slot, count + 1, m.reps[2 * slot + 1]) { return match_status::Limit; }
                        pc = op.c;
                    }
                },
                regexp_op::Look => { if !save(m, op.a, pos, if op.flags & regexp_op_flag::Negative != 0 { choice_kind::Negative } else { choice_kind::Positive }) { return match_status::Limit; } },
                regexp_op::LookEnd => {
                    // The lookahead's body matched: drop its choices (it is
                    // atomic) down to its boundary, then continue after it
                    // at the position before it, keeping its captures; a
                    // negative one undoes them and fails.
                    loop {
                        if !spend(m, 1) { return match_status::Limit; }
                        if m.stack.len == 0 { return match_status::Invalid; }
                        let f = m.stack.pop();
                        if f.kind == choice_kind::Choice { continue; }
                        pc = f.pc; pos = f.pos;
                        if f.kind == choice_kind::Negative { if !rewind(m, f.trail) { return match_status::Limit; } ok = false; }
                        break;
                    }
                },
                regexp_op::Accept => { matched = true; },
                _ => { return match_status::Invalid; },
            }
            if matched { break; }
            if ok { continue; }
            // Backtrack: the newest choice resumes; a positive
            // lookahead's boundary means its body failed, so the failure
            // goes on below it; a negative one's that its body failed,
            // so the lookahead holds and matching resumes after it.
            var resumed = false;
            while m.stack.len > 0 {
                if !spend(m, 1) { return match_status::Limit; }
                let f = m.stack.pop();
                if !rewind(m, f.trail) { return match_status::Limit; }
                pc = f.pc; pos = f.pos;
                if f.kind != choice_kind::Positive { resumed = true; break; }
            }
            if !resumed { break; }
        }
        if matched {
            if !spend(m, groups) { return match_status::Limit; }
            if p.flags & regexp_flag::HasIndices != 0 && !spend(m, groups * 5) { return match_status::Limit; }
            m.caps[0] = index; m.caps[1] = pos;
            return match_status::Matched;
        }
        if !rewind(m, 0) { return match_status::Limit; }
        if sticky || index == len { return match_status::None; }
        index = advance_index(input, index, unicode);
    }
    return match_status::None;
}
