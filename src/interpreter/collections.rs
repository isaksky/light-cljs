//! Immutable payloads. Trees share Rc nodes; their leaves contain only raw Values.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionError {
    Resource(ResourceError),
    WrongOwner,
    ExpectedCollection,
    RequiresRealization,
    ExpectedSequential,
    ExpectedMap,
    ExpectedSet,
    ExpectedVectorOrMap,
    ExpectedMapEntry,
    InvalidIndex,
    EmptyCollection,
    UnsupportedRecordEmpty,
    UnsupportedMetadata,
    ExpectedMetadataMap,
}
impl fmt::Display for CollectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Self::Resource(error) = self {
            return error.fmt(f);
        }
        f.write_str(match self {
            Self::Resource(_) => unreachable!(),
            Self::WrongOwner => "value belongs to another interpreter owner",
            Self::ExpectedCollection => "expected a collection",
            Self::RequiresRealization => "deferred sequence requires a mutable Context to realize",
            Self::ExpectedSequential => "expected a list or vector",
            Self::ExpectedMap => "expected a map",
            Self::ExpectedSet => "expected a set",
            Self::ExpectedVectorOrMap => "expected a vector or map",
            Self::ExpectedMapEntry => "map conj requires a two-element vector",
            Self::InvalidIndex => "vector index must be an integer from zero through its count",
            Self::EmptyCollection => "cannot pop an empty collection",
            Self::UnsupportedRecordEmpty => "empty is unsupported for records",
            Self::UnsupportedMetadata => "value does not support metadata",
            Self::ExpectedMetadataMap => "metadata must be a map or nil",
        })
    }
}
impl std::error::Error for CollectionError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumberError {
    ZeroDenominator,
    Overflow,
}
impl fmt::Display for NumberError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ZeroDenominator => "ratio denominator is zero",
            Self::Overflow => "reduced ratio does not fit signed 64-bit components",
        })
    }
}
impl std::error::Error for NumberError {}

pub(super) enum ListData {
    Empty,
    Cons {
        head: Value,
        tail: Value,
        len: usize,
        proper_list: bool,
    },
}
impl ListData {
    pub(super) fn is_list(&self) -> bool {
        matches!(
            self,
            Self::Empty
                | Self::Cons {
                    proper_list: true,
                    ..
                }
        )
    }
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Cons { len, .. } => *len,
        }
    }
}

// Finite seq views retain their language Counted distinction. Members are
// evaluated and traced; optional entry projection is deferred until access.
#[derive(Clone)]
pub(super) struct EagerSeq {
    values: Rc<[Value]>,
    start: usize,
    pub(super) counted: bool,
    projection: Option<(Value, usize, usize)>,
    concat: Option<Rc<ConcatSeq>>,
    // A finite lazy seq view: an invalid source has a sentinel length of one
    // solely for traversal admission, and fails when its first member is read.
    deferred: Option<(Value, usize)>,
}
struct ConcatSeq {
    left: Value,
    right: Value,
    left_len: usize,
    len: usize,
    // Only successful projection validation is memoized. Values stay in their
    // traced source; no roots or runtime borrows are captured by this cache.
    validated: std::cell::Cell<usize>,
}
impl EagerSeq {
    pub(super) fn len(&self) -> usize {
        self.concat.as_ref().map_or_else(
            || {
                self.deferred.map_or_else(
                    || self.projection.map_or(self.values.len(), |(_, _, len)| len),
                    |(_, len)| len,
                )
            },
            |concat| concat.len,
        ) - self.start
    }
    pub(super) fn sources(&self) -> impl Iterator<Item = Value> {
        self.projection
            .map(|(source, _, _)| source)
            .into_iter()
            .chain(self.deferred.map(|(source, _)| source))
            .chain(
                self.concat
                    .iter()
                    .flat_map(|concat| [concat.left, concat.right]),
            )
    }
}
impl std::ops::Deref for EagerSeq {
    type Target = [Value];
    fn deref(&self) -> &[Value] {
        if self.projection.is_some() || self.concat.is_some() || self.deferred.is_some() {
            &[]
        } else {
            &self.values[self.start..]
        }
    }
}
impl FromIterator<Value> for EagerSeq {
    fn from_iter<T: IntoIterator<Item = Value>>(iter: T) -> Self {
        Self {
            values: iter.into_iter().collect(),
            start: 0,
            counted: true,
            projection: None,
            concat: None,
            deferred: None,
        }
    }
}

// A 32-way persistent vector. Updates copy at most 32 entries at each level;
// nested values are never copied. Rc nodes have bounded logarithmic drop depth.
#[derive(Clone)]
pub(super) struct Sequence {
    root: Rc<SequenceNode>,
    depth: u32,
    len: usize,
    pub(super) map_entry: bool,
}
#[derive(Clone)]
enum SequenceNode {
    Leaf(Vec<Value>),
    Branch(Vec<Rc<SequenceNode>>),
}
impl Sequence {
    pub(super) fn trace(&self, targets: &mut Vec<Target>) {
        fn visit(node: &SequenceNode, targets: &mut Vec<Target>) {
            match node {
                SequenceNode::Leaf(values) => {
                    targets.extend(values.iter().copied().map(Target::Value));
                }
                SequenceNode::Branch(children) => {
                    for child in children {
                        visit(child, targets);
                    }
                }
            }
        }
        // Only the 32-way tree is recursive (bounded by usize index bits),
        // never its language values. Their graph is traced iteratively.
        visit(&self.root, targets);
    }

