//! JavaScript string values (clj-quickjs-65).
//!
//! # Representation
//!
//! A JavaScript string is a primitive: an immutable sequence of UTF-16 code
//! units, compared by content. [`JsString`] stores one as a thin `Arc`
//! (one pointer, so a raw VM value stays two words) to an immutable body
//! that owns flat code units or two immutable string children. The body is
//! **independently owned immutable `Arc` storage outside the heap**, like
//! executable code and collection trie nodes (`docs/architecture.md`, "String values"):
//!
//! * a string is not a heap object and has no heap edges, so the collector
//!   neither traces nor sweeps it; a string lives exactly as long as some
//!   value (frame slot, operand, capture, collection entry, root, constant
//!   pool, host copy) holds its `Arc`. That is ordinary Rust ownership of an
//!   acyclic string graph, not reference counting of heap objects: no string
//!   can reference another value, so no cycle can form through one;
//! * it is persistent and shareable by construction: nothing can change its
//!   contents, so publishing one, storing one in a persistent collection or
//!   reading it from another task shares the same body without copying and
//!   without any publication walk (`crate::value::share`);
//! * equality, hashing and ordering read only the body, never the heap, so
//!   `===`, map keys (SameValueZero) and relational comparison need
//!   no heap lock.
//!
//! # UTF-16 code units
//!
//! The body stores code units (`u16`), not Rust `str` bytes, because
//! JavaScript defines `length`, indexing and relational order on code units
//! and a string may contain lone surrogates. [`JsString::from_code_units`]
//! accepts any sequence: the lexer builds string literals with it, so a lone
//! surrogate escape is a lone code unit and two surrogate escapes that spell
//! a pair are the same string as the astral character (clj-quickjs-141).
//! Consequently:
//!
//! * `length` is [`JsString::len`], the number of stored code units (O(1));
//! * relational order is [`Ord`]: lexicographic over code units, the
//!   ECMAScript IsLessThan order for two strings (QuickJS `js_string_compare`,
//!   `research/quickjs/quickjs.c`, revision 7d1c13c5d8f8, compares code units
//!   too), not Unicode scalar order;
//! * equality, hashing and the map-key hash compare code-unit sequences,
//!   never how a string was produced;
//! * conversion to Rust text ([`JsString::to_string_lossy`], `Display`)
//!   replaces a lone surrogate with U+FFFD; [`JsString::code_units`] and
//!   `Debug` (a quoted literal that writes a lone surrogate as `\u{d800}`)
//!   are exact. None of them panics.
//!
//! Concatenations up to 32 units copy into a flat buffer. Larger results
//! retain their operands in O(1), with a cached length. The first contiguous
//! read flattens iteratively into a cached flat string; subsequent reads share it.
//! Both traversal and destruction are iterative, even for deep append/prepend
//! chains. `code_units` flattening is synchronous O(length). `StringFlatten`
//! supplies budgeted traversal/copying for consumers such as Date parsing.

use std::cell::Cell;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

const FLAT_CONCAT_LIMIT: usize = 32;
/// The byte charge of one string body's fixed part (header, cache and
/// storage descriptor), conservatively.
const BODY_BYTES: usize = 256;

thread_local! {
    static COPIED_CODE_UNITS: Cell<u64> = const { Cell::new(0) };
    #[cfg(test)]
    static CURSOR_NODE_RELEASES: Cell<usize> = const { Cell::new(0) };
    #[cfg(test)]
    static STRING_BODY_RELEASES: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn cursor_release_counts() -> (usize, usize) {
    (
        CURSOR_NODE_RELEASES.with(Cell::get),
        STRING_BODY_RELEASES.with(Cell::get),
    )
}

fn record_copies(units: usize) {
    COPIED_CODE_UNITS.with(|n| n.set(n.get().saturating_add(units as u64)));
}

/// Maximum string length in code units: `2^30 - 1`, as QuickJS's
/// `JS_STRING_LEN_MAX`. Constructors/builders reject an overlong result before
/// allocating its storage. The VM raises catchable InternalError "string too
/// long", including for rope concatenation; allocation exhaustion remains a
/// separate, uncatchable OutOfMemory failure (clj-quickjs-231).
pub const MAX_STRING_LEN: usize = (1 << 30) - 1;

/// An immutable JavaScript string value: a sequence of UTF-16 code units.
/// See the module docs. Cloning is O(1) and shares the body.
#[derive(Clone)]
pub struct JsString(Arc<Body>);

/// The immutable body of a string.
struct Body {
    // Keeping the allocation as a Vec lets the incremental builder transfer
    // ownership without a potentially linear shrink/copy at its final step.
    len: usize,
    flat: OnceLock<JsString>,
    storage: Storage,
    reservation: Option<Arc<crate::heap::budget::ByteReservation>>,
}

enum Storage {
    Flat(Vec<u16>),
    Rope(Option<(JsString, JsString)>),
}

impl Body {
    fn flat(units: Vec<u16>) -> Self {
        Self {
            len: units.len(),
            flat: OnceLock::new(),
            storage: Storage::Flat(units),
            reservation: None,
        }
    }

    fn take_children(&mut self) -> Option<(JsString, JsString)> {
        match &mut self.storage {
            Storage::Flat(_) => None,
            Storage::Rope(children) => children.take(),
        }
    }
}

impl Drop for Body {
    fn drop(&mut self) {
        #[cfg(test)]
        STRING_BODY_RELEASES.with(|count| count.set(count.get() + 1));
        // Arc's default recursive destruction would overflow on a long
        // chain. Consume each uniquely owned body with its children removed.
        // into_inner also avoids a recursive last-release race when several
        // threads release shared subtrees concurrently.
        let mut pending = Vec::new();
        if let Some((left, right)) = self.take_children() {
            pending.push(left);
            pending.push(right);
        }
        while let Some(child) = pending.pop() {
            if let Some(mut body) = Arc::into_inner(child.0)
                && let Some((left, right)) = body.take_children()
            {
                pending.push(left);
                pending.push(right);
            }
        }
    }
}

/// A formatter's output (ListFormat, DisplayNames), extended one piece at a
/// time across native steps. [`JsString::concat_accounted`] charges the
/// whole result on every call, which a retained rope chain would accumulate
/// quadratically. Here each appended rope node is charged once, for itself
/// and its piece's body, and one cumulative credit covers the eventual
/// flattened copy of the whole output: every piece's units, the shared first
/// piece included, plus the copy's body. Flattening is infallible
/// ([`JsString::code_units`]), so the copy must be admitted before it can be
/// made. [`StringAccumulator::finish`] moves the credit into the returned
/// body, whose flatten cache shares that body's reservation: the credit
/// lives as long as either the rope or its cached flat copy, whichever is
/// released last.
pub(crate) struct StringAccumulator {
    /// Only the accumulator holds the latest node until `finish`, so the
    /// credit can still be moved into it.
    text: JsString,
    /// Charge for the flattened copy of `text`; uncharged until the first
    /// piece is extended.
    flat_credit: crate::heap::budget::ByteReservation,
}

/// Reports metadata only. Formatting `text` would flatten it, and the
/// flatten cache would share the node charge that `finish` must still own.
impl std::fmt::Debug for StringAccumulator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StringAccumulator")
            .field("len", &self.text.len())
            .field("flat_credit_charged", &self.flat_credit.is_charged())
            .finish_non_exhaustive()
    }
}

impl Default for StringAccumulator {
    fn default() -> Self {
        Self {
            text: JsString::from(""),
            flat_credit: Default::default(),
        }
    }
}

impl StringAccumulator {
    /// Append `piece`. A denial leaves the accumulated output unchanged.
    pub(crate) fn push(
        &mut self,
        heap: &crate::heap::Heap,
        piece: &JsString,
    ) -> Result<(), crate::vm::dispatch::Fault> {
        use crate::vm::dispatch::Fault;
        if piece.is_empty() {
            return Ok(());
        }
        if self.text.is_empty() {
            // Shared, not copied: its body is its owner's charge.
            self.text = piece.clone();
            return Ok(());
        }
        let len = self
            .text
            .len()
            .checked_add(piece.len())
            .filter(|&n| n <= MAX_STRING_LEN)
            .ok_or(Fault::StringTooLong)?;
        // The node and the piece's body (a separator or chunk may be fresh),
        // then this extension's share of the flattened copy: the piece's
        // units, or on the first extension the whole output and its body.
        // A limited budget marks even a zero-byte charge as charged.
        let node = JsString::reserve_owned(heap, piece.len(), 2)?;
        let flat = if self.flat_credit.is_charged() {
            JsString::reserve_owned(heap, piece.len(), 0)?
        } else {
            JsString::reserve_owned(heap, len, 1)?
        };
        self.text = self
            .text
            .concat(piece)
            .expect("checked length")
            .with_reservation(node.is_charged().then(|| Arc::new(node)));
        self.flat_credit.merge(flat);
        Ok(())
    }

    /// The accumulated output, owning the flattened copy's credit.
    pub(crate) fn finish(self) -> JsString {
        let Self {
            mut text,
            flat_credit,
        } = self;
        if !flat_credit.is_charged() {
            return text;
        }
        // Every charged extension made `text` a fresh body that only this
        // accumulator holds, with an unshared node charge (no flatten cache
        // can have cloned it: the output was never read).
        let body = Arc::get_mut(&mut text.0).expect("accumulator owns its output body");
        Arc::get_mut(
            body.reservation
                .as_mut()
                .expect("a charged extension charges its node"),
        )
        .expect("unshared node charge")
        .merge(flat_credit);
        text
    }
}

impl JsString {
    pub(crate) fn import(&self, heap: &crate::heap::Heap) -> Option<Self> {
        if !heap.has_byte_limit()
            || self
                .0
                .reservation
                .as_ref()
                .is_some_and(|ticket| ticket.belongs_to(&heap.byte_budget()))
        {
            return Some(self.clone());
        }
        Self::copy_units(heap, self.code_units())
    }

    pub(crate) fn copy_units(heap: &crate::heap::Heap, units: &[u16]) -> Option<Self> {
        let ticket = Self::reserve(heap, units.len(), 1).ok()?;
        let mut copy = Vec::new();
        copy.try_reserve_exact(units.len()).ok()?;
        copy.extend_from_slice(units);
        Self::from_owned_code_units(copy).map(|text| text.with_reservation(ticket))
    }

    /// A fresh body holding `text`, a leaf's transient Rust output (a Date
    /// or Intl formatter's), charged to `heap` before the body is made.
    pub(crate) fn from_str_accounted(
        heap: &crate::heap::Heap,
        text: &str,
    ) -> Result<Self, crate::vm::dispatch::Fault> {
        let ticket = Self::reserve(heap, text.encode_utf16().count(), 1)?;
        Ok(Self::from(text).with_reservation(ticket))
    }

    /// The one shared body of a fixed spelling a leaf returns (an Intl
    /// option such as `"long"`, a plural category, a typed array's name),
    /// interned process-wide by content and never released. Like an
    /// executable's constants, the byte budget does not count it: retaining
    /// it allocates nothing. `text` is static, so the table is bounded by
    /// the binary's own spellings. Hot paths use a [`FixedText`] instead,
    /// which reaches its body without the table's lock.
    pub(crate) fn fixed(text: &'static str) -> Self {
        use std::collections::HashMap;
        use std::sync::{LazyLock, PoisonError, RwLock};
        static TABLE: LazyLock<RwLock<HashMap<&'static str, JsString>>> =
            LazyLock::new(Default::default);
        if let Some(body) = TABLE
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(text)
        {
            return body.clone();
        }
        TABLE
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(text)
            .or_insert_with(|| Self::from(text))
            .clone()
    }

    /// [`JsString::code_unit_at`] for a result that may be retained
    /// (`s[i]`): the fresh one-unit body is charged to `heap` first.
    pub(crate) fn code_unit_at_accounted(
        &self,
        heap: &crate::heap::Heap,
        index: usize,
    ) -> Result<Option<Self>, crate::vm::dispatch::Fault> {
        if index >= self.len() {
            return Ok(None);
        }
        let unit = self.code_units()[index];
        let ticket = Self::reserve(heap, 1, 1)?;
        Ok(Self::from_owned_code_units(vec![unit]).map(|text| text.with_reservation(ticket)))
    }
    pub(crate) fn with_reservation(
        mut self,
        reservation: Option<Arc<crate::heap::budget::ByteReservation>>,
    ) -> Self {
        if let Some(body) = Arc::get_mut(&mut self.0) {
            body.reservation = reservation;
        }
        self
    }

