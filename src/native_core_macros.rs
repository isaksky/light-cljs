//! The core macros the analyzer expands in Rust rather than in the
//! compile-time engine (light-cljs-426): each gives the forms its definition
//! in src/compiler_core_macros.cljs gives, built from the call's own data so
//! that what it reuses keeps its identity, and so its spans, as the native
//! `let` and `fn` do ([`crate::compiler_data::Form::from_expansion`]).
//!
//! A call such a definition would reject (a wrong arity, an odd `cond`, a
//! binding that is not a two-form vector, a `defmulti` option it refuses),
//! or one whose expansion this module cannot be sure of (a `js-obj` of more
//! than eight entries, whose order is a hash map's in the engine, or keys
//! whose equality is the engine's), is declined: [`expand`] returns
//! `Ok(None)` before it allocates a name, and the engine expands the call,
//! failing with its own message and position. So is a call the engine's
//! conversion refuses or a call outside a scope of generated names.
//! The names a definition allocates are those the compile-time runtime's `gensym` gives, in the
//! same order and with the same prefixes ([`Gensym`]), taken from the
//! scope's count without entering an engine (light-cljs-432,
//! [`crate::generated_names::gensym`]), so generated names, their scopes
//! and what a capture records of them stay what the engine gives, and a
//! unit whose only macros expand here needs no engine
//! ([`native_expansion`], which the check of macro-free units walks).
use crate::CollectionKind;
use crate::compiler_data::{DataBuilder, Datum, DatumKind, Name};

/// The core macros the analyzer expands in Rust.
pub(crate) fn native(name: &str) -> bool {
    matches!(
        name,
        "and"
            | "or"
            | "when"
            | "when-not"
            | "if-not"
            | "cond"
            | "condp"
            | "->"
            | "->>"
            | "cond->"
            | "cond->>"
            | "some->"
            | "some->>"
            | "case"
            | "if-let"
            | "when-let"
            | "if-some"
            | "when-some"
            | "when-first"
            | "js-obj"
            | "defmethod"
            | "defmulti"
            | "vswap!"
            | "for"
            | "doseq"
    )
}

/// Allocates names with the compile-time runtime's `gensym`, one per
/// prefix, in order. It is called at most once per expansion, after the
/// call is known to expand.
pub(crate) type Gensym<'a> = &'a mut dyn FnMut(&[&str]) -> Result<Vec<Datum>, String>;

const C: DataBuilder = DataBuilder;

fn symbol(name: &str) -> Datum {
    C.symbol(Name::new(None, name))
}
fn core(name: &str) -> Datum {
    C.symbol(Name::new(Some("cljs.core"), name))
}
fn list(items: Vec<Datum>) -> Datum {
    Datum::new(DatumKind::Collection {
        kind: CollectionKind::List,
        children: items.into(),
    })
}
fn vector(items: Vec<Datum>) -> Datum {
    Datum::new(DatumKind::Collection {
        kind: CollectionKind::Vector,
        children: items.into(),
    })
}
fn children(datum: &Datum) -> &[Datum] {
    match datum.kind() {
        DatumKind::Collection { children, .. } => children,
        _ => &[],
    }
}
/// `seq?`: the engine's lists, which `#()` bodies are there too.
fn is_seq(datum: &Datum) -> bool {
    matches!(
        datum.kind(),
        DatumKind::Collection {
            kind: CollectionKind::List | CollectionKind::AnonymousFn,
            ..
        }
    )
}
fn is_vector(datum: &Datum) -> bool {
    matches!(
        datum.kind(),
        DatumKind::Collection {
            kind: CollectionKind::Vector,
            ..
        }
    )
}
fn is_keyword(datum: &Datum, name: &str) -> bool {
    matches!(datum.kind(), DatumKind::Keyword(n) if *n == Name::new(None, name))
}
/// `(meta datum)`: none for nil metadata.
fn meta(datum: &Datum) -> Option<Datum> {
    datum
        .metadata()
        .filter(|m| !matches!(m.kind(), DatumKind::Nil))
}
/// `(with-meta result metadata)`.
fn with_meta(result: Datum, metadata: Option<Datum>) -> Datum {
    match metadata {
        Some(metadata) => result.with_metadata(metadata),
        None => result,
    }
}

/// What tells the engine's primitive values apart: a scalar, keyword or
/// string of the call converts to one, which the engine cannot tell from
/// another of its value.
#[derive(Hash, PartialEq, Eq)]
enum Primitive {
    Nil,
    Boolean(bool),
    Number(u64),
    Text(std::sync::Arc<[u16]>),
    Keyword(Name),
}
fn primitive(datum: &Datum) -> Option<Primitive> {
    let number = |value: f64| {
        Primitive::Number(if value.is_nan() {
            f64::NAN.to_bits()
        } else {
            value.to_bits()
        })
    };
    Some(match datum.kind() {
        DatumKind::Nil => Primitive::Nil,
        DatumKind::Boolean(value) => Primitive::Boolean(*value),
        DatumKind::Integer(value) => number(*value as f64),
        DatumKind::Double(value) => number(*value),
        DatumKind::Ratio {
            numerator,
            denominator,
        } => number(*numerator as f64 / *denominator as f64),
        DatumKind::Character(value) => Primitive::Text(std::sync::Arc::from([*value])),
        DatumKind::String(value) => Primitive::Text(value.clone()),
        DatumKind::Keyword(name) => Primitive::Keyword(name.clone()),
        DatumKind::CompilerLiteral {
            kind: crate::compiler_data::CompilerLiteralKind::BigInteger,
            payload,
        } => match payload.kind() {
            DatumKind::String(text) => number(
                String::from_utf16_lossy(text)
                    .parse::<f64>()
                    .unwrap_or(f64::NAN),
            ),
            _ => return None,
        },
        _ => return None,
    })
}

/// The call's arguments as the engine's expansion returns them: the
/// engine knows a primitive value only by its value, so where an expansion
/// places one outside the collections and symbols it returns as they were,
/// the value converts back as the last argument datum of that value in the
/// order the engine converted the arguments (each datum after its children
/// and its metadata), and keeps that datum's spelling and span; a value no
/// argument has is new. `arguments` holds the identities of every argument
/// datum, which the expansion may return unchanged.
struct Arguments {
    primitives: std::collections::HashMap<Primitive, Datum>,
    identities: std::collections::HashSet<usize>,
}
impl Arguments {
    fn new(arguments: &[Datum]) -> Self {
        fn record(datum: &Datum, arguments: &mut Arguments) {
            match datum.kind() {
                DatumKind::Collection { children, .. } => {
                    for child in children {
                        record(child, arguments);
                    }
                }
                DatumKind::CompilerLiteral { payload, .. } if !big_integer(datum) => {
                    record(payload, arguments)
                }
                _ => {}
            }
            if let Some(metadata) = datum.metadata() {
                record(&metadata, arguments);
            }
            arguments.identities.insert(datum.identity());
            if let Some(key) = primitive(datum) {
                arguments.primitives.insert(key, datum.clone());
            }
        }
        let mut result = Self {
            primitives: Default::default(),
            identities: Default::default(),
        };
        for argument in arguments {
            record(argument, &mut result);
        }
        result
    }
    /// `expanded` as the engine returns it.
    fn returned(&self, expanded: &Datum) -> Datum {
        if let Some(key) = primitive(expanded) {
            return self.primitives.get(&key).unwrap_or(expanded).clone();
        }
        if self.identities.contains(&expanded.identity()) {
            return expanded.clone();
        }
        let metadata = expanded.metadata().map(|m| self.returned(&m));
        let result = match expanded.kind() {
            DatumKind::Collection { kind, children } => Datum::new(DatumKind::Collection {
                kind: *kind,
                children: children.iter().map(|c| self.returned(c)).collect(),
            }),
            _ if metadata.is_none() => return expanded.clone(),
            kind => Datum::new(kind.clone()),
        };
        match metadata {
            Some(metadata) => result.with_metadata(metadata),
            None => result,
        }
    }
}

/// The expansion of the call `call` of the core macro `name` (one that
/// [`native`] knows) in namespace `namespace`, or `None` if the engine
/// expands it instead (the module documentation says when).
pub(crate) fn expand(
    name: &str,
    call: &Datum,
    namespace: &str,
    gensym: Gensym<'_>,
) -> Result<Option<Datum>, String> {
    let DatumKind::Collection {
        kind: CollectionKind::List,
        children: parts,
    } = call.kind()
    else {
        return Ok(None);
    };
    let Some(args) = parts.get(1..) else {
        return Ok(None);
    };
    Ok(match name {
        "and" | "or" => Some(short_circuit(args, name == "or", gensym)?),
        "when" => args.split_first().map(|(test, body)| {
            list(vec![
                symbol("if"),
                test.clone(),
                list(
                    [symbol("do")]
                        .into_iter()
                        .chain(body.iter().cloned())
                        .collect(),
                ),
            ])
        }),
        "when-not" => args.split_first().map(|(test, body)| {
            list(vec![
                symbol("if"),
                test.clone(),
                C.nil(),
                list(
                    [symbol("do")]
                        .into_iter()
                        .chain(body.iter().cloned())
                        .collect(),
                ),
            ])
        }),
        "if-not" => match args {
            [test, then] | [test, then, _] => Some(list(vec![
                symbol("if"),
                list(vec![core("not"), test.clone()]),
                then.clone(),
                args.get(2).cloned().unwrap_or_else(|| C.nil()),
            ])),
            _ => None,
        },
        "cond" => match args {
            [] => Some(C.nil()),
            [_] => None,
            [test, then, rest @ ..] => Some(list(vec![
                symbol("if"),
                test.clone(),
                then.clone(),
                list(
                    [core("cond")]
                        .into_iter()
                        .chain(rest.iter().cloned())
                        .collect(),
                ),
            ])),
        },
        "condp" => match args {
            [pred, value, clauses @ ..] => Some(condp(pred, value, clauses, gensym)?),
            _ => None,
        },
        "->" | "->>" => args.split_first().map(|(value, forms)| {
            forms.iter().fold(value.clone(), |value, form| {
                thread_step(value, form, name == "->>")
            })
        }),
        "cond->" | "cond->>" | "some->" | "some->>" => match args.split_first() {
            Some((value, clauses)) => conditional_thread(
                value,
                clauses,
                name.ends_with(">>"),
                name.starts_with("some"),
                gensym,
            )?,
            None => None,
        },
        "case" => args
            .split_first()
            .map(|(value, clauses)| case(call, value, clauses)),
        "if-let" | "if-some" => match args {
            [bindings, body @ ..] if !body.is_empty() => {
                conditional_binding(bindings, body, name == "if-some", false, false, gensym)?
            }
            _ => None,
        },
        "when-let" | "when-some" | "when-first" => match args.split_first() {
            Some((bindings, body)) => conditional_binding(
                bindings,
                body,
                name == "when-some",
                true,
                name == "when-first",
                gensym,
            )?,
            None => None,
        },
        "js-obj" => js_obj(args, gensym)?,
        "defmethod" => match args {
            [multifn, dispatch, tail @ ..] => Some(list(vec![
                core("-add-method"),
                multifn.clone(),
                dispatch.clone(),
                list(
                    [core("fn")]
                        .into_iter()
                        .chain(tail.iter().cloned())
                        .collect(),
                ),
            ])),
            _ => None,
        },
        "defmulti" => match args.split_first() {
            Some((name, declarations)) => defmulti(name, declarations, namespace)?,
            None => None,
        },
        "for" => match args {
            // Its definition, of one fixed arity, ignores further arguments.
            [bindings, body, ..] => sequence_for(bindings, body, gensym)?,
            _ => None,
        },
        "doseq" => match args.split_first() {
            Some((bindings, body)) => sequence_doseq(bindings, body, gensym)?,
            None => None,
        },
        "vswap!" => match args {
            [volatile, f, rest @ ..] => Some(list(vec![
                core("-vreset!"),
                volatile.clone(),
                list(
                    [f.clone(), list(vec![core("-deref"), volatile.clone()])]
                        .into_iter()
                        .chain(rest.iter().cloned())
                        .collect(),
                ),
            ])),
            _ => None,
        },
        _ => None,
    })
}

