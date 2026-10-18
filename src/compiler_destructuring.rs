//! Pure compiler destructuring of source-ordered compiler forms.
use crate::compiler_data::{DataBuilder, Datum, DatumView, Form, Name};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type Entries = Vec<(Datum, Datum)>;
fn named(v: &Datum, name: &str) -> bool {
    matches!(v.inspect(), DatumView::Symbol(n) if n == Name::new(None, name))
}
fn ident(v: &Datum) -> bool {
    matches!(v.inspect(), DatumView::Symbol(_) | DatumView::Keyword(_))
}
fn truth(v: &Datum) -> bool {
    !matches!(v.inspect(), DatumView::Nil | DatumView::Boolean(false))
}
fn extended(pattern: &Datum) -> Result<bool> {
    match pattern.inspect() {
        DatumView::Map(_) => {
            for (binding, value) in pattern.entries()? {
                if let DatumView::Keyword(n) = binding.inspect() {
                    let name = String::from_utf16_lossy(n.name());
                    if matches!(
                        name.as_str(),
                        "keys!"
                            | "syms!"
                            | "strs!"
                            | "select"
                            | "defaults"
                            | "all"
                            | "excess"
                            | "missing"
                    ) {
                        return Ok(true);
                    }
                    if matches!(name.as_str(), "keys" | "syms" | "strs")
                        && value.elements()?.iter().any(|v| named(v, "&"))
                    {
                        return Ok(true);
                    }
                } else if extended(&binding)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        DatumView::Vector(_) => {
            for value in pattern.elements()? {
                if extended(&value)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

pub(super) fn expand(
    c: &DataBuilder,
    form: &Form,
    name: &Name,
) -> std::result::Result<Option<Form>, String> {
    (|| -> Result<Option<Form>> {
        if name.namespace() != Some(Name::new(None, "cljs.core").name()) {
            return Ok(None);
        }
        let name = String::from_utf16_lossy(name.name());
        if !matches!(name.as_str(), "let" | "fn" | "loop" | "letfn") {
            return Ok(None);
        }
        let parts = form.data().elements()?;
        let args = &parts[1..];
        let expander = Expander { c };
        let output = if name == "letfn" {
            let Some(specs) = args
                .first()
                .filter(|v| matches!(v.inspect(), DatumView::Vector(_)))
            else {
                return Ok(None);
            };
            let mut bindings = vec![];
            let mut found = false;
            for spec in specs.elements()? {
                if !matches!(spec.inspect(), DatumView::List(_)) {
                    return Ok(None);
                }
                let parts = spec.elements()?;
                let Some(function_name) = parts
                    .first()
                    .filter(|v| matches!(v.inspect(), DatumView::Symbol(_)))
                else {
                    return Ok(None);
                };
                let declarations = &parts[1..];
                let Some(first) = declarations.first() else {
                    return Ok(None);
                };
                let methods = if matches!(first.inspect(), DatumView::Vector(_)) {
                    vec![declarations.to_vec()]
                } else {
                    if declarations
                        .iter()
                        .any(|value| !matches!(value.inspect(), DatumView::List(_)))
                    {
                        return Ok(None);
                    }
                    declarations
                        .iter()
                        .map(Datum::elements)
                        .collect::<std::result::Result<Vec<_>, _>>()?
                };
                for method in methods {
                    if let Some(params) = method.first() {
                        found |= extended(params)?;
                    }
                }
                let mut function_args = vec![function_name.clone()];
                function_args.extend_from_slice(declarations);
                bindings.extend([function_name.clone(), expander.call("fn", &function_args)?]);
            }
            if !found {
                return Ok(None);
            }
            expander.body("letfn*", bindings, &args[1..])?
        } else if name == "fn" {
            let start = usize::from(
                args.first()
                    .is_some_and(|v| matches!(v.inspect(), DatumView::Symbol(_))),
            );
            let declarations = &args[start..];
            let Some(first) = declarations.first() else {
                return Ok(None);
            };
            let methods = if matches!(first.inspect(), DatumView::Vector(_)) {
                vec![declarations.to_vec()]
            } else {
                if declarations
                    .iter()
                    .any(|value| !matches!(value.inspect(), DatumView::List(_)))
                {
                    return Ok(None);
                }
                declarations
                    .iter()
                    .map(Datum::elements)
                    .collect::<std::result::Result<Vec<_>, _>>()?
            };
            let mut found = false;
            for method in &methods {
                if let Some(params) = method.first() {
                    found |= extended(params)?;
                }
            }
            if !found {
                return Ok(None);
            }
            expander.function(&args[..start], methods)?
        } else {
            let Some(bindings) = args.first() else {
                return Ok(None);
            };
            if !matches!(bindings.inspect(), DatumView::Vector(_)) {
                return Ok(None);
            }
            let pairs = bindings.elements()?;
            let mut found = false;
            for pattern in pairs.iter().step_by(2) {
                found |= extended(pattern)?;
            }
            if !found {
                return Ok(None);
            }
            if !pairs.len().is_multiple_of(2) {
                return Err("binding vector requires binding/initializer pairs".into());
            }
            if name == "let" {
                let mut bindings = vec![];
                for pair in pairs.as_chunks::<2>().0 {
                    expander.bind(&mut bindings, &pair[0], pair[1].clone(), 0)?;
                }
                expander.body("let*", bindings, &args[1..])?
            } else {
                let mut outer = vec![];
                let mut targets = vec![];
                let mut inner = vec![];
                for pair in pairs.as_chunks::<2>().0 {
                    if matches!(pair[0].inspect(), DatumView::Keyword(_)) {
                        return Err("unsupported binding key".into());
                    }
                    let simple = matches!(pair[0].inspect(), DatumView::Symbol(_));
                    let temp = if simple {
                        pair[0].clone()
                    } else {
                        expander.fresh()
                    };
                    outer.extend([temp.clone(), pair[1].clone()]);
                    if !simple {
                        expander.bind(&mut outer, &pair[0], temp.clone(), 0)?;
                    }
                    targets.extend([temp.clone(), temp.clone()]);
                    expander.bind(&mut inner, &pair[0], temp, 0)?;
                }
                let body = expander.body("let*", inner, &args[1..])?;
                let body = expander.body("loop*", targets, &[body])?;
                expander.body("let*", outer, &[body])?
            }
        };
        Ok(Some(Form::from_expansion(
            output,
            form,
            Name::new(Some("cljs.core"), &name),
        )))
    })()
    .map_err(|e| e.to_string())
}

struct Expander<'a> {
    c: &'a DataBuilder,
}
impl Expander<'_> {
    fn sym(&self, name: &str) -> Datum {
        self.c.symbol(Name::new(None, name))
    }
    fn kw(&self, name: &str) -> Datum {
        self.c.keyword(Name::new(None, name))
    }
    fn fresh(&self) -> Datum {
        self.sym(&format!(
            "compiler_destructure__{}__auto__",
            crate::generated_names::next_destructure()
        ))
    }
    fn call(&self, name: &str, args: &[Datum]) -> Result<Datum> {
        let op = if matches!(
            name,
            "let*" | "letfn*" | "loop*" | "fn*" | "if" | "do" | "quote" | "throw" | "new"
        ) {
            self.sym(name)
        } else {
            self.c.symbol(Name::new(Some("cljs.core"), name))
        };
        let mut forms = vec![op];
        forms.extend_from_slice(args);
        Ok(self.c.list(&forms)?)
    }
    fn body(&self, op: &str, bindings: Vec<Datum>, body: &[Datum]) -> Result<Datum> {
        let mut args = vec![self.c.vector(&bindings)?];
        args.extend_from_slice(body);
        self.call(op, &args)
    }
    fn entries(&self, value: &Datum) -> Result<Entries> {
        Ok(value.entries()?)
    }
    fn assoc_pattern(&self, pattern: &Datum, key: Datum, value: Datum) -> Result<Datum> {
        Ok(self.c.assoc(pattern, &key, &value)?)
    }
    fn local(&self, binding: &Datum) -> Result<Datum> {
        let name = match binding.inspect() {
            DatumView::Keyword(n) | DatumView::Symbol(n) => n,
            _ => return Err("binding must be named".into()),
        };
        let local = self.c.symbol(Name::from_utf16(None, name.name()));
        Ok(if let Some(meta) = binding.metadata() {
            self.c.with_meta(&local, &meta)?
        } else {
            local
        })
    }
    fn function(&self, name: &[Datum], methods: Vec<Vec<Datum>>) -> Result<Datum> {
        let mut output = name.to_vec();
        for method in methods {
            let Some(params) = method
                .first()
                .filter(|v| matches!(v.inspect(), DatumView::Vector(_)))
            else {
                return Err("function parameters must be a vector".into());
            };
            let mut body = method[1..].to_vec();
            let conditions = if body.len() > 1 && matches!(body[0].inspect(), DatumView::Map(_)) {
                Some(body.remove(0))
            } else {
                params.metadata()
            };
            if let Some(conditions) = conditions {
                if let Some(post) = conditions.get(&self.kw("post"))?.filter(truth) {
                    let mut tail = vec![];
                    for condition in post.elements()? {
                        tail.push(self.call("assert", &[condition])?);
                    }
                    tail.push(self.sym("%"));
                    body = vec![self.body(
                        "let*",
                        vec![self.sym("%"), self.call("do", &body)?],
                        &tail,
                    )?];
                }
                if let Some(pre) = conditions.get(&self.kw("pre"))?.filter(truth) {
                    let mut before = vec![];
                    for condition in pre.elements()? {
                        before.push(self.call("assert", &[condition])?);
                    }
                    before.extend(body);
                    body = before;
                }
            }
            let mut parameters = vec![];
            let mut bindings = vec![];
            let parts = params.elements()?;
            let mut rest = false;
            for (index, param) in parts.iter().enumerate() {
                if named(param, "&") {
                    if rest || index + 2 != parts.len() {
                        return Err("& requires one final function parameter".into());
                    }
                    rest = true;
                    parameters.push(param.clone());
                } else if matches!(param.inspect(), DatumView::Symbol(_)) {
                    parameters.push(param.clone());
                } else {
                    if matches!(param.inspect(), DatumView::Keyword(_)) {
                        return Err("unsupported function binding key".into());
                    }
                    let temp = self.fresh();
                    parameters.push(temp.clone());
                    self.bind(&mut bindings, param, temp, 0)?;
                }
            }
            let mut method = vec![self.c.vector(&parameters)?];
            method.push(self.body("let*", bindings, &body)?);
            output.push(self.c.list(&method)?);
        }
        self.call("fn*", &output)
    }
    fn bind(
        &self,
        out: &mut Vec<Datum>,
        pattern: &Datum,
        value: Datum,
        depth: usize,
    ) -> Result<()> {
        if depth > 128 {
            return Err("compiler destructuring nesting limit exceeded".into());
        }
        match pattern.inspect() {
            DatumView::Symbol(_) | DatumView::Keyword(_) => {
                out.extend([self.local(pattern)?, value])
            }
            DatumView::Vector(_) => {
                let whole = self.fresh();
                out.extend([whole.clone(), value]);
                let parts = pattern.elements()?;
                let has_rest = parts.iter().any(|v| named(v, "&"));
                let seq = self.fresh();
                if has_rest {
                    out.extend([seq.clone(), self.call("seq", std::slice::from_ref(&whole))?]);
                }
                let mut index = 0;
                let mut rest = false;
                while index < parts.len() {
                    let part = &parts[index];
                    if named(part, "&") {
                        if rest || index + 1 == parts.len() {
                            return Err("& requires one rest binding".into());
                        }
                        self.bind(out, &parts[index + 1], seq.clone(), depth + 1)?;
                        rest = true;
                        index += 2;
                    } else if part.value_eq(&self.kw("as")) {
                        if index + 2 != parts.len() {
                            return Err(":as requires one final binding".into());
                        }
                        self.bind(out, &parts[index + 1], whole.clone(), depth + 1)?;
                        break;
                    } else {
                        if rest {
                            return Err("only :as may follow a rest binding".into());
                        }
                        let item = if has_rest {
                            let first = self.fresh();
                            out.extend([
                                first.clone(),
                                self.call("first", std::slice::from_ref(&seq))?,
                                seq.clone(),
                                self.call("next", std::slice::from_ref(&seq))?,
                            ]);
                            first
                        } else {
                            self.call(
                                "nth",
                                &[whole.clone(), self.c.integer(index as i64), self.c.nil()],
                            )?
                        };
                        self.bind(out, part, item, depth + 1)?;
                        index += 1;
                    }
                }
            }
            DatumView::Map(_) => self.map(out, pattern, value, depth + 1)?,
            _ => return Err("unsupported binding form".into()),
        }
        Ok(())
    }
    // Building the map before filtering preserves evaluation of selector
    // key expressions even when a nested selection has a nil result.
    fn some_vals(&self, entries: &Entries) -> Result<Datum> {
        self.call("some-vals", &[self.c.map(entries)?])
    }
    fn map(&self, out: &mut Vec<Datum>, pattern: &Datum, value: Datum, depth: usize) -> Result<()> {
        let c = self.c;
        let directive = |name: &str| -> Result<Option<Datum>> {
            Ok(pattern.get(&self.kw(name))?.filter(truth))
        };
        let defaults = directive("or")?;
        let defaults_as = directive("defaults")?;
        if defaults_as.is_some() && defaults.is_none() {
            return Err("Can't specify :defaults without :or".into());
        }
        let mut default_vars = vec![];
        if let Some(defaults) = &defaults {
            if !matches!(defaults.inspect(), DatumView::Map(_)) {
                return Err(":or defaults must be a map".into());
            }
            for (key, expression) in self.entries(defaults)? {
                let temp = self.fresh();
                out.extend([temp.clone(), expression]);
                default_vars.push((key, temp));
            }
        }
        let whole = self.fresh();
        out.extend([
            whole.clone(),
            value,
            whole.clone(),
            self.call(
                "if",
                &[
                    self.call("seq?", std::slice::from_ref(&whole))?,
                    self.call("seq-to-map-for-destructuring", std::slice::from_ref(&whole))?,
                    whole.clone(),
                ],
            )?,
        ]);
        if let Some(alias) = directive("as")? {
            out.extend([alias, whole.clone()]);
        }
        let select = directive("select")?;
        let all = directive("all")?;
        let excess = directive("excess")?;
        let missing = directive("missing")?;
        let not_found = missing.as_ref().map(|_| self.fresh());
        if let Some(missing) = &missing {
            out.extend([
                missing.clone(),
                c.nil(),
                not_found.as_ref().unwrap().clone(),
                self.call("new", &[c.symbol(Name::new(Some("js"), "Object"))])?,
            ]);
        }
        let mut selected: Vec<Datum> = vec![];
        let mut binding_keys: Entries = vec![];
        let mut subs: [Entries; 4] = std::array::from_fn(|_| vec![]);
        let mut pending = vec![];
        for (binding, key) in self.entries(pattern)? {
            if let DatumView::Keyword(dir) = binding.inspect() {
                let which = String::from_utf16_lossy(dir.name());
                if dir.namespace().is_none()
                    && matches!(
                        which.as_str(),
                        "as" | "or" | "defaults" | "select" | "all" | "excess" | "missing"
                    )
                {
                    continue;
                }
                let required = which.ends_with('!');
                let which = which.trim_end_matches('!');
                if !matches!(which, "keys" | "syms" | "strs") {
                    return Err("unsupported map destructuring directive".into());
                }
                if !matches!(key.inspect(), DatumView::Vector(_)) {
                    return Err("map directive requires a vector".into());
                }
                let mut before_amp = true;
                for binding in key.elements()? {
                    if named(&binding, "&") {
                        if !before_amp {
                            return Err("& can only appear once in map directive".into());
                        }
                        before_amp = false;
                        continue;
                    }
                    let key = if before_amp {
                        let name = match binding.inspect() {
                            DatumView::Symbol(n) | DatumView::Keyword(n) => n,
                            _ => return Err("map directive requires named bindings".into()),
                        };
                        let key_name =
                            Name::from_utf16(dir.namespace().or(name.namespace()), name.name());
                        match which {
                            "keys" => c.keyword(key_name),
                            "syms" => self.call("quote", &[c.symbol(key_name)])?,
                            _ => {
                                let mut text = String::new();
                                if matches!(binding.inspect(), DatumView::Keyword(_)) {
                                    text.push(':');
                                }
                                if let Some(ns) = name.namespace() {
                                    text.push_str(&String::from_utf16_lossy(ns));
                                    text.push('/');
                                }
                                text.push_str(&String::from_utf16_lossy(name.name()));
                                c.string(text.encode_utf16().collect::<Vec<_>>())
                            }
                        }
                    } else {
                        if matches!(binding.inspect(), DatumView::Symbol(_)) {
                            return Err(
                                "binding symbols can only appear before &, use keys after".into()
                            );
                        }
                        binding.clone()
                    };
                    if !selected.iter().any(|v| v.value_eq(&key)) {
                        selected.push(key.clone());
                    }
                    if before_amp {
                        binding_keys.push((self.local(&binding)?, key.clone()));
                        pending.push((binding, key, required));
                    } else if required {
                        pending.push((self.fresh(), key, true));
                    }
                }
            } else {
                if !selected.iter().any(|v| v.value_eq(&key)) {
                    selected.push(key.clone());
                }
                let mut binding = binding;
                if matches!(binding.inspect(), DatumView::Map(_)) {
                    for (index, (name, enabled)) in [
                        ("select", &select),
                        ("all", &all),
                        ("excess", &excess),
                        ("missing", &missing),
                    ]
                    .iter()
                    .enumerate()
                    {
                        if enabled.is_some() {
                            let sub = binding
                                .get(&self.kw(name))?
                                .filter(truth)
                                .unwrap_or_else(|| self.fresh());
                            binding = self.assoc_pattern(&binding, self.kw(name), sub.clone())?;
                            subs[index].push((key.clone(), sub));
                        }
                    }
                } else if matches!(binding.inspect(), DatumView::Symbol(_)) {
                    binding_keys.push((binding.clone(), key.clone()));
                }
                pending.push((binding, key, false));
            }
        }
        for (binding, key, required) in pending {
            let local = if ident(&binding) {
                self.local(&binding)?
            } else {
                binding.clone()
            };
            let local_default = default_vars.iter().find(|(k, _)| k.value_eq(&local));
            let key_default = default_vars.iter().find(|(k, _)| k.value_eq(&key));
            if local_default.is_some() && key_default.is_some() {
                return Err("Multiple :or defaults for same key".into());
            }
            let default = local_default.or(key_default).map(|(_, v)| v.clone());
            if required && default.is_some() {
                return Err("Can't supply default value for required key".into());
            }
            let mut get_args = vec![whole.clone(), key.clone()];
            if let Some(default) = default {
                get_args.push(default);
            }
            let get = self.call("get", &get_args)?;
            let value = if required {
                if let Some(missing) = &missing {
                    let sentinel = not_found.as_ref().unwrap().clone();
                    let found = self.fresh();
                    let absent = self.fresh();
                    out.extend([
                        found.clone(),
                        self.call("get", &[whole.clone(), key.clone(), sentinel.clone()])?,
                        absent.clone(),
                        self.call("identical?", &[found.clone(), sentinel])?,
                        missing.clone(),
                        self.call(
                            "if",
                            &[
                                absent.clone(),
                                self.call("assoc", &[missing.clone(), key.clone(), c.nil()])?,
                                missing.clone(),
                            ],
                        )?,
                    ]);
                    self.call("if", &[absent, c.nil(), found])?
                } else {
                    self.call("req!", &[whole.clone(), key.clone()])?
                }
            } else {
                get
            };
            self.bind(out, &local, value, depth)?;
        }
        let new_defaults =
            defaults.is_some() && (defaults_as.is_some() || select.is_some() || all.is_some());
        let mut default_map = vec![];
        for (key, value) in &default_vars {
            let key = if matches!(key.inspect(), DatumView::Symbol(_)) {
                match binding_keys.iter().find(|(b, _)| b.value_eq(key)) {
                    Some((_, key)) => key.clone(),
                    None if new_defaults => {
                        return Err("symbol in :or does not refer to a binding".into());
                    }
                    None => c.nil(),
                }
            } else {
                key.clone()
            };
            if new_defaults && !selected.iter().any(|v| v.value_eq(&key)) {
                return Err("keys appear only in :or".into());
            }
            default_map.push((key, value.clone()));
        }
        let dm = if default_map.is_empty() {
            c.nil()
        } else {
            c.map(&default_map)?
        };
        let selected = c.vector(&selected)?;
        if let Some(select) = select {
            let merged = self.fresh();
            out.extend([
                select,
                self.body(
                    "let*",
                    vec![
                        merged.clone(),
                        self.call(
                            "merge",
                            &[dm.clone(), whole.clone(), self.some_vals(&subs[0])?],
                        )?,
                    ],
                    &[self.call(
                        "if",
                        &[
                            merged.clone(),
                            self.call("select-keys", &[merged, selected.clone()])?,
                            c.nil(),
                        ],
                    )?],
                )?,
            ]);
        }
        if let Some(all) = all {
            out.extend([
                all,
                self.call(
                    "merge",
                    &[dm.clone(), whole.clone(), self.some_vals(&subs[1])?],
                )?,
            ]);
        }
        if let Some(excess) = excess {
            out.extend([
                excess,
                self.call(
                    "merge",
                    &[
                        self.call(
                            "not-empty",
                            &[self.call(
                                "apply",
                                &[
                                    c.symbol(Name::new(Some("cljs.core"), "dissoc")),
                                    whole,
                                    selected,
                                ],
                            )?],
                        )?,
                        self.some_vals(&subs[2])?,
                    ],
                )?,
            ]);
        }
        if let Some(missing) = missing {
            out.extend([
                missing.clone(),
                self.call("merge", &[missing, self.some_vals(&subs[3])?])?,
            ]);
        }
        if let Some(defaults_as) = defaults_as {
            out.extend([defaults_as, dm]);
        }
        Ok(())
    }
}
