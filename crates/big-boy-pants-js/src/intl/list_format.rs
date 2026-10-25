//! Immutable ListFormat slots and safe access to the pinned separator rules.
use crate::{
    heap::{Heap, HeapObject},
    ids::TaskId,
    value::{JsString, Value},
    vm::dispatch::Fault,
};

#[derive(Clone, Debug)]
pub(crate) struct ListFormat {
    pub locale: String,
    pub kind: &'static str,
    pub style: &'static str,
}

impl ListFormat {
    pub fn new(locale: String) -> Self {
        Self {
            locale,
            kind: "conjunction",
            style: "long",
        }
    }

    pub fn separator(&self, count: usize, index: usize) -> &'static str {
        use super::data::lists::{LOCALES, SEPARATORS};
        let locale = LOCALES
            .iter()
            .position(|&tag| tag == self.locale)
            .expect("resolved ListFormat locale has generated patterns");
        let kind = match self.kind {
            "conjunction" => 0,
            "disjunction" => 1,
            _ => 2,
        };
        let style = match self.style {
            "long" => 0,
            "short" => 1,
            _ => 2,
        };
        let part = if count == 2 {
            3
        } else if index + 1 == count {
            2
        } else {
            1
        };
        SEPARATORS[locale * 36 + kind * 12 + style * 4 + part]
    }

    pub fn resolved(&self, heap: &Heap, task: TaskId) -> Result<Value, Fault> {
        super::service::resolved_options(
            heap,
            task,
            [
                (
                    "locale",
                    Value::String(JsString::from(self.locale.as_str())),
                ),
                ("type", Value::String(JsString::from(self.kind))),
                ("style", Value::String(JsString::from(self.style))),
            ],
        )
    }
}

pub(crate) fn record(heap: &Heap, task: TaskId, value: &Value) -> Result<ListFormat, Fault> {
    let invalid = || Fault::TypeError("ListFormat receiver required".into());
    let Value::Object(id) = value else {
        return Err(invalid());
    };
    heap.read(*id, |object| match object {
        HeapObject::Plain(object) => {
            if object.owner().is_some_and(|owner| owner != task) {
                return Err(Fault::ForeignObject {
                    owner: object.owner(),
                    caller: task,
                });
            }
            object.list_format().cloned().ok_or_else(invalid)
        }
        _ => Err(invalid()),
    })
    .ok_or(Fault::Internal("ListFormat reclaimed"))?
}

#[cfg(test)]
mod tests {
    #[test]
    fn generated_list_patterns_cover_resolved_locales() {
        use crate::intl::data::lists::{LOCALES, SEPARATORS};
        assert_eq!(LOCALES, crate::intl::LOCALES);
        assert_eq!(SEPARATORS.len(), LOCALES.len() * 36);
        for pattern in SEPARATORS.chunks_exact(4) {
            assert_eq!(pattern[0], "");
        }
    }
}
