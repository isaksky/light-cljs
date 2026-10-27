//! Exact comparison messages, built in bounded, byte-accounted VM steps.
//! The operands remain roots until formatting finishes. A shared form DAG
//! may describe exponentially much text; no collection snapshot or expanded
//! string is allocated before admission against the aggregate heap budget.
use super::{Action, Fault};
use crate::collections::form::{self, FormBody, FormMap};
use crate::collections::{ComparisonOperands, PVector};
use crate::heap::Heap;
use crate::heap::budget::{ByteReservation, GrowthCharge, reserve_elements};
use crate::heap::gc::{Trace, TraceEdge};
use crate::value::string::{MAX_STRING_LEN, StringFlatten};
use crate::value::{JsString, Value};
use std::collections::{BTreeMap, btree_map};

type Order = (bool, [u8; 7], usize);
type Entries = BTreeMap<Order, (Value, Value)>;

#[derive(Debug)]
enum Items {
    Positional(PVector, usize),
    Ordered(btree_map::IntoIter<Order, (Value, Value)>, ByteReservation),
}

impl Items {
    fn next(&mut self) -> Option<(Value, Value)> {
        match self {
            Self::Positional(v, i) => {
                let key = v.get(*i)?;
                let value = v.get(*i + 1).unwrap();
                *i += 2;
                Some((key, value))
            }
            Self::Ordered(items, charge) => {
                let _ = charge; // Retain the snapshot's admission until consumed.
                items.next().map(|(_, pair)| pair)
            }
        }
    }
}

#[derive(Debug)]
enum Part {
    Value(Value, bool),
    Text(&'static str),
    String {
        flatten: Option<StringFlatten>,
        text: Option<JsString>,
        index: usize,
        readable: bool,
    },
    Sequence(PVector, usize, &'static str),
    Scan {
        cursor: crate::collections::map::Cursor,
        entries: Entries,
        charge: ByteReservation,
        set: bool,
    },
    Map(Items, bool, bool),
}

#[derive(Debug)]
pub(super) struct Diagnostic {
    roots: ComparisonOperands,
    pending: Vec<Part>,
    pending_charge: GrowthCharge,
    units: Vec<u16>,
    output_charge: GrowthCharge,
    header_charge: ByteReservation,
    host: String,
    host_index: usize,
    host_charge: GrowthCharge,
    host_header: ByteReservation,
}

impl Trace for Diagnostic {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        // Every pending value comes from these immutable source graphs.
        self.roots.0.trace(visit);
        self.roots.1.trace(visit);
    }
}

impl Diagnostic {
    pub(super) fn new(heap: &Heap, roots: ComparisonOperands) -> Result<Self, Fault> {
        let mut out = Self {
            roots,
            pending: Vec::new(),
            pending_charge: GrowthCharge::default(),
            units: Vec::new(),
            output_charge: GrowthCharge::default(),
            header_charge: JsString::reserve_owned(heap, 0, 1)?,
            host: String::new(),
            host_index: 0,
            host_charge: GrowthCharge::default(),
            host_header: reserve_elements(heap, 1, 256)?,
        };
        out.push(heap, Part::Value(out.roots.1.clone(), false))?;
        out.push(heap, Part::Text(" to "))?;
        out.push(heap, Part::Value(out.roots.0.clone(), false))?;
        out.push(heap, Part::Text("Cannot compare "))?;
        Ok(out)
    }

    fn push(&mut self, heap: &Heap, part: Part) -> Result<(), Fault> {
        self.pending_charge
            .admit(heap, self.pending.len(), std::mem::size_of::<Part>())?;
        if self.pending.len() == self.pending.capacity() {
            self.pending
                .try_reserve_exact(self.pending.len().max(8))
                .map_err(|_| Fault::OutOfMemory("comparison traversal"))?;
        }
        self.pending.push(part);
        Ok(())
    }

    fn append(&mut self, heap: &Heap, units: &[u16]) -> Result<(), Fault> {
        self.append_with_limit::<MAX_STRING_LEN>(heap, units)
    }

    fn append_with_limit<const LIMIT: usize>(
        &mut self,
        heap: &Heap,
        units: &[u16],
    ) -> Result<(), Fault> {
        if self
            .units
            .len()
            .checked_add(units.len())
            .is_none_or(|len| len > LIMIT)
        {
            return Err(Fault::StringTooLong);
        }
        for &unit in units {
            self.output_charge.admit(heap, self.units.len(), 2)?;
            if self.units.len() == self.units.capacity() {
                self.units
                    .try_reserve_exact(self.units.len().max(8))
                    .map_err(|_| Fault::OutOfMemory("comparison message"))?;
            }
            self.units.push(unit);
        }
        Ok(())
    }

