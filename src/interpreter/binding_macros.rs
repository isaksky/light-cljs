//! Ordinary binding macros lower owned data to the existing indexed primitives.
//! Semantics follow pinned SCI's destructure/fns; no printed/reparsed expansion.
use super::*;
use crate::parser::CollectionKind;

pub(super) fn invalid(message: &'static str) -> NativeError {
    InvokeError::Dispatch(EvalErrorKind::InvalidForm(message)).into()
}
fn symbol(c: &Context, name: &str) -> RootedValue {
    c.symbol(Name::new(None, name))
}
fn keyword(c: &Context, name: &str) -> RootedValue {
    c.keyword(Name::new(None, name))
}
fn named(v: &RootedValue, name: &str) -> bool {
    matches!(v.inspect(), ValueView::Symbol(n) if n == Name::new(None, name))
}
fn directive(v: &RootedValue, name: &str) -> bool {
    matches!(v.inspect(), ValueView::Keyword(n) if n == Name::new(None, name))
}
pub(super) fn call(c: &Context, name: &str, args: &[RootedValue]) -> NativeResult {
    let mut forms = vec![if [
        "let*", "loop*", "fn*", "letfn*", "if", "do", "def", "quote", "throw", "case*", "recur",
    ]
    .contains(&name)
    {
        symbol(c, name)
    } else {
        c.symbol(Name::new(Some("clojure.core"), name))
    }];
    forms.extend_from_slice(args);
    Ok(c.list(&forms)?)
}
pub(super) fn fresh(c: &Context) -> RootedValue {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .expect("binding gensym identities exhausted");
    symbol(c, &format!("binding__{id}__auto__"))
}
fn local(c: &Context, value: &RootedValue) -> NativeResult {
    let name = match value.inspect() {
        ValueView::Symbol(n) | ValueView::Keyword(n) => n,
        _ => return Err(invalid("binding must be a symbol")),
    };
    let result = c.symbol(Name::from_utf16(None, name.name()));
    Ok(if let Some(meta) = value.metadata() {
        c.with_meta(&result, &meta)?
    } else {
        result
    })
}

