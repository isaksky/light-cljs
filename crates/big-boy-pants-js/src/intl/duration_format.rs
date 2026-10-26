//! Immutable DurationFormat slots and bounded duration composition. Unit
//! patterns and list separators reproduce the pinned QuickJS snapshot.
use super::{
    list_format::ListFormat, number_format::NumberFormat, plural_rules::PluralRules, service,
};
use crate::{
    heap::{Heap, HeapObject},
    ids::TaskId,
    value::{JsString, Value},
    vm::{dispatch::Fault, property},
};

pub(crate) const UNITS: [&str; 10] = [
    "years",
    "months",
    "weeks",
    "days",
    "hours",
    "minutes",
    "seconds",
    "milliseconds",
    "microseconds",
    "nanoseconds",
];
pub(crate) const SINGULAR: [&str; 10] = [
    "year",
    "month",
    "week",
    "day",
    "hour",
    "minute",
    "second",
    "millisecond",
    "microsecond",
    "nanosecond",
];
pub(crate) const DISPLAYS: [&str; 10] = [
    "yearsDisplay",
    "monthsDisplay",
    "weeksDisplay",
    "daysDisplay",
    "hoursDisplay",
    "minutesDisplay",
    "secondsDisplay",
    "millisecondsDisplay",
    "microsecondsDisplay",
    "nanosecondsDisplay",
];
// ToDurationRecord observes properties alphabetically, independently of output order.
pub(crate) const READ_ORDER: [usize; 10] = [3, 4, 8, 7, 5, 1, 9, 6, 2, 0];

