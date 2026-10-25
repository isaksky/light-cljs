//! Pinned QuickJS segmentation heuristics, with RI pairs corrected to satisfy
//! the Segmenter contract. This is intentionally not complete UAX #29.
//!
//! Adapted from intl/segmenter.c, QuickJS 7d1c13c5d8f8.
//! Copyright (c) 2017-2021 Fabrice Bellard
//! Copyright (c) 2017-2021 Charlie Gordon
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in
//! all copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
//! THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
//! THE SOFTWARE.
use crate::{
    heap::{Heap, HeapObject, gc::Trace},
    ids::TaskId,
    value::{JsString, Value},
    vm::dispatch::Fault,
};

#[derive(Clone, Debug)]
pub(crate) enum Data {
    Segmenter {
        locale: String,
        word: bool,
    },
    Segments {
        segmenter: Value,
        input: JsString,
        text: JsString,
    },
    Iterator {
        segmenter: Value,
        input: JsString,
        text: JsString,
        index: usize,
    },
}
impl Trace for Data {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        match self {
            Self::Segmenter { .. } => {}
            Self::Segments { segmenter, .. } | Self::Iterator { segmenter, .. } => {
                segmenter.trace(visit)
            }
        }
    }
}

pub(crate) fn access<R>(
    heap: &Heap,
    task: TaskId,
    value: &Value,
    f: impl FnOnce(&mut Data) -> Result<R, Fault>,
) -> Result<R, Fault> {
    let invalid = || Fault::TypeError("incompatible Segmenter receiver".into());
    let Value::Object(id) = value else {
        return Err(invalid());
    };
    heap.with_object_mut(*id, |object| {
        let HeapObject::Plain(object) = object else {
            return Err(invalid());
        };
        if object.owner().is_some_and(|owner| owner != task) {
            return Err(Fault::ForeignObject {
                owner: object.owner(),
                caller: task,
            });
        }
        f(object.segmenter().ok_or_else(invalid)?)
    })
    .ok_or(Fault::Internal("Segmenter reclaimed"))?
}
pub(crate) fn record(heap: &Heap, task: TaskId, value: &Value) -> Result<Data, Fault> {
    access(heap, task, value, |data| Ok(data.clone()))
}
pub(crate) fn options(heap: &Heap, task: TaskId, value: &Value) -> Result<(String, bool), Fault> {
    match record(heap, task, value)? {
        Data::Segmenter { locale, word } => Ok((locale, word)),
        _ => Err(Fault::TypeError("Segmenter receiver required".into())),
    }
}
pub(crate) fn resolved(heap: &Heap, task: TaskId, value: &Value) -> Result<Value, Fault> {
    let (locale, word) = options(heap, task, value)?;
    super::service::resolved_options(
        heap,
        task,
        [
            ("locale", Value::String(JsString::from(locale.as_str()))),
            (
                "granularity",
                Value::String(JsString::from(if word { "word" } else { "grapheme" })),
            ),
        ],
    )
}

fn scalar(units: &[u16], index: usize) -> (u32, usize) {
    let a = units[index] as u32;
    if (0xd800..=0xdbff).contains(&a)
        && let Some(&b) = units.get(index + 1)
        && (0xdc00..=0xdfff).contains(&b)
    {
        (0x10000 + ((a - 0xd800) << 10) + b as u32 - 0xdc00, 2)
    } else {
        (a, 1)
    }
}
fn word_char(c: u32) -> bool {
    c > 127
        || (c as u8).is_ascii_alphanumeric()
        || matches!(c, 0x2e | 0x2d | 0x2b | 0x25 | 0x24 | 0x5f | 0x27)
}
fn jamo(c: u32) -> u8 {
    match c {
        0x1100..=0x115f => 1,
        0x1160..=0x11a7 => 2,
        0x11a8..=0x11ff => 5,
        0xac00..=0xd7a3 => {
            if (c - 0xac00).is_multiple_of(28) {
                3
            } else {
                4
            }
        }
        _ => 0,
    }
}
fn regional(c: u32) -> bool {
    (0x1f1e6..=0x1f1ff).contains(&c)
}
fn extender(prev: u32, c: u32) -> bool {
    super::data::collation::CCC
        .binary_search_by_key(&c, |&(cp, _)| cp)
        .is_ok()
        || matches!(c, 0x0e31..=0x0e3a | 0x0e47..=0x0e4e | 0x1f3fb..=0x1f3ff | 0xfe00..=0xfe0f | 0x200d)
        || prev == 0x200d
}

