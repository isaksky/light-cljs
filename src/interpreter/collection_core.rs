//! Language-level collection polymorphism over the persistent value layer.
//! No callback runs while the value store is borrowed.
use super::*;

fn type_error(argument: usize, expected: &'static str) -> NativeError {
    NativeError::Type { argument, expected }
}
fn nil(value: &RootedValue) -> bool {
    matches!(value.inspect(), ValueView::Nil)
}
pub(super) fn truthy(value: &RootedValue) -> bool {
    !matches!(value.inspect(), ValueView::Nil | ValueView::Boolean(false))
}

/// A shallow snapshot used only by operations that consume the whole input.
/// Sequence nodes and nested members themselves are never deep-copied.
fn items(
    context: &mut Context,
    value: &RootedValue,
    argument: usize,
) -> Result<Vec<RootedValue>, NativeError> {
    match value.inspect() {
        ValueView::Nil => Ok(Vec::new()),
        ValueView::String(units) => {
            context.charge_allocation(units.len() as u64)?;
            context.accounting.work(units.len() as u64)?;
            Ok(units.iter().map(|u| context.character(*u)).collect())
        }
        ValueView::Map(_) | ValueView::Record(_) => value
            .entries()?
            .into_iter()
            .map(|(k, v)| context.map_entry(&k, &v).map_err(Into::into))
            .collect(),
        ValueView::LazySeq => context.sequence_elements(value),
        ValueView::List(_) | ValueView::Vector(_) | ValueView::Set(_) | ValueView::Seq(_) => {
            Ok(value.elements()?)
        }
        _ => Err(type_error(argument, "seqable value")),
    }
}
pub(super) fn sequence(
    context: &mut Context,
    value: &RootedValue,
    argument: usize,
) -> NativeResult {
    if matches!(value.inspect(), ValueView::LazySeq) {
        return context.realize_lazy(value);
    }
    sequence_ready(context, value, argument)
}
pub(super) fn sequence_ready(
    context: &mut Context,
    value: &RootedValue,
    argument: usize,
) -> NativeResult {
    let mut value = value.clone();
    while let Some(source) = context.deferred_source(&value) {
        context.accounting.work(1)?;
        value = source;
    }
    if matches!(value.inspect(), ValueView::LazySeq) {
        let is_cons = matches!(
            context.owner.store.borrow().heap_value(value.value()),
            Some(HeapValue::LazyCons { .. })
        );
        if is_cons {
            return Ok(value);
        }
        return context.realize_lazy(&value);
    }
    context
        .owner
        .store
        .borrow()
        .force_concat_head(value.value())?;
    match value.inspect() {
        ValueView::List(0) | ValueView::Seq(0) | ValueView::Nil => Ok(context.nil()),
        ValueView::List(_) | ValueView::Seq(_) => Ok(value),
        _ => {
            let members = items(context, &value, argument)?;
            Ok(context.eager_sequence(&members, !matches!(value.inspect(), ValueView::Set(_)))?)
        }
    }
}
// Advance the root as well as the cursor: a discarded prefix does not retain
// its realized nodes. Tail realization occurs only on the next demand.
pub(super) struct Cursor {
    root: RootedValue,
    index: usize,
}
impl Cursor {
    pub(super) fn new(
        context: &mut Context,
        input: &RootedValue,
        argument: usize,
    ) -> Result<Self, NativeError> {
        Ok(Self {
            root: sequence(context, input, argument)?,
            index: 0,
        })
    }
    pub(super) fn is_empty(&self) -> bool {
        matches!(self.root.inspect(), ValueView::Nil)
            || self.root.count().is_some_and(|n| self.index >= n)
    }
    pub(super) fn establish(&mut self, context: &mut Context) -> Result<(), NativeError> {
        if matches!(self.root.inspect(), ValueView::LazySeq) {
            self.root = sequence(context, &self.root, 0)?;
        }
        self.root
            .owner
            .store
            .borrow()
            .force_sequence_position(self.root.value(), self.index)?;
        Ok(())
    }
    pub(super) fn next(
        &mut self,
        context: &mut Context,
    ) -> Result<Option<RootedValue>, NativeError> {
        context
            .poll_execution(PollPoint::Iteration)
            .map_err(InvokeError::Dispatch)?;
        if matches!(self.root.inspect(), ValueView::LazySeq) {
            self.root = sequence(context, &self.root, 0)?;
        }
        if self.is_empty() {
            return Ok(None);
        }
        let next = self
            .root
            .owner
            .store
            .borrow()
            .sequence_step(self.root.value(), self.index)?;
        Ok(next.map(|(head, tail, index)| {
            let head = self.root.owner.value(head);
            self.root = self.root.owner.value(tail);
            self.index = index;
            head
        }))
    }
}
fn lookup(
    context: &mut Context,
    value: &RootedValue,
    key: &RootedValue,
) -> Result<Option<RootedValue>, NativeError> {
    match value.inspect() {
        ValueView::String(units) => Ok(match key.inspect() {
            ValueView::Integer(i) => usize::try_from(i as i32)
                .ok()
                .and_then(|i| units.get(i))
                .map(|u| context.character(*u)),
            ValueView::Double(n) => usize::try_from(n as i32)
                .ok()
                .and_then(|i| units.get(i))
                .map(|u| context.character(*u)),
            ValueView::Ratio {
                numerator,
                denominator,
            } => usize::try_from((numerator / denominator) as i32)
                .ok()
                .and_then(|i| units.get(i))
                .map(|u| context.character(*u)),
            _ => None,
        }),
        ValueView::Vector(_) => match key.inspect() {
            ValueView::Integer(i) => Ok(value.get(&context.integer(i as i32 as i64))?),
            _ => Ok(None),
        },
        ValueView::Map(_) | ValueView::Record(_) | ValueView::Set(_) => {
            context.realize_key(key)?;
            Ok(value.get(key)?)
        }
        _ => Ok(None),
    }
}
fn assoc(
    context: &mut Context,
    value: &RootedValue,
    key: &RootedValue,
    replacement: &RootedValue,
) -> NativeResult {
    context.realize_key(key)?;
    if nil(value) {
        return Ok(context.map(&[(key.clone(), replacement.clone())])?);
    }
    if matches!(value.inspect(), ValueView::Vector(_))
        && !matches!(key.inspect(), ValueView::Integer(_))
    {
        return Err(type_error(1, "integer vector index"));
    }
    let index;
    let key = if matches!(value.inspect(), ValueView::Vector(_)) {
        let ValueView::Integer(i) = key.inspect() else {
            unreachable!()
        };
        index = context.integer(i as i32 as i64);
        &index
    } else {
        key
    };
    Ok(context.assoc(value, key, replacement)?)
}
pub(super) fn conj(
    context: &mut Context,
    value: &RootedValue,
    member: &RootedValue,
) -> NativeResult {
    if matches!(value.inspect(), ValueView::Set(_)) {
        context.realize_key(member)?;
    }
    if matches!(value.inspect(), ValueView::Map(_) | ValueView::Record(_))
        && matches!(member.inspect(), ValueView::Vector(_))
        && let Some(key) = member.nth(0)?
    {
        context.realize_key(&key)?;
    }
    match value.inspect() {
        ValueView::Nil => Ok(context.list(std::slice::from_ref(member))?),
        ValueView::Map(_) | ValueView::Record(_) => match member.inspect() {
            ValueView::Nil => Ok(value.clone()),
            ValueView::Vector(_) => Ok(context.conj(value, member)?),
            _ => {
                let mut result = value.clone();
                let mut entries = Cursor::new(context, member, 1)?;
                while let Some(entry) = entries.next(context)? {
                    if !context.is_map_entry(&entry) {
                        return Err(type_error(1, "sequence of map entries"));
                    }
                    result = context.conj(&result, &entry)?;
                }
                Ok(result)
            }
        },
        ValueView::LazySeq => {
            let tail = sequence(context, value, 0)?;
            Ok(context.cons_value(member, &tail)?)
        }
        ValueView::Seq(_) => Ok(context.cons_value(member, value)?),
        _ => Ok(context.conj(value, member)?),
    }
}
fn nth(
    context: &mut Context,
    value: &RootedValue,
    key: &RootedValue,
    default: Option<&RootedValue>,
) -> NativeResult {
    // JVM nth accepts numeric indexes via intValue, unlike get/vector invocation.
    let index = match key.inspect() {
        ValueView::Integer(i) => {
            i32::try_from(i).map_err(|_| NativeError::Arithmetic(ArithmeticError::Overflow))?
        }
        ValueView::Double(n) => i32::try_from(n as i64)
            .map_err(|_| NativeError::Arithmetic(ArithmeticError::Overflow))?,
        ValueView::Ratio {
            numerator,
            denominator,
        } => i32::try_from(numerator / denominator)
            .map_err(|_| NativeError::Arithmetic(ArithmeticError::Overflow))?,
        _ => return Err(type_error(1, "numeric index")),
    };
    if nil(value) {
        return Ok(default.cloned().unwrap_or_else(|| context.nil()));
    }
    let found = match value.inspect() {
        ValueView::String(units) => usize::try_from(index)
            .ok()
            .and_then(|i| units.get(i))
            .map(|u| context.character(*u)),
        ValueView::LazySeq if index < 0 => None,
        ValueView::LazySeq => {
            let mut cursor = Cursor::new(context, value, 0)?;
            let mut found = None;
            for _ in 0..=index {
                found = cursor.next(context)?;
                if found.is_none() {
                    break;
                }
            }
            found
        }
        ValueView::List(_) | ValueView::Vector(_) | ValueView::Seq(_) => {
            if index < 0 {
                None
            } else {
                value.nth(index as usize)?
            }
        }
        _ => return Err(type_error(0, "indexed or sequential value")),
    };
    found
        .or_else(|| default.cloned())
        .ok_or(NativeError::Collection(CollectionError::InvalidIndex))
}