/// `macro-short-circuit`: `and` and `or`.
fn short_circuit(forms: &[Datum], is_or: bool, gensym: Gensym<'_>) -> Result<Datum, String> {
    let Some((last, init)) = forms.split_last() else {
        return Ok(if is_or { C.nil() } else { C.boolean(true) });
    };
    let names = if init.is_empty() {
        Vec::new()
    } else {
        gensym(&vec!["logic"; init.len()])?
    };
    let mut result = last.clone();
    for (form, temp) in init.iter().rev().zip(names) {
        let test = if is_or {
            vec![symbol("if"), temp.clone(), temp.clone(), result]
        } else {
            vec![symbol("if"), temp.clone(), result, temp.clone()]
        };
        result = list(vec![
            symbol("let*"),
            vector(vec![temp, form.clone()]),
            list(test),
        ]);
    }
    Ok(result)
}

/// `macro-thread-step`.
fn thread_step(value: Datum, form: &Datum, last: bool) -> Datum {
    let forms: Vec<Datum> = if is_seq(form) {
        if children(form).is_empty() {
            vec![C.nil()]
        } else {
            children(form).to_vec()
        }
    } else {
        vec![form.clone()]
    };
    let mut items = Vec::with_capacity(forms.len() + 1);
    if last {
        items.extend(forms);
        items.push(value);
    } else {
        let mut forms = forms.into_iter();
        items.extend(forms.next());
        items.push(value);
        items.extend(forms);
    }
    with_meta(list(items), meta(form))
}

/// `macro-conditional-thread`: `cond->`, `cond->>`, `some->` and `some->>`.
fn conditional_thread(
    value: &Datum,
    clauses: &[Datum],
    last: bool,
    some: bool,
    gensym: Gensym<'_>,
) -> Result<Option<Datum>, String> {
    if !some && !clauses.len().is_multiple_of(2) {
        return Ok(None);
    }
    let temp = gensym(&["thread"])?.remove(0);
    let width = if some { 1 } else { 2 };
    let mut bindings = vec![temp.clone(), value.clone()];
    let mut result = temp.clone();
    for (index, clause) in clauses.chunks(width).enumerate() {
        let threaded = thread_step(temp.clone(), clause.last().unwrap(), last);
        if index > 0 {
            bindings.push(temp.clone());
            bindings.push(result);
        }
        result = if some {
            list(vec![
                symbol("if"),
                list(vec![core("nil?"), temp.clone()]),
                C.nil(),
                threaded,
            ])
        } else {
            list(vec![
                symbol("if"),
                clause[0].clone(),
                threaded,
                temp.clone(),
            ])
        };
    }
    Ok(Some(list(vec![symbol("let*"), vector(bindings), result])))
}

/// `macro-conditional-binding`: `if-let`, `if-some`, `when-let`,
/// `when-some` and `when-first`.
fn conditional_binding(
    bindings: &Datum,
    body: &[Datum],
    some: bool,
    when: bool,
    first: bool,
    gensym: Gensym<'_>,
) -> Result<Option<Datum>, String> {
    let [binding, input] = children(bindings) else {
        return Ok(None);
    };
    if !is_vector(bindings) {
        return Ok(None);
    }
    let temp = gensym(&["binding"])?.remove(0);
    let input = if first {
        list(vec![core("seq"), input.clone()])
    } else {
        input.clone()
    };
    let value = if first {
        list(vec![core("first"), temp.clone()])
    } else {
        temp.clone()
    };
    let mut yes = vec![core("let"), vector(vec![binding.clone(), value])];
    if when {
        yes.extend(body.iter().cloned());
    } else {
        yes.push(body[0].clone());
    }
    let yes = list(yes);
    let no = if when {
        C.nil()
    } else {
        body.get(1).cloned().unwrap_or_else(|| C.nil())
    };
    let test = if some {
        list(vec![
            symbol("if"),
            list(vec![core("nil?"), temp.clone()]),
            no,
            yes,
        ])
    } else {
        list(vec![symbol("if"), temp.clone(), yes, no])
    };
    Ok(Some(list(vec![
        symbol("let*"),
        vector(vec![temp, input]),
        test,
    ])))
}

/// `macro-condp`.
fn condp(
    pred: &Datum,
    value: &Datum,
    clauses: &[Datum],
    gensym: Gensym<'_>,
) -> Result<Datum, String> {
    let mut pairs = Vec::new();
    let mut at = 0;
    while clauses.len() - at >= 2 {
        let arrow = is_keyword(&clauses[at + 1], ">>") && clauses.len() - at >= 3;
        let width = if arrow { 3 } else { 2 };
        pairs.push((&clauses[at..at + width], arrow));
        at += width;
    }
    let arrows = pairs.iter().filter(|(_, arrow)| *arrow).count();
    let mut names = gensym(
        &["predicate", "value"]
            .into_iter()
            .chain(std::iter::repeat_n("matched", arrows))
            .collect::<Vec<_>>(),
    )?
    .into_iter();
    let (p, v) = (names.next().unwrap(), names.next().unwrap());
    let mut result = match clauses.get(at) {
        Some(default) => default.clone(),
        None => list(vec![
            symbol("compiler-case*"),
            v.clone(),
            vector(vec![]),
            vector(vec![]),
        ]),
    };
    for (clause, arrow) in pairs.into_iter().rev() {
        let test = list(vec![p.clone(), clause[0].clone(), v.clone()]);
        result = if arrow {
            let matched = names.next().unwrap();
            list(vec![
                symbol("let*"),
                vector(vec![matched.clone(), test]),
                list(vec![
                    symbol("if"),
                    matched.clone(),
                    list(vec![clause[2].clone(), matched]),
                    result,
                ]),
            ])
        } else {
            list(vec![symbol("if"), test, clause[1].clone(), result])
        };
    }
    Ok(list(vec![
        symbol("let*"),
        vector(vec![p, pred.clone(), v, value.clone()]),
        result,
    ]))
}

/// `macro-case`, whose metadata keeps the whole call for analysis.
fn case(call: &Datum, value: &Datum, clauses: &[Datum]) -> Datum {
    let pairs = clauses.as_chunks::<2>().0.iter();
    let groups = pairs
        .clone()
        .map(|pair| {
            if is_seq(&pair[0]) {
                vector(children(&pair[0]).to_vec())
            } else {
                vector(vec![pair[0].clone()])
            }
        })
        .collect();
    let thens = pairs.map(|pair| pair[1].clone()).collect();
    let mut result = vec![
        symbol("compiler-case*"),
        value.clone(),
        vector(groups),
        vector(thens),
    ];
    if !clauses.len().is_multiple_of(2) {
        result.push(clauses.last().unwrap().clone());
    }
    let source = Datum::new(DatumKind::Collection {
        kind: CollectionKind::Map,
        children: vec![
            C.keyword(Name::new(Some("light-cljs"), "case-source")),
            call.clone(),
        ]
        .into(),
    });
    list(result).with_metadata(source)
}

/// Whether the engine's `=` would find `a` and `b` equal, or `None` where
/// this cannot tell it exactly.
fn engine_equal(a: &Datum, b: &Datum) -> Option<bool> {
    use DatumKind as D;
    const EXACT: i64 = 1 << 53;
    Some(match (a.kind(), b.kind()) {
        (D::Integer(x), _) | (_, D::Integer(x)) if x.unsigned_abs() > EXACT as u64 => {
            return None;
        }
        (D::Nil, D::Nil) => true,
        (D::Boolean(x), D::Boolean(y)) => x == y,
        (D::Integer(x), D::Integer(y)) => x == y,
        (D::Double(x), D::Double(y)) => x == y,
        (D::Integer(x), D::Double(y)) | (D::Double(y), D::Integer(x)) => *x as f64 == *y,
        (D::String(x), D::String(y)) => x == y,
        (D::Symbol(x), D::Symbol(y)) | (D::Keyword(x), D::Keyword(y)) => x == y,
        (
            D::Collection {
                kind: CollectionKind::List | CollectionKind::Vector,
                children: x,
            },
            D::Collection {
                kind: CollectionKind::List | CollectionKind::Vector,
                children: y,
            },
        ) => {
            // Undecided children decline even where a length differs.
            let mut equal = x.len() == y.len();
            for (x, y) in x.iter().zip(y.iter()) {
                equal &= engine_equal(x, y)?;
            }
            equal
        }
        (
            D::Character(_)
            | D::Ratio { .. }
            | D::CompilerLiteral { .. }
            | D::Collection {
                kind: CollectionKind::AnonymousFn | CollectionKind::Map | CollectionKind::Set,
                ..
            },
            _,
        )
        | (
            _,
            D::Character(_)
            | D::Ratio { .. }
            | D::CompilerLiteral { .. }
            | D::Collection {
                kind: CollectionKind::AnonymousFn | CollectionKind::Map | CollectionKind::Set,
                ..
            },
        ) => return None,
        _ => false,
    })
}

