//! UTC time-value core (clj-quickjs-179). Fresh Rust implementation of
//! ECMA-262 MakeDay/MakeTime/MakeDate/TimeClip; QuickJS's time_clip,
//! set_date_fields and js_Date_UTC (research/quickjs/quickjs.c, revision
//! 7d1c13c5d8f8) are semantic references, not ported code.
//!
//! Clock reads are unrecorded host leaf effects. One generated rule-table zone
//! is selected per runtime (UTC without a host). Never read the OS clock or
//! zone here. Snapshot coverage is limited; uncovered names are rejected.
//! Shared decomposition, ISO and non-locale string formatting are implemented
//! below, together with ISO and QuickJS fallback parsing (-184/-185).
use crate::heap::{Heap, HeapObject};
use crate::ids::TaskId;
use crate::object::OrdinaryObject;
use crate::value::Value;
use crate::vm::dispatch::{Fault, to_number};

pub(crate) const MS_PER_DAY: f64 = 86_400_000.0;

/// Parsed calendar fields use a zero-based month, with no 1900 adjustment.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ParsedDate {
    fields: [i32; 7],
    pub(crate) is_local: bool,
    offset_minutes: i32,
}

impl Default for ParsedDate {
    fn default() -> Self {
        Self {
            fields: [0, 0, 1, 0, 0, 0, 0],
            is_local: false,
            offset_minutes: 0,
        }
    }
}

impl ParsedDate {
    pub(crate) fn time(self, zone: crate::host::TimeZone) -> f64 {
        let [y, m, d, h, min, s, ms] = self.fields.map(f64::from);
        let wall = make_date(make_day(y, m, d), make_time(h, min, s, ms));
        let utc = if self.is_local {
            zone.utc_time_millis(wall)
        } else {
            wall - f64::from(self.offset_minutes) * 60_000.0
        };
        time_clip(utc)
    }
}

/// Date parsing entry point, shared by Date.parse and string construction.
/// It owns immutable strings, Rust buffers and scalar state, with no heap edges.
#[derive(Debug)]
pub(crate) struct Parser {
    text: Option<crate::value::JsString>,
    flatten: crate::value::string::StringFlatten,
    iso: IsoParser,
    fallback: Option<FallbackParser>,
}

impl Parser {
    pub(crate) fn new(text: crate::value::JsString) -> Result<Self, Fault> {
        Ok(Self {
            text: None,
            flatten: crate::value::string::StringFlatten::new(text)
                .ok_or(Fault::OutOfMemory("Date parse input allocation failed"))?,
            iso: IsoParser::default(),
            fallback: None,
        })
    }

    /// At most `budget` scan steps and constant-size calendar/zone
    /// lookups per step. Rope traversal/copying is also budgeted. Total O(input
    /// length) work; flat inputs share storage, ropes use an O(length) buffer.
    /// There is no date-string length cap; every trailing unit is validated.
    pub(crate) fn advance(&mut self, budget: usize) -> Option<Option<ParsedDate>> {
        let Some(text) = &self.text else {
            self.text = self.flatten.advance(budget);
            return None;
        };
        if let Some(fallback) = &mut self.fallback {
            return fallback.advance(text.code_units(), budget);
        }
        match self.iso.advance(text.code_units(), budget) {
            Some(None) => {
                self.fallback = Some(FallbackParser::default());
                None
            }
            result => result,
        }
    }
}

/// Pure UTF-16 ISO grammar, separate from zone lookup and heap operations.
#[derive(Debug, Default)]
struct IsoParser {
    next: usize,
    started: bool,
    fraction_multiplier: Option<i32>,
    parsed: ParsedDate,
}

struct IsoCursor<'a> {
    units: &'a [u16],
    next: usize,
}

impl IsoCursor<'_> {
    fn eat(&mut self, ascii: u8) -> bool {
        if self.units.get(self.next) == Some(&u16::from(ascii)) {
            self.next += 1;
            true
        } else {
            false
        }
    }

    fn digits(&mut self, count: usize) -> Option<i32> {
        let mut number = 0;
        for _ in 0..count {
            let &unit = self.units.get(self.next)?;
            if !(48..=57).contains(&unit) {
                return None;
            }
            number = number * 10 + i32::from(unit - 48);
            self.next += 1;
        }
        Some(number)
    }
}