    pub(crate) fn reserve(
        heap: &crate::heap::Heap,
        units: usize,
        bodies: usize,
    ) -> Result<Option<Arc<crate::heap::budget::ByteReservation>>, crate::vm::dispatch::Fault> {
        let reservation = Self::reserve_owned(heap, units, bodies)?;
        Ok(reservation.is_charged().then(|| Arc::new(reservation)))
    }

    /// The charge of `units` code units in `bodies` string bodies, before
    /// any owner holds it.
    pub(crate) fn reserve_owned(
        heap: &crate::heap::Heap,
        units: usize,
        bodies: usize,
    ) -> Result<crate::heap::budget::ByteReservation, crate::vm::dispatch::Fault> {
        units
            .checked_mul(2)
            .and_then(|n| {
                bodies
                    .checked_mul(BODY_BYTES)
                    .and_then(|b| n.checked_add(b))
            })
            .and_then(|bytes| heap.reserve_bytes(bytes))
            .ok_or(crate::vm::dispatch::Fault::HeapLimit)
    }

    pub(crate) fn concat_accounted(
        &self,
        heap: &crate::heap::Heap,
        other: &Self,
    ) -> Result<Self, crate::vm::dispatch::Fault> {
        if self.is_empty() {
            return Ok(other.clone());
        }
        if other.is_empty() {
            return Ok(self.clone());
        }
        let len = self
            .len()
            .checked_add(other.len())
            .filter(|&n| n <= MAX_STRING_LEN)
            .ok_or(crate::vm::dispatch::Fault::StringTooLong)?;
        // Include the possible flattened body now: code_units, equality and
        // hashing are infallible and cannot attempt a later quota admission.
        let reservation = Self::reserve(
            heap,
            len.checked_mul(16)
                .ok_or(crate::vm::dispatch::Fault::HeapLimit)?,
            2,
        )?;
        Ok(self
            .concat(other)
            .expect("checked length")
            .with_reservation(reservation))
    }
    /// Transfer a preallocated, incrementally filled UTF-16 buffer without
    /// shrinking or copying its contents at a native step boundary.
    /// Every empty string shares one body (an empty `slice`, `charAt` past
    /// the end, `String()`), like a fixed spelling: retaining one allocates
    /// nothing the byte budget would have to count.
    pub(crate) fn from_owned_code_units(units: Vec<u16>) -> Option<JsString> {
        static EMPTY: OnceLock<JsString> = OnceLock::new();
        if units.is_empty() {
            return Some(
                EMPTY
                    .get_or_init(|| JsString(Arc::new(Body::flat(Vec::new()))))
                    .clone(),
            );
        }
        (units.len() <= MAX_STRING_LEN).then(|| JsString(Arc::new(Body::flat(units))))
    }

    /// A string from arbitrary code units, including lone surrogates.
    /// `None` when longer than [`MAX_STRING_LEN`].
    pub fn from_code_units(units: impl Into<Box<[u16]>>) -> Option<JsString> {
        let units = units.into();
        Self::from_owned_code_units(units.into_vec())
    }

    /// The number of UTF-16 code units (JavaScript `length`).
    pub fn len(&self) -> usize {
        self.0.len
    }

    /// True for the empty string `""`.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The one-code-unit string at `index` (`s[index]`, `s.charAt(index)`),
    /// or `None` at or past the end. Constant time after the first flatten.
    pub fn code_unit_at(&self, index: usize) -> Option<JsString> {
        if index >= self.len() {
            return None;
        }
        let unit = self.code_units()[index];
        Self::from_owned_code_units(vec![unit])
    }

    /// The code units, in order. A rope flattens once, in O(length) time
    /// using an explicit traversal stack. Concurrent readers share one cache.
    pub fn code_units(&self) -> &[u16] {
        if let Storage::Flat(units) = &self.0.storage {
            return units;
        }
        let flat = self.0.flat.get_or_init(|| {
            let mut units = Vec::with_capacity(self.len());
            let mut pending = vec![self.0.as_ref()];
            while let Some(body) = pending.pop() {
                match &body.storage {
                    Storage::Flat(flat) => units.extend_from_slice(flat),
                    Storage::Rope(Some((left, right))) => {
                        if let Some(flat) = body.flat.get() {
                            pending.push(flat.0.as_ref());
                        } else {
                            pending.push(right.0.as_ref());
                            pending.push(left.0.as_ref());
                        }
                    }
                    Storage::Rope(None) => unreachable!("only a dropping body loses children"),
                }
            }
            debug_assert_eq!(units.len(), self.len());
            record_copies(units.len());
            Self::from_owned_code_units(units)
                .expect("checked string length")
                .with_reservation(self.0.reservation.clone())
        });
        let Storage::Flat(units) = &flat.0.storage else {
            unreachable!("the cache always holds a flat leaf")
        };
        units
    }

    /// Copy a caller-bounded string without populating its flatten cache.
    /// Reserve both buffers fallibly before walking the immutable rope.
    pub(crate) fn try_copy_code_units_to(&self, out: &mut Vec<u16>) -> Option<()> {
        out.try_reserve_exact(self.len()).ok()?;
        if let Storage::Flat(units) = &self.0.storage {
            out.extend_from_slice(units);
            record_copies(self.len());
            return Some(());
        }
        let mut pending = Vec::new();
        // Nonempty rope children imply at most len pending nodes. Empty
        // strings never form rope children (concat returns its other input).
        pending.try_reserve_exact(self.len()).ok()?;
        pending.push(self.0.as_ref());
        while let Some(body) = pending.pop() {
            match &body.storage {
                Storage::Flat(units) => out.extend_from_slice(units),
                Storage::Rope(Some((left, right))) => {
                    pending.push(right.0.as_ref());
                    pending.push(left.0.as_ref());
                }
                Storage::Rope(None) => unreachable!("live rope retains its children"),
            }
        }
        record_copies(self.len());
        Some(())
    }

    /// Reuse an existing flat leaf without retaining a flattened rope's
    /// obsolete ancestors and their caches. No code units are copied.
    fn concat_operand(&self) -> JsString {
        self.0.flat.get().unwrap_or(self).clone()
    }

    /// The contents as Rust text; a lone surrogate becomes U+FFFD.
    pub fn to_string_lossy(&self) -> String {
        String::from_utf16_lossy(self.code_units())
    }

    /// The concatenation `self + other`, or `None` when the result would be
    /// longer than [`MAX_STRING_LEN`]. Concatenating with `""` returns the
    /// other operand's body unchanged.
    pub fn concat(&self, other: &JsString) -> Option<JsString> {
        if other.is_empty() {
            return Some(self.clone());
        }
        if self.is_empty() {
            return Some(other.clone());
        }
        let len = self.len().checked_add(other.len())?;
        if len > MAX_STRING_LEN {
            return None;
        }
        if len > FLAT_CONCAT_LIMIT {
            return Some(JsString(Arc::new(Body {
                len,
                flat: OnceLock::new(),
                storage: Storage::Rope(Some((self.concat_operand(), other.concat_operand()))),
                reservation: None,
            })));
        }
        let mut units = Vec::with_capacity(len);
        units.extend_from_slice(self.code_units());
        units.extend_from_slice(other.code_units());
        record_copies(len);
        JsString::from_owned_code_units(units)
    }

    /// Cumulative units copied by concatenation, lazy flattening and native
    /// string builders on the current OS thread. Diagnostic only: input
    /// creation and one-unit indexing results are excluded. Take differences
    /// around work on one thread; other threads have independent counters.
    pub fn copied_code_units() -> u64 {
        COPIED_CODE_UNITS.with(Cell::get)
    }

    /// True when both values share one body (an O(1) identity test; equal
    /// contents in different bodies are still `==`).
    pub(crate) fn same_body(&self, other: &JsString) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Number of values sharing this body (tests of release).
    #[cfg(test)]
    pub(crate) fn body_refs(&self) -> usize {
        Arc::strong_count(&self.0)
    }

    /// A private, deterministic content hash (FNV-1a over the code units),
    /// used by persistent map keys: equal strings hash equally.
    pub(crate) fn content_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &u in self.code_units() {
            h ^= u64::from(u);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h ^ (self.len() as u64).rotate_left(32)
    }
}

/// Incremental rope flattening for consumers that require a contiguous input.
/// Each step visits one rope node, copies one unit, or releases one leaf.
/// Flat inputs are reused. No heap edges or final buffer copy.
#[derive(Debug)]
pub(crate) struct StringFlatten {
    // A linked stack avoids an input-sized reallocation/copy on a deep rope.
    pending: std::collections::LinkedList<JsString>,
    leaf: Option<(JsString, usize)>,
    units: Vec<u16>,
    ready: Option<JsString>,
    reservation: Option<Arc<crate::heap::budget::ByteReservation>>,
}

impl StringFlatten {
    pub(crate) fn new_accounted(
        heap: &crate::heap::Heap,
        text: JsString,
    ) -> Result<Self, crate::vm::dispatch::Fault> {
        // Cover the flattened output and the traversal's linked-node stack.
        let reservation = if matches!(&text.0.storage, Storage::Flat(_)) {
            None
        } else {
            JsString::reserve(
                heap,
                text.len()
                    .checked_mul(64)
                    .ok_or(crate::vm::dispatch::Fault::HeapLimit)?,
                1,
            )?
        };
        let mut flatten =
            Self::new(text).ok_or(crate::vm::dispatch::Fault::OutOfMemory("string flatten"))?;
        flatten.reservation = reservation;
        Ok(flatten)
    }
    pub(crate) fn new(text: JsString) -> Option<Self> {
        let ready = match &text.0.storage {
            Storage::Flat(_) => Some(text.clone()),
            Storage::Rope(_) => None,
        };
        let mut units = Vec::new();
        let mut pending = std::collections::LinkedList::new();
        if ready.is_none() {
            // Like StringBuild, reserve once without filling units, so no
            // growing-buffer copy can escape a later advance's work budget.
            units.try_reserve_exact(text.len()).ok()?;
            pending.push_back(text);
        }
        Some(Self {
            pending,
            leaf: None,
            units,
            ready,
            reservation: None,
        })
    }

    pub(crate) fn advance(&mut self, budget: usize) -> Option<JsString> {
        if let Some(text) = self.ready.take() {
            return Some(text);
        }
        for _ in 0..budget {
            if let Some((text, next)) = &mut self.leaf {
                // Only flat storage is installed in leaf, so this never
                // calls code_units' synchronous rope traversal.
                let Storage::Flat(units) = &text.0.storage else {
                    unreachable!("flatten cursor leaf must be flat");
                };
                if let Some(&unit) = units.get(*next) {
                    self.units.push(unit);
                    record_copies(1);
                    *next += 1;
                } else {
                    self.leaf = None;
                }
            } else if let Some(text) = self.pending.pop_back() {
                match &text.0.storage {
                    Storage::Flat(_) => self.leaf = Some((text, 0)),
                    Storage::Rope(Some((left, right))) => {
                        self.pending.push_back(right.clone());
                        self.pending.push_back(left.clone());
                    }
                    Storage::Rope(None) => unreachable!("live rope retains its children"),
                }
                // Children are retained before releasing their parent, so
                // consuming a uniquely owned chain also releases it in steps.
            } else {
                return JsString::from_owned_code_units(std::mem::take(&mut self.units))
                    .map(|text| text.with_reservation(self.reservation.take()));
            }
        }
        None
    }
}

/// One bounded traversal step, including rope-node visits. No lazy flattening.
#[derive(Debug)]
pub(crate) enum CursorStep {
    Unit(u16),
    Continue,
    End,
}

#[derive(Debug)]
pub(crate) struct UnitCursor {
    // A deep rope must not cause an input-sized stack reallocation in a step.
    pending: std::collections::LinkedList<(JsString, usize)>,
}
impl UnitCursor {
    pub(crate) fn new(text: JsString) -> Self {
        Self {
            pending: std::iter::once((text, 0)).collect(),
        }
    }
    pub(crate) fn step(&mut self) -> CursorStep {
        let Some((text, offset)) = self.pop() else {
            return CursorStep::End;
        };
        match &text.0.storage {
            Storage::Flat(units) => {
                let Some(&unit) = units.get(offset) else {
                    return CursorStep::Continue;
                };
                self.pending.push_back((text, offset + 1));
                CursorStep::Unit(unit)
            }
            Storage::Rope(Some((left, right))) => {
                self.pending.push_back((right.clone(), 0));
                self.pending.push_back((left.clone(), 0));
                CursorStep::Continue
            }
            Storage::Rope(None) => unreachable!("live rope has children"),
        }
    }

