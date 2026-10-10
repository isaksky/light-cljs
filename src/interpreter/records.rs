//! Immutable nominal records share the persistent map payload. Type descriptors
//! are ordinary traced heap values, so redefining a name does not retain its past.
use super::*;
use binding_macros::{call, invalid};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(super) struct TypeId(HeapId);
impl TypeId {
    pub(super) fn value(self) -> Value {
        Value::Heap(self.0)
    }
}
pub(super) struct TypeDescriptor {
    pub(super) name: Name,
    pub(super) keys: Vec<Value>,
    pub(super) implementations: Vec<protocols::Implementation>,
}
impl TypeDescriptor {
    pub(super) fn trace(&self, trace: &mut Vec<Target>) {
        trace.extend(self.keys.iter().copied().map(Target::Value));
        for implementation in &self.implementations {
            implementation.trace(trace);
        }
    }
}
#[derive(Clone)]
pub(super) struct Record {
    pub(super) type_id: TypeId,
    pub(super) data: MapData,
}
impl Record {
    pub(super) fn trace(&self, trace: &mut Vec<Target>) {
        trace.push(Target::Value(self.type_id.value()));
        self.data.trace(trace);
    }
}
pub(super) fn descriptor(store: &Store, id: TypeId) -> &TypeDescriptor {
    let HeapValue::RecordType(t) = store.heap.get(id.0.0).expect("rooted record type") else {
        unreachable!("typed record identity")
    };
    t
}
pub(super) fn replace_implementations(
    store: &mut Store,
    id: TypeId,
    table: Vec<protocols::Implementation>,
) {
    let revision = store
        .protocol_revision
        .checked_add(1)
        .expect("protocol revisions exhausted");
    let HeapValue::RecordType(t) = store.heap.get_mut(id.0.0) else {
        unreachable!("rooted record type")
    };
    t.implementations = table;
    store.protocol_revision = revision;
}
pub(super) fn type_id(value: &RootedValue) -> Option<TypeId> {
    let store = value.owner.store.borrow();
    match store.heap_value(value.value()) {
        Some(HeapValue::Record(r)) => Some(r.type_id),
        Some(HeapValue::RecordType(_)) => match store.unannotated(value.value()) {
            Value::Heap(id) => Some(TypeId(id)),
            _ => unreachable!(),
        },
        _ => None,
    }
}
fn field_keys(c: &Context, fields: &RootedValue) -> Result<Vec<RootedValue>, NativeError> {
    if !matches!(fields.inspect(), ValueView::Vector(_)) {
        return Err(invalid("defrecord fields must be a vector"));
    }
    let mut keys = Vec::new();
    let mut names = std::collections::HashSet::new();
    for field in fields.elements()? {
        c.accounting.work(1)?;
        c.charge_allocation(1)?;
        let ValueView::Symbol(name) = field.inspect() else {
            return Err(invalid(
                "record fields must be distinct unqualified symbols",
            ));
        };
        if name.namespace().is_some() || name == Name::new(None, "&") || !names.insert(name.clone())
        {
            return Err(invalid(
                "record fields must be distinct unqualified symbols",
            ));
        }
        if let Some(meta) = field.metadata() {
            for flag in ["volatile-mutable", "unsynchronized-mutable"] {
                if meta
                    .get(&c.keyword(Name::new(None, flag)))?
                    .is_some_and(|v| collection_core::truthy(&v))
                {
                    return Err(invalid("mutable record fields are unsupported"));
                }
            }
        }
        keys.push(c.keyword(name));
    }
    Ok(keys)
}
fn create_type(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let ValueView::Symbol(name) = args[0].inspect() else {
        return Err(invalid("record type requires a qualified symbol"));
    };
    if name.namespace().is_none() {
        return Err(invalid("record type requires a qualified symbol"));
    }
    c.check_primitive(&Name::new(None, "def"))
        .map_err(InvokeError::Dispatch)?;
    c.check_name(&name, false).map_err(InvokeError::Dispatch)?;
    let keys = field_keys(c, &args[1])?;
    let implementations = protocols::implementation_table(c, &args[2..])?;
    c.charge_allocation(keys.len() as u64 + 1)?;
    let value = Value::Heap(HeapId(c.owner.store.borrow_mut().heap.insert(
        HeapValue::RecordType(TypeDescriptor {
            name,
            keys: keys.iter().map(RootedValue::value).collect(),
            implementations,
        }),
    )));
    Ok(c.owner.checked_value(value)?)
}
fn construct(c: &mut Context, args: &[RootedValue], positional: bool) -> NativeResult {
    if !matches!(args[0].inspect(), ValueView::RecordType(_)) {
        return Err(NativeError::Type {
            argument: 0,
            expected: "record type descriptor",
        });
    }
    let id = type_id(&args[0]).unwrap();
    let keys: Vec<_> = {
        let store = c.owner.store.borrow();
        let t = descriptor(&store, id);
        c.check_name(&t.name, false)
            .map_err(InvokeError::Dispatch)?;
        c.charge_allocation(t.keys.len() as u64 + 1)?;
        t.keys.iter().map(|k| c.owner.value(*k)).collect()
    };
    let mut data = if positional {
        if !matches!(args[1].inspect(), ValueView::Vector(_)) {
            return Err(NativeError::Type {
                argument: 1,
                expected: "positional field vector",
            });
        }
        let values = args[1].elements()?;
        if values.len() != keys.len() {
            return Err(InvokeError::Dispatch(EvalErrorKind::Arity {
                fixed: vec![keys.len()].into_boxed_slice(),
                variadic: None,
                actual: values.len(),
            })
            .into());
        }
        let mut data = MapData::default();
        let store = c.owner.store.borrow();
        for (key, value) in keys.iter().zip(values.iter()) {
            c.accounting.work(1)?;
            data.assoc(&store, key.value(), value.value());
        }
        data
    } else {
        let store = c.owner.store.borrow();
        match store.heap_value(args[1].value()) {
            Some(HeapValue::Map(data)) => data.clone(),
            Some(HeapValue::Record(r)) => r.data.clone(),
            _ if matches!(args[1].inspect(), ValueView::Nil) => MapData::default(),
            _ => {
                return Err(NativeError::Type {
                    argument: 1,
                    expected: "map, record or nil",
                });
            }
        }
    };
    if !positional {
        let store = c.owner.store.borrow();
        for key in keys {
            c.accounting.work(1)?;
            if data.get(&store, key.value()).is_none() {
                data.assoc(&store, key.value(), Value::Nil);
            }
        }
    }
    let value = Value::Heap(HeapId(
        c.owner
            .store
            .borrow_mut()
            .heap
            .insert(HeapValue::Record(Record { type_id: id, data })),
    ));
    Ok(c.owner.checked_value(value)?)
}
const PRIVATE: NativeMetadata = NativeMetadata {
    namespace: "clojure.core",
    name: "defrecord",
    arity: Arity::Exact(2),
    effects: NativeEffects::Context,
    errors: &[NativeErrorClass::Invocation],
};
static CREATE_TYPE: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        arity: Arity::AtLeast(2),
        ..PRIVATE
    },
    callback: create_type,
};
static POSITIONAL: NativeDefinition = NativeDefinition {
    metadata: PRIVATE,
    callback: |c, a| construct(c, a, true),
};
static FROM_MAP: NativeDefinition = NativeDefinition {
    metadata: PRIVATE,
    callback: |c, a| construct(c, a, false),
};

