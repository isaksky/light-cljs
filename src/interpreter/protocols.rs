//! User protocol declarations and ordinary heap-backed method callables.
//! Descriptors form traced cycles with their methods; no global registry roots
//! old declarations. Captured reify objects own raw descriptor/closure tables.
use super::*;
use binding_macros::{call, invalid};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(super) struct ProtocolId(HeapId);
impl ProtocolId {
    pub(super) fn value(self) -> Value {
        Value::Heap(self.0)
    }
}
pub(super) struct Protocol {
    pub(super) name: Name,
    methods: Vec<Method>,
}
struct Method {
    name: Name,
    arities: Box<[usize]>,
    function: Value,
}
impl Protocol {
    pub(super) fn trace(&self, trace: &mut Vec<Target>) {
        trace.extend(self.methods.iter().map(|m| Target::Value(m.function)));
    }
}
#[derive(Clone, Copy)]
pub(super) struct MethodFunction {
    pub(super) protocol: ProtocolId,
    method: usize,
}
#[derive(Clone)]
pub(super) struct Implementation {
    pub(super) protocol: ProtocolId,
    /// A missing method is a partial, nominal implementation.
    pub(super) methods: Vec<(usize, Value)>,
}
impl Implementation {
    pub(super) fn trace(&self, trace: &mut Vec<Target>) {
        trace.push(Target::Value(self.protocol.value()));
        trace.extend(
            self.methods
                .iter()
                .map(|(_, function)| Target::Value(*function)),
        );
    }
}
fn implementations(store: &Store, receiver: Value) -> &[Implementation] {
    match store.heap_value(receiver) {
        Some(HeapValue::Reify(table)) => return table,
        Some(HeapValue::Record(record)) => {
            return &records::descriptor(store, record.type_id).implementations;
        }
        _ => {}
    }
    #[cfg(test)]
    if let Some(HeapValue::ProtocolFixture(table)) = store.heap_value(receiver) {
        return table;
    }
    &[]
}
fn descriptor(store: &Store, id: ProtocolId) -> &Protocol {
    let HeapValue::Protocol(protocol) = store.heap.get(id.0.0).expect("rooted protocol") else {
        unreachable!("typed protocol identity")
    };
    protocol
}
fn protocol_id(value: &RootedValue) -> Option<ProtocolId> {
    let store = value.owner.store.borrow();
    let Value::Heap(id) = store.unannotated(value.value()) else {
        return None;
    };
    matches!(store.heap.get(id.0), Ok(HeapValue::Protocol(_))).then_some(ProtocolId(id))
}
struct Declaration {
    name: Name,
    symbol: RootedValue,
    doc: Option<RootedValue>,
    methods: Vec<DeclarationMethod>,
}
struct DeclarationMethod {
    name: Name,
    symbol: RootedValue,
    doc: Option<RootedValue>,
    arglists: Vec<RootedValue>,
    arities: Vec<usize>,
}
fn declaration(
    c: &Context,
    namespace: &Name,
    args: &[RootedValue],
) -> Result<Declaration, NativeError> {
    c.accounting.work(args.len() as u64 + 1)?;
    c.charge_allocation(args.len() as u64 + 1)?;
    let Some(symbol) = args.first() else {
        return Err(invalid("defprotocol requires a symbol"));
    };
    let ValueView::Symbol(name) = symbol.inspect() else {
        return Err(invalid("defprotocol requires a symbol"));
    };
    if name.namespace().is_some_and(|ns| ns != namespace.name()) {
        return Err(invalid(
            "defprotocol cannot define outside the current namespace",
        ));
    }
    let name = Name::from_utf16(Some(namespace.name()), name.name());
    let mut tail = &args[1..];
    let doc = tail
        .first()
        .filter(|v| matches!(v.inspect(), ValueView::String(_)))
        .cloned();
    if doc.is_some() {
        tail = &tail[1..];
    }
    let mut methods = Vec::new();
    let mut names = std::collections::HashSet::new();
    for method in tail {
        c.accounting.work(1)?;
        if !matches!(method.inspect(), ValueView::List(_) | ValueView::Seq(_)) {
            return Err(invalid(
                "defprotocol requires method lists; protocol options are unsupported",
            ));
        }
        let parts = method.elements()?;
        c.charge_allocation(parts.len() as u64 + 1)?;
        let Some(symbol) = parts.first() else {
            return Err(invalid("protocol method requires a symbol"));
        };
        let ValueView::Symbol(method_name) = symbol.inspect() else {
            return Err(invalid("protocol method requires a symbol"));
        };
        if method_name.namespace().is_some() || method_name.name() == name.name() {
            return Err(invalid(
                "protocol method must have a distinct unqualified name",
            ));
        }
        if !names.insert(method_name.clone()) {
            return Err(invalid("duplicate protocol method"));
        }
        let mut signatures = &parts[1..];
        let doc = signatures
            .last()
            .filter(|v| matches!(v.inspect(), ValueView::String(_)))
            .cloned();
        if doc.is_some() {
            signatures = &signatures[..signatures.len() - 1];
        }
        if signatures.is_empty() {
            return Err(invalid("protocol method requires a receiver signature"));
        }
        let mut arities = Vec::new();
        for signature in signatures {
            c.accounting.work(1)?;
            if !matches!(signature.inspect(), ValueView::Vector(_)) {
                return Err(invalid("protocol signature must be a vector"));
            }
            let parameters = signature.elements()?;
            c.charge_allocation(parameters.len() as u64 + 1)?;
            if parameters.is_empty() {
                return Err(invalid("protocol signature requires a receiver"));
            }
            let mut bindings = std::collections::HashSet::new();
            for parameter in &parameters {
                c.accounting.work(1)?;
                let ValueView::Symbol(n) = parameter.inspect() else {
                    return Err(invalid("protocol parameters must be symbols"));
                };
                if n == Name::new(None, "&") {
                    return Err(invalid("variadic protocol signatures are unsupported"));
                }
                if n.namespace().is_some() || !bindings.insert(n) {
                    return Err(invalid(
                        "protocol parameters must be distinct unqualified symbols",
                    ));
                }
            }
            c.accounting.work(arities.len() as u64)?;
            if arities.contains(&parameters.len()) {
                return Err(invalid("duplicate protocol method arity"));
            }
            arities.push(parameters.len());
        }
        arities.sort_unstable();
        methods.push(DeclarationMethod {
            name: Name::from_utf16(Some(namespace.name()), method_name.name()),
            symbol: symbol.clone(),
            doc,
            arglists: signatures.to_vec(),
            arities,
        });
    }
    Ok(Declaration {
        name,
        symbol: symbol.clone(),
        doc,
        methods,
    })
}
fn checked_existing(
    c: &Context,
    ns: NamespaceId,
    declaration: &Declaration,
) -> Result<Option<ProtocolId>, NativeError> {
    c.check_primitive(&Name::new(None, "def"))
        .map_err(InvokeError::Dispatch)?;
    c.check_name(&declaration.name, false)
        .map_err(InvokeError::Dispatch)?;
    let bare = Name::from_utf16(None, declaration.name.name());
    let var = c.lookup_mapping(ns, &bare);
    let existing = if let Some(var) = var {
        c.check_var_write(var)
            .map_err(|e| InvokeError::Dispatch(e.kind()))?;
        let store = c.owner.store.borrow();
        if !store.var_belongs_to(var, ns) {
            return Err(invalid("defprotocol conflicts with a referred Var"));
        }
        let v = store.vars.get(var.0).unwrap();
        if v.protocol_method.is_some() {
            return Err(invalid(
                "protocol name conflicts with a protocol method Var",
            ));
        }
        v.protocol
    } else {
        None
    };
    if let Some(id) = existing {
        let store = c.owner.store.borrow();
        let protocol = descriptor(&store, id);
        c.accounting.work(
            (protocol.methods.len() as u64).saturating_mul(declaration.methods.len() as u64),
        )?;
        if protocol.methods.len() != declaration.methods.len()
            || declaration.methods.iter().any(|m| {
                !protocol
                    .methods
                    .iter()
                    .any(|old| old.name == m.name && old.arities.as_ref() == m.arities)
            })
        {
            return Err(invalid(
                "changing a protocol's method names or arities is unsupported",
            ));
        }
    }
    for method in &declaration.methods {
        c.accounting.work(1)?;
        c.check_name(&method.name, false)
            .map_err(InvokeError::Dispatch)?;
        let bare = Name::from_utf16(None, method.name.name());
        if let Some(var) = c.lookup_mapping(ns, &bare) {
            c.check_var_write(var)
                .map_err(|e| InvokeError::Dispatch(e.kind()))?;
            let store = c.owner.store.borrow();
            let v = store.vars.get(var.0).unwrap();
            if !store.var_belongs_to(var, ns)
                || v.protocol.is_some()
                || v.protocol_method.is_some_and(|(p, _)| Some(p) != existing)
                || (v.root.is_some() && v.protocol_method.is_none())
            {
                return Err(invalid(
                    "protocol method conflicts with an existing namespace Var",
                ));
            }
        }
    }
    c.accounting.check()?;
    Ok(existing)
}
fn install(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let namespace = args[0].namespace_id().ok_or(NativeError::Type {
        argument: 0,
        expected: "namespace",
    })?;
    let ns = c
        .local_namespace(namespace)
        .map_err(|_| NativeError::WrongOwner)?;
    let ValueView::Namespace(name) = c
        .namespace_value(ns)
        .map_err(|_| NativeError::WrongOwner)?
        .inspect()
    else {
        unreachable!()
    };
    let declaration = declaration(c, &name, &args[1].elements()?)?;
    if ns == c.registry().borrow().core {
        c.core_var(&Name::from_utf16(None, declaration.name.name()));
        for method in &declaration.methods {
            c.core_var(&Name::from_utf16(None, method.name.name()));
        }
    }
    let existing = checked_existing(c, ns, &declaration)?;
    let protocol_var = c
        .intern_language_var(ns, Name::from_utf16(None, declaration.name.name()))
        .map_err(|e| InvokeError::Dispatch(EvalErrorKind::Namespace(e)))?;
    let mut method_vars = Vec::new();
    for method in &declaration.methods {
        c.accounting.work(1)?;
        method_vars.push(
            c.intern_language_var(ns, Name::from_utf16(None, method.name.name()))
                .map_err(|e| InvokeError::Dispatch(EvalErrorKind::Namespace(e)))?,
        );
    }
    c.charge_allocation(declaration.methods.len() as u64 * 3 + 1)?;
    for method in &declaration.methods {
        c.charge_allocation(method.arities.len() as u64)?;
    }
    c.accounting.work(
        (declaration.methods.len() as u64).saturating_mul(declaration.methods.len() as u64),
    )?;
    // No callback or collection between construction and publishing the complete
    // table; all allocating validation has already succeeded.
    let mut store = c.owner.store.borrow_mut();
    let id = existing.unwrap_or_else(|| {
        let id = ProtocolId(HeapId(store.heap.insert(HeapValue::Protocol(Protocol {
            name: declaration.name.clone(),
            methods: Vec::new(),
        }))));
        let methods = declaration
            .methods
            .iter()
            .enumerate()
            .map(|(index, method)| {
                let function = Value::Heap(HeapId(store.heap.insert(HeapValue::ProtocolMethod(
                    MethodFunction {
                        protocol: id,
                        method: index,
                    },
                ))));
                Method {
                    name: method.name.clone(),
                    arities: method.arities.clone().into_boxed_slice(),
                    function,
                }
            })
            .collect();
        let HeapValue::Protocol(p) = store.heap.get_mut(id.0.0) else {
            unreachable!()
        };
        p.methods = methods;
        id
    });
    // allocation accounting is sticky: do not attach a failed construction.
    c.accounting.check()?;
    store
        .vars
        .get_mut(protocol_var.var_id().unwrap().0)
        .protocol = Some(id);
    for (declaration, var) in declaration.methods.iter().zip(method_vars) {
        let index = descriptor(&store, id)
            .methods
            .iter()
            .position(|m| m.name == declaration.name)
            .unwrap();
        store.vars.get_mut(var.var_id().unwrap().0).protocol_method = Some((id, index));
    }
    store.protocol_revision = store
        .protocol_revision
        .checked_add(1)
        .expect("protocol revisions exhausted");
    drop(store);
    Ok(c.owner.value(id.value()))
}
fn method_function(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let id = protocol_id(&args[0]).ok_or(NativeError::Type {
        argument: 0,
        expected: "protocol",
    })?;
    let ValueView::Symbol(name) = args[1].inspect() else {
        return Err(NativeError::Type {
            argument: 1,
            expected: "method symbol",
        });
    };
    c.accounting
        .work(descriptor(&c.owner.store.borrow(), id).methods.len() as u64)?;
    let value = descriptor(&c.owner.store.borrow(), id)
        .methods
        .iter()
        .find(|m| m.name.name() == name.name())
        .ok_or_else(|| invalid("unknown protocol method"))?
        .function;
    Ok(c.owner.value(value))
}
const PRIVATE_METADATA: NativeMetadata = NativeMetadata {
    // Private callable literals require the same permission as their public macro;
    // no helper Vars are installed or discoverable through namespace resolution.
    namespace: "clojure.core",
    name: "defprotocol",
    arity: Arity::Exact(2),
    effects: NativeEffects::Context,
    errors: &[NativeErrorClass::Invocation],
};
static INSTALL: NativeDefinition = NativeDefinition {
    metadata: PRIVATE_METADATA,
    callback: install,
};
static METHOD: NativeDefinition = NativeDefinition {
    metadata: PRIVATE_METADATA,
    callback: method_function,
};
fn defprotocol(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let namespace = c.current_namespace();
    let ValueView::Namespace(name) = namespace.inspect() else {
        unreachable!()
    };
    let declaration = declaration(c, &name, &args[2..])?;
    checked_existing(c, namespace.namespace_id().unwrap(), &declaration)?;
    let local = binding_macros::fresh(c);
    let bare = c.symbol(Name::from_utf16(None, declaration.name.name()));
    let mut metadata = declaration.symbol.metadata().unwrap_or(c.map(&[])?);
    if let Some(doc) = declaration.doc {
        metadata = c.assoc(&metadata, &c.keyword(Name::new(None, "doc")), &doc)?;
    }
    let mut body = vec![call(
        c,
        "def",
        &[c.with_meta(&bare, &metadata)?, local.clone()],
    )?];
    for method in declaration.methods {
        c.accounting.work(1)?;
        let mut metadata = method.symbol.metadata().unwrap_or(c.map(&[])?);
        if let Some(doc) = method.doc {
            metadata = c.assoc(&metadata, &c.keyword(Name::new(None, "doc")), &doc)?;
        }
        metadata = c.assoc(
            &metadata,
            &c.keyword(Name::new(None, "protocol")),
            &c.list(&[c.symbol(Name::new(None, "var")), bare.clone()])?,
        )?;
        metadata = c.assoc(
            &metadata,
            &c.keyword(Name::new(None, "arglists")),
            &call(c, "quote", &[c.eager_sequence(&method.arglists, true)?])?,
        )?;
        body.push(call(
            c,
            "def",
            &[
                c.with_meta(&method.symbol, &metadata)?,
                c.list(&[
                    c.native_function(&METHOD),
                    local.clone(),
                    call(c, "quote", std::slice::from_ref(&method.symbol))?,
                ])?,
            ],
        )?);
    }
    body.push(call(c, "quote", &[bare])?);
    let mut expansion = vec![c.vector(&[
        local,
        c.list(&[
            c.native_function(&INSTALL),
            namespace,
            call(c, "quote", &[c.list(&args[2..])?])?,
        ])?,
    ])?];
    expansion.extend(body);
    call(c, "let*", &expansion)
}
fn implementation_error(protocol: &Name, method: &Name, message: &'static str) -> NativeError {
    InvokeError::Dispatch(EvalErrorKind::InvalidProtocolImplementation {
        protocol: Box::new(protocol.clone()),
        method: Box::new(method.clone()),
        message,
    })
    .into()
}

