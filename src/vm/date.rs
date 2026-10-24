//! UTC time-value core (clj-quickjs-179). Fresh Rust implementation of
//! ECMA-262 MakeDay/MakeTime/MakeDate/TimeClip; QuickJS's time_clip,
//! set_date_fields and js_Date_UTC (research/quickjs/quickjs.c, revision
//! 7d1c13c5d8f8) are semantic references, not ported code.
//!
//! Clock reads are unrecorded host leaf effects. One generated rule-table zone
//! is selected per runtime (UTC without a host). Never read the OS clock or
//! zone here. Snapshot coverage is limited; uncovered names are rejected.
//! UTC decomposition and ISO formatting
//! are implemented below; parsing and other formats remain separate pellets.
use crate::heap::{Heap, HeapObject};
use crate::ids::TaskId;
use crate::object::OrdinaryObject;
use crate::value::Value;
use crate::vm::dispatch::{Fault, to_number};

pub(crate) const MS_PER_DAY: f64 = 86_400_000.0;

/// UTC calendar fields shared by ISO formatting and UTC getters/setters. Month is
/// zero-based and weekday counts from Sunday (zero), as in ECMA-262.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct UtcFields {
    pub(crate) year: i32,
    pub(crate) month: u8,
    pub(crate) date: u8,
    pub(crate) hours: u8,
    pub(crate) minutes: u8,
    pub(crate) seconds: u8,
    pub(crate) milliseconds: u16,
    pub(crate) weekday: u8,
}

pub(crate) const UTC_GETTERS: [&str; 8] = [
    "getUTCFullYear",
    "getUTCMonth",
    "getUTCDate",
    "getUTCHours",
    "getUTCMinutes",
    "getUTCSeconds",
    "getUTCMilliseconds",
    "getUTCDay",
];

pub(crate) const LOCAL_GETTERS: [&str; 8] = [
    "getFullYear",
    "getMonth",
    "getDate",
    "getHours",
    "getMinutes",
    "getSeconds",
    "getMilliseconds",
    "getDay",
];
pub(crate) const LOCAL_SETTERS: [(&str, u32); 7] = [
    ("setFullYear", 3),
    ("setMonth", 2),
    ("setDate", 1),
    ("setHours", 4),
    ("setMinutes", 3),
    ("setSeconds", 2),
    ("setMilliseconds", 1),
];

pub(crate) fn zone(heap: &Heap, host: &mut dyn crate::Host) -> crate::host::TimeZone {
    *heap
        .date_zone
        .get_or_init(|| crate::host::call_time_zone(host))
}

pub(crate) fn now(host: &mut dyn crate::Host) -> Result<f64, Fault> {
    crate::host::call_current_time(host)
        .map(time_clip)
        .map_err(Fault::RangeError)
}

pub(crate) fn get_local_field(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    field: u8,
    zone: crate::host::TimeZone,
) -> Result<Value, Fault> {
    let time = this_time_value(heap, task, this)?;
    let number = if time.is_nan() {
        f64::NAN
    } else if field == 8 {
        -f64::from(zone.offset_seconds(time)) / 60.0 + 0.0
    } else {
        calendar_fields(zone.local_time_millis(time)).map_or(f64::NAN, |fields| {
            if field == 7 {
                f64::from(fields.weekday)
            } else {
                fields.numbers()[field as usize]
            }
        })
    };
    Ok(Value::Number(number))
}

/// Indexed by the first field replaced, in MakeDay/MakeTime field order.
pub(crate) const UTC_SETTERS: [(&str, u32); 7] = [
    ("setUTCFullYear", 3),
    ("setUTCMonth", 2),
    ("setUTCDate", 1),
    ("setUTCHours", 4),
    ("setUTCMinutes", 3),
    ("setUTCSeconds", 2),
    ("setUTCMilliseconds", 1),
];

