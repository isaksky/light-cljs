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
            "language" => return self.language_name(code),
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

    fn language_name(&self, code: &str) -> Result<Value, Fault> {
        // A unicode_language_id contains no singleton extensions, including
        // private use. Use Locale's parser/aliases without maximizing or
        // minimizing: explicitly supplied scripts and regions remain visible.
        if code.split('-').any(|part| part.len() == 1) {
            return Err(Fault::RangeError("invalid language code".into()));
        }
        let tag = super::tag::LanguageTag::parse(code)
            .map_err(|_| Fault::RangeError("invalid language code".into()))?;
        let code = tag.base_name();
        let data = locale_data(&self.locale);
        // Like the snapshot, all styles read the same language-name pool;
        // CLDR metadata keys such as -alt-short are not language identifiers.
        let language_name = |key: &str| find(data.languages, key);
        let Some(mut name) = language_name(&tag.language) else {
            return Ok(if self.fallback == "none" {
                Value::Undefined
            } else {
                string(&code)
            });
        };
        let mut script = tag.script.as_deref();
        let mut region = tag.region.as_deref();
        if self.language_display == "dialect" {
            // The snapshot explicitly supplies these compound spellings in
            // displaynames.c. They precede partial dialect lookup, preserving
            // e.g. Chinese (Simplified, China) rather than Simplified Chinese
            // (China), including the snapshot's English spelling in French.
            let dialect_name =
                |key: &str| compound_language_name(key).or_else(|| language_name(key));
            if let Some(exact) = dialect_name(&code) {
                return Ok(string(exact));
            }
            // Prefer the longest dialect key. Script+region precedes script,
            // which precedes region; unmatched components are composed below.
            let mut candidates = Vec::with_capacity(3);
            if let (Some(sc), Some(rg)) = (script, region) {
                candidates.push((format!("{}-{sc}-{rg}", tag.language), true, true));
            }
            if let Some(sc) = script {
                candidates.push((format!("{}-{sc}", tag.language), true, false));
            }
            if let Some(rg) = region {
                candidates.push((format!("{}-{rg}", tag.language), false, true));
            }
            for (key, used_script, used_region) in candidates {
                if let Some(dialect) = dialect_name(&key) {
                    name = dialect;
                    if used_script {
                        script = None;
                    }
                    if used_region {
                        region = None;
                    }
                    break;
                }
            }
        }
        let mut details = Vec::new();
        if let Some(script) = script {
            details.push(find(data.scripts, script).unwrap_or(script));
        }
        if let Some(region) = region {
            let short = (self.style != "long")
                .then(|| find(data.regions_short, region))
                .flatten();
            details.push(
                short
                    .or_else(|| find(data.regions, region))
                    .unwrap_or(region),
            );
        }
        details.extend(tag.variants.iter().map(String::as_str));
        // The pinned snapshot has no localeDisplayPattern, separator or variant
        // pools. Both translated locales use this shared pattern; unknown
        // components retain canonical codes rather than invented translations.
        Ok(if details.is_empty() {
            string(name)
        } else {
            string(&format!("{name} ({})", details.join(", ")))
        })
    }
}

fn compound_language_name(code: &str) -> Option<&'static str> {
    data::dn_language_fallbacks
        .chunks_exact(2)
        .find_map(|pair| {
            (super::pool_string(pair[0]?, 0)? == code)
                .then(|| super::pool_string(pair[1]?, 0))
                .flatten()
        })
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