    fn new() -> Self {
        Self {
            root: Rc::new(SequenceNode::Leaf(Vec::new())),
            depth: 0,
            len: 0,
            map_entry: false,
        }
    }
    pub(super) fn len(&self) -> usize {
        self.len
    }
    fn full(len: usize, depth: u32) -> bool {
        1usize
            .checked_shl((depth + 1) * 5)
            .is_some_and(|capacity| len == capacity)
    }
    fn path(depth: u32, value: Value) -> Rc<SequenceNode> {
        Rc::new(if depth == 0 {
            SequenceNode::Leaf(vec![value])
        } else {
            SequenceNode::Branch(vec![Self::path(depth - 1, value)])
        })
    }
    fn push_node(node: &mut Rc<SequenceNode>, depth: u32, index: usize, value: Value) {
        match Rc::make_mut(node) {
            SequenceNode::Leaf(values) => values.push(value),
            SequenceNode::Branch(children) => {
                let child = (index >> (depth * 5)) & 31;
                if child == children.len() {
                    children.push(Self::path(depth - 1, value));
                } else {
                    Self::push_node(&mut children[child], depth - 1, index, value);
                }
            }
        }
    }
    fn push(&mut self, value: Value) {
        self.map_entry = false;
        if Self::full(self.len, self.depth) {
            self.root = Rc::new(SequenceNode::Branch(vec![
                self.root.clone(),
                Self::path(self.depth, value),
            ]));
            self.depth += 1;
        } else {
            Self::push_node(&mut self.root, self.depth, self.len, value);
        }
        self.len += 1;
    }
    fn get(&self, index: usize) -> Option<Value> {
        if index >= self.len {
            return None;
        }
        let mut node = self.root.as_ref();
        let mut depth = self.depth;
        loop {
            match node {
                SequenceNode::Leaf(values) => return values.get(index & 31).copied(),
                SequenceNode::Branch(children) => {
                    node = &children[(index >> (depth * 5)) & 31];
                    depth -= 1;
                }
            }
        }
    }
    fn assoc_node(node: &mut Rc<SequenceNode>, depth: u32, index: usize, value: Value) {
        match Rc::make_mut(node) {
            SequenceNode::Leaf(values) => values[index & 31] = value,
            SequenceNode::Branch(children) => Self::assoc_node(
                &mut children[(index >> (depth * 5)) & 31],
                depth - 1,
                index,
                value,
            ),
        }
    }
    fn assoc(&mut self, index: usize, value: Value) {
        self.map_entry = false;
        if index == self.len {
            self.push(value);
        } else {
            Self::assoc_node(&mut self.root, self.depth, index, value);
        }
    }
    fn pop_node(node: &mut Rc<SequenceNode>) -> bool {
        match Rc::make_mut(node) {
            SequenceNode::Leaf(values) => {
                values.pop();
                values.is_empty()
            }
            SequenceNode::Branch(children) => {
                if Self::pop_node(children.last_mut().expect("nonempty branch")) {
                    children.pop();
                }
                children.is_empty()
            }
        }
    }
    fn pop(&mut self) {
        self.map_entry = false;
        Self::pop_node(&mut self.root);
        self.len -= 1;
        if let SequenceNode::Branch(children) = self.root.as_ref()
            && children.len() == 1
        {
            self.root = children[0].clone();
            self.depth -= 1;
        }
    }
    pub(super) fn iter(&self) -> impl Iterator<Item = Value> + '_ {
        let mut stack = vec![self.root.as_ref()];
        let mut leaf = [].iter();
        std::iter::from_fn(move || {
            loop {
                if let Some(value) = leaf.next() {
                    return Some(*value);
                }
                match stack.pop()? {
                    SequenceNode::Leaf(values) => leaf = values.iter(),
                    SequenceNode::Branch(children) => {
                        stack.extend(children.iter().rev().map(Rc::as_ref))
                    }
                }
            }
        })
    }
}

// A persistent radix trie over content hashes. Full-hash collisions remain in
// leaf buckets and are always resolved by value equality, never by hash alone.
#[derive(Clone, Default)]
pub(super) struct MapData {
    root: Option<Rc<MapNode>>,
    len: usize,
}
#[derive(Clone)]
enum MapNode {
    Leaf {
        hash: u64,
        entries: Vec<(Value, Value)>,
    },
    Branch([Option<Rc<MapNode>>; 16]),
}
impl MapData {
    pub(super) fn trace(&self, targets: &mut Vec<Target>) {
        fn visit(node: &MapNode, targets: &mut Vec<Target>) {
            match node {
                MapNode::Leaf { entries, .. } => {
                    targets.extend(
                        entries
                            .iter()
                            .flat_map(|(key, value)| [Target::Value(*key), Target::Value(*value)]),
                    );
                }
                MapNode::Branch(children) => {
                    for child in children.iter().flatten() {
                        visit(child, targets);
                    }
                }
            }
        }
        // A 64-bit hash permits at most 16 branch levels. Reuse the owner's
        // target buffer instead of allocating an iterator stack for every map.
        if let Some(root) = &self.root {
            visit(root, targets);
        }
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }
    pub(super) fn bucket(&self, hash: u64) -> &[(Value, Value)] {
        let mut node = self.root.as_deref();
        let mut shift = 0;
        while let Some(current) = node {
            match current {
                MapNode::Leaf {
                    hash: found,
                    entries,
                } => return if *found == hash { entries } else { &[] },
                MapNode::Branch(children) => {
                    node = children[((hash >> shift) & 15) as usize].as_deref();
                    shift += 4;
                }
            }
        }
        &[]
    }
    pub(super) fn get(&self, store: &Store, key: Value) -> Option<(Value, Value)> {
        self.bucket(equality::hash(store, key))
            .iter()
            .copied()
            .find(|(candidate, _)| equality::equal(store, *candidate, store, key))
    }
    fn insert_node(
        node: &mut Option<Rc<MapNode>>,
        shift: u32,
        hash: u64,
        key: Value,
        value: Value,
        store: &Store,
    ) -> bool {
        let Some(current) = node else {
            *node = Some(Rc::new(MapNode::Leaf {
                hash,
                entries: vec![(key, value)],
            }));
            return true;
        };
        if let MapNode::Leaf { hash: old_hash, .. } = current.as_ref()
            && *old_hash != hash
        {
            let mut children = std::array::from_fn(|_| None);
            children[((old_hash >> shift) & 15) as usize] = Some(current.clone());
            *current = Rc::new(MapNode::Branch(children));
        }
        match Rc::make_mut(current) {
            MapNode::Leaf { entries, .. } => {
                if let Some(entry) = entries
                    .iter_mut()
                    .find(|(candidate, _)| equality::equal(store, *candidate, store, key))
                {
                    entry.1 = value;
                    false // Preserve the original key representative.
                } else {
                    entries.push((key, value));
                    true
                }
            }
            MapNode::Branch(children) => Self::insert_node(
                &mut children[((hash >> shift) & 15) as usize],
                shift + 4,
                hash,
                key,
                value,
                store,
            ),
        }
    }
    pub(super) fn assoc(&mut self, store: &Store, key: Value, value: Value) {
        if Self::insert_node(
            &mut self.root,
            0,
            equality::hash(store, key),
            key,
            value,
            store,
        ) {
            self.len += 1;
        }
    }
    fn remove_node(
        node: &mut Option<Rc<MapNode>>,
        shift: u32,
        hash: u64,
        key: Value,
        store: &Store,
    ) -> bool {
        let Some(current) = node else {
            return false;
        };
        match Rc::make_mut(current) {
            MapNode::Leaf {
                hash: found,
                entries,
            } => {
                if *found != hash {
                    return false;
                }
                let Some(index) = entries
                    .iter()
                    .position(|(candidate, _)| equality::equal(store, *candidate, store, key))
                else {
                    return false;
                };
                entries.remove(index);
                if entries.is_empty() {
                    *node = None;
                }
                true
            }
            MapNode::Branch(children) => {
                let removed = Self::remove_node(
                    &mut children[((hash >> shift) & 15) as usize],
                    shift + 4,
                    hash,
                    key,
                    store,
                );
                if children.iter().all(Option::is_none) {
                    *node = None;
                }
                removed
            }
        }
    }
    fn remove(&mut self, store: &Store, key: Value) {
        if Self::remove_node(&mut self.root, 0, equality::hash(store, key), key, store) {
            self.len -= 1;
        }
    }
    pub(super) fn entries(&self) -> impl Iterator<Item = (Value, Value)> + '_ {
        let mut stack: Vec<_> = self.root.iter().map(Rc::as_ref).collect();
        let mut leaf = [].iter();
        std::iter::from_fn(move || {
            loop {
                if let Some(entry) = leaf.next() {
                    return Some(*entry);
                }
                match stack.pop()? {
                    MapNode::Leaf { entries, .. } => leaf = entries.iter(),
                    MapNode::Branch(children) => {
                        stack.extend(children.iter().filter_map(Option::as_deref))
                    }
                }
            }
        })
    }
}

