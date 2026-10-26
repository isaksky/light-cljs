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
//! conversion refuses, and every call outside a scope of generated names
//! ([`crate::MacroHost::expand_core_native`]). The names a definition
//! allocates are those the compile-time runtime's `gensym` gives, in the
//! same order and with the same prefixes ([`Gensym`]), taken from the
//! scope's count without entering an engine (light-cljs-432,
//! [`crate::generated_names::gensym`]), so generated names, their scopes
//! and what a capture records of them stay what the engine gives, and a
//! unit whose only macros expand here needs no engine
//! ([`native_expansion`], which the check of macro-free units walks).
use crate::CollectionKind;
use crate::compiler_data::{DataBuilder, Datum, DatumKind, Name};

/// The core macros the analyzer expands in Rust for the selected backend.
/// Additional bootstrap form builders serve the native backend's empty
/// bootstrap table; legacy engines retain their expansion accounting,
/// environment conversion and poll-budget behavior for those calls.
pub(crate) fn native(name: &str, native_backend: bool) -> bool {
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
    ) || native_backend
        && matches!(
            name,
            "as->"
                | "doto"
                | "js-fn?"
                | "while"
                | "dotimes"
                | "memfn"
                | "amap"
                | "areduce"
                | ".."
                | "lazy-cat"
                | "comment"
                | "with-in-str"
                | "js-debugger"
                | "js-comment"
                | "locking"
                | "time"
                | "unchecked-max"
                | "unchecked-min"
                | "hash-map"
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
        "js-fn?" => match args {
            [value] => Some(list(vec![
                symbol("js*"),
                C.string(
                    "typeof ~{} === 'function'"
                        .encode_utf16()
                        .collect::<Vec<_>>(),
                ),
                value.clone(),
            ])),
            _ => None,
        },
        "comment" => Some(C.nil()),
        "locking" => Some(list(
            std::iter::once(symbol("do"))
                .chain(args.iter().skip(1).cloned())
                .collect(),
        )),
        "with-in-str" => args.split_first().map(|(text, body)| {
            list(vec![
                core("compiler-reader-with-input"),
                text.clone(),
                list([vec![symbol("fn*"), vector(vec![])], body.to_vec()].concat()),
            ])
        }),
        "js-debugger" => Some(list(vec![
            symbol("do"),
            list(vec![
                symbol("js*"),
                C.string("debugger".encode_utf16().collect::<Vec<_>>()),
            ]),
            C.nil(),
        ])),
        "js-comment" => args.first().and_then(js_comment),
        "lazy-cat" => Some(list(
            std::iter::once(core("concat"))
                .chain(
                    args.iter()
                        .map(|coll| list(vec![core("lazy-seq"), coll.clone()])),
                )
                .collect(),
        )),
        ".." => match args {
            [value, form, more @ ..] => {
                let step = list(vec![symbol("."), value.clone(), form.clone()]);
                Some(if more.is_empty() {
                    step
                } else {
                    list([vec![core(".."), step], more.to_vec()].concat())
                })
            }
            _ => None,
        },
        "memfn" => match args.split_first() {
            Some((method, args)) => {
                let target = gensym(&["target"])?.remove(0);
                Some(list(vec![
                    core("fn"),
                    vector([vec![target.clone()], args.to_vec()].concat()),
                    list(vec![
                        symbol("."),
                        target,
                        list([vec![method.clone()], args.to_vec()].concat()),
                    ]),
                ]))
            }
            None => None,
        },
        "while" => args.split_first().map(|(test, body)| {
            let mut body = [vec![symbol("do")], body.to_vec()].concat();
            body.push(list(vec![symbol("recur")]));
            list(vec![
                symbol("loop*"),
                vector(vec![]),
                list(vec![symbol("if"), test.clone(), list(body), C.nil()]),
            ])
        }),
        "dotimes" => match args.split_first() {
            Some((bindings, body)) if is_vector(bindings) && children(bindings).len() == 2 => Some(
                dotimes(&children(bindings)[0], &children(bindings)[1], body, gensym)?,
            ),
            _ => None,
        },
        "amap" => match args {
            [array, index, result, expr, ..] => {
                Some(array_iteration(array, index, result, None, expr, gensym)?)
            }
            _ => None,
        },
        "areduce" => match args {
            [array, index, result, init, expr, ..] => Some(array_iteration(
                array,
                index,
                result,
                Some(init),
                expr,
                gensym,
            )?),
            _ => None,
        },
        "unchecked-max" | "unchecked-min" => match args {
            [value] => Some(value.clone()),
            [x, y, more @ ..] => Some(unchecked_extreme(
                x,
                y,
                more,
                name == "unchecked-max",
                gensym,
            )?),
            _ => None,
        },
        "hash-map" => Some(hash_map(args)),
        "time" => match args.first() {
            Some(expr) => Some(timed(expr, gensym)?),
            None => None,
        },
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
        "as->" => match args {
            [value, name, forms @ ..] => {
                let mut bindings = vec![name.clone(), value.clone()];
                for form in forms.iter().take(forms.len().saturating_sub(1)) {
                    bindings.extend([name.clone(), form.clone()]);
                }
                Some(list(vec![
                    core("let"),
                    vector(bindings),
                    forms.last().unwrap_or(name).clone(),
                ]))
            }
            _ => None,
        },
        "doto" => match args.split_first() {
            Some((value, forms)) => {
                let target = gensym(&["target"])?.remove(0);
                let mut result = vec![core("let"), vector(vec![target.clone(), value.clone()])];
                for form in forms {
                    // Unlike ->, doto does not copy the step's metadata.
                    let mut step = if is_seq(form) {
                        let forms = children(form);
                        vec![
                            forms.first().cloned().unwrap_or_else(|| C.nil()),
                            target.clone(),
                        ]
                    } else {
                        vec![form.clone(), target.clone()]
                    };
                    step.extend(
                        children(form)
                            .iter()
                            .skip(1)
                            .filter(|_| is_seq(form))
                            .cloned(),
                    );
                    result.push(list(step));
                }
                result.push(target);
                Some(list(result))
            }
            None => None,
        },
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

/// `macro-dotimes`: evaluate the limit once, including for empty loops.
fn dotimes(
    name: &Datum,
    limit: &Datum,
    body: &[Datum],
    gensym: Gensym<'_>,
) -> Result<Datum, String> {
    let bound = gensym(&["limit"])?.remove(0);
    let mut body = [vec![symbol("do")], body.to_vec()].concat();
    body.push(list(vec![
        symbol("recur"),
        list(vec![core("inc"), name.clone()]),
    ]));
    Ok(list(vec![
        symbol("let*"),
        vector(vec![bound.clone(), limit.clone()]),
        list(vec![
            core("loop"),
            vector(vec![name.clone(), number(0.0)]),
            list(vec![
                symbol("if"),
                list(vec![core("<"), name.clone(), bound]),
                list(body),
                C.nil(),
            ]),
        ]),
    ]))
}

/// `macro-amap` and `macro-areduce`: bind the input and its length once.
fn array_iteration(
    array: &Datum,
    index: &Datum,
    result: &Datum,
    init: Option<&Datum>,
    expr: &Datum,
    gensym: Gensym<'_>,
) -> Result<Datum, String> {
    let names = gensym(&["array", "length"])?;
    let (arr, len) = (names[0].clone(), names[1].clone());
    let mut bindings = vec![
        arr.clone(),
        array.clone(),
        len.clone(),
        list(vec![core("alength"), arr.clone()]),
    ];
    let mut loop_bindings = vec![index.clone(), number(0.0)];
    let next = list(vec![core("inc"), index.clone()]);
    let body = if let Some(init) = init {
        loop_bindings.extend([result.clone(), init.clone()]);
        list(vec![symbol("recur"), next, expr.clone()])
    } else {
        bindings.extend([result.clone(), list(vec![core("aclone"), arr])]);
        list(vec![
            symbol("do"),
            list(vec![
                core("aset"),
                result.clone(),
                index.clone(),
                expr.clone(),
            ]),
            list(vec![symbol("recur"), next]),
        ])
    };
    Ok(list(vec![
        core("let"),
        vector(bindings),
        list(vec![
            core("loop"),
            vector(loop_bindings),
            list(vec![
                symbol("if"),
                list(vec![core("<"), index.clone(), len]),
                body,
                result.clone(),
            ]),
        ]),
    ]))
}

/// `macro-unchecked-extreme`: the n-ary form nests ordinary max/min calls.
fn unchecked_extreme(
    x: &Datum,
    y: &Datum,
    more: &[Datum],
    max: bool,
    gensym: Gensym<'_>,
) -> Result<Datum, String> {
    let extreme = core(if max { "max" } else { "min" });
    if !more.is_empty() {
        return Ok(list(
            [
                vec![extreme.clone(), list(vec![extreme, x.clone(), y.clone()])],
                more.to_vec(),
            ]
            .concat(),
        ));
    }
    let names = gensym(&["x", "y"])?;
    let (a, b) = (names[0].clone(), names[1].clone());
    Ok(list(vec![
        symbol("let*"),
        vector(vec![a.clone(), x.clone(), b.clone(), y.clone()]),
        list(vec![
            symbol("if"),
            list(vec![
                core(if max { ">" } else { "<" }),
                a.clone(),
                b.clone(),
            ]),
            a,
            b,
        ]),
    ]))
}

/// `macro-hash-map`, including its operand grouping and missing-value marker.
fn hash_map(args: &[Datum]) -> Datum {
    if args.is_empty() {
        return list(vec![symbol(".-EMPTY"), core("PersistentHashMap")]);
    }
    let mut keys = vec![core("array")];
    let mut values = vec![core("array")];
    let mut complete = true;
    for pair in args.chunks(2) {
        let pair: Vec<_> = pair.iter().filter(|d| {
            !matches!(d.kind(), DatumKind::Keyword(n) if *n == Name::new(Some("cljs.core"), "missing"))
        }).cloned().collect();
        keys.push(pair.first().cloned().unwrap_or_else(|| C.nil()));
        complete &= pair.len() == 2;
        if complete {
            values.push(pair[1].clone());
        }
    }
    list(vec![
        core("PersistentHashMap.fromArrays"),
        list(keys),
        list(values),
    ])
    .with_metadata(
        C.map(&[(C.keyword(Name::new(None, "tag")), core("PersistentHashMap"))])
            .unwrap(),
    )
}

/// `macro-js-comment`: split and trim UTF-16 directly, preserving lone surrogates.
fn js_comment(comment: &Datum) -> Option<Datum> {
    let DatumKind::String(text) = comment.kind() else {
        return None;
    };
    let mut lines: Vec<_> = text.split(|c| *c == 10).collect();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    let mut template: Vec<_> = "\n/**\n * ".encode_utf16().collect();
    template.extend(lines.first().copied().unwrap_or_default());
    template.push(10);
    for line in lines.iter().skip(1) {
        template.extend(" * ".encode_utf16());
        template.extend(line.strip_prefix(&[32, 32, 32]).unwrap_or(line));
        template.push(10);
    }
    template.extend(" */\n".encode_utf16());
    Some(list(vec![
        symbol("do"),
        list(vec![symbol("js*"), C.string(template)]),
        C.nil(),
    ]))
}

/// `macro-time`: retain the expression's result after printing elapsed time.
fn timed(expr: &Datum, gensym: Gensym<'_>) -> Result<Datum, String> {
    let names = gensym(&["start", "ret"])?;
    let (start, ret) = (names[0].clone(), names[1].clone());
    let clock = || list(vec![core("system-time")]);
    Ok(list(vec![
        core("let"),
        vector(vec![start.clone(), clock(), ret.clone(), expr.clone()]),
        list(vec![
            core("prn"),
            list(vec![
                core("str"),
                C.string("Elapsed time: ".encode_utf16().collect::<Vec<_>>()),
                list(vec![
                    symbol(".toFixed"),
                    list(vec![core("-"), clock(), start]),
                    number(6.0),
                ]),
                C.string(" msecs".encode_utf16().collect::<Vec<_>>()),
            ]),
        ]),
        ret,
    ]))
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
    // own metadata where nothing is added to it, else, where it has none,
    // a map of the attributes' keyword keys within an array map's eight
    // entries, and the docstring.
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
                    None => metadata.push((key, value)),
                }
            }
            if metadata.len() > 8 {
                return Ok(None);
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
/// second pass takes them.
struct Names {
    dry: Option<Vec<&'static str>>,
    allocated: std::vec::IntoIter<Datum>,
}
impl Names {
    fn next(&mut self, prefix: &'static str) -> Datum {
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
    if !is_vector(bindings)
        || pairs.is_empty()
        || !pairs.len().is_multiple_of(2)
        || pairs.len() / 2 > DEPTH
    {
        return Ok(None);
    }
    // Each binding and its modifiers; a modifier first pops no group.
    let mut groups: Vec<Group<'_>> = Vec::new();
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
        list(vec![symbol(FOR_ITERATOR), pairs[1].clone()]),
    ])))
}