impl UtcFields {
    fn numbers(&self) -> [f64; 7] {
        [
            f64::from(self.year),
            f64::from(self.month),
            f64::from(self.date),
            f64::from(self.hours),
            f64::from(self.minutes),
            f64::from(self.seconds),
            f64::from(self.milliseconds),
        ]
    }
}

pub(crate) fn get_utc_field(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    field: u8,
) -> Result<Value, Fault> {
    let number = utc_fields(this_time_value(heap, task, this)?).map_or(f64::NAN, |fields| {
        if field == 7 {
            f64::from(fields.weekday)
        } else {
            fields.numbers()[field as usize]
        }
    });
    Ok(Value::Number(number))
}

/// `time` is captured before the first observable argument conversion. All
/// arguments have completed ordered ToNumber in the traced native frame.
pub(crate) fn set_utc_field(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    first: u8,
    time: f64,
    args: &[Value],
) -> Result<Value, Fault> {
    set_field(heap, task, this, first, time, args, None)
}

pub(crate) fn set_local_field(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    first: u8,
    time: f64,
    args: &[Value],
    zone: crate::host::TimeZone,
) -> Result<Value, Fault> {
    set_field(heap, task, this, first, time, args, Some(zone))
}

fn set_field(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    first: u8,
    time: f64,
    args: &[Value],
    zone: Option<crate::host::TimeZone>,
) -> Result<Value, Fault> {
    let time = if time.is_nan() {
        time
    } else {
        zone.map_or(time, |z| z.local_time_millis(time))
    };
    let time = if first == 0 && time.is_nan() {
        0.0
    } else {
        time
    };
    let Some(fields) = calendar_fields(time) else {
        // Invalid original times return NaN without overwriting changes that
        // argument callbacks may have made to the receiver's slot.
        return Ok(Value::Number(f64::NAN));
    };
    let mut numbers = fields.numbers();
    for (out, arg) in numbers[first as usize..].iter_mut().zip(args) {
        *out = to_number(arg)?;
    }
    let [year, month, date, hour, minute, second, ms] = numbers;
    let time = make_date(
        make_day(year, month, date),
        make_time(hour, minute, second, ms),
    );
    set_time(
        heap,
        task,
        this,
        zone.map_or(time, |z| z.utc_time_millis(time)),
    )
}

/// Decompose a TimeClipped value without host inputs or floating calendar
/// arithmetic. Invalid dates have no fields. All valid milliseconds fit i64
/// exactly; Euclidean division keeps negative days and sub-day fields correct.
pub(crate) fn utc_fields(time: f64) -> Option<UtcFields> {
    if !time.is_finite() || time.abs() > 8.64e15 {
        return None;
    }
    calendar_fields(time)
}