impl Store {
    pub(super) fn unannotated(&self, value: Value) -> Value {
        if let Value::Heap(id) = value
            && let HeapValue::Annotated { value, .. } = self.heap.get(id.0).expect("live value")
        {
            return *value;
        }
        value
    }
    fn metadata(&self, value: Value) -> Option<Value> {
        if let Value::Var(id) = value {
            let meta = self.vars.get(id.0).expect("live Var").metadata;
            return (!matches!(meta, Value::Nil)).then_some(meta);
        }
        if let Some(HeapValue::Atom(atom)) = self.heap_value(value) {
            return (!matches!(atom.metadata, Value::Nil)).then_some(atom.metadata);
        }
        if let Value::Heap(id) = value
            && let HeapValue::Annotated { metadata, .. } = self.heap.get(id.0).expect("live value")
        {
            return Some(*metadata);
        }
        None
    }
    pub(super) fn heap_value(&self, value: Value) -> Option<&HeapValue> {
        if let Value::Heap(id) = self.unannotated(value) {
            Some(self.heap.get(id.0).expect("live value"))
        } else {
            None
        }
    }
    pub(super) fn map_data(&self, value: Value) -> Option<&MapData> {
        match self.heap_value(value)? {
            HeapValue::Map(data) => Some(data),
            HeapValue::Record(r) => Some(&r.data),
            _ => None,
        }
    }
    fn allocate(&mut self, value: HeapValue) -> Value {
        Value::Heap(HeapId(self.heap.insert(value)))
    }
    fn annotate(&mut self, value: Value, metadata: Option<Value>) -> Value {
        match metadata {
            Some(metadata) => self.allocate(HeapValue::Annotated { value, metadata }),
            None => value,
        }
    }
    pub(super) fn sequence_len(&self, value: Value) -> Option<usize> {
        match self.heap_value(value)? {
            HeapValue::Deferred(d) if !d.delay => d.realized_value().and_then(|v| {
                if matches!(v, Value::Nil) {
                    Some(0)
                } else {
                    self.sequence_len(v)
                }
            }),
            HeapValue::Vector(v) => Some(v.len()),
            HeapValue::List(v) => Some(v.len()),
            HeapValue::ArgumentSeq(v) => Some(v.len()),
            _ => None,
        }
    }
    pub(super) fn sequence_nth(
        &self,
        mut value: Value,
        mut index: usize,
    ) -> Result<Option<Value>, CollectionError> {
        enum Step {
            Read(Value, usize),
            Project(usize),
            Validated(Rc<ConcatSeq>, usize),
        }
        let mut steps = Vec::new();
        let mut first = Some(Step::Read(value, index));
        let mut result = None;
        while let Some(step) = first.take().or_else(|| steps.pop()) {
            self.accounting.work(1).map_err(CollectionError::Resource)?;
            match step {
                Step::Project(projection) => {
                    result = match result.and_then(|v| self.heap_value(v)) {
                        Some(HeapValue::Vector(entry)) if entry.map_entry => entry.get(projection),
                        _ => return Err(CollectionError::ExpectedMapEntry),
                    };
                }
                Step::Validated(concat, prefix) => {
                    concat.validated.set(concat.validated.get().max(prefix));
                }
                Step::Read(source, position) => {
                    value = source;
                    index = position;
                    loop {
                        self.accounting.work(1).map_err(CollectionError::Resource)?;
                        match self.heap_value(value) {
                            Some(HeapValue::Vector(v)) => {
                                result = v.get(index);
                                break;
                            }
                            None if matches!(self.unannotated(value), Value::Nil) => {
                                result = None;
                                break;
                            }
                            Some(HeapValue::List(ListData::Empty)) => {
                                result = None;
                                break;
                            }
                            Some(HeapValue::List(ListData::Cons { head, tail, .. }))
                            | Some(HeapValue::LazyCons { head, tail }) => {
                                if index == 0 {
                                    result = Some(*head);
                                    break;
                                }
                                value = *tail;
                                index -= 1;
                            }
                            Some(HeapValue::Deferred(d)) => {
                                value = d
                                    .realized_value()
                                    .ok_or(CollectionError::RequiresRealization)?;
                            }
                            Some(HeapValue::ArgumentSeq(v)) => {
                                if let Some((source, _)) = v.deferred {
                                    index += v.start;
                                    value = source;
                                } else if let Some((source, projection, len)) = v.projection {
                                    if index >= len - v.start {
                                        result = None;
                                        break;
                                    }
                                    steps.push(Step::Project(projection));
                                    index += v.start;
                                    value = source;
                                } else if let Some(concat) = &v.concat {
                                    index += v.start;
                                    let target = if index < concat.left_len {
                                        (concat.left, index)
                                    } else {
                                        (concat.right, index - concat.left_len)
                                    };
                                    steps.push(Step::Read(target.0, target.1));
                                    let prefix = index.saturating_add(1).min(concat.left_len);
                                    self.accounting
                                        .charge(
                                            ResourceLimit::Allocation,
                                            (prefix.saturating_sub(concat.validated.get()) as u64)
                                                .saturating_mul(2),
                                        )
                                        .map_err(CollectionError::Resource)?;
                                    for offset in (concat.validated.get()..prefix).rev() {
                                        steps.push(Step::Validated(concat.clone(), offset + 1));
                                        steps.push(Step::Read(concat.left, offset));
                                    }
                                    break;
                                } else {
                                    result = v.get(index).copied();
                                    break;
                                }
                            }
                            _ => return Err(CollectionError::ExpectedSequential),
                        }
                    }
                }
            }
        }
        Ok(result)
    }
    pub(super) fn force_sequence_position(
        &self,
        value: Value,
        index: usize,
    ) -> Result<(), CollectionError> {
        if let Some(HeapValue::Deferred(d)) = self.heap_value(value) {
            d.realized_value()
                .ok_or(CollectionError::RequiresRealization)?;
        }
        if matches!(self.heap_value(value), Some(HeapValue::ArgumentSeq(v)) if v.concat.is_some() || v.deferred.is_some())
        {
            self.sequence_nth(value, index)?;
        }
        Ok(())
    }
    pub(super) fn force_concat_head(&self, value: Value) -> Result<(), CollectionError> {
        if matches!(self.heap_value(value), Some(HeapValue::ArgumentSeq(v)) if v.concat.is_some() || v.deferred.is_some())
        {
            self.sequence_nth(value, 0)?;
        }
        Ok(())
    }
    pub(super) fn check_sequence_count(&self, mut value: Value) -> Result<(), CollectionError> {
        loop {
            self.accounting.work(1).map_err(CollectionError::Resource)?;
            match self.heap_value(value) {
                Some(HeapValue::List(ListData::Cons {
                    proper_list: true, ..
                })) => break,
                Some(HeapValue::List(ListData::Cons { tail, .. })) => value = *tail,
                Some(HeapValue::ArgumentSeq(v)) if v.deferred.is_some() => {
                    if let Some(last) = v.len().checked_sub(1) {
                        self.sequence_nth(value, last)?;
                    }
                    break;
                }
                Some(HeapValue::ArgumentSeq(v)) if v.concat.is_some() => {
                    let concat = v.concat.as_ref().unwrap();
                    // count walks the lazy left side; the right side keeps
                    // its own count/realization convention (keys may count
                    // without projecting their entries).
                    if concat.left_len > 0 {
                        self.sequence_nth(value, concat.left_len.saturating_sub(v.start + 1))?;
                    }
                    value = concat.right;
                }
                _ => break,
            }
        }
        Ok(())
    }

