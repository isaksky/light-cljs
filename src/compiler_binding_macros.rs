//! Pure seed binding transforms extracted from the native bootstrap.
use crate::compiler_data::{DataBuilder, Datum, DatumView, Name};
use std::sync::atomic::{AtomicU64, Ordering};
fn truthy(v: &Datum) -> bool {
    !matches!(v.inspect(), DatumView::Nil | DatumView::Boolean(false))
}
pub(crate) fn invalid(message: &'static str) -> String {
    message.into()
}
fn symbol(c: &DataBuilder, name: &str) -> Datum {
    c.symbol(Name::new(None, name))
}
fn keyword(c: &DataBuilder, name: &str) -> Datum {
    c.keyword(Name::new(None, name))
}
/// Whether `v` has truthy `:async` metadata, as upstream reads it from the
/// symbols of an `^:async` `fn` or `defn`.
pub(crate) fn asynchronous(c: &DataBuilder, v: &Datum) -> Result<bool, String> {
    let Some(metadata) = v.metadata() else {
        return Ok(false);
    };
    Ok(matches!(metadata.inspect(), DatumView::Map(_))
        && metadata
            .get(&keyword(c, "async"))?
            .is_some_and(|value| truthy(&value)))
}
/// The list `form` with `{:async true}` metadata on its head symbol: the
/// `fn*` of an `^:async` `fn`, or the `fn` of an `^:async` `defn`.
pub(crate) fn mark_asynchronous(c: &DataBuilder, form: &Datum) -> Result<Datum, String> {
    let mut forms = form.elements()?;
    let Some(head) = forms.first_mut() else {
        return Ok(form.clone());
    };
    let metadata = head.metadata().unwrap_or(c.map(&[])?);
    let metadata = c.assoc(&metadata, &keyword(c, "async"), &c.boolean(true))?;
    *head = c.with_meta(head, &metadata)?;
    let result = c.list(&forms)?;
    Ok(match form.metadata() {
        Some(metadata) => result.with_metadata(metadata),
        None => result,
    })
}
fn named(v: &Datum, name: &str) -> bool {
    matches!(v.inspect(), DatumView::Symbol(n) if n == Name::new(None, name))
}
fn directive(v: &Datum, name: &str) -> bool {
    matches!(v.inspect(), DatumView::Keyword(n) if n == Name::new(None, name))
}
pub(crate) fn call(c: &DataBuilder, name: &str, args: &[Datum]) -> Result<Datum, String> {
    let mut forms = vec![if [
        "let*",
        "loop*",
        "fn*",
        "letfn*",
        "if",
        "do",
        "def",
        "quote",
        "throw",
        "case*",
        "compiler-case*",
        "compiler-assert*",
        "recur",
    ]
    .contains(&name)
    {
        symbol(c, name)
    } else {
        c.symbol(Name::new(Some("clojure.core"), name))
    }];
    forms.extend_from_slice(args);
    c.list(&forms)
}
pub(crate) fn fresh(c: &DataBuilder) -> Datum {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .expect("binding gensym identities exhausted");
    symbol(c, &format!("binding__{id}__auto__"))
}
fn local(c: &DataBuilder, value: &Datum) -> Result<Datum, String> {
    let name = match value.inspect() {
        DatumView::Symbol(n) | DatumView::Keyword(n) => n,
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
    c: &'a DataBuilder,
}
impl<'a> Expander<'a> {
    fn new(c: &'a DataBuilder) -> Result<Self, String> {
        Ok(Self { c })
    }
    fn entries(&self, map: &Datum) -> Result<Vec<(Datum, Datum)>, String> {
        map.entries()
    }

    fn bindings(&self, bindings: &Datum) -> Result<Vec<Datum>, String> {
        if !matches!(bindings.inspect(), DatumView::Vector(_)) {
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
            .all(|v| matches!(v.inspect(), DatumView::Symbol(_)));
        for pair in pairs.as_chunks::<2>().0 {
            if symbols {
                result.extend_from_slice(pair);
            } else {
                if matches!(pair[0].inspect(), DatumView::Keyword(_)) {
                    return Err(invalid("unsupported binding key"));
                }
                self.bind(&mut result, &pair[0], pair[1].clone(), 0)?;
            }
        }
        Ok(result)
    }
    fn bind(
        &self,
        out: &mut Vec<Datum>,
        pattern: &Datum,
        value: Datum,
        depth: usize,
    ) -> Result<(), String> {
        if depth >= 128 {
            return Err(invalid("compiler destructuring nesting limit exceeded"));
        }
        match pattern.inspect() {
            DatumView::Symbol(_) | DatumView::Keyword(_) => {
                out.extend([local(self.c, pattern)?, value])
            }
            DatumView::Vector(_) => self.vector(out, pattern, value, depth + 1)?,
            DatumView::Map(_) => self.map(out, pattern, value, depth + 1)?,
            _ => return Err(invalid("unsupported binding form")),
        }
        Ok(())
    }
    fn vector(
        &self,
        out: &mut Vec<Datum>,
        pattern: &Datum,
        value: Datum,
        depth: usize,
    ) -> Result<(), String> {
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
        out: &mut Vec<Datum>,
        pattern: &Datum,
        value: Datum,
        depth: usize,
    ) -> Result<(), String> {
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
            .filter(|v| !matches!(v.inspect(), DatumView::Nil))
            .unwrap_or(c.map(&[])?);
        if !matches!(defaults.inspect(), DatumView::Map(_) | DatumView::Nil) {
            return Err(invalid(":or defaults must be a map"));
        }
        if let Some(binding) = pattern.get(&keyword(c, "as"))?.filter(truthy) {
            if !matches!(binding.inspect(), DatumView::Symbol(n) if n.namespace().is_none()) {
                return Err(invalid("map :as requires an unqualified symbol"));
            }
            out.extend([binding, whole.clone()]);
        }
        for (binding, key) in self.entries(pattern)? {
            if directive(&binding, "or") || directive(&binding, "as") {
                continue;
            }
            if let DatumView::Keyword(dir) = binding.inspect() {
                let which = String::from_utf16(dir.name()).unwrap_or_default();
                if !["keys", "strs", "syms"].contains(&which.as_str()) {
                    return Err(invalid("unsupported map destructuring directive"));
                }
                if !matches!(key.inspect(), DatumView::Vector(_)) {
                    return Err(invalid("map directive requires a vector"));
                }
                for binding in key.elements()? {
                    let name = match binding.inspect() {
                        DatumView::Symbol(n) | DatumView::Keyword(n) => n,
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
                            if matches!(binding.inspect(), DatumView::Keyword(_)) {
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
        out: &mut Vec<Datum>,
        binding: &Datum,
        key: Datum,
        whole: &Datum,
        defaults: &Datum,
        depth: usize,
    ) -> Result<(), String> {
        let localized = if matches!(
            binding.inspect(),
            DatumView::Symbol(_) | DatumView::Keyword(_)
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
    fn function(&self, declarations: &[Datum]) -> Result<Datum, String> {
        self.function_with_fields(declarations, &[])
    }
    fn function_with_fields(
        &self,
        declarations: &[Datum],
        fields: &[(Datum, Datum)],
    ) -> Result<Datum, String> {
        let c = self.c;
        let mut start = 0;
        let mut result = Vec::new();
        if let Some(name) = declarations
            .first()
            .filter(|v| matches!(v.inspect(), DatumView::Symbol(_)))
        {
            result.push(name.clone());
            start += 1;
        }
        let declarations = &declarations[start..];
        let Some(first) = declarations.first() else {
            return Err(invalid("function requires a parameter declaration"));
        };
        let methods = if matches!(first.inspect(), DatumView::Vector(_)) {
            vec![declarations.to_vec()]
        } else {
            declarations
                .iter()
                .map(|v| {
                    if !matches!(v.inspect(), DatumView::List(_)) {
                        return Err(invalid("function requires method lists"));
                    }
                    v.elements()
                })
                .collect::<Result<Vec<_>, String>>()?
        };
        for method in methods {
            let Some(params) = method
                .first()
                .filter(|v| matches!(v.inspect(), DatumView::Vector(_)))
            else {
                return Err(invalid("function parameters must be a vector"));
            };
            let mut body = method[1..].to_vec();
            let conditions = if body.len() > 1 && matches!(body[0].inspect(), DatumView::Map(_)) {
                Some(body.remove(0))
            } else {
                params.metadata()
            };
            if let Some(conditions) = conditions {
                if let Some(post) = conditions.get(&keyword(c, "post"))?.filter(truthy) {
                    let mut forms = vec![c.vector(&[symbol(c, "%"), call(c, "do", &body)?])?];
                    for condition in post.elements()? {
                        forms.push(call(c, "assert", &[condition])?);
                    }
                    forms.push(symbol(c, "%"));
                    body = vec![call(c, "let*", &forms)?];
                }
                if let Some(pre) = conditions.get(&keyword(c, "pre"))?.filter(truthy) {
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
                } else if matches!(param.inspect(), DatumView::Symbol(_)) {
                    parameters.push(param.clone());
                } else {
                    if matches!(param.inspect(), DatumView::Keyword(_)) {
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
            // Fields surround destructuring and the body, but never shadow an
            // explicit parameter. Destructured locals shadow fields in the inner
            // let above. The actual receiver is reread on every recur iteration.
            // fn permits repeated symbols with the last argument winning. Keep
            // a separate receiver slot when its public name is shadowed.
            if !fields.is_empty() && parameters[1..].iter().any(|p| p.value_eq(&parameters[0])) {
                parameters[0] = fresh(c);
            }
            let mut field_bindings = Vec::new();
            for (field, key) in fields {
                if !parameters.iter().any(|p| p.value_eq(field)) {
                    field_bindings.extend([
                        field.clone(),
                        c.list(&[key.clone(), parameters[0].clone()])?,
                    ]);
                }
            }
            if !field_bindings.is_empty() {
                let mut args = vec![c.vector(&field_bindings)?];
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

pub(crate) fn let_macro(c: &mut DataBuilder, args: &[Datum]) -> Result<Datum, String> {
    let bindings = Expander::new(c)?.bindings(&args[2])?;
    let mut forms = vec![c.vector(&bindings)?];
    forms.extend_from_slice(&args[3..]);
    call(c, "let*", &forms)
}
pub(crate) fn loop_macro(c: &mut DataBuilder, args: &[Datum]) -> Result<Datum, String> {
    let expander = Expander::new(c)?;
    // Validate first, including nested patterns before emitting any execution.
    expander.bindings(&args[2])?;
    let pairs = args[2].elements()?;
    if pairs
        .iter()
        .step_by(2)
        .all(|v| matches!(v.inspect(), DatumView::Symbol(_)))
    {
        return call(c, "loop*", &args[2..]);
    }
    let mut outer = Vec::new();
    let mut targets = Vec::new();
    let mut inner = Vec::new();
    for pair in pairs.as_chunks::<2>().0 {
        let temp = if matches!(pair[0].inspect(), DatumView::Symbol(_)) {
            pair[0].clone()
        } else {
            fresh(c)
        };
        outer.extend([temp.clone(), pair[1].clone()]);
        if !matches!(pair[0].inspect(), DatumView::Symbol(_)) {
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
pub(crate) fn fn_macro(c: &mut DataBuilder, args: &[Datum]) -> Result<Datum, String> {
    Expander::new(c)?.function(&args[2..])
}
pub(crate) fn letfn_macro(c: &mut DataBuilder, args: &[Datum]) -> Result<Datum, String> {
    if !matches!(args[2].inspect(), DatumView::Vector(_)) {
        return Err(invalid("letfn requires a vector"));
    }
    let e = Expander::new(c)?;
    let mut bindings = Vec::new();
    for spec in args[2].elements()? {
        if !matches!(spec.inspect(), DatumView::List(_)) {
            return Err(invalid("letfn requires function declarations"));
        }
        let forms = spec.elements()?;
        let Some(name) = forms
            .first()
            .filter(|v| matches!(v.inspect(), DatumView::Symbol(n) if n.namespace().is_none()))
        else {
            return Err(invalid("letfn requires function names"));
        };
        bindings.extend([name.clone(), e.function(&forms)?]);
    }
    let mut forms = vec![c.vector(&bindings)?];
    forms.extend_from_slice(&args[3..]);
    call(c, "letfn*", &forms)
}
pub(crate) fn defn_macro(c: &mut DataBuilder, args: &[Datum]) -> Result<Datum, String> {
    definition(c, args, false)
}
pub(crate) fn private_defn(c: &mut DataBuilder, args: &[Datum]) -> Result<Datum, String> {
    definition(c, args, true)
}
fn definition(c: &DataBuilder, args: &[Datum], private: bool) -> Result<Datum, String> {
    let name = &args[2];
    if !matches!(name.inspect(), DatumView::Symbol(_)) {
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
        .is_some_and(|v| matches!(v.inspect(), DatumView::String(_)))
    {
        metadata = c.assoc(&metadata, &keyword(c, "doc"), &args[start])?;
        start += 1;
    }
    if args
        .get(start)
        .is_some_and(|v| matches!(v.inspect(), DatumView::Map(_)))
    {
        for (k, v) in args[start].entries()? {
            metadata = c.assoc(&metadata, &k, &v)?;
        }
        start += 1;
    }
    let mut declarations = &args[start..];
    let multi = !declarations
        .first()
        .is_some_and(|v| matches!(v.inspect(), DatumView::Vector(_)));
    if multi
        && declarations
            .last()
            .is_some_and(|v| matches!(v.inspect(), DatumView::Map(_)))
    {
        for (k, v) in declarations.last().unwrap().entries()? {
            metadata = c.assoc(&metadata, &k, &v)?;
        }
        declarations = &declarations[..declarations.len() - 1];
    }
    let methods = if multi {
        declarations
            .iter()
            .map(Datum::elements)
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![declarations.to_vec()]
    };
    let mut arglists = Vec::new();
    for method in methods {
        let Some(params) = method
            .first()
            .filter(|v| matches!(v.inspect(), DatumView::Vector(_)))
        else {
            return Err(invalid("defn requires parameter vectors"));
        };
        let mut params = params.clone();
        if method.len() > 2 && matches!(method[1].inspect(), DatumView::Map(_)) {
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
            &call(c, "quote", &[c.list(&arglists)?])?,
        )?;
    }
    for (k, v) in metadata.entries()? {
        name_metadata = c.assoc(&name_metadata, &k, &v)?;
    }
    let name = c.with_meta(name, &name_metadata)?;
    // Upstream's `fn*` reads `:async` from the Var name `def` passes it.
    let mut function = call(c, "fn", declarations)?;
    if asynchronous(c, &name)? {
        function = mark_asynchronous(c, &function)?;
    }
    call(c, "def", &[name, function])
}