fn calendar_fields(time: f64) -> Option<UtcFields> {
    // LocalTime may extend one offset beyond TimeClip at the endpoints.
    if !time.is_finite() || time.abs() > 8.64e15 + MS_PER_DAY {
        return None;
    }
    debug_assert_eq!(time, time.trunc(), "Date values must be TimeClipped");
    let time = time as i64;
    let days = time.div_euclid(86_400_000);
    let within_day = time.rem_euclid(86_400_000);
    let start_of_year = |year: i64| {
        365 * (year - 1970) + (year - 1969).div_euclid(4) - (year - 1901).div_euclid(100)
            + (year - 1601).div_euclid(400)
    };
    // The ±100,000,000-day clip range lies strictly within these year bounds.
    // Binary search takes at most 20 steps, then at most 11 month steps, so
    // decomposition/formatting is a bounded leaf operation with no callbacks.
    let (mut year, mut upper) = (-300_000_i64, 300_000_i64);
    while upper - year > 1 {
        let middle = year + (upper - year) / 2;
        if start_of_year(middle) <= days {
            year = middle;
        } else {
            upper = middle;
        }
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let mut day_of_year = days - start_of_year(year);
    let mut month = 0;
    for length in [31, 28 + i64::from(leap), 31, 30, 31, 30, 31, 31, 30, 31, 30] {
        if day_of_year < length {
            break;
        }
        day_of_year -= length;
        month += 1;
    }
    Some(UtcFields {
        year: year as i32,
        month,
        date: (day_of_year + 1) as u8,
        hours: (within_day / 3_600_000) as u8,
        minutes: (within_day / 60_000 % 60) as u8,
        seconds: (within_day / 1_000 % 60) as u8,
        milliseconds: (within_day % 1_000) as u16,
        weekday: (days + 4).rem_euclid(7) as u8,
    })
}

pub(crate) fn to_iso_string(heap: &Heap, task: TaskId, this: &Value) -> Result<Value, Fault> {
    let fields = utc_fields(this_time_value(heap, task, this)?)
        .ok_or_else(|| Fault::RangeError("Invalid time value".into()))?;
    let year = if (0..=9999).contains(&fields.year) {
        format!("{:04}", fields.year)
    } else {
        // Sign plus six digits (width includes the sign), including year -1.
        format!("{:+07}", fields.year)
    };
    Ok(Value::String(
        format!(
            "{year}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            fields.month + 1,
            fields.date,
            fields.hours,
            fields.minutes,
            fields.seconds,
            fields.milliseconds,
        )
        .as_str()
        .into(),
    ))
}

/// Bound the shared leaf StringToNumber scan (including rope flattening).
/// Generic budgeted numeric parsing remains clj-quickjs-265. Observable
/// ToPrimitive completes before this check, and later fields are untouched
/// when the current conversion exceeds the limit.
pub(crate) fn number_argument(value: &Value) -> Result<f64, Fault> {
    const MAX_NUMBER_CONVERSION_UNITS: usize = 4096;
    if let Value::String(text) = value
        && text.len() > MAX_NUMBER_CONVERSION_UNITS
    {
        return Err(Fault::RangeError(format!(
            "Date numeric conversion limit of {MAX_NUMBER_CONVERSION_UNITS} UTF-16 units exceeded"
        )));
    }
    to_number(value)
}

pub(crate) fn time_clip(time: f64) -> f64 {
    if time.abs() <= 8.64e15 {
        time.trunc() + 0.0
    } else {
        f64::NAN
    }
}

pub(crate) fn make_time(hour: f64, minute: f64, second: f64, ms: f64) -> f64 {
    if ![hour, minute, second, ms].iter().all(|n| n.is_finite()) {
        return f64::NAN;
    }
    // These are separate IEEE operations, deliberately neither regrouped nor
    // fused with mul_add: cancellation can make even enormous fields valid.
    ((hour.trunc() * 3_600_000.0 + minute.trunc() * 60_000.0) + second.trunc() * 1_000.0)
        + ms.trunc()
}

pub(crate) fn make_day(year: f64, month: f64, date: f64) -> f64 {
    if ![year, month, date].iter().all(|n| n.is_finite()) {
        return f64::NAN;
    }
    let month = month.trunc();
    // MakeDay floors the mathematical quotient before converting to Number.
    // Floating-point division can round up across that integer boundary.
    const I64_LIMIT: f64 = 9_223_372_036_854_775_808.0;
    let whole_years = if (-I64_LIMIT..I64_LIMIT).contains(&month) {
        (month as i64).div_euclid(12) as f64
    } else {
        // Here the quotient's f64 spacing U is at least 128. In units of U,
        // its fractional part is 0, 1/3 or 2/3 (division by 12 removes only
        // powers of two and a factor of 3). Each rounding midpoint is at least
        // U/6 > 1 away, so flooring cannot change the rounded Number value.
        month / 12.0
    };
    let year = year.trunc() + whole_years;
    const I128_LIMIT: f64 = (1_u128 << 127) as f64;
    if !(-I128_LIMIT..I128_LIMIT).contains(&year) {
        return f64::NAN;
    }
    let year = year as i128;
    let month = ((month % 12.0 + 12.0) % 12.0) as usize;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    const BEFORE_MONTH: [u16; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    // Gregorian counts and month offsets are mathematical integers, converted
    // to Number only once. Euclidean division floors negative years. Do not
    // TimeClip intermediates: later fields can cancel even huge day counts.
    // Reject calendar values beyond our checked integer storage, rather than
    // saturating a cast, overflowing, or returning a rounded calendar count.
    let start = (|| {
        year.checked_sub(1970)?
            .checked_mul(365)?
            .checked_add(year.checked_sub(1969)?.div_euclid(4))?
            .checked_sub(year.checked_sub(1901)?.div_euclid(100))?
            .checked_add(year.checked_sub(1601)?.div_euclid(400))?
            .checked_add(i128::from(BEFORE_MONTH[month]))?
            .checked_add(i128::from(leap && month >= 2))
    })();
    let Some(start) = start else {
        return f64::NAN;
    };
    start as f64 + date.trunc() - 1.0
}

pub(crate) fn make_date(day: f64, time: f64) -> f64 {
    if !day.is_finite() || !time.is_finite() {
        return f64::NAN;
    }
    let time = day * MS_PER_DAY + time;
    if time.is_finite() { time } else { f64::NAN }
}

/// Inputs have already undergone observable ToNumber in argument order.
pub(crate) fn utc(fields: &[Value]) -> Result<Value, Fault> {
    fields_time(fields).map(|time| Value::Number(time_clip(time)))
}

pub(crate) fn fields_time(fields: &[Value]) -> Result<f64, Fault> {
    let mut numbers = [f64::NAN, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
    for (out, value) in numbers.iter_mut().zip(fields) {
        *out = to_number(value)?;
    }
    let [mut year, month, date, hour, minute, second, ms] = numbers;
    if year.is_finite() && (0.0..=99.0).contains(&year.trunc()) {
        year = 1900.0 + year.trunc();
    }
    Ok(make_date(
        make_day(year, month, date),
        make_time(hour, minute, second, ms),
    ))
}

/// Brand inspection reads only an object's own slot and observes task ownership.
pub(crate) fn time_value(heap: &Heap, task: TaskId, this: &Value) -> Result<Option<f64>, Fault> {
    let Value::Object(id) = this else {
        return Ok(None);
    };
    heap.read(*id, |object| match object {
        HeapObject::Plain(o) => {
            if let Some(owner) = o.owner()
                && owner != task
            {
                return Err(Fault::ForeignObject {
                    owner: Some(owner),
                    caller: task,
                });
            }
            Ok(o.date_value())
        }
        _ => Ok(None),
    })
    .unwrap_or(Err(Fault::Internal("Date receiver was reclaimed")))
}

pub(crate) fn this_time_value(heap: &Heap, task: TaskId, this: &Value) -> Result<f64, Fault> {
    time_value(heap, task, this)?
        .ok_or_else(|| Fault::TypeError("Date method requires a Date receiver".into()))
}

/// A Date argument is copied before ToPrimitive; every other argument reaches
/// here after resumable ToPrimitive with the default hint.
pub(crate) fn construct(heap: &Heap, task: TaskId, value: &Value) -> Result<Value, Fault> {
    if matches!(value, Value::String(_)) {
        return Err(Fault::Unsupported(
            "Date string parsing is not supported".into(),
        ));
    }
    let time = time_clip(to_number(value)?);
    let realm =
        crate::realm::intrinsics(heap).ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
    let object = OrdinaryObject::with_prototype(task, realm.date_prototype).with_date_value(time);
    heap.alloc(HeapObject::Plain(object))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))
}

/// Internal-slot mutation is independent of extensibility/frozen properties.
/// The shared intrinsic prototype remains immutable, as with all intrinsics.
pub(crate) fn set_time(heap: &Heap, task: TaskId, this: &Value, time: f64) -> Result<Value, Fault> {
    this_time_value(heap, task, this)?;
    let Value::Object(id) = this else {
        unreachable!()
    };
    let time = time_clip(time);
    heap.with_object_mut(*id, |object| match object {
        HeapObject::Plain(o) if o.is_intrinsic() => {
            Err(Fault::TypeError("realm intrinsics are immutable".into()))
        }
        HeapObject::Plain(o) => {
            o.set_date_value(time);
            Ok(Value::Number(time))
        }
        _ => Err(Fault::Internal("Date receiver lost its slot")),
    })
    .unwrap_or(Err(Fault::Internal("Date receiver was reclaimed")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_utc_decomposition() {
        // Synthetic field expectations; months are zero-based, Sunday is zero.
        for (time, fields) in [
            (0.0, (1970, 0, 1, 0, 0, 0, 0, 4)),
            (-1.0, (1969, 11, 31, 23, 59, 59, 999, 3)),
            (1506098258091.0, (2017, 8, 22, 16, 37, 38, 91, 5)),
            (8.64e15, (275760, 8, 13, 0, 0, 0, 0, 6)),
            (-8.64e15, (-271821, 3, 20, 0, 0, 0, 0, 2)),
            (-62198755200000.0, (-1, 0, 1, 0, 0, 0, 0, 5)),
            (253402300800000.0, (10000, 0, 1, 0, 0, 0, 0, 6)),
        ] {
            let actual = utc_fields(time).unwrap();
            assert_eq!(
                (
                    actual.year,
                    actual.month,
                    actual.date,
                    actual.hours,
                    actual.minutes,
                    actual.seconds,
                    actual.milliseconds,
                    actual.weekday,
                ),
                fields,
                "{time}"
            );
        }
        for time in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            8.64e15 + 1.0,
            -8.64e15 - 1.0,
        ] {
            assert!(utc_fields(time).is_none(), "{time}");
        }
    }

    #[test]
    fn date_utc_calendar_boundaries() {
        // Synthetic calendar checks around every month boundary over two full
        // Gregorian cycles (including year zero), and near the clip limits.
        for year in (-400..400).chain([-271821, -10000, 9999, 10000, 275760]) {
            for month in 0..12 {
                let start = make_date(make_day(f64::from(year), f64::from(month), 1.0), 0.0);
                for time in [start - 1.0, start, start + 1.0] {
                    if time.abs() > 8.64e15 {
                        continue;
                    }
                    let f = utc_fields(time).unwrap();
                    assert_eq!(
                        make_date(
                            make_day(f64::from(f.year), f64::from(f.month), f64::from(f.date)),
                            make_time(
                                f64::from(f.hours),
                                f64::from(f.minutes),
                                f64::from(f.seconds),
                                f64::from(f.milliseconds)
                            ),
                        ),
                        time,
                    );
                    if time == start {
                        assert_eq!((f.year, i32::from(f.month), f.date), (year, month, 1));
                    }
                    let next_week = utc_fields(time + 7.0 * MS_PER_DAY);
                    if let Some(next) = next_week {
                        assert_eq!(f.weekday, next.weekday);
                    }
                }
            }
        }
    }

    #[test]
    fn make_day_huge_months() {
        // Synthetic regressions: adjacent representable months must balance
        // using the mathematical quotient, before converting it to Number.
        for (year, month, balanced_year, balanced_month, expected) in [
            (
                -6004799503160663.0,
                72057594037927936.0,
                -2.0,
                4.0,
                -62219923200000.0,
            ),
            (
                -6004799503160663.0,
                72057594037927952.0,
                -1.0,
                8.0,
                -62177760000000.0,
            ),
            (
                -6004799503160663.0,
                72057594037927968.0,
                1.0,
                0.0,
                -62135596800000.0,
            ),
            (
                6004799503160663.0,
                -72057594037927936.0,
                1.0,
                8.0,
                -62114601600000.0,
            ),
            (
                6004799503160663.0,
                -72057594037927952.0,
                0.0,
                4.0,
                -62156764800000.0,
            ),
            (
                6004799503160663.0,
                -72057594037927968.0,
                -1.0,
                0.0,
                -62198755200000.0,
            ),
            (
                -768614336404564608.0,
                9223372036854774784.0,
                0.0,
                4.0,
                -62156764800000.0,
            ),
            (
                -768614336404564608.0,
                9223372036854775808.0,
                0.0,
                8.0,
                -62146137600000.0,
            ),
            (
                768614336404564608.0,
                -9223372036854774784.0,
                0.0,
                8.0,
                -62146137600000.0,
            ),
            (
                768614336404564608.0,
                -9223372036854775808.0,
                0.0,
                4.0,
                -62156764800000.0,
            ),
            (
                -768614336404564864.0,
                9223372036854777856.0,
                0.0,
                4.0,
                -62156764800000.0,
            ),
            (
                768614336404564864.0,
                -9223372036854777856.0,
                0.0,
                8.0,
                -62146137600000.0,
            ),
        ] {
            let day = make_day(year, month, 1.0);
            assert_eq!(
                day,
                make_day(balanced_year, balanced_month, 1.0),
                "{year}, {month}"
            );
            assert_eq!(time_clip(make_date(day, 0.0)), expected, "{year}, {month}");
        }
        for exponent in [53, 63, 127, 900, 1020] {
            let year = 2.0_f64.powi(exponent);
            for sign in [-1.0, 1.0] {
                assert_eq!(
                    make_day(-sign * year, sign * 12.0 * year, 1.0),
                    make_day(0.0, 0.0, 1.0)
                );
            }
        }
    }

    // Date.UTC rows adapted from QuickJS tests/test_builtin.js::test_date,
    // revision 7d1c13c5d8f8. Copyright (c) 2017-2021 Fabrice Bellard
    // and Charlie Gordon.
    // Permission is hereby granted, free of charge, to any person obtaining a copy
    // of this software and associated documentation files (the "Software"), to deal
    // in the Software without restriction, including without limitation the rights
    // to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
    // copies of the Software, and to permit persons to whom the Software is
    // furnished to do so, subject to the following conditions:
    // The above copyright notice and this permission notice shall be included in
    // all copies or substantial portions of the Software.
    // THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
    // IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
    // FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
    // THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
    // LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
    // OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
    // THE SOFTWARE.
    #[test]
    fn date_time_arithmetic() {
        fn calculate(input: &[f64]) -> f64 {
            let mut fields = [f64::NAN, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
            for (field, input) in fields.iter_mut().zip(input) {
                *field = *input;
            }
            let [mut y, m, d, h, min, s, ms] = fields;
            if (0.0..=99.0).contains(&y.trunc()) {
                y = 1900.0 + y.trunc();
            }
            time_clip(make_date(make_day(y, m, d), make_time(h, min, s, ms)))
        }
        let full = [2017.0, 9.0, 22.0, 18.0, 10.0, 11.0, 91.0];
        for (i, expected) in [
            1483228800000.0,
            1506816000000.0,
            1508630400000.0,
            1508695200000.0,
            1508695800000.0,
            1508695811000.0,
            1508695811091.0,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(calculate(&full[..=i]), expected);
            let mut nan = full;
            nan[i] = f64::NAN;
            assert!(calculate(&nan[..=i]).is_nan());
        }
        assert_eq!(
            calculate(&[2017.0, 9.0, 22.0, 18.0, 10.0, 11.0, 91.0, f64::NAN]),
            1508695811091.0
        );
        assert_eq!(
            calculate(&[
                1970.0,
                0.0,
                1.0,
                80063993375.0,
                29.0,
                1.0,
                -288230376151711740.0
            ]),
            29312.0
        );
        assert_eq!(
            calculate(&[
                1970.0,
                0.0,
                213503982336.0,
                0.0,
                0.0,
                0.0,
                -18446744073709552000.0
            ]),
            34447360.0
        );
        assert_eq!(
            calculate(&[2017.0, 9.0, 22.0 - 1e10, 18.0 + 24e10]),
            1508695200000.0
        );
        assert_eq!(
            calculate(&[2017.0, 9.0, 22.0, 18.0 - 1e10, 10.0 + 60e10]),
            1508695800000.0
        );
        assert_eq!(
            calculate(&[2017.0, 9.0, 22.0, 18.0, 10.0 - 1e10, 11.0 + 60e10]),
            1508695811000.0
        );
        assert_eq!(
            calculate(&[2017.0, 9.0, 22.0, 18.0, 10.0, 11.0 - 1e12, 91.0 + 1000e12]),
            1508695811091.0
        );

        assert!(time_clip(8.64e15 + 1.0).is_nan());
        assert!(time_clip(-8.64e15 - 1.0).is_nan());
        assert_eq!(time_clip(-0.0).to_bits(), 0.0_f64.to_bits());
        assert_eq!(time_clip(-0.9).to_bits(), 0.0_f64.to_bits());
        assert_eq!(time_clip(8.64e15), 8.64e15);
        assert_eq!(time_clip(-8.64e15), -8.64e15);
        for time in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(time_clip(time).is_nan());
        }
        assert_eq!(make_day(1970.0, 0.0, 1.0), 0.0);
        assert_eq!(make_day(1969.0, 12.0, 1.0), 0.0);
        assert_eq!(make_day(1970.0, -1.0, 1.0), -31.0);
        assert_eq!(
            make_day(2000.0, 2.0, 1.0) - make_day(2000.0, 1.0, 1.0),
            29.0
        );
        assert_eq!(
            make_day(1900.0, 2.0, 1.0) - make_day(1900.0, 1.0, 1.0),
            28.0
        );
        assert_eq!(make_day(0.0, 2.0, 1.0) - make_day(0.0, 1.0, 1.0), 29.0);
        assert_eq!(calculate(&[275760.0, 8.0, 13.0]), 8.64e15);
        assert_eq!(calculate(&[-271821.0, 3.0, 20.0]), -8.64e15);
        assert_eq!(make_day(1_001_970.0, -12_000_000.0, 1.0), 0.0);
        assert_eq!(
            calculate(&[35184372088833.0, 0.0, -12850828021936060.0]),
            -86400000.0
        );
        for (year, month, start) in [
            (35184372088833.0, 0.0, 12850828021936060_i128),
            (35184372088833.0, 2.0, 12850828021936119),
            (-35184372088833.0, 0.0, -12850828023375115),
            (-35184372088833.0, 2.0, -12850828023375056),
        ] {
            assert_eq!(calculate(&[year, month, -(start as f64)]), -86400000.0);
        }
        for (year, february_days) in [(-100.0, 28.0), (-400.0, 29.0)] {
            assert_eq!(
                make_day(year, 2.0, 1.0) - make_day(year, 1.0, 1.0),
                february_days
            );
        }
        for sign in [-1.0, 1.0] {
            assert!(make_day(sign * 2.0_f64.powi(100), 0.0, 1.0).is_finite());
            for year in [2.0_f64.powi(126), 2.0_f64.powi(127), f64::MAX] {
                assert!(make_day(sign * year, 0.0, 1.0).is_nan());
            }
        }
        // Even a normalized year outside TimeClip can cancel against date.
        let days = make_day(1_000_000.0, 0.0, 1.0);
        assert_eq!(calculate(&[1_000_000.0, 0.0, 1.0 - days]), 0.0);
        assert_eq!(calculate(&[99.9]), calculate(&[1999.0]));
        assert_eq!(calculate(&[-0.9]), calculate(&[1900.0]));
        assert_eq!(make_time(1.9, -2.9, 3.9, -4.9), 3_482_996.0);
        assert!(make_day(f64::INFINITY, 0.0, 1.0).is_nan());
        assert!(make_time(0.0, f64::INFINITY, 0.0, 0.0).is_nan());
        assert!(make_date(1e308, 0.0).is_nan());
    }
}
