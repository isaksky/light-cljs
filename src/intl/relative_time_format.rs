//! Relative-time patterns from the pinned QuickJS service snapshot.
use super::{
    data::relative_time as data, number_format::NumberFormat, plural_rules::PluralRules, service,
};
use crate::{
    heap::{Heap, HeapObject},
    ids::TaskId,
    value::{JsString, Value},
    vm::{dispatch::Fault, property},
};

pub(crate) const UNITS: &[&str] = &[
    "second", "minute", "hour", "day", "week", "month", "quarter", "year",
];
#[derive(Clone, Debug)]
pub(crate) struct RelativeTimeFormat {
    pub digits: NumberFormat,
    pub style: &'static str,
    pub numeric: &'static str,
}
impl RelativeTimeFormat {
    pub fn new(locale: String, data: &'static super::LocaleData) -> Self {
        Self {
            digits: NumberFormat::new(locale, data),
            style: "long",
            numeric: "always",
        }
    }
    pub fn resolved(&self, heap: &Heap, task: TaskId) -> Result<Value, Fault> {
        service::resolved_options(
            heap,
            task,
            [
                ("locale", string(&self.digits.locale)),
                ("style", string(self.style)),
                ("numeric", string(self.numeric)),
                ("numberingSystem", string(self.digits.numbering)),
            ],
        )
    }
    fn parts(&self, value: f64, unit: &str) -> Result<Vec<(&'static str, String)>, Fault> {
        let index = UNITS
            .iter()
            .position(|&u| u == unit)
            .ok_or_else(|| Fault::RangeError("invalid relative-time unit".into()))?;
        let language = self.digits.locale.split('-').next().unwrap();
        if self.numeric == "auto" && value.fract() == 0.0 && (-2.0..=2.0).contains(&value) {
            let row = data::rtf_auto_locales
                .iter()
                .position(|&s| text(s) == Some(language))
                .unwrap_or_else(|| {
                    data::rtf_auto_locales
                        .iter()
                        .position(|&s| text(s) == Some("en"))
                        .unwrap()
                });
            if let Some(s) =
                text(data::rtf_auto[row * 40 + (value as i32 + 2) as usize * 8 + index])
            {
                return Ok(vec![("literal", s.to_owned())]);
            }
        }
        let row = data::rtf_units
            .chunks_exact(37)
            .find(|r| text(r[0]) == Some(language))
            .unwrap_or(&data::rtf_units[..37]);
        let one = PluralRules {
            digits: self.digits.clone(),
            ordinal: false,
        }
        .select(value.abs())?
            == "one";
        let unit_form = if self.style == "narrow" && language == "en" {
            text(data::rtf_en_narrow[index]).unwrap()
        } else {
            let offset = if self.style == "long" { 1 } else { 17 };
            text(row[offset + usize::from(!one) * 8 + index]).unwrap()
        };
        let future = !value.is_sign_negative();
        let prefix = text(row[if future { 33 } else { 35 }]).unwrap();
        let suffix = text(row[if future { 34 } else { 36 }]).unwrap();
        let mut parts = Vec::new();
        if !prefix.is_empty() {
            parts.push(("literal", prefix.to_owned()));
        }
        parts.extend(self.digits.unsigned_decimal_parts(value.abs())?);
        parts.push(("literal", format!(" {unit_form}{suffix}")));
        Ok(parts)
    }
    pub fn format(
        &self,
        heap: &Heap,
        task: TaskId,
        value: f64,
        unit: &str,
        to_parts: bool,
    ) -> Result<Value, Fault> {
        let unit = unit.strip_suffix('s').unwrap_or(unit);
        let parts = self.parts(value, unit)?;
        if !to_parts {
            return Ok(string(
                &parts.into_iter().map(|(_, s)| s).collect::<String>(),
            ));
        }
        let result = property::new_array(heap, task)?;
        for (kind, value) in parts {
            let mut fields = vec![("type", string(kind)), ("value", string(&value))];
            if kind != "literal" {
                fields.push(("unit", string(unit)));
            }
            let part = service::resolved_options(heap, task, fields)?;
            property::append(heap, task, &result, Some(part))?;
        }
        Ok(result)
    }
}
fn text(bytes: Option<&'static [u8]>) -> Option<&'static str> {
    super::pool_string(bytes?, 0)
}
fn string(text: &str) -> Value {
    Value::String(JsString::from(text))
}
pub(crate) fn record(
    heap: &Heap,
    task: TaskId,
    value: &Value,
) -> Result<RelativeTimeFormat, Fault> {
    let invalid = || Fault::TypeError("RelativeTimeFormat receiver required".into());
    let Value::Object(id) = value else {
        return Err(invalid());
    };
    heap.read(*id, |object| match object {
        HeapObject::Plain(o) => {
            if o.owner().is_some_and(|owner| owner != task) {
                return Err(Fault::ForeignObject {
                    owner: o.owner(),
                    caller: task,
                });
            }
            o.relative_time_format().cloned().ok_or_else(invalid)
        }
        _ => Err(invalid()),
    })
    .ok_or(Fault::Internal("RelativeTimeFormat reclaimed"))?
}
