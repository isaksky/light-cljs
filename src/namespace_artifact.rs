//! A completed namespace as an owned, self-contained artifact
//! (docs/compiler-cache.md, "Binary artifacts").
//!
//! An artifact holds what installing the namespace later needs without
//! analyzing its source or executing its macro calls: its declarations, its
//! semantic IR before application optimization, the changes it made to
//! earlier namespaces' globals, its deferred units in prepared form, and its
//! diagnostics. It is captured where `NamespaceAnalysis::analyze` returns
//! (`crate::artifact_capture`), only when a compilation asks for it.
//!
//! These types are relocatable by construction. Nothing here names an arena
//! ID of a program (`ExprId`, `GlobalId`, `LocalId` and the rest), a source
//! index, a compilation unit ID, a reference count or an engine handle:
//! - references to the artifact's own entries are dense indices into its
//!   own tables ([`ExprRef`], [`LocalRef`], [`GlobalRef::Own`], ...);
//! - references to earlier namespaces are stable identities, namespace and
//!   name ([`ExternalGlobal`], [`SourceRef::Namespace`]);
//! - deferred units are indices into [`DeferredUnits::units`], and compile-time
//!   JavaScript that names a unit ID records where ([`RelocatableCode`]).
//!
//! The only items this module takes from the compiler are plain enums of
//! values (`CoreFnId`, `ExprContext`, `CompilerLiteralKind`,
//! `CollectionKind`, `MethodDispatch`); `tests/compiler/namespace_artifacts.rs`
//! checks that this file names nothing else.

use std::fmt::Write as _;

pub use crate::analyzer::{CoreFnId, ExprContext, MethodDispatch};
pub use crate::compiler_data::CompilerLiteralKind;
pub use crate::parser::CollectionKind;

/// Byte offsets into the text of the source a [`SourceRef`] names.
pub type Bytes = (u32, u32);

/// An expression of [`SemanticIr::exprs`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ExprRef(pub u32);
/// A lexical binding of [`SemanticIr::locals`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LocalRef(pub u32);
/// A loop or function target of [`SemanticIr::recur_targets`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct RecurRef(pub u32);
/// UTF-16 text of [`SemanticIr::strings`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct StringRef(pub u32);
/// A keyword or symbol name of [`SemanticIr::named_literals`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct NamedRef(pub u32);
/// A host path of [`SemanticIr::js_globals`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct JsGlobalRef(pub u32);
/// A loadable site of [`Declarations::lazy_targets`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LazyRef(pub u32);
/// A deferred unit of [`DeferredUnits::units`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct UnitRef(pub u32);

/// A global binding: one this namespace's analysis created
/// ([`Declarations::globals`]) or one an earlier namespace did, by identity
/// ([`References::globals`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum GlobalRef {
    Own(u32),
    External(u32),
}

/// The source an expression, binding or warning came from: this namespace's,
/// or an earlier namespace's ([`References::namespaces`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SourceRef {
    Own,
    Namespace(u32),
}

/// What capture produced for one namespace: its artifact, or why it has none.
#[derive(Debug)]
pub struct NamespaceCapture {
    pub namespace: String,
    pub artifact: Result<NamespaceArtifact, String>,
}

/// One namespace's compilation, owned and relocatable.
#[derive(Clone, Debug, PartialEq)]
pub struct NamespaceArtifact {
    pub header: Header,
    pub declarations: Declarations,
    pub ir: SemanticIr,
    /// Changes this namespace made to globals created before it, such as a
    /// definition of a Var an earlier namespace reserved by referring to it.
    pub patches: Vec<GlobalPatch>,
    pub units: DeferredUnits,
    pub diagnostics: Diagnostics,
    pub references: References,
    /// Generated names the artifact retains (light-cljs-317); capture leaves
    /// this empty.
    pub names: Option<GeneratedNames>,
    /// Logical work the artifact charged (light-cljs-318); capture leaves
    /// this empty.
    pub accounting: Option<Accounting>,
}

/// Source identity and the shape of the namespace's body.
#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    pub namespace: String,
    /// The file the loader read, as it named it.
    pub path: String,
    pub source_length: u32,
    /// Read as `.cljc`, with the `:cljs` branches selected.
    pub cljc: bool,
    /// The file name analysis reports, when it is not the path.
    pub source_name: Option<String>,
    /// Whether this was the entry namespace, whose body returns the program's
    /// value rather than discarding its last form.
    pub entry: bool,
    /// The namespace's body: the root `finish` runs for this source.
    pub root: ExprRef,
}

/// Earlier namespaces and their globals, by stable identity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct References {
    pub namespaces: Vec<String>,
    pub globals: Vec<ExternalGlobal>,
}

/// A global created before this namespace. A program has one global for each
/// namespace and name, so the pair identifies it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalGlobal {
    pub namespace: String,
    pub name: String,
    /// Declared when this namespace's analysis ended, rather than only
    /// reserved by a reference.
    pub declared: bool,
    /// Its visibility in the compile-time session: `Some(true)` for a private
    /// Var, `None` when no Var registered it.
    pub private: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingKind {
    Value,
    Macro,
}

