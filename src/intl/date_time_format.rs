//! Gregorian component formatting using the pinned QuickJS name pools and
//! component layout. Style pattern pools remain in data::dates for -217.
use super::{LocaleData, data::dates, number_format, pool_string, service};
use crate::{
    heap::{Heap, HeapObject},
    host::TimeZone,
    ids::TaskId,
    value::{JsString, Value},
    vm::{date, dispatch::Fault},
};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Field {
    Weekday,
    Month,
    Day,
    Year,
    Era,
}
#[derive(Clone, Debug)]
pub(crate) enum PatternItem {
    Literal(&'static str),
    Field(Field),
}

/// Immutable slots contain owned strings, static tables and a value-only zone.
/// The cached bound function lives on the ordinary object and is traced there.
#[derive(Clone, Debug)]
pub(crate) struct DateTimeFormat {
    pub locale: String,
    pub data: &'static LocaleData,
    pub numbering: &'static str,
    pub zone: TimeZone,
    pub time_zone: String,
    pub weekday: Option<&'static str>,
    pub era: Option<&'static str>,
    pub year: Option<&'static str>,
    pub month: Option<&'static str>,
    pub day: Option<&'static str>,
    pub pattern: Vec<PatternItem>,
}
impl DateTimeFormat {
    pub fn new(locale: String, data: &'static LocaleData) -> Self {
        let tag = super::tag::LanguageTag::parse(&locale).expect("resolved tag");
        let numbering = tag
            .keywords
            .get("nu")
            .and_then(|n| number_format::numbering_system(n))
            .unwrap_or(if tag.language == "ar" { "arab" } else { "latn" });
        Self {
            locale,
            data,
            numbering,
            zone: TimeZone::UTC,
            time_zone: "UTC".into(),
            weekday: None,
            era: None,
            year: None,
            month: None,
            day: None,
            pattern: vec![],
        }
    }
    pub fn finish(&mut self) {
        // Era alone does not suppress ToDateTimeOptions' numeric date defaults.
        if self.weekday.is_none()
            && self.year.is_none()
            && self.month.is_none()
            && self.day.is_none()
        {
            self.year = Some("numeric");
            self.month = Some("numeric");
            self.day = Some("numeric");
        }
        use Field::*;
        use PatternItem::{Field as F, Literal as L};
        let mut p = vec![];
        if self.weekday.is_some() {
            p.push(F(Weekday));
            if self.month.is_some() || self.day.is_some() || self.year.is_some() {
                p.push(L(", "));
            }
        }
        // QuickJS's manual fields deliberately use this layout even for de/fr/ja.
        let slashed = self.locale.split('-').next() == Some("en")
            && self.month.is_some_and(|m| m != "long" && m != "short");
        if self.month.is_some() {
            p.push(F(Month));
            if self.day.is_some() || self.year.is_some() {
                p.push(L(if slashed { "/" } else { " " }));
            }
        }
        if self.day.is_some() {
            p.push(F(Day));
            if self.year.is_some() {
                p.push(L(if slashed { "/" } else { ", " }));
            }
        }
        if self.year.is_some() {
            p.push(F(Year));
            if self.era.is_some() {
                p.extend([L(" "), F(Era)]);
            }
        }
        self.pattern = p;
    }
    fn name(&self, field: Field, fields: &date::UtcFields) -> Option<&'static str> {
        let d = self.data;
        let (table, index, item) = match field {
            Field::Month => match self.month? {
                "long" => (
                    dates::month_long_tab,
                    d.month_long_idx,
                    usize::from(fields.month),
                ),
                "short" => (
                    dates::month_short_tab,
                    d.month_short_idx,
                    usize::from(fields.month),
                ),
                "narrow" => (
                    dates::month_narrow_tab,
                    d.month_narrow_idx,
                    usize::from(fields.month),
                ),
                _ => return None,
            },
            Field::Weekday => match self.weekday? {
                "long" => (
                    dates::weekday_long_tab,
                    d.weekday_long_idx,
                    usize::from(fields.weekday),
                ),
                "short" => (
                    dates::weekday_short_tab,
                    d.weekday_short_idx,
                    usize::from(fields.weekday),
                ),
                _ => (
                    dates::weekday_narrow_tab,
                    d.weekday_narrow_idx,
                    usize::from(fields.weekday),
                ),
            },
            Field::Era => (
                dates::era_tab,
                d.era_idx,
                match self.era? {
                    "long" => 0,
                    "short" => 2,
                    _ => 4,
                } + usize::from(fields.year > 0),
            ),
            _ => return None,
        };
        pool_string(table[index as usize]?, item)
    }
    pub fn format(&self, time: f64) -> Result<Value, Fault> {
        let fields = fields_from_time_value(time, self.zone)?;
        let mut result = String::new();
        for item in &self.pattern {
            match item {
                PatternItem::Literal(text) => result.push_str(text),
                PatternItem::Field(field) => {
                    if let Some(name) = self.name(*field, &fields) {
                        result.push_str(name);
                        continue;
                    }
                    let (mut value, width) = match field {
                        Field::Month => (i32::from(fields.month) + 1, self.month),
                        Field::Day => (i32::from(fields.date), self.day),
                        Field::Year => (
                            if fields.year > 0 {
                                fields.year
                            } else {
                                1 - fields.year
                            },
                            self.year,
                        ),
                        _ => return Err(Fault::Internal("missing date field name")),
                    };
                    if matches!(field, Field::Year) && width == Some("2-digit") {
                        value %= 100;
                    }
                    let text = if width == Some("2-digit") {
                        format!("{value:02}")
                    } else {
                        value.to_string()
                    };
                    result.push_str(&number_format::map_digits(self.numbering, &text));
                }
            }
        }
        Ok(Value::String(JsString::from(result.as_str())))
    }
    pub fn resolved(&self, heap: &Heap, task: TaskId) -> Result<Value, Fault> {
        let string = |s: &str| Value::String(JsString::from(s));
        let mut fields = vec![
            ("locale", string(&self.locale)),
            ("calendar", string("gregory")),
            ("numberingSystem", string(self.numbering)),
            ("timeZone", string(&self.time_zone)),
        ];
        for (name, value) in [
            ("weekday", self.weekday),
            ("era", self.era),
            ("year", self.year),
            ("month", self.month),
            ("day", self.day),
        ] {
            if let Some(value) = value {
                fields.push((name, string(value)));
            }
        }
        service::resolved_options(heap, task, fields)
    }
}
pub(crate) fn fields_from_time_value(time: f64, zone: TimeZone) -> Result<date::UtcFields, Fault> {
    let time = date::time_clip(time);
    if time.is_nan() {
        return Err(Fault::RangeError("invalid time value".into()));
    }
    date::calendar_fields(zone.local_time_millis(time)).ok_or(Fault::Internal("local date fields"))
}

