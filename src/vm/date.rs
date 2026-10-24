//! UTC time-value core (clj-quickjs-179). Fresh Rust implementation of
//! ECMA-262 MakeDay/MakeTime/MakeDate/TimeClip; QuickJS's time_clip,
//! set_date_fields and js_Date_UTC (research/quickjs/quickjs.c, revision
//! 7d1c13c5d8f8) are semantic references, not ported code.
//!
//! Clock and local-zone integration belongs here when clj-quickjs-182 lands:
//! obtain the clock as an unrecorded leaf effect through the embedding host,
//! and keep one generated rule-table zone per runtime (UTC without a host).
//! Snapshot transition coverage is limited: reject uncovered zones explicitly.
//! Never read the OS clock or zone here. Parsing/formatting are separate pellets.
use crate::heap::{Heap, HeapObject};
use crate::ids::TaskId;
use crate::object::OrdinaryObject;
use crate::value::Value;
use crate::vm::dispatch::{Fault, to_number};

pub(crate) const MS_PER_DAY: f64 = 86_400_000.0;

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
    if !year.is_finite() {
        return f64::NAN;
    }
    let month = ((month % 12.0 + 12.0) % 12.0) as usize;
    let leap = year % 4.0 == 0.0 && (year % 100.0 != 0.0 || year % 400.0 == 0.0);
    const BEFORE_MONTH: [u16; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    // Gregorian leap counts use floor, including negative years. No TimeClip
    // or year-range cutoff is applied to intermediate days: later fields can
    // cancel them. Integer day counts throughout the clipped range are exact.
    let days = 365.0 * (year - 1970.0) + ((year - 1969.0) / 4.0).floor()
        - ((year - 1901.0) / 100.0).floor()
        + ((year - 1601.0) / 400.0).floor();
    let start = days + f64::from(BEFORE_MONTH[month]) + f64::from(leap && month >= 2);
    start + date.trunc() - 1.0
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
    let mut numbers = [f64::NAN, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
    for (out, value) in numbers.iter_mut().zip(fields) {
        *out = to_number(value)?;
    }
    let [mut year, month, date, hour, minute, second, ms] = numbers;
    if year.is_finite() && (0.0..=99.0).contains(&year.trunc()) {
        year = 1900.0 + year.trunc();
    }
    Ok(Value::Number(time_clip(make_date(
        make_day(year, month, date),
        make_time(hour, minute, second, ms),
    ))))
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