    pub(super) fn sequence_step(
        &self,
        value: Value,
        index: usize,
    ) -> Result<Option<(Value, Value, usize)>, CollectionError> {
        match self.heap_value(value) {
            Some(HeapValue::Deferred(d)) if !d.delay => self.sequence_step(
                d.realized_value()
                    .ok_or(CollectionError::RequiresRealization)?,
                index,
            ),
            Some(HeapValue::List(ListData::Empty)) => Ok(None),
            Some(
                HeapValue::List(ListData::Cons { head, tail, .. })
                | HeapValue::LazyCons { head, tail },
            ) if index == 0 => Ok(Some((*head, *tail, 0))),
            _ => Ok(self
                .sequence_nth(value, index)?
                .map(|head| (head, value, index + 1))),
        }
    }
    pub(super) fn sequential(&self, value: Value) -> Result<Vec<Value>, CollectionError> {
        let length = self.sequence_len(value).unwrap_or(0) as u64;
        self.accounting
            .charge(ResourceLimit::Allocation, length)
            .map_err(CollectionError::Resource)?;
        self.accounting
            .work(length)
            .map_err(CollectionError::Resource)?;
        match self.heap_value(value) {
            Some(HeapValue::Vector(v)) => Ok(v.iter().collect()),
            Some(HeapValue::ArgumentSeq(v))
                if v.projection.is_none() && v.concat.is_none() && v.deferred.is_none() =>
            {
                Ok(v.to_vec())
            }
            Some(HeapValue::ArgumentSeq(v)) => (0..v.len())
                .map(|i| self.sequence_nth(value, i).map(Option::unwrap))
                .collect(),
            Some(HeapValue::Deferred(_) | HeapValue::LazyCons { .. }) => {
                let mut values = Vec::new();
                let mut current = value;
                let mut index = 0;
                loop {
                    while let Some(HeapValue::Deferred(d)) = self.heap_value(current) {
                        current = d
                            .realized_value()
                            .ok_or(CollectionError::RequiresRealization)?;
                    }
                    if matches!(current, Value::Nil) {
                        break;
                    }
                    let Some((head, tail, next)) = self.sequence_step(current, index)? else {
                        break;
                    };
                    values.push(head);
                    current = tail;
                    index = next;
                }
                Ok(values)
            }
            Some(HeapValue::List(_)) => {
                let mut result = Vec::new();
                let mut current = value;
                loop {
                    self.accounting.work(1).map_err(CollectionError::Resource)?;
                    match self.heap_value(current).expect("list tail") {
                        HeapValue::List(ListData::Empty) => break,
                        HeapValue::List(ListData::Cons { head, tail, .. }) => {
                            result.push(*head);
                            current = *tail;
                        }
                        HeapValue::ArgumentSeq(_) => {
                            result.extend(self.sequential(current)?);
                            break;
                        }
                        _ => unreachable!("list tail"),
                    }
                }
                Ok(result)
            }
            _ => Err(CollectionError::ExpectedSequential),
        }
    }
}

