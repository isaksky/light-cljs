//! Shared service locale resolution, option validation and result construction.
//! JavaScript coercions/getters are handled by the resumable native reader.
use super::{LocaleData, tag::LanguageTag};
use crate::{
    heap::{Heap, HeapObject},
    ids::TaskId,
    object::OrdinaryObject,
    value::{JsString, Value},
    vm::{
        dispatch::{self, Fault},
        property,
    },
};

pub(crate) fn supported_locale(tag: &str, keys: &[&str]) -> Option<(String, &'static LocaleData)> {
    let parsed = LanguageTag::parse(tag).ok()?;
    let mut base = parsed.base_name();
    loop {
        if super::LOCALES.contains(&base.as_str()) {
            let data = super::lookup(&base)?;
            let mut result = LanguageTag::parse(&base).ok()?;
            for key in keys {
                if let Some(value) = parsed.keywords.get(*key) {
                    // Services supply their relevant keys; nu additionally has a
                    // finite supported-value set in the pinned number tables.
                    if *key != "nu" || super::number_format::numbering_system(value).is_some() {
                        result.keywords.insert((*key).into(), value.clone());
                    }
                }
            }
            return Some((result.to_string(), data));
        }
        base = base.rsplit_once('-')?.0.to_owned();
    }
}

pub(crate) fn resolve_locale(
    heap: &Heap,
    task: TaskId,
    locales: &Value,
    keys: &[&str],
) -> Result<Option<(String, &'static LocaleData)>, Fault> {
    for value in locale_values(heap, task, locales)? {
        let Value::String(tag) = value else {
            return Err(Fault::Internal("canonical locale list"));
        };
        if let Some(found) = supported_locale(&tag.to_string_lossy(), keys) {
            return Ok(Some(found));
        }
    }
    Ok(None)
}
fn locale_values(heap: &Heap, task: TaskId, locales: &Value) -> Result<Vec<Value>, Fault> {
    let len = dispatch::to_number(&property::get_field(
        heap,
        task,
        locales,
        &JsString::from("length"),
    )?)? as u64;
    (0..len)
        .map(|i| {
            property::get(
                heap,
                task,
                locales,
                &property::Key::from_name(&JsString::from(i.to_string().as_str())),
            )
        })
        .collect()
}
pub(crate) fn supported_locales(
    heap: &Heap,
    task: TaskId,
    locales: &Value,
) -> Result<Value, Fault> {
    let result = property::new_array(heap, task)?;
    for value in locale_values(heap, task, locales)? {
        if let Value::String(tag) = &value
            && supported_locale(&tag.to_string_lossy(), &[]).is_some()
        {
            property::append(heap, task, &result, Some(value))?;
        }
    }
    Ok(result)
}
/// The strict GetOptionsObject used by new services. NumberFormat retains its
/// existing primitive-boxing behavior for compatibility with its pinned output.
pub(crate) fn get_options_object(value: &Value) -> Result<(), Fault> {
    if matches!(value, Value::Undefined | Value::Object(_)) {
        Ok(())
    } else {
        Err(Fault::TypeError("Intl options must be an object".into()))
    }
}
pub(crate) fn string_option(
    value: &Value,
    allowed: &[&'static str],
) -> Result<&'static str, Fault> {
    let text = dispatch::to_js_string(value)?.to_string_lossy();
    allowed
        .iter()
        .copied()
        .find(|&s| s == text)
        .ok_or_else(|| Fault::RangeError("invalid Intl option".into()))
}
pub(crate) fn number_option(value: &Value, min: usize, max: usize) -> Result<usize, Fault> {
    let n = dispatch::to_number(value)?;
    if n.is_nan() || n < min as f64 || n > max as f64 {
        return Err(Fault::RangeError("Intl numeric option out of range".into()));
    }
    Ok(n.floor() as usize)
}
pub(crate) fn resolved_options(
    heap: &Heap,
    task: TaskId,
    fields: impl IntoIterator<Item = (&'static str, Value)>,
) -> Result<Value, Fault> {
    let proto = crate::realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("realm exhausted"))?
        .object_prototype;
    let value = heap
        .alloc(HeapObject::Plain(OrdinaryObject::with_prototype(
            task, proto,
        )))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("heap exhausted"))?;
    for (name, field) in fields {
        property::define_field(heap, task, &value, &JsString::from(name), field)?;
    }
    Ok(value)
}
