//! Gregorian component and style formatting using the pinned QuickJS pools.
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
    Hour,
    Minute,
    Second,
    Fraction,
    DayPeriod,
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
    pub hour: Option<&'static str>,
    pub minute: Option<&'static str>,
    pub second: Option<&'static str>,
    pub fractional_second_digits: Option<u8>,
    pub day_period: Option<&'static str>,
    pub hour_cycle: &'static str,
    pub date_style: Option<&'static str>,
    pub time_style: Option<&'static str>,
    pub time_zone_name: Option<&'static str>,
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
        // Use the snapshot's short time pattern for the locale default. Styles
        // themselves are separate: manual components use QuickJS's layout.
        let default_cycle = super::table_string(
            dates::time_pattern_tab,
            usize::from(data.timePatternShort_idx),
        )
        .and_then(|pattern| pattern.chars().find(|c| "hHKk".contains(*c)))
        .map(|hour| match hour {
            'K' => "h11",
            'h' => "h12",
            'k' => "h24",
            _ => "h23",
        })
        .expect("snapshot short time pattern contains an hour");
        let hour_cycle = tag
            .keywords
            .get("hc")
            .and_then(|cycle| match cycle.as_str() {
                "h11" => Some("h11"),
                "h12" => Some("h12"),
                "h23" => Some("h23"),
                "h24" => Some("h24"),
                _ => None,
            })
            .unwrap_or(default_cycle);
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
            hour: None,
            minute: None,
            second: None,
            fractional_second_digits: None,
            day_period: None,
            hour_cycle,
            date_style: None,
            time_style: None,
            time_zone_name: None,
            pattern: vec![],
        }
    }
    pub fn finish(&mut self) -> Result<(), Fault> {
        if self.date_style.is_some() || self.time_style.is_some() {
            if [
                self.weekday,
                self.era,
                self.year,
                self.month,
                self.day,
                self.day_period,
                self.hour,
                self.minute,
                self.second,
                self.time_zone_name,
            ]
            .iter()
            .any(Option::is_some)
                || self.fractional_second_digits.is_some()
            {
                return Err(Fault::TypeError(
                    "DateTimeFormat styles cannot be combined with component fields".into(),
                ));
            }
            return Ok(());
        }
        if self.time_zone_name.is_some() {
            return Err(Fault::RangeError(
                "DateTimeFormat timeZoneName formatting is not supported yet".into(),
            ));
        }
        // Era alone does not suppress ToDateTimeOptions' numeric date defaults.
        if self.weekday.is_none()
            && self.year.is_none()
            && self.month.is_none()
            && self.day.is_none()
            && self.hour.is_none()
            && self.minute.is_none()
            && self.second.is_none()
            && self.fractional_second_digits.is_none()
            && self.day_period.is_none()
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
        let has_time = self.hour.is_some() || self.minute.is_some() || self.second.is_some();
        if has_time || self.fractional_second_digits.is_some() {
            if !p.is_empty() {
                p.push(L(", "));
            }
            // QuickJS's component layout uses colons, including Japanese and
            // locales whose style patterns put the day period before the hour.
            let mut has_field = false;
            for (option, field) in [
                (self.hour, Hour),
                (self.minute, Minute),
                (self.second, Second),
            ] {
                if option.is_some() {
                    if has_field {
                        p.push(L(":"));
                    }
                    p.push(F(field));
                    has_field = true;
                }
            }
            if self.fractional_second_digits.is_some() {
                if has_field {
                    p.push(L(if self.numbering == "arab" { "٫" } else { "." }));
                }
                p.push(F(Fraction));
            }
        }
        if self.day_period.is_some() || (self.hour.is_some() && self.is_12_hour()) {
            if !p.is_empty() {
                p.push(L(if self.locale == "en-US" {
                    "\u{202f}"
                } else {
                    " "
                }));
            }
            p.push(F(DayPeriod));
        }
        self.pattern = p;
        Ok(())
    }
    fn is_12_hour(&self) -> bool {
        matches!(self.hour_cycle, "h11" | "h12")
    }
    fn style_pattern(&self, style: &str, kind: &str) -> &'static str {
        let d = self.data;
        let (table, indices) = match kind {
            "date" => (
                dates::date_pattern_tab,
                [
                    d.datePatternFull_idx,
                    d.datePatternLong_idx,
                    d.datePatternMedium_idx,
                    d.datePatternShort_idx,
                ]
                .map(usize::from),
            ),
            "time" => (
                dates::time_pattern_tab,
                [
                    d.timePatternFull_idx,
                    d.timePatternLong_idx,
                    d.timePatternMedium_idx,
                    d.timePatternShort_idx,
                ]
                .map(usize::from),
            ),
            _ => (
                dates::combined_pattern_tab,
                [
                    d.combinedPatternFull_idx,
                    d.combinedPatternLong_idx,
                    d.combinedPatternMedium_idx,
                    d.combinedPatternShort_idx,
                ]
                .map(usize::from),
            ),
        };
        let index = match style {
            "full" => 0,
            "long" => 1,
            "medium" => 2,
            _ => 3,
        };
        super::table_string(table, indices[index]).expect("validated snapshot pattern")
    }
    fn format_styles(&self, fields: &date::UtcFields, time: f64) -> String {
        let date = self.date_style.map(|style| {
            self.format_cldr(self.style_pattern(style, "date"), fields, time, ["", ""])
        });
        let clock = self.time_style.map(|style| {
            self.format_cldr(self.style_pattern(style, "time"), fields, time, ["", ""])
        });
        match (date, clock) {
            (Some(date), Some(clock)) => self.format_cldr(
                self.style_pattern(self.date_style.unwrap(), "combined"),
                fields,
                time,
                [&clock, &date],
            ),
            (Some(text), None) | (None, Some(text)) => text,
            _ => unreachable!("style format requires a style"),
        }
    }
    /// Interpret only trusted, bounded snapshot patterns. Widths and literal
    /// handling follow QuickJS's pattern path, independently of manual fields.
    fn format_cldr(
        &self,
        pattern: &str,
        fields: &date::UtcFields,
        time: f64,
        subformats: [&str; 2],
    ) -> String {
        let mut result = String::new();
        let mut chars = pattern.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                    result.push('\'');
                } else {
                    for literal in chars.by_ref() {
                        if literal == '\'' {
                            break;
                        }
                        result.push(literal);
                    }
                }
                continue;
            }
            if c == '{' {
                let mut lookahead = chars.clone();
                if let Some(digit @ ('0' | '1')) = lookahead.next()
                    && lookahead.next() == Some('}')
                {
                    result.push_str(subformats[usize::from(digit == '1')]);
                    chars = lookahead;
                    continue;
                }
            }
            if !c.is_ascii_alphabetic() {
                result.push(c);
                continue;
            }
            let mut width = 1;
            while chars.peek() == Some(&c) {
                chars.next();
                width += 1;
            }
            let value = match c {
                'y' => {
                    let year = if fields.year > 0 {
                        fields.year
                    } else {
                        1 - fields.year
                    };
                    if width == 2 { year % 100 } else { year }
                }
                'M' if width <= 2 => i32::from(fields.month) + 1,
                'd' => i32::from(fields.date),
                'h' | 'H' | 'K' | 'k' => {
                    let hour = i32::from(fields.hours);
                    match self.hour_cycle {
                        "h11" => hour % 12,
                        "h12" => (hour + 11) % 12 + 1,
                        "h24" if hour == 0 => 24,
                        _ => hour,
                    }
                }
                'm' => i32::from(fields.minutes),
                's' => i32::from(fields.seconds),
                'M' | 'E' | 'G' => {
                    let (field, name_width) = match c {
                        'M' => (Field::Month, if width == 3 { "short" } else { "long" }),
                        'E' => (
                            Field::Weekday,
                            if width <= 3 {
                                "short"
                            } else if width == 4 {
                                "long"
                            } else {
                                "narrow"
                            },
                        ),
                        _ => (
                            Field::Era,
                            if width <= 3 {
                                "short"
                            } else if width == 4 {
                                "long"
                            } else {
                                "narrow"
                            },
                        ),
                    };
                    result.push_str(
                        self.name(field, fields, Some(name_width))
                            .expect("snapshot field name"),
                    );
                    continue;
                }
                'a' | 'b' | 'B' => {
                    if self.is_12_hour() {
                        let name = if c == 'B' {
                            let item = match fields.hours {
                                6..=11 => 0,
                                12 => 1,
                                13..=17 => 2,
                                18..=20 => 3,
                                _ => 4,
                            };
                            pool_string(
                                dates::dayperiod_tab[usize::from(self.data.dayperiod_idx)].unwrap(),
                                item,
                            )
                        } else {
                            self.name(Field::DayPeriod, fields, None)
                        };
                        result.push_str(name.expect("snapshot day period"));
                    } else if result.ends_with([' ', '\u{202f}']) {
                        // QuickJS removes one preceding separator, including
                        // its narrow no-break space, when suppressing AM/PM.
                        result.pop();
                    }
                    continue;
                }
                'z' | 'Z' | 'v' | 'V' => {
                    result.push_str(&self.style_zone_name(time, width >= 4));
                    continue;
                }
                _ => {
                    result.extend(std::iter::repeat_n(c, width));
                    continue;
                }
            };
            let text = if width == 2 {
                format!("{value:02}")
            } else {
                value.to_string()
            };
            result.push_str(&number_format::map_digits(self.numbering, &text));
        }
        result
    }
    fn style_zone_name(&self, time: f64, long: bool) -> String {
        // Snapshot style patterns use English UTC/Vienna names, raw explicit
        // offsets, and GMT offsets for the remaining embedded named zones.
        if self.time_zone == "UTC" {
            return if long {
                "Coordinated Universal Time"
            } else {
                "UTC"
            }
            .into();
        }
        if self.time_zone.starts_with(['+', '-']) {
            return self.time_zone.clone();
        }
        let offset = self.zone.offset_seconds(date::time_clip(time));
        if long && self.time_zone == "Europe/Vienna" {
            match offset {
                3600 => return "Central European Standard Time".into(),
                7200 => return "Central European Summer Time".into(),
                _ => {}
            }
        }
        // The reference truncates offset seconds for named-zone display.
        let minutes = offset.abs() / 60;
        let sign = if offset < 0 { '-' } else { '+' };
        if minutes == 0 {
            "GMT".into()
        } else if long {
            format!("GMT{sign}{:02}:{:02}", minutes / 60, minutes % 60)
        } else if minutes % 60 == 0 {
            format!("GMT{sign}{}", minutes / 60)
        } else {
            format!("GMT{sign}{}:{:02}", minutes / 60, minutes % 60)
        }
    }
    fn name(
        &self,
        field: Field,
        fields: &date::UtcFields,
        width: Option<&str>,
    ) -> Option<&'static str> {
        let d = self.data;
        let (table, index, item) = match field {
            Field::Month => match width.or(self.month)? {
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
            Field::Weekday => match width.or(self.weekday)? {
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
                match width.or(self.era)? {
                    "long" => 0,
                    "short" => 2,
                    _ => 4,
                } + usize::from(fields.year > 0),
            ),
            Field::DayPeriod => match self.day_period {
                Some(width) => (
                    if width == "narrow" {
                        dates::dayperiod_narrow_tab
                    } else {
                        dates::dayperiod_tab
                    },
                    if width == "narrow" {
                        d.dayperiod_narrow_idx
                    } else {
                        d.dayperiod_idx
                    },
                    // The reference uses these hour boundaries for every
                    // locale's five-entry day-period pool.
                    match fields.hours {
                        6..=11 => 0,
                        12 => 1,
                        13..=17 => 2,
                        18..=20 => 3,
                        _ => 4,
                    },
                ),
                None => (dates::ampm_tab, d.ampm_idx, usize::from(fields.hours >= 12)),
            },
            _ => return None,
        };
        pool_string(table[index as usize]?, item)
    }
    pub fn format(&self, time: f64) -> Result<Value, Fault> {
        let fields = fields_from_time_value(time, self.zone)?;
        if self.date_style.is_some() || self.time_style.is_some() {
            return Ok(Value::String(JsString::from(
                self.format_styles(&fields, time).as_str(),
            )));
        }
        let mut result = String::new();
        for item in &self.pattern {
            match item {
                PatternItem::Literal(text) => result.push_str(text),
                PatternItem::Field(field) => {
                    if let Some(name) = self.name(*field, &fields, None) {
                        result.push_str(name);
                        continue;
                    }
                    let (mut value, width) = match field {
                        Field::Month => (i32::from(fields.month) + 1, self.month),
                        Field::Day => (i32::from(fields.date), self.day),
                        Field::Hour => {
                            let h = i32::from(fields.hours);
                            let value = match self.hour_cycle {
                                "h11" => h % 12,
                                "h12" => (h + 11) % 12 + 1,
                                "h24" if h == 0 => 24,
                                _ => h,
                            };
                            (
                                value,
                                if !self.is_12_hour()
                                    && (self.minute.is_some() || self.second.is_some())
                                {
                                    Some("2-digit")
                                } else {
                                    self.hour
                                },
                            )
                        }
                        Field::Minute => (
                            i32::from(fields.minutes),
                            if self.hour.is_some() || self.second.is_some() {
                                Some("2-digit")
                            } else {
                                self.minute
                            },
                        ),
                        Field::Second => (
                            i32::from(fields.seconds),
                            if self.minute.is_some() {
                                Some("2-digit")
                            } else {
                                self.second
                            },
                        ),
                        Field::Fraction => {
                            let digits = self.fractional_second_digits.expect("fraction pattern");
                            let value = fields.milliseconds / 10_u16.pow(u32::from(3 - digits));
                            let text = format!("{value:0width$}", width = usize::from(digits));
                            result.push_str(&number_format::map_digits(self.numbering, &text));
                            continue;
                        }
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
        if self.hour.is_some() || self.time_style.is_some() {
            fields.push(("hourCycle", string(self.hour_cycle)));
            fields.push(("hour12", Value::Bool(self.is_12_hour())));
        }
        if self.date_style.is_some() || self.time_style.is_some() {
            for (name, style) in [
                ("dateStyle", self.date_style),
                ("timeStyle", self.time_style),
            ] {
                if let Some(style) = style {
                    fields.push((name, string(style)));
                }
            }
            return service::resolved_options(heap, task, fields);
        }
        for (name, value) in [
            ("weekday", self.weekday),
            ("era", self.era),
            ("year", self.year),
            ("month", self.month),
            ("day", self.day),
            ("dayPeriod", self.day_period),
            ("hour", self.hour),
            ("minute", self.minute),
            ("second", self.second),
        ] {
            if let Some(value) = value {
                fields.push((name, string(value)));
            }
        }
        if let Some(digits) = self.fractional_second_digits {
            fields.push(("fractionalSecondDigits", Value::Number(f64::from(digits))));
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
    fn cldr_date_pattern_formatter() {
        let fields = fields_from_time_value(1673740800000.0, TimeZone::UTC)
            .ok()
            .unwrap();
        for (locale, pattern, expected) in [
            ("en", "M/d/yy", "1/15/23"),
            ("de", "dd.MM.yy", "15.01.23"),
            ("de", "d. MMMM y", "15. Januar 2023"),
            ("de", "EEEE, d. MMMM y", "Sonntag, 15. Januar 2023"),
            ("en", "EEEE, MMM d, y", "Sunday, Jan 15, 2023"),
            ("en", "'day' d '' 'month' MM", "day 15 ' month 01"),
        ] {
            let format = DateTimeFormat::new(locale.into(), super::super::lookup(locale).unwrap());
            assert_eq!(
                format.format_cldr(pattern, &fields, 1673740800000.0, ["", ""]),
                expected
            );
        }
    }

    #[test]
    fn cldr_patterns_cover_every_supported_locale_and_style() {
        for locale in super::super::LOCALES {
            for style in ["full", "long", "medium", "short"] {
                let mut format =
                    DateTimeFormat::new((*locale).into(), super::super::lookup(locale).unwrap());
                format.date_style = Some(style);
                format.time_style = Some(style);
                assert!(format.finish().is_ok());
                let Value::String(text) = format.format(1673740800000.0).ok().unwrap() else {
                    panic!("string expected")
                };
                assert!(!text.is_empty(), "{locale} {style}");
            }
        }
    }
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