pub(crate) fn explicit_zone(text: &str, host: TimeZone) -> Result<TimeZone, Fault> {
    let invalid = || Fault::RangeError("unsupported or invalid DateTimeFormat timeZone".into());
    if ["UTC", "Etc/UTC", "Etc/GMT", "GMT"]
        .iter()
        .any(|n| n.eq_ignore_ascii_case(text))
    {
        return Ok(TimeZone::UTC);
    }
    let bytes = text.as_bytes();
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        let digits: Vec<u8> = match bytes.len() {
            3 | 5 if bytes[1..].iter().all(u8::is_ascii_digit) => bytes[1..].to_vec(),
            6 if bytes[3] == b':'
                && bytes[1..3]
                    .iter()
                    .chain(&bytes[4..])
                    .all(u8::is_ascii_digit) =>
            {
                [bytes[1], bytes[2], bytes[4], bytes[5]].to_vec()
            }
            _ => return Err(invalid()),
        };
        let hour = i32::from(digits[0] - b'0') * 10 + i32::from(digits[1] - b'0');
        let minute = if digits.len() == 4 {
            i32::from(digits[2] - b'0') * 10 + i32::from(digits[3] - b'0')
        } else {
            0
        };
        if hour > 23 || minute > 59 {
            return Err(invalid());
        }
        return TimeZone::fixed_offset_seconds(
            (hour * 60 + minute) * 60 * if bytes[0] == b'-' { -1 } else { 1 },
        )
        .map_err(|_| invalid());
    }
    if text.eq_ignore_ascii_case(&host.identifier())
        || TimeZone::named_case_insensitive(text).ok() == Some(host)
    {
        return Ok(host);
    }
    Err(invalid())
}
pub(crate) fn record(heap: &Heap, task: TaskId, value: &Value) -> Result<DateTimeFormat, Fault> {
    let invalid = || Fault::TypeError("DateTimeFormat receiver required".into());
    let Value::Object(id) = value else {
        return Err(invalid());
    };
    heap.read(*id, |o| match o {
        HeapObject::Plain(o) => {
            if o.owner().is_some_and(|owner| owner != task) {
                return Err(Fault::ForeignObject {
                    owner: o.owner(),
                    caller: task,
                });
            }
            o.date_time_format().cloned().ok_or_else(invalid)
        }
        _ => Err(invalid()),
    })
    .ok_or(Fault::Internal("DateTimeFormat reclaimed"))?
}