/// Shared protocol-clause lowerer. `lower_method` receives one validated
/// [parameters] body... overload and can introduce record field bindings before
/// returning its fn overload form. Names are resolved once to descriptor IDs;
/// emitted functions retain ordinary lexical capture and recur semantics.
pub(super) fn implementation_forms(
    c: &mut Context,
    clauses: &[RootedValue],
    mut lower_method: impl FnMut(&mut Context, &[RootedValue]) -> NativeResult,
) -> Result<Vec<RootedValue>, NativeError> {
    c.charge_allocation(clauses.len() as u64 + 1)?;
    let mut groups = Vec::new();
    let mut seen_protocols = std::collections::HashSet::new();
    let mut tail = clauses;
    while let Some(symbol) = tail.first() {
        c.accounting.work(1)?;
        let ValueView::Symbol(name) = symbol.inspect() else {
            return Err(invalid("implementation requires a protocol symbol"));
        };
        let var = c
            .resolve_var(&name)
            .map_err(|e| InvokeError::Dispatch(EvalErrorKind::Namespace(e)))?
            .ok_or_else(|| InvokeError::Dispatch(EvalErrorKind::UnresolvedSymbol(name.clone())))?;
        let protocol = c
            .var_root(var.var_id().unwrap())
            .map_err(|_| NativeError::WrongOwner)?;
        let id = protocol_id(&protocol)
            .ok_or_else(|| invalid("implementation requires a user protocol"))?;
        if !seen_protocols.insert(id) {
            let name = descriptor(&c.owner.store.borrow(), id).name.clone();
            return Err(implementation_error(
                &name,
                &name,
                "duplicate protocol implementation",
            ));
        }
        tail = &tail[1..];
        let mut methods: Vec<(usize, Vec<RootedValue>, Vec<usize>)> = Vec::new();
        while let Some(form) = tail.first() {
            if matches!(form.inspect(), ValueView::Symbol(_)) {
                break;
            }
            c.accounting.work(1)?;
            if !matches!(form.inspect(), ValueView::List(_) | ValueView::Seq(_)) {
                return Err(invalid("protocol implementation requires method lists"));
            }
            let parts = form.elements()?;
            c.charge_allocation(parts.len() as u64 + 1)?;
            let Some(ValueView::Symbol(method_name)) = parts.first().map(RootedValue::inspect)
            else {
                return Err(invalid("protocol implementation requires a method symbol"));
            };
            let (index, protocol_name, qualified_method, arities) = {
                let store = c.owner.store.borrow();
                let p = descriptor(&store, id);
                c.accounting.work(p.methods.len() as u64 + 1)?;
                let qualified = Name::from_utf16(p.name.namespace(), method_name.name());
                let index = p
                    .methods
                    .iter()
                    .position(|m| m.name == qualified)
                    .filter(|_| method_name.namespace().is_none());
                let index = index.ok_or_else(|| {
                    implementation_error(&p.name, &qualified, "unknown protocol method")
                })?;
                c.charge_allocation(p.methods[index].arities.len() as u64)?;
                (
                    index,
                    p.name.clone(),
                    qualified,
                    p.methods[index].arities.clone(),
                )
            };
            let invalid_method =
                |message| implementation_error(&protocol_name, &qualified_method, message);
            let Some(parameters) = parts
                .get(1)
                .filter(|v| matches!(v.inspect(), ValueView::Vector(_)))
            else {
                return Err(invalid_method("method parameters must be a vector"));
            };
            let parameters = parameters.elements()?;
            c.charge_allocation(parameters.len() as u64 + 1)?;
            c.accounting
                .work(parameters.len() as u64 + arities.len() as u64)?;
            if parameters.iter().any(
                |v| matches!(v.inspect(), ValueView::Symbol(ref n) if n == &Name::new(None, "&")),
            ) {
                return Err(invalid_method(
                    "variadic protocol implementations are unsupported",
                ));
            }
            if !arities.contains(&parameters.len()) {
                return Err(invalid_method("undeclared protocol method arity"));
            }
            c.accounting.work(methods.len() as u64 + 1)?;
            let pos = if let Some(pos) = methods.iter().position(|(i, _, _)| *i == index) {
                pos
            } else {
                methods.push((index, Vec::new(), Vec::new()));
                methods.len() - 1
            };
            let (_, overloads, implemented) = &mut methods[pos];
            c.accounting.work(implemented.len() as u64 + 1)?;
            if implemented.contains(&parameters.len()) {
                return Err(invalid_method("duplicate protocol method arity"));
            }
            implemented.push(parameters.len());
            overloads.push(lower_method(c, &parts[1..])?);
            tail = &tail[1..];
        }
        let mut group = vec![protocol];
        for (index, overloads, _) in methods {
            group.push(c.integer(index as i64));
            group.push(call(c, "fn", &overloads)?);
        }
        groups.push(c.vector(&group)?);
    }
    Ok(groups)
}