impl IsoParser {
    /// Fixed-width prefix work is bounded independently of input length.
    fn start(&mut self, units: &[u16]) -> Option<()> {
        let mut cursor = IsoCursor { units, next: 0 };
        let expanded = cursor.eat(b'+');
        let negative = !expanded && cursor.eat(b'-');
        let mut year = cursor.digits(if expanded || negative { 6 } else { 4 })?;
        if negative {
            if year == 0 {
                return None;
            }
            year = -year;
        }
        self.parsed.fields[0] = year;
        if cursor.eat(b'-') {
            self.parsed.fields[1] = cursor.digits(2)? - 1;
            if cursor.eat(b'-') {
                self.parsed.fields[2] = cursor.digits(2)?;
            }
        }
        if cursor.eat(b'T') {
            self.parsed.is_local = true;
            self.parsed.fields[3] = cursor.digits(2)?;
            if !cursor.eat(b':') {
                return None;
            }
            self.parsed.fields[4] = cursor.digits(2)?;
            if cursor.eat(b':') {
                self.parsed.fields[5] = cursor.digits(2)?;
                if cursor.eat(b'.') {
                    if !units
                        .get(cursor.next)
                        .is_some_and(|u| (48..=57).contains(u))
                    {
                        return None;
                    }
                    self.fraction_multiplier = Some(100);
                }
            }
        }
        self.next = cursor.next;
        Some(())
    }

    fn finish(&mut self, units: &[u16]) -> Option<ParsedDate> {
        let mut cursor = IsoCursor {
            units,
            next: self.next,
        };
        // QuickJS recognizes ISO zone suffixes on date-only forms too. They
        // must win over the fallback's signed-year interpretation.
        if cursor.eat(b'Z') {
            self.parsed.is_local = false;
        } else {
            let positive = cursor.eat(b'+');
            let negative = !positive && cursor.eat(b'-');
            if positive || negative {
                let hour = cursor.digits(2)?;
                cursor.eat(b':');
                let minute = cursor.digits(2)?;
                if hour > 23 || minute > 59 {
                    return None;
                }
                self.parsed.offset_minutes = (hour * 60 + minute) * if negative { -1 } else { 1 };
                self.parsed.is_local = false;
            }
        }
        let [_, month, day, hour, minute, second, ms] = self.parsed.fields;
        if cursor.next != units.len()
            || !(0..12).contains(&month)
            || !(1..=31).contains(&day)
            || hour > 24
            || minute > 59
            || second > 59
            || (hour == 24 && (minute != 0 || second != 0 || ms != 0))
        {
            return None;
        }
        Some(self.parsed)
    }

    fn advance(&mut self, units: &[u16], budget: usize) -> Option<Option<ParsedDate>> {
        if !self.started {
            self.started = true;
            if self.start(units).is_none() {
                return Some(None);
            }
        }
        if let Some(multiplier) = &mut self.fraction_multiplier {
            for _ in 0..budget {
                let Some(&digit) = units.get(self.next).filter(|&&u| (48..=57).contains(&u)) else {
                    self.fraction_multiplier = None;
                    break;
                };
                self.parsed.fields[6] += i32::from(digit - 48) * *multiplier;
                *multiplier /= 10;
                self.next += 1;
            }
            if self.fraction_multiplier.is_some() {
                return None;
            }
        }
        Some(self.finish(units))
    }
}

/// Pure, resumable QuickJS fallback grammar. Every unbounded scan (including
/// leading zeroes, ignored words and nested comments) consumes the step budget.
/// Name matching and lookahead inspect only fixed-size ASCII prefixes.
#[derive(Debug)]
struct FallbackParser {
    next: usize,
    parsed: ParsedDate,
    has_year: bool,
    has_month: bool,
    has_time: bool,
    numbers: [i32; 3],
    number_count: usize,
    scan: FallbackScan,
}

#[derive(Clone, Copy, Debug)]
enum FallbackNumber {
    Date,
    Signed { negative: bool, immediate: bool },
    Minute,
    Second,
}

#[derive(Clone, Copy, Debug)]
enum FallbackScan {
    Main,
    Separators,
    Word {
        month: bool,
    },
    Comment {
        depth: usize,
    },
    Number {
        kind: FallbackNumber,
        start: usize,
        value: i32,
        count: usize,
    },
    Fraction {
        count: usize,
        multiplier: i32,
    },
    AfterTime,
}

impl Default for FallbackParser {
    fn default() -> Self {
        Self {
            next: 0,
            // Keep a one-based month until numeric field ordering is resolved.
            parsed: ParsedDate {
                fields: [2001, 1, 1, 0, 0, 0, 0],
                is_local: true,
                offset_minutes: 0,
            },
            has_year: false,
            has_month: false,
            has_time: false,
            numbers: [0; 3],
            number_count: 0,
            scan: FallbackScan::Main,
        }
    }
}