pub(crate) fn get_format(heap: &Heap, task: TaskId, this: &Value) -> Result<Value, Fault> {
    record(heap, task, this)?;
    let Value::Object(id) = this else {
        unreachable!()
    };
    if let Some(cached) = heap
        .read(*id, |o| match o {
            HeapObject::Plain(o) => o.date_time_format_bound().cloned(),
            _ => None,
        })
        .flatten()
    {
        return Ok(cached);
    }
    let realm = crate::realm::intrinsics(heap).ok_or(Fault::OutOfMemory("realm exhausted"))?;
    let mut metadata = crate::object::OrdinaryObject::intrinsic(Some(realm.function_prototype));
    for (name, value) in [
        ("name", Value::String(JsString::from(""))),
        ("length", Value::Number(1.0)),
    ] {
        let key = heap
            .keys()
            .intern(&JsString::from(name))
            .ok_or(Fault::OutOfMemory("keys exhausted"))?;
        metadata.define_with(key, value, crate::object::PropertyFlags::FUNCTION_INFO);
    }
    let metadata = heap
        .alloc(HeapObject::Plain(metadata))
        .ok_or(Fault::OutOfMemory("heap exhausted"))?;
    let bound = heap
        .alloc(HeapObject::BoundFunction(
            crate::vm::function::BoundFunction {
                target: Value::Object(realm.intl_date_time_format_bound),
                this: this.clone(),
                args: Box::new([]),
                owner: task,
                metadata,
                properties: None,
            },
        ))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("heap exhausted"))?;
    heap.with_object_mut(*id, |o| {
        if let HeapObject::Plain(o) = o {
            o.set_date_time_format_bound(bound.clone());
        }
    })
    .ok_or(Fault::Internal("DateTimeFormat reclaimed"))?;
    Ok(bound)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dtf_fields_from_time_value() {
        let local = date::make_date(
            date::make_day(2017.0, 11.0, 12.0),
            date::make_time(6.0, 0.0, 0.0, 0.0),
        );
        for minutes in [0, 60, -300] {
            let zone = TimeZone::fixed_offset_seconds(minutes * 60).unwrap();
            let fields = fields_from_time_value(zone.utc_time_millis(local), zone)
                .ok()
                .unwrap();
            assert_eq!(
                (
                    fields.year,
                    fields.month,
                    fields.date,
                    fields.weekday,
                    fields.hours
                ),
                (2017, 11, 12, 2, 6)
            );
        }
    }
}