#[derive(Clone, Debug)]
pub(crate) struct DurationFormat {
    pub digits: NumberFormat,
    pub style: &'static str,
    pub units: [&'static str; 10],
    pub always: [bool; 10],
    pub fractional: Option<usize>,
}
impl DurationFormat {
    pub fn new(locale: String, data: &'static super::LocaleData) -> Self {
        Self {
            digits: NumberFormat::new(locale, data),
            style: "short",
            units: ["short"; 10],
            always: [false; 10],
            fractional: None,
        }
    }
    pub fn unit_style(&mut self, index: usize, explicit: Option<&'static str>) {
        let previous = index.checked_sub(1).map(|i| self.units[i]);
        let numeric_previous = matches!(previous, Some("numeric" | "2-digit" | "fractional"));
        let style = explicit.unwrap_or_else(|| {
            if self.style == "digital" {
                if index < 4 { "short" } else { "numeric" }
            } else if numeric_previous {
                "numeric"
            } else {
                self.style
            }
        });
        self.always[index] = explicit.is_some()
            || ((self.style == "digital" || numeric_previous) && (4..=6).contains(&index));
        self.units[index] = if index >= 7 && style == "numeric" {
            self.always[index] = false;
            "fractional"
        } else {
            style
        };
    }
    pub fn finish_unit(&mut self, index: usize) -> Result<(), Fault> {
        let style = self.units[index];
        let previous = index.checked_sub(1).map(|i| self.units[i]);
        if (style == "fractional" && self.always[index])
            || (previous == Some("fractional") && style != "fractional")
            || (matches!(previous, Some("numeric" | "2-digit"))
                && !matches!(style, "numeric" | "2-digit" | "fractional"))
        {
            return Err(Fault::RangeError(
                "conflicting duration unit options".into(),
            ));
        }
        if (index == 5 || index == 6) && matches!(previous, Some("numeric" | "2-digit")) {
            self.units[index] = "2-digit";
        }
        Ok(())
    }
    pub fn resolved(&self, heap: &Heap, task: TaskId) -> Result<Value, Fault> {
        let mut fields = vec![
            ("locale", service::text_option(heap, &self.digits.locale)?),
            (
                "numberingSystem",
                service::fixed_option(self.digits.numbering),
            ),
            ("style", service::fixed_option(self.style)),
        ];
        for i in 0..10 {
            fields.push((
                UNITS[i],
                service::fixed_option(if self.units[i] == "fractional" {
                    "numeric"
                } else {
                    self.units[i]
                }),
            ));
            fields.push((
                DISPLAYS[i],
                service::fixed_option(if self.always[i] { "always" } else { "auto" }),
            ));
        }
        if let Some(n) = self.fractional {
            fields.push(("fractionalDigits", Value::Number(n as f64)));
        }
        service::resolved_options(heap, task, fields)
    }
    pub fn format(
        &self,
        heap: &Heap,
        task: TaskId,
        values: &[f64; 10],
        to_parts: bool,
    ) -> Result<Value, Fault> {
        validate(values)?;
        let negative = values.iter().any(|&n| n < 0.0);
        let mut groups: Vec<Vec<Part>> = Vec::new();
        let mut numeric = false;
        for i in 0..10 {
            let style = self.units[i];
            let combine = (6..9).contains(&i) && self.units[i + 1] == "fractional";
            let required =
                i == 5 && numeric && (self.always[6] || values[6..].iter().any(|&n| n != 0.0));
            if !(values[i] != 0.0
                || self.always[i]
                || required
                || combine && values[i + 1..].iter().any(|&n| n != 0.0))
            {
                continue;
            }
            let mut digits = self.digits.clone();
            let is_numeric = matches!(style, "numeric" | "2-digit");
            if is_numeric {
                digits.grouping = "false";
            }
            if style == "2-digit" {
                digits.min_integer = 2;
            }
            let magnitude = values[i].abs();
            let mut decimal = if combine {
                // Inputs are integral binary64 values; u128 retains their exact
                // represented integer before addition. Validation bounds this sum.
                let mut total = 0u128;
                for (j, value) in values.iter().enumerate().skip(i) {
                    total += (value.abs() as u128) * 1000u128.pow((9 - j) as u32);
                }
                let exponent = 3 * (9 - i);
                let divisor = 10u128.pow(exponent as u32);
                digits.min_fraction = self.fractional.unwrap_or(0);
                digits.max_fraction = self.fractional.unwrap_or(9);
                digits.rounding = "trunc";
                format!(
                    "{}.{:0width$}",
                    total / divisor,
                    total % divisor,
                    width = exponent
                )
            } else {
                crate::value::string::number_to_string(magnitude)
            };
            let plural_value = decimal.parse::<f64>().expect("bounded duration decimal");
            if groups.is_empty() && negative {
                decimal.insert(0, '-');
            }
            let mut parts: Vec<Part> = digits
                // A transient operand: only the Rust part texts survive.
                .decimal_parts(&Value::String(JsString::from(decimal.as_str())))?
                .into_iter()
                .map(|(kind, text)| Part {
                    kind,
                    text,
                    unit: Some(SINGULAR[i]),
                })
                .collect();
            if !is_numeric && style != "fractional" {
                let mut plural_digits = self.digits.clone();
                plural_digits.max_fraction = 9;
                let one = PluralRules {
                    digits: plural_digits,
                    ordinal: false,
                }
                .select(plural_value)?
                    == "one";
                let pattern = unit_pattern(&self.digits.locale, SINGULAR[i], style, one);
                let (prefix, suffix) = pattern
                    .split_once("{number}")
                    .expect("pinned unit placeholder");
                let mut full = Vec::new();
                affix(&mut full, prefix, SINGULAR[i], true);
                full.append(&mut parts);
                affix(&mut full, suffix, SINGULAR[i], false);
                parts = full;
            }
            if numeric {
                let group = groups.last_mut().unwrap();
                group.push(Part {
                    kind: "literal",
                    text: ":".into(),
                    unit: None,
                });
                group.extend(parts);
            } else {
                groups.push(parts);
            }
            numeric |= is_numeric;
            if combine {
                break;
            }
        }
        let list = ListFormat {
            locale: self.digits.locale.split("-u-").next().unwrap().into(),
            kind: "unit",
            style: if self.style == "digital" {
                "short"
            } else {
                self.style
            },
        };
        let mut parts = Vec::new();
        let count = groups.len();
        for (i, group) in groups.iter_mut().enumerate() {
            if i > 0 {
                parts.push(Part {
                    kind: "literal",
                    text: list.separator(count, i).into(),
                    unit: None,
                });
            }
            parts.append(group);
        }
        // The formatted text is data-dependent and charged; part types and
        // units are fixed spellings with one shared body each.
        if !to_parts {
            return service::text_option(
                heap,
                &parts.into_iter().map(|p| p.text).collect::<String>(),
            );
        }
        let result = property::new_array(heap, task)?;
        for part in parts {
            let mut fields = vec![
                ("type", service::fixed_option(part.kind)),
                ("value", service::text_option(heap, &part.text)?),
            ];
            if let Some(unit) = part.unit {
                fields.push(("unit", service::fixed_option(unit)));
            }
            let item = service::resolved_options(heap, task, fields)?;
            property::append(heap, task, &result, Some(item))?;
        }
        Ok(result)
    }
}
#[derive(Debug)]
struct Part {
    kind: &'static str,
    text: String,
    unit: Option<&'static str>,
}
fn affix(parts: &mut Vec<Part>, text: &str, unit: &'static str, prefix: bool) {
    let (a, b) = if prefix {
        let trimmed = text.trim_end_matches(' ');
        (trimmed, &text[trimmed.len()..])
    } else {
        let trimmed = text.trim_start_matches(' ');
        (&text[..text.len() - trimmed.len()], trimmed)
    };
    for (text, literal) in [(a, !prefix), (b, prefix)] {
        if !text.is_empty() {
            parts.push(Part {
                kind: if literal { "literal" } else { "unit" },
                text: text.into(),
                unit: Some(unit),
            });
        }
    }
}
fn unit_pattern(locale: &str, unit: &str, style: &str, one: bool) -> &'static str {
    use super::data::units::nf_unit_patterns;
    let text = |s| super::pool_string(s, 0).unwrap();
    let locale = locale.split("-u-").next().unwrap();
    let rows = || nf_unit_patterns.chunks_exact(8);
    let row = rows()
        .find(|r| text(r[0].unwrap()) == locale && text(r[1].unwrap()) == unit)
        .or_else(|| {
            rows().find(|r| {
                let prefix = text(r[0].unwrap());
                locale
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
                    && text(r[1].unwrap()) == unit
            })
        });
    let index = match style {
        "narrow" => 4,
        "long" => 6,
        _ => 2,
    };
    row.and_then(|r| {
        if one {
            r[index + 1].or(r[index])
        } else {
            r[index]
        }
    })
    .map(text)
    .unwrap_or_else(|| fallback_pattern(unit))
}
fn fallback_pattern(unit: &str) -> &'static str {
    match unit {
        "year" => "{number} year",
        "month" => "{number} month",
        "week" => "{number} week",
        "day" => "{number} day",
        "hour" => "{number} hour",
        "minute" => "{number} minute",
        "second" => "{number} second",
        "millisecond" => "{number} millisecond",
        "microsecond" => "{number} microsecond",
        _ => "{number} nanosecond",
    }
}
fn validate(values: &[f64; 10]) -> Result<(), Fault> {
    let limits = [
        4294967296.0,
        4294967296.0,
        4294967296.0,
        104249991375.0,
        2501999792984.0,
        150119987579017.0,
        9007199254740992.0,
        9007199254740992000.0,
        9007199254740992000000.0,
        9007199254740992000000000.0,
    ];
    let positive = values.iter().any(|&n| n > 0.0);
    let negative = values.iter().any(|&n| n < 0.0);
    if positive && negative
        || values
            .iter()
            .zip(limits)
            .any(|(&n, limit)| !n.is_finite() || n.fract() != 0.0 || n.abs() >= limit)
    {
        return Err(Fault::RangeError("invalid duration".into()));
    }
    let mut total = 0u128;
    for (value, scale) in values[3..].iter().zip([
        86400000000000u128,
        3600000000000,
        60000000000,
        1000000000,
        1000000,
        1000,
        1,
    ]) {
        total += (value.abs() as u128) * scale;
    }
    if total >= 9007199254740992u128 * 1000000000 {
        return Err(Fault::RangeError("duration out of range".into()));
    }
    Ok(())
}
pub(crate) fn record(heap: &Heap, task: TaskId, value: &Value) -> Result<DurationFormat, Fault> {
    let invalid = || Fault::TypeError("DurationFormat receiver required".into());
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
            o.duration_format().cloned().ok_or_else(invalid)
        }
        _ => Err(invalid()),
    })
    .ok_or(Fault::Internal("DurationFormat reclaimed"))?
}