impl RootedValue {
    /// O(1) count of collection members; scalar values report None.
    pub fn count(&self) -> Option<usize> {
        let store = self.owner.store.borrow();
        match store.heap_value(self.value())? {
            HeapValue::List(v) => Some(v.len()),
            HeapValue::ArgumentSeq(v) => Some(v.len()),
            HeapValue::Vector(v) => Some(v.len()),
            HeapValue::Map(v) | HeapValue::Set(v) => Some(v.len()),
            HeapValue::Record(r) => Some(r.data.len()),
            _ => None,
        }
    }
    /// Root a list/vector/argument sequence's elements in order or a set's members in
    /// unspecified order. This snapshot roots members, not the parent collection.
    pub fn elements(&self) -> Result<Vec<RootedValue>, CollectionError> {
        let store = self.owner.store.borrow();
        let values = match store.heap_value(self.value()) {
            Some(HeapValue::Set(values)) => {
                store
                    .accounting
                    .charge(ResourceLimit::Allocation, values.len() as u64)
                    .map_err(CollectionError::Resource)?;
                store
                    .accounting
                    .work(values.len() as u64)
                    .map_err(CollectionError::Resource)?;
                values.entries().map(|(key, _)| key).collect()
            }
            _ => store.sequential(self.value())?,
        };
        values
            .into_iter()
            .map(|value| self.owner.checked_value(value))
            .collect()
    }
    /// Map/record entries are unordered; callers must not infer evaluation order here.
    pub fn entries(&self) -> Result<Vec<(RootedValue, RootedValue)>, CollectionError> {
        let store = self.owner.store.borrow();
        let Some(values) = store.map_data(self.value()) else {
            return Err(CollectionError::ExpectedMap);
        };
        store
            .accounting
            .charge(
                ResourceLimit::Allocation,
                (values.len() as u64).saturating_mul(2),
            )
            .map_err(CollectionError::Resource)?;
        store
            .accounting
            .work(values.len() as u64)
            .map_err(CollectionError::Resource)?;
        values
            .entries()
            .map(|(key, value)| {
                Ok((
                    self.owner.checked_value(key)?,
                    self.owner.checked_value(value)?,
                ))
            })
            .collect()
    }
    pub fn nth(&self, index: usize) -> Result<Option<RootedValue>, CollectionError> {
        let store = self.owner.store.borrow();
        store
            .sequence_nth(self.value(), index)?
            .map(|value| self.owner.checked_value(value))
            .transpose()
    }
    /// None means absent; Some(nil) and Some(false) remain distinguishable.
    /// Set lookup returns the retained member, including its original metadata.
    pub fn get(&self, key: &RootedValue) -> Result<Option<RootedValue>, CollectionError> {
        if !Rc::ptr_eq(&self.owner, &key.owner) {
            return Err(CollectionError::WrongOwner);
        }
        let store = self.owner.store.borrow();
        if matches!(
            store.heap_value(self.value()),
            Some(HeapValue::Map(_) | HeapValue::Set(_) | HeapValue::Record(_))
        ) {
            equality::try_hash(&store, key.value())?;
        }
        let found = match store.heap_value(self.value()) {
            Some(
                HeapValue::Map(values) | HeapValue::Record(records::Record { data: values, .. }),
            ) => values.get(&store, key.value()).map(|(_, value)| value),
            Some(HeapValue::Set(values)) => values.get(&store, key.value()).map(|(key, _)| key),
            Some(HeapValue::Vector(values)) => match store.unannotated(key.value()) {
                Value::Integer(index) => usize::try_from(index)
                    .ok()
                    .and_then(|index| values.get(index)),
                _ => None,
            },
            _ => return Err(CollectionError::ExpectedCollection),
        };
        store
            .accounting
            .check()
            .map_err(CollectionError::Resource)?;
        found
            .map(|value| self.owner.checked_value(value))
            .transpose()
    }
    pub fn contains(&self, key: &RootedValue) -> Result<bool, CollectionError> {
        Ok(self.get(key)?.is_some())
    }
    pub fn metadata(&self) -> Option<RootedValue> {
        self.owner
            .store
            .borrow()
            .metadata(self.value())
            .map(|value| self.owner.value(value))
    }
}