pub(super) fn implementation_table(
    c: &Context,
    groups: &[RootedValue],
) -> Result<Vec<Implementation>, NativeError> {
    c.charge_allocation(groups.len() as u64 + 1)?;
    let mut table = Vec::with_capacity(groups.len());
    let mut seen = std::collections::HashSet::new();
    for group in groups {
        c.accounting.work(1)?;
        let parts = group.elements()?;
        c.charge_allocation(parts.len() as u64 + 1)?;
        let Some(protocol) = parts.first().and_then(protocol_id) else {
            return Err(invalid("implementation table requires a protocol"));
        };
        c.check_name(&descriptor(&c.owner.store.borrow(), protocol).name, false)
            .map_err(InvokeError::Dispatch)?;
        if parts.len() % 2 != 1 || !seen.insert(protocol) {
            return Err(invalid("invalid protocol implementation table"));
        }
        let mut methods = Vec::new();
        for pair in parts[1..].as_chunks::<2>().0 {
            c.accounting.work(1)?;
            let ValueView::Integer(index) = pair[0].inspect() else {
                return Err(invalid("invalid protocol method index"));
            };
            let index =
                usize::try_from(index).map_err(|_| invalid("invalid protocol method index"))?;
            let store = c.owner.store.borrow();
            let p = descriptor(&store, protocol);
            let method = p
                .methods
                .get(index)
                .ok_or_else(|| invalid("invalid protocol method index"))?;
            c.accounting.work(methods.len() as u64 + 1)?;
            if methods.iter().any(|(i, _)| *i == index)
                || !evaluation::protocol_implementation_arities(
                    &store,
                    pair[1].value(),
                    &method.arities,
                )?
            {
                return Err(implementation_error(
                    &p.name,
                    &method.name,
                    "invalid protocol method function",
                ));
            }
            methods.push((index, pair[1].value()));
        }
        table.push(Implementation { protocol, methods });
    }
    Ok(table)
}
fn construct_reify(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let metadata = &args[0];
    if !matches!(metadata.inspect(), ValueView::Map(_) | ValueView::Nil) {
        return Err(invalid("reify metadata must be a map or nil"));
    }
    let table = implementation_table(c, &args[1..])?;
    let value = Value::Heap(HeapId(
        c.owner
            .store
            .borrow_mut()
            .heap
            .insert(HeapValue::Reify(table)),
    ));
    let value = if matches!(metadata.inspect(), ValueView::Nil) {
        value
    } else {
        c.charge_allocation(1)?;
        Value::Heap(HeapId(c.owner.store.borrow_mut().heap.insert(
            HeapValue::Annotated {
                value,
                metadata: metadata.value(),
            },
        )))
    };
    c.accounting.check()?;
    Ok(c.owner.value(value))
}
static CONSTRUCT_REIFY: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "clojure.core",
        name: "reify",
        arity: Arity::AtLeast(1),
        effects: NativeEffects::Context,
        errors: &[NativeErrorClass::Invocation],
    },
    callback: construct_reify,
};
fn reify(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let mut expansion = vec![
        c.native_function(&CONSTRUCT_REIFY),
        call(c, "quote", &[args[0].metadata().unwrap_or(c.nil())])?,
    ];
    expansion.extend(implementation_forms(c, &args[2..], |c, parts| {
        c.list(parts).map_err(Into::into)
    })?);
    c.list(&expansion).map_err(Into::into)
}

