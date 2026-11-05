//! Root rule set from the pinned QuickJS snapshot: NFD, folded scalar order,
//! combining-mark secondary order, then case. This is not a DUCET database.
//! Generated locale tailoring weights reuse these immutable root readers.
use super::{data::collation as data, service};
use crate::{
    heap::{Heap, HeapObject},
    ids::TaskId,
    object::{OrdinaryObject, PropertyFlags},
    value::{
        JsString, Value,
        string::{CursorStep, UnitCursor},
    },
    vm::dispatch::Fault,
};
use std::cmp::Ordering;

#[derive(Clone, Debug)]
pub(crate) struct Collator {
    pub locale: String,
    pub usage: &'static str,
    pub sensitivity: &'static str,
    pub ignore_punctuation: bool,
    pub numeric: bool,
    pub case_first: &'static str,
    pub collation: &'static str,
    german: bool,
    turkish: bool,
}
impl Collator {
    pub fn new(locale: String) -> Self {
        let language = locale.split('-').next().unwrap_or("");
        let german = language == "de";
        let turkish = language == "tr";
        Self {
            locale,
            usage: "sort",
            sensitivity: "variant",
            ignore_punctuation: false,
            numeric: false,
            case_first: "false",
            collation: "default",
            german,
            turkish,
        }
    }
    pub fn supported_collation(&self, value: &str) -> &'static str {
        if self.german && self.usage == "sort" && value == "phonebk" {
            "phonebk"
        } else {
            "default"
        }
    }
    pub fn resolved(&self, heap: &Heap, task: TaskId) -> Result<Value, Fault> {
        let text = service::fixed_option;
        service::resolved_options(
            heap,
            task,
            [
                ("locale", service::text_option(heap, &self.locale)?),
                ("usage", text(self.usage)),
                ("sensitivity", text(self.sensitivity)),
                ("ignorePunctuation", Value::Bool(self.ignore_punctuation)),
                ("collation", text(self.collation)),
                ("numeric", Value::Bool(self.numeric)),
                ("caseFirst", text(self.case_first)),
            ],
        )
    }
}
fn mapping(table: &'static [(u32, &'static [u32])], cp: u32) -> Option<&'static [u32]> {
    table
        .binary_search_by_key(&cp, |&(c, _)| c)
        .ok()
        .map(|i| table[i].1)
}
fn cc(cp: u32) -> u8 {
    data::CCC
        .binary_search_by_key(&cp, |&(c, _)| c)
        .ok()
        .map_or(0, |i| data::CCC[i].1)
}
fn mark(cp: u32) -> bool {
    matches!(cp, 0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe20..=0xfe2f)
}
fn significant(cp: u32) -> bool {
    matches!(cp, 0x30..=0x39 | 0x41..=0x5a | 0x61..=0x7a)
        || (cp >= 128 && char::from_u32(cp).is_some_and(crate::frontend::lexer::is_ident_start))
}