/// One scalar per advance. Input must already be flat (flattened with a
/// budget by the caller); no call here can traverse an unbounded rope.
#[derive(Debug)]
pub(crate) struct Scan {
    pub start: usize,
    pub end: usize,
    pub word_like: bool,
    prev: u32,
    regional_count: usize,
    word: bool,
}
impl Scan {
    pub fn new(text: &JsString, start: usize, word: bool) -> Self {
        let (prev, width) = scalar(text.code_units(), start);
        Self {
            start,
            end: start + width,
            word_like: word_char(prev),
            prev,
            regional_count: usize::from(regional(prev)),
            word,
        }
    }
    /// true means the current segment is complete.
    pub fn advance(&mut self, text: &JsString) -> bool {
        if self.end == text.len() {
            return true;
        }
        let (c, width) = scalar(text.code_units(), self.end);
        let join = if self.word {
            word_char(c) == self.word_like
        } else {
            matches!(
                (jamo(self.prev), jamo(c)),
                (1, 1..=4) | (2 | 3, 2 | 5) | (4 | 5, 5)
            ) || extender(self.prev, c)
                || (regional(self.prev) && regional(c) && self.regional_count % 2 == 1)
        };
        if !join {
            return true;
        }
        self.end += width;
        self.prev = c;
        self.regional_count = if regional(c) {
            self.regional_count + 1
        } else {
            0
        };
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn breaks(text: &str, word: bool) -> (Vec<usize>, Vec<bool>) {
        let text = JsString::from(text);
        let mut offsets = vec![0];
        let mut flags = vec![];
        let mut pos = 0;
        while pos < text.len() {
            let mut scan = Scan::new(&text, pos, word);
            while !scan.advance(&text) {}
            pos = scan.end;
            offsets.push(pos);
            flags.push(scan.word_like);
        }
        (offsets, flags)
    }
    #[test]
    fn grapheme_and_word_breaks() {
        assert_eq!(breaks("𠮷野家", false).0, [0, 2, 3, 4]);
        assert_eq!(breaks("éx", false).0, [0, 2, 3]);
        assert_eq!(breaks("🇫🇷🇩🇪", false).0, [0, 4, 8]);
        assert_eq!(
            breaks("Hello world", true),
            (vec![0, 5, 6, 11], vec![true, false, true])
        );
        assert_eq!(breaks("👩🏽‍💻x", false).0, [0, 7, 8]);
        assert_eq!(breaks("각x", false).0, [0, 3, 4]);
        for text in [
            "ก\u{0e31}x", // Thai extender with canonical combining class zero.
            "ก\u{0e47}x", // The second Thai extender range.
            "a\u{fe0f}x", // Variation selector, also combining class zero.
            "각x",        // Precomposed LV followed by T.
            "각ᆨx",        // Precomposed LVT followed by T.
            "ᄀ가x",      // L followed by LV.
            "ᄀ각x",      // L followed by LVT.
            "ᅡᅡx",          // V followed by V.
            "ᆨᆨx",          // T followed by T.
        ] {
            assert_eq!(breaks(text, false).0, [0, 2, 3], "{text}");
        }
        assert_eq!(breaks("가나x", false).0, [0, 1, 2, 3]);
        assert_eq!(breaks("ᄀxᅡ", false).0, [0, 1, 2, 3]);
        assert_eq!(breaks("🇫🇷🇩", false).0, [0, 4, 6]);
        assert_eq!(breaks("🇫a", false).0, [0, 2, 3]);
        // Deliberate snapshot limits, not a claim of UAX #29 conformance.
        assert_eq!(breaks("\r\nx", false).0, [0, 1, 2, 3]);
        assert_eq!(breaks("a.b 😀!", true).1, [true, false, true, false]);
    }
}