fn extension_target(c: &Context, var: &RootedValue) -> Result<RootedValue, NativeError> {
    let id = var
        .var_id()
        .ok_or_else(|| invalid("extension target must be a named record type Var"))?;
    c.check_var_write(id)
        .map_err(|e| InvokeError::Dispatch(e.kind()))?;
    let value = c.var_root(id).map_err(|_| NativeError::WrongOwner)?;
    let ValueView::RecordType(name) = value.inspect() else {
        return Err(invalid(
            "extension supports only named record types; nil, built-in selectors, host classes and reify are unsupported",
        ));
    };
    c.check_name(&name, false).map_err(InvokeError::Dispatch)?;
    Ok(value)
}

fn extend_table(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let target = extension_target(c, &args[0])?;
    let id = records::type_id(&target).unwrap();
    let updates = implementation_table(c, &args[1..])?;
    // Target and evaluated group arguments root both tables while preparing a
    // complete replacement. Every fallible check precedes publication.
    let mut table = {
        let store = c.owner.store.borrow();
        let old = &records::descriptor(&store, id).implementations;
        c.charge_allocation(old.len() as u64 + updates.len() as u64 + 1)?;
        for group in old {
            c.accounting.work(group.methods.len() as u64 + 1)?;
            c.charge_allocation(group.methods.len() as u64)?;
        }
        old.clone()
    };
    for update in updates {
        c.accounting.work(table.len() as u64 + 1)?;
        if let Some(group) = table.iter_mut().find(|g| g.protocol == update.protocol) {
            c.charge_allocation(update.methods.len() as u64)?;
            for (index, function) in update.methods {
                c.accounting.work(group.methods.len() as u64 + 1)?;
                if let Some((_, old)) = group.methods.iter_mut().find(|(i, _)| *i == index) {
                    *old = function;
                } else {
                    group.methods.push((index, function));
                }
            }
        } else {
            table.push(update);
        }
    }
    // Root the return value before changing dispatch, too: creating a root is
    // accounted and must not turn an allocation failure into a partial update.
    let result = c.nil();
    c.accounting.check()?;
    records::replace_implementations(&mut c.owner.store.borrow_mut(), id, table);
    Ok(result)
}
static EXTEND_TYPE: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "clojure.core",
        name: "extend-type",
        arity: Arity::AtLeast(1),
        effects: NativeEffects::Context,
        errors: &[NativeErrorClass::Invocation],
    },
    callback: extend_table,
};
static EXTEND_PROTOCOL: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        name: "extend-protocol",
        ..EXTEND_TYPE.metadata
    },
    callback: extend_table,
};