const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

// Abbreviations, order and offsets follow js_tzabbr. Prefix matching is
// intentional (for example UTC is tested before UT).
const FALLBACK_ZONES: [(&str, i32); 18] = [
    ("GMT", 0),
    ("UTC", 0),
    ("UT", 0),
    ("Z", 0),
    ("EDT", -240),
    ("EST", -300),
    ("CDT", -300),
    ("CST", -360),
    ("MDT", -360),
    ("MST", -420),
    ("PDT", -420),
    ("PST", -480),
    ("WET", 0),
    ("WEST", 60),
    ("CET", 60),
    ("CEST", 120),
    ("EET", 120),
    ("EEST", 180),
];

impl FallbackParser {
    fn unit(units: &[u16], next: usize) -> Option<u16> {
        units.get(next).map(|&u| if u == 0x2212 { 45 } else { u })
    }

    fn matches(units: &[u16], next: usize, name: &str) -> bool {
        name.bytes().enumerate().all(|(i, byte)| {
            units
                .get(next + i)
                .is_some_and(|&u| u <= 127 && (u as u8).eq_ignore_ascii_case(&byte))
        })
    }

    fn number(&mut self, kind: FallbackNumber) {
        self.scan = FallbackScan::Number {
            kind,
            start: self.next,
            value: 0,
            count: 0,
        };
    }

    fn short_year(value: i32) -> i32 {
        value
            + if value < 50 {
                2000
            } else if value < 100 {
                1900
            } else {
                0
            }
    }

    fn finish(&self) -> Option<ParsedDate> {
        if self.number_count + usize::from(self.has_year) + usize::from(self.has_month) > 3 {
            return None;
        }
        let mut result = self.parsed;
        let fields = &mut result.fields;
        let [a, b, c] = self.numbers;
        match self.number_count {
            0 if !self.has_year => return None,
            0 => {}
            1 if self.has_month => fields[2] = a,
            1 => fields[1] = a,
            2 if self.has_year => {
                fields[1] = a;
                fields[2] = b;
            }
            2 if self.has_month => {
                fields[0] = Self::short_year(b);
                fields[2] = a;
            }
            2 => {
                fields[1] = a;
                fields[2] = b;
            }
            3 => {
                fields[0] = Self::short_year(c);
                fields[1] = a;
                fields[2] = b;
            }
            _ => return None,
        }
        let [_, month, day, hour, minute, second, ms] = *fields;
        if !(1..=12).contains(&month)
            || !(1..=31).contains(&day)
            || hour > 24
            || minute > 59
            || second > 59
            || (hour == 24 && (minute != 0 || second != 0 || ms != 0))
        {
            return None;
        }
        fields[1] -= 1;
        Some(result)
    }

    /// Finish a non-strict signed offset after its digit run. This lookahead
    /// consumes at most a colon and two digits. Excess digit pairs are dropped
    /// from the right, as in QuickJS; leading zeroes do not impose a length cap.
    fn offset(&self, units: &[u16], mut value: i32, mut count: usize) -> Option<(i32, usize)> {
        if count == 0 {
            return None;
        }
        if count > 4 {
            let pairs = (count - 3) / 2;
            value = if pairs >= 5 {
                0
            } else {
                value / 100_i32.pow(pairs as u32)
            };
            count -= pairs * 2;
        }
        let mut next = self.next;
        let (hour, minute) = if count > 2 {
            (value / 100, value % 100)
        } else if units.get(next) == Some(&58) {
            let mut cursor = IsoCursor {
                units,
                next: next + 1,
            };
            let minute = cursor.digits(2)?;
            next = cursor.next;
            (value, minute)
        } else {
            (value, 0)
        };
        (hour <= 23 && minute <= 59).then_some((hour * 60 + minute, next))
    }