    fn pop(&mut self) -> Option<(JsString, usize)> {
        let entry = self.pending.pop_back();
        #[cfg(test)]
        if entry.is_some() {
            CURSOR_NODE_RELEASES.with(|count| count.set(count.get() + 1));
        }
        entry
    }

    /// Release one pending node without scanning flat units. Move children
    /// out of uniquely owned ropes before dropping their body, preventing a
    /// destructor cascade. Shared subtrees need no walk. False means done.
    pub(crate) fn discard_step(&mut self) -> bool {
        let Some((text, _)) = self.pop() else {
            return false;
        };
        if let Some(mut body) = Arc::into_inner(text.0)
            && let Some((left, right)) = body.take_children()
        {
            self.pending.push_back((right, 0));
            self.pending.push_back((left, 0));
        }
        true
    }
}

#[cfg(test)]
impl Drop for UnitCursor {
    fn drop(&mut self) {
        // Count implicit list disposal too, so a consumer that forgets
        // incremental cleanup cannot hide its work from budget tests.
        CURSOR_NODE_RELEASES.with(|count| count.set(count.get() + self.pending.len()));
    }
}

/// Incremental flat-string construction. Each part names a source, starting
/// unit, and output length. A part longer than the source suffix repeats it
/// (padding). All storage is task-owned Rust data with no collector edges.
#[derive(Debug)]
pub(crate) struct StringBuild {
    parts: std::vec::IntoIter<(JsString, usize, usize)>,
    offset: usize,
    units: Vec<u16>,
    concatenated: Option<JsString>,
    reservation: Option<Arc<crate::heap::budget::ByteReservation>>,
    /// The charge for `parts`' retained allocation, carried over from the
    /// operation that collected them; released with the builder.
    parts_reservation: crate::heap::budget::ByteReservation,
}

impl StringBuild {
    pub(crate) fn new_accounted(
        heap: &crate::heap::Heap,
        parts: Vec<(JsString, usize, usize)>,
    ) -> Result<Self, crate::vm::dispatch::Fault> {
        let len = parts
            .iter()
            .try_fold(0usize, |n, (_, _, len)| n.checked_add(*len))
            .ok_or(crate::vm::dispatch::Fault::StringTooLong)?;
        Self::with_length_accounted(heap, parts, len)
    }

    pub(crate) fn with_length_accounted(
        heap: &crate::heap::Heap,
        parts: Vec<(JsString, usize, usize)>,
        len: usize,
    ) -> Result<Self, crate::vm::dispatch::Fault> {
        if len > MAX_STRING_LEN {
            return Err(crate::vm::dispatch::Fault::StringTooLong);
        }
        let reservation = JsString::reserve(heap, len, 1)?;
        let mut build = Self::with_length(parts, len).ok_or(
            crate::vm::dispatch::Fault::OutOfMemory("string output buffer"),
        )?;
        build.reservation = reservation;
        Ok(build)
    }

    pub(crate) fn concatenation_accounted(
        heap: &crate::heap::Heap,
        parts: Vec<(JsString, usize, usize)>,
    ) -> Result<Self, crate::vm::dispatch::Fault> {
        let len = parts
            .iter()
            .try_fold(0usize, |n, (_, _, len)| n.checked_add(*len))
            .filter(|&n| n <= MAX_STRING_LEN)
            .ok_or(crate::vm::dispatch::Fault::StringTooLong)?;
        let reservation = JsString::reserve(heap, len, parts.len().saturating_add(2))?;
        let mut build =
            Self::concatenation(parts).ok_or(crate::vm::dispatch::Fault::StringTooLong)?;
        build.reservation = reservation;
        Ok(build)
    }
    /// Whole-source concatenation keeps ropes rather than copying prefixes.
    /// Conversion has already completed in observable order in the VM.
    pub(crate) fn concatenation(parts: Vec<(JsString, usize, usize)>) -> Option<Self> {
        let len = parts.iter().try_fold(0usize, |n, (text, start, len)| {
            debug_assert_eq!((*start, *len), (0, text.len()));
            n.checked_add(*len)
        })?;
        if len > MAX_STRING_LEN {
            return None;
        }
        Some(Self {
            parts: parts.into_iter(),
            offset: 0,
            units: Vec::new(),
            concatenated: Some(JsString::from("")),
            reservation: None,
            parts_reservation: Default::default(),
        })
    }

    pub(crate) fn new(parts: Vec<(JsString, usize, usize)>) -> Option<Self> {
        let len = parts
            .iter()
            .try_fold(0usize, |n, (_, _, len)| n.checked_add(*len))?;
        Self::with_length(parts, len)
    }

    /// The caller has already accumulated and checked the sum of part
    /// lengths incrementally. Avoid scanning a long join's parts again.
    pub(crate) fn with_length(parts: Vec<(JsString, usize, usize)>, len: usize) -> Option<Self> {
        if len > MAX_STRING_LEN {
            return None;
        }
        let mut units = Vec::new();
        // Reserve once so bounded copying never triggers a growing-buffer
        // copy. Allocation itself is not interruptible; no units are filled.
        if !matches!(parts.as_slice(), [(text, 0, n)] if *n == text.len()) {
            units.try_reserve_exact(len).ok()?;
        }
        Some(Self {
            parts: parts.into_iter(),
            offset: 0,
            units,
            concatenated: None,
            reservation: None,
            parts_reservation: Default::default(),
        })
    }

    /// Keep the charge of the parts vector this builder now owns.
    pub(crate) fn with_parts_reservation(
        mut self,
        parts: crate::heap::budget::ByteReservation,
    ) -> Self {
        self.parts_reservation.merge(parts);
        self
    }

    /// Copy at most `budget` units/part boundaries; moving the finished Vec
    /// into immutable Arc storage does no final linear copy.
    pub(crate) fn advance(&mut self, budget: usize) -> Option<JsString> {
        if let Some(result) = &mut self.concatenated {
            for _ in 0..budget {
                let Some((text, _, _)) = self.parts.next() else {
                    return Some(result.clone());
                };
                *result = result
                    .concat(&text)
                    .expect("checked concatenation length")
                    .with_reservation(self.reservation.clone());
            }
            return None;
        }
        if let [(text, 0, len)] = self.parts.as_slice()
            && *len == text.len()
            && self.units.is_empty()
            && self.offset == 0
        {
            return Some(text.clone());
        }
        for _ in 0..budget {
            let Some((text, start, len)) = self.parts.as_slice().first() else {
                return JsString::from_owned_code_units(std::mem::take(&mut self.units))
                    .map(|text| text.with_reservation(self.reservation.clone()));
            };
            if self.offset == *len {
                // Release each consumed body in a bounded step, rather than
                // dropping every retained string at completion.
                self.parts.next();
                self.offset = 0;
            } else {
                self.units
                    .push(text.code_units()[start + self.offset % (text.len() - start)]);
                record_copies(1);
                self.offset += 1;
            }
        }
        None
    }
}

/// One fixed spelling's shared body, held in its own `static` (`typeof`'s
/// type names, `"true"`/`"false"`): [`JsString::fixed`] without the
/// table lookup, for operations on the instruction hot path.
pub(crate) struct FixedText {
    text: &'static str,
    body: OnceLock<JsString>,
}

impl FixedText {
    pub(crate) const fn new(text: &'static str) -> Self {
        Self {
            text,
            body: OnceLock::new(),
        }
    }

    /// The shared body, made on first use and never released.
    pub(crate) fn get(&self) -> JsString {
        self.body.get_or_init(|| JsString::from(self.text)).clone()
    }

    pub(crate) fn as_str(&self) -> &'static str {
        self.text
    }

    /// Compare exact code units without re-encoding the Rust spelling.
    pub(crate) fn matches(&self, text: &JsString) -> bool {
        self.body.get_or_init(|| JsString::from(self.text)) == text
    }
}

impl From<&str> for JsString {
    /// # Panics
    ///
    /// If `s` encodes to more than [`MAX_STRING_LEN`] code units.
    fn from(s: &str) -> Self {
        let units: Vec<u16> = s.encode_utf16().collect();
        JsString::from_code_units(units).expect("string longer than MAX_STRING_LEN")
    }
}

/// Incremental KMP search over UTF-16 units. Prefix construction and scanning
/// both resume between bounded batches of comparisons. Buffers are immutable
/// leaf strings and Rust-owned indices, with no collector edges.
#[derive(Debug)]
pub(crate) struct StringSearch {
    text: JsString,
    needle: JsString,
    prefix: Vec<usize>,
    matched: usize,
    next: usize,
    end: usize,
    last: bool,
    result: isize,
    done: bool,
    reservation: Option<Arc<crate::heap::budget::ByteReservation>>,
}

impl StringSearch {
    pub(crate) fn new_accounted(
        heap: &crate::heap::Heap,
        text: JsString,
        needle: JsString,
        position: usize,
        last: bool,
    ) -> Result<Self, crate::vm::dispatch::Fault> {
        let reservation = JsString::reserve(
            heap,
            needle
                .len()
                .checked_mul(8)
                .ok_or(crate::vm::dispatch::Fault::HeapLimit)?,
            1,
        )?;
        let mut search = Self::new(text, needle, position, last);
        search.reservation = reservation;
        Ok(search)
    }
    /// Reserve before a new consumer starts scanning, so prefix-table growth
    /// never copies an input-sized buffer inside a charged comparison step.
    pub(crate) fn reserve_prefix(&mut self) -> Result<(), std::collections::TryReserveError> {
        self.prefix
            .try_reserve_exact(self.needle.len().saturating_sub(self.prefix.len()))
    }

    /// Resume after a consumed match without rebuilding the prefix table.
    /// Split/replacement consume non-overlapping matches, so no partial match survives.
    pub(crate) fn restart(&mut self, position: usize) {
        self.next = position;
        self.matched = 0;
        self.result = -1;
        self.done = self.needle.len() > self.text.len();
    }

    /// Position is already clamped to [0, text.len()]. For reverse search,
    /// scan forward through the last eligible match and retain its start.
    pub(crate) fn new(text: JsString, needle: JsString, position: usize, last: bool) -> Self {
        let done = needle.is_empty() || needle.len() > text.len();
        let result = if needle.is_empty() {
            position as isize
        } else {
            -1
        };
        let end = if last {
            position.saturating_add(needle.len()).min(text.len())
        } else {
            text.len()
        };
        Self {
            text,
            needle,
            prefix: vec![0],
            matched: 0,
            next: if last { 0 } else { position },
            end,
            last,
            result,
            done,
            reservation: None,
        }
    }

    /// At most `comparisons` code-unit comparisons, including KMP fallback
    /// work. O(text.len() + needle.len()) total, O(needle.len()) index storage.
    pub(crate) fn advance(&mut self, comparisons: usize) -> Option<isize> {
        if self.done {
            return Some(self.result);
        }
        let needle = self.needle.code_units();
        for _ in 0..comparisons {
            if self.prefix.len() < needle.len() {
                if needle[self.prefix.len()] == needle[self.matched] {
                    self.matched += 1;
                    self.prefix.push(self.matched);
                } else if self.matched > 0 {
                    self.matched = self.prefix[self.matched - 1];
                } else {
                    self.prefix.push(0);
                }
                if self.prefix.len() == needle.len() {
                    self.matched = 0;
                }
                continue;
            }
            if self.next == self.end {
                self.done = true;
                return Some(self.result);
            }
            if self.text.code_units()[self.next] == needle[self.matched] {
                self.next += 1;
                self.matched += 1;
                if self.matched == needle.len() {
                    self.result = (self.next - needle.len()) as isize;
                    if !self.last {
                        self.done = true;
                        return Some(self.result);
                    }
                    self.matched = self.prefix[self.matched - 1];
                }
            } else if self.matched > 0 {
                self.matched = self.prefix[self.matched - 1];
            } else {
                self.next += 1;
            }
        }
        None
    }
}

/// Incremental string split. Each advance scans or copies at most 256 units
/// (including KMP fallback work), or emits one code-unit/whole-string part.
/// This owns only immutable leaf strings and Rust buffers, with no heap edges.
#[derive(Debug)]
pub(crate) struct StringSplit {
    text: JsString,
    separator: Option<JsString>,
    search: Option<StringSearch>,
    build: Option<StringBuild>,
    start: usize,
    tail: bool,
    done: bool,
    reservation: Option<Arc<crate::heap::budget::ByteReservation>>,
}

pub(crate) enum SplitStep {
    Continue,
    Part(JsString),
    Done,
}