/// Each step visits a rope node/unit, inserts one decomposed scalar, moves one
/// out-of-order combining mark, or emits one bounded (at most three) fold.
#[derive(Debug)]
struct Keys {
    cursor: UnitCursor,
    high: Option<u16>,
    saved: Option<u16>,
    done: bool,
    expansion: &'static [u32],
    scalar: Option<u32>,
    insert: Option<usize>,
    nfd: Vec<u32>,
    at: usize,
    keys: [Vec<u32>; 4],
    prepared: bool,
}
impl Keys {
    fn new(text: JsString) -> Result<Self, Fault> {
        // The exporter checks the snapshot's maximum canonical expansion (4).
        // Reserve without filling memory, as StringBuild does: later pushes
        // cannot perform an input-sized reallocation/copy outside the budget.
        let mut nfd = Vec::new();
        nfd.try_reserve_exact(
            text.len()
                .checked_mul(4)
                .ok_or(Fault::OutOfMemory("collation input too large"))?,
        )
        .map_err(|_| Fault::OutOfMemory("collation input too large"))?;
        Ok(Self {
            cursor: UnitCursor::new(text),
            high: None,
            saved: None,
            done: false,
            expansion: &[],
            scalar: None,
            insert: None,
            nfd,
            at: 0,
            keys: [vec![], vec![], vec![], vec![]],
            prepared: false,
        })
    }
    fn scalar(&mut self, cp: u32) {
        if let Some(nfd) = mapping(data::NFD, cp) {
            self.expansion = nfd;
        } else {
            self.scalar = Some(cp);
        }
    }
    fn step(&mut self, options: &Collator) -> Result<bool, Fault> {
        if let Some(i) = self.insert.take() {
            if i > 0 && cc(self.nfd[i]) != 0 && cc(self.nfd[i - 1]) > cc(self.nfd[i]) {
                self.nfd.swap(i, i - 1);
                self.insert = Some(i - 1);
            }
            return Ok(false);
        }
        if let Some(cp) = self.scalar.take().or_else(|| {
            let (&cp, rest) = self.expansion.split_first()?;
            self.expansion = rest;
            Some(cp)
        }) {
            self.nfd.push(cp);
            self.insert = Some(self.nfd.len() - 1);
            return Ok(false);
        }
        if !self.done {
            let next = self
                .saved
                .take()
                .map_or_else(|| self.cursor.step(), CursorStep::Unit);
            match next {
                CursorStep::Continue => (),
                CursorStep::End => {
                    self.done = true;
                    if let Some(high) = self.high.take() {
                        self.scalar(u32::from(high));
                    }
                }
                CursorStep::Unit(unit) => {
                    if let Some(high) = self.high.take() {
                        if (0xdc00..=0xdfff).contains(&unit) {
                            self.scalar(
                                0x10000 + ((u32::from(high) - 0xd800) << 10) + u32::from(unit)
                                    - 0xdc00,
                            );
                        } else {
                            self.saved = Some(unit);
                            self.scalar(u32::from(high));
                        }
                    } else if (0xd800..=0xdbff).contains(&unit) {
                        self.high = Some(unit);
                    } else {
                        self.scalar(u32::from(unit));
                    }
                }
            }
            return Ok(false);
        }
        if !self.prepared {
            let folded_len = self
                .nfd
                .len()
                .checked_mul(3)
                .ok_or(Fault::OutOfMemory("collation key too large"))?;
            for (i, key) in self.keys.iter_mut().enumerate() {
                key.try_reserve_exact(if i == 3 { self.nfd.len() } else { folded_len })
                    .map_err(|_| Fault::OutOfMemory("collation key too large"))?;
            }
            self.prepared = true;
            return Ok(false);
        }
        let Some(&cp) = self.nfd.get(self.at) else {
            return Ok(true);
        };
        self.at += 1;
        if options.ignore_punctuation && !significant(cp) {
            return Ok(false);
        }
        let umlaut = options.german && self.nfd.get(self.at) == Some(&0x0308);
        let tailored = if umlaut {
            mapping(
                if options.collation == "phonebk" || options.usage == "search" {
                    data::PHONEBOOK
                } else {
                    data::GERMAN_SORT
                },
                cp,
            )
        } else if options.turkish {
            mapping(data::TURKISH, cp)
        } else {
            None
        };
        let folded = tailored
            .or_else(|| mapping(data::FOLD, cp))
            .unwrap_or(std::slice::from_ref(&cp));
        if !mark(cp) {
            self.keys[0].extend_from_slice(folded);
            // Align tertiary weights with primary fold expansions (ß -> ss).
            // Digits retain their identity so numeric runs, including leading
            // zeros, are skipped equivalently at every comparison level.
            let upper = crate::regexp::property::lookup("Uppercase")
                .unwrap()
                .contains(cp, false, false);
            let weight = u32::from(if options.case_first == "upper" {
                !upper
            } else {
                upper
            });
            self.keys[2].extend(folded.iter().map(|&c| if digit(c) { c } else { weight }));
        }
        self.keys[1].extend_from_slice(folded);
        self.keys[3].push(cp);
        if umlaut && tailored.is_some() {
            // Only primary/secondary consume the diaeresis. Preserve the full
            // canonical input for the variant fallback, including this mark.
            self.at += 1;
            if !options.ignore_punctuation {
                self.keys[3].push(0x0308);
            }
        }
        Ok(false)
    }
}
fn digit(cp: u32) -> bool {
    (0x30..=0x39).contains(&cp)
}
#[derive(Debug)]
struct Digits {
    end: [usize; 2],
    significant: [usize; 2],
    scanning: usize,
    offset: usize,
}
#[derive(Debug)]
pub(crate) struct Comparison {
    options: Collator,
    sides: [Keys; 2],
    building: usize,
    level: usize,
    at: [usize; 2],
    digits: Option<Digits>,
}
impl Comparison {
    pub fn new(options: Collator, a: JsString, b: JsString) -> Result<Self, Fault> {
        Ok(Self {
            options,
            sides: [Keys::new(a)?, Keys::new(b)?],
            building: 0,
            level: 0,
            at: [0, 0],
            digits: None,
        })
    }
    /// Bounded even for equal strings, descending combining classes and digit
    /// runs of arbitrary length. The VM charges every group of 128 steps.
    pub fn advance(&mut self) -> Result<Option<Ordering>, Fault> {
        for _ in 0..128 {
            if self.building < 2 {
                if self.sides[self.building].step(&self.options)? {
                    self.building += 1;
                }
                continue;
            }
            if self.level == 4 || (self.level == 3 && self.options.sensitivity == "case") {
                return Ok(Some(Ordering::Equal));
            }
            if self.level == 1 && matches!(self.options.sensitivity, "base" | "case") {
                self.level += 1;
                continue;
            }
            if self.level == 2 && matches!(self.options.sensitivity, "base" | "accent") {
                return Ok(Some(Ordering::Equal));
            }
            let keys = [
                &self.sides[0].keys[self.level],
                &self.sides[1].keys[self.level],
            ];
            if let Some(d) = &mut self.digits {
                if d.scanning < 2 {
                    let side = d.scanning;
                    if keys[side].get(d.end[side]).is_some_and(|&c| digit(c)) {
                        if d.significant[side] == d.end[side] && keys[side][d.end[side]] == 0x30 {
                            d.significant[side] += 1;
                        }
                        d.end[side] += 1;
                    } else {
                        d.scanning += 1;
                    }
                    continue;
                }
                let lengths = [d.end[0] - d.significant[0], d.end[1] - d.significant[1]];
                let order = lengths[0].cmp(&lengths[1]);
                if !order.is_eq() {
                    return Ok(Some(order));
                }
                if d.offset < lengths[0] {
                    let order = keys[0][d.significant[0] + d.offset]
                        .cmp(&keys[1][d.significant[1] + d.offset]);
                    d.offset += 1;
                    if !order.is_eq() {
                        return Ok(Some(order));
                    }
                } else {
                    self.at = d.end;
                    self.digits = None;
                }
                continue;
            }
            match (keys[0].get(self.at[0]), keys[1].get(self.at[1])) {
                (None, None) => {
                    self.level += 1;
                    self.at = [0, 0];
                }
                (None, Some(_)) => return Ok(Some(Ordering::Less)),
                (Some(_), None) => return Ok(Some(Ordering::Greater)),
                (Some(&a), Some(&b)) => {
                    if self.options.numeric && digit(a) && digit(b) {
                        self.digits = Some(Digits {
                            end: self.at,
                            significant: self.at,
                            scanning: 0,
                            offset: 0,
                        });
                        continue;
                    }
                    let order = a.cmp(&b);
                    if !order.is_eq() {
                        return Ok(Some(order));
                    }
                    self.at[0] += 1;
                    self.at[1] += 1;
                }
            }
        }
        Ok(None)
    }
}
pub(crate) fn record(heap: &Heap, task: TaskId, value: &Value) -> Result<Collator, Fault> {
    let Value::Object(id) = value else {
        return Err(Fault::TypeError("Collator receiver required".into()));
    };
    heap.read(*id, |o| match o {
        HeapObject::Plain(o) => {
            if o.owner().is_some_and(|owner| owner != task) {
                return Err(Fault::ForeignObject {
                    owner: o.owner(),
                    caller: task,
                });
            }
            o.collator()
                .cloned()
                .ok_or_else(|| Fault::TypeError("Collator receiver required".into()))
        }
        _ => Err(Fault::TypeError("Collator receiver required".into())),
    })
    .ok_or(Fault::Internal("Collator reclaimed"))?
}