    fn finish_number(
        &mut self,
        units: &[u16],
        kind: FallbackNumber,
        start: usize,
        value: i32,
        count: usize,
        valid: bool,
    ) -> Option<()> {
        if !valid || count == 0 {
            match kind {
                FallbackNumber::Signed { immediate, .. } => {
                    self.next = start - usize::from(immediate);
                    self.scan = FallbackScan::Separators;
                }
                FallbackNumber::Date
                    if !self.has_year
                        && !self.has_month
                        && !self.has_time
                        && self.number_count == 0 =>
                {
                    self.next = start;
                    self.scan = FallbackScan::Word { month: false };
                }
                _ => return None,
            }
            return Some(());
        }
        self.scan = FallbackScan::Separators;
        match kind {
            FallbackNumber::Date => {
                if units.get(self.next) == Some(&58) {
                    self.next += 1;
                    self.parsed.fields[3] = value;
                    self.number(FallbackNumber::Minute);
                } else if !self.has_year && (count > 2 || !(1..=31).contains(&value)) {
                    self.parsed.fields[0] = if count > 2 {
                        value
                    } else {
                        Self::short_year(value)
                    };
                    self.has_year = true;
                } else {
                    *self.numbers.get_mut(self.number_count)? = value;
                    self.number_count += 1;
                }
            }
            FallbackNumber::Signed {
                negative,
                immediate,
            } => {
                if self.has_time
                    && let Some((offset, next)) = self.offset(units, value, count)
                {
                    self.next = next;
                    self.parsed.offset_minutes = if negative { -offset } else { offset };
                    self.parsed.is_local = false;
                } else if immediate {
                    // Failed immediate offset attempts leave the sign for the
                    // usual separator/year grammar. At most one retry is needed.
                    self.next = start - 1;
                } else {
                    if negative && value == 0 {
                        return None;
                    }
                    self.parsed.fields[0] = if negative { -value } else { value };
                    self.has_year = true;
                }
            }
            FallbackNumber::Minute => {
                self.parsed.fields[4] = value;
                if units.get(self.next) == Some(&58) {
                    self.next += 1;
                    self.number(FallbackNumber::Second);
                } else {
                    self.scan = FallbackScan::AfterTime;
                }
            }
            FallbackNumber::Second => {
                self.parsed.fields[5] = value;
                if matches!(units.get(self.next), Some(44 | 46))
                    && units
                        .get(self.next + 1)
                        .is_some_and(|u| (48..=57).contains(u))
                {
                    self.next += 1;
                    self.parsed.fields[6] = 0;
                    self.scan = FallbackScan::Fraction {
                        count: 0,
                        multiplier: 100,
                    };
                } else {
                    self.scan = FallbackScan::AfterTime;
                }
            }
        }
        Some(())
    }

    fn step(&mut self, units: &[u16]) -> Option<Option<ParsedDate>> {
        let unit = Self::unit(units, self.next);
        // Do not inherit QuickJS's NUL-terminated prefix truncation.
        if unit == Some(0) {
            return Some(None);
        }
        match self.scan {
            FallbackScan::Main => {
                let Some(c) = unit else {
                    return Some(self.finish());
                };
                if c == 32 {
                    self.next += 1;
                } else if c == 43 || c == 45 {
                    self.next += 1;
                    self.number(FallbackNumber::Signed {
                        negative: c == 45,
                        immediate: false,
                    });
                } else if (48..=57).contains(&c) {
                    self.number(FallbackNumber::Date);
                } else if let Some(month) = MONTH_NAMES
                    .iter()
                    .position(|name| Self::matches(units, self.next, name))
                {
                    self.parsed.fields[1] = month as i32 + 1;
                    self.has_month = true;
                    self.next += 3;
                    self.scan = FallbackScan::Word { month: true };
                } else if self.has_time && Self::matches(units, self.next, "PM") {
                    if self.parsed.fields[3] < 12 {
                        self.parsed.fields[3] += 12;
                    }
                    self.next += 2;
                } else if self.has_time && Self::matches(units, self.next, "AM") {
                    if self.parsed.fields[3] == 12 {
                        self.parsed.fields[3] = 0;
                    }
                    self.next += 2;
                } else if let Some(&(name, offset)) = FALLBACK_ZONES
                    .iter()
                    .find(|(name, _)| Self::matches(units, self.next, name))
                {
                    self.parsed.offset_minutes = offset;
                    self.parsed.is_local = false;
                    self.next += name.len();
                } else if c == 40 {
                    self.next += 1;
                    self.scan = FallbackScan::Comment { depth: 1 };
                } else if c == 41
                    || self.has_year
                    || self.has_month
                    || self.has_time
                    || self.number_count != 0
                {
                    return Some(None);
                } else {
                    self.scan = FallbackScan::Word { month: false };
                }
            }
            FallbackScan::Separators => {
                if matches!(unit, Some(44..=47)) {
                    self.next += 1;
                } else {
                    self.scan = FallbackScan::Main;
                }
            }
            FallbackScan::Word { month } => {
                if unit.is_none()
                    || matches!(unit, Some(32 | 45 | 47 | 40 | 41))
                    || (month && unit.is_some_and(|c| (48..=57).contains(&c)))
                {
                    self.scan = FallbackScan::Separators;
                } else {
                    self.next += 1;
                }
            }
            FallbackScan::Comment { mut depth } => {
                let Some(c) = unit else {
                    return Some(None);
                };
                depth += usize::from(c == 40);
                depth -= usize::from(c == 41);
                self.next += 1;
                self.scan = if depth == 0 {
                    FallbackScan::Separators
                } else {
                    FallbackScan::Comment { depth }
                };
            }
            FallbackScan::Number {
                kind,
                start,
                value,
                count,
            } => {
                let digit = unit.filter(|c| (48..=57).contains(c));
                let capped =
                    matches!(kind, FallbackNumber::Minute | FallbackNumber::Second) && count == 2;
                if let Some(digit) = digit
                    && !capped
                    && value < 100_000_000
                {
                    self.next += 1;
                    self.scan = FallbackScan::Number {
                        kind,
                        start,
                        value: value * 10 + i32::from(digit - 48),
                        count: count + 1,
                    };
                } else if self
                    .finish_number(
                        units,
                        kind,
                        start,
                        value,
                        count,
                        digit.is_none() || capped || value < 100_000_000,
                    )
                    .is_none()
                {
                    return Some(None);
                }
            }
            FallbackScan::Fraction { count, multiplier } => {
                if let Some(digit) = unit.filter(|c| (48..=57).contains(c))
                    && count < 9
                {
                    self.parsed.fields[6] += i32::from(digit - 48) * multiplier;
                    self.next += 1;
                    self.scan = FallbackScan::Fraction {
                        count: count + 1,
                        multiplier: multiplier / 10,
                    };
                } else {
                    self.scan = FallbackScan::AfterTime;
                }
            }
            FallbackScan::AfterTime => {
                self.has_time = true;
                if matches!(unit, Some(43 | 45)) {
                    self.next += 1;
                    self.number(FallbackNumber::Signed {
                        negative: unit == Some(45),
                        immediate: true,
                    });
                } else {
                    self.scan = FallbackScan::Separators;
                }
            }
        }
        None
    }

