//! Iterative structural equality and content hashing. Neither walks the native
//! stack with language nesting. Memoization reuses repeated collection results;
//! distinct lists still scan their logical elements, including shared tails.
use super::*;

fn mix(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}
fn combine(left: u64, right: u64) -> u64 {
    mix(left.wrapping_mul(31).wrapping_add(right))
}
fn units(units: &[u16]) -> u64 {
    // Exact UTF-16, deliberately collision-tolerant (e.g. "Aa" and "BB").
    units.iter().fold(0u64, |hash, unit| {
        hash.wrapping_mul(31).wrapping_add(u64::from(*unit))
    })
}
fn name_hash(name: &Name) -> u64 {
    combine(
        name.namespace().map_or(0, |ns| combine(1, units(ns))),
        units(name.name()),
    )
}

struct ContentHasher<'a> {
    store: &'a Store,
    memo: HashMap<HeapId, u64>,
}
#[derive(Clone, Copy)]
enum Kind {
    Sequential,
    Map,
    Record(records::TypeId),
    Set,
}
enum HashTask {
    Visit(Value),
    Finish(HeapId, Kind, usize),
}
impl<'a> ContentHasher<'a> {
    fn new(store: &'a Store) -> Self {
        Self {
            store,
            memo: HashMap::new(),
        }
    }
    fn hash(&mut self, value: Value) -> Result<u64, CollectionError> {
        let mut tasks = vec![HashTask::Visit(value)];
        let mut results = Vec::new();
        while let Some(task) = tasks.pop() {
            self.store
                .accounting
                .work(1)
                .map_err(CollectionError::Resource)?;
            match task {
                HashTask::Finish(id, kind, count) => {
                    let start = results.len() - count;
                    let children = &results[start..];
                    let hash = match kind {
                        Kind::Sequential => children
                            .iter()
                            .fold(mix(9), |acc, value| combine(acc, *value)),
                        Kind::Map | Kind::Record(_) => combine(
                            if let Kind::Record(type_id) = kind {
                                identity_hash(18, type_id)
                            } else {
                                10
                            },
                            children.as_chunks::<2>().0.iter().fold(0u64, |acc, pair| {
                                acc.wrapping_add(combine(pair[0], pair[1]))
                            }),
                        ),
                        Kind::Set => combine(
                            11,
                            children
                                .iter()
                                .fold(0u64, |acc, value| acc.wrapping_add(*value)),
                        ),
                    };
                    results.truncate(start);
                    results.push(hash);
                    self.memo.insert(id, hash);
                }
                HashTask::Visit(value) => {
                    let hash = match self.store.unannotated(value) {
                        Value::Nil => mix(1),
                        Value::Boolean(value) => combine(2, u64::from(value)),
                        Value::Integer(value) => combine(3, value as u64),
                        Value::Double(value) => combine(
                            4,
                            if value == 0.0 {
                                0
                            } else if value.is_nan() {
                                f64::NAN.to_bits()
                            } else {
                                value.to_bits()
                            },
                        ),
                        Value::Ratio(n, d) => combine(5, combine(n as u64, d as u64)),
                        Value::Character(value) => combine(6, u64::from(value)),
                        Value::Native(definition) => {
                            combine(13, std::ptr::from_ref(definition) as usize as u64)
                        }
                        Value::Namespace(id) => identity_hash(14, id),
                        Value::Var(id) => identity_hash(15, id),
                        Value::Unbound(_) => mix(16),
                        Value::Symbol(id) => {
                            combine(7, name_hash(self.store.names.get(id.0).expect("live name")))
                        }
                        Value::Keyword(id) => {
                            combine(8, name_hash(self.store.names.get(id.0).expect("live name")))
                        }
                        Value::Heap(id) => {
                            if let Some(hash) = self.memo.get(&id) {
                                results.push(*hash);
                                continue;
                            }
                            let (kind, children) =
                                match self.store.heap.get(id.0).expect("live value") {
                                    HeapValue::String(value) => {
                                        self.store
                                            .accounting
                                            .work(value.len() as u64)
                                            .map_err(CollectionError::Resource)?;
                                        let hash = combine(12, units(value));
                                        self.memo.insert(id, hash);
                                        results.push(hash);
                                        continue;
                                    }
                                    HeapValue::Deferred(d) if !d.delay => {
                                        (Kind::Sequential, self.store.sequential(Value::Heap(id))?)
                                    }
                                    HeapValue::LazyCons { .. }
                                    | HeapValue::List(_)
                                    | HeapValue::Vector(_)
                                    | HeapValue::ArgumentSeq(_) => {
                                        (Kind::Sequential, self.store.sequential(Value::Heap(id))?)
                                    }
                                    HeapValue::Map(data)
                                    | HeapValue::Record(records::Record { data, .. }) => {
                                        self.store
                                            .accounting
                                            .charge(
                                                ResourceLimit::Allocation,
                                                (data.len() as u64).saturating_mul(2),
                                            )
                                            .map_err(CollectionError::Resource)?;
                                        (
                                            match self.store.heap.get(id.0).unwrap() {
                                                HeapValue::Record(r) => Kind::Record(r.type_id),
                                                _ => Kind::Map,
                                            },
                                            data.entries().flat_map(|(k, v)| [k, v]).collect(),
                                        )
                                    }
                                    HeapValue::Set(data) => {
                                        self.store
                                            .accounting
                                            .charge(ResourceLimit::Allocation, data.len() as u64)
                                            .map_err(CollectionError::Resource)?;
                                        (Kind::Set, data.entries().map(|(k, _)| k).collect())
                                    }
                                    HeapValue::Annotated { .. } => {
                                        unreachable!("annotation removed")
                                    }
                                    HeapValue::CompilerLiteral(kind, value)
                                        if matches!(
                                            kind,
                                            super::CompilerLiteralKind::BigInteger
                                                | super::CompilerLiteralKind::Uuid
                                                | super::CompilerLiteralKind::Instant
                                        ) =>
                                    {
                                        tasks.push(HashTask::Visit(*value));
                                        continue;
                                    }
                                    HeapValue::CompilerLiteral(_, _)
                                    | HeapValue::Deferred(_)
                                    | HeapValue::Closure(_)
                                    | HeapValue::HigherOrder(_)
                                    | HeapValue::Reify(_)
                                    | HeapValue::Protocol(_)
                                    | HeapValue::RecordType(_)
                                    | HeapValue::ProtocolMethod(_)
                                    | HeapValue::Reduced(_)
                                    | HeapValue::Atom(_)
                                    | HeapValue::Input(_)
                                    | HeapValue::Output(_)
                                    | HeapValue::Volatile(_)
                                    | HeapValue::Exception(_) => {
                                        use std::hash::{Hash, Hasher};
                                        let mut hasher =
                                            std::collections::hash_map::DefaultHasher::new();
                                        id.hash(&mut hasher);
                                        results.push(hasher.finish());
                                        continue;
                                    }
                                    #[cfg(test)]
                                    HeapValue::ProtocolFixture(_) | HeapValue::Edges(_) => {
                                        use std::hash::{Hash, Hasher};
                                        let mut hasher =
                                            std::collections::hash_map::DefaultHasher::new();
                                        id.hash(&mut hasher);
                                        results.push(hasher.finish());
                                        continue;
                                    }
                                };
                            tasks.push(HashTask::Finish(id, kind, children.len()));
                            tasks.extend(children.into_iter().rev().map(HashTask::Visit));
                            continue;
                        }
                    };
                    results.push(hash);
                }
            }
        }
        Ok(results[0])
    }
}
pub(super) fn hash(store: &Store, value: Value) -> u64 {
    // A single fallback preserves the total helper's hash/equality invariant
    // even when two enclosing values share an identical failing projection.
    try_hash(store, value).unwrap_or_else(|_| mix(17))
}
pub(super) fn try_hash(store: &Store, value: Value) -> Result<u64, CollectionError> {
    ContentHasher::new(store).hash(value)
}

