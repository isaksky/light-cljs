//! Temporary compiler/native reader and macro-host conversion boundary.
//! Delete this entire adapter with the old native evaluator in light-cljs-115.
//! No compiler form stores these native roots, sources, or executable units.
use crate::{compiler_data as owned, interpreter as native};
use std::collections::HashMap;

pub(crate) fn source(context: &native::Context, source: &owned::Source) -> native::Source {
    let offset = source.line_column(0).expect("source start").0 - 1;
    context.source_at_line(source.name().into(), source.text().into(), offset)
}

#[cfg(test)]
pub(crate) fn own_form(form: &native::Form) -> Result<owned::Form, String> {
    own_form_with_sources(form, HashMap::new())
}
pub(crate) fn own_form_with_sources(
    form: &native::Form,
    mut sources: HashMap<native::SourceId, owned::Source>,
) -> Result<owned::Form, String> {
    fn datum(
        form: &native::Form,
        id: owned::FormId,
        values: &mut [Option<owned::Datum>],
        memo: &mut HashMap<native::HeapId, owned::Datum>,
        depth: usize,
    ) -> Result<owned::Datum, String> {
        if depth >= 256 {
            return Err("compiler form conversion nesting limit exceeded".into());
        }
        if let Some(data) = &values[id.0] {
            return Ok(data.clone());
        }
        let node = form.at(id).unwrap();
        // The native lowerer has already bounded/realized sequence output. Read
        // that finite form shape instead of traversing the original LazySeq.
        let mut data = match &node.node().kind {
            owned::FormKind::Collection { kind, children } => {
                owned::Datum::new(owned::DatumKind::Collection {
                    kind: *kind,
                    children: children
                        .iter()
                        .map(|&child| datum(form, child, values, memo, depth + 1))
                        .collect::<Result<_, _>>()?,
                })
            }
            owned::FormKind::Scalar => {
                if let native::ValueView::CompilerLiteral(kind) = node.data().inspect()
                    && let Some(payload) = node.node().compiler_payload
                {
                    owned::Datum::new(owned::DatumKind::CompilerLiteral {
                        kind,
                        payload: datum(form, payload, values, memo, depth + 1)?,
                    })
                } else {
                    own_datum(&node.data(), memo, depth + 1)?
                }
            }
        };
        if let Some(metadata) = node.node().metadata {
            data = data.with_metadata(datum(form, metadata, values, memo, depth + 1)?);
        }
        values[id.0] = Some(data.clone());
        if let Some(heap) = node.data().heap_id() {
            memo.insert(heap, data.clone());
        }
        Ok(data)
    }
    let mut memo = HashMap::new();
    let mut values = vec![None; form.nodes().len()];
    let mut data = Vec::with_capacity(form.nodes().len());
    let mut spans = Vec::with_capacity(form.nodes().len());
    for index in 0..form.nodes().len() {
        let node = form.at(owned::FormId(index)).unwrap();
        let span = node.span();
        let source = sources.entry(span.source().id()).or_insert_with(|| {
            owned::Source::with_line_offset(
                span.source().name(),
                span.source().text(),
                span.source().line_column(0).unwrap().0 - 1,
            )
        });
        spans.push(source.span(span.bytes()).expect("validated native span"));
        data.push(datum(form, node.id(), &mut values, &mut memo, 0)?);
    }
    Ok(owned::Form::from_parts(
        form.nodes().to_vec(),
        data,
        spans,
        form.id(),
    ))
}