// External clauses use fn-style grouped overloads. Normalize only their shape
// and let the shared lowerer validate protocol/method/arity identities.
fn external_clauses(c: &Context, clauses: &[RootedValue]) -> Result<Vec<RootedValue>, NativeError> {
    c.charge_allocation(clauses.len() as u64 + 1)?;
    let mut normalized = Vec::new();
    for clause in clauses {
        c.accounting.work(1)?;
        if matches!(clause.inspect(), ValueView::Symbol(_)) {
            normalized.push(clause.clone());
            continue;
        }
        if !matches!(clause.inspect(), ValueView::List(_) | ValueView::Seq(_)) {
            return Err(invalid("protocol implementation requires method lists"));
        }
        let parts = clause.elements()?;
        c.charge_allocation(parts.len() as u64 + 1)?;
        if matches!(
            parts.get(1).map(RootedValue::inspect),
            Some(ValueView::Vector(_))
        ) {
            normalized.push(clause.clone());
            continue;
        }
        if parts.len() < 2 {
            return Err(invalid(
                "protocol implementation requires method parameters",
            ));
        }
        for overload in &parts[1..] {
            c.accounting.work(1)?;
            if !matches!(overload.inspect(), ValueView::List(_) | ValueView::Seq(_)) {
                return Err(invalid("protocol method overload must be a list"));
            }
            let mut method = vec![parts[0].clone()];
            method.extend(overload.elements()?);
            normalized.push(c.list(&method)?);
        }
    }
    Ok(normalized)
}
fn extension_form(
    c: &mut Context,
    selector: &RootedValue,
    clauses: &[RootedValue],
    primitive: &'static NativeDefinition,
) -> NativeResult {
    let ValueView::Symbol(name) = selector.inspect() else {
        return Err(invalid(
            "extension supports only named record type selectors",
        ));
    };
    let target = c.resolve_var(&name)
        .map_err(|e| InvokeError::Dispatch(EvalErrorKind::Namespace(e)))?
        .ok_or_else(|| invalid("extension selector must resolve to a named record type Var; built-in and host selectors are unsupported"))?;
    extension_target(c, &target)?;
    let clauses = external_clauses(c, clauses)?;
    let mut form = vec![c.native_function(primitive), target];
    form.extend(implementation_forms(c, &clauses, |c, parts| {
        c.list(parts).map_err(Into::into)
    })?);
    c.list(&form).map_err(Into::into)
}
fn extend_type(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    extension_form(c, &args[2], &args[3..], &EXTEND_TYPE)
}
fn extend_protocol(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    // Validate even an empty extension's protocol selector through the same path.
    implementation_forms(c, &args[2..3], |c, parts| c.list(parts).map_err(Into::into))?;
    let mut forms = Vec::new();
    let mut tail = &args[3..];
    while let Some(selector) = tail.first() {
        c.accounting.work(1)?;
        let mut end = 1;
        while end < tail.len() {
            c.accounting.work(1)?;
            if !matches!(tail[end].inspect(), ValueView::List(_) | ValueView::Seq(_)) {
                break;
            }
            end += 1;
        }
        c.charge_allocation(end as u64)?;
        let mut clauses = vec![args[2].clone()];
        clauses.extend_from_slice(&tail[1..end]);
        forms.push(extension_form(c, selector, &clauses, &EXTEND_PROTOCOL)?);
        tail = &tail[end..];
    }
    forms.push(c.nil());
    call(c, "do", &forms)
}