struct Expander<'a> {
    c: &'a Context,
    maps: HashMap<HeapId, Form>,
}
impl<'a> Expander<'a> {
    fn new(c: &'a Context) -> Result<Self, NativeError> {
        let mut maps = HashMap::new();
        if let Some(form) = c.macros.sources.last() {
            let mut pending = vec![form.id()];
            while let Some(id) = pending.pop() {
                c.accounting.work(1)?;
                let child = form.at(id).unwrap();
                if let FormKind::Collection { kind, children } = &child.node().kind {
                    c.charge_allocation(children.len() as u64 + 1)?;
                    pending.extend(children.iter().copied());
                    if *kind == CollectionKind::Map {
                        maps.insert(child.data().heap_id().unwrap(), child.clone());
                    }
                }
                pending.extend(child.node().metadata);
            }
        }
        Ok(Self { c, maps })
    }
    fn entries(&self, map: &RootedValue) -> Result<Vec<(RootedValue, RootedValue)>, NativeError> {
        if let Some(form) = map.heap_id().and_then(|id| self.maps.get(&id)) {
            let FormKind::Collection { children, .. } = &form.node().kind else {
                unreachable!()
            };
            self.c.charge_allocation(children.len() as u64)?;
            Ok(children
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| (form.at(p[0]).unwrap().data(), form.at(p[1]).unwrap().data()))
                .collect())
        } else {
            Ok(map.entries()?)
        }
    }
    fn bindings(&self, bindings: &RootedValue) -> Result<Vec<RootedValue>, NativeError> {
        if !matches!(bindings.inspect(), ValueView::Vector(_)) {
            return Err(invalid("binding declaration must be a vector"));
        }
        let pairs = bindings.elements()?;
        if !pairs.len().is_multiple_of(2) {
            return Err(invalid("binding vector requires binding/initializer pairs"));
        }
        let mut result = Vec::new();
        // SCI preserves a symbol-only declaration for primitive validation.
        let symbols = pairs
            .iter()
            .step_by(2)
            .all(|v| matches!(v.inspect(), ValueView::Symbol(_)));
        for pair in pairs.as_chunks::<2>().0 {
            if symbols {
                result.extend_from_slice(pair);
            } else {
                if matches!(pair[0].inspect(), ValueView::Keyword(_)) {
                    return Err(invalid("unsupported binding key"));
                }
                self.bind(&mut result, &pair[0], pair[1].clone(), 0)?;
            }
        }
        Ok(result)
    }
    fn bind(
        &self,
        out: &mut Vec<RootedValue>,
        pattern: &RootedValue,
        value: RootedValue,
        depth: usize,
    ) -> Result<(), NativeError> {
        self.c.accounting.work(1)?;
        self.c.charge_allocation(2)?;
        if depth >= self.c.accounting.depths().1 {
            return Err(InvokeError::Dispatch(
                self.c.accounting.kind(
                    self.c
                        .accounting
                        .fail(ResourceError::EvaluationDepthExceeded),
                ),
            )
            .into());
        }
        match pattern.inspect() {
            ValueView::Symbol(_) | ValueView::Keyword(_) => {
                out.extend([local(self.c, pattern)?, value])
            }
            ValueView::Vector(_) => self.vector(out, pattern, value, depth + 1)?,
            ValueView::Map(_) => self.map(out, pattern, value, depth + 1)?,
            _ => return Err(invalid("unsupported binding form")),
        }
        Ok(())
    }
    fn vector(
        &self,
        out: &mut Vec<RootedValue>,
        pattern: &RootedValue,
        value: RootedValue,
        depth: usize,
    ) -> Result<(), NativeError> {
        let c = self.c;
        let parts = pattern.elements()?;
        let whole = fresh(c);
        out.extend([whole.clone(), value]);
        let has_rest = parts.iter().any(|v| named(v, "&"));
        let seq = fresh(c);
        if has_rest {
            out.extend([seq.clone(), call(c, "seq", std::slice::from_ref(&whole))?]);
        }
        let mut index = 0;
        let mut rest = false;
        while index < parts.len() {
            let part = &parts[index];
            if named(part, "&") {
                if rest || index + 1 == parts.len() {
                    return Err(invalid("& requires one rest binding"));
                }
                self.bind(out, &parts[index + 1], seq.clone(), depth)?;
                rest = true;
                index += 2;
            } else if directive(part, "as") {
                if index + 2 != parts.len() {
                    return Err(invalid(":as requires one final binding"));
                }
                self.bind(out, &parts[index + 1], whole.clone(), depth)?;
                break;
            } else {
                if rest {
                    return Err(invalid("only :as may follow a rest binding"));
                }
                let item = if has_rest {
                    let first = fresh(c);
                    out.extend([
                        first.clone(),
                        call(c, "first", std::slice::from_ref(&seq))?,
                        seq.clone(),
                        call(c, "next", std::slice::from_ref(&seq))?,
                    ]);
                    first
                } else {
                    call(c, "nth", &[whole.clone(), c.integer(index as i64), c.nil()])?
                };
                self.bind(out, part, item, depth)?;
                index += 1;
            }
        }
        Ok(())
    }
    fn map(
        &self,
        out: &mut Vec<RootedValue>,
        pattern: &RootedValue,
        value: RootedValue,
        depth: usize,
    ) -> Result<(), NativeError> {
        let c = self.c;
        let whole = fresh(c);
        out.extend([
            whole.clone(),
            value,
            whole.clone(),
            call(
                c,
                "if",
                &[
                    call(c, "seq?", std::slice::from_ref(&whole))?,
                    call(
                        c,
                        "seq-to-map-for-destructuring",
                        std::slice::from_ref(&whole),
                    )?,
                    whole.clone(),
                ],
            )?,
        ]);
        let defaults = pattern
            .get(&keyword(c, "or"))?
            .filter(|v| !matches!(v.inspect(), ValueView::Nil))
            .unwrap_or(c.map(&[])?);
        if !matches!(defaults.inspect(), ValueView::Map(_) | ValueView::Nil) {
            return Err(invalid(":or defaults must be a map"));
        }
        if let Some(binding) = pattern
            .get(&keyword(c, "as"))?
            .filter(collection_core::truthy)
        {
            if !matches!(binding.inspect(), ValueView::Symbol(n) if n.namespace().is_none()) {
                return Err(invalid("map :as requires an unqualified symbol"));
            }
            out.extend([binding, whole.clone()]);
        }
        for (binding, key) in self.entries(pattern)? {
            if directive(&binding, "or") || directive(&binding, "as") {
                continue;
            }
            if let ValueView::Keyword(dir) = binding.inspect() {
                let which = String::from_utf16(dir.name()).unwrap_or_default();
                if !["keys", "strs", "syms"].contains(&which.as_str()) {
                    return Err(invalid("unsupported map destructuring directive"));
                }
                if !matches!(key.inspect(), ValueView::Vector(_)) {
                    return Err(invalid("map directive requires a vector"));
                }
                for binding in key.elements()? {
                    let name = match binding.inspect() {
                        ValueView::Symbol(n) | ValueView::Keyword(n) => n,
                        _ => return Err(invalid("map directive requires named bindings")),
                    };
                    if named(&binding, "&") {
                        return Err(invalid("extended map directives are not supported"));
                    }
                    let key = match which.as_str() {
                        "keys" => c.keyword(Name::from_utf16(
                            dir.namespace().or(name.namespace()),
                            name.name(),
                        )),
                        "syms" => call(
                            c,
                            "quote",
                            &[c.symbol(Name::from_utf16(
                                dir.namespace().or(name.namespace()),
                                name.name(),
                            ))],
                        )?,
                        _ => {
                            let mut text = Vec::new();
                            if matches!(binding.inspect(), ValueView::Keyword(_)) {
                                text.push(b':' as u16);
                            }
                            if let Some(ns) = name.namespace() {
                                text.extend_from_slice(ns);
                                text.push(b'/' as u16);
                            }
                            text.extend_from_slice(name.name());
                            c.string(text)
                        }
                    };
                    self.lookup(out, &binding, key, &whole, &defaults, depth)?;
                }
            } else {
                self.lookup(out, &binding, key, &whole, &defaults, depth)?;
            }
        }
        Ok(())
    }
    fn lookup(
        &self,
        out: &mut Vec<RootedValue>,
        binding: &RootedValue,
        key: RootedValue,
        whole: &RootedValue,
        defaults: &RootedValue,
        depth: usize,
    ) -> Result<(), NativeError> {
        let localized = if matches!(
            binding.inspect(),
            ValueView::Symbol(_) | ValueView::Keyword(_)
        ) {
            local(self.c, binding)?
        } else {
            binding.clone()
        };
        let mut args = vec![whole.clone(), key];
        // Ordinary :or is an eager get argument, including for present nil.
        let local_default = defaults.get(&localized)?;
        let key_default = defaults.get(&args[1])?;
        if local_default.is_some() && key_default.is_some() {
            return Err(invalid("multiple :or defaults for the same key"));
        }
        if let Some(default) = local_default.or(key_default) {
            args.push(default);
        }
        self.bind(out, &localized, call(self.c, "get", &args)?, depth)
    }
    fn function(&self, declarations: &[RootedValue]) -> NativeResult {
        let c = self.c;
        let mut start = 0;
        let mut result = Vec::new();
        if let Some(name) = declarations
            .first()
            .filter(|v| matches!(v.inspect(), ValueView::Symbol(_)))
        {
            result.push(name.clone());
            start += 1;
        }
        let declarations = &declarations[start..];
        let Some(first) = declarations.first() else {
            return Err(invalid("function requires a parameter declaration"));
        };
        let methods = if matches!(first.inspect(), ValueView::Vector(_)) {
            vec![declarations.to_vec()]
        } else {
            declarations
                .iter()
                .map(|v| {
                    if !matches!(v.inspect(), ValueView::List(_) | ValueView::Seq(_)) {
                        return Err(invalid("function requires method lists"));
                    }
                    Ok(v.elements()?)
                })
                .collect::<Result<Vec<_>, NativeError>>()?
        };
        for method in methods {
            let Some(params) = method
                .first()
                .filter(|v| matches!(v.inspect(), ValueView::Vector(_)))
            else {
                return Err(invalid("function parameters must be a vector"));
            };
            let mut body = method[1..].to_vec();
            let conditions = if body.len() > 1 && matches!(body[0].inspect(), ValueView::Map(_)) {
                Some(body.remove(0))
            } else {
                params.metadata()
            };
            if let Some(conditions) = conditions {
                if let Some(post) = conditions
                    .get(&keyword(c, "post"))?
                    .filter(collection_core::truthy)
                {
                    let mut forms = vec![c.vector(&[symbol(c, "%"), call(c, "do", &body)?])?];
                    for condition in post.elements()? {
                        forms.push(call(c, "assert", &[condition])?);
                    }
                    forms.push(symbol(c, "%"));
                    body = vec![call(c, "let*", &forms)?];
                }
                if let Some(pre) = conditions
                    .get(&keyword(c, "pre"))?
                    .filter(collection_core::truthy)
                {
                    let mut forms = Vec::new();
                    for condition in pre.elements()? {
                        forms.push(call(c, "assert", &[condition])?);
                    }
                    forms.extend(body);
                    body = forms;
                }
            }
            let mut parameters = Vec::new();
            let mut bindings = Vec::new();
            let parts = params.elements()?;
            let mut rest = false;
            for (i, param) in parts.iter().enumerate() {
                if named(param, "&") {
                    if rest || i + 2 != parts.len() {
                        return Err(invalid("& requires one final function parameter"));
                    }
                    rest = true;
                    parameters.push(param.clone());
                } else if matches!(param.inspect(), ValueView::Symbol(_)) {
                    parameters.push(param.clone());
                } else {
                    if matches!(param.inspect(), ValueView::Keyword(_)) {
                        return Err(invalid("unsupported function binding key"));
                    }
                    let value = fresh(c);
                    parameters.push(value.clone());
                    self.bind(&mut bindings, param, value, 0)?;
                }
            }
            if !bindings.is_empty() {
                let mut args = vec![c.vector(&bindings)?];
                args.extend(body);
                body = vec![call(c, "let*", &args)?];
            }
            let mut method = vec![c.vector(&parameters)?];
            method.extend(body);
            result.push(c.list(&method)?);
        }
        call(c, "fn*", &result)
    }
}