fn own_datum(
    value: &native::RootedValue,
    memo: &mut HashMap<native::HeapId, owned::Datum>,
    depth: usize,
) -> Result<owned::Datum, String> {
    if depth >= 256 {
        return Err("compiler datum conversion nesting limit exceeded".into());
    }
    if let Some(id) = value.heap_id()
        && let Some(data) = memo.get(&id)
    {
        return Ok(data.clone());
    }
    use native::ValueView as V;
    use owned::DatumKind as D;
    let kind = match value.inspect() {
        V::Nil => D::Nil,
        V::Boolean(v) => D::Boolean(v),
        V::Integer(v) => D::Integer(v),
        V::Double(v) => D::Double(v),
        V::Ratio {
            numerator,
            denominator,
        } => D::Ratio {
            numerator,
            denominator,
        },
        V::Character(v) => D::Character(v),
        V::String(v) => D::String(v),
        V::Symbol(v) => D::Symbol(v),
        V::Keyword(v) => D::Keyword(v),
        V::CompilerLiteral(kind) => D::CompilerLiteral {
            kind,
            payload: own_datum(&value.compiler_literal_data().unwrap(), memo, depth + 1)?,
        },
        view @ (V::List(_) | V::Vector(_) | V::Map(_) | V::Set(_) | V::Seq(_)) => {
            use crate::CollectionKind as K;
            let (kind, values) = match view {
                V::Map(_) => (
                    K::Map,
                    value
                        .entries()
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .flat_map(|(k, v)| [k, v])
                        .collect(),
                ),
                V::Set(_) => (K::Set, value.elements().map_err(|e| e.to_string())?),
                V::Vector(_) => (K::Vector, value.elements().map_err(|e| e.to_string())?),
                _ => (K::List, value.elements().map_err(|e| e.to_string())?),
            };
            D::Collection {
                kind,
                children: values
                    .iter()
                    .map(|v| own_datum(v, memo, depth + 1))
                    .collect::<Result<_, _>>()?,
            }
        }
        view => return Err(format!("runtime value generated by a macro: {view:?}")),
    };
    let mut data = owned::Datum::new(kind);
    if let Some(meta) = value.metadata() {
        data = data.with_metadata(own_datum(&meta, memo, depth + 1)?);
    }
    if let Some(id) = value.heap_id() {
        memo.insert(id, data.clone());
    }
    Ok(data)
}