enum EqualTask {
    Compare(Value, Value),
    All(Vec<EqualTask>),
    Any(Vec<EqualTask>),
    Reduce { pending: Vec<EqualTask>, any: bool },
    Remember(HeapId, HeapId),
    Sequence(Value, usize, Value, usize),
    SequenceNext(Value, usize, Value, usize),
}

pub(super) fn equal(left_store: &Store, left: Value, right_store: &Store, right: Value) -> bool {
    try_equal(left_store, left, right_store, right).unwrap_or(false)
}
pub(super) fn try_equal(
    left_store: &Store,
    left: Value,
    right_store: &Store,
    right: Value,
) -> Result<bool, CollectionError> {
    let mut tasks = vec![EqualTask::Compare(left, right)];
    let mut results = Vec::new();
    let mut memo = HashMap::new();
    let mut left_hash = ContentHasher::new(left_store);
    while let Some(task) = tasks.pop() {
        left_store
            .accounting
            .work(1)
            .map_err(CollectionError::Resource)?;
        match task {
            EqualTask::SequenceNext(left, li, right, ri) => {
                if results.pop().unwrap() {
                    tasks.push(EqualTask::Sequence(left, li, right, ri));
                } else {
                    results.push(false);
                }
            }
            EqualTask::Sequence(left, li, right, ri) => {
                // Lazy seq positions must be established even when the other
                // sequence ends. Ordinary projected members remain unforced.
                left_store.force_sequence_position(left, li)?;
                right_store.force_sequence_position(right, ri)?;
                // Stop without forcing a remaining projected element.
                let left_done = left_store.sequence_len(left).is_some_and(|n| li == n)
                    || matches!(left_store.unannotated(left), Value::Nil);
                let right_done = right_store.sequence_len(right).is_some_and(|n| ri == n)
                    || matches!(right_store.unannotated(right), Value::Nil);
                if left_done || right_done {
                    results.push(left_done && right_done);
                    continue;
                }
                let a = left_store.sequence_step(left, li)?;
                let b = right_store.sequence_step(right, ri)?;
                let (Some((a, left, li)), Some((b, right, ri))) = (a, b) else {
                    results.push(a.is_none() && b.is_none());
                    continue;
                };
                tasks.push(EqualTask::SequenceNext(left, li, right, ri));
                tasks.push(EqualTask::Compare(a, b));
            }
            EqualTask::Remember(a, b) => {
                memo.insert((a, b), *results.last().expect("comparison result"));
            }
            EqualTask::All(mut pending) => {
                if let Some(next) = pending.pop() {
                    tasks.push(EqualTask::Reduce {
                        pending,
                        any: false,
                    });
                    tasks.push(next);
                } else {
                    results.push(true);
                }
            }
            EqualTask::Any(mut pending) => {
                if let Some(next) = pending.pop() {
                    tasks.push(EqualTask::Reduce { pending, any: true });
                    tasks.push(next);
                } else {
                    results.push(false);
                }
            }
            EqualTask::Reduce { mut pending, any } => {
                let value = results.pop().unwrap();
                if value == any {
                    results.push(value);
                } else if let Some(next) = pending.pop() {
                    tasks.push(EqualTask::Reduce { pending, any });
                    tasks.push(next);
                } else {
                    results.push(!any);
                }
            }
            EqualTask::Compare(left, right) => {
                let left = left_store.unannotated(left);
                let right = right_store.unannotated(right);
                let scalar = match (left, right) {
                    (Value::Nil, Value::Nil) => true,
                    (Value::Boolean(a), Value::Boolean(b)) => a == b,
                    (Value::Integer(a), Value::Integer(b)) => a == b,
                    (Value::Double(a), Value::Double(b)) => a == b,
                    (Value::Ratio(a, b), Value::Ratio(c, d)) => (a, b) == (c, d),
                    (Value::Character(a), Value::Character(b)) => a == b,
                    (Value::Native(a), Value::Native(b)) => std::ptr::eq(a, b),
                    (Value::Namespace(a), Value::Namespace(b)) => a == b,
                    (Value::Var(a), Value::Var(b)) => a == b,
                    (Value::Unbound(_), Value::Unbound(_)) => true,
                    (Value::Symbol(a), Value::Symbol(b))
                    | (Value::Keyword(a), Value::Keyword(b)) => {
                        left_store.names.get(a.0).unwrap() == right_store.names.get(b.0).unwrap()
                    }
                    (Value::Heap(a), Value::Heap(b)) => {
                        // Immutable heap object identity is reflexive, including
                        // a collection containing NaN. Scalar NaN remains IEEE.
                        if a == b {
                            results.push(true);
                            continue;
                        }
                        if let Some(value) = memo.get(&(a, b)) {
                            results.push(*value);
                            continue;
                        }
                        let x = left_store.heap.get(a.0).unwrap();
                        let y = right_store.heap.get(b.0).unwrap();
                        match (x, y) {
                            (
                                HeapValue::CompilerLiteral(ka, a),
                                HeapValue::CompilerLiteral(kb, b),
                            ) if ka == kb
                                && matches!(
                                    ka,
                                    super::CompilerLiteralKind::BigInteger
                                        | super::CompilerLiteralKind::Uuid
                                        | super::CompilerLiteralKind::Instant
                                ) =>
                            {
                                tasks.push(EqualTask::Compare(*a, *b));
                                continue;
                            }
                            (HeapValue::String(x), HeapValue::String(y)) => {
                                left_store
                                    .accounting
                                    .work(x.len().min(y.len()) as u64)
                                    .map_err(CollectionError::Resource)?;
                                x == y
                            }
                            (
                                HeapValue::List(_)
                                | HeapValue::Deferred(_)
                                | HeapValue::LazyCons { .. }
                                | HeapValue::Vector(_)
                                | HeapValue::ArgumentSeq(_),
                                HeapValue::List(_)
                                | HeapValue::Deferred(_)
                                | HeapValue::LazyCons { .. }
                                | HeapValue::Vector(_)
                                | HeapValue::ArgumentSeq(_),
                            ) if !matches!(x, HeapValue::Deferred(d) if d.delay)
                                && !matches!(y, HeapValue::Deferred(d) if d.delay) =>
                            {
                                tasks.push(EqualTask::Remember(a, b));
                                tasks.push(EqualTask::Sequence(left, 0, right, 0));
                                continue;
                            }
                            (HeapValue::Map(x), HeapValue::Map(y))
                            | (
                                HeapValue::Record(records::Record { data: x, .. }),
                                HeapValue::Record(records::Record { data: y, .. }),
                            )
                            | (HeapValue::Set(x), HeapValue::Set(y)) => {
                                if let (HeapValue::Record(a), HeapValue::Record(b)) = (
                                    left_store.heap.get(a.0).unwrap(),
                                    right_store.heap.get(b.0).unwrap(),
                                ) && a.type_id != b.type_id
                                {
                                    results.push(false);
                                    continue;
                                }
                                if x.len() != y.len() {
                                    results.push(false);
                                    continue;
                                }
                                let is_map = matches!(
                                    left_store.heap.get(a.0).unwrap(),
                                    HeapValue::Map(_) | HeapValue::Record(_)
                                );
                                tasks.push(EqualTask::Remember(a, b));
                                left_store
                                    .accounting
                                    .charge(ResourceLimit::Allocation, x.len() as u64)
                                    .map_err(CollectionError::Resource)?;
                                let mut entries = Vec::with_capacity(x.len());
                                for (key, value) in x.entries() {
                                    let mut candidates = Vec::new();
                                    for (other_key, other_value) in y.bucket(left_hash.hash(key)?) {
                                        let pair = if is_map {
                                            EqualTask::All(vec![
                                                EqualTask::Compare(value, *other_value),
                                                EqualTask::Compare(key, *other_key),
                                            ])
                                        } else {
                                            EqualTask::Compare(key, *other_key)
                                        };
                                        candidates.push(pair);
                                    }
                                    candidates.reverse();
                                    entries.push(EqualTask::Any(candidates));
                                }
                                entries.reverse();
                                tasks.push(EqualTask::All(entries));
                                continue;
                            }
                            _ => false,
                        }
                    }
                    _ => false,
                };
                results.push(scalar);
            }
        }
    }
    Ok(results[0])
}

fn identity_hash(tag: u64, id: impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hash);
    combine(tag, hash.finish())
}