/// What a namespace declares, and the inputs `finish` reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Declarations {
    /// The globals this namespace's analysis created, in creation order. Most
    /// are its own Vars; a reference can reserve another namespace's.
    pub globals: Vec<Global>,
    /// Protocols this namespace defined, with their methods.
    pub protocols: Vec<(GlobalRef, Vec<ProtocolMethod>)>,
    pub record_constructors: Vec<GlobalRef>,
    /// Record and type fields: the binding, its type's recur target, and
    /// whether the field is mutable.
    pub field_bindings: Vec<(LocalRef, RecurRef, bool)>,
    pub type_methods: Vec<RecurRef>,
    /// Functions compiled as macros, which take two implicit parameters.
    pub macro_functions: Vec<ExprRef>,
    pub macros: Vec<MacroDeclaration>,
    /// The `ns` form's aliases and requires, in its order. `finish` turns the
    /// requires into `namespace_requires`.
    pub aliases: Vec<(String, String)>,
    pub requires: Vec<String>,
    /// How names resolve: `(local, namespace, original)` refers, excluded
    /// core names and `:refer`red host names.
    pub refers: Vec<(String, String, String)>,
    pub excludes: Vec<String>,
    pub js_refers: Vec<(String, JsRefer)>,
    /// The reader context the program registers for this namespace.
    pub reader_context: ReaderContext,
    /// The macro host's view of the `ns` form: imports, renamed macros,
    /// `:import`ed host names, and the namespace's symbol and docstring.
    pub imports: Vec<Import>,
    pub renames: Vec<(String, String, String)>,
    pub namespace_imports: Vec<(String, String)>,
    pub namespace_symbol: Option<Datum>,
    pub namespace_doc: Option<Datum>,
    /// Every `light.lazy/loadable` site of the namespace, in analysis order.
    pub lazy_targets: Vec<LazyTarget>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsRefer {
    Global(String),
    Package(String, Option<String>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReaderContext {
    pub aliases: Vec<(String, String)>,
    pub refers: Vec<(String, String, String)>,
    pub excludes: Vec<String>,
    pub macros: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Import {
    pub namespace: String,
    pub alias: Option<String>,
    pub refers: Vec<String>,
}

/// A global binding as analysis left it.
#[derive(Clone, Debug, PartialEq)]
pub struct Global {
    pub namespace: String,
    pub name: String,
    pub flags: GlobalFlags,
    pub metadata: Option<ExprRef>,
    pub span: Bytes,
    pub source: SourceRef,
    /// Registered as a Var in the compile-time session, and whether private.
    pub private: Option<bool>,
    /// Its kind in the compile-time session, when one is registered.
    pub binding: Option<BindingKind>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GlobalFlags {
    pub type_descriptor: bool,
    pub declared: bool,
    pub constant: bool,
    pub define: bool,
    pub boolean_tag: bool,
}

/// An earlier global after this namespace changed it.
#[derive(Clone, Debug, PartialEq)]
pub struct GlobalPatch {
    pub global: GlobalRef,
    pub flags: GlobalFlags,
    pub metadata: Option<ExprRef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LazyTarget {
    /// The `ExprKind::Loadable` site.
    pub site: ExprRef,
    pub namespace: String,
    /// The target Var, or `None` for a namespace target.
    pub global: Option<GlobalRef>,
}

/// A `defmacro` of this namespace, compiled to compile-time JavaScript.
#[derive(Clone, Debug, PartialEq)]
pub struct MacroDeclaration {
    pub name: String,
    /// `CompiledMacro.generation`, also its `:light-cljs/generation`: one more
    /// than the definition it replaced.
    pub generation: u32,
    pub metadata: Datum,
    /// The `defmacro` form's place, for diagnostics naming the macro.
    pub definition: Site,
    /// The function the macro host evaluates, and the declarations it saw.
    pub code: RelocatableCode,
    pub view: RegistrationView,
}

/// A source place by file name, for provenance outside the namespace's IR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    pub source: String,
    pub bytes: Bytes,
}

/// Compile-time JavaScript, with the places its text names a deferred unit
/// (`globalThis.__lightCompilerTest(N)`), to rewrite when the units are
/// registered again under other IDs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RelocatableCode {
    pub text: String,
    /// Each unit ID's byte range in `text`, in text order.
    pub units: Vec<(Bytes, UnitRef)>,
}

/// The declarations a compile-time unit saw when it was defined: every
/// earlier namespace as it was when this one began, the first `own_globals`
/// of [`Declarations::globals`], and the globals its own analysis added;
/// and the namespace's reader context as the unit's registration names it,
/// which can lack macros the namespace declares later.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegistrationView {
    pub own_globals: u32,
    pub added: Vec<GlobalRef>,
    pub reader_context: Option<ReaderContext>,
}

/// The semantic IR of the namespace's body and of its drained deferred units,
/// before application optimization.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SemanticIr {
    pub exprs: Vec<Expr>,
    pub locals: Vec<LocalBinding>,
    pub recur_targets: Vec<RecurTarget>,
    /// The UTF-16 text of every string and name the IR uses.
    pub strings: Vec<Vec<u16>>,
    pub named_literals: Vec<NamedLiteral>,
    pub js_globals: Vec<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Bytes,
    pub context: ExprContext,
    pub source: SourceRef,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalBinding {
    pub name: String,
    pub span: Bytes,
    pub source: SourceRef,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecurTarget {
    pub params: Vec<LocalRef>,
    pub protocol_impl: bool,
    pub receiver_from_this: bool,
    pub used: bool,
    pub argument_slots: Vec<Option<u32>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NamedLiteral {
    pub namespace: Option<StringRef>,
    pub name: StringRef,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProtocolMethod {
    pub global: GlobalRef,
    pub key: String,
    pub arities: Vec<u32>,
    pub variadic: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FnMethod {
    pub params: Vec<LocalRef>,
    pub rest: Option<LocalRef>,
    pub body: ExprRef,
    pub target: RecurRef,
    pub captures: Vec<LocalRef>,
    pub asynchronous: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CatchClause {
    pub class: Option<ExprRef>,
    pub binding: LocalRef,
    pub body: ExprRef,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeLayout {
    pub constructor: Option<GlobalRef>,
    pub body: Option<ExprRef>,
    pub masks: Option<ExprRef>,
    pub map_constructor: Option<GlobalRef>,
    pub fields: Vec<String>,
}

/// `crate::analyzer::ExprKind` with artifact references.
#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Nil,
    Bool(bool),
    Number(f64),
    String(StringRef),
    ReaderLiteral {
        kind: CompilerLiteralKind,
        items: Vec<ExprRef>,
        quoted: bool,
    },
    DefinitionTest {
        value: ExprRef,
        unit: UnitRef,
    },
    WithMeta {
        value: ExprRef,
        metadata: ExprRef,
    },
    Var {
        definition: bool,
        value: ExprRef,
        symbol: NamedRef,
        metadata: Option<VarMetadata>,
    },
    Keyword(NamedRef),
    Symbol(NamedRef),
    List(Vec<ExprRef>),
    Vector(Vec<ExprRef>),
    Map(Vec<ExprRef>),
    Set(Vec<ExprRef>),
    LiteralMap(Vec<ExprRef>),
    LiteralSet(Vec<ExprRef>),
    CoreFn(CoreFnId),
    Local(LocalRef),
    Global(GlobalRef),
    JsGlobal(JsGlobalRef),
    ThisAs {
        binding: LocalRef,
        body: ExprRef,
    },
    JsArguments,
    JsTemplate {
        segments: Vec<String>,
        args: Vec<ExprRef>,
    },
    JsIndex {
        receiver: ExprRef,
        indices: Vec<ExprRef>,
    },
    JsDelete {
        receiver: ExprRef,
        key: ExprRef,
    },
    QualifiedMethod {
        class: ExprRef,
        method: Option<String>,
    },
    CopyArguments(ExprRef),
    DefProtocol {
        global: GlobalRef,
        methods: Vec<ProtocolMethod>,
        via_metadata: bool,
    },
    ProtocolPredicate {
        protocol: ExprRef,
        value: ExprRef,
        key: String,
        direct: bool,
    },
    ExtendType {
        instance: bool,
        return_target: bool,
        target: ExprRef,
        methods: Vec<(MethodDispatch, FnMethod)>,
    },
    Reify {
        metadata: Option<ExprRef>,
        captures: Vec<LocalRef>,
        methods: Vec<(MethodDispatch, FnMethod)>,
    },
    DefType {
        record: bool,
        global: GlobalRef,
        methods: Vec<(MethodDispatch, FnMethod)>,
        layout: Box<TypeLayout>,
    },
    Construct {
        callee: ExprRef,
        args: Vec<ExprRef>,
    },
    Assert {
        test: ExprRef,
        message: Option<ExprRef>,
        form: StringRef,
    },
    Def {
        global: GlobalRef,
        init: Option<ExprRef>,
        test: Option<ExprRef>,
        once: bool,
    },
    SetGlobal {
        target: ExprRef,
        value: ExprRef,
    },
    Binding {
        bindings: Vec<(ExprRef, ExprRef)>,
        body: ExprRef,
    },
    WithOutStr(ExprRef),
    Exists(ExprRef),
    Loadable {
        target: LazyRef,
        class: ExprRef,
    },
    LoadableType,
    BindingExists(ExprRef),
    BooleanHint(ExprRef),
    Property {
        receiver: ExprRef,
        property: String,
    },
    MethodCall {
        receiver: ExprRef,
        method: String,
        args: Vec<ExprRef>,
    },
    If {
        test: ExprRef,
        yes: ExprRef,
        no: ExprRef,
    },
    Do(Vec<ExprRef>),
    Let {
        bindings: Vec<(LocalRef, ExprRef)>,
        body: ExprRef,
    },
    LetFn {
        bindings: Vec<(LocalRef, ExprRef)>,
        body: ExprRef,
    },
    Loop {
        target: RecurRef,
        bindings: Vec<(LocalRef, ExprRef)>,
        body: ExprRef,
    },
    Recur {
        target: RecurRef,
        args: Vec<ExprRef>,
    },
    Throw(ExprRef),
    Await(ExprRef),
    Try {
        body: ExprRef,
        catches: Vec<CatchClause>,
        finally: Option<ExprRef>,
    },
    Case {
        strict: bool,
        keyword_tests: bool,
        test: ExprRef,
        branches: Vec<(ExprRef, ExprRef)>,
        default: Option<ExprRef>,
    },
    Fn {
        name: Option<LocalRef>,
        methods: Vec<FnMethod>,
    },
    Call {
        callee: ExprRef,
        args: Vec<ExprRef>,
    },
    InstanceOf {
        class: ExprRef,
        value: ExprRef,
    },
}

/// The metadata a `(var x)` reflects: an expression of the artifact, or the
/// metadata expression an earlier namespace's global had when this namespace
/// began, which the var form shares rather than copies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarMetadata {
    Expr(ExprRef),
    Global(GlobalRef),
}

/// The namespace's compile-time units and their registrations, in the
/// compilation session's order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeferredUnits {
    pub units: Vec<Unit>,
    pub registrations: Vec<Registration>,
}

/// A compile-time unit, analyzed and prepared: its IR is the namespace's
/// ([`SemanticIr`]), and what remains for the session is its JavaScript.
#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    pub name: UnitName,
    /// Its definition's generation among the session's definitions of the
    /// same name: 1 for the first.
    pub generation: u32,
    /// How often it was analyzed, and whether a macro ran it (its
    /// `CompilationUse`).
    pub analyses: u32,
    pub emitted: bool,
    pub code: RelocatableCode,
    pub view: RegistrationView,
    /// The extensions visible where it was defined: how many bundles this
    /// namespace had registered before it.
    pub extension_frontier: u32,
}

/// A unit's catalog name. Declaration bundles and `:test` metadata units are
/// numbered by unit ID, so they are recorded without the number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitName {
    /// `ns/name`, the definition's Var.
    Var(String),
    /// `declaration#N`.
    Declaration,
    /// `<definition name> :test#N`.
    Test(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Registration {
    /// A definition's compile-time root; `unit` is `None` for a definition
    /// without an initializer.
    Root {
        name: String,
        unit: Option<UnitRef>,
        test: Option<UnitRef>,
        once: bool,
        location: String,
    },
    /// A top-level declaration bundle: the Vars it defines, by `ns/name`, and
    /// the extensions it installs.
    Bundle {
        products: Vec<String>,
        unit: UnitRef,
        extensions: Vec<Extension>,
        once: bool,
        /// Analyzed with its owner rather than from its own snapshot.
        group: bool,
        location: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extension {
    pub protocol: String,
    pub selector: Option<String>,
    pub identity: ExtensionIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionIdentity {
    /// `method@<source index>:<offset>`: a `defmethod` at this byte offset of
    /// this namespace's source.
    Method(u32),
    Text(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diagnostics {
    /// The warnings the namespace's analysis reported, in order.
    pub warnings: Vec<Warning>,
    /// Every source macro call the analysis expanded, in expansion order,
    /// nested expansions included: the macro-origin record of the IR they
    /// produced. Compiler-owned `cljs.core` macros are part of the compiler
    /// and are not listed.
    pub expansions: Vec<MacroExpansion>,
}

/// One executed source macro call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroExpansion {
    pub macro_namespace: String,
    pub name: String,
    /// The definition that ran ([`MacroDeclaration::generation`]).
    pub generation: u32,
    pub call: Bytes,
    pub source: SourceRef,
    pub definition: Site,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub kind: String,
    pub message: String,
    pub span: Bytes,
    pub source: SourceRef,
}

/// Reserved for light-cljs-317.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeneratedNames {}

/// Reserved for light-cljs-318.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Accounting {}

/// Reader data, owned.
#[derive(Clone, Debug, PartialEq)]
pub struct Datum {
    pub kind: DatumKind,
    pub metadata: Option<Box<Datum>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DatumKind {
    Nil,
    Boolean(bool),
    Integer(i64),
    Double(f64),
    Ratio {
        numerator: i64,
        denominator: i64,
    },
    Character(u16),
    String(Vec<u16>),
    Symbol(Option<Vec<u16>>, Vec<u16>),
    Keyword(Option<Vec<u16>>, Vec<u16>),
    Collection {
        kind: CollectionKind,
        children: Vec<Datum>,
    },
    CompilerLiteral {
        kind: CompilerLiteralKind,
        payload: Box<Datum>,
    },
}

/// Every reference an expression kind holds, for [`NamespaceArtifact::validate`].
enum Reference {
    Expr(ExprRef),
    Local(LocalRef),
    Recur(RecurRef),
    String(StringRef),
    Named(NamedRef),
    Global(GlobalRef),
    JsGlobal(JsGlobalRef),
    Lazy(LazyRef),
    Unit(UnitRef),
}

fn method_references(method: &FnMethod, visit: &mut impl FnMut(Reference)) {
    for &local in method
        .params
        .iter()
        .chain(&method.rest)
        .chain(&method.captures)
    {
        visit(Reference::Local(local));
    }
    visit(Reference::Expr(method.body));
    visit(Reference::Recur(method.target));
}

fn kind_references(kind: &ExprKind, visit: &mut impl FnMut(Reference)) {
    use ExprKind::*;
    use Reference as R;
    let exprs = |ids: &[ExprRef], visit: &mut dyn FnMut(Reference)| {
        for &id in ids {
            visit(R::Expr(id));
        }
    };
    match kind {
        Nil | Bool(_) | Number(_) | CoreFn(_) | JsArguments | LoadableType => {}
        String(id) => visit(R::String(*id)),
        Keyword(id) | Symbol(id) => visit(R::Named(*id)),
        Local(id) => visit(R::Local(*id)),
        Global(id) => visit(R::Global(*id)),
        JsGlobal(id) => visit(R::JsGlobal(*id)),
        Loadable { target, class } => {
            visit(R::Lazy(*target));
            visit(R::Expr(*class));
        }
        ReaderLiteral { items, .. }
        | List(items)
        | Vector(items)
        | Map(items)
        | Set(items)
        | LiteralMap(items)
        | LiteralSet(items)
        | Do(items) => exprs(items, visit),
        DefinitionTest { value, unit } => {
            visit(R::Expr(*value));
            visit(R::Unit(*unit));
        }
        WithMeta { value, metadata } => {
            visit(R::Expr(*value));
            visit(R::Expr(*metadata));
        }
        Var {
            value,
            symbol,
            metadata,
            ..
        } => {
            visit(R::Expr(*value));
            visit(R::Named(*symbol));
            match metadata {
                Some(VarMetadata::Expr(id)) => visit(R::Expr(*id)),
                Some(VarMetadata::Global(global)) => visit(R::Global(*global)),
                None => {}
            }
        }
        ThisAs { binding, body } => {
            visit(R::Local(*binding));
            visit(R::Expr(*body));
        }
        JsTemplate { args, .. } => exprs(args, visit),
        JsIndex { receiver, indices } => {
            visit(R::Expr(*receiver));
            exprs(indices, visit);
        }
        JsDelete { receiver, key } => {
            visit(R::Expr(*receiver));
            visit(R::Expr(*key));
        }
        QualifiedMethod { class, .. } => visit(R::Expr(*class)),
        CopyArguments(id) | WithOutStr(id) | Exists(id) | BindingExists(id) | BooleanHint(id)
        | Throw(id) | Await(id) => visit(R::Expr(*id)),
        DefProtocol {
            global, methods, ..
        } => {
            visit(R::Global(*global));
            for method in methods {
                visit(R::Global(method.global));
            }
        }
        ProtocolPredicate {
            protocol, value, ..
        } => {
            visit(R::Expr(*protocol));
            visit(R::Expr(*value));
        }
        ExtendType {
            target, methods, ..
        } => {
            visit(R::Expr(*target));
            for (_, method) in methods {
                method_references(method, visit);
            }
        }
        Reify {
            metadata,
            captures,
            methods,
        } => {
            exprs(metadata.as_slice(), visit);
            for &local in captures {
                visit(R::Local(local));
            }
            for (_, method) in methods {
                method_references(method, visit);
            }
        }
        DefType {
            global,
            methods,
            layout,
            ..
        } => {
            visit(R::Global(*global));
            for &global in layout.constructor.iter().chain(&layout.map_constructor) {
                visit(R::Global(global));
            }
            for &id in layout.body.iter().chain(&layout.masks) {
                visit(R::Expr(id));
            }
            for (_, method) in methods {
                method_references(method, visit);
            }
        }
        Construct { callee, args } | Call { callee, args } => {
            visit(R::Expr(*callee));
            exprs(args, visit);
        }
        Assert {
            test,
            message,
            form,
        } => {
            visit(R::Expr(*test));
            exprs(message.as_slice(), visit);
            visit(R::String(*form));
        }
        Def {
            global, init, test, ..
        } => {
            visit(R::Global(*global));
            for &id in init.iter().chain(test) {
                visit(R::Expr(id));
            }
        }
        SetGlobal { target, value } => {
            visit(R::Expr(*target));
            visit(R::Expr(*value));
        }
        Binding { bindings, body } => {
            for &(a, b) in bindings {
                visit(R::Expr(a));
                visit(R::Expr(b));
            }
            visit(R::Expr(*body));
        }
        Property { receiver, .. } => visit(R::Expr(*receiver)),
        MethodCall { receiver, args, .. } => {
            visit(R::Expr(*receiver));
            exprs(args, visit);
        }
        If { test, yes, no } => {
            for id in [test, yes, no] {
                visit(R::Expr(*id));
            }
        }
        Let { bindings, body } | LetFn { bindings, body } => {
            for &(local, init) in bindings {
                visit(R::Local(local));
                visit(R::Expr(init));
            }
            visit(R::Expr(*body));
        }
        Loop {
            target,
            bindings,
            body,
        } => {
            visit(R::Recur(*target));
            for &(local, init) in bindings {
                visit(R::Local(local));
                visit(R::Expr(init));
            }
            visit(R::Expr(*body));
        }
        Recur { target, args } => {
            visit(R::Recur(*target));
            exprs(args, visit);
        }
        Try {
            body,
            catches,
            finally,
        } => {
            visit(R::Expr(*body));
            for catch in catches {
                exprs(catch.class.as_slice(), visit);
                visit(R::Local(catch.binding));
                visit(R::Expr(catch.body));
            }
            exprs(finally.as_slice(), visit);
        }
        Case {
            test,
            branches,
            default,
            ..
        } => {
            visit(R::Expr(*test));
            for &(a, b) in branches {
                visit(R::Expr(a));
                visit(R::Expr(b));
            }
            exprs(default.as_slice(), visit);
        }
        Fn { name, methods } => {
            if let Some(name) = name {
                visit(R::Local(*name));
            }
            for method in methods {
                method_references(method, visit);
            }
        }
        InstanceOf { class, value } => {
            visit(R::Expr(*class));
            visit(R::Expr(*value));
        }
    }
}

impl NamespaceArtifact {
    /// Check that every reference the artifact holds names an entry of its
    /// own tables, so installing it needs nothing but the artifact and the
    /// identities it names.
    pub fn validate(&self) -> Result<(), String> {
        let ir = &self.ir;
        let fits = |what: &str, index: u32, len: usize| {
            if (index as usize) < len {
                Ok(())
            } else {
                Err(format!("{what} {index} is outside its table of {len}"))
            }
        };
        let source = |source: SourceRef| match source {
            SourceRef::Own => Ok(()),
            SourceRef::Namespace(index) => {
                fits("namespace", index, self.references.namespaces.len())
            }
        };
        let global = |global: GlobalRef| match global {
            GlobalRef::Own(index) => fits("global", index, self.declarations.globals.len()),
            GlobalRef::External(index) => {
                fits("external global", index, self.references.globals.len())
            }
        };
        let expr = |id: ExprRef| fits("expression", id.0, ir.exprs.len());
        let local = |id: LocalRef| fits("local", id.0, ir.locals.len());
        let recur = |id: RecurRef| fits("recur target", id.0, ir.recur_targets.len());
        let unit = |id: UnitRef| fits("unit", id.0, self.units.units.len());
        let string = |id: StringRef| fits("string", id.0, ir.strings.len());
        let mut error = None;
        for (index, entry) in ir.exprs.iter().enumerate() {
            source(entry.source)?;
            kind_references(&entry.kind, &mut |reference| {
                let checked = match reference {
                    Reference::Expr(id) => expr(id),
                    Reference::Local(id) => local(id),
                    Reference::Recur(id) => recur(id),
                    Reference::String(id) => string(id),
                    Reference::Named(id) => fits("name", id.0, ir.named_literals.len()),
                    Reference::Global(id) => global(id),
                    Reference::JsGlobal(id) => fits("host path", id.0, ir.js_globals.len()),
                    Reference::Lazy(id) => {
                        fits("lazy target", id.0, self.declarations.lazy_targets.len())
                    }
                    Reference::Unit(id) => unit(id),
                };
                if let Err(message) = checked {
                    error.get_or_insert(format!("expression {index}: {message}"));
                }
            });
        }
        if let Some(error) = error {
            return Err(error);
        }
        for binding in &ir.locals {
            source(binding.source)?;
        }
        for target in &ir.recur_targets {
            target.params.iter().try_for_each(|&id| local(id))?;
        }
        for literal in &ir.named_literals {
            literal.namespace.iter().try_for_each(|&id| string(id))?;
            string(literal.name)?;
        }
        expr(self.header.root)?;
        let declarations = &self.declarations;
        for entry in &declarations.globals {
            source(entry.source)?;
            entry.metadata.iter().try_for_each(|&id| expr(id))?;
        }
        for (protocol, methods) in &declarations.protocols {
            global(*protocol)?;
            methods
                .iter()
                .try_for_each(|method| global(method.global))?;
        }
        declarations
            .record_constructors
            .iter()
            .try_for_each(|&id| global(id))?;
        for &(binding, target, _) in &declarations.field_bindings {
            local(binding)?;
            recur(target)?;
        }
        declarations
            .type_methods
            .iter()
            .try_for_each(|&id| recur(id))?;
        declarations
            .macro_functions
            .iter()
            .try_for_each(|&id| expr(id))?;
        for target in &declarations.lazy_targets {
            expr(target.site)?;
            target.global.iter().try_for_each(|&id| global(id))?;
        }
        for patch in &self.patches {
            global(patch.global)?;
            patch.metadata.iter().try_for_each(|&id| expr(id))?;
        }
        // The references are in text order and do not overlap, so
        // installation can rewrite them in one pass.
        let code = |code: &RelocatableCode| {
            let mut after = 0;
            code.units.iter().try_for_each(|&((start, end), id)| {
                unit(id)?;
                if start < after {
                    return Err(format!(
                        "unit reference {start}..{end} overlaps or precedes the one before it"
                    ));
                }
                after = end;
                if start <= end && code.text.get(start as usize..end as usize).is_some() {
                    Ok(())
                } else {
                    Err(format!("unit reference {start}..{end} is outside its code"))
                }
            })
        };
        let view = |view: &RegistrationView| {
            fits(
                "view frontier",
                view.own_globals,
                declarations.globals.len() + 1,
            )?;
            view.added.iter().try_for_each(|&id| global(id))
        };
        for declaration in &declarations.macros {
            code(&declaration.code)?;
            view(&declaration.view)?;
        }
        for entry in &self.units.units {
            code(&entry.code)?;
            view(&entry.view)?;
        }
        for registration in &self.units.registrations {
            match registration {
                Registration::Root {
                    unit: root, test, ..
                } => {
                    root.iter().chain(test).try_for_each(|&id| unit(id))?;
                }
                Registration::Bundle { unit: bundle, .. } => unit(*bundle)?,
            }
        }
        for warning in &self.diagnostics.warnings {
            source(warning.source)?;
        }
        for expansion in &self.diagnostics.expansions {
            source(expansion.source)?;
        }
        Ok(())
    }

    /// An estimate of the bytes the artifact owns, for `--stats`.
    pub fn approximate_bytes(&self) -> usize {
        use std::mem::size_of;
        let text = |s: &String| s.len();
        let code =
            |c: &RelocatableCode| c.text.len() + c.units.len() * size_of::<(Bytes, UnitRef)>();
        let ir = &self.ir;
        let mut bytes = size_of::<Self>()
            + ir.exprs.len() * size_of::<Expr>()
            + ir.locals
                .iter()
                .map(|l| size_of::<LocalBinding>() + l.name.len())
                .sum::<usize>()
            + ir.recur_targets.len() * size_of::<RecurTarget>()
            + ir.strings
                .iter()
                .map(|s| s.len() * 2 + size_of::<Vec<u16>>())
                .sum::<usize>()
            + ir.named_literals.len() * size_of::<NamedLiteral>()
            + self
                .declarations
                .globals
                .iter()
                .map(|g| size_of::<Global>() + g.namespace.len() + g.name.len())
                .sum::<usize>()
            + self
                .references
                .globals
                .iter()
                .map(|g| size_of::<ExternalGlobal>() + g.namespace.len() + g.name.len())
                .sum::<usize>()
            + self.references.namespaces.iter().map(text).sum::<usize>()
            + self
                .diagnostics
                .warnings
                .iter()
                .map(|w| size_of::<Warning>() + w.message.len())
                .sum::<usize>();
        for expr in &ir.exprs {
            kind_references(&expr.kind, &mut |_| bytes += size_of::<u32>());
        }
        bytes += self
            .declarations
            .macros
            .iter()
            .map(|m| code(&m.code))
            .sum::<usize>();
        bytes += self
            .units
            .units
            .iter()
            .map(|u| size_of::<Unit>() + code(&u.code))
            .sum::<usize>();
        bytes += self.units.registrations.len() * size_of::<Registration>();
        bytes
    }

    /// A readable listing of every section, for debugging and tests.
    pub fn describe(&self) -> String {
        let mut out = String::new();
        let h = &self.header;
        writeln!(out, "artifact {}", h.namespace).unwrap();
        writeln!(out, "[header]").unwrap();
        writeln!(
            out,
            "path {} length {} cljc {} entry {} root {}",
            h.path, h.source_length, h.cljc, h.entry, h.root.0
        )
        .unwrap();
        if let Some(name) = &h.source_name {
            writeln!(out, "source-name {name}").unwrap();
        }
        let d = &self.declarations;
        writeln!(out, "[declarations]").unwrap();
        for (index, global) in d.globals.iter().enumerate() {
            writeln!(
                out,
                "global {index} {}/{} {:?} metadata {:?} span {:?} source {:?} private {:?} binding {:?}",
                global.namespace,
                global.name,
                global.flags,
                global.metadata.map(|e| e.0),
                global.span,
                global.source,
                global.private,
                global.binding
            )
            .unwrap();
        }
        for (protocol, methods) in &d.protocols {
            writeln!(out, "protocol {protocol:?} {methods:?}").unwrap();
        }
        for global in &d.record_constructors {
            writeln!(out, "record-constructor {global:?}").unwrap();
        }
        for field in &d.field_bindings {
            writeln!(out, "field {field:?}").unwrap();
        }
        for target in &d.type_methods {
            writeln!(out, "type-method {}", target.0).unwrap();
        }
        for expr in &d.macro_functions {
            writeln!(out, "macro-function {}", expr.0).unwrap();
        }
        for m in &d.macros {
            writeln!(
                out,
                "macro {} generation {} definition {}@{:?} view {:?} code {} bytes units {:?}",
                m.name,
                m.generation,
                m.definition.source,
                m.definition.bytes,
                m.view,
                m.code.text.len(),
                m.code.units
            )
            .unwrap();
            writeln!(out, "  metadata {}", describe_datum(&m.metadata)).unwrap();
        }
        writeln!(out, "aliases {:?}", d.aliases).unwrap();
        writeln!(out, "requires {:?}", d.requires).unwrap();
        writeln!(out, "refers {:?}", d.refers).unwrap();
        writeln!(out, "excludes {:?}", d.excludes).unwrap();
        writeln!(out, "js-refers {:?}", d.js_refers).unwrap();
        writeln!(out, "reader-context {:?}", d.reader_context).unwrap();
        writeln!(out, "imports {:?}", d.imports).unwrap();
        writeln!(out, "renames {:?}", d.renames).unwrap();
        writeln!(out, "namespace-imports {:?}", d.namespace_imports).unwrap();
        if let Some(symbol) = &d.namespace_symbol {
            writeln!(out, "namespace-symbol {}", describe_datum(symbol)).unwrap();
        }
        if let Some(doc) = &d.namespace_doc {
            writeln!(out, "namespace-doc {}", describe_datum(doc)).unwrap();
        }
        for (index, target) in d.lazy_targets.iter().enumerate() {
            writeln!(
                out,
                "lazy-target {index} site {} namespace {} global {:?}",
                target.site.0, target.namespace, target.global
            )
            .unwrap();
        }
        writeln!(out, "[ir]").unwrap();
        let ir = &self.ir;
        for (index, expr) in ir.exprs.iter().enumerate() {
            writeln!(
                out,
                "expr {index} {:?} {:?} {:?} {:?}",
                expr.kind, expr.span, expr.context, expr.source
            )
            .unwrap();
        }
        for (index, local) in ir.locals.iter().enumerate() {
            writeln!(
                out,
                "local {index} {} {:?} {:?}",
                local.name, local.span, local.source
            )
            .unwrap();
        }
        for (index, target) in ir.recur_targets.iter().enumerate() {
            writeln!(out, "recur-target {index} {target:?}").unwrap();
        }
        for (index, string) in ir.strings.iter().enumerate() {
            writeln!(out, "string {index} {:?}", String::from_utf16_lossy(string)).unwrap();
        }
        for (index, literal) in ir.named_literals.iter().enumerate() {
            writeln!(
                out,
                "name {index} {:?} {}",
                literal.namespace.map(|s| s.0),
                literal.name.0
            )
            .unwrap();
        }
        for (index, path) in ir.js_globals.iter().enumerate() {
            writeln!(out, "host-path {index} {}", path.join(".")).unwrap();
        }
        writeln!(out, "[patches]").unwrap();
        for patch in &self.patches {
            writeln!(
                out,
                "patch {:?} {:?} metadata {:?}",
                patch.global,
                patch.flags,
                patch.metadata.map(|e| e.0)
            )
            .unwrap();
        }
        writeln!(out, "[deferred-units]").unwrap();
        for (index, unit) in self.units.units.iter().enumerate() {
            writeln!(
                out,
                "unit {index} {:?} generation {} analyses {} emitted {} frontier {} view {:?} code {} bytes units {:?}",
                unit.name,
                unit.generation,
                unit.analyses,
                unit.emitted,
                unit.extension_frontier,
                unit.view,
                unit.code.text.len(),
                unit.code.units
            )
            .unwrap();
        }
        for registration in &self.units.registrations {
            writeln!(out, "registration {registration:?}").unwrap();
        }
        writeln!(out, "[diagnostics]").unwrap();
        for warning in &self.diagnostics.warnings {
            writeln!(
                out,
                "warning {} {:?} {:?}: {}",
                warning.kind, warning.span, warning.source, warning.message
            )
            .unwrap();
        }
        for expansion in &self.diagnostics.expansions {
            writeln!(
                out,
                "expansion {}/{} generation {} call {:?} {:?} definition {}@{:?}",
                expansion.macro_namespace,
                expansion.name,
                expansion.generation,
                expansion.call,
                expansion.source,
                expansion.definition.source,
                expansion.definition.bytes
            )
            .unwrap();
        }
        writeln!(out, "[references]").unwrap();
        for (index, namespace) in self.references.namespaces.iter().enumerate() {
            writeln!(out, "namespace {index} {namespace}").unwrap();
        }
        for (index, global) in self.references.globals.iter().enumerate() {
            writeln!(
                out,
                "external {index} {}/{} declared {} private {:?}",
                global.namespace, global.name, global.declared, global.private
            )
            .unwrap();
        }
        writeln!(out, "[names]").unwrap();
        writeln!(
            out,
            "{}",
            if self.names.is_some() {
                "recorded"
            } else {
                "not recorded"
            }
        )
        .unwrap();
        writeln!(out, "[accounting]").unwrap();
        writeln!(
            out,
            "{}",
            if self.accounting.is_some() {
                "recorded"
            } else {
                "not recorded"
            }
        )
        .unwrap();
        out
    }
}

/// Reader data printed as source, with metadata in `^{...}` prefixes.
pub fn describe_datum(datum: &Datum) -> String {
    let mut out = String::new();
    write_datum(&mut out, datum);
    out
}

fn write_datum(out: &mut String, datum: &Datum) {
    if let Some(metadata) = &datum.metadata {
        out.push('^');
        write_datum(out, metadata);
        out.push(' ');
    }
    let name = |namespace: &Option<Vec<u16>>, name: &[u16]| match namespace {
        Some(namespace) => format!(
            "{}/{}",
            String::from_utf16_lossy(namespace),
            String::from_utf16_lossy(name)
        ),
        None => String::from_utf16_lossy(name),
    };
    match &datum.kind {
        DatumKind::Nil => out.push_str("nil"),
        DatumKind::Boolean(value) => write!(out, "{value}").unwrap(),
        DatumKind::Integer(value) => write!(out, "{value}").unwrap(),
        DatumKind::Double(value) => write!(out, "{value:?}").unwrap(),
        DatumKind::Ratio {
            numerator,
            denominator,
        } => write!(out, "{numerator}/{denominator}").unwrap(),
        DatumKind::Character(value) => write!(out, "\\u{value:04x}").unwrap(),
        DatumKind::String(value) => write!(out, "{:?}", String::from_utf16_lossy(value)).unwrap(),
        DatumKind::Symbol(namespace, symbol) => out.push_str(&name(namespace, symbol)),
        DatumKind::Keyword(namespace, keyword) => {
            out.push(':');
            out.push_str(&name(namespace, keyword));
        }
        DatumKind::Collection { kind, children } => {
            let (open, close) = match kind {
                CollectionKind::List => ("(", ")"),
                CollectionKind::AnonymousFn => ("#(", ")"),
                CollectionKind::Vector => ("[", "]"),
                CollectionKind::Map => ("{", "}"),
                CollectionKind::Set => ("#{", "}"),
            };
            out.push_str(open);
            for (index, child) in children.iter().enumerate() {
                if index > 0 {
                    out.push(' ');
                }
                write_datum(out, child);
            }
            out.push_str(close);
        }
        DatumKind::CompilerLiteral { kind, payload } => {
            write!(out, "#light-cljs/{kind:?} ").unwrap();
            write_datum(out, payload);
        }
    }
}

// An artifact can move to another thread, as a store writer would need, so
// it holds no `Rc`; and it owns its data, so it borrows nothing.
const _: () = {
    const fn owned<T: Send + Sync + 'static>() {}
    owned::<NamespaceArtifact>();
};