impl Context {
    fn reserve_collection(&self, units: usize) -> Result<(), CollectionError> {
        self.charge_allocation(units as u64)
            .and_then(|()| self.accounting.work(units as u64))
            .map_err(CollectionError::Resource)
    }
    fn check_values(&self, values: &[&RootedValue]) -> Result<(), CollectionError> {
        if values
            .iter()
            .any(|value| !Rc::ptr_eq(&self.owner, &value.owner))
        {
            Err(CollectionError::WrongOwner)
        } else {
            Ok(())
        }
    }
    pub fn ratio(&self, numerator: i64, denominator: i64) -> Result<RootedValue, NumberError> {
        if denominator == 0 {
            return Err(NumberError::ZeroDenominator);
        }
        let mut numerator = i128::from(numerator);
        let mut denominator = i128::from(denominator);
        if denominator < 0 {
            numerator = -numerator;
            denominator = -denominator;
        }
        let (mut a, mut b) = (numerator.abs(), denominator);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        numerator /= a;
        denominator /= a;
        let numerator = i64::try_from(numerator).map_err(|_| NumberError::Overflow)?;
        let denominator = i64::try_from(denominator).map_err(|_| NumberError::Overflow)?;
        Ok(self.owner.value(if denominator == 1 {
            Value::Integer(numerator)
        } else {
            Value::Ratio(numerator, denominator)
        }))
    }
    pub fn vector(&self, values: &[RootedValue]) -> Result<RootedValue, CollectionError> {
        self.reserve_collection(values.len().saturating_add(1))?;
        self.check_values(&values.iter().collect::<Vec<_>>())?;
        let mut data = Sequence::new();
        for value in values {
            self.accounting.check().map_err(CollectionError::Resource)?;
            data.push(value.value());
        }
        let value = self
            .owner
            .store
            .borrow_mut()
            .allocate(HeapValue::Vector(data));
        self.accounting.check().map_err(CollectionError::Resource)?;
        self.owner.checked_value(value)
    }
    /// Input is logical sequence order; conj later adds at the front.
    pub fn list(&self, values: &[RootedValue]) -> Result<RootedValue, CollectionError> {
        self.reserve_collection(values.len().saturating_add(1))?;
        self.check_values(&values.iter().collect::<Vec<_>>())?;
        let mut store = self.owner.store.borrow_mut();
        let mut tail = store.allocate(HeapValue::List(ListData::Empty));
        for (index, value) in values.iter().rev().enumerate() {
            self.accounting.check().map_err(CollectionError::Resource)?;
            tail = store.allocate(HeapValue::List(ListData::Cons {
                head: value.value(),
                tail,
                len: index + 1,
                proper_list: true,
            }));
        }
        self.accounting.check().map_err(CollectionError::Resource)?;
        self.owner.checked_value(tail)
    }
    /// Consume already evaluated pairs in supplied order. Later equal keys win,
    /// retaining the first key object. Evaluation must occur before this call.
    pub fn map(
        &self,
        entries: &[(RootedValue, RootedValue)],
    ) -> Result<RootedValue, CollectionError> {
        self.reserve_collection(entries.len().saturating_mul(2).saturating_add(1))?;
        self.check_values(
            &entries
                .iter()
                .flat_map(|(key, value)| [key, value])
                .collect::<Vec<_>>(),
        )?;
        let mut store = self.owner.store.borrow_mut();
        let mut data = MapData::default();
        for (key, value) in entries {
            self.accounting.check().map_err(CollectionError::Resource)?;
            equality::try_hash(&store, key.value())?;
            data.assoc(&store, key.value(), value.value());
        }
        let value = store.allocate(HeapValue::Map(data));
        self.accounting.check().map_err(CollectionError::Resource)?;
        self.owner.checked_value(value)
    }
    pub fn set(&self, values: &[RootedValue]) -> Result<RootedValue, CollectionError> {
        self.reserve_collection(values.len().saturating_add(1))?;
        self.check_values(&values.iter().collect::<Vec<_>>())?;
        let mut store = self.owner.store.borrow_mut();
        let mut data = MapData::default();
        for value in values {
            self.accounting.check().map_err(CollectionError::Resource)?;
            equality::try_hash(&store, value.value())?;
            data.assoc(&store, value.value(), Value::Nil);
        }
        let value = store.allocate(HeapValue::Set(data));
        self.accounting.check().map_err(CollectionError::Resource)?;
        self.owner.checked_value(value)
    }
    pub fn with_meta(
        &self,
        value: &RootedValue,
        metadata: &RootedValue,
    ) -> Result<RootedValue, CollectionError> {
        self.reserve_collection(1)?;
        self.check_values(&[value, metadata])?;
        let mut store = self.owner.store.borrow_mut();
        let base = store.unannotated(value.value());
        if matches!(store.heap_value(base), Some(HeapValue::Deferred(d)) if d.delay) {
            return Err(CollectionError::UnsupportedMetadata);
        }
        if matches!(store.heap_value(base), Some(HeapValue::Vector(v)) if v.map_entry) {
            return Err(CollectionError::UnsupportedMetadata);
        }
        if !matches!(base, Value::Symbol(_))
            && !matches!(
                store.heap_value(base),
                Some(
                    HeapValue::List(_)
                        | HeapValue::Deferred(_)
                        | HeapValue::LazyCons { .. }
                        | HeapValue::ArgumentSeq(_)
                        | HeapValue::Vector(_)
                        | HeapValue::Map(_)
                        | HeapValue::Set(_)
                        | HeapValue::Reify(_)
                        | HeapValue::Record(_)
                )
            )
        {
            return Err(CollectionError::UnsupportedMetadata);
        }
        let metadata = match store.unannotated(metadata.value()) {
            Value::Nil => None,
            _ if matches!(store.heap_value(metadata.value()), Some(HeapValue::Map(_))) => {
                Some(metadata.value())
            }
            _ => return Err(CollectionError::ExpectedMetadataMap),
        };
        // Reify metadata replacement creates a new identity while retaining raw
        // method/capture edges, including when the new metadata is nil.
        let base = if let Some(HeapValue::Reify(table)) = store.heap_value(base) {
            self.reserve_collection(table.iter().map(|i| i.methods.len() + 1).sum())?;
            let table = table.clone();
            store.allocate(HeapValue::Reify(table))
        } else {
            base
        };
        let value = store.annotate(base, metadata);
        self.accounting.check().map_err(CollectionError::Resource)?;
        self.owner.checked_value(value)
    }
    pub fn assoc(
        &self,
        collection: &RootedValue,
        key: &RootedValue,
        value: &RootedValue,
    ) -> Result<RootedValue, CollectionError> {
        self.reserve_collection(3)?;
        self.check_values(&[collection, key, value])?;
        let mut store = self.owner.store.borrow_mut();
        let updated = match store.heap_value(collection.value()) {
            Some(HeapValue::Map(data)) => {
                let mut data = data.clone();
                equality::try_hash(&store, key.value())?;
                data.assoc(&store, key.value(), value.value());
                HeapValue::Map(data)
            }
            Some(HeapValue::Record(record)) => {
                let mut record = record.clone();
                equality::try_hash(&store, key.value())?;
                record.data.assoc(&store, key.value(), value.value());
                HeapValue::Record(record)
            }
            Some(HeapValue::Vector(data)) => {
                let Value::Integer(index) = store.unannotated(key.value()) else {
                    return Err(CollectionError::InvalidIndex);
                };
                let index = usize::try_from(index).map_err(|_| CollectionError::InvalidIndex)?;
                if index > data.len() {
                    return Err(CollectionError::InvalidIndex);
                }
                let mut data = data.clone();
                data.assoc(index, value.value());
                HeapValue::Vector(data)
            }
            _ => return Err(CollectionError::ExpectedVectorOrMap),
        };
        let metadata = store.metadata(collection.value());
        let value = store.allocate(updated);
        let value = store.annotate(value, metadata);
        self.accounting.check().map_err(CollectionError::Resource)?;
        self.owner.checked_value(value)
    }
    pub fn conj(
        &self,
        collection: &RootedValue,
        value: &RootedValue,
    ) -> Result<RootedValue, CollectionError> {
        self.reserve_collection(2)?;
        self.check_values(&[collection, value])?;
        let mut store = self.owner.store.borrow_mut();
        let updated = match store.heap_value(collection.value()) {
            Some(HeapValue::List(data)) => HeapValue::List(ListData::Cons {
                head: value.value(),
                tail: collection.value(),
                len: data.len() + 1,
                proper_list: data.is_list(),
            }),
            Some(HeapValue::Vector(data)) => {
                let mut data = data.clone();
                data.push(value.value());
                HeapValue::Vector(data)
            }
            Some(HeapValue::Set(data)) => {
                let mut data = data.clone();
                equality::try_hash(&store, value.value())?;
                data.assoc(&store, value.value(), Value::Nil);
                HeapValue::Set(data)
            }
            Some(HeapValue::Map(data) | HeapValue::Record(records::Record { data, .. })) => {
                let Some(HeapValue::Vector(entry)) = store.heap_value(value.value()) else {
                    return Err(CollectionError::ExpectedMapEntry);
                };
                if entry.len() != 2 {
                    return Err(CollectionError::ExpectedMapEntry);
                }
                let mut data = data.clone();
                equality::try_hash(&store, entry.get(0).unwrap())?;
                data.assoc(&store, entry.get(0).unwrap(), entry.get(1).unwrap());
                match store.heap_value(collection.value()) {
                    Some(HeapValue::Record(r)) => HeapValue::Record(records::Record {
                        type_id: r.type_id,
                        data,
                    }),
                    _ => HeapValue::Map(data),
                }
            }
            _ => return Err(CollectionError::ExpectedCollection),
        };
        let metadata = store.metadata(collection.value());
        let value = store.allocate(updated);
        let value = store.annotate(value, metadata);
        self.accounting.check().map_err(CollectionError::Resource)?;
        self.owner.checked_value(value)
    }
    fn remove_key(
        &self,
        collection: &RootedValue,
        key: &RootedValue,
        set: bool,
    ) -> Result<RootedValue, CollectionError> {
        self.reserve_collection(1)?;
        self.check_values(&[collection, key])?;
        let mut store = self.owner.store.borrow_mut();
        let data = match (store.heap_value(collection.value()), set) {
            (
                Some(HeapValue::Map(data) | HeapValue::Record(records::Record { data, .. })),
                false,
            )
            | (Some(HeapValue::Set(data)), true) => data,
            _ => {
                return Err(if set {
                    CollectionError::ExpectedSet
                } else {
                    CollectionError::ExpectedMap
                });
            }
        };
        equality::try_hash(&store, key.value())?;
        if data.get(&store, key.value()).is_none() {
            return Ok(collection.clone());
        }
        let mut data = data.clone();
        data.remove(&store, key.value());
        let metadata = store.metadata(collection.value());
        let updated = if set {
            HeapValue::Set(data)
        } else if let Some(HeapValue::Record(r)) = store.heap_value(collection.value()) {
            let keys = &records::descriptor(&store, r.type_id).keys;
            self.accounting
                .work(keys.len() as u64)
                .map_err(CollectionError::Resource)?;
            if keys
                .iter()
                .any(|basis| equality::equal(&store, *basis, &store, key.value()))
            {
                HeapValue::Map(data)
            } else {
                HeapValue::Record(records::Record {
                    type_id: r.type_id,
                    data,
                })
            }
        } else {
            HeapValue::Map(data)
        };
        let value = store.allocate(updated);
        let value = store.annotate(value, metadata);
        self.accounting.check().map_err(CollectionError::Resource)?;
        self.owner.checked_value(value)
    }
    pub fn dissoc(
        &self,
        collection: &RootedValue,
        key: &RootedValue,
    ) -> Result<RootedValue, CollectionError> {
        self.remove_key(collection, key, false)
    }
    pub fn disj(
        &self,
        collection: &RootedValue,
        key: &RootedValue,
    ) -> Result<RootedValue, CollectionError> {
        self.remove_key(collection, key, true)
    }