fn satisfies(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let id = protocol_id(&args[0]).ok_or(NativeError::Type {
        argument: 0,
        expected: "protocol",
    })?;
    let store = c.owner.store.borrow();
    let table = implementations(&store, args[1].value());
    c.accounting.work(table.len() as u64 + 1)?;
    Ok(c.boolean(
        table
            .iter()
            .any(|implementation| implementation.protocol == id),
    ))
}
pub fn protocol_definitions() -> &'static [NativeDefinition] {
    static DEFINITIONS: &[NativeDefinition] = &[
        NativeDefinition {
            metadata: NativeMetadata {
                arity: Arity::AtLeast(3),
                ..EXTEND_TYPE.metadata
            },
            callback: extend_type,
        },
        NativeDefinition {
            metadata: NativeMetadata {
                arity: Arity::AtLeast(3),
                ..EXTEND_PROTOCOL.metadata
            },
            callback: extend_protocol,
        },
        NativeDefinition {
            metadata: NativeMetadata {
                namespace: "clojure.core",
                name: "reify",
                arity: Arity::AtLeast(2),
                effects: NativeEffects::Context,
                errors: &[NativeErrorClass::Invocation],
            },
            callback: reify,
        },
        NativeDefinition {
            metadata: NativeMetadata {
                namespace: "clojure.core",
                name: "defprotocol",
                arity: Arity::AtLeast(3),
                effects: NativeEffects::Context,
                errors: &[NativeErrorClass::Invocation],
            },
            callback: defprotocol,
        },
        NativeDefinition {
            metadata: NativeMetadata {
                namespace: "clojure.core",
                name: "satisfies?",
                arity: Arity::Exact(2),
                effects: NativeEffects::Context,
                errors: &[NativeErrorClass::Type],
            },
            callback: satisfies,
        },
    ];
    DEFINITIONS
}
fn receiver_category(value: &RootedValue) -> &'static str {
    match value.inspect() {
        ValueView::Nil => "nil",
        ValueView::Boolean(_) => "boolean",
        ValueView::Integer(_) | ValueView::Double(_) | ValueView::Ratio { .. } => "number",
        ValueView::Character(_) => "character",
        ValueView::String(_) => "string",
        ValueView::Symbol(_) => "symbol",
        ValueView::Keyword(_) => "keyword",
        ValueView::Vector(_) => "vector",
        ValueView::Map(_) => "map",
        ValueView::Set(_) => "set",
        ValueView::List(_) | ValueView::Seq(_) | ValueView::LazySeq => "sequence",
        ValueView::Function | ValueView::NativeFunction(_) => "function",
        ValueView::Protocol(_) => "protocol",
        ValueView::Reify => "reify",
        ValueView::Record(_) => "record",
        ValueView::RecordType(_) => "record type",
        ValueView::Var(_) => "Var",
        ValueView::Namespace(_) => "namespace",
        ValueView::Unbound(_) => "unbound",
        ValueView::Atom => "atom",
        ValueView::Volatile => "volatile",
        ValueView::Delay => "delay",
        ValueView::Reduced => "reduced",
        ValueView::Input => "input",
        ValueView::Output => "output",
        ValueView::CompilerLiteral(_) => "compiler-reader-value",
        ValueView::Exception(_) => "exception",
    }
}
pub(super) fn invoke_method(
    c: &mut Context,
    callee: &RootedValue,
    arguments: &[RootedValue],
    applied: Option<&RootedValue>,
) -> Option<Result<RootedValue, InvokeError>> {
    let method = match c.owner.store.borrow().heap_value(callee.value()) {
        Some(HeapValue::ProtocolMethod(method)) => *method,
        _ => return None,
    };
    Some((|| {
        c.poll_execution(PollPoint::Function)
            .map_err(InvokeError::Dispatch)?;
        let (name, protocol_name, arities) = {
            let store = c.owner.store.borrow();
            let p = descriptor(&store, method.protocol);
            let m = &p.methods[method.method];
            c.charge_allocation(m.arities.len() as u64)
                .map_err(|e| InvokeError::Dispatch(c.accounting.kind(e)))?;
            (m.name.clone(), p.name.clone(), m.arities.clone())
        };
        c.check_name(&name, false).map_err(InvokeError::Dispatch)?;
        let mut forced = Vec::new();
        let arguments = if let Some(applied) = applied {
            // Realize at most max arity + 1, including infinite argument sources.
            let maximum = arities.iter().copied().max().unwrap_or(0);
            let mut cursor =
                collection_core::Cursor::new(c, applied, 0).map_err(deferred::invoke_error)?;
            for _ in 0..=maximum {
                c.poll_execution(PollPoint::Iteration)
                    .map_err(InvokeError::Dispatch)?;
                let Some(value) = cursor.next(c).map_err(deferred::invoke_error)? else {
                    break;
                };
                c.charge_allocation(1)
                    .map_err(|e| InvokeError::Dispatch(c.accounting.kind(e)))?;
                forced.push(value);
            }
            &forced
        } else {
            arguments
        };
        let actual = applied
            .and_then(RootedValue::count)
            .unwrap_or(arguments.len());
        if !arities.contains(&actual) {
            return Err(InvokeError::Dispatch(EvalErrorKind::Arity {
                fixed: arities,
                variadic: None,
                actual,
            }));
        }
        if c.active_calls >= c.accounting.depths().0 {
            return Err(InvokeError::Dispatch(
                c.accounting
                    .kind(c.accounting.fail(ResourceError::CallDepthExceeded)),
            ));
        }
        let selected = {
            let store = c.owner.store.borrow();
            let table = implementations(&store, arguments[0].value());
            c.accounting
                .work(table.len() as u64 + 1)
                .map_err(|e| InvokeError::Dispatch(c.accounting.kind(e)))?;
            let implementation = table.iter().find(|i| i.protocol == method.protocol);
            if let Some(i) = implementation {
                c.accounting
                    .work(i.methods.len() as u64)
                    .map_err(|e| InvokeError::Dispatch(c.accounting.kind(e)))?;
            }
            implementation.and_then(|i| {
                i.methods
                    .iter()
                    .find(|(index, _)| *index == method.method)
                    .map(|(_, f)| c.owner.value(*f))
            })
        };
        let selected = selected.ok_or_else(|| {
            InvokeError::Dispatch(EvalErrorKind::MissingProtocolImplementation {
                protocol: Box::new(protocol_name),
                method: Box::new(name),
                receiver: receiver_category(&arguments[0]),
            })
        })?;
        // Selected function and receiver remain rooted across arbitrary callbacks.
        c.active_calls += 1;
        let result = c.invoke(&selected, arguments);
        c.active_calls -= 1;
        result
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(
        c: &mut Context,
        protocol: &RootedValue,
        methods: &[(&str, RootedValue)],
    ) -> RootedValue {
        let id = protocol_id(protocol).unwrap();
        let mut store = c.owner.store.borrow_mut();
        let table = methods
            .iter()
            .map(|(name, f)| {
                let index = descriptor(&store, id)
                    .methods
                    .iter()
                    .position(|m| m.name.name() == Name::new(None, name).name())
                    .unwrap();
                (index, f.value())
            })
            .collect();
        let raw = Value::Heap(HeapId(store.heap.insert(HeapValue::ProtocolFixture(vec![
            Implementation {
                protocol: id,
                methods: table,
            },
        ]))));
        drop(store);
        c.owner.value(raw)
    }
    fn install_value(c: &mut Context, name: &str, value: &RootedValue) {
        let var = c
            .intern_var(c.current_namespace_id(), Name::new(None, name))
            .unwrap();
        c.set_var_root(var.var_id().unwrap(), value).unwrap();
    }
    fn number(value: RootedValue) -> i64 {
        let ValueView::Integer(n) = value.inspect() else {
            panic!("integer")
        };
        n
    }
    #[test]
    fn private_receivers_dispatch_through_direct_saved_apply_var_and_closure_calls() {
        let mut c = Context::new();
        c.eval_string("(defprotocol P (f [x] [x y]) (missing [x])) (def saved f)")
            .unwrap();
        let protocol = c.eval_string("P").unwrap();
        let implementation = c
            .eval_string("(let [captured 40] (fn ([x] (+ captured 2)) ([x y] (+ captured y))))")
            .unwrap();
        let receiver = fixture(&mut c, &protocol, &[("f", implementation.clone())]);
        install_value(&mut c, "receiver", &receiver);
        for expression in [
            "(f receiver)",
            "(saved receiver)",
            "(apply f [receiver])",
            "((var f) receiver)",
            "((fn [f x] (f x)) saved receiver)",
            "(f receiver 2)",
            "(apply f [receiver 2])",
        ] {
            assert_eq!(
                number(c.eval_string(expression).unwrap()),
                42,
                "{expression}"
            );
        }
        assert_eq!(
            c.eval_string("(satisfies? P receiver)").unwrap().inspect(),
            ValueView::Boolean(true)
        );
        assert!(matches!(
            c.eval_string("(missing receiver)").unwrap_err().kind,
            EvalErrorKind::MissingProtocolImplementation { .. }
        ));
        c.eval_string("(defprotocol P (missing [r]) (f [r y] [r]))")
            .unwrap();
        assert_eq!(number(c.eval_string("(saved receiver 2)").unwrap()), 42);
        assert_eq!(
            c.eval_string("(identical? saved f)").unwrap().inspect(),
            ValueView::Boolean(true)
        );
        c.eval_string("(def f (fn [_] 99))").unwrap();
        assert_eq!(number(c.eval_string("(f receiver)").unwrap()), 99);
        assert_eq!(number(c.eval_string("(saved receiver)").unwrap()), 42);
        // Marker and partial declarations count nominally, with no fake language constructor.
        c.eval_string("(defprotocol Marker)").unwrap();
        let marker = c.eval_string("Marker").unwrap();
        let empty = fixture(&mut c, &marker, &[]);
        assert_eq!(
            satisfies(&mut c, &[marker, empty]).unwrap().inspect(),
            ValueView::Boolean(true)
        );
    }
    #[test]
    fn selected_implementation_survives_reentrant_replacement_and_collection() {
        static REPLACE: NativeDefinition = NativeDefinition {
            metadata: NativeMetadata {
                namespace: "host",
                name: "replace",
                arity: Arity::Exact(1),
                effects: NativeEffects::Context,
                errors: &[],
            },
            callback: |c, args| {
                let next = c.eval_string("(fn [_] 99)").unwrap();
                let mut store = c.owner.store.borrow_mut();
                let HeapValue::ProtocolFixture(table) =
                    store.heap.get_mut(args[0].heap_id().unwrap().0)
                else {
                    unreachable!()
                };
                table[0].methods[0].1 = next.value();
                store.protocol_revision += 1;
                drop(store);
                c.collect_garbage();
                Ok(c.nil())
            },
        };
        let mut c = Context::new();
        c.register_native(&REPLACE);
        c.eval_string("(defprotocol P (f [x]))").unwrap();
        let p = c.eval_string("P").unwrap();
        let implementation = c.eval_string("(fn [x] (host/replace x) 7)").unwrap();
        let receiver = fixture(&mut c, &p, &[("f", implementation.clone())]);
        let method = c.eval_string("f").unwrap();
        drop(implementation);
        assert_eq!(
            number(c.invoke(&method, std::slice::from_ref(&receiver)).unwrap()),
            7
        );
        assert_eq!(number(c.invoke(&method, &[receiver]).unwrap()), 99);
    }
    #[test]
    fn retained_methods_descriptors_and_receivers_trace_cycles_without_context_roots() {
        let mut c = Context::new();
        c.eval_string("(defprotocol P (f [x]))").unwrap();
        let protocol = c.eval_string("P").unwrap();
        let method = c.eval_string("f").unwrap();
        let implementation = c.eval_string("(let [n [42]] (fn [_] (first n)))").unwrap();
        let receiver = fixture(&mut c, &protocol, &[("f", implementation.clone())]);
        let mut fork = c.fork().unwrap();
        // Drop source context, code/closure handles, then remove mappings in the survivor.
        drop((c, implementation));
        let ns = fork.current_namespace_id();
        for name in ["P", "f"] {
            fork.unmap_var(ns, &Name::new(None, name)).unwrap();
        }
        fork.clear_analysis_reuse();
        fork.collect_garbage();
        assert_eq!(
            number(
                fork.invoke(&method, std::slice::from_ref(&receiver))
                    .unwrap()
            ),
            42
        );
        assert_eq!(
            satisfies(&mut fork, &[protocol.clone(), receiver.clone()])
                .unwrap()
                .inspect(),
            ValueView::Boolean(true)
        );
        let mut foreign = Context::new();
        assert!(matches!(
            foreign.invoke(&method, std::slice::from_ref(&receiver)),
            Err(InvokeError::Dispatch(EvalErrorKind::WrongOwner))
        ));
        let before = fork.memory_stats().heap_values;
        drop((protocol, method, receiver));
        let after = fork.collect_garbage().after;
        assert!(after.heap_values < before);
        assert_eq!((after.executable_units, after.sources), (0, 0));
    }
    #[test]
    fn dispatch_recursion_and_lazy_apply_respect_controls() {
        let mut c = Context::new();
        c.eval_string("(defprotocol P (f [x]))").unwrap();
        let p = c.eval_string("P").unwrap();
        let method = c.eval_string("f").unwrap();
        let receiver = fixture(&mut c, &p, &[("f", method.clone())]);
        c.set_resource_limits(ResourceLimits {
            call_depth: 8,
            ..Default::default()
        });
        assert_eq!(
            c.invoke(&method, &[receiver]).unwrap_err().class(),
            ErrorClass::Control
        );
        let token = CancellationToken::default();
        c.set_cancellation_token(Some(token.clone()));
        token.cancel();
        assert_eq!(
            c.invoke(&method, &[c.nil()]).unwrap_err().class(),
            ErrorClass::Control
        );
    }
    #[test]
    #[ignore = "manual release-mode protocol/native/closure timing; no performance threshold"]
    fn protocol_dispatch_measurement() {
        use std::{hint::black_box, time::Instant};
        let mut c = Context::new();
        c.eval_string("(defprotocol P (f [x]))").unwrap();
        let protocol = c.eval_string("P").unwrap();
        let method = c.eval_string("f").unwrap();
        let closure = c.eval_string("(fn [x] x)").unwrap();
        let native = c.eval_string("identity").unwrap();
        let receiver = fixture(&mut c, &protocol, &[("f", closure.clone())]);
        for (label, function) in [
            ("native", native),
            ("closure", closure),
            ("protocol", method),
        ] {
            let mut samples = Vec::new();
            for _ in 0..7 {
                let start = Instant::now();
                for _ in 0..2000 {
                    let result = c
                        .invoke(black_box(&function), std::slice::from_ref(&receiver))
                        .unwrap();
                    assert!(result.same_identity(&receiver));
                }
                samples.push(start.elapsed().as_nanos() / 2000);
                c.collect_garbage();
            }
            samples.sort_unstable();
            println!(
                "{label} median_ns_per_call={} range={}..{}",
                samples[3], samples[0], samples[6]
            );
        }
        for (label, source) in [
            ("defn", "(defn ordinary [x] x)"),
            ("protocol_registration", "(defprotocol P (f [x]))"),
        ] {
            let mut samples = Vec::new();
            for _ in 0..7 {
                let start = Instant::now();
                for _ in 0..100 {
                    black_box(c.eval_string(black_box(source)).unwrap());
                }
                samples.push(start.elapsed().as_nanos() / 100);
                c.collect_garbage();
            }
            samples.sort_unstable();
            println!(
                "{label} median_ns_per_registration={} range={}..{}",
                samples[3], samples[0], samples[6]
            );
        }
    }
}