/// Shared projection retains both row metadata and the map's original equal
/// key representatives, including symbol metadata.
pub(super) fn select_keys(
    context: &mut Context,
    source: &RootedValue,
    keys: &RootedValue,
) -> NativeResult {
    let mut result = context.map(&[])?;
    let mut keys = Cursor::new(context, keys, 1)?;
    while let Some(key) = keys.next(context)? {
        if !matches!(
            source.inspect(),
            ValueView::Nil | ValueView::Map(_) | ValueView::Record(_) | ValueView::Vector(_)
        ) {
            return Err(type_error(0, "associative value"));
        }
        if let Some(value) = lookup(context, source, &key)? {
            let key = if matches!(source.inspect(), ValueView::Map(_) | ValueView::Record(_)) {
                context.retained_key(source, &key)?.unwrap()
            } else {
                key
            };
            result = context.assoc(&result, &key, &value)?;
        }
    }
    if let Some(meta) = source.metadata() {
        result = context.with_meta(&result, &meta)?;
    }
    Ok(result)
}

#[derive(Clone, Copy)]
enum Operation {
    List,
    ListStar,
    Vector,
    Vec,
    HashMap,
    HashSet,
    Set,
    Conj,
    Disj,
    Assoc,
    Dissoc,
    Get,
    GetIn,
    AssocIn,
    Update,
    UpdateIn,
    Contains,
    Find,
    Nth,
    Count,
    Empty,
    EmptyP,
    Keys,
    Vals,
    SelectKeys,
    Merge,
    MergeWith,
    Peek,
    Pop,
    Seqable,
    Indexed,
    Reversible,
    MapEntry,
}
fn call(context: &mut Context, args: &[RootedValue], operation: Operation) -> NativeResult {
    use Operation::*;
    match operation {
        List => Ok(context.list(args)?),
        ListStar => {
            let mut tail = sequence(context, args.last().unwrap(), args.len() - 1)?;
            if args.len() > 1 && nil(&tail) && !nil(args.last().unwrap()) {
                tail = context.list(&[])?;
            }
            for head in args[..args.len() - 1].iter().rev() {
                tail = context.cons_value(head, &tail)?;
            }
            Ok(tail)
        }
        Vector => Ok(context.vector(args)?),
        Vec => {
            if matches!(args[0].inspect(), ValueView::Vector(_)) && !context.is_map_entry(&args[0])
            {
                return Ok(context.with_meta(&args[0], &context.nil())?);
            }
            {
                let values = items(context, &args[0], 0)?;
                Ok(context.vector(&values)?)
            }
        }
        HashMap => {
            for pair in args.chunks(2) {
                context.realize_key(&pair[0])?;
            }
            if !args.len().is_multiple_of(2) {
                return Err(type_error(args.len() - 1, "key followed by a value"));
            }
            let pairs: std::vec::Vec<_> = args
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| (pair[0].clone(), pair[1].clone()))
                .collect();
            Ok(context.map(&pairs)?)
        }
        HashSet => {
            for value in args {
                context.realize_key(value)?;
            }
            Ok(context.set(args)?)
        }
        Set => {
            if matches!(args[0].inspect(), ValueView::Set(_)) {
                return Ok(context.with_meta(&args[0], &context.nil())?);
            }
            {
                let values = items(context, &args[0], 0)?;
                for value in &values {
                    context.realize_key(value)?;
                }
                Ok(context.set(&values)?)
            }
        }
        Conj => {
            let mut result = args
                .first()
                .cloned()
                .map(Ok)
                .unwrap_or_else(|| context.vector(&[]))?;
            for member in &args[1.min(args.len())..] {
                result = conj(context, &result, member)?;
            }
            Ok(result)
        }
        Assoc => {
            if args.len().is_multiple_of(2) {
                return Err(type_error(args.len() - 1, "key followed by a value"));
            }
            let mut result = args[0].clone();
            for pair in args[1..].as_chunks::<2>().0.iter() {
                result = assoc(context, &result, &pair[0], &pair[1])?;
            }
            Ok(result)
        }
        Disj | Dissoc => {
            let mut result = args[0].clone();
            if nil(&result) {
                return Ok(result);
            }
            for key in &args[1..] {
                context.realize_key(key)?;
                result = if matches!(operation, Disj) {
                    context.disj(&result, key)?
                } else {
                    context.dissoc(&result, key)?
                };
            }
            Ok(result)
        }
        Get => Ok(lookup(context, &args[0], &args[1])?
            .unwrap_or_else(|| args.get(2).cloned().unwrap_or_else(|| context.nil()))),
        Contains => {
            if !matches!(
                args[0].inspect(),
                ValueView::Nil
                    | ValueView::Map(_)
                    | ValueView::Record(_)
                    | ValueView::Set(_)
                    | ValueView::Vector(_)
                    | ValueView::String(_)
            ) {
                return Err(type_error(0, "map, set, vector, string or nil"));
            }
            if matches!(args[0].inspect(), ValueView::String(_))
                && !matches!(
                    args[1].inspect(),
                    ValueView::Integer(_) | ValueView::Double(_) | ValueView::Ratio { .. }
                )
            {
                return Err(type_error(1, "numeric string index"));
            }
            {
                let found = lookup(context, &args[0], &args[1])?.is_some();
                Ok(context.boolean(found))
            }
        }
        Find => {
            if nil(&args[0]) {
                return Ok(context.nil());
            }
            if !matches!(
                args[0].inspect(),
                ValueView::Map(_) | ValueView::Record(_) | ValueView::Vector(_)
            ) {
                return Err(type_error(0, "associative value"));
            }
            match lookup(context, &args[0], &args[1])? {
                None => Ok(context.nil()),
                Some(value) => {
                    // find retains the original equal key representative.
                    let key =
                        if matches!(args[0].inspect(), ValueView::Map(_) | ValueView::Record(_)) {
                            context.retained_key(&args[0], &args[1])?.unwrap()
                        } else {
                            args[1].clone()
                        };
                    Ok(context.map_entry(&key, &value)?)
                }
            }
        }
        GetIn => {
            let mut value = args[0].clone();
            let mut keys = Cursor::new(context, &args[1], 1)?;
            while let Some(key) = keys.next(context)? {
                match lookup(context, &value, &key)? {
                    Some(found) => value = found,
                    None => return Ok(args.get(2).cloned().unwrap_or_else(|| context.nil())),
                }
            }
            Ok(value)
        }
        AssocIn | UpdateIn | Update => {
            let mut keys = if matches!(operation, Update) {
                vec![args[1].clone()]
            } else {
                items(context, &args[1], 1)?
            };
            if keys.is_empty() {
                keys.push(context.nil());
            }
            let mut value = args[0].clone();
            let mut parents = std::vec::Vec::with_capacity(keys.len() - 1);
            for key in &keys[..keys.len() - 1] {
                let child = lookup(context, &value, key)?.unwrap_or_else(|| context.nil());
                parents.push(value);
                value = child;
            }
            let last = keys.last().unwrap();
            let replacement = if matches!(operation, AssocIn) {
                args[2].clone()
            } else {
                let mut arguments = std::vec::Vec::with_capacity(args.len() - 2);
                arguments.push(lookup(context, &value, last)?.unwrap_or_else(|| context.nil()));
                arguments.extend_from_slice(&args[3..]);
                context.invoke(&args[2], &arguments)?
            };
            let mut result = assoc(context, &value, last, &replacement)?;
            for (parent, key) in parents.into_iter().zip(keys).rev() {
                result = assoc(context, &parent, &key, &result)?;
            }
            Ok(result)
        }
        Nth => nth(context, &args[0], &args[1], args.get(2)),
        Count => {
            if matches!(args[0].inspect(), ValueView::LazySeq) {
                let mut cursor = Cursor::new(context, &args[0], 0)?;
                let mut count = 0;
                while cursor.next(context)?.is_some() {
                    count += 1;
                }
                return Ok(context.integer(count));
            }
            context
                .owner
                .store
                .borrow()
                .check_sequence_count(args[0].value())?;
            let count = match args[0].inspect() {
                ValueView::Nil => 0,
                ValueView::String(units) => units.len(),
                _ => args[0]
                    .count()
                    .ok_or_else(|| type_error(0, "countable value"))?,
            };
            Ok(context.integer(count as i64))
        }
        EmptyP => {
            if matches!(args[0].inspect(), ValueView::LazySeq) {
                let seq = sequence(context, &args[0], 0)?;
                return Ok(context.boolean(matches!(seq.inspect(), ValueView::Nil)));
            }
            context
                .owner
                .store
                .borrow()
                .force_concat_head(args[0].value())?;
            let empty = match args[0].inspect() {
                ValueView::Nil => true,
                ValueView::String(units) => units.is_empty(),
                ValueView::List(n)
                | ValueView::Seq(n)
                | ValueView::Vector(n)
                | ValueView::Map(n)
                | ValueView::Record(n)
                | ValueView::Set(n) => n == 0,
                _ => return Err(type_error(0, "seqable value")),
            };
            Ok(context.boolean(empty))
        }
        Empty => {
            let empty = match args[0].inspect() {
                ValueView::Seq(_) | ValueView::LazySeq => return Ok(context.list(&[])?),
                ValueView::List(_) => context.list(&[])?,
                ValueView::Vector(_) if context.is_map_entry(&args[0]) => return Ok(context.nil()),
                ValueView::Vector(_) => context.vector(&[])?,
                ValueView::Map(_) => context.map(&[])?,
                ValueView::Record(_) => return Err(CollectionError::UnsupportedRecordEmpty.into()),
                ValueView::Set(_) => context.set(&[])?,
                _ => return Ok(context.nil()),
            };
            match args[0].metadata() {
                Some(meta) => Ok(context.with_meta(&empty, &meta)?),
                None => Ok(empty),
            }
        }
        Keys | Vals => {
            if matches!(args[0].inspect(), ValueView::LazySeq) {
                return context.lazy_projection(&args[0], usize::from(matches!(operation, Vals)));
            }
            // Entry projection is delayed: keys/vals may be returned or counted
            // without reading an invalid member. nth/vec/equality force it.
            let source = match args[0].inspect() {
                ValueView::List(_) | ValueView::Seq(_) | ValueView::Vector(_) => args[0].clone(),
                _ => {
                    let values = items(context, &args[0], 0)?;
                    context.eager_sequence(&values, true)?
                }
            };
            Ok(context.projected_sequence(&source, usize::from(matches!(operation, Vals)))?)
        }
        SelectKeys => select_keys(context, &args[0], &args[1]),
        Merge | MergeWith => {
            let start = usize::from(matches!(operation, MergeWith));
            let maps = &args[start..];
            if !maps.iter().any(truthy) {
                return Ok(context.nil());
            }
            let Some((first, rest)) = maps.split_first() else {
                return Ok(context.nil());
            };
            let mut result = first.clone();
            for map in rest {
                if !truthy(&result) {
                    result = context.map(&[])?;
                }
                if matches!(operation, Merge) {
                    result = conj(context, &result, map)?;
                } else {
                    let mut entries = Cursor::new(context, map, start)?;
                    while let Some(entry) = entries.next(context)? {
                        if !context.is_map_entry(&entry) {
                            return Err(type_error(start, "map or sequence of map entries"));
                        }
                        let key = entry.nth(0)?.unwrap();
                        let incoming = entry.nth(1)?.unwrap();
                        let value = match lookup(context, &result, &key)? {
                            Some(existing) => context.invoke(&args[0], &[existing, incoming])?,
                            None => incoming,
                        };
                        result = assoc(context, &result, &key, &value)?;
                    }
                }
            }
            Ok(result)
        }
        Peek | Pop => {
            if nil(&args[0]) {
                return Ok(context.nil());
            }
            let index = match args[0].inspect() {
                ValueView::List(_) => 0,
                ValueView::Vector(n) => n.saturating_sub(1),
                _ => return Err(type_error(0, "list, vector or nil")),
            };
            if matches!(operation, Pop) {
                Ok(context.pop(&args[0])?)
            } else {
                Ok(args[0].nth(index)?.unwrap_or_else(|| context.nil()))
            }
        }
        Seqable => Ok(context.boolean(matches!(
            args[0].inspect(),
            ValueView::LazySeq
                | ValueView::Nil
                | ValueView::String(_)
                | ValueView::List(_)
                | ValueView::Seq(_)
                | ValueView::Map(_)
                | ValueView::Record(_)
                | ValueView::Set(_)
                | ValueView::Vector(_)
        ))),
        Indexed | Reversible => {
            Ok(context.boolean(matches!(args[0].inspect(), ValueView::Vector(_))))
        }
        MapEntry => Ok(context.boolean(context.is_map_entry(&args[0]))),
    }
}