/// `macro-js-obj`, for at most eight entries.
fn js_obj(entries: &[Datum], gensym: Gensym<'_>) -> Result<Option<Datum>, String> {
    let raw = entries.as_chunks::<2>().0;
    if raw.len() > 8 {
        return Ok(None);
    }
    let mut pairs: Vec<&[Datum; 2]> = Vec::with_capacity(raw.len());
    for pair in raw {
        let mut found = false;
        for p in &mut pairs {
            let Some(equal) = engine_equal(&p[0], &pair[0]) else {
                return Ok(None);
            };
            if equal {
                *p = pair;
                found = true;
            }
        }
        if !found {
            pairs.push(pair);
        }
    }
    // The engine's characters are strings.
    if pairs
        .iter()
        .any(|p| matches!(p[0].kind(), DatumKind::Character(_)))
    {
        return Ok(None);
    }
    let strings: Vec<_> = pairs
        .iter()
        .filter(|p| matches!(p[0].kind(), DatumKind::String(_)))
        .collect();
    let symbols: Vec<_> = pairs
        .iter()
        .filter(|p| matches!(p[0].kind(), DatumKind::Symbol(_)))
        .collect();
    let expressions: Vec<_> = pairs
        .iter()
        .filter(|p| !matches!(p[0].kind(), DatumKind::String(_) | DatumKind::Symbol(_)))
        .collect();
    let mut names = gensym(
        &std::iter::repeat_n("key", expressions.len())
            .chain(["object"])
            .collect::<Vec<_>>(),
    )?;
    let object = names.pop().unwrap();
    let mut bindings = Vec::new();
    for (name, pair) in names.iter().zip(&expressions) {
        bindings.push(name.clone());
        bindings.push(pair[0].clone());
    }
    let template = format!("({{{}}})", vec!["~{}:~{}"; strings.len()].join(","));
    let mut object_form = vec![
        symbol("js*"),
        C.string(template.encode_utf16().collect::<Vec<_>>()),
    ];
    for pair in &strings {
        object_form.push(pair[0].clone());
        object_form.push(pair[1].clone());
    }
    bindings.push(object.clone());
    bindings.push(list(object_form));
    let mut result = vec![core("let"), vector(bindings)];
    for pair in &symbols {
        result.push(list(vec![
            core("unchecked-set"),
            object.clone(),
            pair[0].clone(),
            pair[1].clone(),
        ]));
    }
    for (name, pair) in names.iter().zip(&expressions) {
        result.push(list(vec![
            core("unchecked-set"),
            object.clone(),
            name.clone(),
            pair[1].clone(),
        ]));
    }
    result.push(object);
    Ok(Some(list(result)))
}

/// `macro-defmulti`, whose `&env` it reads for the namespace's name only.
fn defmulti(
    name: &Datum,
    declarations: &[Datum],
    namespace: &str,
) -> Result<Option<Datum>, String> {
    let mut rest = declarations;
    // The engine's `string?` holds for a character, which this leaves to it.
    if matches!(rest.first().map(Datum::kind), Some(DatumKind::Character(_))) {
        return Ok(None);
    }
    let doc = match rest.first().map(Datum::kind) {
        Some(DatumKind::String(_)) => {
            let doc = rest[0].clone();
            rest = &rest[1..];
            Some(doc)
        }
        _ => None,
    };
    let attributes = match rest.first().map(Datum::kind) {
        Some(DatumKind::Collection {
            kind: CollectionKind::Map,
            ..
        }) => {
            let attributes = rest[0].clone();
            rest = &rest[1..];
            Some(attributes)
        }
        _ => None,
    };
    let DatumKind::Symbol(symbol_name) = name.kind() else {
        return Ok(None);
    };
    let Some((dispatch, options)) = rest.split_first() else {
        return Ok(None);
    };
    if !options.len().is_multiple_of(2) {
        return Ok(None);
    }
    // `(apply hash-map options)`: the last value of a key wins.
    let (mut default, mut hierarchy) = (None, None);
    for option in options.as_chunks::<2>().0 {
        if is_keyword(&option[0], "default") {
            default = Some(option[1].clone());
        } else if is_keyword(&option[0], "hierarchy") {
            hierarchy = Some(option[1].clone());
        } else {
            return Ok(None);
        }
    }
    // `(merge (meta name) attributes (if doc {:doc doc} {}))`: the name's
    // own metadata where nothing is added to it, else a map of keyword keys
    // within an array map's eight entries.
    let attributes = attributes
        .map(|a| a.entries())
        .transpose()?
        .unwrap_or_default();
    let metadata = match meta(name) {
        Some(metadata) if attributes.is_empty() && doc.is_none() => metadata,
        Some(_) => return Ok(None),
        None => {
            let mut metadata: Vec<(Datum, Datum)> = Vec::new();
            let doc = doc.map(|doc| (C.keyword(Name::new(None, "doc")), doc));
            for (key, value) in attributes.into_iter().chain(doc) {
                if !matches!(key.kind(), DatumKind::Keyword(_)) {
                    return Ok(None);
                }
                match metadata.iter_mut().find(|(k, _)| k.value_eq(&key)) {
                    Some(entry) => entry.1 = value,
                    // A ninth distinct key promotes the legacy array map.
                    // Decline before doing unbounded quadratic Rust work or
                    // rebuilding its entries in the wrong iteration order.
                    None => {
                        if metadata.len() == 8 {
                            return Ok(None);
                        }
                        metadata.push((key, value));
                    }
                }
            }
            C.map(&metadata)?
        }
    };
    let qualified = match symbol_name.namespace() {
        Some(_) => return Ok(None),
        None => format!(
            "{namespace}/{}",
            String::from_utf16_lossy(symbol_name.name())
        ),
    };
    let named = name.with_metadata(metadata);
    let quoted = match qualified.find('/') {
        Some(slash) if slash >= 1 => Name::new(Some(&qualified[..slash]), &qualified[slash + 1..]),
        _ => Name::new(None, &qualified),
    };
    let atom = |value: Datum| list(vec![core("atom"), value]);
    let empty = || {
        Datum::new(DatumKind::Collection {
            kind: CollectionKind::Map,
            children: Box::new([]),
        })
    };
    Ok(Some(list(vec![
        core("defonce"),
        named,
        list(vec![
            symbol("new"),
            core("MultiFn"),
            list(vec![symbol("quote"), C.symbol(quoted)]),
            dispatch.clone(),
            default.unwrap_or_else(|| C.keyword(Name::new(None, "default"))),
            hierarchy.unwrap_or_else(|| list(vec![core("get-global-hierarchy")])),
            atom(empty()),
            atom(empty()),
            atom(empty()),
            atom(C.nil()),
        ]),
    ])))
}

/// The syntax-quote gensyms (`x#`) of `sequence-for` and `sequence-doseq`
/// in src/compiler_sequence_macros.cljs, as the bootstrap reads them in its
/// scope of generated names ([`crate::generated_names::BOOTSTRAP_SCOPE`]),
/// the same in every expansion. The sequence macro tests fail if the
/// definitions come to read others.
const FOR_NESTED: &str = "nested__0_1__auto__";
const FOR_INNER: &str = "inner__0_2__auto__";
const FOR_CHUNK: &str = "c__0_3__auto__";
const FOR_SIZE: &str = "size__0_4__auto__";
const FOR_ITERATOR: &str = "iter__0_5__auto__";
const DOSEQ_CHUNK: &str = "c__0_6__auto__";

/// The names an expansion allocates in order: a first pass with `dry`
/// records their prefixes, so that one `gensym` call allocates them, and a
/// second pass takes them. A pass that builds only a `doseq` expansion's
/// [`Shape`] allocates none: each name is its prefix.
struct Names {
    dry: Option<Vec<&'static str>>,
    allocated: std::vec::IntoIter<Datum>,
    shape: Option<Shape>,
}
/// What [`doseq_shape`] keeps while it builds: the steps it built, by the
/// pairs left and the `recur` form they end in (none, a binding's or its
/// chunk loop's), which the names no longer tell apart, so that each is
/// built once and shared; and how the definition splices the lists that
/// splice ([`Piece`]), by their identity.
#[derive(Default)]
struct Shape {
    steps: std::collections::HashMap<(usize, u8), (bool, Datum)>,
    pieces: Pieces,
}
/// The lists of an expansion that splice, by identity, with their pieces.
type Pieces = std::collections::HashMap<usize, Vec<Piece>>;
impl Names {
    /// Note that the definition builds `list` from `pieces`.
    fn splice(&mut self, list: &Datum, pieces: Vec<Piece>) {
        if let Some(shape) = &mut self.shape {
            shape.pieces.insert(list.identity(), pieces);
        }
    }
    fn next(&mut self, prefix: &'static str) -> Datum {
        if self.shape.is_some() {
            return symbol(prefix);
        }
        match &mut self.dry {
            Some(prefixes) => {
                prefixes.push(prefix);
                symbol(prefix)
            }
            None => self.allocated.next().expect("an allocated name"),
        }
    }
}

/// A number the engine creates, which it returns as a double.
fn number(value: f64) -> Datum {
    C.double(value)
}

fn modifier(datum: &Datum) -> Option<&'static str> {
    ["let", "while", "when"]
        .into_iter()
        .find(|name| is_keyword(datum, name))
}

/// A binding of `for`, its expression and its modifiers.
type Group<'a> = (&'a Datum, &'a Datum, Vec<(&'static str, &'a Datum)>);

/// `sequence-for`.
fn sequence_for(
    bindings: &Datum,
    body: &Datum,
    gensym: Gensym<'_>,
) -> Result<Option<Datum>, String> {
    let pairs = children(bindings);
    // More bindings than conversion levels expand past them in any case.
    if !is_vector(bindings) || pairs.len() / 2 > DEPTH {
        return Ok(None);
    }
    for_expansion(pairs, body, gensym)
}