impl StringSplit {
    pub(crate) fn new_accounted(
        heap: &crate::heap::Heap,
        text: JsString,
        separator: Option<JsString>,
    ) -> Result<Self, crate::vm::dispatch::Fault> {
        // The complete partition shares one reservation, including each possible one-unit body.
        let reservation = JsString::reserve(
            heap,
            text.len()
                .checked_add(separator.as_ref().map_or(0, JsString::len))
                .and_then(|n| n.checked_mul(512))
                .ok_or(crate::vm::dispatch::Fault::HeapLimit)?,
            1,
        )?;
        let mut split = Self::new(text, separator);
        split.reservation = reservation;
        Ok(split)
    }
    pub(crate) fn new(text: JsString, separator: Option<JsString>) -> Self {
        let search = separator
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|s| StringSearch::new(text.clone(), s.clone(), 0, false));
        Self {
            text,
            separator,
            search,
            build: None,
            start: 0,
            tail: false,
            done: false,
            reservation: None,
        }
    }

    pub(crate) fn advance(&mut self) -> Option<SplitStep> {
        if self.done {
            return Some(SplitStep::Done);
        }
        if let Some(build) = &mut self.build {
            return Some(match build.advance(256) {
                None => SplitStep::Continue,
                Some(part) => {
                    self.build = None;
                    self.done = self.tail;
                    SplitStep::Part(part.with_reservation(self.reservation.clone()))
                }
            });
        }
        let Some(separator) = &self.separator else {
            self.done = true;
            return Some(SplitStep::Part(self.text.clone()));
        };
        if separator.is_empty() {
            return Some(match self.text.code_unit_at(self.start) {
                None => {
                    self.done = true;
                    SplitStep::Done
                }
                Some(part) => {
                    self.start += 1;
                    SplitStep::Part(part.with_reservation(self.reservation.clone()))
                }
            });
        }
        let search = self.search.as_mut()?;
        let Some(index) = search.advance(256) else {
            return Some(SplitStep::Continue);
        };
        let end = if index < 0 {
            self.tail = true;
            self.text.len()
        } else {
            index as usize
        };
        self.build = Some(StringBuild::new(vec![(
            self.text.clone(),
            self.start,
            end - self.start,
        )])?);
        self.start = end + if self.tail { 0 } else { separator.len() };
        if !self.tail {
            search.restart(self.start);
        }
        Some(SplitStep::Continue)
    }
}

impl PartialEq for JsString {
    fn eq(&self, other: &Self) -> bool {
        self.same_body(other)
            || (self.len() == other.len() && self.code_units() == other.code_units())
    }
}

impl Eq for JsString {}

impl PartialEq<str> for JsString {
    fn eq(&self, other: &str) -> bool {
        self.code_units().iter().copied().eq(other.encode_utf16())
    }
}

impl PartialEq<&str> for JsString {
    fn eq(&self, other: &&str) -> bool {
        *self == **other
    }
}

impl Hash for JsString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.code_units().hash(state);
    }
}

/// Lexicographic order over code units: JavaScript's relational order for
/// two strings. See the module docs.
impl Ord for JsString {
    fn cmp(&self, other: &Self) -> Ordering {
        self.code_units().cmp(other.code_units())
    }
}

impl PartialOrd for JsString {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

/// Exact: a quoted, escaped literal like `str`'s `Debug`, except that a lone
/// surrogate is written as the escape `\u{d800}` (not U+FFFD), so two
/// different strings never print the same.
impl fmt::Debug for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("\"")?;
        let mut run = String::new();
        for unit in char::decode_utf16(self.code_units().iter().copied()) {
            match unit {
                Ok(c) => run.push(c),
                Err(lone) => {
                    write_escaped_run(f, &run)?;
                    run.clear();
                    write!(f, "\\u{{{:x}}}", lone.unpaired_surrogate())?;
                }
            }
        }
        write_escaped_run(f, &run)?;
        f.write_str("\"")
    }
}

/// Writes `run` escaped as `str`'s `Debug` does, without the quotes.
fn write_escaped_run(f: &mut fmt::Formatter<'_>, run: &str) -> fmt::Result {
    if run.is_empty() {
        return Ok(());
    }
    let quoted = format!("{run:?}");
    f.write_str(&quoted[1..quoted.len() - 1])
}

/// JavaScript Number::toString(x) in radix 10 (ECMA-262 Number::toString,
/// QuickJS `js_dtoa2` via `JS_ToStringInternal`, `research/quickjs/quickjs.c`,
/// revision 7d1c13c5d8f8): `"NaN"`, `"0"` for both zeros, `"Infinity"` /
/// `"-Infinity"`, otherwise the shortest decimal digits that round-trip to
/// `x`, laid out in plain decimal when `1e-6 <= |x| < 1e21` and in exponent
/// form (`"1e+21"`, `"1e-7"`, `"1.5e-7"`) otherwise.
///
/// Only the layout below is JavaScript's own; Rust's `{}` layout differs
/// (`1e21` prints `1000000000000000000000`, `1e-7` prints `0.0000001`) and
/// is not used. The digits are [`shortest_digits`]. Other radixes use
/// [`number_to_string_radix`].
pub fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x == 0.0 {
        return "0".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let (digits, n) = shortest_digits(x.abs());
    let k = digits.len() as i32;
    let mut out = String::with_capacity(digits.len() + 8);
    if x < 0.0 {
        out.push('-');
    }
    if k <= n && n <= 21 {
        // An integer: the digits, then n - k zeros.
        out.push_str(&digits);
        out.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n && n <= 21 {
        // The point falls inside the digits.
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        // A small fraction: "0.", -n zeros, the digits.
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n) as usize));
        out.push_str(&digits);
    } else {
        // Exponent form: d[.ddd]e±(n - 1).
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if n > 0 { '+' } else { '-' });
        out.push_str(&(n - 1).unsigned_abs().to_string());
    }
    out
}

/// The digits `s` (as ASCII, no trailing zero) and exponent `n` of a finite
/// positive `x` per Number::toString: `x = 0.s × 10^n` where `s` has as few
/// digits `k` as possible such that it round-trips to `x`; among several
/// such `s`, the one closest to `x`, and of two equally close, the even one.
///
/// Rust's `{:e}` gives the shortest round-trip digits, so it fixes `k`, but
/// on an exact decimal tie it rounds up (`1000000000000000.25` gives
/// `...3`, JavaScript requires `...2`). Rust's `{:.*e}` with `k - 1`
/// fractional digits is the correctly rounded `k`-digit decimal of the
/// exact binary value, ties to even, i.e. the closest `k`-digit candidate;
/// it is used whenever it round-trips. It may not (next to a power of two
/// the round-trip interval is narrower below `x` than above, so the closest
/// candidate can fall outside it); then the round-trip candidates all lie on
/// the wide side and `{:e}`'s choice stands.
pub(crate) fn shortest_digits(x: f64) -> (String, i32) {
    let parse = |sci: &str| -> (String, i32) {
        let (mantissa, exponent) = sci
            .split_once('e')
            .expect("`{:e}` of a finite number has an exponent");
        // `mantissa` is `d` or `d.ddd`, all ASCII.
        let mut digits: String = mantissa.chars().filter(|c| *c != '.').collect();
        while digits.len() > 1 && digits.ends_with('0') {
            digits.pop();
        }
        let n = exponent
            .parse::<i32>()
            .expect("`{:e}` exponent is an integer")
            + 1;
        (digits, n)
    };
    let shortest = format!("{x:e}");
    let (digits, n) = parse(&shortest);
    let closest = format!("{x:.*e}", digits.len() - 1);
    if closest != shortest && closest.parse::<f64>() == Ok(x) {
        return parse(&closest);
    }
    (digits, n)
}

/// Number formatting in radix 2..36 (clj-quickjs-149). Radix 10 retains
/// Number::toString's decimal layout; other radixes never use exponents.
///
/// Semantic reference: `js_dtoa`'s free-format path in
/// research/quickjs/dtoa.c, revision 7d1c13c5d8f8. Written afresh using exact
/// remainders and binary64 rounding boundaries rather than floating-point
/// repeated multiplication. Integers are emitted exactly at every magnitude.
/// A fractional prefix stops as soon as rounding it up or down identifies x;
/// when both do, choose the nearest, ties to even. Boundary inclusion follows
/// the even binary significand, including subnormals and powers of two.
///
/// This leaf has a fixed bound: at most 1024 integer digits or 1075 fractional
/// digits, each operating on 34 u32 limbs (1088 bits). No input-sized loop,
/// JavaScript callback, heap edge or retained native continuation is needed.
///
/// # Panics
/// Panics if radix is outside 2..36. Callers validate it before entry.
pub fn number_to_string_radix(x: f64, radix: u32) -> String {
    assert!((2..=36).contains(&radix));
    if radix == 10 || !x.is_finite() || x == 0.0 {
        return number_to_string(x);
    }
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let bits = x.abs().to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let mut mantissa = bits & ((1 << 52) - 1);
    let e = if exponent == 0 {
        -1074
    } else {
        mantissa |= 1 << 52;
        exponent - 1023 - 52
    };
    let mut integer = if e >= 0 {
        RadixInteger::shifted(mantissa, e as usize)
    } else {
        RadixInteger::shifted(mantissa.checked_shr((-e) as u32).unwrap_or(0), 0)
    };
    let mut digits = Vec::new();
    loop {
        digits.push(integer.divide(radix) as u8);
        if integer == RadixInteger::ZERO {
            break;
        }
    }
    digits.reverse();
    let mut integer_len = digits.len();
    if e < 0 {
        // One common exact denominator 2^1075 covers every binary64 half-ulp.
        let fractional_bits = if -e >= 64 {
            mantissa
        } else {
            mantissa & ((1 << -e) - 1)
        };
        let mut remainder = RadixInteger::shifted(fractional_bits, (e + 1075) as usize);
        if remainder != RadixInteger::ZERO {
            let upper_bit = (e + 1074) as usize;
            let mut upper = RadixInteger::shifted(1, upper_bit);
            let mut lower = RadixInteger::shifted(
                1,
                upper_bit - usize::from(exponent > 1 && mantissa == 1 << 52),
            );
            let inclusive = mantissa & 1 == 0;
            for _ in 0..1075 {
                remainder.multiply(radix);
                digits.push(remainder.take_digit() as u8);
                lower.multiply(radix);
                upper.multiply(radix);
                let down = remainder.within(&lower, inclusive);
                let up = RadixInteger::DENOMINATOR
                    .minus(&remainder)
                    .within(&upper, inclusive);
                if !down && !up {
                    continue;
                }
                let half = RadixInteger::shifted(1, 1074);
                let round_up = up
                    && (!down
                        || remainder.cmp(&half).is_gt()
                        || (remainder == half
                            && digits
                                .iter()
                                .fold(0, |p, d| (p * radix + u32::from(*d)) & 1)
                                != 0));
                if round_up {
                    let mut at = digits.len();
                    loop {
                        at -= 1;
                        digits[at] += 1;
                        if u32::from(digits[at]) != radix {
                            break;
                        }
                        digits[at] = 0;
                        if at == 0 {
                            // A non-integer binary64 is below 2^52, so its
                            // exact integer part cannot overflow to a new digit
                            // except through this final fractional rounding.
                            digits.insert(0, 1);
                            integer_len += 1;
                            break;
                        }
                    }
                }
                break;
            }
        }
    }
    while digits.len() > integer_len && digits.last() == Some(&0) {
        digits.pop();
    }
    let mut out = String::with_capacity(digits.len() + 2);
    if x < 0.0 {
        out.push('-');
    }
    for (i, digit) in digits.into_iter().enumerate() {
        if i == integer_len {
            out.push('.');
        }
        out.push(DIGITS[digit as usize] as char);
    }
    out
}

/// Fixed storage for radix formatting only. The denominator is 2^1075;
/// multiplying a remainder or boundary by at most 36 fits in 1088 bits.
#[derive(Clone, Copy, PartialEq, Eq)]
struct RadixInteger([u32; 34]);

impl RadixInteger {
    const ZERO: Self = Self([0; 34]);
    const DENOMINATOR: Self = {
        let mut limbs = [0; 34];
        limbs[33] = 1 << 19;
        Self(limbs)
    };

    fn shifted(value: u64, shift: usize) -> Self {
        let mut out = Self::ZERO;
        for bit in 0..64 {
            if value & (1 << bit) != 0 {
                let at = shift + bit;
                out.0[at / 32] |= 1 << (at % 32);
            }
        }
        out
    }

