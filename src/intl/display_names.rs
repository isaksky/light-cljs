//! Immutable DisplayNames slots and readers for the verified QuickJS NUL pools.
use super::{data::display_names as data, service};
use crate::{
    heap::{Heap, HeapObject},
    ids::TaskId,
    value::{JsString, Value},
    vm::dispatch::Fault,
};

pub(crate) const TYPES: &[&str] = &[
    "language",
    "region",
    "script",
    "currency",
    "calendar",
    "dateTimeField",
];
pub(crate) const FIELDS: &[&str] = &[
    "era",
    "year",
    "quarter",
    "month",
    "weekOfYear",
    "weekday",
    "day",
    "dayPeriod",
    "hour",
    "minute",
    "second",
    "timeZoneName",
];
#[derive(Clone, Debug)]
pub(crate) struct DisplayNames {
    pub locale: String,
    pub style: &'static str,
    pub kind: &'static str,
    pub fallback: &'static str,
    pub language_display: &'static str,
}
impl DisplayNames {
    pub fn new(locale: String) -> Self {
        Self {
            locale,
            style: "long",
            kind: "region",
            fallback: "code",
            language_display: "dialect",
        }
    }
    pub fn resolved(&self, heap: &Heap, task: TaskId) -> Result<Value, Fault> {
        let mut fields = vec![
            ("locale", string(&self.locale)),
            ("style", string(self.style)),
            ("type", string(self.kind)),
            ("fallback", string(self.fallback)),
        ];
        if self.kind == "language" {
            fields.push(("languageDisplay", string(self.language_display)));
        }
        service::resolved_options(heap, task, fields)
    }
    pub fn of(&self, code: &str) -> Result<Value, Fault> {
        let invalid = || Fault::RangeError(format!("invalid {} code", self.kind));
        let code = match self.kind {
            "region" => {
                if !((code.len() == 2 && code.bytes().all(|b| b.is_ascii_alphabetic()))
                    || (code.len() == 3 && code.bytes().all(|b| b.is_ascii_digit())))
                {
                    return Err(invalid());
                }
                code.to_ascii_uppercase()
            }
            "script" => {
                if code.len() != 4 || !code.bytes().all(|b| b.is_ascii_alphabetic()) {
                    return Err(invalid());
                }
                let mut code = code.to_ascii_lowercase();
                code[..1].make_ascii_uppercase();
                code
            }
            "currency" => {
                // Snapshot policy: QuickJS rejects lowercase currency spellings.
                if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_uppercase()) {
                    return Err(invalid());
                }
                code.to_owned()
            }
            "calendar" => {
                if !super::tag::unicode_type(code) {
                    return Err(invalid());
                }
                let lower = code.to_ascii_lowercase();
                // Unicode calendar type aliases; data stores the legacy keys.
                match lower.as_str() {
                    "ethiopic-amete-alem" => "ethioaa".to_owned(),
                    "islamicc" => "islamic-civil".to_owned(),
                    _ => lower,
                }
            }
            "dateTimeField" => {
                if !FIELDS.contains(&code) {
                    return Err(invalid());
                }
                code.to_owned()
            }
            "language" => {
                return Err(Fault::TypeError(
                    "DisplayNames language names are not implemented (clj-quickjs-224)".into(),
                ));
            }
            _ => unreachable!(),
        };
        let data = locale_data(&self.locale);
        let name = match self.kind {
            "region" => {
                let short = (self.style != "long")
                    .then(|| find(data.regions_short, &code))
                    .flatten();
                short.or_else(|| find(data.regions, &code))
            }
            "script" => find(data.scripts, &code),
            "currency" => find(data.currencies, &code),
            "calendar" => find(
                data.calendars,
                match code.as_str() {
                    "gregory" => "gregorian",
                    "ethioaa" => "ethiopic-amete-alem",
                    _ => &code,
                },
            ),
            "dateTimeField" => find(data.dateTimeFields, &code),
            _ => unreachable!(),
        };
        Ok(match name {
            Some(name) => string(name),
            None if self.fallback == "none" => Value::Undefined,
            None => string(&code),
        })
    }
}
fn string(text: &str) -> Value {
    Value::String(JsString::from(text))
}
/// Exact language matching avoids treating e.g. `enochian` as `en`.
/// The agreed locale set is shared by services; English is the terminal data root.
fn locale_data(locale: &str) -> &'static data::DisplayNamesData {
    let language = locale.split('-').next().unwrap();
    data::display_names_data
        .iter()
        .find(|row| row.locale.and_then(|p| super::pool_string(p, 0)) == Some(language))
        .unwrap_or(&data::display_names_data[0])
}
/// Alternating code/name strings followed by an empty code terminator.
fn find(pool: Option<&'static [u8]>, code: &str) -> Option<&'static str> {
    let mut strings = pool?.split(|&b| b == 0);
    while let Some(key) = strings.next() {
        if key.is_empty() {
            break;
        }
        let name = strings.next()?;
        if key == code.as_bytes() {
            return std::str::from_utf8(name).ok();
        }
    }
    None
}
pub(crate) fn record(heap: &Heap, task: TaskId, value: &Value) -> Result<DisplayNames, Fault> {
    let invalid = || Fault::TypeError("DisplayNames receiver required".into());
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
            o.display_names().cloned().ok_or_else(invalid)
        }
        _ => Err(invalid()),
    })
    .ok_or(Fault::Internal("DisplayNames reclaimed"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_pools_are_complete_pairs() {
        assert_eq!(
            data::display_names_count as usize,
            data::display_names_data.len()
        );
        for row in data::display_names_data {
            for pool in [
                row.languages,
                row.regions,
                row.regions_short,
                row.scripts,
                row.calendars,
                row.dateTimeFields,
                row.currencies,
            ] {
                let mut parts = pool.unwrap().split(|&b| b == 0);
                loop {
                    let key = parts.next().unwrap();
                    if key.is_empty() {
                        break;
                    }
                    assert!(std::str::from_utf8(key).is_ok());
                    let name = parts.next().unwrap();
                    assert!(!name.is_empty());
                    assert!(std::str::from_utf8(name).is_ok());
                }
                assert_eq!(parts.next(), Some(&b""[..]));
                assert_eq!(parts.next(), None);
            }
        }
    }
}