/// The expansion of `sequence-for` of the binding pairs `pairs`, or `None`
/// where the definition rejects them.
fn for_expansion(
    pairs: &[Datum],
    body: &Datum,
    gensym: Gensym<'_>,
) -> Result<Option<Datum>, String> {
    if !pairs.len().is_multiple_of(2) {
        return Ok(None);
    }
    // Each binding and its modifiers; a modifier first pops no group. With
    // no binding, the definition destructures one of nil from nothing.
    let nil = C.nil();
    let mut groups: Vec<Group<'_>> = Vec::new();
    if pairs.is_empty() {
        groups.push((&nil, &nil, Vec::new()));
    }
    for pair in pairs.as_chunks::<2>().0 {
        if matches!(pair[0].kind(), DatumKind::Keyword(_)) {
            let (Some(group), Some(name)) = (groups.last_mut(), modifier(&pair[0])) else {
                return Ok(None);
            };
            group.2.push((name, &pair[1]));
        } else {
            groups.push((&pair[0], &pair[1], Vec::new()));
        }
    }
    let prefixes = ["iter", "seq", "index", "buffer"];
    let names = gensym(&prefixes.repeat(groups.len()))?;
    fn emit(groups: &[Group<'_>], body: &Datum, names: &mut std::vec::IntoIter<Datum>) -> Datum {
        let (bind, _, mods) = &groups[0];
        let mut level = names.by_ref().take(4);
        let (Some(iter), Some(s), Some(i), Some(b)) =
            (level.next(), level.next(), level.next(), level.next())
        else {
            unreachable!("four names a binding");
        };
        let more = groups.get(1);
        let nested = more.map(|_| emit(&groups[1..], body, names));
        let rest = || list(vec![core("rest"), s.clone()]);
        let base = match (more, nested) {
            (Some((_, next_expr, _)), Some(nested)) => list(vec![
                core("let"),
                vector(vec![
                    symbol(FOR_NESTED),
                    nested,
                    symbol(FOR_INNER),
                    list(vec![
                        core("seq"),
                        list(vec![symbol(FOR_NESTED), (*next_expr).clone()]),
                    ]),
                ]),
                list(vec![
                    symbol("if"),
                    symbol(FOR_INNER),
                    list(vec![
                        core("concat"),
                        symbol(FOR_INNER),
                        list(vec![iter.clone(), rest()]),
                    ]),
                    list(vec![symbol("recur"), rest()]),
                ]),
            ]),
            _ => list(vec![
                core("cons"),
                body.clone(),
                list(vec![iter.clone(), rest()]),
            ]),
        };
        let wrap =
            |body: Datum, (name, value): &(&'static str, &Datum), skip: &dyn Fn() -> Datum| {
                let value = (*value).clone();
                match *name {
                    "let" => list(vec![core("let"), value, body]),
                    "while" => list(vec![core("when"), value, body]),
                    _ => list(vec![symbol("if"), value, body, skip()]),
                }
            };
        let skip = || list(vec![symbol("recur"), rest()]);
        let normal = mods.iter().rev().fold(base, |body, m| wrap(body, m, &skip));
        let increment = || list(vec![core("inc"), i.clone()]);
        let chunk_base = list(vec![
            symbol("do"),
            list(vec![
                C.symbol(Name::new(Some("clojure.core"), "chunk-append")),
                b.clone(),
                body.clone(),
            ]),
            list(vec![symbol("recur"), increment()]),
        ]);
        let skip_chunk = || list(vec![symbol("recur"), increment()]);
        let chunk_body = mods
            .iter()
            .rev()
            .fold(chunk_base, |body, m| wrap(body, m, &skip_chunk));
        let clojure = |name| C.symbol(Name::new(Some("clojure.core"), name));
        let looped = if more.is_some() {
            list(vec![
                core("when-first"),
                vector(vec![(*bind).clone(), s.clone()]),
                normal,
            ])
        } else {
            let chunk = list(vec![clojure("chunk"), b.clone()]);
            list(vec![
                core("when-let"),
                vector(vec![s.clone(), list(vec![core("seq"), s.clone()])]),
                list(vec![
                    symbol("if"),
                    list(vec![clojure("chunked-seq?"), s.clone()]),
                    list(vec![
                        core("let"),
                        vector(vec![
                            symbol(FOR_CHUNK),
                            list(vec![clojure("chunk-first"), s.clone()]),
                            symbol(FOR_SIZE),
                            list(vec![core("count"), symbol(FOR_CHUNK)]),
                            b.clone(),
                            list(vec![clojure("chunk-buffer"), symbol(FOR_SIZE)]),
                        ]),
                        list(vec![
                            symbol("if"),
                            list(vec![
                                core("boolean"),
                                list(vec![
                                    core("loop"),
                                    vector(vec![i.clone(), number(0.0)]),
                                    list(vec![
                                        symbol("if"),
                                        list(vec![core("<"), i.clone(), symbol(FOR_SIZE)]),
                                        list(vec![
                                            core("let"),
                                            vector(vec![
                                                (*bind).clone(),
                                                list(vec![
                                                    core("-nth"),
                                                    symbol(FOR_CHUNK),
                                                    i.clone(),
                                                ]),
                                            ]),
                                            chunk_body,
                                        ]),
                                        C.boolean(true),
                                    ]),
                                ]),
                            ]),
                            list(vec![
                                clojure("chunk-cons"),
                                chunk.clone(),
                                list(vec![
                                    iter.clone(),
                                    list(vec![clojure("chunk-rest"), s.clone()]),
                                ]),
                            ]),
                            list(vec![clojure("chunk-cons"), chunk, C.nil()]),
                        ]),
                    ]),
                    list(vec![
                        core("let"),
                        vector(vec![(*bind).clone(), list(vec![core("first"), s.clone()])]),
                        normal,
                    ]),
                ]),
            ])
        };
        list(vec![
            core("fn"),
            iter,
            vector(vec![s.clone()]),
            list(vec![
                core("lazy-seq"),
                list(vec![
                    core("loop"),
                    vector(vec![s.clone(), s.clone()]),
                    looped,
                ]),
            ]),
        ])
    }
    let expansion = emit(&groups, body, &mut names.into_iter());
    Ok(Some(list(vec![
        core("let"),
        vector(vec![symbol(FOR_ITERATOR), expansion]),
        list(vec![
            symbol(FOR_ITERATOR),
            pairs.get(1).cloned().unwrap_or_else(|| C.nil()),
        ]),
    ])))
}

/// The most names a native `doseq` allocates: its expansion grows with
/// them, doubling at each nested binding.
const DOSEQ_NAMES: usize = 1024;

/// The binding pairs of a `doseq` call the definition accepts, or `None`
/// for bindings that are not a vector of pairs or name a keyword it
/// rejects.
fn doseq_pairs(bindings: &Datum) -> Option<&[Datum]> {
    let exprs = children(bindings);
    let valid = is_vector(bindings)
        && exprs.len().is_multiple_of(2)
        && !exprs
            .iter()
            .step_by(2)
            .any(|k| matches!(k.kind(), DatumKind::Keyword(_)) && modifier(k).is_none());
    valid.then_some(exprs)
}

/// The pairs of a `for` or `doseq` of more pairs than conversion levels
/// that decide what the engine's conversion of its expansion refuses,
/// which nests past [`DEPTH`] levels: each pair nests the forms of the
/// pairs after it a level deeper at least, and the conversion visits a
/// form's first item and so the forms that nest deepest before the items
/// that follow them, so that it refuses the expansion of these pairs, down
/// to a level past [`DEPTH`], where it refuses the whole.
#[cfg(all(test, light_v8))]
fn deep_pairs(pairs: &[Datum]) -> &[Datum] {
    &pairs[..pairs.len().min(2 * (DEPTH + 2))]
}

/// `sequence-doseq` leaves large expansions to the engine.
fn sequence_doseq(
    bindings: &Datum,
    body: &[Datum],
    gensym: Gensym<'_>,
) -> Result<Option<Datum>, String> {
    // Its construction recurses once a pair, as the definition's does:
    // more pairs than conversion levels are left to the engine.
    let Some(exprs) = doseq_pairs(bindings).filter(|exprs| exprs.len() / 2 <= DEPTH) else {
        return Ok(None);
    };
    let mut allocated = 0usize;
    for k in exprs.iter().step_by(2).rev() {
        allocated = if matches!(k.kind(), DatumKind::Keyword(_)) {
            allocated + 1
        } else {
            allocated.saturating_mul(2).saturating_add(4)
        };
    }
    if allocated > DOSEQ_NAMES {
        return Ok(None);
    }
    let mut dry = Names {
        dry: Some(Vec::new()),
        allocated: Vec::new().into_iter(),
        shape: None,
    };
    doseq_step(&mut dry, None, 0, exprs, body);
    let prefixes = dry.dry.unwrap();
    let allocated = if prefixes.is_empty() {
        Vec::new()
    } else {
        gensym(&prefixes)?
    };
    let mut names = Names {
        dry: None,
        allocated: allocated.into_iter(),
        shape: None,
    };
    Ok(Some(doseq_step(&mut names, None, 0, exprs, body).1))
}

/// The shape of the expansion of a `doseq` with the binding `bindings` and
/// body `body` (`None` where [`doseq_pairs`] declines it, and of its
/// [`deep_pairs`] only), which the emulation of the engine's conversion
/// walks ([`wide_sequence`]): its forms
/// with each name its prefix, so that the two steps the definition builds
/// at each binding, which differ only in names, are one shared subtree and
/// the shape takes room in its pairs rather than in its expansion's size;
/// and the pieces of its lists that splice.
#[cfg(all(test, light_v8))]
fn doseq_shape(bindings: &Datum, body: &[Datum]) -> Option<(Datum, Pieces)> {
    let exprs = deep_pairs(doseq_pairs(bindings)?);
    let mut names = Names {
        dry: None,
        allocated: Vec::new().into_iter(),
        shape: Some(Shape::default()),
    };
    let (_, form) = doseq_step(&mut names, None, 0, exprs, body);
    Some((form, names.shape.unwrap().pieces))
}

/// The definition's `step` on the pairs `exprs`, whose `recur` form is
/// `recform`, of the kind `ends` (none, a binding's or its chunk loop's,
/// which a [`Shape`] tells apart): whether the form needs its caller's
/// `recur`, and the form.
fn doseq_step(
    names: &mut Names,
    recform: Option<Datum>,
    ends: u8,
    exprs: &[Datum],
    body: &[Datum],
) -> (bool, Datum) {
    if let Some(built) = names
        .shape
        .as_ref()
        .and_then(|shape| shape.steps.get(&(exprs.len(), ends)))
    {
        return built.clone();
    }
    let step = doseq_step_built(names, recform, ends, exprs, body);
    if let Some(shape) = &mut names.shape {
        shape.steps.insert((exprs.len(), ends), step.clone());
    }
    step
}

/// `(form... ~@(when needrec [recform]))`, the lists of [`doseq_step`] that
/// end with the `recur` form where their step needs it.
fn doseq_again(names: &mut Names, needrec: bool, form: Vec<Datum>, recform: &Datum) -> Datum {
    let mut pieces = vec![Piece::One; form.len()];
    pieces.push(if needrec {
        Piece::Vector(1)
    } else {
        Piece::Empty
    });
    let mut form = form;
    if needrec {
        form.push(recform.clone());
    }
    let form = list(form);
    names.splice(&form, pieces);
    form
}

/// [`doseq_step`], building the step.
fn doseq_step_built(
    names: &mut Names,
    recform: Option<Datum>,
    ends: u8,
    exprs: &[Datum],
    body: &[Datum],
) -> (bool, Datum) {
    let Some((k, rest)) = exprs.split_first() else {
        let mut done = vec![symbol("do")];
        done.extend(body.iter().cloned());
        done.push(C.nil());
        let done = list(done);
        names.splice(&done, vec![Piece::One, Piece::Rest(body.len()), Piece::One]);
        return (true, done);
    };
    let v = &rest[0];
    let seq = names.next("seq__");
    let keyword = matches!(k.kind(), DatumKind::Keyword(_));
    let (recform, ends) = if keyword {
        (recform, ends)
    } else {
        let recform = list(vec![
            symbol("recur"),
            list(vec![core("next"), seq.clone()]),
            C.nil(),
            number(0.0),
            number(0.0),
        ]);
        (Some(recform), 1)
    };
    let (needrec, subform) = doseq_step(names, recform.clone(), ends, &rest[1..], body);
    let recform_datum = || recform.clone().unwrap_or_else(|| C.nil());
    match modifier(k) {
        Some("let") => (needrec, list(vec![core("let"), v.clone(), subform])),
        Some("while") => (
            false,
            doseq_again(
                names,
                needrec,
                vec![core("when"), v.clone(), subform],
                &recform_datum(),
            ),
        ),
        Some(_) => (
            false,
            list(vec![
                symbol("if"),
                v.clone(),
                doseq_again(
                    names,
                    needrec,
                    vec![symbol("do"), subform],
                    &recform_datum(),
                ),
                recform_datum(),
            ]),
        ),
        None => {
            let tag = C
                .map(&[(C.keyword(Name::new(None, "tag")), symbol("not-native"))])
                .expect("a map");
            let chunk = names.next("chunk__");
            let chunk = chunk.with_metadata(tag);
            let count = names.next("count__");
            let i = names.next("i__");
            let recform_chunk = list(vec![
                symbol("recur"),
                seq.clone(),
                chunk.clone(),
                count.clone(),
                list(vec![core("inc"), i.clone()]),
            ]);
            let (_, subform_chunk) =
                doseq_step(names, Some(recform_chunk.clone()), 2, &rest[1..], body);
            let clojure = |name| C.symbol(Name::new(Some("clojure.core"), name));
            let recform = recform_datum();
            let chunk_body = doseq_again(
                names,
                needrec,
                vec![
                    core("let"),
                    vector(vec![
                        k.clone(),
                        list(vec![core("-nth"), chunk.clone(), i.clone()]),
                    ]),
                    subform_chunk,
                ],
                &recform_chunk,
            );
            let seq_body = doseq_again(
                names,
                needrec,
                vec![
                    core("let"),
                    vector(vec![k.clone(), list(vec![core("first"), seq.clone()])]),
                    subform,
                ],
                &recform,
            );
            (
                true,
                list(vec![
                    core("loop"),
                    vector(vec![
                        seq.clone(),
                        list(vec![core("seq"), v.clone()]),
                        chunk.clone(),
                        C.nil(),
                        count.clone(),
                        number(0.0),
                        i.clone(),
                        number(0.0),
                    ]),
                    list(vec![
                        symbol("if"),
                        list(vec![
                            core("boolean"),
                            list(vec![core("<"), i.clone(), count.clone()]),
                        ]),
                        chunk_body,
                        list(vec![
                            core("when-let"),
                            vector(vec![seq.clone(), list(vec![core("seq"), seq.clone()])]),
                            list(vec![
                                symbol("if"),
                                list(vec![clojure("chunked-seq?"), seq.clone()]),
                                list(vec![
                                    core("let"),
                                    vector(vec![
                                        symbol(DOSEQ_CHUNK),
                                        list(vec![clojure("chunk-first"), seq.clone()]),
                                    ]),
                                    list(vec![
                                        symbol("recur"),
                                        list(vec![clojure("chunk-rest"), seq.clone()]),
                                        symbol(DOSEQ_CHUNK),
                                        list(vec![core("count"), symbol(DOSEQ_CHUNK)]),
                                        number(0.0),
                                    ]),
                                ]),
                                seq_body,
                            ]),
                        ]),
                    ]),
                ]),
            )
        }
    }
}

/// The levels a form, its metadata a level below it, may nest in the
/// engine's conversions of a macro call and of its expansion, which fail
/// where it nests deeper ([`crate::form_bridge::ConversionLimits`]): a call
/// that this module would expand where a conversion fails is left to the
/// engine, which fails as it does.
const DEPTH: usize = 128;

/// Whether `datum` is a large integer literal, which the engine's
/// conversions take as a number, without its payload.
fn big_integer(datum: &Datum) -> bool {
    matches!(
        datum.kind(),
        DatumKind::CompilerLiteral {
            kind: crate::compiler_data::CompilerLiteralKind::BigInteger,
            ..
        }
    )
}

/// The nodes the engine's conversion of `datum` takes, as the bridges count
/// them, or `None` if it nests deeper than `room` levels.
fn within(datum: &Datum, room: usize) -> Option<usize> {
    if room == 0 {
        return None;
    }
    let mut nodes = 1usize;
    if let Some(metadata) = datum.metadata() {
        nodes = nodes.saturating_add(within(&metadata, room - 1)?);
    }
    match datum.kind() {
        DatumKind::Collection { children, .. } => {
            for child in children {
                nodes = nodes.saturating_add(within(child, room - 1)?);
            }
        }
        DatumKind::CompilerLiteral { payload, .. } if !big_integer(datum) => {
            nodes = nodes.saturating_add(within(payload, room - 1)?);
        }
        _ => {}
    }
    Some(nodes)
}

/// Where a syntax-quoted list of a definition takes its items from, as
/// the engine builds it: `(seq (concat (list a) b ...))`, so that its
/// lazy sequences, which the engine's conversion back realizes and
/// charges, follow from its pieces ([`quoted_steps`]).
#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(all(test, light_v8)), allow(dead_code))]
enum Piece {
    /// `~x`, or a form: one item.
    One,
    /// `~@x` of `nil`.
    Empty,
    /// `~@body`: the macro's rest argument, of so many items.
    Rest(usize),
    /// `~@[x ...]`: a vector of so many items.
    Vector(usize),
}

/// What the engine's conversion charges, beyond its nodes, to read a
/// syntax-quoted list of `pieces` (js_runtime.js, `encode`): each item
/// takes a step of the sequence, then the lazy sequences that reaching it
/// realizes; the end takes a step and the lazy sequences after the last
/// item. These are the lazy sequences for each item, and those at the end.
///
/// `concat` of one piece is the piece, and of two the lazy sequence
/// `concatTwo` builds, whose `seq` the list is: its first item is read,
/// the first of the second piece behind one lazy sequence, and the second
/// piece's rest as that piece. Of more, `concatMany` over `concatTwo` of
/// the first two: after the first item, each item of a piece is behind one
/// lazy sequence (none for the rest of a vector's chunk), and the first of
/// a piece after the second behind one more for each piece it passed into
/// on its way; the end is behind one, and one for each piece after the
/// last item.
#[cfg(all(test, light_v8))]
fn quoted_steps(pieces: &[Piece]) -> (Vec<u32>, u32) {
    let length = |piece: &Piece| match *piece {
        Piece::One => 1,
        Piece::Empty => 0,
        Piece::Rest(n) | Piece::Vector(n) => n,
    };
    let mut lazies = Vec::new();
    if pieces.len() <= 2 {
        // Only templates of single items splice two pieces or fewer.
        debug_assert!(pieces.iter().all(|piece| matches!(piece, Piece::One)));
        lazies.extend((0..pieces.len() as u32).map(|index| index.min(1)));
        return (lazies, 0);
    }
    let mut entered = 0;
    for (index, piece) in pieces.iter().enumerate() {
        if index >= 2 {
            entered += 1;
        }
        for item in 0..length(piece) {
            lazies.push(if lazies.is_empty() {
                0
            } else if item == 0 {
                1 + entered
            } else {
                u32::from(!matches!(piece, Piece::Vector(_)))
            });
            entered = 0;
        }
    }
    if lazies.is_empty() {
        // `(sequence nil)`: the empty list.
        return (lazies, 0);
    }
    (lazies, 1 + entered)
}

/// How the engine holds the lists of a form it converts back, which
/// decides what reading them charges beyond their nodes.
#[cfg(all(test, light_v8))]
#[derive(Clone, Copy)]
enum Lists<'a> {
    /// Uncounted: what the conversion charges for the nodes alone.
    #[allow(dead_code)]
    Nodes,
    /// Decoded from compiler data, as a call is: each item takes a step,
    /// and the end one more.
    Decoded,
    /// An expansion of a definition that builds every list of its own with
    /// syntax quote: the call's data (`arguments`, by identity) as
    /// decoded, and its own lists as [`quoted_steps`] says, of `pieces`
    /// where they splice and of single items elsewhere.
    Quoted {
        arguments: &'a std::collections::HashSet<usize>,
        pieces: &'a Pieces,
    },
}