/// Callable data shares arity, owner and failure handling with ordinary calls.
pub(super) fn invoke_data(
    context: &mut Context,
    callee: &RootedValue,
    args: &[RootedValue],
) -> Option<Result<RootedValue, InvokeError>> {
    let arities: &[usize] = match callee.inspect() {
        ValueView::Symbol(_) | ValueView::Keyword(_) | ValueView::Map(_) => &[1, 2],
        ValueView::Vector(_) | ValueView::Set(_) => &[1],
        _ => return None,
    };
    let result = (|| {
        context.poll_execution(PollPoint::Function)?;
        if !arities.contains(&args.len()) {
            return Err(EvalErrorKind::Arity {
                fixed: arities.into(),
                variadic: None,
                actual: args.len(),
            });
        }
        let result = match callee.inspect() {
            ValueView::Symbol(_) | ValueView::Keyword(_) => {
                lookup(context, &args[0], callee).map(|v| {
                    v.unwrap_or_else(|| args.get(1).cloned().unwrap_or_else(|| context.nil()))
                })
            }
            ValueView::Vector(_) => {
                if !matches!(args[0].inspect(), ValueView::Integer(_)) {
                    Err(type_error(0, "integer vector index"))
                } else {
                    lookup(context, callee, &args[0]).and_then(|v| {
                        v.ok_or(NativeError::Collection(CollectionError::InvalidIndex))
                    })
                }
            }
            _ => lookup(context, callee, &args[0]).map(|v| {
                v.unwrap_or_else(|| args.get(1).cloned().unwrap_or_else(|| context.nil()))
            }),
        };
        result.map_err(|error| EvalErrorKind::Native {
            name: Box::new(Name::new(None, "collection invocation")),
            error,
        })
    })();
    Some(result.map_err(|kind| match kind {
        EvalErrorKind::Native {
            error: NativeError::Invocation(error),
            ..
        } => error.into_error(),
        kind => InvokeError::Dispatch(kind),
    }))
}