    fn multiply(&mut self, radix: u32) {
        let mut carry = 0;
        for limb in &mut self.0 {
            carry += u64::from(*limb) * u64::from(radix);
            *limb = carry as u32;
            carry >>= 32;
        }
        debug_assert_eq!(carry, 0);
    }

    fn divide(&mut self, radix: u32) -> u32 {
        let mut remainder = 0;
        for limb in self.0.iter_mut().rev() {
            let value = (remainder << 32) | u64::from(*limb);
            *limb = (value / u64::from(radix)) as u32;
            remainder = value % u64::from(radix);
        }
        remainder as u32
    }

    fn take_digit(&mut self) -> u32 {
        let digit = self.0[33] >> 19;
        self.0[33] &= (1 << 19) - 1;
        digit
    }

    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.iter().rev().cmp(other.0.iter().rev())
    }

    fn within(&self, boundary: &Self, inclusive: bool) -> bool {
        let order = self.cmp(boundary);
        order.is_lt() || (inclusive && order.is_eq())
    }

    fn minus(&self, other: &Self) -> Self {
        let mut out = *self;
        let mut borrow = false;
        for (limb, rhs) in out.0.iter_mut().zip(other.0) {
            let (value, a) = limb.overflowing_sub(rhs);
            let (value, b) = value.overflowing_sub(u32::from(borrow));
            *limb = value;
            borrow = a || b;
        }
        debug_assert!(!borrow);
        out
    }
}

/// True for a StrWhiteSpaceChar code unit (ECMA-262 StringToNumber):
/// WhiteSpace (TAB, VT, FF, ZWNBSP U+FEFF and every Unicode `Zs` character:
/// space, NBSP U+00A0, U+1680, U+2000..=U+200A, U+202F, U+205F, U+3000) or a
/// LineTerminator (LF, CR, U+2028, U+2029). All of them are BMP characters,
/// so one code unit decides. (QuickJS tests the same set, the
/// regular-expression `\s` characters, with `lre_is_space`,
/// `research/quickjs/libunicode.h`, revision 7d1c13c5d8f8.)
pub(crate) fn is_str_white_space(u: u16) -> bool {
    matches!(
        u,
        0x09..=0x0D
            | 0x20
            | 0xA0
            | 0x1680
            | 0x2000..=0x200A
            | 0x2028
            | 0x2029
            | 0x202F
            | 0x205F
            | 0x3000
            | 0xFEFF
    )
}

/// JavaScript StringToNumber (ECMA-262 §7.1.4.1.1; QuickJS `js_atof` with
/// the flags `JS_ToNumberHintFree` passes, `research/quickjs/quickjs.c`,
/// revision 7d1c13c5d8f8). Leading and trailing StrWhiteSpace is ignored;
/// what remains must be, in full:
///
/// * nothing: `0`;
/// * `Infinity`, `+Infinity` or `-Infinity` (case-sensitive);
/// * an unsigned `0x`/`0X`, `0o`/`0O` or `0b`/`0B` integer with at least one
///   digit, of any length, correctly rounded (round half to even);
/// * a decimal literal with an optional sign, fraction and exponent: `12`,
///   `-1.5`, `.5`, `5.`, `1e3`, `-0` (negative zero), correctly rounded.
///   Leading zeros are decimal (`"010"` is 10, no legacy octal).
///
/// Anything else is NaN: numeric separators (`1_000`), a BigInt suffix
/// (`1n`), a signed radix prefix (`-0x1`), trailing garbage (`12px`),
/// another spelling of infinity (`infinity`, `inf`), or a non-ASCII digit.
pub fn string_to_number(s: &JsString) -> f64 {
    let units = s.code_units();
    let start = units
        .iter()
        .position(|&u| !is_str_white_space(u))
        .unwrap_or(units.len());
    let end = units
        .iter()
        .rposition(|&u| !is_str_white_space(u))
        .map_or(start, |i| i + 1);
    let trimmed = &units[start..end];
    if trimmed.is_empty() {
        return 0.0;
    }
    // Every accepted form is ASCII; anything else is NaN.
    let Some(text) = trimmed
        .iter()
        .map(|&u| u8::try_from(u).ok().filter(u8::is_ascii))
        .collect::<Option<Vec<u8>>>()
    else {
        return f64::NAN;
    };
    match text.as_slice() {
        b"Infinity" | b"+Infinity" => return f64::INFINITY,
        b"-Infinity" => return f64::NEG_INFINITY,
        [b'0', b'x' | b'X', digits @ ..] => return radix_integer(digits, 4),
        [b'0', b'o' | b'O', digits @ ..] => return radix_integer(digits, 3),
        [b'0', b'b' | b'B', digits @ ..] => return radix_integer(digits, 1),
        _ => {}
    }
    if !is_str_decimal_literal(&text) {
        return f64::NAN;
    }
    // The text is ASCII and matches StrDecimalLiteral without `Infinity`,
    // a subset of the grammar Rust's correctly rounded `f64` parser
    // accepts, with the same meaning.
    std::str::from_utf8(&text)
        .ok()
        .and_then(|t| t.parse::<f64>().ok())
        .unwrap_or(f64::NAN)
}

/// True when `t` is `[+-]? (D+ ('.' D*)? | '.' D+) ([eE] [+-]? D+)?` with
/// ASCII decimal digits `D`.
fn is_str_decimal_literal(t: &[u8]) -> bool {
    let mut prefix = DecimalPrefix::default();
    t.iter().all(|&c| prefix.push(u16::from(c))) && prefix.complete()
}

/// Strict, budgeted StringToNumber for native callers. Flatten, trim and
/// validate the entire token before reusing the bounded numeric accumulator.
/// Unlike parseFloat/parseInt, trailing garbage and incomplete exponents fail.
#[derive(Debug)]
pub(crate) struct StringToNumber {
    flatten: Option<StringFlatten>,
    text: Option<JsString>,
    next: usize,
    start: usize,
    end: usize,
    validating: bool,
    grammar: DecimalPrefix,
    radix: Option<u32>,
    parser: Option<NumberParse>,
}

impl StringToNumber {
    pub(crate) fn new_accounted(
        heap: &crate::heap::Heap,
        text: JsString,
    ) -> Result<Self, crate::vm::dispatch::Fault> {
        let flatten = StringFlatten::new_accounted(heap, text)?;
        Ok(Self {
            flatten: Some(flatten),
            text: None,
            next: 0,
            start: usize::MAX,
            end: 0,
            validating: false,
            grammar: DecimalPrefix::default(),
            radix: None,
            parser: None,
        })
    }
    #[cfg(test)]
    pub(crate) fn new(text: JsString) -> Option<Self> {
        Some(Self {
            flatten: Some(StringFlatten::new(text)?),
            text: None,
            next: 0,
            start: usize::MAX,
            end: 0,
            validating: false,
            grammar: DecimalPrefix::default(),
            radix: None,
            parser: None,
        })
    }

    pub(crate) fn advance(&mut self, budget: usize) -> Option<f64> {
        if let Some(flatten) = &mut self.flatten {
            self.text = Some(flatten.advance(budget)?);
            self.flatten = None;
            return None;
        }
        if let Some(parser) = &mut self.parser {
            return parser.advance(budget);
        }
        let text = self.text.as_ref().unwrap();
        let units = text.code_units(); // Incrementally flattened above.
        if !self.validating {
            for _ in 0..budget {
                if self.next == units.len() {
                    if self.start == usize::MAX {
                        return Some(0.0);
                    }
                    let token = &units[self.start..self.end];
                    if token.iter().take(10).copied().eq("Infinity".encode_utf16())
                        || token
                            .iter()
                            .take(10)
                            .copied()
                            .eq("+Infinity".encode_utf16())
                    {
                        return Some(f64::INFINITY);
                    }
                    if token
                        .iter()
                        .take(10)
                        .copied()
                        .eq("-Infinity".encode_utf16())
                    {
                        return Some(f64::NEG_INFINITY);
                    }
                    self.radix = match token {
                        [48, 88 | 120, ..] => Some(16),
                        [48, 79 | 111, ..] => Some(8),
                        [48, 66 | 98, ..] => Some(2),
                        _ => None,
                    };
                    self.start += if self.radix.is_some() { 2 } else { 0 };
                    if self.start == self.end {
                        return Some(f64::NAN);
                    }
                    self.next = self.start;
                    self.validating = true;
                    return None;
                }
                if !is_str_white_space(units[self.next]) {
                    self.start = self.start.min(self.next);
                    self.end = self.next + 1;
                }
                self.next += 1;
            }
        } else {
            for _ in 0..budget {
                if self.next == self.end {
                    if self.radix.is_none() && !self.grammar.complete() {
                        return Some(f64::NAN);
                    }
                    self.parser = Some(NumberParse::range(
                        text.clone(),
                        self.radix,
                        self.start,
                        self.end,
                    ));
                    return None;
                }
                let c = units[self.next];
                let valid = if let Some(radix) = self.radix {
                    char::from_u32(u32::from(c)).is_some_and(|c| c.is_digit(radix))
                } else {
                    self.grammar.push(c)
                };
                if !valid {
                    return Some(f64::NAN);
                }
                self.next += 1;
            }
        }
        None
    }
}

/// The shared StrDecimalLiteral prefix grammar (Infinity is handled by
/// callers). State 3 is a point with no digit yet; 5/6 are an incomplete
/// exponent, which parseFloat discards but StringToNumber rejects.
#[derive(Debug, Default)]
struct DecimalPrefix(u8);

impl DecimalPrefix {
    fn push(&mut self, c: u16) -> bool {
        let digit = (48..=57).contains(&c);
        self.0 = match (self.0, c) {
            (0, 43 | 45) => 1,
            (0..=2, _) if digit => 2,
            (0 | 1, 46) => 3,
            (2, 46) => 4,
            (3 | 4, _) if digit => 4,
            (2 | 4, 69 | 101) => 5,
            (5, 43 | 45) => 6,
            (5..=7, _) if digit => 7,
            _ => return false,
        };
        true
    }

    fn complete(&self) -> bool {
        matches!(self.0, 2 | 4 | 7)
    }
}

/// Budgeted parseInt/parseFloat scanning over immutable leaf storage. No
/// heap edges. Each advance inspects at most `budget` input units; integer
/// arithmetic touches at most 32 limbs per unit and decimal finalization
/// parses at most 1100 retained digits plus a sticky digit and exponent.
#[derive(Debug)]
pub(crate) struct NumberParse {
    text: JsString,
    next: usize,
    end: usize,
    started: bool,
    negative: bool,
    radix: Option<u32>,
    integer: [u32; 32],
    any: bool,
    decimal: DecimalPrefix,
    digits: String,
    fraction: i64,
    dropped: i64,
    sticky: bool,
    exponent: i64,
    exponent_negative: bool,
}

impl NumberParse {
    /// `None` selects parseFloat; `Some(0)` detects hex for parseInt.
    pub(crate) fn new(text: JsString, radix: Option<u32>) -> Self {
        let end = text.len();
        Self::range(text, radix, 0, end)
    }

    /// Scan an already delimited token without copying its input body.
    pub(crate) fn range(text: JsString, radix: Option<u32>, start: usize, end: usize) -> Self {
        debug_assert!(start <= end && end <= text.len());
        Self {
            text,
            radix,
            next: start,
            end,
            started: false,
            negative: false,
            integer: [0; 32],
            any: false,
            decimal: DecimalPrefix::default(),
            digits: String::new(),
            fraction: 0,
            dropped: 0,
            sticky: false,
            exponent: 0,
            exponent_negative: false,
        }
    }

