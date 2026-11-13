import std;
import math;
import os;
import .source_vm;
import .realm;
import .array_methods;
import .array_callbacks;
import .array_copy;
import .function_calls;

namespace source;

// Date objects (light-cljs-940; decisions 2 to 6 of docs/javascript-coverage.md;
// ECMAScript 21.4; the Pants engine's vm/date.rs, vm/native.rs and
// builtin.rs; QuickJS's js_date_constructor, js_Date_UTC, get_date_field,
// set_date_field, js_date_toJSON and js_date_Symbol_toPrimitive).
//
// A Date is one node_kind::Date row (decision 2: no new table), an ordinary
// object whose internal slot [[DateValue]] is its `value`, a Number that is
// NaN or an integral time value TimeClip accepts (+0, never -0). It keeps
// its own properties in a shape or records, as a Map does, and no element
// run. Setting the slot is not a property write: a frozen Date's time value
// changes; one in a row the job did not allocate faults NativeBoundary
// before it changes (decision 3). A Date the form bridge made from a
// compiler instant (light-cljs-963) keeps that instant's occurrence in
// `origin` and its time value in `namespace_value` (Null for any other
// Date), so that export sends it back as the compiler's form only while
// the two time values are equal (form_bridge.goose).
//
// Time arithmetic (MakeTime, MakeDay, MakeDate, TimeClip, the fields of a
// time value) is plain Goose over f64 and i64 in the specification's
// floating-point order, the Pants engine's: each product and sum is its own
// rounding (no fused multiply-add), MakeDay floors the month's whole years
// exactly and counts Gregorian days in integers, converted to a Number once.
// A year beyond ±2.5e16 has no exact i64 day count here: MakeDay then answers
// an approximation and DateValue (fields_time) checks that the result is
// outside TimeClip's range whatever the rounding; a combination of fields
// that could cancel back into range fails by name (decision 1).
//
// The clock and the time zone are inputs of the embedder (decision 3, as the
// Pants engine's Host has them, rust/src/host.rs): `date_clock` says
// whether a clock is supplied, a thread program's global that its spawns
// inherit (Goose copies globals into a spawned worker). The compiler
// session supplies the system clock (clock_mode::System, os time_ns), as the
// V8 build has one; the conformance runner supplies none, as the Pants
// engine's command line does, so Date.now(), new Date() and Date() throw a
// catchable RangeError; tests use a fixed clock (clock_mode::Fixed,
// date_clock_time). Reading the clock is a leaf input, not job state:
// nothing for snapshots or reclamation. The time zone is UTC on every Goose
// path (the Pants engine's default), so local time is UTC time, LocalTime
// and UTC are the identity, and getTimezoneOffset is 0; named zones are a
// follow-up.
//
// Built-ins (decision 5). Plain functions, no user code: Date.now, getTime,
// valueOf, the getters, getTimezoneOffset, toISOString, Annex B getYear and
// Date called as a function (the clock, then its string: the string formats
// are light-cljs-941's, a named step). Date native frames (Native.Date, its
// Builtin the payload), since a conversion runs user code:
//   - new Date(...): the one-argument form's ToPrimitive (no hint) of an
//     object, a Date's own time value taken without one; two to seven
//     arguments convert by ToNumber in order (later ones neither convert nor
//     count), the year's 0 to 99 meaning 1900 to 1999; then
//     OrdinaryCreateFromConstructor reads newTarget's `prototype` (light-cljs-887),
//     after every conversion, as the specification orders it.
//   - Date.UTC: ToNumber of its first seven arguments in order.
//   - setTime and the setters (UTC and local, Annex B setYear):
//     thisTimeValue is read first and kept in the frame (`output`), then
//     each given argument converts by ToNumber in order, so a valueOf that
//     changes the receiver does not change the fields the setter starts
//     from; a time value that was NaN answers NaN and writes nothing
//     (setFullYear and setYear start from +0 instead).
//   - toJSON: ToObject, ToPrimitive with hint number (the receiver's own
//     @@toPrimitive runs above), null for a non-finite Number, else
//     Invoke(O, "toISOString").
//   - Date.prototype[@@toPrimitive]: OrdinaryToPrimitive in a Convert frame
//     that starts at its methods (method 1), toString first for "string" and
//     "default", valueOf first for "number"; any other hint a TypeError.
// These plan no conversions (coerce.goose): a plan converts every planned
// position before the body reads the receiver's slot, and the setters read
// it first; the constructor's one-argument form takes a Date's slot without
// ToPrimitive and parses a string; Date.UTC and the constructor convert no
// more than seven arguments, and plans convert every later one too.
//
// A Date frame's state is its fields and operands: the arguments being
// converted are its operands from its operand base (`argument_count` of
// them, `index` the next), converted in place by a Convert frame above it
// (ToNumber) and left as Numbers; `callee` the constructor's newTarget;
// `receiver` the Date a setter writes (toJSON's object, @@toPrimitive's
// receiver); `output` a setter's time value or the constructor's, once
// known. In-job reclamation marks them as any frame's Values
// (reclaim.goose), saved continuations copy them (continuations.goose
// SavedFrame) and snapshot validation checks the phases
// (snapshot.goose date_frame_valid); a budget cut, limit or suspension
// falls between two conversions.