pub(super) fn let_macro(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let bindings = Expander::new(c)?.bindings(&args[2])?;
    let mut forms = vec![c.vector(&bindings)?];
    forms.extend_from_slice(&args[3..]);
    call(c, "let*", &forms)
}
pub(super) fn loop_macro(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let expander = Expander::new(c)?;
    // Validate first, including nested patterns before emitting any execution.
    expander.bindings(&args[2])?;
    let pairs = args[2].elements()?;
    if pairs
        .iter()
        .step_by(2)
        .all(|v| matches!(v.inspect(), ValueView::Symbol(_)))
    {
        return call(c, "loop*", &args[2..]);
    }
    let mut outer = Vec::new();
    let mut targets = Vec::new();
    let mut inner = Vec::new();
    for pair in pairs.as_chunks::<2>().0 {
        let temp = if matches!(pair[0].inspect(), ValueView::Symbol(_)) {
            pair[0].clone()
        } else {
            fresh(c)
        };
        outer.extend([temp.clone(), pair[1].clone()]);
        if !matches!(pair[0].inspect(), ValueView::Symbol(_)) {
            expander.bind(&mut outer, &pair[0], temp.clone(), 0)?;
        }
        targets.extend([temp.clone(), temp.clone()]);
        expander.bind(&mut inner, &pair[0], temp, 0)?;
    }
    let mut body = vec![c.vector(&inner)?];
    body.extend_from_slice(&args[3..]);
    call(
        c,
        "let*",
        &[
            c.vector(&outer)?,
            call(c, "loop*", &[c.vector(&targets)?, call(c, "let*", &body)?])?,
        ],
    )
}
pub(super) fn fn_macro(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    Expander::new(c)?.function(&args[2..])
}
pub(super) fn letfn_macro(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    if !matches!(args[2].inspect(), ValueView::Vector(_)) {
        return Err(invalid("letfn requires a vector"));
    }
    let e = Expander::new(c)?;
    let mut bindings = Vec::new();
    for spec in args[2].elements()? {
        if !matches!(spec.inspect(), ValueView::List(_) | ValueView::Seq(_)) {
            return Err(invalid("letfn requires function declarations"));
        }
        let forms = spec.elements()?;
        let Some(name) = forms
            .first()
            .filter(|v| matches!(v.inspect(), ValueView::Symbol(n) if n.namespace().is_none()))
        else {
            return Err(invalid("letfn requires function names"));
        };
        bindings.extend([name.clone(), e.function(&forms)?]);
    }
    let mut forms = vec![c.vector(&bindings)?];
    forms.extend_from_slice(&args[3..]);
    call(c, "letfn*", &forms)
}
pub(super) fn defn_macro(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    definition(c, args, false)
}
pub(super) fn private_defn(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    definition(c, args, true)
}
fn definition(c: &Context, args: &[RootedValue], private: bool) -> NativeResult {
    let name = &args[2];
    if !matches!(name.inspect(), ValueView::Symbol(_)) {
        return Err(invalid("defn requires a symbol"));
    }
    let mut name_metadata = name.metadata().unwrap_or(c.map(&[])?);
    if private {
        name_metadata = c.assoc(&name_metadata, &keyword(c, "private"), &c.boolean(true))?;
    }
    let mut metadata = c.map(&[])?;
    let mut start = 3;
    if args
        .get(start)
        .is_some_and(|v| matches!(v.inspect(), ValueView::String(_)))
    {
        metadata = c.assoc(&metadata, &keyword(c, "doc"), &args[start])?;
        start += 1;
    }
    if args
        .get(start)
        .is_some_and(|v| matches!(v.inspect(), ValueView::Map(_)))
    {
        for (k, v) in args[start].entries()? {
            metadata = c.assoc(&metadata, &k, &v)?;
        }
        start += 1;
    }
    let mut declarations = &args[start..];
    let multi = !declarations
        .first()
        .is_some_and(|v| matches!(v.inspect(), ValueView::Vector(_)));
    if multi
        && declarations
            .last()
            .is_some_and(|v| matches!(v.inspect(), ValueView::Map(_)))
    {
        for (k, v) in declarations.last().unwrap().entries()? {
            metadata = c.assoc(&metadata, &k, &v)?;
        }
        declarations = &declarations[..declarations.len() - 1];
    }
    let methods = if multi {
        declarations
            .iter()
            .map(RootedValue::elements)
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![declarations.to_vec()]
    };
    let mut arglists = Vec::new();
    for method in methods {
        let Some(params) = method
            .first()
            .filter(|v| matches!(v.inspect(), ValueView::Vector(_)))
        else {
            return Err(invalid("defn requires parameter vectors"));
        };
        let mut params = params.clone();
        if method.len() > 2 && matches!(method[1].inspect(), ValueView::Map(_)) {
            let mut meta = params.metadata().unwrap_or(c.map(&[])?);
            for (k, v) in method[1].entries()? {
                meta = c.assoc(&meta, &k, &v)?;
            }
            params = c.with_meta(&params, &meta)?;
        }
        arglists.push(params);
    }
    // Explicit :arglists metadata overrides the computed signature, as in SCI.
    if metadata.get(&keyword(c, "arglists"))?.is_none() {
        metadata = c.assoc(
            &metadata,
            &keyword(c, "arglists"),
            &call(c, "quote", &[c.eager_sequence(&arglists, true)?])?,
        )?;
    }
    for (k, v) in metadata.entries()? {
        name_metadata = c.assoc(&name_metadata, &k, &v)?;
    }
    call(
        c,
        "def",
        &[
            c.with_meta(name, &name_metadata)?,
            call(c, "fn", declarations)?,
        ],
    )
}
pub(super) fn seq_to_map(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let values = if matches!(args[0].inspect(), ValueView::Nil) {
        Vec::new()
    } else {
        args[0].elements()?
    };
    if values.len() == 1 {
        return Ok(values[0].clone());
    }
    let mut result = c.map(&[])?;
    let (pairs, tail) = values.as_chunks::<2>();
    for pair in pairs {
        c.checkpoint(PollPoint::Iteration)?;
        result = c.assoc(&result, &pair[0], &pair[1])?;
    }
    if let Some(last) = tail
        .first()
        .filter(|v| !matches!(v.inspect(), ValueView::Nil))
    {
        result = collection_core::conj(c, &result, last)?;
    }
    Ok(result)
}