/// Why an emulated conversion stops.
#[cfg(all(test, light_v8))]
#[derive(Debug, PartialEq)]
enum Stop {
    /// What the engine's conversion refuses, and where, as it reports it.
    Refused(String),
    /// Where the engine's order is its hashes' (a set, or a map of more
    /// than eight entries, or keys it may merge), or a form it converts
    /// in ways this does not follow: what it refuses cannot be told.
    Unknown,
}

/// The engine's conversion of a form back to compiler data under the node
/// limit (js_runtime.js `compilerFormBridge`, `encode`), emulated: it
/// visits each node before its children (a map's keys before their
/// values) and its metadata last, charging every node and every step of a
/// sequence ([`Lists`]) to `remaining`, and refuses the first charge past
/// it (`node limit`, `sequence limit` or, for a lazy sequence,
/// `lazy realization failure`, at the path of the node or the list), a
/// node deeper than [`DEPTH`] (`depth limit`), and a missing argument
/// (`undefined`, [`MISSING`]).
#[cfg(all(test, light_v8))]
struct Conversion<'a> {
    remaining: usize,
    lists: Lists<'a>,
}
#[cfg(all(test, light_v8))]
impl Conversion<'_> {
    /// What converting `datum` refuses under the node limit `nodes`, if
    /// anything.
    fn run(datum: &Datum, nodes: usize, lists: Lists<'_>) -> Result<(), Stop> {
        let mut conversion = Conversion {
            remaining: nodes,
            lists,
        };
        conversion.visit(datum, &mut "$".to_owned(), 0)
    }
    fn charge(&mut self, what: &str, path: &str) -> Result<(), Stop> {
        match self.remaining.checked_sub(1) {
            Some(remaining) => {
                self.remaining = remaining;
                Ok(())
            }
            None => Err(Stop::Refused(format!("{what} at {path}"))),
        }
    }
    /// Visit `datum`, a child of the node at `path` and `depth`, at the
    /// path that `part` extends it with.
    fn child(
        &mut self,
        path: &mut String,
        depth: usize,
        part: std::fmt::Arguments<'_>,
        datum: &Datum,
    ) -> Result<(), Stop> {
        let length = path.len();
        std::fmt::Write::write_fmt(path, part).expect("a string");
        let visited = self.visit(datum, path, depth + 1);
        path.truncate(length);
        visited
    }
    fn visit(&mut self, datum: &Datum, path: &mut String, depth: usize) -> Result<(), Stop> {
        self.charge("node limit", path)?;
        if depth > DEPTH {
            return Err(Stop::Refused(format!("depth limit at {path}")));
        }
        match datum.kind() {
            DatumKind::Symbol(name) if *name == Name::new(None, MISSING) => {
                return Err(Stop::Refused(format!("undefined at {path}")));
            }
            DatumKind::Collection { kind, children } => match kind {
                CollectionKind::List | CollectionKind::AnonymousFn => {
                    let steps = match self.lists {
                        Lists::Nodes => None,
                        Lists::Decoded => Some((vec![0; children.len()], 0)),
                        Lists::Quoted { arguments, pieces } => {
                            Some(if arguments.contains(&datum.identity()) {
                                (vec![0; children.len()], 0)
                            } else if let Some(pieces) = pieces.get(&datum.identity()) {
                                quoted_steps(pieces)
                            } else {
                                quoted_steps(&vec![Piece::One; children.len()])
                            })
                        }
                    };
                    for (index, item) in children.iter().enumerate() {
                        if let Some((lazies, _)) = &steps {
                            self.charge("sequence limit", path)?;
                            for _ in 0..lazies[index] {
                                self.charge("lazy realization failure", path)?;
                            }
                        }
                        self.child(path, depth, format_args!("[{index}]"), item)?;
                    }
                    if let Some((_, end)) = steps {
                        self.charge("sequence limit", path)?;
                        for _ in 0..end {
                            self.charge("lazy realization failure", path)?;
                        }
                    }
                }
                CollectionKind::Vector => {
                    for (index, item) in children.iter().enumerate() {
                        self.child(path, depth, format_args!("[{index}]"), item)?;
                    }
                }
                CollectionKind::Map | CollectionKind::Set => {
                    let map = *kind == CollectionKind::Map;
                    // The engine decodes keys it finds equal into one entry.
                    let Some(entries) = engine_entries(children, map) else {
                        return Err(Stop::Unknown);
                    };
                    if map && children.len() <= 16 {
                        // An array map of up to eight keys keeps their order.
                        for (index, (key, value)) in entries.iter().enumerate() {
                            self.child(path, depth, format_args!(".key[{index}]"), key)?;
                            if let Some(value) = value {
                                self.child(path, depth, format_args!(".value[{index}]"), value)?;
                            }
                        }
                    } else {
                        // In the engine's order only the whole is known.
                        let mut whole = Conversion {
                            remaining: self.remaining,
                            lists: self.lists,
                        };
                        for item in entries
                            .iter()
                            .flat_map(|(key, value)| std::iter::once(*key).chain(*value))
                        {
                            whole
                                .visit(item, path, depth + 1)
                                .map_err(|_| Stop::Unknown)?;
                        }
                        self.remaining = whole.remaining;
                    }
                }
            },
            DatumKind::CompilerLiteral { kind, payload } => {
                use crate::compiler_data::CompilerLiteralKind as K;
                match kind {
                    K::BigInteger => {}
                    K::Regex | K::Uuid => {
                        self.child(path, depth, format_args!(".payload"), payload)?
                    }
                    _ => return Err(Stop::Unknown),
                }
            }
            _ => {}
        }
        if let Some(metadata) = datum.metadata() {
            self.child(path, depth, format_args!(".meta"), &metadata)?;
        }
        Ok(())
    }
}