    fn text(&mut self, heap: &Heap, text: &str) -> Result<(), Fault> {
        // All callers pass bounded ASCII punctuation or number spellings.
        for unit in text.encode_utf16() {
            self.append(heap, &[unit])?;
        }
        Ok(())
    }

    fn string(&mut self, heap: &Heap, text: JsString, readable: bool) -> Result<(), Fault> {
        if readable {
            self.text(heap, "\"")?;
        }
        self.push(
            heap,
            Part::String {
                flatten: Some(StringFlatten::new_accounted(heap, text)?),
                text: None,
                index: 0,
                readable,
            },
        )
    }

    pub(super) fn step(&mut self, heap: &Heap) -> Result<Action, Fault> {
        let Some(part) = self.pending.pop() else {
            // UTF-8 is only a host diagnostic. Decode in bounded steps too,
            // preserving the previous lossy display of lone UTF-16 surrogates.
            for _ in 0..128 {
                let Some(&unit) = self.units.get(self.host_index) else {
                    break;
                };
                self.host_index += 1;
                let c = if (0xd800..=0xdbff).contains(&unit) {
                    if let Some(&low) = self.units.get(self.host_index)
                        && (0xdc00..=0xdfff).contains(&low)
                    {
                        self.host_index += 1;
                        char::from_u32(
                            0x10000 + ((u32::from(unit) - 0xd800) << 10) + u32::from(low) - 0xdc00,
                        )
                        .unwrap()
                    } else {
                        char::REPLACEMENT_CHARACTER
                    }
                } else {
                    char::from_u32(u32::from(unit)).unwrap_or(char::REPLACEMENT_CHARACTER)
                };
                let needed = self.host.len() + c.len_utf8();
                self.host_charge.admit(heap, needed - 1, 1)?;
                if needed > self.host.capacity() {
                    let capacity = needed.max(self.host.capacity().saturating_mul(2)).max(8);
                    self.host
                        .try_reserve_exact(capacity - self.host.len())
                        .map_err(|_| Fault::OutOfMemory("comparison host diagnostic"))?;
                }
                self.host.push(c);
            }
            if self.host_index < self.units.len() {
                return Ok(Action::Continue);
            }
            let mut charge = std::mem::take(&mut self.header_charge);
            charge.merge(std::mem::take(&mut self.output_charge).into_reservation());
            let message = JsString::from_owned_code_units(std::mem::take(&mut self.units))
                .ok_or(Fault::StringTooLong)?
                .with_reservation(charge.is_charged().then(|| std::sync::Arc::new(charge)));
            let mut host_charge = std::mem::take(&mut self.host_header);
            host_charge.merge(std::mem::take(&mut self.host_charge).into_reservation());
            let diagnostic =
                crate::error::Message::accounted(std::mem::take(&mut self.host), host_charge);
            return Err(Fault::ExactError(message, diagnostic));
        };
        match part {
            Part::Text(text) => self.text(heap, text)?,
            Part::String {
                mut flatten,
                mut text,
                mut index,
                readable,
            } => {
                if let Some(build) = &mut flatten {
                    text = build.advance(128);
                    if text.is_some() {
                        flatten = None;
                    }
                }
                if let Some(text) = &text {
                    let end = (index + 128).min(text.len());
                    for &unit in &text.code_units()[index..end] {
                        let escape = if readable {
                            match unit {
                                34 => Some("\\\""),
                                92 => Some("\\\\"),
                                8 => Some("\\b"),
                                12 => Some("\\f"),
                                10 => Some("\\n"),
                                13 => Some("\\r"),
                                9 => Some("\\t"),
                                _ => None,
                            }
                        } else {
                            None
                        };
                        if let Some(escape) = escape {
                            self.text(heap, escape)?;
                        } else {
                            self.append(heap, &[unit])?;
                        }
                    }
                    index = end;
                    if index == text.len() {
                        if readable {
                            self.text(heap, "\"")?;
                        }
                        return Ok(Action::Continue);
                    }
                }
                self.push(
                    heap,
                    Part::String {
                        flatten,
                        text,
                        index,
                        readable,
                    },
                )?;
            }
            Part::Sequence(items, index, end) => {
                if let Some(value) = items.get(index) {
                    if index > 0 {
                        self.text(heap, " ")?;
                    }
                    self.push(heap, Part::Sequence(items, index + 1, end))?;
                    self.push(heap, Part::Value(value, true))?;
                } else {
                    self.text(heap, end)?;
                }
            }
            Part::Scan {
                mut cursor,
                mut entries,
                charge,
                set,
            } => {
                if let Some((key, value)) = cursor.next() {
                    let hash = form::hash(heap, &key) as u32;
                    let nil = matches!(
                        form::unwrap(heap, key.clone()),
                        Value::Null | Value::Undefined
                    );
                    entries.insert(
                        (
                            !nil,
                            std::array::from_fn(|i| ((hash >> (i * 5)) & 31) as u8),
                            entries.len(),
                        ),
                        (key, value),
                    );
                    self.push(
                        heap,
                        Part::Scan {
                            cursor,
                            entries,
                            charge,
                            set,
                        },
                    )?;
                } else {
                    self.push(
                        heap,
                        Part::Map(Items::Ordered(entries.into_iter(), charge), set, false),
                    )?;
                }
            }
            Part::Map(mut items, set, printed) => {
                if let Some((key, value)) = items.next() {
                    if printed {
                        self.text(heap, if set { " " } else { ", " })?;
                    }
                    self.push(heap, Part::Map(items, set, true))?;
                    if !set {
                        self.push(heap, Part::Value(value, true))?;
                        self.push(heap, Part::Text(" "))?;
                    }
                    self.push(heap, Part::Value(key, true))?;
                } else {
                    self.text(heap, "}")?;
                }
            }
            Part::Value(value, nested) => match form::unwrap(heap, value) {
                Value::Null | Value::Undefined => {
                    self.text(heap, if nested { "nil" } else { "" })?
                }
                Value::Bool(value) => self.text(heap, if value { "true" } else { "false" })?,
                Value::String(value) => self.string(heap, value, nested)?,
                Value::Number(value) => self.text(
                    heap,
                    &if nested && value.is_nan() {
                        "##NaN".into()
                    } else if nested && value == f64::INFINITY {
                        "##Inf".into()
                    } else if nested && value == f64::NEG_INFINITY {
                        "##-Inf".into()
                    } else if nested && value == 0.0 && value.is_sign_negative() {
                        "-0.0".into()
                    } else {
                        crate::value::string::number_to_string(value)
                    },
                )?,
                value => {
                    let object = form::form_object(heap, &value);
                    match object.as_deref().map(|f| f.body()) {
                        Some(FormBody::Keyword(name) | FormBody::Symbol(name)) => {
                            if matches!(object.as_ref().unwrap().body(), FormBody::Keyword(_)) {
                                self.text(heap, ":")?;
                            }
                            self.push(heap, Part::Value(Value::String(name.name.clone()), false))?;
                            if let Some(ns) = &name.namespace {
                                self.push(heap, Part::Text("/"))?;
                                self.push(heap, Part::Value(Value::String(ns.clone()), false))?;
                            }
                        }
                        Some(FormBody::Vector(items) | FormBody::List(items)) => {
                            let vector =
                                matches!(object.as_ref().unwrap().body(), FormBody::Vector(_));
                            self.text(heap, if vector { "[" } else { "(" })?;
                            self.push(
                                heap,
                                Part::Sequence(items.clone(), 0, if vector { "]" } else { ")" }),
                            )?;
                        }
                        Some(FormBody::Map(map) | FormBody::Set(map)) => {
                            let set = matches!(object.as_ref().unwrap().body(), FormBody::Set(_));
                            self.text(heap, if set { "#{" } else { "{" })?;
                            match map {
                                FormMap::Positional(pairs) => self.push(
                                    heap,
                                    Part::Map(Items::Positional(pairs.clone(), 0), set, false),
                                )?,
                                FormMap::Hashed(map) => {
                                    // Conservative B-tree node/entry admission, before
                                    // the first insertion. One entry is visited per step.
                                    let charge = reserve_elements(heap, map.len(), 512)?;
                                    self.push(
                                        heap,
                                        Part::Scan {
                                            cursor: crate::collections::map::Cursor::new(map),
                                            entries: Entries::new(),
                                            charge,
                                            set,
                                        },
                                    )?;
                                }
                            }
                        }
                        _ => self.text(heap, "[object Object]")?,
                    }
                }
            },
        }
        Ok(Action::Continue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_limit_is_checked_before_output_growth() {
        let heap = Heap::default();
        let roots = ComparisonOperands(Value::Number(1.0), Value::Number(2.0));
        let mut diagnostic =
            Diagnostic::new(&heap, roots).unwrap_or_else(|_| panic!("diagnostic setup must fit"));
        let before = heap.byte_budget().used();
        assert!(matches!(
            diagnostic.append_with_limit::<8>(&heap, &[0; 9]),
            Err(Fault::StringTooLong)
        ));
        assert!(diagnostic.units.is_empty());
        assert_eq!(heap.byte_budget().used(), before);
        diagnostic
            .append_with_limit::<8>(&heap, &[0; 8])
            .unwrap_or_else(|_| panic!("output at the limit must fit"));
        let before = heap.byte_budget().used();
        assert!(matches!(
            diagnostic.append_with_limit::<8>(&heap, &[0]),
            Err(Fault::StringTooLong)
        ));
        assert_eq!(diagnostic.units.len(), 8);
        assert_eq!(heap.byte_budget().used(), before);
    }
}