    pub(super) fn is_map_entry(&self, value: &RootedValue) -> bool {
        matches!(self.owner.store.borrow().heap_value(value.value()), Some(HeapValue::Vector(v)) if v.map_entry)
    }
    pub(super) fn retained_key(
        &self,
        map: &RootedValue,
        key: &RootedValue,
    ) -> Result<Option<RootedValue>, CollectionError> {
        let store = self.owner.store.borrow();
        let Some(data) = store.map_data(map.value()) else {
            return Ok(None);
        };
        let found = data.get(&store, key.value()).map(|(key, _)| key);
        self.accounting.check().map_err(CollectionError::Resource)?;
        found.map(|key| self.owner.checked_value(key)).transpose()
    }
    pub(super) fn map_entry(
        &self,
        key: &RootedValue,
        value: &RootedValue,
    ) -> Result<RootedValue, CollectionError> {
        self.reserve_collection(3)?;
        self.check_values(&[key, value])?;
        let mut sequence = Sequence::new();
        sequence.push(key.value());
        sequence.push(value.value());
        sequence.map_entry = true;
        let raw = self
            .owner
            .store
            .borrow_mut()
            .allocate(HeapValue::Vector(sequence));
        self.accounting.check().map_err(CollectionError::Resource)?;
        self.owner.checked_value(raw)
    }
    pub(super) fn eager_sequence(
        &self,
        values: &[RootedValue],
        counted: bool,
    ) -> Result<RootedValue, CollectionError> {
        if values.is_empty() {
            self.reserve_collection(1)?;
            return Ok(self.nil());
        }
        self.eager_sequence_value(values, counted)
    }
    /// A walk of an empty seq is still a seq, whereas `seq` returns nil.
    pub(super) fn eager_sequence_value(
        &self,
        values: &[RootedValue],
        counted: bool,
    ) -> Result<RootedValue, CollectionError> {
        self.reserve_collection(values.len().saturating_add(1))?;
        let raw = self
            .owner
            .store
            .borrow_mut()
            .allocate(HeapValue::ArgumentSeq(EagerSeq {
                values: values.iter().map(RootedValue::value).collect(),
                start: 0,
                counted,
                projection: None,
                concat: None,
                deferred: None,
            }));
        self.accounting.check().map_err(CollectionError::Resource)?;
        self.owner.checked_value(raw)
    }
    pub(super) fn projected_sequence(
        &self,
        source: &RootedValue,
        index: usize,
    ) -> Result<RootedValue, CollectionError> {
        self.reserve_collection(1)?;
        let len = source.count().unwrap_or(0);
        if len == 0 {
            return Ok(self.nil());
        }
        let raw = self
            .owner
            .store
            .borrow_mut()
            .allocate(HeapValue::ArgumentSeq(EagerSeq {
                values: Rc::from([]),
                start: 0,
                counted: false,
                projection: Some((source.value(), index, len)),
                concat: None,
                deferred: None,
            }));
        self.accounting.check().map_err(CollectionError::Resource)?;
        self.owner.checked_value(raw)
    }
    pub(super) fn deferred_source(&self, source: &RootedValue) -> Option<RootedValue> {
        let store = self.owner.store.borrow();
        let HeapValue::ArgumentSeq(seq) = store.heap_value(source.value())? else {
            return None;
        };
        // A rest view has already advanced through the source; keep its offset.
        if seq.start != 0 {
            return None;
        }
        seq.deferred.map(|(value, _)| self.owner.value(value))
    }
    /// A finite concatenation view keeps deferred members unread. This is an
    /// internal argument/capture view, not an eager implementation of lazy concat.
    pub(super) fn concat_sequence(
        &self,
        left: &RootedValue,
        right: &RootedValue,
    ) -> Result<RootedValue, CollectionError> {
        self.reserve_collection(1)?;
        if matches!(left.inspect(), ValueView::LazySeq)
            || matches!(right.inspect(), ValueView::LazySeq)
        {
            self.reserve_collection(3)?;
            let mut store = self.owner.store.borrow_mut();
            let thunk = store.allocate(HeapValue::HigherOrder(sequence_core::HigherOrder {
                operation: sequence_core::Combinator::LazyConcat,
                captures: vec![left.value(), right.value()].into_boxed_slice(),
                tail: None,
            }));
            let value = store.allocate(HeapValue::Deferred(deferred::Deferred::new(
                thunk, false, true,
            )));
            return self.owner.checked_value(value);
        }
        let left_len = left.count().unwrap_or(0);
        let right_len = right.count().unwrap_or(0);
        if left_len == 0 {
            return Ok(right.clone());
        }
        let raw = self
            .owner
            .store
            .borrow_mut()
            .allocate(HeapValue::ArgumentSeq(EagerSeq {
                values: Rc::from([]),
                start: 0,
                counted: false,
                projection: None,
                concat: Some(Rc::new(ConcatSeq {
                    left: left.value(),
                    right: right.value(),
                    left_len,
                    len: left_len + right_len,
                    validated: std::cell::Cell::new(0),
                })),
                deferred: None,
            }));
        self.accounting.check().map_err(CollectionError::Resource)?;
        self.owner.checked_value(raw)
    }
    /// Drop one element without realizing a projected entry or copying members.
    pub(super) fn sequence_tail(
        &self,
        sequence: &RootedValue,
    ) -> Result<RootedValue, CollectionError> {
        self.reserve_collection(1)?;
        let mut store = self.owner.store.borrow_mut();
        let raw = match store.heap_value(sequence.value()) {
            Some(HeapValue::List(ListData::Cons { tail, .. }))
            | Some(HeapValue::LazyCons { tail, .. }) => *tail,
            Some(HeapValue::ArgumentSeq(data)) if data.len() > 1 => {
                if let Some(concat) = &data.concat
                    && data.start + 1 == concat.left_len
                {
                    // Lazy concat delegates to the right sequence once its
                    // left side is exhausted. Do not project that right head.
                    concat.right
                } else {
                    let mut data = data.clone();
                    data.start += 1;
                    store.allocate(HeapValue::ArgumentSeq(data))
                }
            }
            _ => store.allocate(HeapValue::List(ListData::Empty)),
        };
        self.accounting.check().map_err(CollectionError::Resource)?;
        self.owner.checked_value(raw)
    }
    pub(super) fn cons_value(
        &self,
        head: &RootedValue,
        tail: &RootedValue,
    ) -> Result<RootedValue, CollectionError> {
        self.reserve_collection(2)?;
        self.check_values(&[head, tail])?;
        if matches!(tail.inspect(), ValueView::Nil) {
            return self.list(std::slice::from_ref(head));
        }
        if matches!(tail.inspect(), ValueView::LazySeq) {
            let raw = self.owner.store.borrow_mut().allocate(HeapValue::LazyCons {
                head: head.value(),
                tail: tail.value(),
            });
            return self.owner.checked_value(raw);
        }
        let len = match tail.inspect() {
            ValueView::List(len) | ValueView::Seq(len) => len,
            _ => return Err(CollectionError::ExpectedSequential),
        };
        let raw = self
            .owner
            .store
            .borrow_mut()
            .allocate(HeapValue::List(ListData::Cons {
                head: head.value(),
                tail: tail.value(),
                len: len + 1,
                proper_list: false,
            }));
        self.accounting.check().map_err(CollectionError::Resource)?;
        self.owner.checked_value(raw)
    }
    pub fn pop(&self, collection: &RootedValue) -> Result<RootedValue, CollectionError> {
        self.reserve_collection(1)?;
        self.check_values(&[collection])?;
        let mut store = self.owner.store.borrow_mut();
        let updated = match store.heap_value(collection.value()) {
            Some(HeapValue::List(ListData::Empty)) => return Err(CollectionError::EmptyCollection),
            Some(HeapValue::List(ListData::Cons {
                tail,
                len,
                proper_list: true,
                ..
            })) => {
                if *len > 1 {
                    return Ok(self.owner.value(*tail));
                }
                HeapValue::List(ListData::Empty)
            }
            Some(HeapValue::Vector(data)) => {
                if data.len() == 0 {
                    return Err(CollectionError::EmptyCollection);
                }
                let mut data = data.clone();
                data.pop();
                HeapValue::Vector(data)
            }
            _ => return Err(CollectionError::ExpectedSequential),
        };
        let metadata = store.metadata(collection.value());
        let value = store.allocate(updated);
        let value = store.annotate(value, metadata);
        self.accounting.check().map_err(CollectionError::Resource)?;
        self.owner.checked_value(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn vector_updates_copy_only_the_changed_tree_path() {
        let mut original = Sequence::new();
        for i in 0..1024 {
            original.push(Value::Integer(i));
        }
        let mut updated = original.clone();
        updated.assoc(37, Value::Integer(-1));
        let (SequenceNode::Branch(a), SequenceNode::Branch(b)) =
            (original.root.as_ref(), updated.root.as_ref())
        else {
            panic!("branches");
        };
        assert_eq!(a.len(), 32);
        for i in 0..32 {
            assert_eq!(Rc::ptr_eq(&a[i], &b[i]), i != 1);
        }
        assert!(original.get(37).unwrap().same_identity(Value::Integer(37)));
        let mut appended = original.clone();
        appended.push(Value::Integer(1024));
        let SequenceNode::Branch(children) = appended.root.as_ref() else {
            panic!("grown root");
        };
        assert!(Rc::ptr_eq(&children[0], &original.root));
    }

    fn nodes(map: &MapData) -> HashSet<*const MapNode> {
        let mut stack: Vec<_> = map.root.iter().cloned().collect();
        let mut result = HashSet::new();
        while let Some(node) = stack.pop() {
            result.insert(Rc::as_ptr(&node));
            if let MapNode::Branch(children) = node.as_ref() {
                stack.extend(children.iter().flatten().cloned());
            }
        }
        result
    }
    #[test]
    fn map_updates_share_unaffected_branches_and_copy_at_most_one_hash_path() {
        let context = Context::new();
        let store = context.owner.store.borrow();
        let mut original = MapData::default();
        for i in 0..1024 {
            original.assoc(&store, Value::Integer(i), Value::Integer(i));
        }
        let old = nodes(&original);
        let mut updated = original.clone();
        updated.assoc(&store, Value::Integer(37), Value::Nil);
        let new = nodes(&updated);
        assert!(old.intersection(&new).count() > 1024);
        assert!(new.difference(&old).count() <= 17);
        let mut removed = original.clone();
        removed.remove(&store, Value::Integer(37));
        assert!(nodes(&removed).difference(&old).count() <= 16);
        assert_eq!(original.len(), 1024);
        assert_eq!(updated.len(), 1024);
        assert_eq!(removed.len(), 1023);
    }
}