fn defrecord(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let ValueView::Symbol(name) = args[2].inspect() else {
        return Err(invalid("defrecord requires an unqualified type symbol"));
    };
    if name.namespace().is_some() || name == Name::new(None, "&") {
        return Err(invalid("defrecord requires an unqualified type symbol"));
    }
    let keys = field_keys(c, &args[3])?;
    let ValueView::Namespace(namespace) = c.current_namespace().inspect() else {
        unreachable!()
    };
    let qualified = Name::from_utf16(Some(namespace.name()), name.name());
    let type_symbol = c.symbol(qualified.clone());
    let constructor_names = ["->", "map->"].map(|prefix| {
        let units: Vec<_> = prefix
            .encode_utf16()
            .chain(name.name().iter().copied())
            .collect();
        Name::from_utf16(None, &units)
    });
    c.check_primitive(&Name::new(None, "def"))
        .map_err(InvokeError::Dispatch)?;
    for n in std::iter::once(name.clone()).chain(constructor_names.iter().cloned()) {
        c.check_name(&Name::from_utf16(Some(namespace.name()), n.name()), false)
            .map_err(InvokeError::Dispatch)?;
        if let Some(var) = c.lookup_mapping(c.current_namespace_id(), &n) {
            c.check_var_write(var)
                .map_err(|e| InvokeError::Dispatch(e.kind()))?;
            let store = c.owner.store.borrow();
            let v = store.vars.get(var.0).unwrap();
            if !store.var_belongs_to(var, c.current_namespace_id())
                || v.protocol.is_some()
                || v.protocol_method.is_some()
            {
                return Err(invalid(
                    "record declaration conflicts with an existing namespace Var",
                ));
            }
        }
    }
    let fields: Vec<_> = args[3].elements()?.into_iter().zip(keys).collect();
    let groups = protocols::implementation_forms(c, &args[4..], |c, parts| {
        binding_macros::record_method(c, parts, &fields)
    })?;
    let mut create = vec![
        c.native_function(&CREATE_TYPE),
        call(c, "quote", std::slice::from_ref(&type_symbol))?,
        call(c, "quote", &[args[3].clone()])?,
    ];
    create.extend(groups);
    let mut forms = vec![call(c, "def", &[args[2].clone(), c.list(&create)?])?];
    for (index, constructor) in constructor_names.iter().enumerate() {
        let parameters: Vec<_> = (0..if index == 0 { fields.len() } else { 1 })
            .map(|_| binding_macros::fresh(c))
            .collect();
        let input = if index == 0 {
            c.vector(&parameters)?
        } else {
            parameters[0].clone()
        };
        let function = call(
            c,
            "fn*",
            &[
                c.vector(&parameters)?,
                c.list(&[
                    c.native_function(if index == 0 { &POSITIONAL } else { &FROM_MAP }),
                    type_symbol.clone(),
                    input,
                ])?,
            ],
        )?;
        let arglist = if index == 0 {
            args[3].clone()
        } else {
            c.vector(&[c.symbol(Name::new(None, "m"))])?
        };
        let metadata = c.map(&[
            (
                c.keyword(Name::new(None, "arglists")),
                call(c, "quote", &[c.eager_sequence(&[arglist], true)?])?,
            ),
            (
                c.keyword(Name::new(None, "doc")),
                c.string(
                    (if index == 0 {
                        "Positional constructor for "
                    } else {
                        "Map constructor for "
                    })
                    .encode_utf16()
                    .chain(name.name().iter().copied())
                    .chain([u16::from(b'.')])
                    .collect::<Vec<_>>(),
                ),
            ),
        ])?;
        forms.push(call(
            c,
            "def",
            &[
                c.with_meta(&c.symbol(constructor.clone()), &metadata)?,
                function,
            ],
        )?);
    }
    forms.push(type_symbol);
    call(c, "do", &forms)
}

pub fn record_definitions() -> &'static [NativeDefinition] {
    static DEFINITIONS: &[NativeDefinition] = &[
        NativeDefinition {
            metadata: NativeMetadata {
                arity: Arity::AtLeast(4),
                ..PRIVATE
            },
            callback: defrecord,
        },
        NativeDefinition {
            metadata: NativeMetadata {
                namespace: "clojure.core",
                name: "record?",
                arity: Arity::Exact(1),
                effects: NativeEffects::Pure,
                errors: &[],
            },
            callback: |c, a| Ok(c.boolean(matches!(a[0].inspect(), ValueView::Record(_)))),
        },
    ];
    DEFINITIONS
}