/// The entries the engine decodes a map's `children` (keys and values
/// in turn, `map`) or a set's into: a key it finds equal to an earlier one
/// keeps the earlier key and its place and replaces the value. `None`
/// where it may find keys equal that this cannot tell: two collections, as
/// it compares them by value, or two `NaN`s. Numbers it compares as
/// numbers (`1` and `1.0`, `0` and `-0.0` are one key), characters and
/// strings as strings, symbols by name.
#[cfg(all(test, light_v8))]
fn engine_entries(children: &[Datum], map: bool) -> Option<Vec<(&Datum, Option<&Datum>)>> {
    #[derive(PartialEq, Eq, Hash)]
    enum Key {
        Primitive(Primitive),
        Symbol(Name),
        Other,
    }
    let key = |datum: &Datum| match (primitive(datum), datum.kind()) {
        (Some(Primitive::Number(bits)), _) if f64::from_bits(bits) == 0.0 => {
            Key::Primitive(Primitive::Number(0f64.to_bits()))
        }
        (Some(value), _) => Key::Primitive(value),
        (None, DatumKind::Symbol(name)) => Key::Symbol(name.clone()),
        _ => Key::Other,
    };
    let nan = Key::Primitive(Primitive::Number(f64::NAN.to_bits()));
    // Each key's entry, by the key.
    let mut keys: std::collections::HashMap<Key, usize> = std::collections::HashMap::new();
    let mut entries: Vec<(&Datum, Option<&Datum>)> = Vec::new();
    for pair in children.chunks(if map { 2 } else { 1 }) {
        let k = key(&pair[0]);
        match keys.entry(k) {
            std::collections::hash_map::Entry::Occupied(seen)
                if *seen.key() == Key::Other || *seen.key() == nan =>
            {
                return None;
            }
            std::collections::hash_map::Entry::Occupied(seen) => {
                entries[*seen.get()].1 = pair.get(1);
            }
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(entries.len());
                entries.push((&pair[0], pair.get(1)));
            }
        }
    }
    Some(entries)
}

/// What the V8 build's engine makes of the call `call` of `doseq`, which
/// [`expand`] declines for the names its expansion would allocate (or
/// would expand, below that) or for its pairs, or of `for`, which it
/// declines for its pairs, under the node limit `nodes`: it converts the
/// call, takes a snapshot of it (the call converted back, as decoded),
/// runs the definition and converts the expansion back.
#[cfg(all(test, light_v8))]
fn wide_sequence(name: &str, call: &Datum, nodes: usize) -> Result<(), Stop> {
    let args = children(call).get(1..).unwrap_or_default();
    let Some((bindings, body)) = args.split_first() else {
        return Err(Stop::Unknown);
    };
    let deep = children(bindings).len() / 2 > DEPTH;
    let shape = match name {
        "doseq" => doseq_shape(bindings, body),
        // `for` has a native expansion of up to that many pairs.
        "for" if deep && is_vector(bindings) => {
            let mut prefixes =
                |prefixes: &[&str]| Ok(prefixes.iter().map(|prefix| symbol(prefix)).collect());
            let body = body.first().cloned().unwrap_or_else(|| symbol(MISSING));
            for_expansion(deep_pairs(children(bindings)), &body, &mut prefixes)
                .ok()
                .flatten()
                .map(|form| (form, Pieces::new()))
        }
        _ => None,
    };
    let Some((shape, pieces)) = shape else {
        return Err(Stop::Unknown);
    };
    Conversion::run(call, nodes, Lists::Decoded)?;
    let mut arguments = Arguments::new(args).identities;
    arguments.insert(call.identity());
    let converted = Conversion::run(
        &shape,
        nodes,
        Lists::Quoted {
            arguments: &arguments,
            pieces: &pieces,
        },
    );
    if deep && converted.is_ok() {
        return Err(Stop::Unknown);
    }
    converted
}

/// The names the compile-time runtime's `gensym` gives, one for each of
/// `prefixes` in order, in this thread's innermost scope of generated
/// names, allocated from the scope's count without a runtime
/// ([`crate::generated_names::gensym`], light-cljs-432).
fn scope_gensyms(prefixes: &[&str]) -> Result<Vec<Datum>, String> {
    prefixes
        .iter()
        .map(|prefix| {
            debug_assert!(prefix.bytes().all(|b| b.is_ascii_alphabetic() || b == b'_'));
            crate::generated_names::gensym(prefix)
                .map(|name| symbol(&name))
                .ok_or_else(|| "gensym outside every scope of generated names".to_owned())
        })
        .collect()
}

/// What the call `call` of the core macro `name` in `namespace` expands to
/// here, with stand-ins for the names it would allocate, if
/// [`crate::MacroHost::expand_core_native`] expands it rather than leaving
/// it to the engine under the node limit `nodes`: `None` exactly where it
/// would decline the call. The analyzer's check of macro-free units walks
/// it in the call's place (light-cljs-432), so that a unit it admits never
/// leaves a call to the engine. Its shape does not depend on the names: a
/// stand-in equals nothing but itself, as an allocated name equals nothing
/// a source spells ([`crate::generated_names::stand_in`]), so an expansion
/// that compares symbols, as `js-obj` does its keys, decides alike.
pub(crate) fn native_expansion(
    name: &str,
    call: &Datum,
    namespace: &str,
    nodes: usize,
) -> Option<Datum> {
    if !crate::generated_names::scoped() || within(call, DEPTH).is_none_or(|count| count > nodes) {
        return None;
    }
    let mut stand_ins = |prefixes: &[&str]| -> Result<Vec<Datum>, String> {
        Ok(prefixes
            .iter()
            .map(|prefix| symbol(&crate::generated_names::stand_in(prefix)))
            .collect())
    };
    let expanded = expand(name, call, namespace, &mut stand_ins).ok()?;
    // As [`crate::MacroHost::expand_core_native`] expands it.
    let expanded = expanded?;
    within(&expanded, DEPTH)?;
    Some(expanded)
}

/// What stands for an argument the call of a core macro leaves out, which
/// its definition receives as `undefined`: a symbol no source spells.
#[cfg(all(test, light_v8))]
const MISSING: &str = "\u{0}undefined";