pub(crate) fn get_compare(heap: &Heap, task: TaskId, this: &Value) -> Result<Value, Fault> {
    record(heap, task, this)?;
    let Value::Object(id) = this else {
        unreachable!()
    };
    if let Some(cached) = heap
        .read(*id, |o| match o {
            HeapObject::Plain(o) => o.collator_bound().cloned(),
            _ => None,
        })
        .flatten()
    {
        return Ok(cached);
    }
    let realm = crate::realm::intrinsics(heap).ok_or(Fault::OutOfMemory("realm exhausted"))?;
    let mut metadata = OrdinaryObject::intrinsic(heap, Some(realm.function_prototype));
    for (name, value) in [
        ("name", Value::String(JsString::from(""))),
        ("length", Value::Number(2.0)),
    ] {
        let key = heap
            .keys()
            .intern(&JsString::from(name))
            .ok_or(Fault::OutOfMemory("keys exhausted"))?;
        metadata.define_with(key, value, PropertyFlags::FUNCTION_INFO);
    }
    let metadata = heap
        .alloc(HeapObject::Plain(metadata))
        .ok_or(Fault::OutOfMemory("heap exhausted"))?;
    let bound = heap
        .alloc(HeapObject::BoundFunction(
            crate::vm::function::BoundFunction {
                target: Value::Object(realm.intl_collator_bound),
                this: this.clone(),
                args: std::sync::Arc::from([]),
                _reservation: None,
                owner: task,
                metadata,
                properties: None,
            },
        ))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("heap exhausted"))?;
    heap.with_object_mut(*id, |o| {
        if let HeapObject::Plain(o) = o {
            o.set_collator_bound(bound.clone());
        }
    })
    .ok_or(Fault::Internal("Collator reclaimed"))?;
    Ok(bound)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn compare(options: &Collator, a: &str, b: &str) -> Ordering {
        let mut state = Comparison::new(options.clone(), JsString::from(a), JsString::from(b))
            .ok()
            .unwrap();
        for _ in 0..10000 {
            if let Some(result) = state.advance().ok().unwrap() {
                return result;
            }
        }
        panic!("comparison did not finish");
    }
    #[test]
    fn root_collation_order() {
        let mut options = Collator::new("en".into());
        for (a, b) in [("a", "A"), ("A", "á"), ("á", "b")] {
            assert_eq!(compare(&options, a, b), Ordering::Less);
        }
        assert_eq!(compare(&options, "e\u{301}", "é"), Ordering::Equal);
        assert_eq!(
            compare(&options, "a\u{315}\u{300}", "à\u{315}"),
            Ordering::Equal
        );
        assert_eq!(
            compare(&options, "각", "\u{1100}\u{1161}\u{11a8}"),
            Ordering::Equal
        );
        assert_eq!(
            compare(&options, "𝅗𝅥", "\u{1d157}\u{1d165}"),
            Ordering::Equal
        );
        assert_eq!(compare(&options, "item2", "item10"), Ordering::Greater);
        options.numeric = true;
        assert_eq!(compare(&options, "item2", "item10"), Ordering::Less);
        assert_eq!(compare(&options, "item0002", "item2"), Ordering::Equal);
        options.sensitivity = "accent";
        assert_eq!(compare(&options, "a", "A"), Ordering::Equal);
        options.sensitivity = "base";
        assert_eq!(compare(&options, "a", "á"), Ordering::Equal);
        assert_eq!(compare(&options, "Straße", "STRASSE"), Ordering::Equal);
    }

    #[test]
    fn root_collation_full_folding() {
        let mut options = Collator::new("en".into());
        for sensitivity in ["base", "accent"] {
            options.sensitivity = sensitivity;
            assert_eq!(compare(&options, "ẞ", "ß"), Ordering::Equal);
            assert_eq!(compare(&options, "ẞ", "ss"), Ordering::Equal);
        }
        for &(_, expansion) in data::FOLD {
            assert!(
                expansion
                    .iter()
                    .all(|&cp| mapping(data::FOLD, cp).is_none()),
                "full fold must be idempotent"
            );
        }
    }

    #[test]
    fn root_collation_case_sensitivity_ignores_variants() {
        let mut options = Collator::new("en".into());
        options.sensitivity = "case";
        for (a, b) in [("ſ", "s"), ("ς", "σ"), ("ß", "ss"), ("á", "a")] {
            assert_eq!(compare(&options, a, b), Ordering::Equal);
        }
        assert_eq!(compare(&options, "σ", "Σ"), Ordering::Less);
        assert_eq!(compare(&options, "ß", "ẞ"), Ordering::Less);
        options.case_first = "upper";
        assert_eq!(compare(&options, "ß", "ẞ"), Ordering::Greater);
        options.sensitivity = "variant";
        assert_ne!(compare(&options, "ſ", "s"), Ordering::Equal);
        options.numeric = true;
        assert_eq!(compare(&options, "00012", "12"), Ordering::Equal);
    }
}