/// The most names a native `doseq` allocates: its expansion grows with
/// them, doubling at each nested binding.
const DOSEQ_NAMES: usize = 1024;

/// `sequence-doseq`.
fn sequence_doseq(
    bindings: &Datum,
    body: &[Datum],
    gensym: Gensym<'_>,
) -> Result<Option<Datum>, String> {
    let exprs = children(bindings);
    if !is_vector(bindings) || !exprs.len().is_multiple_of(2) {
        return Ok(None);
    }
    if exprs
        .iter()
        .step_by(2)
        .any(|k| matches!(k.kind(), DatumKind::Keyword(_)) && modifier(k).is_none())
    {
        return Ok(None);
    }
    // Its construction recurses once a pair and doubles at each binding,
    // as the definition's does: more pairs than conversion levels, or more
    // names than [`DOSEQ_NAMES`], are left to the engine.
    if exprs.len() / 2 > DEPTH {
        return Ok(None);
    }
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
    /// `step`: whether the form needs its caller's `recur`, and the form.
    fn step(
        names: &mut Names,
        recform: Option<Datum>,
        exprs: &[Datum],
        body: &[Datum],
    ) -> (bool, Datum) {
        let Some((k, rest)) = exprs.split_first() else {
            let mut done = vec![symbol("do")];
            done.extend(body.iter().cloned());
            done.push(C.nil());
            return (true, list(done));
        };
        let v = &rest[0];
        let seq = names.next("seq__");
        let keyword = matches!(k.kind(), DatumKind::Keyword(_));
        let recform = if keyword {
            recform
        } else {
            Some(list(vec![
                symbol("recur"),
                list(vec![core("next"), seq.clone()]),
                C.nil(),
                number(0.0),
                number(0.0),
            ]))
        };
        let (needrec, subform) = step(names, recform.clone(), &rest[1..], body);
        let recform_datum = || recform.clone().unwrap_or_else(|| C.nil());
        let again = |form: Vec<Datum>, recform: &Datum| {
            let mut form = form;
            if needrec {
                form.push(recform.clone());
            }
            list(form)
        };
        match modifier(k) {
            Some("let") => (needrec, list(vec![core("let"), v.clone(), subform])),
            Some("while") => (
                false,
                again(vec![core("when"), v.clone(), subform], &recform_datum()),
            ),
            Some(_) => (
                false,
                list(vec![
                    symbol("if"),
                    v.clone(),
                    again(vec![symbol("do"), subform], &recform_datum()),
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
                let (_, subform_chunk) = step(names, Some(recform_chunk.clone()), &rest[1..], body);
                let clojure = |name| C.symbol(Name::new(Some("clojure.core"), name));
                let recform = recform_datum();
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
                            again(
                                vec![
                                    core("let"),
                                    vector(vec![
                                        k.clone(),
                                        list(vec![core("-nth"), chunk.clone(), i.clone()]),
                                    ]),
                                    subform_chunk,
                                ],
                                &recform_chunk,
                            ),
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
                                    again(
                                        vec![
                                            core("let"),
                                            vector(vec![
                                                k.clone(),
                                                list(vec![core("first"), seq.clone()]),
                                            ]),
                                            subform,
                                        ],
                                        &recform,
                                    ),
                                ]),
                            ]),
                        ]),
                    ]),
                )
            }
        }
    }
    let mut dry = Names {
        dry: Some(Vec::new()),
        allocated: Vec::new().into_iter(),
    };
    step(&mut dry, None, exprs, body);
    let prefixes = dry.dry.unwrap();
    let allocated = if prefixes.is_empty() {
        Vec::new()
    } else {
        gensym(&prefixes)?
    };
    let mut names = Names {
        dry: None,
        allocated: allocated.into_iter(),
    };
    Ok(Some(step(&mut names, None, exprs, body).1))
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
    let expanded = expand(name, call, namespace, &mut stand_ins).ok()??;
    within(&expanded, DEPTH)?;
    Some(expanded)
}

