//! Intl.Locale internal-slot access. Locale instances use ordinary task-owned
//! objects; their immutable parsed records contain only owned strings.
use super::tag::LanguageTag;
use crate::heap::{Heap, HeapObject};
use crate::ids::TaskId;
use crate::value::{JsString, Value};
use crate::vm::dispatch::Fault;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    BaseName,
    Language,
    Script,
    Region,
    Calendar,
    Collation,
    HourCycle,
    CaseFirst,
    Numeric,
    NumberingSystem,
    Variants,
}
impl Field {
    pub(crate) const ALL: [Self; 11] = [
        Self::BaseName,
        Self::Language,
        Self::Script,
        Self::Region,
        Self::Calendar,
        Self::Collation,
        Self::HourCycle,
        Self::CaseFirst,
        Self::Numeric,
        Self::NumberingSystem,
        Self::Variants,
    ];
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::BaseName => "baseName",
            Self::Language => "language",
            Self::Script => "script",
            Self::Region => "region",
            Self::Calendar => "calendar",
            Self::Collation => "collation",
            Self::HourCycle => "hourCycle",
            Self::CaseFirst => "caseFirst",
            Self::Numeric => "numeric",
            Self::NumberingSystem => "numberingSystem",
            Self::Variants => "variants",
        }
    }
    pub(crate) fn getter_name(self) -> &'static str {
        match self {
            Self::BaseName => "get baseName",
            Self::Language => "get language",
            Self::Script => "get script",
            Self::Region => "get region",
            Self::Calendar => "get calendar",
            Self::Collation => "get collation",
            Self::HourCycle => "get hourCycle",
            Self::CaseFirst => "get caseFirst",
            Self::Numeric => "get numeric",
            Self::NumberingSystem => "get numberingSystem",
            Self::Variants => "get variants",
        }
    }
}

pub(crate) fn record(
    heap: &Heap,
    task: TaskId,
    value: &Value,
) -> Result<Option<LanguageTag>, Fault> {
    let Value::Object(id) = value else {
        return Ok(None);
    };
    heap.read(*id, |object| {
        let HeapObject::Plain(o) = object else {
            return Ok(None);
        };
        if !o.is_intrinsic() && o.owner().is_some_and(|owner| owner != task) {
            return Err(Fault::ForeignObject {
                owner: o.owner(),
                caller: task,
            });
        }
        Ok(o.locale_data().cloned())
    })
    .ok_or(Fault::Internal("Locale receiver was reclaimed"))?
}

pub(crate) fn inspect(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    field: Option<Field>,
) -> Result<Value, Fault> {
    let tag = record(heap, task, this)?
        .ok_or_else(|| Fault::TypeError("Intl.Locale receiver required".into()))?;
    let text = match field {
        None => Some(tag.to_string()),
        Some(Field::BaseName) => Some(tag.base_name()),
        Some(Field::Language) => Some(tag.language.clone()),
        Some(Field::Script) => tag.script.clone(),
        Some(Field::Region) => tag.region.clone(),
        Some(Field::Variants) => (!tag.variants.is_empty()).then(|| tag.variants.join("-")),
        Some(Field::Numeric) => {
            return Ok(tag
                .keywords
                .get("kn")
                .map(|v| Value::Bool(v.is_empty() || v == "true"))
                .unwrap_or(Value::Bool(false)));
        }
        Some(field) => {
            let key = match field {
                Field::Calendar => "ca",
                Field::Collation => "co",
                Field::HourCycle => "hc",
                Field::CaseFirst => "kf",
                Field::NumberingSystem => "nu",
                _ => unreachable!(),
            };
            tag.keywords.get(key).cloned()
        }
    };
    Ok(text
        .map(|s| Value::String(JsString::from(s.as_str())))
        .unwrap_or(Value::Undefined))
}

/// Create a new Locale from immutable slots, without user property access or
/// coercion. The allocation uses the realm's Locale prototype, as construction
/// does, and therefore never calls an overridden constructor or string method.
pub(crate) fn likely_subtags(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    minimize: bool,
) -> Result<Value, Fault> {
    let tag = record(heap, task, this)?
        .ok_or_else(|| Fault::TypeError("Intl.Locale receiver required".into()))?;
    let mut tag = if minimize {
        super::likely::minimize(&tag)
    } else {
        super::likely::maximize(&tag)
    };
    // Inferring the language can activate a language-specific variant alias.
    // Match the Locale constructor's normalization before publishing the slots.
    tag.apply_base_aliases();
    let prototype = crate::realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("realm exhausted"))?
        .intl_locale_prototype;
    let object =
        crate::object::OrdinaryObject::with_prototype(task, prototype).with_locale_data(tag);
    heap.alloc(HeapObject::Plain(object))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("heap exhausted"))
}