    pub(crate) fn advance(&mut self, budget: usize) -> Option<f64> {
        for _ in 0..budget {
            let Some(&c) = self.text.code_units()[..self.end].get(self.next) else {
                return Some(self.finish());
            };
            if !self.started {
                self.next += 1;
                if is_str_white_space(c) {
                    continue;
                }
                self.started = true;
                self.negative = c == 45;
                if !matches!(c, 43 | 45) {
                    self.next -= 1;
                }
                let rest = &self.text.code_units()[self.next..self.end];
                if let Some(radix) = &mut self.radix {
                    if matches!(*radix, 0 | 16)
                        && rest.first() == Some(&48)
                        && matches!(rest.get(1), Some(88 | 120))
                    {
                        *radix = 16;
                        self.next += 2;
                    } else if *radix == 0 {
                        *radix = 10;
                    }
                } else if rest.len() >= 8 && rest[..8].iter().copied().eq("Infinity".encode_utf16())
                {
                    return Some(self.signed(f64::INFINITY));
                }
                continue;
            }
            self.next += 1;
            if let Some(radix) = self.radix {
                let digit = match c {
                    48..=57 => u32::from(c - 48),
                    65..=90 => u32::from(c - 65) + 10,
                    97..=122 => u32::from(c - 97) + 10,
                    _ => return Some(self.finish()),
                };
                if digit >= radix {
                    return Some(self.finish());
                }
                self.any = true;
                let mut carry = u64::from(digit);
                for limb in &mut self.integer {
                    carry += u64::from(*limb) * u64::from(radix);
                    *limb = carry as u32;
                    carry >>= 32;
                }
                if carry != 0 {
                    // Every possible extension is already beyond 2^1024.
                    return Some(self.signed(f64::INFINITY));
                }
            } else {
                let before = self.decimal.0;
                // The optional sign was consumed above; a second sign is invalid.
                if (before == 0 && matches!(c, 43 | 45)) || !self.decimal.push(c) {
                    return Some(self.finish());
                }
                if self.decimal.0 == 7 {
                    // Greater than twice MAX_STRING_LEN, so saturation cannot
                    // hide cancellation against a long fraction/significand.
                    self.exponent = (self.exponent * 10 + i64::from(c - 48)).min(4_000_000_000);
                } else if before == 5 && c == 45 {
                    self.exponent_negative = true;
                } else if (48..=57).contains(&c) {
                    self.any = true;
                    if matches!(before, 3 | 4) {
                        self.fraction += 1;
                    }
                    if c != 48 || !self.digits.is_empty() {
                        if self.digits.len() < 1100 {
                            self.digits.push(c as u8 as char);
                        } else {
                            self.dropped += 1;
                            self.sticky |= c != 48;
                        }
                    }
                }
            }
        }
        None
    }

    fn signed(&self, n: f64) -> f64 {
        if self.negative { -n } else { n }
    }

    fn finish(&self) -> f64 {
        if !self.any {
            return f64::NAN;
        }
        if self.radix.is_some() {
            // Stream the exact (at most 1024-bit) integer into the same
            // top-bits/guard/sticky rounding rule used by StringToNumber.
            let mut mant = 0u64;
            let mut dropped = 0u64;
            let mut sticky = false;
            for limb in self.integer.iter().rev() {
                for k in (0..32).rev() {
                    let bit = u64::from((limb >> k) & 1);
                    if mant >> 63 == 0 {
                        mant = (mant << 1) | bit;
                    } else {
                        dropped += 1;
                        sticky |= bit != 0;
                    }
                }
            }
            return self.signed(round_integer(mant, dropped, sticky));
        }
        if self.digits.is_empty() {
            return self.signed(0.0);
        }
        let mut shift = self.dropped - self.fraction;
        if self.decimal.0 == 7 {
            shift += if self.exponent_negative {
                -self.exponent
            } else {
                self.exponent
            };
        }
        let mut digits = self.digits.clone();
        if self.sticky {
            // A binary64 rounding boundary is a dyadic rational with at
            // most 1075 fractional decimal places and fewer than 1100
            // significant digits. Beyond 1100 digits only nonzeroness can
            // affect which side of that boundary this value occupies.
            digits.push('1');
            shift -= 1;
        }
        digits.push('e');
        digits.push_str(&shift.to_string());
        self.signed(digits.parse::<f64>().expect("validated decimal"))
    }
}

/// The value of the unsigned integer whose digits in radix
/// `2^bits_per_digit` (2, 8 or 16) are `digits`, correctly rounded to the
/// nearest `f64` (ties to even), or NaN when `digits` is empty or holds a
/// character that is not a digit of the radix. Any length is accepted;
/// past 2^1024 the value is Infinity.
fn radix_integer(digits: &[u8], bits_per_digit: u32) -> f64 {
    if digits.is_empty() {
        return f64::NAN;
    }
    let radix = 1u32 << bits_per_digit;
    // The first 64 significant bits, the number of later bits (`dropped`)
    // and whether any of those was 1 (`sticky`).
    let mut mant: u64 = 0;
    let mut dropped: u64 = 0;
    let mut sticky = false;
    for &c in digits {
        let Some(d) = char::from(c).to_digit(radix) else {
            return f64::NAN;
        };
        for k in (0..bits_per_digit).rev() {
            let bit = u64::from((d >> k) & 1);
            if mant >> 63 == 0 {
                mant = (mant << 1) | bit;
            } else {
                dropped += 1;
                sticky |= bit == 1;
            }
        }
    }
    round_integer(mant, dropped, sticky)
}

fn round_integer(mant: u64, dropped: u64, sticky: bool) -> f64 {
    if mant == 0 {
        return 0.0;
    }
    // Keep the top 53 significant bits of `mant`; the bits below them and
    // `sticky` round half to even. (Bits are dropped only once `mant` holds
    // 64 significant bits, so `shift` is 0 only when nothing was dropped.)
    let shift = 11u32.saturating_sub(mant.leading_zeros());
    let mut m = mant >> shift;
    if shift > 0 {
        let rest = mant & ((1u64 << shift) - 1);
        let half = 1u64 << (shift - 1);
        if rest > half || (rest == half && (sticky || m & 1 == 1)) {
            m += 1;
        }
    }
    // `m <= 2^53` (a carry may reach 2^53 exactly), so it is exact in f64.
    let exponent = u64::from(shift) + dropped;
    if exponent > 1023 {
        return f64::INFINITY;
    }
    // 2^exponent, exact for 0 <= exponent <= 1023; the product rounds to
    // Infinity on overflow and is otherwise exact.
    let scale = f64::from_bits((exponent + 1023) << 52);
    m as f64 * scale
}

#[cfg(test)]
mod tests {
    #[test]
    fn string_to_number_budgeted() {
        for text in [
            "",
            " \u{feff}",
            "1",
            "-0",
            "+1.5",
            ".5",
            "5.",
            "1e3",
            "1e-3",
            "1e+",
            "1e",
            "1.2.3",
            "1_000",
            "0x10",
            "0Xff",
            "0b101",
            "0o77",
            "0b2",
            "0x",
            "-0x1",
            "1n",
            "Infinity",
            "+Infinity",
            "-Infinity",
            "Infinityx",
            " 1 2 ",
            "１２",
            "\u{a0}30\u{202f}",
        ] {
            let rope = JsString::from(" ")
                .concat(&JsString::from(text))
                .unwrap()
                .concat(&JsString::from(" "))
                .unwrap();
            let expected = super::string_to_number(&rope);
            let mut scan = super::StringToNumber::new(rope).unwrap();
            let actual = (0..1000)
                .find_map(|_| scan.advance(1))
                .expect("bounded scan finishes");
            assert!(
                actual.to_bits() == expected.to_bits() || actual.is_nan() && expected.is_nan(),
                "{text:?}: {actual} vs {expected}"
            );
        }
        for text in [
            format!("{}1", "0".repeat(20_000)),
            format!("1.{}1", "0".repeat(20_000)),
            format!("0x{}f", "0".repeat(20_000)),
            format!("1e{}1", "0".repeat(20_000)),
            format!("1{}x", "0".repeat(20_000)),
        ] {
            let text = JsString::from(text.as_str());
            let expected = super::string_to_number(&text);
            let mut scan = super::StringToNumber::new(text).unwrap();
            assert!(scan.advance(256).is_none());
            let actual = (0..1000)
                .find_map(|_| scan.advance(256))
                .expect("large scan finishes");
            assert!(actual.to_bits() == expected.to_bits() || actual.is_nan() && expected.is_nan());
        }
    }

    #[test]
    fn appending_after_index_reads_releases_cached_prefixes() {
        fn reachable_units(text: &JsString) -> usize {
            let mut pending = vec![text.0.as_ref()];
            let mut seen = std::collections::HashSet::new();
            let mut units = 0;
            while let Some(body) = pending.pop() {
                if !seen.insert(std::ptr::from_ref(body)) {
                    continue;
                }
                if let Some(flat) = body.flat.get() {
                    pending.push(flat.0.as_ref());
                }
                match &body.storage {
                    Storage::Flat(flat) => units += flat.len(),
                    Storage::Rope(Some((left, right))) => {
                        pending.push(left.0.as_ref());
                        pending.push(right.0.as_ref());
                    }
                    Storage::Rope(None) => unreachable!(),
                }
            }
            units
        }
        for prepend in [false, true] {
            let mut text = JsString::from("");
            for _ in 0..5000 {
                let unit = JsString::from("x");
                text = if prepend {
                    unit.concat(&text)
                } else {
                    text.concat(&unit)
                }
                .unwrap();
                assert_eq!(text.code_units()[0], b'x' as u16);
            }
            assert!(
                reachable_units(&text) <= 2 * text.len(),
                "obsolete cached prefixes remain reachable"
            );
        }
    }

    #[test]
    fn concat_assembly_resumes_across_part_batches() {
        let parts = (0..600)
            .map(|i| (JsString::from_code_units(vec![i]).unwrap(), 0, 1))
            .collect();
        let mut build = StringBuild::concatenation(parts).unwrap();
        assert!(build.advance(256).is_none());
        assert_eq!(build.concatenated.as_ref().unwrap().len(), 256);
        assert_eq!(build.parts.len(), 344);
        assert!(build.advance(256).is_none());
        assert_eq!(build.concatenated.as_ref().unwrap().len(), 512);
        assert_eq!(build.parts.len(), 88);
        let result = build.advance(256).unwrap();
        assert_eq!(result.code_units(), (0..600).collect::<Vec<u16>>());
    }