impl crate::MacroHost {
    /// Expand the call `form` of the core macro `name`, spelled as `site`
    /// gives it at its span, natively ([`expand`]), charged as the engine
    /// charges an expansion; `None` if the engine expands it instead. It
    /// never enters an engine: the names it allocates come from the scope's
    /// count ([`scope_gensyms`]).
    pub(crate) fn expand_core_native(
        &mut self,
        name: &str,
        form: &crate::compiler_data::Form,
        namespace: &str,
        site: (&str, &std::ops::Range<usize>),
    ) -> Result<Option<crate::compiler_data::Form>, String> {
        // Outside every scope of generated names, `gensym` skips the names
        // the heap decoded, which the engine's conversion of the call adds.
        if !crate::generated_names::scoped() {
            return Ok(None);
        }
        let call = form.data();
        // A call the engine's conversion refuses, too deep or larger than the
        // node limit (which each conversion may take whole), is left to it,
        // which fails as it does; so is never one it converts.
        if within(&call, DEPTH).is_none_or(|nodes| nodes > self.limits.generated_nodes) {
            return Ok(None);
        }
        if self.engine.expansions.get() >= self.limits.expansions {
            return Err("compiler macro expansion limit exceeded".into());
        }
        // The room the engine's conversion of the call needs, as it notes
        // it ([`crate::macro_host::MacroHost::expand_javascript`]), so a
        // namespace's accounting admits it where this expands it. Its
        // environment is never converted here.
        if self.measures_work() {
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
        let (operator, span) = site;
        crate::macro_host::record_native_expansion(|| crate::macro_host::NativeExpansion {
            nodes: expanded.nodes().len(),
            operator: operator.to_owned(),
            span: span.clone(),
        });
        self.engine.expansions.set(self.engine.expansions.get() + 1);
        self.engine.generated_nodes.set(
            self.engine
                .generated_nodes
                .get()
                .saturating_add(expanded.nodes().len()),
        );
        if self.engine.generated_nodes.get() > self.limits.generated_nodes {
            return Err("compiler macro generated-node limit exceeded".into());
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
        assert!(native(&name, true), "{source}: {name} is not native");
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
            "(as-> x n)",
            "(as-> x n (f n) (g n n) n)",
            "(as-> x ^:kept n (as-> n n (f n)))",
            "(as-> x [a b] (+ a b))",
            "(as-> x)",
            "(doto x)",
            "(doto x f (g a b) ())",
            "(doto x ^:discarded (f 1) ^:discarded g)",
            "(doto)",
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

    #[cfg(light_quickjs)]
    #[test]
    fn bootstrap_macros_expand_as_their_quickjs_definitions() {
        compare_all(&[
            "(as-> x n)",
            "(as-> x n (f n n) (as-> n n (g n)))",
            "(doto x f (g a) ^:discarded (h) ())",
            "(cond-> x a f b (g 1))",
            "(cond->> x a f b (g 1))",
            "(some-> x f (g 1))",
            "(some->> x f (g 1))",
            "(js-fn? x)",
            "(js-fn?)",
            "(js-fn? x y)",
            "(comment (unresolved) x)",
            "(locking (unresolved) a b)",
            "(locking)",
            "(with-in-str x a b)",
            "(with-in-str x)",
            "(with-in-str)",
            "(js-debugger)",
            "(js-debugger ignored)",
            "(js-comment \"first\\n   second\\n\\n\")",
            "(js-comment \"\\n\")",
            "(js-comment \"\\uD800\\n   \\uDC00\")",
            "(js-comment 1)",
            "(lazy-cat)",
            "(lazy-cat a (f x) [1 2])",
            "(.. x (f a) -b (g))",
            "(.. x f)",
            "(.. x)",
            "(memfn f a b)",
            "(memfn f)",
            "(memfn)",
            "(while test a b)",
            "(while test)",
            "(while)",
            "(dotimes [i n] a b)",
            "(dotimes [i n])",
            "(dotimes (i n) a)",
            "(dotimes [i n j m] a)",
            "(amap a i ret (f i ret))",
            "(amap a i ret (f i ret) ignored)",
            "(amap a i ret)",
            "(areduce a i ret 0 (f i ret))",
            "(areduce a i ret 0 (f i ret) ignored)",
            "(areduce a i ret 0)",
            "(unchecked-max x)",
            "(unchecked-max x y)",
            "(unchecked-min x y)",
            "(unchecked-max x y a b)",
            "(unchecked-min x y a b)",
            "(unchecked-max)",
            "(hash-map)",
            "(hash-map :a 1 :b 2 :a 3)",
            "(hash-map :a 1 :b)",
            "(hash-map :cljs.core/missing a b c)",
            "(hash-map :cljs.core/missing :cljs.core/missing a b)",
            "(hash-map :a 1.0 :b 1 :c -0.0)",
            "(time x)",
            "(time (f) ignored)",
            "(time)",
        ]);
    }

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
        let mut native = |source: &str| {
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
        assert!(!native(&format!("(doseq [x xs {modifiers}] x)")));
        assert!(!native(&format!("(for [x xs {modifiers}] x)")));
        let bindings = |n| (0..n).map(|i| format!("x{i} xs ")).collect::<String>();
        assert!(native(&format!("(doseq [{}] x)", bindings(7))));
        assert!(!native(&format!("(doseq [{}] x)", bindings(9))));
        assert!(!native(&format!("(for [{}] x)", bindings(200))));
    }

    #[test]
    #[cfg(light_quickjs)]
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
            "~(for [] x)",
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