// The clock input (see above).
let clock_mode::None: i64 = 0;
let clock_mode::System: i64 = 1;
let clock_mode::Fixed: i64 = 2;
var date_clock:      i64 = clock_mode::None;
var date_clock_time: f64 = 0;

// A Date frame's phase (its pc): what it awaits on its operand top.
let date_pc::Fields = 0;        // the next argument's ToNumber, in place
let date_pc::Primitive = 1;     // new Date(o): ToPrimitive(o) with no hint, in place
let date_pc::Prototype = 2;     // Get(newTarget, "prototype") from its getter
let date_pc::JsonPrimitive = 3; // toJSON: ToPrimitive(O, number), in place
let date_pc::JsonMethod = 4;    // toJSON: Get(O, "toISOString") from its getter
let date_pc::JsonResult = 5;    // toJSON: the toISOString call's result
let date_pc::Ordinary = 6;      // @@toPrimitive: OrdinaryToPrimitive's result, in place

let MS_PER_DAY:   f64     = 86400000.0;
let MONTH_DAYS:   i64[12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
let BEFORE_MONTH: i64[12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];

// The fields of a time value, in MakeDay and MakeTime order, and its weekday.
struct DateFields { year: i64 = 1970, month: i64 = 0, date: i64 = 1, hours: i64 = 0, minutes: i64 = 0, seconds: i64 = 0, ms: i64 = 0, weekday: i64 = 4 }

inline fn date_nan() -> f64 { return 0.0 / 0.0; }
inline fn floor_div(a: i64, b: i64) -> i64 { let q = a / b; return if a % b != 0 && (a < 0) != (b < 0) { q - 1 } else { q }; }
inline fn leap_year(y: i64) -> bool { return y % 4 == 0 && (y % 100 != 0 || y % 400 == 0); }
// Days from 1970-01-01 to January 1 of year y (DayFromYear).
inline fn day_from_year(y: i64) -> i64 { return 365 * (y - 1970) + floor_div(y - 1969, 4) - floor_div(y - 1901, 100) + floor_div(y - 1601, 400); }

// Whether t is a time value TimeClip gives: NaN, or an integer within
// ±8.64e15 that is not -0 (snapshot validation).
fn valid_time_value(t: f64) -> bool {
    if t != t { return true; }
    if !(abs(t) <= 8.64e15) || trunc(t) != t { return false; }
    return t != 0 || 1.0 / t > 0;
}
// TimeClip (21.4.1.31): +0 for -0, NaN beyond ±8.64e15.
fn time_clip(t: f64) -> f64 {
    if abs(t) <= 8.64e15 { let x: f64 = trunc(t); return x + 0.0; }
    return date_nan();
}
// MakeTime (21.4.1.27): each product and sum rounded on its own.
fn make_time(hour: f64, minute: f64, second: f64, ms: f64) -> f64 {
    if !is_finite(hour) || !is_finite(minute) || !is_finite(second) || !is_finite(ms) { return date_nan(); }
    let h:   f64 = trunc(hour) * 3600000.0;
    let m:   f64 = trunc(minute) * 60000.0;
    let hm:  f64 = h + m;
    let s:   f64 = trunc(second) * 1000.0;
    let hms: f64 = hm + s;
    return hms + trunc(ms);
}
// MakeDay (21.4.1.28), the Pants engine's make_day: the month's whole
// years floored exactly (an i64 quotient, or for a month beyond ±2^63 the
// Number quotient, whose floor cannot change the rounded year), the month
// within the year by the exact remainder, and the Gregorian day count in
// integers converted to a Number once. Answers whether the day is exact: a
// year beyond ±2.5e16 (365 times it nears 2^63) gets an approximation (see fields_time).
fn make_day(year: f64, month: f64, date: f64) -> f64, bool {
    if !is_finite(year) || !is_finite(month) || !is_finite(date) { return date_nan(), true; }
    let m: f64 = trunc(month);
    var whole: f64 = 0;
    if m >= -9223372036854775808.0 && m < 9223372036854775808.0 { let q: i64 = floor_div(m as i64, 12); whole = q; } else { whole = m / 12.0; }
    let y: f64 = trunc(year) + whole;
    // The Pants engine's i128 day count overflows from about ±4.66e35.
    if !(abs(y) < 4.661402e35) { return date_nan(), true; }
    var r: f64 = m % 12.0;
    if r < 0 { r += 12.0; }
    let mi: i64 = r as i64;
    let d:  f64 = trunc(date);
    if abs(y) < 25000000000000000.0 {
        let yi:    i64 = y as i64;
        let start: i64 = day_from_year(yi) + BEFORE_MONTH[mi] + (if leap_year(yi) && mi >= 2 { 1 } else { 0 });
        let s:     f64 = start;
        let sd:    f64 = s + d;
        return sd - 1.0, true;
    }
    let approx: f64 = (y - 1970.0) * 365.2425 + 30.0 * r;
    return approx + d - 1.0, false;
}
// MakeDate (21.4.1.29).
fn make_date(day: f64, time: f64) -> f64 {
    if !is_finite(day) || !is_finite(time) { return date_nan(); }
    let ms: f64 = day * MS_PER_DAY;
    let t:  f64 = ms + time;
    return if is_finite(t) { t } else { date_nan() };
}
// MakeDate(MakeDay(year, month, date), MakeTime(hours, minutes, seconds,
// ms)) of seven Numbers, before TimeClip: true and the time value, or false
// when MakeDay's day was approximate and the sum could lie in TimeClip's
// range (the caller fails by name).
fn fields_time(n: const f64[:]) -> bool, f64 {
    let day, exact = make_day(n[0], n[1], n[2]);
    let time = make_time(n[3], n[4], n[5], n[6]);
    let t = make_date(day, time);
    if exact || !is_finite(t) { return true, t; }
    // The approximation's error is far below 2^-40 of the day's milliseconds.
    let scale: f64 = abs(day * MS_PER_DAY);
    if abs(t) > 8.64e15 + scale * 9.094947017729282e-13 { return true, t; }
    return false, t;
}
// The fields of TimeClipped time value t (LocalTime is the identity under
// UTC), the Pants engine's calendar_fields: false for NaN.
fn date_fields(t: f64) -> bool, DateFields {
    if !is_finite(t) || abs(t) > 8.64e15 + MS_PER_DAY { return false, DateFields {}; }
    let ms: i64 = t as i64;
    let days = floor_div(ms, 86400000); let within = ms - days * 86400000;
    var year: i64 = -300000; var upper: i64 = 300000;
    while upper - year > 1 { let middle = year + (upper - year) / 2; if day_from_year(middle) <= days { year = middle; } else { upper = middle; } }
    var day = days - day_from_year(year); var month: i64 = 0;
    while month < 11 {
        let length = MONTH_DAYS[month] + (if month == 1 && leap_year(year) { 1 } else { 0 });
        if day < length { break; }
        day -= length; month += 1;
    }
    var weekday = (days + 4) % 7; if weekday < 0 { weekday += 7; }
    return true, DateFields { year: year, month: month, date: day + 1, hours: within / 3600000, minutes: within / 60000 % 60, seconds: within / 1000 % 60, ms: within % 1000, weekday: weekday };
}
// The fields as MakeDay and MakeTime's seven Numbers.
fn field_numbers(f: DateFields) -> f64[7] {
    let y: f64 = f.year; let mo: f64 = f.month; let d: f64 = f.date; let h: f64 = f.hours; let mi: f64 = f.minutes; let s: f64 = f.seconds; let ms: f64 = f.ms;
    return [y, mo, d, h, mi, s, ms];
}

// Whether v is a Date (has [[DateValue]]).
fn is_date(heap: const Heap&, v: Value) -> bool { return v.is_object() && v.id() < heap.nodes.len && node(heap, v.id()).kind == node_kind::Date; }
// Whether node n is a Date whose `valueOf` and `toISOString` reads reach
// %Date.prototype%'s, which no job changes (decision 3): no own property and
// the realm's prototype. The form operations and the printer, which the V8
// build runs through those methods and Goose runs without JavaScript, read
// its slot only then; another Date fails by name there.
fn intrinsic_date(n: const Node&) -> bool { return n.kind == node_kind::Date && n.key_count == 0 && !n.initialized && same_bits(n.proto, object(date_prototype_row())); }
// thisTimeValue(receiver): the time value of a Date, else a TypeError.
fn this_time_value(job: Job&, receiver: Value) -> bool, f64 {
    if !is_date(job.heap, receiver) { type_error(job, "not a Date object"); return false, 0; }
    return true, node(job.heap, receiver.id()).value.num();
}
// A new Date of time value tv (TimeClipped) with prototype `proto`.
fn new_date(job: Job&, tv: f64, proto: Value) -> Value {
    return allocate(job.heap, Node { kind: node_kind::Date, value: number(tv), proto: if proto.is_object() { proto } else { object(date_prototype_row()) } });
}
// Set Date o's time value to tv (TimeClipped); false at an image row.
fn set_date_value(job: Job&, o: Value, tv: f64) -> bool {
    if !writable(job, o.id()) { image_write(job, Value.Undefined, "a Date's time value"); return false; }
    node_mut(job.heap, o.id()).value = number(tv);
    return true;
}
// The clock's time value (Date.now, new Date()), or a RangeError when the
// embedder supplies no clock.
fn clock_time(job: Job&) -> bool, f64 {
    if date_clock == clock_mode::System { let ms: i64 = time_ns() / 1000000; let t: f64 = ms; return true, time_clip(t); }
    if date_clock == clock_mode::Fixed { return true, time_clip(date_clock_time); }
    range_error(job, "no host clock supplied");
    return false, 0;
}

// The field a getter reads (DateFields order, 7 the weekday), -1 for none.
fn getter_field(b: Builtin) -> i64 {
    return match b {
        DatePrototypeGetFullYear, DatePrototypeGetUTCFullYear, DatePrototypeGetYear => 0, DatePrototypeGetMonth, DatePrototypeGetUTCMonth => 1,
        DatePrototypeGetDate, DatePrototypeGetUTCDate => 2, DatePrototypeGetHours, DatePrototypeGetUTCHours => 3,
        DatePrototypeGetMinutes, DatePrototypeGetUTCMinutes => 4, DatePrototypeGetSeconds, DatePrototypeGetUTCSeconds => 5,
        DatePrototypeGetMilliseconds, DatePrototypeGetUTCMilliseconds => 6, DatePrototypeGetDay, DatePrototypeGetUTCDay => 7,
        _ => -1,
    };
}
// A setter's first field (DateFields order): -1 for setTime, -2 for a
// built-in that is no setter.
fn setter_first(b: Builtin) -> i64 {
    return match b {
        DatePrototypeSetTime => -1,
        DatePrototypeSetMilliseconds, DatePrototypeSetUTCMilliseconds => 6, DatePrototypeSetSeconds, DatePrototypeSetUTCSeconds => 5,
        DatePrototypeSetMinutes, DatePrototypeSetUTCMinutes => 4, DatePrototypeSetHours, DatePrototypeSetUTCHours => 3,
        DatePrototypeSetDate, DatePrototypeSetUTCDate => 2, DatePrototypeSetMonth, DatePrototypeSetUTCMonth => 1,
        DatePrototypeSetFullYear, DatePrototypeSetUTCFullYear, DatePrototypeSetYear => 0,
        _ => -2,
    };
}
// How many arguments a setter converts at most: the fields from its first
// to the next it does not set (milliseconds, the date and setYear take one).
fn setter_count(b: Builtin) -> i64 {
    let first = setter_first(b);
    if first < 0 || b == Builtin.DatePrototypeSetYear { return 1; }
    return if first <= 2 { 3 - first } else { 7 - first };
}
inline fn date_setter(b: Builtin) -> bool { return setter_first(b) >= -1; }
// Whether built-in b runs as a Date native frame (native_start).
fn date_native(b: Builtin) -> bool { return b == Builtin.DateUTC || b == Builtin.DatePrototypeToJSON || b == Builtin.DatePrototypeToPrimitive || date_setter(b); }

// The plain Date built-ins (see above).
fn date_builtin(job: Job&, b: Builtin, receiver: Value, args: const Value[:]) -> Value {
    let heap .= job.heap;
    if b == Builtin.Date {
        // Called as a function: ToDateString of the clock's time.
        let ok, t = clock_time(job);
        if !ok { return Value.Undefined; }
        return unsupported_step(job, b, "the current time's string (Date.prototype.toString, light-cljs-941)");
    }
    if b == Builtin.DateNow { let ok, t = clock_time(job); return if ok { number(t) } else { Value.Undefined }; }
    let ok, t = this_time_value(job, receiver);
    if !ok { return Value.Undefined; }
    if b == Builtin.DatePrototypeGetTime || b == Builtin.DatePrototypeValueOf { return number(t); }
    if b == Builtin.DatePrototypeGetTimezoneOffset { return number(if t != t { t } else { 0.0 }); }
    if b == Builtin.DatePrototypeToISOString { return iso_string(job, t); }
    let field = getter_field(b);
    if field < 0 { heap.fault = Fault.InvalidState; return Value.Undefined; }
    let valid, f = date_fields(t);
    if !valid { return number(date_nan()); }
    let n: i64 = match field { 0 => f.year, 1 => f.month, 2 => f.date, 3 => f.hours, 4 => f.minutes, 5 => f.seconds, 6 => f.ms, _ => f.weekday };
    let v: f64 = if b == Builtin.DatePrototypeGetYear { n - 1900 } else { n };
    return number(v);
}
// Append n's decimal digits, zero-padded to `width`, to `out`.
fn append_padded(out: u8[>..]&, n: i64, width: i64) {
    var digits: u8[>..] = []; var x = n;
    while x > 0 || digits.len == 0 { digits.push((48 + x % 10) as u8); x = x / 10; }
    var missing = width - digits.len; while missing > 0 { out.push(48); missing -= 1; }
    var i = digits.len - 1; while i >= 0 { out.push(digits[i]); i -= 1; }
}
// Date.prototype.toISOString (21.4.4.36) of time value t: the extended
// format, a year outside 0 to 9999 as a sign and six digits; a RangeError
// for NaN.
fn iso_string(job: Job&, t: f64) -> Value {
    let valid, f = date_fields(t);
    if !valid { range_error(job, "Invalid time value"); return Value.Undefined; }
    var out: u8[>..] = [];
    if f.year >= 0 && f.year <= 9999 { append_padded(out, f.year, 4); }
    else { out.push(if f.year < 0 { 45 } else { 43 }); append_padded(out, if f.year < 0 { -f.year } else { f.year }, 6); }
    out.push(45); append_padded(out, f.month + 1, 2); out.push(45); append_padded(out, f.date, 2);
    out.push(84); append_padded(out, f.hours, 2); out.push(58); append_padded(out, f.minutes, 2); out.push(58); append_padded(out, f.seconds, 2);
    out.push(46); append_padded(out, f.ms, 3); out.push(90);
    if !charge(job.heap, out.len) { return Value.Undefined; }
    return text(job.heap, out);
}

// ---- Date native frames (see above)

// The answer of a Date built-in: the frame at `at` finishes with it, or
// with no frame it goes at the call's operand base.
fn date_answer(job: Job&, at: i64, base: i64, v: Value) {
    if job.heap.fault != Fault.None { return; }
    if at >= 0 { finish_native(job, job.frames[at], v); return; }
    job.operands.resize(base);
    if job.frames.len == 0 { deliver(job, v); } else { push_operand(job, v); }
}
// Push a Date frame for built-in b at operand base `base` in phase pc, the
// operands from `base` its `count` values: its row, or -1 at a limit.
fn date_frame(job: Job&, b: Builtin, base: i64, pc: i64, count: i64, callee: Value, receiver: Value, output: Value) -> i64 {
    if job.frames.len >= job.heap.limits.frames { job.heap.fault = Fault.StackLimit; return -1; }
    job.frames.push(Frame { function: -1, pc: pc, local_base: job.locals.len, operand_base: base, callee: callee, receiver: receiver, output: output, argument_count: count, native: Native.Date { which: b } });
    return job.frames.len - 1;
}
// Place `values` as the operands from `base` (they may lie above it).
fn date_operands(job: Job&, base: i64, values: const Value[:]) -> bool {
    if values.len > job.heap.limits.stack - job.locals.len - base { job.heap.fault = Fault.StackLimit; return false; }
    var copied: Value[>..] = []; copied.append(values);
    job.operands.resize(base); job.operands.append(copied);
    return true;
}
// Convert the operands from `base + from` to `base + count` by ToNumber in
// place: a primitive here (a symbol's TypeError ends it), an object in a
// Convert frame started above the Date frame at `at` (its index left at
// that operand). True when every operand is a Number.
fn convert_fields(job: Job&, b: Builtin, at: i64, base: i64, count: i64, from: i64) -> bool {
    let heap .= job.heap;
    var i = from;
    while i < count {
        let v = form_value(heap, job.operands[base + i]);
        if heap.fault != Fault.None { return false; }
        if v.is_object() && convertible(heap, v) {
            if at < 0 { heap.fault = Fault.InvalidState; return false; }
            job.frames[at].index = i;
            start_convert(job, Hint.Number, true, 1, job.operands.len - (base + i));
            return false;
        }
        let n = primitive_number(job, b, v);
        if heap.fault != Fault.None { return false; }
        job.operands[base + i] = number(n);
        i += 1;
    }
    if at >= 0 { job.frames[at].index = count; }
    return true;
}
// Whether one of `values` is an object a conversion would run code for.
fn any_convertible(heap: const Heap&, values: const Value[:]) -> bool { for v in values { if convertible(heap, v) { return true; } } return false; }
// The seven fields of Date.UTC and the constructor from the `count`
// converted operands at `base` (a missing month 0, date 1, the rest 0), the
// year's 0 to 99 mapped to 1900 to 1999 (MakeFullYear).
fn constructor_fields(job: const Job&, base: i64, count: i64) -> f64[7] {
    var n: f64[7] = [date_nan(), 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
    for i in count { n[i] = job.operands[base + i].num(); }
    if is_finite(n[0]) { let y: f64 = trunc(n[0]); if y >= 0 && y <= 99 { n[0] = 1900.0 + y; } }
    return n;
}
// The time value of seven fields, TimeClipped; a cancellation the
// approximate day count cannot decide fails by name.
fn clipped_fields(job: Job&, b: Builtin, n: const f64[:]) -> bool, f64 {
    let exact, t = fields_time(n);
    if !exact { unsupported_step(job, b, "a year beyond 2.5e16 whose time fields cancel"); return false, 0; }
    return true, time_clip(t);
}

// Start Date native built-in b (date_native), called with `receiver` and
// `args`, its operand base `base` (native_start's contract).
fn start_date(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    let heap .= job.heap;
    if b == Builtin.DatePrototypeToPrimitive { start_date_to_primitive(job, receiver, arg(args, 0), base); return; }
    if b == Builtin.DatePrototypeToJSON {
        let o = to_object(job, receiver);
        if heap.fault != Fault.None { return; }
        if !convertible(heap, o) { unsupported_step(job, b, "ToPrimitive of a light-cljs runtime value"); return; }
        if !date_operands(job, base, [o]) { return; }
        if date_frame(job, b, base, date_pc::JsonPrimitive, 1, Value.Undefined, o, Value.Undefined) < 0 { return; }
        start_convert(job, Hint.Number, false, 1, 1);
        return;
    }
    var count: i64 = min(args.len, 7);
    var output = Value.Undefined;
    if date_setter(b) {
        let ok, t = this_time_value(job, receiver);
        if !ok { return; }
        count = min(args.len, setter_count(b)); output = number(t);
    } else if count == 0 { date_answer(job, -1, base, number(date_nan())); return; }
    if !date_operands(job, base, args[..count]) { return; }
    var at: i64 = -1;
    if any_convertible(heap, job.operands[base..]) { at = date_frame(job, b, base, date_pc::Fields, count, Value.Undefined, if b == Builtin.DateUTC { Value.Undefined } else { receiver }, output); if at < 0 { return; } }
    if !convert_fields(job, b, at, base, count, 0) { return; }
    date_fields_done(job, b, at, base, count, receiver, output);
}
// Every argument of Date.UTC, a setter or the constructor (b Date, with
// newTarget `nt` and function f) is a Number at `base`: finish.
fn date_fields_done(job: Job&, b: Builtin, at: i64, base: i64, count: i64, receiver: Value, output: Value) {
    let heap .= job.heap;
    if b == Builtin.DateUTC {
        let n = constructor_fields(job, base, count);
        let ok, t = clipped_fields(job, b, n);
        if ok { date_answer(job, at, base, number(t)); }
        return;
    }
    if b == Builtin.Date {
        let n = constructor_fields(job, base, count);
        let ok, t = clipped_fields(job, b, n);
        if ok { date_prototype_step(job, at, base, job.frames[at].callee, t); }
        return;
    }
    // A setter: its time value read before the conversions.
    let first = setter_first(b);
    let arg0: f64 = if count > 0 { job.operands[base].num() } else { date_nan() };
    if first < 0 {
        let tv = time_clip(arg0);
        if set_date_value(job, receiver, tv) { date_answer(job, at, base, number(tv)); }
        return;
    }
    var t = output.num();
    if t != t && first == 0 { t = 0; }
    if t != t { date_answer(job, at, base, number(t)); return; }
    let valid, f = date_fields(t);
    if !valid { heap.fault = Fault.InvalidState; return; }
    var n = field_numbers(f);
    n[first] = arg0;
    for i in 1..count { n[first + i] = job.operands[base + i].num(); }
    // Annex B setYear (B.2.3.2): MakeFullYear of its argument.
    if b == Builtin.DatePrototypeSetYear && is_finite(arg0) { let y: f64 = trunc(arg0); n[0] = if y >= 0 && y <= 99 { 1900.0 + y } else { y }; }
    let ok, tv = clipped_fields(job, b, n);
    if !ok { return; }
    if set_date_value(job, receiver, tv) { date_answer(job, at, base, number(tv)); }
}
// new Date(...) with newTarget nt, the Date function f and `args`, its
// operand base `base` (construct_builtin's contract).
fn start_date_construct(job: Job&, f: Value, nt: Value, arguments: const Value[:], base: i64) {
    let heap .= job.heap; let b = Builtin.Date;
    let callee = if same_value(heap, nt, f) { Value.Undefined } else { nt };
    // The arguments may lie on the operands this call cuts: at most seven count.
    var args: Value[>..] = []; args.append(arguments[..min(arguments.len, 7)]); let single = arguments.len == 1;
    if args.len == 0 {
        let ok, t = clock_time(job);
        if ok { date_prototype_step(job, -1, base, callee, t); }
        return;
    }
    if single {
        let v = form_value(heap, args[0]);
        if heap.fault != Fault.None { return; }
        // A Date's time value, without ToPrimitive.
        if is_date(heap, v) { date_prototype_step(job, -1, base, callee, time_clip(node(heap, v.id()).value.num())); return; }
        if v.is_object() && convertible(heap, v) {
            if !date_operands(job, base, [v]) { return; }
            if date_frame(job, b, base, date_pc::Primitive, 1, callee, Value.Undefined, Value.Undefined) < 0 { return; }
            start_convert(job, Hint.Default, false, 1, 1);
            return;
        }
        let ok, t = date_primitive_time(job, v);
        if ok { date_prototype_step(job, -1, base, callee, t); }
        return;
    }
    let count: i64 = min(args.len, 7);
    if !date_operands(job, base, args[..count]) { return; }
    var at: i64 = -1;
    if any_convertible(heap, job.operands[base..]) { at = date_frame(job, b, base, date_pc::Fields, count, callee, Value.Undefined, Value.Undefined); if at < 0 { return; } }
    if !convert_fields(job, b, at, base, count, 0) { return; }
    if at >= 0 { date_fields_done(job, b, at, base, count, Value.Undefined, Value.Undefined); return; }
    let n = constructor_fields(job, base, count);
    let ok, t = clipped_fields(job, b, n);
    if ok { date_prototype_step(job, -1, base, callee, t); }
}
// The time value of new Date(p) for primitive p: a string is Date.parse's
// (light-cljs-941, a named step until then), anything else its ToNumber,
// TimeClipped.
fn date_primitive_time(job: Job&, p: Value) -> bool, f64 {
    if p.is_string() { unsupported_step(job, Builtin.Date, "a string argument (Date.parse, light-cljs-941)"); return false, 0; }
    let n = primitive_number(job, Builtin.Date, p);
    if job.heap.fault != Fault.None { return false, 0; }
    return true, time_clip(n);
}
// OrdinaryCreateFromConstructor(newTarget, %Date.prototype%) after the
// time value t: the realm's prototype when newTarget is Date itself
// (`callee` undefined), else newTarget's `prototype`, whose getter runs
// above a Date frame (pushed now if there is none at `at`).
fn date_prototype_step(job: Job&, at: i64, base: i64, callee: Value, t: f64) {
    let heap .= job.heap; let b = Builtin.Date;
    if !callee.is_object() { date_finish_construct(job, at, base, Value.Undefined, t); return; }
    let proto, getter = closure_prototype(job, callee);
    if heap.fault != Fault.None { return; }
    if !getter.is_object() { date_finish_construct(job, at, base, proto, t); return; }
    var f = at;
    if f < 0 { job.operands.resize(base); f = date_frame(job, b, base, date_pc::Prototype, 0, callee, Value.Undefined, number(t)); if f < 0 { return; } }
    else { job.operands.resize(base); job.frames[f].pc = date_pc::Prototype; job.frames[f].output = number(t); job.frames[f].argument_count = 0; job.frames[f].index = 0; }
    run_getter(job, b, getter, callee);
}
fn date_finish_construct(job: Job&, at: i64, base: i64, proto: Value, t: f64) {
    let d = new_date(job, t, proto);
    if job.heap.fault == Fault.None { date_answer(job, at, base, d); }
}
// Date.prototype[@@toPrimitive](hint) (21.4.4.45) with `receiver`: the
// ordinary conversion in a Convert frame above a Date frame.
fn start_date_to_primitive(job: Job&, receiver: Value, hint: Value, base: i64) {
    let heap .= job.heap; let b = Builtin.DatePrototypeToPrimitive;
    if !receiver.is_object() { type_error(job, "not an object"); return; }
    var h = Hint.Number;
    if hint.is_string() && (key_is(heap, hint, "string") || key_is(heap, hint, "default")) { h = Hint.String; }
    else if !hint.is_string() || !key_is(heap, hint, "number") { type_error(job, "invalid hint"); return; }
    if heap.fault != Fault.None { return; }
    if !convertible(heap, receiver) { unsupported_step(job, b, "OrdinaryToPrimitive of a light-cljs runtime value"); return; }
    if !date_operands(job, base, [receiver]) { return; }
    if date_frame(job, b, base, date_pc::Ordinary, 1, Value.Undefined, receiver, Value.Undefined) < 0 { return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.frames.push(Frame { function: -1, pc: convert_pc::Method, local_base: job.locals.len, operand_base: job.operands.len, callee: Value.Undefined, receiver: Value.Undefined, argument_count: 1, native: Native.Convert { hint: h, finish: false, count: 1, method: 1 } });
}
// One step of the Date frame at the top.
fn date_step(job: Job&, frame: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1; let base = frame.operand_base;
    match frame.pc {
        date_pc::Fields => {
            if job.operands.len != base + frame.argument_count || frame.index < 0 || frame.index > frame.argument_count { heap.fault = Fault.InvalidState; return; }
            if !convert_fields(job, b, at, base, frame.argument_count, frame.index) { return; }
            date_fields_done(job, b, at, base, frame.argument_count, frame.receiver, frame.output);
        },
        date_pc::Primitive => {
            if job.operands.len != base + 1 { heap.fault = Fault.InvalidState; return; }
            let p = form_value(heap, job.operands[base]);
            if heap.fault != Fault.None { return; }
            if p.is_object() { heap.fault = Fault.InvalidState; return; }
            let ok, t = date_primitive_time(job, p);
            if ok { date_prototype_step(job, at, base, frame.callee, t); }
        },
        date_pc::Prototype => {
            if job.operands.len != base + 1 || !frame.output.is_number() { heap.fault = Fault.InvalidState; return; }
            let p = job.operands.pop();
            date_finish_construct(job, at, base, if p.is_object() { p } else { Value.Undefined }, frame.output.num());
        },
        date_pc::JsonPrimitive => {
            if job.operands.len != base + 1 { heap.fault = Fault.InvalidState; return; }
            let tv = form_value(heap, job.operands.pop());
            if heap.fault != Fault.None { return; }
            if tv.is_number() && !is_finite(tv.num()) { finish_native(job, frame, Value.Null); return; }
            job.frames[at].pc = date_pc::JsonMethod;
            get_named(job, b, frame.receiver, "toISOString");
        },
        date_pc::JsonMethod => {
            if job.operands.len != base + 1 { heap.fault = Fault.InvalidState; return; }
            let m = job.operands.pop();
            if !is_callable(heap, m) { type_error(job, "object needs toISOString method"); return; }
            job.frames[at].pc = date_pc::JsonResult;
            call_accessor(job, m, frame.receiver, []);
        },
        date_pc::JsonResult, date_pc::Ordinary => {
            if job.operands.len != base + 1 { heap.fault = Fault.InvalidState; return; }
            finish_native(job, frame, job.operands.pop());
        },
        _ => { heap.fault = Fault.InvalidState; },
    }
}