    #[test]
    fn deep_ropes_flatten_and_drop_on_a_small_stack() {
        std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(|| {
                for prepend in [false, true] {
                    let mut text = JsString::from("");
                    let mut snapshot = None;
                    let before = JsString::copied_code_units();
                    for i in 0..100_000 {
                        let unit = JsString::from_code_units(vec![i as u16]).unwrap();
                        text = if prepend {
                            unit.concat(&text)
                        } else {
                            text.concat(&unit)
                        }
                        .unwrap();
                        if i == 49_999 {
                            snapshot = Some(text.clone());
                        }
                    }
                    for (i, &unit) in text.code_units().iter().enumerate() {
                        assert_eq!(
                            unit,
                            if prepend {
                                (99_999 - i) as u16
                            } else {
                                i as u16
                            }
                        );
                    }
                    assert!(JsString::copied_code_units() - before <= 200_000);
                    drop(text);
                    assert_eq!(snapshot.as_ref().unwrap().len(), 50_000);
                    drop(snapshot); // a separately retained ancestor

                    let mut unread = JsString::from("x");
                    for _ in 0..100_000 {
                        unread = if prepend {
                            JsString::from("x").concat(&unread)
                        } else {
                            unread.concat(&JsString::from("x"))
                        }
                        .unwrap();
                    }
                    drop(unread); // destruction must also work without any cache
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn rope_content_hash_order_and_length_limit() {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut rope = JsString::from("");
        let units: Vec<u16> = (0..300).map(|i| 0xd800 + i).collect();
        for &unit in &units {
            rope = rope
                .concat(&JsString::from_code_units(vec![unit]).unwrap())
                .unwrap();
        }
        let flat = JsString::from_code_units(units).unwrap();
        assert_eq!(rope, flat);
        assert_eq!(rope.cmp(&flat), std::cmp::Ordering::Equal);
        assert_eq!(rope.content_hash(), flat.content_hash());
        let hash = |text: &JsString| {
            let mut h = DefaultHasher::new();
            text.hash(&mut h);
            h.finish()
        };
        assert_eq!(hash(&rope), hash(&flat));
        assert!(rope.concat(&JsString::from("")).unwrap().same_body(&rope));
        let before = JsString::copied_code_units();
        assert!(rope.code_unit_at(rope.len()).is_none());
        assert_eq!(JsString::copied_code_units(), before);

        // Build the exact limit as a shared DAG without a gigabyte buffer.
        let mut power = JsString::from("x");
        let mut maximum = JsString::from("");
        for _ in 0..30 {
            maximum = maximum.concat(&power).unwrap();
            if power.len() < 1 << 29 {
                power = power.concat(&power).unwrap();
            }
        }
        assert_eq!(maximum.len(), MAX_STRING_LEN);
        let before = JsString::copied_code_units();
        assert!(maximum.concat(&JsString::from("x")).is_none());
        assert!(power.concat(&power).is_none());
        assert_eq!(JsString::copied_code_units(), before);
    }

    #[test]
    fn shared_rope_dags_flatten_cached_descendants() {
        let units: Vec<u16> = (0..96).map(|i| 0xd7f0 + i).collect();
        let left = JsString::from_code_units(units[..48].to_vec()).unwrap();
        let right = JsString::from_code_units(units[48..].to_vec()).unwrap();
        let rope = left.concat(&right).unwrap();
        // Retain the same uncached rope twice before caching it, so the
        // parent traverses cached descendants rather than substituted leaves.
        let shared = rope.concat(&rope).unwrap();
        assert!(rope.0.flat.get().is_none());
        assert_eq!(rope.code_units(), units);
        let before = JsString::copied_code_units();
        let expected = [units.as_slice(), units.as_slice()].concat();
        assert_eq!(shared.code_units(), expected);
        let after = JsString::copied_code_units();
        assert_eq!(after - before, shared.len() as u64);
        assert_eq!(shared.code_units(), expected);
        assert_eq!(JsString::copied_code_units(), after);
        drop(shared);
        assert_eq!(rope.code_units(), units);
        assert_eq!(JsString::copied_code_units(), after);
    }

    #[test]
    fn concurrent_rope_readers_share_one_flat_cache() {
        use std::sync::mpsc;
        use std::time::Duration;
        let mut rope = JsString::from("");
        for _ in 0..20_000 {
            rope = rope.concat(&JsString::from("x")).unwrap();
        }
        let (done, results) = mpsc::channel();
        let mut starts = Vec::new();
        let readers: Vec<_> = (0..4)
            .map(|_| {
                let rope = rope.clone();
                let done = done.clone();
                let (start, gate) = mpsc::channel();
                starts.push(start);
                std::thread::spawn(move || {
                    gate.recv_timeout(Duration::from_secs(5))
                        .expect("reader start");
                    let before = JsString::copied_code_units();
                    let units = rope.code_units();
                    assert_eq!(units, vec![b'x' as u16; 20_000]);
                    done.send((
                        units.as_ptr() as usize,
                        JsString::copied_code_units() - before,
                    ))
                    .unwrap();
                })
            })
            .collect();
        for start in starts {
            start.send(()).unwrap();
        }
        let results: Vec<_> = (0..4)
            .map(|_| {
                results
                    .recv_timeout(Duration::from_secs(5))
                    .expect("reader completion")
            })
            .collect();
        for reader in readers {
            reader.join().unwrap();
        }
        assert!(results.iter().all(|r| r.0 == results[0].0));
        assert_eq!(results.iter().map(|r| r.1).sum::<u64>(), 20_000);
    }

    fn parsed(text: &str, radix: Option<u32>) -> f64 {
        let mut parser = NumberParse::new(JsString::from(text), radix);
        for _ in 0..text.len() + 2 {
            if let Some(number) = parser.advance(1) {
                return number;
            }
        }
        panic!("parser did not finish");
    }

    #[test]
    fn parse_integer_all_radixes_round_exact_integers_once() {
        for radix in 2..=36u32 {
            for n in [
                0,
                1,
                (1u128 << 53) + 1,
                (1u128 << 53) + 3,
                (1u128 << 100) + (1u128 << 46) + 1,
                u128::MAX,
            ] {
                let mut digits = Vec::new();
                let mut rest = n;
                loop {
                    digits
                        .push(char::from_digit((rest % u128::from(radix)) as u32, radix).unwrap());
                    rest /= u128::from(radix);
                    if rest == 0 {
                        break;
                    }
                }
                let text: String = digits.into_iter().rev().collect();
                assert_eq!(parsed(&text, Some(radix)), n as f64, "{text} radix {radix}");
            }
        }
        for text in ["1".repeat(309), "9".repeat(309), "0".repeat(2000)] {
            assert_eq!(parsed(&text, Some(10)), text.parse::<f64>().unwrap());
        }
        assert_eq!(parsed(&"f".repeat(256), Some(16)), f64::INFINITY);
        let below_overflow = format!("{}{}", "1".repeat(53), "0".repeat(971));
        assert_eq!(parsed(&below_overflow, Some(2)), f64::MAX);
    }

    #[test]
    fn parse_float_retained_digits_preserve_rounding_boundaries() {
        let midpoint = "1.00000000000000011102230246251565404236316680908203125";
        let cases = [
            format!("{midpoint}{}", "0".repeat(2000)),
            format!("{midpoint}{}1", "0".repeat(2000)),
            format!("{}e-2000", "1234567890".repeat(200)),
            format!("0.{}123e2003", "0".repeat(2000)),
            format!("{}e-9999999999999999999999999999", "9".repeat(2000)),
            format!("0.{}1e9999999999999999999999999999", "0".repeat(2000)),
            "2.4703282292062327e-324".to_string(),
            "2.4703282292062328e-324".to_string(),
            "1.7976931348623157e308".to_string(),
            "1.7976931348623159e308".to_string(),
        ];
        for text in cases {
            assert_eq!(
                parsed(&text, None).to_bits(),
                text.parse::<f64>().unwrap().to_bits(),
                "{text}"
            );
        }
    }

    #[test]
    fn incremental_string_build_checks_limits_and_moves_completed_storage() {
        let source = JsString::from("ab");
        assert!(StringBuild::new(vec![(source.clone(), 0, MAX_STRING_LEN + 1)]).is_none());
        assert!(
            StringBuild::new(vec![
                (source.clone(), 0, usize::MAX),
                (source.clone(), 0, 1)
            ])
            .is_none()
        );
        let mut unchanged = StringBuild::new(vec![(source.clone(), 0, 2)]).unwrap();
        assert!(unchanged.units.is_empty());
        assert!(unchanged.advance(1).unwrap().same_body(&source));

        let mut build = StringBuild::new(vec![(source, 1, 513)]).unwrap();
        let allocation = build.units.as_ptr();
        assert!(build.advance(256).is_none());
        assert_eq!(build.units.len(), 256);
        assert!(build.advance(256).is_none());
        assert_eq!(build.units.len(), 512);
        let result = build.advance(256).unwrap();
        assert_eq!(result.len(), 513);
        assert!(result.code_units().iter().all(|unit| *unit == b'b' as u16));
        assert_eq!(result.code_units().as_ptr(), allocation);

        // Crossing into the final full-string part must preserve the prefix;
        // the identity shortcut only applies before output has been copied.
        let tail = JsString::from("z".repeat(512).as_str());
        let mut build =
            StringBuild::new(vec![(JsString::from("x"), 0, 1), (tail, 0, 512)]).unwrap();
        assert!(build.advance(256).is_none());
        assert!(build.advance(256).is_none());
        let result = build.advance(256).unwrap();
        assert_eq!(result.len(), 513);
        assert_eq!(result.code_units()[0], b'x' as u16);
    }

    #[test]
    fn incremental_search_matches_code_unit_windows() {
        use super::{JsString, StringSearch};

        // Exhaust all short binary strings, including a lone surrogate.
        // Slice windows are an independent oracle for both search directions.
        let strings = |max_len| {
            (0..=max_len)
                .flat_map(|len| {
                    (0..1usize << len).map(move |bits| {
                        JsString::from_code_units(
                            (0..len)
                                .map(|i| if bits & (1 << i) == 0 { 97 } else { 0xd800 })
                                .collect::<Vec<_>>(),
                        )
                        .unwrap()
                    })
                })
                .collect::<Vec<_>>()
        };
        for text in strings(5) {
            for needle in strings(4) {
                for position in 0..=text.len() {
                    for last in [false, true] {
                        let expected = if needle.is_empty() {
                            position as isize
                        } else {
                            let mut candidates = text
                                .code_units()
                                .windows(needle.len())
                                .enumerate()
                                .filter(|(i, units)| {
                                    *units == needle.code_units()
                                        && if last { *i <= position } else { *i >= position }
                                })
                                .map(|(i, _)| i as isize);
                            if last {
                                candidates.next_back()
                            } else {
                                candidates.next()
                            }
                            .unwrap_or(-1)
                        };
                        let mut search =
                            StringSearch::new(text.clone(), needle.clone(), position, last);
                        let mut result = None;
                        // One comparison per step forces every KMP state to
                        // survive a boundary, including prefix fallback.
                        for _ in 0..64 {
                            result = search.advance(1);
                            if result.is_some() {
                                break;
                            }
                        }
                        assert_eq!(
                            result,
                            Some(expected),
                            "{text:?} / {needle:?}, {position}, last={last}"
                        );
                    }
                }
            }
        }
    }

    use super::*;

    /// Upstream rows adapt QuickJS test_builtin.js::test_number,
    /// revision 7d1c13c5d8f8; copyright and MIT notice are retained with
    /// tests/js_source.rs::string_code_unit_access. Other rows are synthetic.
    #[test]
    fn number_to_string_with_radix() {
        for (x, radix, expected) in [
            (1.0 - 2.0f64.powi(-53), 12, "0.bbbbbbbbbbbbbba".to_string()),
            (1.3, 7, "1.2046204620462046205".to_string()),
            (1.3, 35, "1.ahhhhhhhhhm".to_string()),
            (1000000000000000128.0, 10, "1000000000000000100".to_string()),
            (255.0, 16, "ff".to_string()),
            (-255.0, 2, "-11111111".to_string()),
            (0.5, 2, "0.1".to_string()),
            (
                0.1,
                2,
                "0.0001100110011001100110011001100110011001100110011001101".to_string(),
            ),
            (2.0f64.powi(60), 2, format!("1{}", "0".repeat(60))),
            (35.0, 36, "z".to_string()),
            (-0.0, 2, "0".to_string()),
            (f64::NAN, 2, "NaN".to_string()),
            (-f64::INFINITY, 16, "-Infinity".to_string()),
        ] {
            assert_eq!(number_to_string_radix(x, radix), expected, "{x:e}, {radix}");
        }
    }

    #[test]
    fn number_to_string_radix_rounds_halfway_coefficients_to_even() {
        // In odd bases, the parity of the whole coefficient matters.
        // 3^33 is odd, so both values lie exactly halfway between radix
        // coefficients. 33 fractional digits first fit their binary64
        // intervals: 1/(2*3^32) > half an ulp, 1/(2*3^33) < half an ulp.
        // The lower coefficient for 1.5 is even; for 2.5 it is odd.
        for (x, expected) in [
            (1.5, format!("1.{}", "1".repeat(33))),
            (2.5, format!("2.{}2", "1".repeat(32))),
        ] {
            assert_eq!(number_to_string_radix(x, 3), expected);
            assert_eq!(number_to_string_radix(-x, 3), format!("-{expected}"));
        }
    }

    #[test]
    fn radix_extremes_and_exact_integers() {
        assert_eq!(
            number_to_string_radix(f64::from_bits(1), 2),
            format!("0.{}1", "0".repeat(1073))
        );
        assert_eq!(
            number_to_string_radix(f64::MAX, 2),
            format!("{}{}", "1".repeat(53), "0".repeat(971))
        );
        assert_eq!(
            number_to_string_radix(f64::MIN_POSITIVE, 16),
            format!("0.{}4", "0".repeat(255))
        );
        assert_eq!(
            number_to_string_radix(1000000000000000128.0, 16),
            "de0b6b3a7640080"
        );
        // Every digit and radix, checked with independent native integer arithmetic.
        for radix in 2..=36 {
            for x in [1u64, 35, 255, 123456789, (1 << 53) - 1] {
                let text = number_to_string_radix(x as f64, radix);
                assert_eq!(u64::from_str_radix(&text, radix), Ok(x));
            }
            for x in [
                f64::from_bits(1),
                f64::MIN_POSITIVE,
                0.1,
                0.5,
                1.3,
                1.0 - f64::EPSILON / 2.0,
            ] {
                let text = number_to_string_radix(x, radix);
                assert!(text.len() <= 1077, "{x:e} base {radix}: {text}");
                assert!(!text.ends_with('0'), "{x:e} base {radix}: {text}");
            }
        }
    }

    #[test]
    fn number_to_string_matches_javascript() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.0, "-1"),
            (0.1, "0.1"),
            (0.5, "0.5"),
            (123.456, "123.456"),
            (-123.456, "-123.456"),
            (0.1 + 0.2, "0.30000000000000004"),
            (100.0, "100"),
            (1e20, "100000000000000000000"),
            (1e21, "1e+21"),
            (-1e21, "-1e+21"),
            (1.5e21, "1.5e+21"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (-1.5e-7, "-1.5e-7"),
            (0.000001, "0.000001"),
            (0.0000015, "0.0000015"),
            (1.5e300, "1.5e+300"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (9007199254740992.0, "9007199254740992"),
            (19686109595169230000.0, "19686109595169230000"),
            (123e-20, "1.23e-18"),
            (2147483648.0, "2147483648"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            // Exact decimal ties between two shortest candidates take the
            // even one (Rust's `{:e}` alone rounds these up).
            (1e15 + 0.25, "1000000000000000.2"),
            (-(1e15 + 0.25), "-1000000000000000.2"),
            (1e15 + 0.75, "1000000000000000.8"),
            (0.5e-6, "5e-7"),
            (0.25, "0.25"),
        ];
        for &(x, expected) in cases {
            assert_eq!(number_to_string(x), expected, "Number::toString({x:e})");
        }
    }

    #[test]
    fn number_to_string_round_trips_with_shortest_length() {
        // A deterministic sample of bit patterns across the whole range:
        // every result parses back to the same number and is never longer
        // than Rust's shortest round-trip digits.
        let mut bits: u64 = 0x9e37_79b9_7f4a_7c15;
        for _ in 0..200_000 {
            bits = bits
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let x = f64::from_bits(bits);
            if !x.is_finite() || x == 0.0 {
                continue;
            }
            let s = number_to_string(x);
            assert_eq!(s.parse::<f64>(), Ok(x), "{s} for {x:e}");
            let (digits, _) = shortest_digits(x.abs());
            let rust_digits = format!("{:e}", x.abs())
                .split('e')
                .next()
                .unwrap()
                .replace('.', "");
            assert_eq!(digits.len(), rust_digits.len(), "{x:e}");
        }
    }

    #[test]
    fn string_to_number_follows_the_string_numeric_literal_grammar() {
        let n = |s: &str| string_to_number(&JsString::from(s));
        for (s, expected) in [
            ("", 0.0),
            (
                " \t\n\r\u{0B}\u{0C}\u{A0}\u{FEFF}\u{1680}\u{2000}\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}",
                0.0,
            ),
            ("\u{3000}42\u{2029}", 42.0),
            ("010", 10.0),
            ("+7", 7.0),
            ("-.5e1", -5.0),
            ("1E+2", 100.0),
            ("1e-2", 0.01),
            ("0.1", 0.1),
            ("1e400", f64::INFINITY),
            ("-1e400", f64::NEG_INFINITY),
            ("1e-400", 0.0),
            ("0x0", 0.0),
            ("0x00000000000000000000000000001", 1.0),
            ("0xFFFFFFFFFFFFFFFF", 18446744073709551615.0),
            ("0B101", 5.0),
            ("0O777", 511.0),
            // 2^53 + 1 is a tie between 2^53 and 2^53 + 2: even wins.
            ("0x20000000000001", 9007199254740992.0),
            // 2^53 + 3 ties up to 2^53 + 4.
            ("0x20000000000003", 9007199254740996.0),
        ] {
            assert_eq!(n(s), expected, "{s:?}");
        }
        // Long radix integers round correctly at any length.
        let zeros = |k: usize| "0".repeat(k);
        for (s, expected) in [
            // Just above the 2^53 + 1 tie (times 2^64), a later 1 bit
            // rounds up.
            (
                format!("0x20000000000001{}1", zeros(15)),
                9007199254740994.0 * 18446744073709551616.0,
            ),
            // (2^53 - 1) * 2^971 is the largest finite value.
            (format!("0xfffffffffffff8{}", zeros(242)), f64::MAX),
            (format!("0xfffffffffffffb{}", "f".repeat(242)), f64::MAX),
            // Halfway between it and 2^1024 rounds to even: Infinity.
            (format!("0xfffffffffffffc{}", zeros(242)), f64::INFINITY),
            (format!("0x1{}", zeros(256)), f64::INFINITY),
            (format!("0b1{}", zeros(1023)), 2f64.powi(1023)),
            (format!("0o1{}", zeros(341)), 2f64.powi(1023)),
        ] {
            assert_eq!(n(&s), expected, "{s:?}");
        }
        assert!(n("-0").is_sign_negative() && n("-0") == 0.0);
        assert!(n("0").is_sign_positive());
        for s in [
            ".",
            "+",
            "-",
            "e5",
            "1e",
            "1e+",
            "..5",
            "1.2.3",
            "0x",
            "0xg",
            "0b2",
            "0o8",
            "-0x1",
            "+0x1",
            "1_000",
            "1n",
            "12px",
            "infinity",
            "inf",
            "NaN",
            "+-1",
            "1 2",
            "0x1.8",
            "\u{FF11}",
            "Infinityx",
            "- 1",
        ] {
            assert!(n(s).is_nan(), "{s:?} gives {}", n(s));
        }
    }

    #[test]
    fn length_and_order_are_in_code_units() {
        let emoji = JsString::from("\u{1F600}");
        assert_eq!(emoji.len(), 2, "a supplementary character is two units");
        // Indexing is per code unit: each half of the pair is a lone surrogate.
        let units = |s: Option<JsString>| s.map(|s| s.code_units().to_vec());
        assert_eq!(units(emoji.code_unit_at(0)), Some(vec![0xD83D]));
        assert_eq!(units(emoji.code_unit_at(1)), Some(vec![0xDE00]));
        assert_eq!(units(emoji.code_unit_at(2)), None);
        // U+FF61 sorts after U+1F600 by code units (0xFF61 > 0xD83D), though
        // not by scalar value.
        let [halfwidth, a, ab, b] = ["\u{FF61}", "a", "ab", "b"].map(JsString::from);
        assert!(halfwidth > emoji);
        assert!(a < ab);
        assert!(b > ab);
    }

    #[test]
    fn lone_surrogates_are_kept_and_shown_as_replacement() {
        let s = JsString::from_code_units(vec![0x61, 0xD800]).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s.code_units(), &[0x61, 0xD800]);
        assert_eq!(s.to_string_lossy(), "a\u{FFFD}");
        assert_eq!(s.to_string(), "a\u{FFFD}");
        assert_ne!(s, JsString::from("a\u{FFFD}"));
        // `Debug` is exact: a lone surrogate is an escape, everything else
        // is escaped as `str`'s `Debug` escapes it.
        assert_eq!(format!("{s:?}"), r#""a\u{d800}""#);
        let mixed =
            JsString::from_code_units(vec![0xDC00, 0x0A, 0x22, 0xD83D, 0xDE00, 0xDBFF]).unwrap();
        assert_eq!(
            format!("{mixed:?}"),
            "\"\\u{dc00}\\n\\\"\u{1F600}\\u{dbff}\""
        );
        assert_eq!(format!("{:?}", JsString::from("")), r#""""#);
        assert_eq!(
            format!("{:?}", JsString::from("a\tb")),
            format!("{:?}", "a\tb")
        );
        // Halves joined by concatenation form the pair.
        let high = JsString::from_code_units(vec![0xDBFF]).unwrap();
        let low = JsString::from_code_units(vec![0xDFFF]).unwrap();
        assert_eq!(high.concat(&low).unwrap(), JsString::from("\u{10FFFF}"));
        assert_ne!(high, low);
        assert!(high < low);
    }

    #[test]
    fn equality_and_hash_follow_content() {
        let a = JsString::from("ab");
        let b = JsString::from("a").concat(&JsString::from("b")).unwrap();
        assert!(!a.same_body(&b));
        assert_eq!(a, b);
        assert_eq!(a.content_hash(), b.content_hash());
        assert_ne!(a.content_hash(), JsString::from("ba").content_hash());
        assert_eq!(a, "ab");
    }

    #[test]
    fn fixed_spellings_share_one_body_across_threads() {
        static TEXT: FixedText = FixedText::new("fixed-text-probe");
        let here = (JsString::fixed("fixed-probe"), TEXT.get());
        let there = std::thread::spawn(|| (JsString::fixed("fixed-probe"), TEXT.get()))
            .join()
            .unwrap();
        assert!(here.0.same_body(&there.0));
        assert!(here.1.same_body(&there.1));
        assert!(here.1.same_body(&TEXT.get()));
        assert_eq!(here.0, "fixed-probe");
        assert_eq!(here.1, "fixed-text-probe");
        assert!(!JsString::fixed("fixed-probe-other").same_body(&here.0));
    }

    #[test]
    fn concatenation_with_empty_shares_the_body() {
        let a = JsString::from("x");
        let empty = JsString::from("");
        assert!(a.concat(&empty).unwrap().same_body(&a));
        assert!(empty.concat(&a).unwrap().same_body(&a));
    }
}

#[cfg(test)]
mod byte_budget_tests {
    use super::*;

    #[test]
    fn cached_flat_body_keeps_reservation_after_rope_and_leaves_drop() {
        let heap = crate::heap::Heap::with_byte_limit(256 * 1024);
        let baseline = heap.byte_budget().used();
        let left = JsString::copy_units(&heap, &[120; 1000]).unwrap();
        let rope = left.concat_accounted(&heap, &left).ok().unwrap();
        assert_eq!(rope.code_units().len(), 2000);
        let flat = rope.0.flat.get().expect("cached flat body").clone();
        drop(rope);
        drop(left);
        assert!(heap.byte_budget().used() > baseline);
        assert_eq!(flat.code_units(), &[120; 2000]);
        drop(flat);
        assert_eq!(heap.byte_budget().used(), baseline);
    }

    /// A large shared first piece and short later pieces: the output's
    /// flattened copy is admitted with the extension, not after it.
    fn accumulate(heap: &crate::heap::Heap, first: &JsString) -> JsString {
        let mut output = StringAccumulator::default();
        for piece in [first, &JsString::from(", "), &JsString::from("y")] {
            output.push(heap, piece).ok().unwrap();
        }
        output.finish()
    }

    #[test]
    fn accumulator_precharges_the_flat_copy_of_a_shared_first_piece() {
        const UNITS: usize = 100_000;
        let heap = crate::heap::Heap::with_byte_limit(4 * UNITS);
        let first = JsString::copy_units(&heap, &[120; UNITS]).unwrap();
        let used = heap.byte_budget().used();
        // The first piece is shared without a charge, but its extension
        // needs room for the whole output's flat copy, which is not left.
        let mut output = StringAccumulator::default();
        output.push(&heap, &first).ok().unwrap();
        assert_eq!(heap.byte_budget().used(), used);
        assert!(matches!(
            output.push(&heap, &JsString::from(", ")),
            Err(crate::vm::dispatch::Fault::HeapLimit)
        ));
        assert_eq!(heap.byte_budget().used(), used, "a denial keeps no charge");
        let text = output.finish();
        assert!(
            text.same_body(&first),
            "a denial leaves the output unchanged"
        );
    }

    #[test]
    fn accumulated_flat_copy_keeps_its_credit_after_the_rope_and_pieces_drop() {
        const UNITS: usize = 100_000;
        let heap = crate::heap::Heap::with_byte_limit(16 * UNITS);
        let baseline = heap.byte_budget().used();
        let first = JsString::copy_units(&heap, &[120; UNITS]).unwrap();
        let shared = heap.byte_budget().used();
        let rope = accumulate(&heap, &first);
        assert_eq!(rope.len(), UNITS + 3);
        assert!(heap.byte_budget().used() - shared >= 2 * rope.len());
        assert_eq!(rope.code_units()[UNITS..], [44, 32, 121]);
        // The cached flat copy outlives the rope, every accumulated node and
        // the first piece, as when a later concatenation adopts it.
        let flat = rope.concat_operand();
        assert!(!flat.same_body(&rope));
        drop(rope);
        drop(first);
        let retained = heap.byte_budget().used() - baseline;
        assert!(
            retained >= 2 * flat.len(),
            "flat copy uncharged: {retained}"
        );
        drop(flat);
        assert_eq!(heap.byte_budget().used(), baseline);
    }

    #[test]
    fn accumulator_debug_does_not_break_finish() {
        let heap = crate::heap::Heap::with_byte_limit(256 * 1024);
        let baseline = heap.byte_budget().used();
        let first = JsString::copy_units(&heap, &[120; 40]).unwrap();
        let mut output = StringAccumulator::default();
        output.push(&heap, &first).ok().unwrap();
        output.push(&heap, &JsString::from("y")).ok().unwrap();
        // Diagnostic formatting must not flatten the output, whose cache
        // would share the node charge that finish moves the credit into.
        let debug = format!("{output:?}");
        let text = output.finish();
        assert!(debug.contains("len: 41"), "{debug}");
        assert_eq!(text.code_units()[40..], [121]);
        drop(text);
        drop(first);
        assert_eq!(heap.byte_budget().used(), baseline);
    }
}