macro_rules! def {
    ($name:literal, $arity:expr, $op:ident) => {
        def!($name, $arity, $op, Context)
    };
    ($name:literal, $arity:expr, $op:ident, $effects:ident) => {
        NativeDefinition {
            metadata: NativeMetadata {
                namespace: "clojure.core",
                name: $name,
                arity: $arity,
                effects: NativeEffects::$effects,
                errors: &[
                    NativeErrorClass::Arity,
                    NativeErrorClass::Type,
                    NativeErrorClass::Arithmetic,
                    NativeErrorClass::Collection,
                    NativeErrorClass::Invocation,
                ],
            },
            callback: |context, args| call(context, args, Operation::$op),
        }
    };
}
pub fn collection_definitions() -> &'static [NativeDefinition] {
    use Arity::{AtLeast, Exact, Range};
    static DEFINITIONS: &[NativeDefinition] = &[
        def!("list", AtLeast(0), List),
        def!("list*", AtLeast(1), ListStar),
        def!("vector", AtLeast(0), Vector),
        def!("vec", Exact(1), Vec),
        def!("hash-map", AtLeast(0), HashMap),
        def!("array-map", AtLeast(0), HashMap),
        def!("hash-set", AtLeast(0), HashSet),
        def!("set", Exact(1), Set),
        def!("conj", AtLeast(0), Conj),
        def!("disj", AtLeast(1), Disj),
        def!("assoc", AtLeast(3), Assoc),
        def!("dissoc", AtLeast(1), Dissoc),
        def!("get", Range { min: 2, max: 3 }, Get),
        def!("get-in", Range { min: 2, max: 3 }, GetIn),
        def!("assoc-in", Exact(3), AssocIn),
        def!("update", AtLeast(3), Update, Context),
        def!("update-in", AtLeast(3), UpdateIn, Context),
        def!("contains?", Exact(2), Contains),
        def!("find", Exact(2), Find),
        def!("nth", Range { min: 2, max: 3 }, Nth),
        def!("count", Exact(1), Count),
        def!("empty", Exact(1), Empty),
        def!("empty?", Exact(1), EmptyP),
        def!("keys", Exact(1), Keys),
        def!("vals", Exact(1), Vals),
        def!("select-keys", Exact(2), SelectKeys),
        def!("merge", AtLeast(0), Merge),
        def!("merge-with", AtLeast(1), MergeWith, Context),
        def!("peek", Exact(1), Peek),
        def!("pop", Exact(1), Pop),
        def!("seqable?", Exact(1), Seqable),
        def!("indexed?", Exact(1), Indexed),
        def!("reversible?", Exact(1), Reversible),
        def!("map-entry?", Exact(1), MapEntry),
    ];
    DEFINITIONS
}