pub(crate) fn native_datum(
    context: &native::Context,
    datum: &owned::Datum,
) -> Result<native::RootedValue, String> {
    native_datum_memo(context, datum, &mut HashMap::new(), 0)
}
fn native_datum_memo(
    context: &native::Context,
    datum: &owned::Datum,
    memo: &mut HashMap<usize, native::RootedValue>,
    depth: usize,
) -> Result<native::RootedValue, String> {
    if depth >= 256 {
        return Err("native datum adapter nesting limit exceeded".into());
    }
    if let Some(value) = memo.get(&datum.identity()) {
        return Ok(value.clone());
    }
    use owned::DatumKind as D;
    let value = match datum.kind() {
        D::Nil => context.nil(),
        D::Boolean(v) => context.boolean(*v),
        D::Integer(v) => context.integer(*v),
        D::Double(v) => context.double(*v),
        D::Ratio {
            numerator,
            denominator,
        } => context
            .ratio(*numerator, *denominator)
            .map_err(|e| e.to_string())?,
        D::Character(v) => context.character(*v),
        D::String(v) => context.string(v.clone()),
        D::Symbol(v) => context.symbol(v.clone()),
        D::Keyword(v) => context.keyword(v.clone()),
        D::CompilerLiteral { kind, payload } => context.compiler_literal(
            *kind,
            &native_datum_memo(context, payload, memo, depth + 1)?,
        ),
        D::Collection { kind, children } => {
            let values = children
                .iter()
                .map(|v| native_datum_memo(context, v, memo, depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            use crate::CollectionKind as K;
            match kind {
                K::AnonymousFn => {
                    return Err("anonymous-function syntax requires reader lowering".into());
                }
                K::List => context.list(&values),
                K::Vector => context.vector(&values),
                K::Set => context.set(&values),
                K::Map => {
                    if !values.len().is_multiple_of(2) {
                        return Err("compiler map datum requires key/value pairs".into());
                    }
                    context.map(
                        &values
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|[k, v]| (k.clone(), v.clone()))
                            .collect::<Vec<_>>(),
                    )
                }
            }
            .map_err(|e| e.to_string())?
        }
    };
    let value = match datum.metadata() {
        Some(meta) => context
            .with_meta(&value, &native_datum_memo(context, &meta, memo, depth + 1)?)
            .map_err(|e| e.to_string())?,
        None => value,
    };
    memo.insert(datum.identity(), value.clone());
    Ok(value)
}

pub(crate) fn native_form_with_sources(
    context: &native::Context,
    form: &owned::Form,
) -> Result<(native::Form, HashMap<native::SourceId, owned::Source>), String> {
    let mut source_origins = HashMap::new();
    let mut sources = HashMap::new();
    let mut datums = HashMap::new();
    let mut values = Vec::with_capacity(form.nodes().len());
    let mut spans = Vec::with_capacity(form.nodes().len());
    for index in 0..form.nodes().len() {
        let node = form.at(owned::FormId(index)).unwrap();
        let span = node.span();
        let src = sources
            .entry(span.source().id())
            .or_insert_with(|| source(context, span.source()));
        source_origins.insert(src.id(), span.source().clone());
        spans.push(src.span(span.bytes()).expect("validated compiler span"));
        values.push(native_datum_memo(context, &node.data(), &mut datums, 0)?);
    }
    Ok((
        context.native_form_adapter(form.nodes().to_vec(), &values, &spans, form.id()),
        source_origins,
    ))
}

pub(crate) fn read_form(
    context: &native::Context,
    source: &owned::Source,
) -> Result<Option<owned::Form>, owned::ReadError> {
    let native_source = self::source(context, source);
    context
        .reader(&native_source)
        .expect("adapter owner")
        .read_form()
        .map_err(|error| read_error(error, source))?
        .map(|form| {
            own_form_with_sources(&form, HashMap::from([(native_source.id(), source.clone())]))
                .map_err(|message| owned::ReadError {
                    message,
                    span: source.span(form.span().bytes()).unwrap(),
                    related: None,
                })
        })
        .transpose()
}
fn read_error(error: native::ReadError, source: &owned::Source) -> owned::ReadError {
    owned::ReadError {
        message: format!("{:?}", error.kind),
        span: source.span(error.span.bytes()).expect("reader span"),
        related: error
            .related
            .map(|span| source.span(span.bytes()).expect("related reader span")),
    }
}
pub(crate) fn lower_form(
    context: &native::Context,
    source: &owned::Source,
    syntax: &crate::SyntaxTree,
    root: crate::NodeId,
    aliases: &[(String, String)],
    discards: &[crate::NodeId],
    globals: &[(String, String, String)],
) -> Result<owned::Form, owned::ReadError> {
    let native_source = self::source(context, source);
    let form = context
        .lower_compiler_form(&native_source, syntax, root, aliases, discards, globals)
        .map_err(|error| read_error(error, source))?;
    own_form_with_sources(&form, HashMap::from([(native_source.id(), source.clone())])).map_err(
        |message| owned::ReadError {
            message,
            span: source.span(form.span().bytes()).unwrap(),
            related: None,
        },
    )
}
pub(crate) fn print_datum(
    context: &mut native::Context,
    data: &owned::Datum,
) -> Result<std::rc::Rc<[u16]>, String> {
    let data = native_datum(context, data)?;
    let value = context
        .print_value(&data, native::PrintOptions::default())
        .map_err(|e| e.to_string())?;
    let native::ValueView::String(text) = value.inspect() else {
        unreachable!("printed string")
    };
    Ok(text)
}
pub(crate) fn method_form(
    context: &native::Context,
    original: &owned::Form,
    method_body: bool,
) -> Result<owned::Form, String> {
    let (original, sources) = native_form_with_sources(context, original)?;
    let children = original.children();
    let mut values = children
        .iter()
        .skip(usize::from(children.first().is_some_and(|f| {
            matches!(f.data().inspect(), native::ValueView::Symbol(_))
        })))
        .map(native::Form::data)
        .collect::<Vec<_>>();
    if method_body && values.len() > 1 {
        values[1] = context
            .list(&[
                context.symbol(owned::Name::new(None, "do")),
                values[1].clone(),
            ])
            .map_err(|e| e.to_string())?;
    }
    values.insert(0, context.symbol(owned::Name::new(Some("cljs.core"), "fn")));
    let data = context.list(&values).map_err(|e| e.to_string())?;
    let form = context
        .macro_form(&data, &original, owned::Name::new(Some("cljs.core"), "fn"))
        .map_err(|e| e.to_string())?;
    own_form_with_sources(&form, sources)
}