impl crate::MacroHost {
    /// Expand the call `form` of the core macro `name`, spelled as `site`
    /// gives it at its span, natively ([`expand`]), charged as the engine
    /// charges an expansion; `None` if the engine expands it instead. In a
    /// scope of generated names, it draws names from the scope's count.
    pub(crate) fn expand_core_native(
        &mut self,
        name: &str,
        form: &crate::compiler_data::Form,
        namespace: &str,
        site: (&str, &std::ops::Range<usize>),
    ) -> Result<Option<crate::compiler_data::Form>, String> {
        if !crate::generated_names::scoped() {
            return Ok(None);
        }
        let exempt = self.budget_exempt();
        let call = form.data();
        // A call the engine's conversion refuses, too deep or larger than the
        // node limit (which each conversion may take whole), is left to it,
        // which fails as it does; so is never one it converts.
        if within(&call, DEPTH).is_none_or(|nodes| nodes > self.conversion_node_limit()) {
            return Ok(None);
        }
        if !exempt && self.engine.expansions.get() >= self.limits.expansions {
            return Err("compiler macro expansion limit exceeded".into());
        }
        // The room the engine's conversion of the call needs, as it notes
        // it ([`crate::macro_host::MacroHost::expand_javascript`]), so a
        // namespace's accounting admits it where this expands it. Its
        // environment is never converted here.
        if !exempt && self.measures_work() {
            self.note_node_room(crate::macro_host::WorkMarks::nodes(&call));
        }
        let Some(expanded) = expand(name, &call, namespace, &mut scope_gensyms)? else {
            return Ok(None);
        };
        // Its names are allocated: the engine allocates others before it
        // fails to convert the expansion back.
        if within(&expanded, DEPTH).is_none() {
            return Ok(None);
        }
        let mut arguments = Arguments::new(&children(&call)[1..]);
        arguments.identities.insert(call.identity());
        let expanded = arguments.returned(&expanded);
        let expanded = crate::compiler_data::Form::from_expansion(
            expanded,
            form,
            Name::new(Some("cljs.core"), name),
        );
        let nodes = expanded.nodes().len();
        let (operator, span) = site;
        crate::macro_host::record_native_expansion(|| crate::macro_host::NativeExpansion {
            nodes,
            operator: operator.to_owned(),
            span: span.clone(),
            exempt,
        });
        if exempt {
            self.engine
                .exempt_expansions
                .set(self.engine.exempt_expansions.get() + 1);
            self.engine
                .exempt_nodes
                .set(self.engine.exempt_nodes.get().saturating_add(nodes));
        } else {
            self.engine.expansions.set(self.engine.expansions.get() + 1);
            self.engine
                .generated_nodes
                .set(self.engine.generated_nodes.get().saturating_add(nodes));
            if self.engine.generated_nodes.get() > self.limits.generated_nodes {
                return Err("compiler macro generated-node limit exceeded".into());
            }
        }
        Ok(Some(expanded))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MacroHost;
    use crate::compiler_data::{Form, FormKind, Source};

    /// Why `native` is not the form `script` is, or `None` if it is the
    /// same: the same data, metadata, spans and origins throughout.
    fn difference(native: &Form, script: &Form, path: &str) -> Option<String> {
        let (n, s) = (native.data(), script.data());
        let shallow = |d: &Datum| match d.kind() {
            DatumKind::Collection { kind, children } => format!("{kind:?}/{}", children.len()),
            other => format!("{other:?}"),
        };
        if shallow(&n) != shallow(&s) {
            return Some(format!("{path}: {} vs {}", shallow(&n), shallow(&s)));
        }
        if native.span().bytes() != script.span().bytes() {
            return Some(format!(
                "{path} {}: span {:?} vs {:?}",
                shallow(&n),
                native.span().bytes(),
                script.span().bytes()
            ));
        }
        if native.node().origin != script.node().origin {
            return Some(format!(
                "{path} {}: origin {:?} vs {:?}",
                shallow(&n),
                native.node().origin,
                script.node().origin
            ));
        }
        match (native.metadata(), script.metadata()) {
            (None, None) => {}
            (Some(n), Some(s)) => {
                if let Some(d) = difference(&n, &s, &format!("{path}^")) {
                    return Some(d);
                }
            }
            (n, s) => {
                return Some(format!(
                    "{path}: metadata {:?} vs {:?}",
                    n.map(|m| m.data()),
                    s.map(|m| m.data())
                ));
            }
        }
        if matches!(native.node().kind, FormKind::Collection { .. }) {
            for (index, (n, s)) in native
                .children()
                .iter()
                .zip(script.children().iter())
                .enumerate()
            {
                if let Some(d) = difference(n, s, &format!("{path}/{index}")) {
                    return Some(d);
                }
            }
        }
        None
    }

    /// Expand `source`, a core macro call, natively and in the engine, each
    /// in a fresh scope of generated names alike, and require the same
    /// form, or that both decline: the engine with an error, and the native
    /// expansion before allocating a name. A source that starts with `~` is
    /// one the native expansion leaves to the engine although it expands.
    fn compare(host: &mut MacroHost, source: &str) {
        let (left, source) = match source.strip_prefix('~') {
            Some(source) => (true, source),
            None => (false, source),
        };
        let text = Source::new("native.cljs", source);
        let call = host.read_compiler_form(&text).unwrap().unwrap();
        let operator = call.children()[0].data();
        let DatumKind::Symbol(operator) = operator.kind() else {
            panic!("{source}: no operator");
        };
        let name = String::from_utf16_lossy(operator.name());
        assert!(native(&name), "{source}: {name} is not native");
        let environment = {
            let key = |name| C.keyword(Name::new(None, name));
            let namespace = C
                .map(&[(key("name"), C.symbol(Name::new(None, "user")))])
                .unwrap();
            C.map(&[(key("ns"), namespace)]).unwrap()
        };
        let script = {
            let _names = crate::generated_names::enter_namespace(7);
            host.expand_core_javascript(&name, &call, &environment, None)
        };
        let expanded = {
            let _names = crate::generated_names::enter_namespace(7);
            let before = host.engine.expansions.get();
            let shape = native_expansion(&name, &call.data(), "user", host.limits.generated_nodes);
            let expanded = host
                .expand_core_native(&name, &call, "user", (&name, &(0..source.len())))
                .unwrap();
            // The macro-free check declines exactly where expansion does,
            // and walks the same shape.
            assert_eq!(
                shape.as_ref().map(|shape| within(shape, usize::MAX)),
                expanded
                    .as_ref()
                    .map(|form| within(&form.data(), usize::MAX)),
                "{source}: the macro-free check's expansion"
            );
            assert_eq!(
                host.engine.expansions.get() - before,
                usize::from(expanded.is_some()),
                "{source}: one expansion charged per native expansion"
            );
            expanded
        };
        match (expanded, script) {
            (Some(native), Ok(script)) => {
                if let Some(difference) = difference(&native, &script, "") {
                    panic!(
                        "{source}: {difference}\nnative {:?}\nengine {:?}",
                        native.data(),
                        script.data()
                    );
                }
            }
            (None, Err(_)) => {}
            (None, Ok(_)) if left => {}
            (native, script) => panic!(
                "{source}: native {:?}, engine {:?}",
                native.map(|f| f.data()),
                script.map(|f| f.data())
            ),
        }
    }

    fn compare_all(sources: &[&str]) {
        let mut host = MacroHost::new();
        for source in sources {
            compare(&mut host, source);
        }
    }

    #[cfg(light_v8)]
    #[test]
    fn logical_macros_expand_as_their_definitions() {
        compare_all(&[
            "(and)",
            "(or)",
            "(and x)",
            "(or 1)",
            "(and a b)",
            "(or a b c d)",
            "(and (f x) [1 2] {:a 1} nil false)",
            "(or nil nil)",
            "(and ^:m (g) \"s\" 1.0 1)",
            "(and 1 1.0 1 -0.0 0 ##NaN ##NaN)",
            "(or :a :a \\a \"a\" nil true)",
        ]);
    }

    #[cfg(light_v8)]
    #[test]
    fn conditionals_expand_as_their_definitions() {
        compare_all(&[
            "(when x)",
            "(when x a b)",
            "(when)",
            "(when-not x)",
            "(when-not (f) a b c)",
            "(when-not)",
            "(if-not x a)",
            "(if-not x a b)",
            "(if-not x)",
            "(if-not x a b c)",
            "(cond)",
            "(cond a b)",
            "(cond a b c d :else e)",
            "(cond a)",
            "(cond a b c)",
            "(cond nil nil nil nil)",
            "(cond :a 1 :b 1 :c [1 :a])",
            "(when :a :a (f :a) 2)",
            "(when true 9223372036854775808.0 9223372036854775808N)",
            "(when \"9223372036854775808\" 9223372036854775808N \"9223372036854775808\")",
        ]);
    }

    #[cfg(light_v8)]
    #[test]
    fn condp_expands_as_its_definition() {
        compare_all(&[
            "(condp = x)",
            "(condp = x 1 :a)",
            "(condp = x 1 :a :default)",
            "(condp = x 1 :a 2 :b :c)",
            "(condp some [1 2] #{0} :>> inc #{1} :>> dec :none)",
            "(condp = x 1 :>>)",
            "(condp = x 1 :>> f 2 :b 3 :>> g)",
            "(condp =)",
            "(condp)",
        ]);
    }

    #[cfg(light_v8)]
    #[test]
    fn threading_macros_expand_as_their_definitions() {
        compare_all(&[
            "(-> x)",
            "(->> x)",
            "(-> x f)",
            "(-> x (f) (g a b) ())",
            "(->> x (f) (g a b) ())",
            "(-> x ^:kept (inc) ^{:tag string} g :k \"s\" [1 2] {:a 1})",
            "(->> x (-> (f a) g) (h))",
            "(-> (-> x f) (->> g))",
            "(->)",
            "(-> x #(f %))",
            "(cond-> x)",
            "(cond-> x a f)",
            "(cond-> x a (f 1) b g (h) ^:m (k))",
            "(cond->> x a (f 1) b g)",
            "(cond-> x a)",
            "(cond->)",
            "(some-> x)",
            "(some-> x f (g 1) (h))",
            "(some->> x f (g 1) (h))",
            "(some->)",
        ]);
    }

    #[cfg(light_v8)]
    #[test]
    fn bootstrap_macros_expand_as_their_definitions() {
        compare_all(&[
            "(cond-> x a f b (g 1))",
            "(cond->> x a f b (g 1))",
            "(some-> x f (g 1))",
            "(some->> x f (g 1))",
        ]);
    }

    #[cfg(light_v8)]
    #[test]
    fn case_expands_as_its_definition() {
        compare_all(&[
            "(case x)",
            "(case x :default)",
            "(case x 1 :a)",
            "(case x 1 :a 2 :b :c)",
            "(case x (1 2) :a (3) :b () :c)",
            "(case x [1 2] :a [] :b)",
            "(case x a :a b :b c)",
            "(case x nil 1 true 2 \"s\" 3 \\c 4 1.5 5)",
            "(case x (a [1] \"s\") :a ((1 2)) :b)",
            "(case)",
        ]);
    }

    #[cfg(light_v8)]
    #[test]
    fn conditional_bindings_expand_as_their_definitions() {
        compare_all(&[
            "(if-let [x y] a)",
            "(if-let [x y] a b)",
            "(if-let [[a b] (f)] (g a) (h b) extra)",
            "(if-let [{:keys [a]} m] a)",
            "(if-let [x y])",
            "(if-let (x y) a)",
            "(if-let [x] a)",
            "(if-let [x y z w] a)",
            "(if-some [x y] a)",
            "(if-some [x y] a b)",
            "(when-let [x y])",
            "(when-let [x y] a b c)",
            "(when-let [x (f)] (g x))",
            "(when-let x)",
            "(when-let)",
            "(when-some [x y] a b)",
            "(when-some [x y])",
            "(when-first [x xs])",
            "(when-first [x xs] a b)",
            "(when-first [[a b] (f)] (g a b))",
            "(when-first [x] a)",
        ]);
    }

    #[test]
    fn js_obj_expands_as_its_definition() {
        compare_all(&[
            "(js-obj)",
            "(js-obj \"a\" 1)",
            "(js-obj \"a\" 1 \"b\" 2 \"a\" 3)",
            "(js-obj a 1 \"b\" 2)",
            "(js-obj (str \"a\" x) 1 :k 2 3 4 a 5 \"s\" 6)",
            "(js-obj (str x) 1 (str x) 2)",
            "(js-obj \"a\")",
            "(js-obj \"a\" 1 \"b\")",
            "(js-obj 1 :a 1.0 :b)",
            "(js-obj [1 2] :a (1 2) :b)",
            "(js-obj \"a\" 1 \"b\" 2 \"c\" 3 \"d\" 4 \"e\" 5 \"f\" 6 \"g\" 7 \"h\" 8)",
            "~(js-obj \"a\" 1 \"b\" 2 \"c\" 3 \"d\" 4 \"e\" 5 \"f\" 6 \"g\" 7 \"h\" 8 \"i\" 9)",
            "(js-obj \"({})\" \"({~{}:~{}})\" x 1)",
            "~(js-obj \\a 1)",
        ]);
    }

    #[cfg(light_v8)]
    #[test]
    fn multimethod_macros_expand_as_their_definitions() {
        compare_all(&[
            "(defmethod area :square [s] (* (:side s) (:side s)))",
            "(defmethod area :default named ([s] 0) ([s t] 1))",
            "(defmethod area [:a :b] fname [s] s)",
            "(defmethod area :x)",
            "(defmethod area)",
            "(defmulti area :shape)",
            "(defmulti area \"Doc.\" :shape)",
            "(defmulti area {:private true} :shape)",
            "(defmulti area \"Doc.\" {:a 1} :shape :default :none)",
            "(defmulti ^:private area :shape :hierarchy h :default :x)",
            "~(defmulti ^{:doc \"old\"} area \"new\" (fn [x] x))",
            "(defmulti area :shape :default :a :default :b)",
            "(defmulti area)",
            "(defmulti area \"Doc.\")",
            "(defmulti area :shape :bad 1)",
            "(defmulti area :shape :default)",
            "(defmulti \"area\" :shape)",
            "(defmulti area \\a :default :x)",
            "~(defmulti area \\a :shape)",
        ]);
    }

    #[test]
    fn calls_the_engine_cannot_convert_are_left_to_it_and_their_room_noted() {
        let mut host = MacroHost::new();
        host.limits.generated_nodes = 100;
        let zeros = vec!["0"; 200].join(" ");
        // The engine's conversion of the call fails, for an argument the
        // expansion discards as for one it keeps.
        compare(&mut host, &format!("(if-let [x true] 1 2 [{zeros}])"));
        compare(&mut host, &format!("(when x [{zeros}])"));
        host.limits.generated_nodes = 100_000;
        host.engine.work.measuring.set(true);
        let before = host.engine.work.node_peak.get();
        compare(&mut host, &format!("(when x [{zeros}])"));
        assert!(host.engine.work.node_peak.get() >= before.max(203));
    }

    #[test]
    fn wide_and_nested_doseq_and_for_are_left_to_the_engine() {
        let mut host = MacroHost::new();
        let native = |host: &mut MacroHost, source: &str| {
            let call = host
                .read_compiler_form(&Source::new("native.cljs", source))
                .unwrap()
                .unwrap();
            let name = if source.starts_with("(for") {
                "for"
            } else {
                "doseq"
            };
            let _names = crate::generated_names::enter_namespace(7);
            let shape = native_expansion(name, &call.data(), "user", host.limits.generated_nodes);
            let expanded = host
                .expand_core_native(name, &call, "user", (name, &(0..1)))
                .unwrap()
                .is_some();
            assert_eq!(shape.is_some(), expanded, "{source}");
            expanded
        };
        let modifiers = ":let [] ".repeat(20_000);
        assert!(!native(&mut host, &format!("(doseq [x xs {modifiers}] x)")));
        assert!(!native(&mut host, &format!("(for [x xs {modifiers}] x)")));
        let bindings = |n| (0..n).map(|i| format!("x{i} xs ")).collect::<String>();
        assert!(native(&mut host, &format!("(doseq [{}] x)", bindings(7))));
        let wide = format!("(doseq [{}] x)", bindings(9));
        assert!(!native(&mut host, &wide));
        host.limits.generated_nodes = 10_000_000;
        assert!(!native(&mut host, &wide));
        assert!(!native(&mut host, &format!("(for [{}] x)", bindings(200))));
    }

    #[test]
    #[cfg(light_v8)]
    fn expansions_outside_every_scope_are_left_to_the_engine() {
        // There `gensym` skips the names the engine decoded from the call.
        let mut host = MacroHost::new();
        let js =
            crate::compile_with_macros("(let* [logic1 42] (or false logic1))", &mut host).unwrap();
        let evaluator = crate::evaluator::Evaluator::new(Default::default()).unwrap();
        let value = evaluator.evaluate_js(&js).unwrap();
        assert_eq!(evaluator.number(value).unwrap(), 42.0);
        let call = host
            .read_compiler_form(&Source::new("native.cljs", "(or a b)"))
            .unwrap()
            .unwrap();
        assert!(
            host.expand_core_native("or", &call, "user", ("or", &(0..8)))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn calls_too_deep_to_convert_are_left_to_the_engine() {
        let mut sources = Vec::new();
        for depth in 120..=127 {
            let nested = format!("{}x{}", "(f ".repeat(depth), ")".repeat(depth));
            sources.push(format!("(when {nested})"));
            sources.push(format!("(or {nested} y)"));
            sources.push(format!("(-> {nested} g)"));
        }
        for forms in (60..=66).chain(124..=130) {
            sources.push(format!("(-> x {})", "f ".repeat(forms)));
            sources.push(format!("(and {})", "x ".repeat(forms)));
        }
        // The reader refuses the deepest; the engine's conversion the rest.
        let mut host = MacroHost::new();
        let readable = sources
            .iter()
            .filter(|source| {
                host.read_compiler_form(&Source::new("native.cljs", source.as_str()))
                    .is_ok()
            })
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert!(readable.len() > sources.len() / 2);
        compare_all(&readable);
    }

    #[test]
    fn sequence_macros_expand_as_their_definitions() {
        compare_all(&[
            "(for [x xs] x)",
            "(for [x xs y ys] [x y])",
            "(for [x xs y ys z zs] (f x y z 0))",
            "(for [x xs :let [a 1] :when (odd? x) :while (< x 9)] x)",
            "(for [[a b] (f) :when a y (g a) :let [z 0] :when (h z)] [a b y z])",
            "(for [x [0 1 true nil] :tag x] x)",
            "(for [x xs :when true] true)",
            "(for [] x)",
            "(for [x] x)",
            "(for [:let [a 1] x xs] x)",
            "(for [x xs :bad 1] x)",
            "(for [x xs])",
            "(for [x xs] a b)",
            "(for (x xs) x)",
            "(doseq [x xs])",
            "(doseq [x xs] (f x))",
            "(doseq [] a b)",
            "(doseq [x xs y ys] (f x y))",
            "(doseq [x xs :let [a 1] :when a :while b] (f x))",
            "(doseq [x xs y ys :let [a 1] :when a :while b] (f x) (g y))",
            "(doseq [:when a x xs] (f x))",
            "(doseq [:let [a 1] :while a] b)",
            "(doseq [[k v] m :let [n 0 :tag 1]] (f k v n))",
            "(doseq [x xs :bad 1] x)",
            "(doseq [x] x)",
            "(doseq (x xs) x)",
            "(doseq)",
        ]);
    }

    /// The keys a map decodes into are indexed, so a map of many keys
    /// takes time in proportion to them: 100,000 keys compared with each
    /// other one by one would take billions of comparisons.
    #[cfg(light_v8)]
    #[test]
    fn engine_entries_large_primitive_maps_scale_linearly() {
        let keys = 100_000;
        let mut children: Vec<Datum> = (0..keys)
            .flat_map(|key| [C.integer(key), C.integer(0)])
            .collect();
        // Equal to `0`: one entry, the first key with the last value.
        children.extend([C.double(-0.0), C.integer(1)]);
        let start = std::time::Instant::now();
        let entries = engine_entries(&children, true).unwrap();
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(entries.len(), keys as usize);
        assert!(entries[0].0.value_eq(&C.integer(0)));
        assert!(entries[0].1.unwrap().value_eq(&C.integer(1)));
    }

    /// What the emulated conversion of a `doseq` or `for` expansion refuses
    /// is what the engine running its definition refuses, at node limits
    /// from the call's own size up to one the expansion fits in, or where
    /// it refuses the expansion's depth, and at the default limit
    /// (light-cljs-620): each node, step of a sequence and lazy sequence it
    /// realizes, for lists that splice the body, an empty body and the
    /// `recur` forms of modifiers, the call's data as it decodes it, and the
    /// first levels of an expansion of more pairs than conversion levels.
    #[cfg(light_v8)]
    #[test]
    fn sequence_conversion_refusals_follow_the_engine() {
        let environment = {
            let key = |name| C.keyword(Name::new(None, name));
            let namespace = C
                .map(&[(key("name"), C.symbol(Name::new(None, "user")))])
                .unwrap();
            C.map(&[(key("ns"), namespace)]).unwrap()
        };
        let bindings = |n| (0..n).map(|i| format!("x{i} xs ")).collect::<String>();
        let modifiers = |n, modifier| format!("{modifier} ").repeat(n);
        let deep = [
            format!("(doseq [x xs {}] x)", modifiers(130, ":let []")),
            format!(
                "(doseq [{}{}] (f x0))",
                bindings(3),
                modifiers(127, ":when a")
            ),
            format!("(for [x xs {}] x)", modifiers(130, ":let []")),
            format!("(for [{}] x0)", bindings(129)),
            format!("(for [{}x xs {}])", bindings(2), modifiers(128, ":while a")),
        ];
        let mut host = MacroHost::new();
        for (source, stride) in [
            ("(doseq [x xs] (f x))", 5),
            ("(doseq [x xs])", 5),
            ("(doseq [x xs] a (b) [c] d)", 5),
            ("(doseq [:when a x xs] (f x))", 5),
            ("(doseq [:let [a 1] :while a] b)", 1),
            ("(doseq [x xs :let [a 1] :when a :while b] (f x))", 13),
            (
                "(doseq [{:keys [a] :or {a 1}} m ^long y (g #\"r\" 9223372036854775808N)] ^{:m [1]} (f a y) #(h %))",
                17,
            ),
            ("(doseq [x xs y ys :when (p x y)] (f x) (g y))", 23),
            // Keys the engine finds equal decode into one entry.
            (
                "(doseq [x xs] (f {-0.0 1 0 [2 3]} {1 :a 1.0 [x y] \\a 4 \"a\" 5}))",
                3,
            ),
            (
                "(doseq [x xs] (f #{0 -0.0 [1]} {9 0 1 1 2 2 3 3 4 4 5 5 6 6 7 7 8 8}))",
                3,
            ),
            (&deep[0], 41),
            (&deep[1], 31),
            (&deep[2], 43),
            (&deep[3], 31),
            (&deep[4], 29),
        ] {
            let call = host
                .read_compiler_form(&Source::new("native.cljs", source))
                .unwrap()
                .unwrap();
            let data = call.data();
            let name = if source.starts_with("(for") {
                "for"
            } else {
                "doseq"
            };
            let mut compare = |limit| {
                host.limits.generated_nodes = limit;
                host.engine.generated_nodes.set(0);
                host.engine.expansions.set(0);
                let engine = {
                    let _names = crate::generated_names::enter_namespace(7);
                    host.expand_core_javascript(name, &call, &environment, None)
                };
                let emulated = wide_sequence(name, &data, limit);
                let refusal = engine.err().map(|error| {
                    error
                        .split("Error: Unsupported macro result ")
                        .nth(1)
                        .and_then(|rest| rest.split([';', '\n']).next())
                        .unwrap_or_else(|| panic!("{source} at {limit}: {error}"))
                        .to_owned()
                });
                let expected = refusal.clone().map(Stop::Refused).map_or(Ok(()), Err);
                // Where the engine refuses inside a set or a hash map, whose
                // order is its hashes', the emulation cannot tell.
                let hashed = source.contains("#{") && refusal.is_some();
                if !(hashed && emulated == Err(Stop::Unknown)) {
                    assert_eq!(emulated, expected, "{source} at {limit}");
                }
                refusal
            };
            let mut limit = within(&data, DEPTH).unwrap();
            let mut checked = 0;
            // Until the expansion converts, or its depth is what it refuses.
            while compare(limit).is_some_and(|refusal| !refusal.starts_with("depth limit")) {
                checked += 1;
                limit += stride;
            }
            compare(MacroHost::new().limits.generated_nodes);
            assert!(checked > 20, "{source}: {checked}");
        }
    }

    #[cfg(light_v8)]
    #[test]
    fn vswap_expands_as_its_definition() {
        compare_all(&[
            "(vswap! v inc)",
            "(vswap! v + 1 2)",
            "(vswap! (f) assoc :a 1)",
            "(vswap! v)",
        ]);
    }
}