    fn advance(&mut self, units: &[u16], budget: usize) -> Option<Option<ParsedDate>> {
        for _ in 0..budget {
            if let Some(result) = self.step(units) {
                return Some(result);
            }
        }
        None
    }
}

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

pub(crate) fn calendar_fields(time: f64) -> Option<UtcFields> {
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

/// Non-locale QuickJS formats. `zone` selects local formatting with date/time
/// part bits 1/2; without a zone the complete UTC format is used. Like QuickJS's
/// get_date_fields, truncate the offset to minutes before decomposing the time.
/// This intentionally differs from the second-precision local field getters.
pub(crate) fn format_string(time: f64, zone: Option<crate::host::TimeZone>, part: u8) -> Value {
    // QuickJS's host offset lookup first divides the integer epoch time by
    // 1000, truncating toward zero (observable just before pre-epoch changes).
    let offset_time = (time as i64 / 1000 * 1000) as f64;
    let offset = zone.map_or(0, |z| z.offset_seconds(offset_time) / 60);
    let Some(fields) = calendar_fields(time + f64::from(offset) * 60_000.0) else {
        return Value::String("Invalid Date".into());
    };
    let day = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][fields.weekday as usize];
    let month = MONTH_NAMES[fields.month as usize];
    let width = if fields.year < 0 { 5 } else { 4 };
    let year = format!("{:0width$}", fields.year);
    let clock = format!(
        "{:02}:{:02}:{:02}",
        fields.hours, fields.minutes, fields.seconds
    );
    let text = if zone.is_some() {
        let date = format!("{day} {month} {:02} {year}", fields.date);
        let sign = if offset < 0 { '-' } else { '+' };
        let minutes = offset.abs();
        let time = format!("{clock} GMT{sign}{:02}{:02}", minutes / 60, minutes % 60);
        match part {
            1 => date,
            2 => time,
            3 => format!("{date} {time}"),
            _ => unreachable!("invalid Date format part"),
        }
    } else {
        format!("{day}, {:02} {month} {year} {clock} GMT", fields.date)
    };
    Value::String(text.as_str().into())
}

