//! Test-only capture at the compiler's actual emission and expansion boundaries.
//! Production builds have neither the hooks nor the thread-local recorders.
use super::*;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

struct Request {
    call: Form,
    environment: Datum,
    expanded: Form,
    origin: ExpansionOrigin,
}
#[cfg(feature = "goose-macros")]
pub(crate) static GENERATED_FORMS: std::sync::Mutex<Option<Vec<(Name, Form, Form)>>> =
    std::sync::Mutex::new(None);
#[cfg(feature = "goose-macros")]
pub(crate) static CONCURRENT_FORMS: std::sync::Mutex<Option<Vec<(Form, Form)>>> =
    std::sync::Mutex::new(None);
#[cfg(feature = "goose-macros")]
pub(crate) fn compare_concurrent_forms(mut a: Vec<(Form, Form)>, mut b: Vec<(Form, Form)>) {
    let key = |pair: &(Form, Form)| {
        (
            pair.0.span().source().name().to_owned(),
            pair.0.span().bytes().start,
        )
    };
    a.sort_by_key(key);
    b.sort_by_key(key);
    assert_eq!(a.len(), b.len());
    for ((call_a, a), (call_b, b)) in a.iter().zip(&b) {
        benchmark::compare(call_a, call_b);
        benchmark::compare(a, b);
    }
}
#[cfg(feature = "goose-macros")]
fn anonymous_argument_index(token: &str) -> Option<i64> {
    match token {
        "%" => Some(1),
        "%&" => Some(-1),
        _ => token.strip_prefix('%')?.parse().ok(),
    }
}
#[cfg(feature = "goose-macros")]
pub(crate) fn validate_generated_scalar_cases(call: &Form, output: &Form) -> usize {
    fn at(form: &Form, path: &[usize]) -> Form {
        path.iter()
            .fold(form.clone(), |f, &i| f.children()[i].clone())
    }
    fn occurrence(input: Form, output: Form) {
        assert_eq!(input.node(), output.node());
        assert_eq!(input.span().source().name(), output.span().source().name());
        assert_eq!(input.span().source().text(), output.span().source().text());
        assert_eq!(
            input.span().bytes(),
            output.span().bytes(),
            "independent scalar occurrence"
        );
        let (mut a, mut b) = (Data::default(), Data::default());
        a.add(&input.data(), 0);
        b.add(&output.data(), 0);
        assert_eq!(a.text(), b.text());
    }
    let span = call.span();
    if span.source().name().ends_with("/workload/pleistum.cljs") && span.bytes() == (6865..6909) {
        // Actual doubled declaration: `(let [~sym (norm (* 2 ~init))] ~@body).
        // The initializer and body are two independently located integer 9s.
        occurrence(at(call, &[1, 1]), at(output, &[1, 1, 1, 2]));
        occurrence(at(call, &[2]), at(output, &[2]));
        2
    } else if span.source().name().ends_with("/workload/storhouher.cljs")
        && span.bytes() == (18455..18625)
    {
        // Actual sum-all maps each x to `(* 19 ~x), inside `(norm (+ ...)).
        // Preserve each argument's position, including both integer 313s.
        for (i, input) in call.children().into_iter().skip(1).enumerate() {
            occurrence(input, at(output, &[1, i + 1, 2]));
        }
        3
    } else {
        0
    }
}
#[cfg(feature = "goose-macros")]
pub(crate) fn validate_generated_anonymous(
    input: &Form,
    reference: &Form,
    output: &Form,
) -> (usize, usize, usize) {
    use crate::compiler_data::{DatumKind, FormId, Origin};
    struct Anonymous {
        form: Form,
        param: Form,
        index: i64,
        body: Vec<std::ops::Range<usize>>,
    }
    fn functions(root: &Form) -> Vec<Anonymous> {
        fn walk(f: &Form, ids: &mut std::collections::HashSet<FormId>, result: &mut Vec<Form>) {
            if !ids.insert(f.id()) {
                return;
            }
            result.push(f.clone());
            for child in f.children() {
                walk(&child, ids, result);
            }
            if let Some(meta) = f.metadata() {
                walk(&meta, ids, result);
            }
            if let Some(payload) = f.node().compiler_payload.and_then(|id| f.at(id)) {
                walk(&payload, ids, result);
            }
        }
        let mut forms = Vec::new();
        walk(root, &mut Default::default(), &mut forms);
        let mut result = Vec::new();
        for f in forms {
            if f.node().origin != Origin::AnonymousFunction {
                continue;
            }
            let children = f.children();
            if children.len() != 3
                || !matches!(children[0].data().kind(), DatumKind::Symbol(n) if n.name() == "fn*".encode_utf16().collect::<Vec<_>>())
            {
                continue;
            }
            let params = children[1].children();
            if params.is_empty() {
                continue;
            }
            fn uses(f: &Form, param: &Datum, result: &mut Vec<std::ops::Range<usize>>) {
                if f.node().origin == Origin::AnonymousArgument && f.data().value_eq(param) {
                    result.push(f.span().bytes());
                }
                for child in f.children() {
                    uses(&child, param, result);
                }
                if let Some(meta) = f.metadata() {
                    uses(&meta, param, result);
                }
            }
            let (mut positional, mut rest) = (0, false);
            for param in params {
                assert_eq!(param.node().origin, Origin::AnonymousArgument);
                if matches!(param.data().kind(), DatumKind::Symbol(n) if n == &Name::new(None, "&"))
                {
                    assert_eq!(param.span().bytes(), f.span().bytes());
                    rest = true;
                    continue;
                }
                let index = if rest {
                    -1
                } else {
                    positional += 1;
                    positional
                };
                let mut body = Vec::new();
                uses(&children[2], &param.data(), &mut body);
                result.push(Anonymous {
                    form: f.clone(),
                    param,
                    index,
                    body,
                });
            }
        }
        result
    }
    let same = |a: &Anonymous, b: &Anonymous| {
        a.form.span().source().name() == b.form.span().source().name()
            && a.form.span().source().text() == b.form.span().source().text()
            && a.form.span().bytes() == b.form.span().bytes()
            && a.param.data().value_eq(&b.param.data())
    };
    let (inputs, references, outputs) = (functions(input), functions(reference), functions(output));
    let (mut raw, mut inherited) = (0, 0);
    for f in &inputs {
        let span = f.form.span();
        let tree = crate::parse(span.source().text()).unwrap();
        assert!(tree.nodes.iter().any(|n| n.span == span.bytes()
            && matches!(
                n.kind,
                crate::NodeKind::Collection {
                    kind: crate::CollectionKind::AnonymousFn,
                    ..
                }
            )));
        let expected = tree
            .nodes
            .iter()
            .filter(|n| {
                matches!(n.kind, crate::NodeKind::Argument { .. })
                    && span.bytes().contains(&n.span.start)
                    && n.span.end <= span.bytes().end
                    && anonymous_argument_index(&span.source().text()[n.span.clone()])
                        == Some(f.index)
            })
            .map(|n| n.span.clone())
            .collect::<Vec<_>>();
        if f.param.span().bytes() == span.bytes() && f.body == expected {
            raw += 1;
            continue;
        }
        // Rust pure expansion deliberately keeps the last occurrence of a
        // shared Datum (compiler_data's existing pure-expansion regression).
        // This must already be present, identically, in both compiler inputs;
        // Goose may never introduce it while returning an originally precise fn.
        let last = expected.last();
        assert!(
            last.is_some_and(|last| f.param.span().bytes() == *last
                && f.body == vec![last.clone(); expected.len()]
                && references.iter().any(|r| same(f, r)
                    && r.param.span().bytes() == f.param.span().bytes()
                    && r.body == f.body)),
            "unclassified compiler input occurrences: {} {:?} binder={:?} body={:?}",
            span.source().name(),
            span.bytes(),
            f.param.span().bytes(),
            f.body
        );
        eprintln!(
            "common compiler anonymous alias: source={} function={:?} last={last:?}",
            span.source().name(),
            span.bytes()
        );
        inherited += 1;
    }
    for f in &outputs {
        let original = inputs
            .iter()
            .find(|i| same(i, f))
            .expect("returned anonymous function must come from this job's input");
        assert_eq!(
            f.param.span().bytes(),
            original.param.span().bytes(),
            "Goose must preserve the input binder occurrence: {} {:?}",
            f.form.span().source().name(),
            f.form.span().bytes()
        );
        assert_eq!(
            f.body, original.body,
            "Goose must preserve every ordered input body occurrence"
        );
    }
    (raw, inherited, outputs.len())
}
#[cfg(feature = "goose-macros")]
pub(crate) fn compare_generated_reference(call: &Form, v8: &Form, goose: &Form) -> usize {
    use crate::compiler_data::{DatumKind, Origin};
    assert_eq!(v8.id(), goose.id());
    assert_eq!(v8.nodes(), goose.nodes());
    let (mut sa, mut sb, mut da, mut db) = (
        Sources::default(),
        Sources::default(),
        Data::default(),
        Data::default(),
    );
    forms(v8, &mut sa, &mut da);
    forms(goose, &mut sb, &mut db);
    assert_eq!(da.text(), db.text());
    assert_eq!(sa.text(), sb.text());
    benchmark::compare_origins(v8, goose);
    let mut originals = Vec::new();
    fn collect(f: Form, originals: &mut Vec<Form>) {
        originals.push(f.clone());
        for child in f.children() {
            collect(child, originals);
        }
        if let Some(meta) = f.metadata() {
            collect(meta, originals);
        }
        if let Some(payload) = f.node().compiler_payload.and_then(|id| f.at(id)) {
            collect(payload, originals);
        }
    }
    collect(call.clone(), &mut originals);
    let mut aliases = 0;
    for id in 0..v8.nodes().len() {
        let a = v8.at(crate::compiler_data::FormId(id)).unwrap();
        let b = goose.at(crate::compiler_data::FormId(id)).unwrap();
        assert_eq!(a.span().source().name(), b.span().source().name());
        assert_eq!(a.span().source().text(), b.span().source().text());
        if a.span().bytes() == b.span().bytes() {
            continue;
        }
        // Reader-created anonymous parameter/binder uses can share a Datum.
        // V8's retain() overwrites that identity's earlier Form with its last
        // occurrence. Goose's bridge validates and retains each wire occurrence.
        // Accept only this independently identifiable reference ambiguity;
        // Goose-to-Goose comparisons remain completely exact.
        let input = originals.iter().find(|f| {
            f.node().origin == Origin::AnonymousArgument
                && f.span().bytes() == b.span().bytes()
                && f.span().source().name() == b.span().source().name()
                && f.data().value_eq(&b.data())
        });
        let latest = input.and_then(|input| {
            originals
                .iter()
                .rev()
                .find(|f| f.data().identity() == input.data().identity())
        });
        // A later call can retain an earlier expansion's whole arena, including
        // anonymous parameters outside its reachable subtree. Check the exact
        // #() binder and last matching placeholder in the immutable source.
        let parameter = match b.data().kind() {
            DatumKind::Symbol(name)
                if name.namespace().is_none() && name.name().last() == Some(&u16::from(b'#')) =>
            {
                let text = String::from_utf16_lossy(name.name());
                text.split_once("__").and_then(|(prefix, _)| {
                    if prefix == "rest" {
                        Some(-1)
                    } else {
                        prefix.strip_prefix('p')?.parse::<i64>().ok()
                    }
                })
            }
            _ => None,
        };
        let source_alias = if let Some(parameter) = parameter {
            let span = b.span();
            let tree = crate::parse(span.source().text()).unwrap();
            tree.nodes.iter().any(|node| {
                if !matches!(
                    node.kind,
                    crate::NodeKind::Collection {
                        kind: crate::CollectionKind::AnonymousFn,
                        ..
                    }
                ) || !node.span.contains(&span.bytes().start)
                    || span.bytes().end > node.span.end
                {
                    return false;
                }
                let uses = tree
                    .nodes
                    .iter()
                    .filter(|n| {
                        matches!(n.kind, crate::NodeKind::Argument { .. })
                            && node.span.contains(&n.span.start)
                            && n.span.end <= node.span.end
                            && anonymous_argument_index(&span.source().text()[n.span.clone()])
                                == Some(parameter)
                    })
                    .map(|n| n.span.clone())
                    .collect::<Vec<_>>();
                (span.bytes() == node.span || uses.contains(&span.bytes()))
                    && uses.last().is_some_and(|last| *last == a.span().bytes())
            })
        } else {
            false
        };
        let scalar_case = b.node().origin == Origin::Source
            && match (
                call.span().source().name(),
                call.span().bytes(),
                a.span().bytes(),
                b.span().bytes(),
                b.data().kind(),
            ) {
                (name, call, v8, goose, DatumKind::Integer(9))
                    if name.ends_with("/workload/pleistum.cljs") =>
                {
                    call == (6865..6909) && v8 == (6907..6908) && goose == (6904..6905)
                }
                (name, call, v8, goose, DatumKind::Integer(313))
                    if name.ends_with("/workload/storhouher.cljs") =>
                {
                    call == (18455..18625) && v8 == (18621..18624) && goose == (18579..18582)
                }
                _ => false,
            };
        assert!(
            scalar_case
                || (matches!(b.data().kind(), DatumKind::Symbol(_))
                    && b.node().origin == Origin::AnonymousArgument
                    && (latest.is_some_and(|f| f.span().bytes() == a.span().bytes()
                        && f.node().origin == a.node().origin)
                        || source_alias)),
            "unclassified reference provenance: node={id} v8={:?} Goose={:?} data={:?} call={:?}",
            a.span().bytes(),
            b.span().bytes(),
            b.data(),
            call.span()
        );
        eprintln!(
            "reference alias: source={} node={id} v8={:?} Goose={:?}",
            b.span().source().name(),
            a.span().bytes(),
            b.span().bytes()
        );
        aliases += 1;
    }
    aliases
}
#[cfg(feature = "goose-macros")]
#[test]
fn generated_reference_ambiguity_requires_original_alias() {
    use crate::compiler_data::{DatumKind, FormId, FormKind, FormNode, Origin, Source};
    let source = Source::new("alias.cljs", "#(% )");
    let shared = Datum::new(DatumKind::Symbol(Name::new(None, "p1")));
    let make = |first, second, shared_identity| {
        let other = if shared_identity {
            shared.clone()
        } else {
            Datum::new(DatumKind::Symbol(Name::new(None, "p1")))
        };
        Form::from_parts(
            vec![
                FormNode {
                    kind: FormKind::Scalar,
                    metadata: None,
                    compiler_payload: None,
                    origin: Origin::AnonymousArgument,
                },
                FormNode {
                    kind: FormKind::Scalar,
                    metadata: None,
                    compiler_payload: None,
                    origin: Origin::AnonymousArgument,
                },
                FormNode {
                    kind: FormKind::Collection {
                        kind: crate::parser::CollectionKind::Vector,
                        children: vec![FormId(0), FormId(1)].into(),
                    },
                    metadata: None,
                    compiler_payload: None,
                    origin: Origin::AnonymousFunction,
                },
            ],
            vec![
                shared.clone(),
                other.clone(),
                Datum::new(DatumKind::Collection {
                    kind: crate::parser::CollectionKind::Vector,
                    children: vec![shared.clone(), other].into(),
                }),
            ],
            vec![
                source.span(first).unwrap(),
                source.span(second).unwrap(),
                source.span(0..5).unwrap(),
            ],
            FormId(2),
        )
    };
    let call = make(0..5, 2..3, true);
    let v8 = make(2..3, 2..3, true);
    assert_eq!(compare_generated_reference(&call, &v8, &call), 1);
    for (call, v8, goose) in [
        (call.clone(), v8.clone(), make(1..2, 2..3, true)),
        (call.clone(), make(1..2, 2..3, true), call.clone()),
        (make(0..5, 2..3, false), v8, call),
    ] {
        assert!(
            std::panic::catch_unwind(|| compare_generated_reference(&call, &v8, &goose)).is_err()
        );
    }
    let call = crate::compiler_reader::read_form(
        &source,
        &crate::compiler_reader::NamespaceView::new("fixture", &source),
    )
    .unwrap()
    .unwrap();
    let v8 = Form::from_parts(
        call.nodes().to_vec(),
        (0..call.nodes().len())
            .map(|i| call.at(FormId(i)).unwrap().data())
            .collect(),
        (0..call.nodes().len())
            .map(|i| {
                let f = call.at(FormId(i)).unwrap();
                if f.node().origin == Origin::AnonymousArgument {
                    source.span(2..3).unwrap()
                } else {
                    f.span()
                }
            })
            .collect(),
        call.id(),
    );
    let unrelated = Form::from_data(
        Datum::new(DatumKind::Nil),
        source.span(0..1).unwrap(),
        Origin::Source,
    );
    assert_eq!(compare_generated_reference(&unrelated, &v8, &call), 1);
    assert_eq!(validate_generated_anonymous(&call, &call, &call), (1, 0, 1));
    assert!(std::panic::catch_unwind(|| validate_generated_anonymous(&call, &v8, &v8)).is_err());
    assert_eq!(validate_generated_anonymous(&v8, &v8, &v8), (0, 1, 1));
    for (text, parameters) in [("#(+ %1 %2)", 2), ("#(identity %2)", 2), ("#(count %&)", 1)] {
        let source = Source::new("arguments.cljs", text);
        let call = crate::compiler_reader::read_form(
            &source,
            &crate::compiler_reader::NamespaceView::new("fixture", &source),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            validate_generated_anonymous(&call, &call, &call),
            (parameters, 0, parameters)
        );
    }
}
thread_local! {
    static REQUESTS: std::cell::RefCell<Option<Vec<Request>>> = const { std::cell::RefCell::new(None) };
    static UNITS: std::cell::RefCell<Option<BTreeSet<(bool, String)>>> = const { std::cell::RefCell::new(None) };
}
pub(crate) fn record(call: &Form, environment: &Datum, expanded: &Form, origin: &ExpansionOrigin) {
    #[cfg(feature = "goose-macros")]
    if let Some(requests) = GENERATED_FORMS.lock().unwrap().as_mut()
        && origin
            .macro_name
            .namespace()
            .is_some_and(|n| String::from_utf16_lossy(n).starts_with("workload."))
    {
        requests.push((origin.macro_name.clone(), call.clone(), expanded.clone()));
    }
    #[cfg(feature = "goose-macros")]
    if let Some(forms) = CONCURRENT_FORMS.lock().unwrap().as_mut()
        && origin
            .macro_name
            .namespace()
            .is_some_and(|n| n == "macros".encode_utf16().collect::<Vec<_>>())
    {
        forms.push((call.clone(), expanded.clone()));
    }
    #[cfg(feature = "goose-macros")]
    benchmark::record_batch(call, origin);
    REQUESTS.with_borrow_mut(|requests| {
        if let Some(requests) = requests {
            requests.push(Request {
                call: call.clone(),
                environment: environment.clone(),
                expanded: expanded.clone(),
                origin: origin.clone(),
            });
        }
    });
}
pub(crate) fn unit(source: &str, native: bool) {
    UNITS.with_borrow_mut(|units| {
        if let Some(units) = units {
            units.insert((native, source.to_owned()));
        }
    });
}
fn artifact(path: &Path, bytes: &[u8], regenerate: bool) {
    if regenerate {
        std::fs::write(path, bytes).unwrap();
    } else {
        assert_eq!(std::fs::read(path).unwrap(), bytes, "{}", path.display());
    }
}
fn output(js: &str) -> String {
    let out = std::process::Command::new("node")
        .arg("-e")
        .arg(js)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

// Variable payloads are flat pools. Datum IDs are occurrence IDs, not equality
// keys; equal values never erase distinct form nodes or source locations.
#[derive(Default)]
struct Data {
    nodes: Vec<String>,
    children: Vec<u32>,
    units: Vec<u16>,
}
impl Data {
    fn string(&mut self, units: &[u16]) -> (usize, usize) {
        let start = self.units.len();
        self.units.extend_from_slice(units);
        assert!(self.units.len() <= 1_048_576);
        (start, units.len())
    }
    fn name(&mut self, name: &Name) -> String {
        let (start, len) = self.string(name.name());
        let namespace = name.namespace().map_or("nil".into(), |ns| {
            let (s, n) = self.string(ns);
            format!("[{s} {n}]")
        });
        format!(":namespace {namespace} :name [{start} {len}]")
    }
    fn add(&mut self, datum: &Datum, depth: usize) -> u32 {
        assert!(depth <= 128 && self.nodes.len() < 100_000);
        use crate::compiler_data::DatumKind::*;
        let metadata = datum
            .metadata()
            .map_or("nil".into(), |d| self.add(&d, depth + 1).to_string());
        let value = match datum.kind() {
            Nil => ":nil".into(),
            Boolean(b) => format!(":boolean :value {b}"),
            Integer(n) => format!(":integer :value {n}"),
            Double(n) => format!(":double :bits {}N", n.to_bits()),
            Ratio {
                numerator,
                denominator,
            } => format!(":ratio :numerator {numerator} :denominator {denominator}"),
            Character(c) => format!(":character :unit {c}"),
            String(s) => {
                let (start, len) = self.string(s);
                format!(":string :units [{start} {len}]")
            }
            Symbol(n) => format!(":symbol {}", self.name(n)),
            Keyword(n) => format!(":keyword {}", self.name(n)),
            Collection { kind, children } => {
                let ids = children
                    .iter()
                    .map(|d| self.add(d, depth + 1))
                    .collect::<Vec<_>>();
                let start = self.children.len();
                self.children.extend(&ids);
                assert!(self.children.len() <= 1_048_576);
                format!(
                    ":collection :kind :{} :children [{start} {}]",
                    format!("{kind:?}").to_lowercase(),
                    ids.len()
                )
            }
            CompilerLiteral { kind, payload } => format!(
                ":compiler-literal :kind :{} :payload {}",
                format!("{kind:?}").to_lowercase(),
                self.add(payload, depth + 1)
            ),
        };
        let id = u32::try_from(self.nodes.len()).unwrap();
        self.nodes
            .push(format!("{{:tag {value} :metadata {metadata}}}"));
        id
    }
    fn text(&self) -> String {
        format!(
            "{{:nodes [{}] :children {:?} :units {:?}}}",
            self.nodes.join("\n"),
            self.children,
            self.units
        )
    }
}
#[derive(Default)]
struct Sources(Vec<crate::compiler_data::Source>);
impl Sources {
    fn add(&mut self, source: &crate::compiler_data::Source) -> usize {
        if let Some(id) = self.0.iter().position(|s| s.id() == source.id()) {
            return id;
        }
        self.0.push(source.clone());
        self.0.len() - 1
    }
    fn text(&self) -> String {
        format!(
            "[{}]",
            self.0
                .iter()
                .map(|s| format!("{{:name {:?} :utf8 {:?}}}", s.name(), s.text().as_bytes()))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}
fn forms(form: &Form, sources: &mut Sources, data: &mut Data) -> String {
    use crate::compiler_data::FormKind;
    let mut nodes = Vec::new();
    let mut children = Vec::new();
    for id in 0..form.nodes().len() {
        let value = form.at(crate::compiler_data::FormId(id)).unwrap();
        let node = &form.nodes()[id];
        let span = value.span();
        let bytes = span.bytes();
        assert!(span.source().text().get(bytes.clone()).is_some());
        let source = sources.add(span.source());
        let kind = match &node.kind {
            FormKind::Scalar => format!(":scalar :datum {}", data.add(&value.data(), 0)),
            FormKind::Collection {
                kind,
                children: ids,
            } => {
                let start = children.len();
                for id in ids {
                    assert!(id.0 < form.nodes().len());
                    children.push(id.0);
                }
                format!(
                    ":{} :children [{start} {}]",
                    format!("{kind:?}").to_lowercase(),
                    ids.len()
                )
            }
        };
        let metadata = node.metadata.map_or("nil".into(), |id| id.0.to_string());
        let payload = node
            .compiler_payload
            .map_or("nil".into(), |id| id.0.to_string());
        nodes.push(format!("{{:kind {kind} :source {source} :span [{} {}] :metadata {metadata} :payload {payload} :origin {:?}}}", bytes.start, bytes.end, format!("{:?}", node.origin)));
    }
    format!(
        "{{:root {} :nodes [{}] :children {children:?}}}",
        form.id().0,
        nodes.join("\n")
    )
}
fn exported(
    root: &Path,
    filename: &str,
    source: &str,
    regenerate: bool,
) -> big_boy_pants_js::frontend::goose::Export {
    let path = root.join(filename);
    artifact(&path, source.as_bytes(), regenerate);
    let export = big_boy_pants_js::frontend::goose::export_source(filename, source)
        .unwrap_or_else(|error| panic!("{filename}: {error}"));
    assert_eq!(&export.bytes[32..32 + source.len()], source.as_bytes());
    artifact(&path.with_extension("gsp"), &export.bytes, regenerate);
    let inventory = format!(
        "{{:parsed true :exported true :goose-executed false :functions {} :instructions {} :operations {:?}}}\n",
        export.functions,
        export.instructions,
        export.operations.iter().collect::<Vec<_>>()
    );
    artifact(
        &path.with_extension("inventory.edn"),
        inventory.as_bytes(),
        regenerate,
    );
    export
}

#[test]
fn source_corpus() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("crates/big-boy-pants-js/prototypes/goose/corpus");
    let regenerate = std::env::var_os("GOOSE_REGENERATE").is_some();
    let schema = big_boy_pants_js::frontend::goose::inspection_schema();
    artifact(
        &root.parent().unwrap().join("export_schema.goose"),
        schema.as_bytes(),
        regenerate,
    );
    let mut runtime_names = BTreeSet::new();
    let mut form_operations = BTreeSet::new();
    for (case, names, expected) in [
        ("churn", &["churn"][..], "0 168 5250\n"),
        ("closure", &["captured"][..], "[[1 11] [2 12] [3 13]]\n"),
        (
            "branches",
            &["left", "right", "consume"][..],
            "[[2 3] [9 16] [2 3 9 16]]\n",
        ),
        ("effect", &["announce"][..], "[7 true]\n"),
        ("context", &["context"][..], "4 4\n"),
    ] {
        let source = std::fs::read_to_string(root.join(format!("{case}.cljs"))).unwrap();
        let mut references = Vec::new();
        for native in [false, true] {
            let mut host = MacroHost::new();
            if native {
                host.use_native_macros();
            }
            let _names = crate::generated_names::enter_namespace(0);
            REQUESTS.with_borrow_mut(|r| *r = Some(Vec::new()));
            UNITS.with_borrow_mut(|r| *r = Some(Default::default()));
            let program = crate::analyzer::analyze_named_with_macro_warnings(
                &format!("{case}.cljs"),
                &source,
                &crate::parse(&source).unwrap(),
                &mut host,
                Default::default(),
                &mut Vec::new(),
            )
            .unwrap_or_else(|error| panic!("{case}, native={native}: {error}"));
            let requests = REQUESTS
                .with_borrow_mut(|r| r.take().unwrap())
                .into_iter()
                .filter(|r| {
                    r.origin
                        .macro_name
                        .namespace()
                        .is_some_and(|ns| ns == "cljs.user".encode_utf16().collect::<Vec<_>>())
                })
                .collect::<Vec<_>>();
            assert!(!requests.is_empty());
            let engine = if native { "pants" } else { "v8" };
            let mut sources = Sources::default();
            let mut data = Data::default();
            let mut jobs = Vec::new();
            for (id, request) in requests.iter().enumerate() {
                let call = data.add(&request.call.data(), 0);
                let arguments = request
                    .call
                    .children()
                    .iter()
                    .skip(1)
                    .map(|f| f.id().0)
                    .collect::<Vec<_>>();
                let environment = data.add(&request.environment, 0);
                let expansion = data.add(&request.expanded.data(), 0);
                let input = forms(&request.call, &mut sources, &mut data);
                let output = forms(&request.expanded, &mut sources, &mut data);
                let macro_name = data.name(&request.origin.macro_name);
                jobs.push(format!("{{:id {id} :kind :macro-call :macro {{{macro_name}}} :call {call} :arguments {arguments:?} :environment {environment} :abi :form-env-input-form-ids :expected-expansion {expansion} :input-forms {input} :expected-output-forms {output}}}"));
            }
            let mut definitions = Vec::new();
            let mut macro_units = BTreeSet::new();
            for name in names {
                let key = format!("cljs.user/{name}");
                let declaration = &host.tables.javascript_macros[&key];
                let MacroFunction::Ready(_, Some(code)) = &declaration.function else {
                    panic!("{key}: actual source macro code required");
                };
                macro_units.insert(code.unit.clone());
                let factory = format!("rt => rt.core.macroReaderFunction(({})(rt))", code.unit);
                let filename = format!("{case}-{name}.{engine}.js");
                let export = exported(&root, &filename, &factory, regenerate);
                runtime_names.extend(export.string_constants);
                form_operations.extend(export.form_operations);
                let span = declaration.definition.bytes();
                assert!(source.get(span.clone()).is_some() && span.end > span.start);
                definitions.push(format!("{{:name {key:?} :generation {} :source {} :span [{} {}] :factory {filename:?}}}", declaration.generation, sources.add(declaration.definition.source()), span.start, span.end));
            }
            let units = UNITS.with_borrow_mut(|r| r.take().unwrap());
            let helpers = units
                .iter()
                .filter(|(is_native, unit)| *is_native == native && !macro_units.contains(unit))
                .collect::<Vec<_>>();
            for (index, (_, unit)) in helpers.iter().enumerate() {
                let export = exported(
                    &root,
                    &format!("{case}-helper-{index}.{engine}.js"),
                    unit,
                    regenerate,
                );
                runtime_names.extend(export.string_constants);
                form_operations.extend(export.form_operations);
            }
            if case == "branches" {
                assert_eq!(
                    helpers.len(),
                    2,
                    "both demanded producer factories must be captured"
                );
            }
            let effects = if case == "effect" {
                "[:compiler-gensym]"
            } else {
                "[]"
            };
            let counts = host.session_handle().unwrap().unit_counts();
            if case == "branches" {
                assert_eq!(
                    counts.executed, 2,
                    "both producer definitions must actually execute"
                );
            }
            let uses = program
                .compilation_uses
                .iter()
                .map(|u| {
                    format!(
                        "{{:name {:?} :generation {} :analyses {} :emissions {}}}",
                        u.name, u.generation, u.analyses, u.emissions
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            let jobs = format!(
                "{{:version 2 :sources {} :data {} :definitions [{}] :jobs [{}] :helper-factories {} :definition-uses [{uses}] :producer-executions {} :declared-effects {effects}}}\n",
                sources.text(),
                data.text(),
                definitions.join("\n"),
                jobs.join("\n"),
                helpers.len(),
                counts.executed
            );
            artifact(
                &root.join(format!("{case}.{engine}.jobs.edn")),
                jobs.as_bytes(),
                regenerate,
            );
            let stdout = output(&crate::emit_js(&program));
            assert_eq!(stdout, expected, "{case}, native={native}");
            references.push((stdout, requests));
            // Host and all engine-owned values die here. The comparisons below
            // still inspect the compiler-owned expansion values and provenance.
        }
        assert_eq!(
            references[0].0, references[1].0,
            "independent reference engines: {case}"
        );
        assert_eq!(references[0].1.len(), references[1].1.len());
        for (a, b) in references[0].1.iter().zip(&references[1].1) {
            assert!(a.call.data().value_eq(&b.call.data()));
            assert!(a.expanded.data().value_eq(&b.expanded.data()));
            assert_eq!(a.call.span().bytes(), b.call.span().bytes());
            assert_eq!(a.origin.macro_name, b.origin.macro_name);
            assert_eq!(a.expanded.nodes().len(), b.expanded.nodes().len());
            for id in 0..a.expanded.nodes().len() {
                let left = a
                    .expanded
                    .at(crate::compiler_data::FormId(id))
                    .unwrap()
                    .span();
                let right = b
                    .expanded
                    .at(crate::compiler_data::FormId(id))
                    .unwrap()
                    .span();
                assert_eq!(left.bytes(), right.bytes());
                assert_eq!(left.source().name(), right.source().name());
            }
        }
        artifact(
            &root.join(format!("{case}.stdout")),
            expected.as_bytes(),
            regenerate,
        );
    }
    runtime_slice(&root, &runtime_names, &form_operations, regenerate);
}

#[test]
fn synthetic_source_export() {
    use big_boy_pants_js::frontend::{ast::StmtKind, parse_script};
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("crates/big-boy-pants-js/prototypes/goose/corpus/synthetic");
    let regenerate = std::env::var_os("GOOSE_REGENERATE").is_some();
    let mut functions = Vec::new();
    for (name, expected) in [("arithmetic", 27.5), ("control_flow", 7307.0)] {
        let source = std::fs::read_to_string(root.join(format!("{name}.js"))).unwrap();
        let parsed = parse_script(&source).unwrap();
        let last = parsed.body.last().unwrap();
        assert!(matches!(last.kind, StmtKind::Expr(_)));
        // Adapt historical script completion to an entry-function return using
        // the existing parser's span. This does not implement ordinary calls.
        functions.push((
            name,
            format!(
                "function synthetic() {{\n{}return {};\n}}",
                &source[..last.span.start],
                &source[last.span.start..last.span.end]
            ),
            expected,
        ));
    }
    let mut wide = String::from("function wide() {\n");
    for i in 0..300 {
        wide.push_str(&format!("let local{i} = {i};\n"));
    }
    wide.push_str("if (local299 < 0) return -1; return local299;\n}");
    functions.push(("wide", wide, 299.0));
    for (name, function, expected) in functions {
        let export = big_boy_pants_js::frontend::goose::export_function(&function).unwrap();
        artifact(
            &root.join(format!("{name}.function.js")),
            function.as_bytes(),
            regenerate,
        );
        artifact(&root.join(format!("{name}.gsp")), &export.bytes, regenerate);
        let reference = output(&format!("console.log(({function})());"));
        assert_eq!(reference.trim().parse::<f64>().unwrap(), expected);
        let rt = big_boy_pants_js::Runtime::new();
        let exe = big_boy_pants_js::frontend::compile_function(&function).unwrap();
        let big_boy_pants_js::RunOutcome::Complete(value) =
            rt.spawn(&exe, &[]).unwrap().run().unwrap()
        else {
            panic!("reference must complete")
        };
        assert_eq!(value.view(), big_boy_pants_js::ValueView::Number(expected));
    }
    let unicode = "function unicode() { return '😀\\ud800'; }";
    let export = big_boy_pants_js::frontend::goose::export_function(unicode).unwrap();
    artifact(
        &root.join("unicode.function.js"),
        unicode.as_bytes(),
        regenerate,
    );
    artifact(&root.join("unicode.gsp"), &export.bytes, regenerate);
}

fn runtime_slice(
    root: &Path,
    names: &BTreeSet<String>,
    form_operations: &BTreeSet<String>,
    regenerate: bool,
) {
    use crate::runtime_manifest::{CoreReference, RootUse, RuntimeManifest};
    let manifest = RuntimeManifest::embedded();
    let mut bindings = BTreeSet::new();
    let mut core = Vec::new();
    for name in names {
        if let Some(reference) = manifest.core_reference(name) {
            let binding = match reference {
                CoreReference::Binding(b) | CoreReference::Member(b, _) => b,
                CoreReference::Cell(_) => "coreCells",
            };
            bindings.insert(binding);
            core.push(format!("[{name:?} {binding:?}]"));
        } else if name != "core" && manifest.exports.names.contains(&name.as_str()) {
            bindings.insert(name.as_str());
        }
    }
    assert!(bindings.contains("macroReaderFunction"));
    let roots = bindings
        .iter()
        .map(|b| (manifest.unit_of(b).unwrap(), RootUse::Call))
        .collect::<Vec<_>>();
    let reached = manifest.reached_by_uses(&roots);
    let kept = reached.iter().map(Option::is_some).collect::<Vec<_>>();
    let bindings = bindings.into_iter().collect::<Vec<_>>();
    let source = manifest.sliced_text(&kept, &bindings);
    // This is a reference runtime source selection, not a claim that the
    // scalar Goose VM implements its classes, private stores or host hooks.
    let units = manifest
        .units
        .iter()
        .zip(&kept)
        .filter(|(_, kept)| **kept)
        .map(|(u, _)| {
            format!(
                "{{:name {:?} :source \"src/js_runtime.js\" :span [{} {}]}}",
                u.name, u.body_range.start, u.body_range.end
            )
        })
        .collect::<Vec<_>>();
    let inventory = format!(
        "{{:selection :conservative-string-constant-roots :native-runtime-source \"src/bbp_runtime.js\" :native-reader-source \"src/bbp_reader.js\" :core-bindings [{}] :exports {bindings:?} :units [{}] :native-form-leaves {:?} :runtime-source-exported false :goose-executed false}}\n",
        core.join("\n"),
        units.join("\n"),
        form_operations.iter().collect::<Vec<_>>()
    );
    artifact(&root.join("runtime.js"), source.as_bytes(), regenerate);
    artifact(&root.join("runtime.edn"), inventory.as_bytes(), regenerate);
    // Independently instantiate the mechanically selected source in V8. The
    // native profile is intentionally the documented form-leaf boundary.
    let probe =
        format!("const runtime = {source}; console.log(typeof runtime.macroReaderFunction);");
    assert_eq!(output(&probe), "function\n");
}

// Driven by macros.bb so timing binaries are built before any measured round.
// The compiler capture is input preparation, outside all reported engine phases.
#[cfg(light_v8)]
#[test]
#[ignore = "serial source-derived macro diagnostic; run macros.bb --measure"]
fn serial_macro_reference() {
    use crate::evaluator::{EvaluatorOptions, MacroEvaluator};
    use std::time::Instant;
    let engine = std::env::var("GOOSE_SERIAL_ENGINE").unwrap();
    assert!(matches!(engine.as_str(), "pants" | "v8"));
    let destination = std::env::var("GOOSE_SERIAL_OUTPUT").unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("crates/big-boy-pants-js/prototypes/goose/corpus");
    let source = std::fs::read_to_string(root.join("churn.cljs")).unwrap();
    let prepared = Instant::now();
    let mut host = MacroHost::new();
    if engine == "pants" {
        host.use_native_macros();
    }
    let _names = crate::generated_names::enter_namespace(0);
    REQUESTS.with_borrow_mut(|r| *r = Some(Vec::new()));
    crate::analyzer::analyze_named_with_macro_warnings(
        "churn.cljs",
        &source,
        &crate::parse(&source).unwrap(),
        &mut host,
        Default::default(),
        &mut Vec::new(),
    )
    .unwrap();
    let requests = REQUESTS
        .with_borrow_mut(|r| r.take().unwrap())
        .into_iter()
        .filter(|r| {
            r.origin
                .macro_name
                .namespace()
                .is_some_and(|ns| ns == "cljs.user".encode_utf16().collect::<Vec<_>>())
        })
        .collect::<Vec<_>>();
    assert_eq!(requests.len(), 3);
    let declaration = &host.tables.javascript_macros["cljs.user/churn"];
    let MacroFunction::Ready(_, Some(code)) = &declaration.function else {
        panic!("source macro required")
    };
    let factory = format!("rt => rt.core.macroReaderFunction(({})(rt))", code.unit);
    assert_eq!(
        factory,
        std::fs::read_to_string(root.join(format!("churn-churn.{engine}.js"))).unwrap()
    );
    let export_started = Instant::now();
    let image_name = format!("churn-churn.{engine}.js");
    let image = big_boy_pants_js::frontend::goose::export_source(&image_name, &factory).unwrap();
    assert_eq!(
        image.bytes,
        std::fs::read(root.join(format!("churn-churn.{engine}.gsp"))).unwrap()
    );
    let source_export_ns = export_started.elapsed().as_nanos();
    let prepared_ns = prepared.elapsed().as_nanos();
    drop(host);
    let begin = Instant::now();
    let options = EvaluatorOptions::default();
    let evaluator: Box<dyn MacroEvaluator> = if engine == "pants" {
        Box::new(crate::bbp_evaluator::BbpEvaluator::new(options).unwrap())
    } else {
        Box::new(crate::v8_evaluator::V8Evaluator::new(options).unwrap())
    };
    let boot_ns = begin.elapsed().as_nanos();
    let begin = Instant::now();
    let function = evaluator.evaluate_unit(&factory).unwrap();
    let load_ns = begin.elapsed().as_nanos();
    let mut rows = format!(
        "{{:phase :boot :engine :{engine} :prepare-ns {prepared_ns} :gsp-export-ns {source_export_ns} :boot-ns {boot_ns} :load-ns {load_ns} :boot-count 1 :initializers 1 :producers 0}}\n"
    );
    let limits = crate::form_bridge::ConversionLimits::default();
    // One full 0/16/100 warmup, then two matching batches on the same session.
    for batch in 0..3 {
        for (index, request) in requests.iter().enumerate() {
            let begin = Instant::now();
            let form = evaluator.import_macro_form(&request.call, limits).unwrap();
            let env = evaluator
                .import_environment(&request.environment, limits)
                .unwrap();
            let arguments = evaluator
                .macro_arguments(form, &request.call, limits)
                .unwrap();
            let mut params = vec![form, env];
            params.extend(arguments.iter().copied());
            let import_ns = begin.elapsed().as_nanos();
            let begin = Instant::now();
            let value = evaluator.call(function, &params).unwrap();
            let invoke_ns = begin.elapsed().as_nanos();
            let begin = Instant::now();
            let output = evaluator
                .export_form(value, &request.call, request.origin.clone(), limits)
                .unwrap();
            let export_ns = begin.elapsed().as_nanos();
            let begin = Instant::now();
            for id in params.into_iter().chain([value]) {
                evaluator.release(id).unwrap();
            }
            let cleanup_ns = begin.elapsed().as_nanos();
            // Compare complete owned records after handle release, outside timing.
            let mut expected_sources = Sources::default();
            let mut expected_data = Data::default();
            let mut actual_sources = Sources::default();
            let mut actual_data = Data::default();
            assert_eq!(
                forms(&request.expanded, &mut expected_sources, &mut expected_data),
                forms(&output, &mut actual_sources, &mut actual_data)
            );
            assert_eq!(expected_sources.text(), actual_sources.text());
            // JS Number exports can differ from reader integer storage; value_eq
            // follows the compiler's cross-engine numeric equality contract.
            assert!(request.expanded.data().value_eq(&output.data()));
            rows.push_str(&format!("{{:phase :job :warmup {} :id {} :case {} :import-ns {import_ns} :invoke-ns {invoke_ns} :export-ns {export_ns} :cleanup-ns {cleanup_ns}}}\n", batch == 0, batch * requests.len() + index, [0,16,100][index]));
        }
    }
    evaluator.release(function).unwrap();
    let begin = Instant::now();
    drop(evaluator);
    rows.push_str(&format!(
        "{{:phase :teardown :cleanup-ns {}}}\n",
        begin.elapsed().as_nanos()
    ));
    std::fs::write(destination, rows).unwrap();
}

#[cfg(feature = "goose-macros")]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_pipeline() {
    let runner = std::path::PathBuf::from(
        std::env::var_os("GOOSE_COMPILER_RUNNER").expect("run bb prototypes/goose/compiler.bb"),
    );
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("crates/big-boy-pants-js/prototypes/goose/corpus");
    for case in [
        "churn",
        "closure",
        "branches",
        "context",
        "reentrant",
        "catch",
    ] {
        let source = std::fs::read_to_string(root.join(format!("{case}.cljs"))).unwrap();
        let mut builds = Vec::new();
        for (engine, workers) in [
            ("v8", 1),
            ("pants", 1),
            ("goose", 1),
            ("goose", 2),
            ("goose", 4),
            ("goose", 8),
        ] {
            let goose = engine == "goose";
            let mut host = MacroHost::new();
            if engine == "pants" {
                host.use_native_macros();
            }
            let session = goose
                .then(|| crate::goose_evaluator::GooseSession::start(&runner, workers).unwrap());
            if let Some(session) = &session {
                host.use_goose_source_macros(session.clone()).unwrap();
                if case == "reentrant" {
                    host.engine
                        .goose
                        .as_ref()
                        .unwrap()
                        .helper_delay
                        .set(std::time::Duration::from_millis(20));
                }
            }
            let _names = crate::generated_names::enter_namespace(0);
            REQUESTS.with_borrow_mut(|r| *r = Some(Vec::new()));
            let program = crate::analyzer::analyze_named_with_macro_warnings(
                &format!("{case}.cljs"),
                &source,
                &crate::parse(&source).unwrap(),
                &mut host,
                Default::default(),
                &mut Vec::new(),
            )
            .unwrap_or_else(|e| panic!("{case}, Goose={goose}: {e}"));
            let requests = REQUESTS
                .with_borrow_mut(|r| r.take().unwrap())
                .into_iter()
                .filter(|r| {
                    r.origin
                        .macro_name
                        .namespace()
                        .is_some_and(|ns| ns == "cljs.user".encode_utf16().collect::<Vec<_>>())
                })
                .collect::<Vec<_>>();
            if let Some(session) = session {
                let stats = session.stats();
                assert_eq!(stats.subprocesses, 1);
                assert_eq!(stats.initializations, 1);
                assert_eq!(stats.jobs, requests.len() as u64);
                let trace = session.trace();
                let producers = match case {
                    "branches" => 2,
                    "reentrant" => 1,
                    _ => 0,
                };
                assert_eq!(
                    trace
                        .iter()
                        .filter(|line| line.starts_with("PRODUCED "))
                        .count(),
                    producers,
                    "{case} producer count, workers={workers}"
                );
                if case == "reentrant" {
                    let need = trace.iter().find(|line| line.starts_with("NEED ")).unwrap();
                    assert!(
                        need.split_whitespace()
                            .last()
                            .unwrap()
                            .parse::<usize>()
                            .unwrap()
                            > 0,
                        "pre-wait mutation must be saved"
                    );
                    assert!(trace.iter().any(|line| line == "START 2"));
                    let finish = trace
                        .iter()
                        .find(|line| line.starts_with("FINISH 1 "))
                        .unwrap();
                    assert_eq!(
                        need.split_whitespace().last(),
                        finish.split_whitespace().last(),
                        "pre-wait writes must never replay"
                    );
                }
                eprintln!("{case}: {stats:?} {trace:?}");
                session.close();
            }
            drop(host);
            // Emission, returned form inspection and analysis happen after full process teardown.
            for request in &requests {
                crate::analyzer::analyze_compiler_form(&request.expanded).unwrap();
            }
            let js = crate::emit_js(&program);
            builds.push((output(&js), js, requests));
        }
        for b in &builds[1..] {
            let a = &builds[0];
            assert_eq!(a.0, b.0, "{case} application output");
            assert_eq!(a.1, b.1, "{case} deterministic JS");
            assert_eq!(a.2.len(), b.2.len());
            for (a, b) in a.2.iter().zip(&b.2) {
                assert!(
                    a.expanded.data().value_eq(&b.expanded.data()),
                    "{case} canonical form"
                );
                assert_eq!(a.expanded.nodes().len(), b.expanded.nodes().len());
                for id in 0..a.expanded.nodes().len() {
                    let (a, b) = (
                        a.expanded.at(crate::compiler_data::FormId(id)).unwrap(),
                        b.expanded.at(crate::compiler_data::FormId(id)).unwrap(),
                    );
                    assert_eq!(a.span().bytes(), b.span().bytes(), "{case} source spans");
                    assert_eq!(
                        a.span().source().text(),
                        b.span().source().text(),
                        "{case} source ownership"
                    );
                    assert_eq!(
                        a.node().kind,
                        b.node().kind,
                        "{case} logical child occurrence IDs"
                    );
                    assert_eq!(
                        a.node().metadata,
                        b.node().metadata,
                        "{case} metadata edges"
                    );
                    assert!(
                        a.data().value_eq(&b.data()),
                        "{case} nested data and metadata"
                    );
                    assert_eq!(
                        std::mem::discriminant(a.data().kind()),
                        std::mem::discriminant(b.data().kind()),
                        "{case} canonical datum kinds"
                    );
                    assert_eq!(a.node().origin, b.node().origin, "{case} origin");
                }
            }
        }
    }
}

#[cfg(feature = "goose-macros")]
#[path = "benchmark.rs"]
mod benchmark;