pub(crate) fn to_utc_string(heap: &Heap, task: TaskId, this: &Value) -> Result<Value, Fault> {
    Ok(format_string(this_time_value(heap, task, this)?, None, 3))
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

/// Construct after resumable argument conversion and optional string parsing.
pub(crate) fn construct(heap: &Heap, task: TaskId, time: f64) -> Result<Value, Fault> {
    let time = time_clip(time);
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

    // Upstream rows share the complete QuickJS copyright/permission notice
    // below (date_time_arithmetic); remaining inputs are synthetic fixtures.
    #[test]
    fn date_fallback_parse_fields() {
        for (text, fields, offset) in [
            ("Jan 1 2000", [2000, 0, 1, 0, 0, 0, 0], None),
            ("Jan 1 2000 00:00", [2000, 0, 1, 0, 0, 0, 0], None),
            ("Jan 1 2000 00:00:00", [2000, 0, 1, 0, 0, 0, 0], None),
            ("Sat Jan 1 2000", [2000, 0, 1, 0, 0, 0, 0], None),
            ("Sat Jan 1 2000 00:00", [2000, 0, 1, 0, 0, 0, 0], None),
            ("Sat Jan 1 2000 00:00:00", [2000, 0, 1, 0, 0, 0, 0], None),
            (
                "Jan 1 2000 00:00:00 GMT+0100",
                [2000, 0, 1, 0, 0, 0, 0],
                Some(60),
            ),
            (
                "Jan 1 2000 00:00:00 GMT+0200",
                [2000, 0, 1, 0, 0, 0, 0],
                Some(120),
            ),
            (
                "Sat Jan 1 2000 00:00:00 GMT+0100",
                [2000, 0, 1, 0, 0, 0, 0],
                Some(60),
            ),
            (
                "Sat Jan 1 2000 00:00:00 GMT+0200",
                [2000, 0, 1, 0, 0, 0, 0],
                Some(120),
            ),
            (
                "Sat, 01 Jan 2000 00:00:00 GMT",
                [2000, 0, 1, 0, 0, 0, 0],
                Some(0),
            ),
            ("Jan 1 2000 12:30 PM", [2000, 0, 1, 12, 30, 0, 0], None),
            ("1/2/2000", [2000, 0, 2, 0, 0, 0, 0], None),
            ("Jan 1 2000 (comment) 00:00", [2000, 0, 1, 0, 0, 0, 0], None),
            ("Jan 1 2000 EST", [2000, 0, 1, 0, 0, 0, 0], Some(-300)),
        ] {
            let units: Vec<u16> = text.encode_utf16().collect();
            let mut parser = FallbackParser::default();
            let parsed = (0..10000)
                .find_map(|_| parser.advance(&units, 1))
                .expect("bounded fallback parse")
                .expect(text);
            assert_eq!(parsed.fields, fields, "{text}");
            assert_eq!(parsed.is_local, offset.is_none(), "{text}");
            assert_eq!(parsed.offset_minutes, offset.unwrap_or(0), "{text}");
        }
        for text in ["Jan 1 2000)", "Jan 1 2000 (x", "foo"] {
            let units: Vec<u16> = text.encode_utf16().collect();
            let mut parser = FallbackParser::default();
            assert!(
                (0..10000)
                    .find_map(|_| parser.advance(&units, 1))
                    .expect("bounded fallback parse")
                    .is_none(),
                "{text}"
            );
        }
    }

    #[test]
    fn date_fallback_order_years_and_tokens() {
        for (text, expected) in [
            ("0", [2000, 0, 1, 0, 0, 0, 0]),
            ("1", [2001, 0, 1, 0, 0, 0, 0]),
            ("12", [2001, 11, 1, 0, 0, 0, 0]),
            ("32", [2032, 0, 1, 0, 0, 0, 0]),
            ("49", [2049, 0, 1, 0, 0, 0, 0]),
            ("50", [1950, 0, 1, 0, 0, 0, 0]),
            ("99", [1999, 0, 1, 0, 0, 0, 0]),
            ("100", [100, 0, 1, 0, 0, 0, 0]),
            ("01/02/03", [2003, 0, 2, 0, 0, 0, 0]),
            ("1/2/49", [2049, 0, 2, 0, 0, 0, 0]),
            ("1/2/50", [1950, 0, 2, 0, 0, 0, 0]),
            ("1/2/0003", [3, 0, 2, 0, 0, 0, 0]),
            ("Jan 1 01", [2001, 0, 1, 0, 0, 0, 0]),
            ("1 Jan 2", [2002, 0, 1, 0, 0, 0, 0]),
            ("2000/1/2", [2000, 0, 2, 0, 0, 0, 0]),
            ("1/2000/2", [2000, 0, 2, 0, 0, 0, 0]),
            ("Jan2000", [2000, 0, 1, 0, 0, 0, 0]),
            ("Jan 1 +0000", [0, 0, 1, 0, 0, 0, 0]),
            ("Sat Jan 01 -0001 00:00 GMT", [-1, 0, 1, 0, 0, 0, 0]),
            (
                "jAnUaRy 1 2000 (nested (comment)) 12:30 aM",
                [2000, 0, 1, 0, 30, 0, 0],
            ),
            ("Jan 1 2000 1:2:3,123456789 PM", [2000, 0, 1, 13, 2, 3, 123]),
            ("Jan 1 2000 24:00", [2000, 0, 1, 24, 0, 0, 0]),
        ] {
            let units: Vec<u16> = text.encode_utf16().collect();
            let mut parser = FallbackParser::default();
            let fields = (0..10000)
                .find_map(|_| parser.advance(&units, 1))
                .expect("bounded fallback parse")
                .expect(text);
            assert_eq!(fields.fields, expected, "{text}");
        }
        for (suffix, offset) in [
            ("GMT", 0),
            ("utc", 0),
            ("Ut", 0),
            ("z", 0),
            ("EDT", -240),
            ("EST", -300),
            ("CDT", -300),
            ("CST", -360),
            ("MDT", -360),
            ("MST", -420),
            ("PDT", -420),
            ("PST", -480),
            ("WET", 0),
            ("WEST", 60),
            ("CET", 60),
            ("CEST", 120),
            ("EET", 120),
            ("EEST", 180),
            ("+1", 60),
            ("-1", -60),
            ("+001", 1),
            ("-01:30", -90),
            ("GMT+12345", 83),
            ("+123456", 754),
            ("\u{2212}0500", -300),
        ] {
            let units: Vec<u16> = format!("Jan 1 2000 0:00 {suffix}").encode_utf16().collect();
            let mut parser = FallbackParser::default();
            let fields = (0..10000)
                .find_map(|_| parser.advance(&units, 1))
                .expect("bounded fallback parse")
                .expect(suffix);
            assert!(!fields.is_local, "{suffix}");
            assert_eq!(fields.offset_minutes, offset, "{suffix}");
        }
        for text in [
            "",
            "Jan",
            "foo",
            "Jan 1 2000)",
            "Jan 1 2000 (x",
            ")Jan 1 2000",
            "Jan) 1 2000",
            "Jan 1 2000 ((x)",
            "Jan 1 2000 xyz",
            "1/2/3/4",
            "Jan 1 2 2000",
            "13/1/2000",
            "2000/0/1",
            "2000/1/0",
            "2000/1/32",
            "Jan 1 -0",
            "Jan 1 2000 25:00",
            "Jan 1 2000 24:00:00.001",
            "Jan 1 2000 0:60",
            "Jan 1 2000 0:00:60",
            "Jan 1 2000 0:",
            "Jan 1 2000 0:00:",
            "Jan 1 2000 0:00 +24:00",
            "Jan 1 2000 0:00 -01:60",
            "Jan 1 2000 0:00:00.1234567890",
            "Jan 1 2000\0",
            "Jan 1 2000\u{d7ff}",
        ] {
            let units: Vec<u16> = text.encode_utf16().collect();
            let mut parser = FallbackParser::default();
            assert!(
                (0..10000)
                    .find_map(|_| parser.advance(&units, 1))
                    .expect("bounded fallback parse")
                    .is_none(),
                "{text}"
            );
        }
    }

    #[test]
    fn date_fallback_scan_budget() {
        for text in [
            format!("{} Jan 1 2000", "weekday".repeat(2000)),
            format!("Jan 1 2000 ({}x{})", "(".repeat(5000), ")".repeat(5000)),
            format!("Jan 1 {}2000", "0".repeat(10000)),
            format!("Jan 1 2000 0:00 +{}0100", "0".repeat(10000)),
            format!("{}Jan 1 2000", " ".repeat(10000)),
        ] {
            let units: Vec<u16> = text.encode_utf16().collect();
            let mut parser = FallbackParser::default();
            assert!(parser.advance(&units, 256).is_none());
            assert!(parser.next <= 256 + 4);
            let fields = (0..units.len() * 4)
                .find_map(|_| parser.advance(&units, 1))
                .expect("bounded fallback parse")
                .expect("valid long input");
            assert_eq!(fields.fields, [2000, 0, 1, 0, 0, 0, 0]);
        }
    }

    // ISO rows adapted from the same QuickJS test_date function credited
    // with the complete copyright/permission notice below (date_time_arithmetic).
    #[test]
    fn date_iso_parse_fields() {
        for (text, expected) in [
            ("0000", -62167219200000.0),
            ("0001", -62135596800000.0),
            ("0099", -59042995200000.0),
            ("+000000-01-01T00:00Z", -62167219200000.0),
            ("2000", 946684800000.0),
            ("2000-01", 946684800000.0),
            ("2000-01-01", 946684800000.0),
            ("2000-01-01T00:00Z", 946684800000.0),
            ("2000-01-01T00:00:00Z", 946684800000.0),
            ("2000-01-01T00:00:00+00:00", 946684800000.0),
            ("2000-01-01T00:00:00.1Z", 946684800100.0),
            ("2000-01-01T00:00:00.10Z", 946684800100.0),
            ("2000-01-01T00:00:00.100Z", 946684800100.0),
            ("2000-01-01T00:00:00.1000Z", 946684800100.0),
            ("2000T00:00", 946684800000.0),
            ("2017-09-22T18:10:11.091Z", 1506103811091.0),
            ("+002000-01-01T00:00:00Z", 946684800000.0),
            ("-000001-01-01T00:00:00Z", -62198755200000.0),
            ("2000-01-01T00:00:00.123456789Z", 946684800123.0),
            ("2020-01-01T01:01:01.123Z", 1577840461123.0),
            ("2020-01-01T01:01:01.1Z", 1577840461100.0),
            ("2020-01-01T01:01:01.12Z", 1577840461120.0),
            ("2020-01-01T01:01:01.1234Z", 1577840461123.0),
            ("2020-01-01T01:01:01.12345Z", 1577840461123.0),
            ("2020-01-01T01:01:01.1235Z", 1577840461123.0),
            ("2020-01-01T01:01:01.9999Z", 1577840461999.0),
        ] {
            let units: Vec<u16> = text.encode_utf16().collect();
            let mut parser = IsoParser::default();
            let fields = loop {
                if let Some(result) = parser.advance(&units, 1) {
                    break result.expect(text);
                }
            };
            assert_eq!(fields.is_local, text == "2000T00:00", "{text}");
            assert_eq!(fields.time(crate::host::TimeZone::UTC), expected, "{text}");
        }
        for text in [
            "",
            "-000000-01-01T00:00:00Z",
            "2000-13",
            "2000-01-01T25:00Z",
            "2000-01-01T24:00:01Z",
        ] {
            let units: Vec<u16> = text.encode_utf16().collect();
            let mut parser = IsoParser::default();
            assert!(parser.advance(&units, 256).unwrap().is_none(), "{text}");
        }
    }

    #[test]
    fn date_iso_parse_budget() {
        // Synthetic input beyond the existing numeric conversion limit. Each
        // advance validates at most its allotted fractional digits.
        let mut units: Vec<u16> = "2000-01-01T00:00:00.123".encode_utf16().collect();
        units.extend(std::iter::repeat_n(u16::from(b'9'), 8192));
        units.push(u16::from(b'Z'));
        let mut parser = IsoParser::default();
        let mut steps = 0;
        let fields = loop {
            let before = parser.next;
            let result = parser.advance(&units, 256);
            if steps > 0 {
                assert!(parser.next - before <= 256);
            }
            steps += 1;
            if let Some(result) = result {
                break result.unwrap();
            }
        };
        assert_eq!(steps, 33);
        assert_eq!(fields.time(crate::host::TimeZone::UTC), 946684800123.0);
        units.push(0xd800);
        let mut parser = IsoParser::default();
        loop {
            if let Some(result) = parser.advance(&units, 256) {
                assert!(result.is_none());
                break;
            }
        }
    }

    #[test]
    fn date_iso_rope_budget() {
        // Synthetic unflattened append chain. Traversal as well as copying
        // must yield, even before the first calendar field can be scanned.
        let mut text = crate::value::JsString::from("2000-01-01T00:00:00.123");
        let zero = crate::value::JsString::from("0");
        for _ in 0..8192 {
            text = text.concat(&zero).unwrap();
        }
        text = text.concat(&crate::value::JsString::from("Z")).unwrap();
        let mut parser = Parser::new(text).ok().expect("input allocation");
        let before = crate::value::JsString::copied_code_units();
        assert!(parser.advance(1).is_none());
        assert!(crate::value::JsString::copied_code_units() - before <= 1);
        for _ in 0..50000 {
            let before = crate::value::JsString::copied_code_units();
            let result = parser.advance(17);
            assert!(crate::value::JsString::copied_code_units() - before <= 17);
            if let Some(result) = result {
                assert_eq!(
                    result.unwrap().time(crate::host::TimeZone::UTC),
                    946684800123.0
                );
                return;
            }
        }
        panic!("bounded rope parse did not complete");
    }

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
