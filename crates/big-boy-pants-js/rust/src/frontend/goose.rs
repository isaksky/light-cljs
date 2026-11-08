//! Bounded source export for the Goose experiment. This module never runs code.
//! The engine frontend supplies parsing, binding resolution and lowering.
use std::collections::BTreeSet;

use crate::frontend::{Backend, ScriptOptions, Span, SyntaxError, SyntaxErrorKind};
use crate::value::Primitive;
use crate::vm::Executable;
use crate::vm::executable::{EvalBinding, EvalBindingKind, EvalSite, EvalSource};
use crate::vm::instr::{Opcode, OperandKind};

/// Version 3 (light-cljs-823) adds the direct-eval site bit of instruction
/// descriptors ([`DIRECT_EVAL_SITE`]); version-2 readers refuse it explicitly.
/// Version 4 (light-cljs-853) adds each function's source text: function
/// flags bit 18 ([`SOURCE_TEXT`]) and the function row's words 13 and 14,
/// so the row is [`FUNCTION_WORDS`] words; version-3 readers refuse it.
/// Version 5 (light-cljs-854) adds function flags bit 19
/// ([`SIMPLE_PARAMETERS`]); version-4 readers refuse it.
/// Version 6 (light-cljs-858) adds the eval-site section after the UTF-16
/// pool: each direct-eval site's capture sources as validated rows and its
/// whole scope as an opaque record ([`encode_eval_site`]); version-5 readers
/// refuse it.
pub const VERSION: u32 = 6;
/// The u32 words of a function row (60 bytes): instruction start/count,
/// constant start/count, child start/count, params, locals, captures,
/// name-unit start/count, JS length, flags, source-text byte start/end.
pub const FUNCTION_WORDS: usize = 15;
/// Function flags bit 18: the function has source text (ECMAScript
/// [[SourceText]], `Executable::source_text_span`), the half-open byte range
/// of the image's source in words 13 and 14, which start and end on UTF-8
/// boundaries. Without it both words are 0: a script, eval code or a
/// synthesized function, whose `Function.prototype.toString` is the
/// native-code form.
pub const SOURCE_TEXT: u32 = 1 << 18;
/// Function flags bit 19: the parameter list is simple (ECMAScript
/// IsSimpleParameterList: no default, rest or destructuring pattern;
/// `Executable::has_simple_parameters`). Sloppy code with one makes a mapped
/// arguments object, whose `callee` is the function; any other makes an
/// unmapped one, whose `callee` is the %ThrowTypeError% accessor.
pub const SIMPLE_PARAMETERS: u32 = 1 << 19;
pub const MAX_SOURCE: usize = 8 * 1024 * 1024;
pub const MAX_RECORDS: usize = 1_048_576;
pub const MAX_FUNCTIONS: usize = 65_536;

/// Source-owned, pointer-free transport. Byte layout is documented in the
/// prototype README; integer fields are little endian, all table IDs are u32.
#[derive(Debug)]
pub struct Export {
    pub bytes: Vec<u8>,
    pub functions: usize,
    pub instructions: usize,
    pub operations: BTreeSet<&'static str>,
    pub string_constants: BTreeSet<String>,
    pub form_operations: BTreeSet<String>,
}

fn problem(source: &str, kind: SyntaxErrorKind, span: Span, message: &str) -> SyntaxError {
    SyntaxError::new(source, kind, span, message)
}

fn bounded(n: usize, limit: usize, source: &str) -> Result<u32, SyntaxError> {
    if n > limit {
        return Err(problem(
            source,
            SyntaxErrorKind::TooLarge,
            Span::new(0, 0),
            "Goose export resource budget exceeded",
        ));
    }
    u32::try_from(n).map_err(|_| {
        problem(
            source,
            SyntaxErrorKind::TooLarge,
            Span::new(0, 0),
            "Goose export u32 overflow",
        )
    })
}

// Every opcode the frontend emits is exported (light-cljs-783,
// docs/javascript-coverage.md decision 1). Export never asserts that the Goose
// VM executes an operation: an unimplemented opcode fails when it runs, with a
// fault that names it, and a dead one never fails.

/// GSP operand category of an opcode (descriptor bits 8..15).
fn operand_tag(kind: OperandKind) -> u32 {
    match kind {
        OperandKind::None => 0,
        OperandKind::Const | OperandKind::Name => 1,
        OperandKind::Local => 2,
        OperandKind::Target => 3,
        OperandKind::Function => 4,
        OperandKind::Capture => 5,
        _ => 6,
    }
}

/// The Goose names of the operand categories (`operand::` in the generated
/// schema), each with a kind of its category: `Immediate` is every other u16
/// operand (counts, selectors, host operations).
const OPERAND_CATEGORIES: [(&str, OperandKind); 7] = [
    ("None", OperandKind::None),
    ("Const", OperandKind::Const),
    ("Local", OperandKind::Local),
    ("Target", OperandKind::Target),
    ("Function", OperandKind::Function),
    ("Capture", OperandKind::Capture),
    ("Immediate", OperandKind::Count),
];

/// Stack-effect word bits 24..27: what the multiplied pop count is
/// (`count::` in the generated schema).
const COUNT_SOURCES: [&str; 4] = ["None", "Operand", "Captures", "FormArity"];

/// Stack-effect word bits 28..31 (`flow::` in the generated schema).
fn flow_code(flow: crate::vm::instr::Flow) -> u32 {
    use crate::vm::instr::Flow;
    match flow {
        Flow::Next => 0,
        Flow::Jump => 1,
        Flow::Branch => 2,
        Flow::Catch => 3,
        Flow::End => 4,
    }
}

/// Descriptor bit 24: the instruction is the call of a direct-eval site
/// (`eval(...)` written as a plain call; `ExecutableSpec::eval_sites`). Image
/// metadata, not an opcode: only the four call opcodes of
/// [`DIRECT_EVAL_CALLS`] carry it, and bits 25..31 stay zero. Each marked call
/// has exactly one row in the eval-site section (version 6, light-cljs-858).
pub const DIRECT_EVAL_SITE: u32 = 1 << 24;
/// The opcodes the lowerer emits at a direct-eval site's offset.
pub const DIRECT_EVAL_CALLS: [Opcode; 4] = [
    Opcode::Call,
    Opcode::CallMethod,
    Opcode::CallSpread,
    Opcode::CallMethodSpread,
];

/// The tag of a constant record (its first word). 5 is never used: it is the
/// Goose VM's heap-reference value tag. A Symbol constant names a well-known
/// symbol by its index here.
const UNDEFINED_CONSTANT: u32 = 0;
const NULL_CONSTANT: u32 = 1;
const BOOL_CONSTANT: u32 = 2;
const NUMBER_CONSTANT: u32 = 3;
const STRING_CONSTANT: u32 = 4;
const SYMBOL_CONSTANT: u32 = 6;
const BIGINT_CONSTANT: u32 = 7;
/// Every constant tag, named as its `Primitive` variant (`constant::` in
/// `inspection_schema`).
const CONSTANT_TAGS: [(&str, u32); 7] = [
    ("Undefined", UNDEFINED_CONSTANT),
    ("Null", NULL_CONSTANT),
    ("Bool", BOOL_CONSTANT),
    ("Number", NUMBER_CONSTANT),
    ("String", STRING_CONSTANT),
    ("Symbol", SYMBOL_CONSTANT),
    ("BigInt", BIGINT_CONSTANT),
];
pub const WELL_KNOWN_SYMBOLS: [&str; 15] = [
    "iterator",
    "asyncIterator",
    "toPrimitive",
    "hasInstance",
    "toStringTag",
    "dispose",
    "asyncDispose",
    "unscopables",
    "species",
    "isConcatSpreadable",
    "match",
    "matchAll",
    "search",
    "replace",
    "split",
];

/// Function flags: strict in bit 0, constructor kind in bits 8..15 (none,
/// ordinary, base class, derived class, default derived class), generator in
/// bit 16 and async in bit 17. The generator and async bits restate which of
/// `GeneratorStart`, `AsyncStart` or `AsyncGeneratorStart` the function's code
/// contains, so a loader learns the kind without scanning instructions.
fn constructor_code(kind: crate::ConstructorKind) -> u32 {
    match kind {
        crate::ConstructorKind::None => 0,
        crate::ConstructorKind::Ordinary => 1,
        crate::ConstructorKind::BaseClass => 2,
        crate::ConstructorKind::DerivedClass => 3,
        crate::ConstructorKind::DefaultDerivedClass => 4,
    }
}
/// Every constructor kind, in code order (`constructor::` in the generated schema).
const CONSTRUCTOR_KINDS: [crate::ConstructorKind; 5] = [
    crate::ConstructorKind::None,
    crate::ConstructorKind::Ordinary,
    crate::ConstructorKind::BaseClass,
    crate::ConstructorKind::DerivedClass,
    crate::ConstructorKind::DefaultDerivedClass,
];
fn function_flags(spec: &crate::vm::ExecutableSpec) -> u32 {
    let constructor = constructor_code(spec.constructor_kind);
    let (mut generator, mut asynchronous) = (false, false);
    let mut offset = 0;
    while offset < spec.code.len() {
        let opcode = Opcode::from_byte(spec.code[offset]).expect("validated opcode");
        match opcode {
            Opcode::GeneratorStart => generator = true,
            Opcode::AsyncStart => asynchronous = true,
            Opcode::AsyncGeneratorStart => (generator, asynchronous) = (true, true),
            _ => {}
        }
        offset += 1 + opcode.operand().size();
    }
    u32::from(spec.strict)
        | (constructor << 8)
        | (u32::from(generator) << 16)
        | (u32::from(asynchronous) << 17)
}

/// Generated stack-effect word of an opcode, read off `Instr::stack_effect`
/// and `Opcode::flow` (the engine validator's own rules): pops are
/// `base + multiplier * n` where `source` names n, pushes are fixed.
///
/// bits 0..7 base pops, 8..15 pushes, 16..23 multiplier, 24..27 source
/// (0 none, 1 the u16 operand, 2 the child function's captures, 3 the form
/// operation's arity), 28..31 flow (0 next, 1 jump, 2 branch, 3 catch, 4 end).
fn stack_effect_word(op: Opcode) -> u32 {
    use crate::vm::instr::{FormOp, Instr};
    let effect = |operand: u32, captures: u16| {
        Instr::decode(op, operand, 0, captures)
            .expect("operand decodes")
            .stack_effect()
    };
    let (source, base, multiplier, pushes) = match op.operand() {
        OperandKind::FormOp => {
            let pushes = effect(FormOp::ALL[0] as u32, 0).1;
            for form in FormOp::ALL {
                assert_eq!(
                    effect(form as u32, 0),
                    (form.arity(), pushes),
                    "{op:?} {form:?}"
                );
            }
            (3, 0, 1, pushes)
        }
        kind @ (OperandKind::Function | OperandKind::Count | OperandKind::ArgCount) => {
            let at = |n: u16| {
                if kind == OperandKind::Function {
                    effect(0, n)
                } else {
                    effect(u32::from(n), 0)
                }
            };
            let ((base, pushes), (one, _)) = (at(0), at(1));
            let multiplier = one - base;
            for n in [2, 7, 65_535] {
                assert_eq!(
                    at(n),
                    (base + multiplier * usize::from(n), pushes),
                    "{op:?}"
                );
            }
            (
                if kind == OperandKind::Function { 2 } else { 1 },
                base,
                multiplier,
                pushes,
            )
        }
        _ => {
            // Operand-independent: a valid selector for ThrowError is 0.
            let fixed = effect(0, 0);
            if let Some(other) = Instr::decode(op, 1, 1, 1) {
                assert_eq!(other.stack_effect(), fixed, "{op:?}");
            }
            (0, fixed.0, 0, fixed.1)
        }
    };
    let flow = flow_code(op.flow());
    assert!(base < 256 && pushes < 256 && multiplier < 256, "{op:?}");
    base as u32 | (pushes as u32) << 8 | (multiplier as u32) << 16 | source << 24 | flow << 28
}

/// The kind of a direct-eval site's capture source (`eval_source::` in the
/// generated schema), in [`EvalSource`]'s order: a source row's first word.
const EVAL_SOURCES: [&str; 6] = [
    "Local",
    "Capture",
    "CurrentFunction",
    "This",
    "GlobalObject",
    "NewTarget",
];
fn eval_source_row(source: EvalSource) -> (u32, u32) {
    match source {
        EvalSource::Local(i) => (0, u32::from(i)),
        EvalSource::Capture(i) => (1, u32::from(i)),
        EvalSource::CurrentFunction => (2, 0),
        EvalSource::This => (3, 0),
        EvalSource::GlobalObject => (4, 0),
        EvalSource::NewTarget => (5, 0),
    }
}
/// What a capture source supplies (`eval_role::` in the generated schema), in
/// the capture order of the Pants engine's `start_direct_eval`: every binding,
/// the receiver, new.target, every dynamic environment (nearest first), the
/// super home, the derived constructor's field initializer. A source row's
/// third word: bits 0..7 the role, bit 8 ([`EVAL_CELL`]) the value is a
/// binding cell.
const EVAL_ROLES: [&str; 6] = [
    "Binding",
    "Receiver",
    "NewTarget",
    "Environment",
    "SuperHome",
    "SuperFields",
];
pub const EVAL_CELL: u32 = 1 << 8;

/// The opaque record of a direct-eval site: the whole [`EvalSite`] but its
/// offset, which the Goose VM hands back unchanged to the run-time compiler
/// ([`RuntimeMode::DirectEval`], [`decode_eval_site`]). Little endian:
///
/// * u8 flags: bit 0 strict, 1 receiver_strict, 2 class_initializer, 3 a
///   super home, 4 super fields, 5 the super fields are a cell, 6 new.target,
///   7 a variable environment;
/// * the receiver, then each present source among new.target, super home,
///   super fields and variable environment, in that order, each a source:
///   u8 kind (an [`EVAL_SOURCES`] index) and u16 index;
/// * u32 binding count, each binding a string, a source, u8 kind (mutable,
///   const, function name, import), u8 cell, u16 environments;
/// * u32 environment count, each a source;
/// * u32 counts of variable bindings, then of lexical conflicts, each entry
///   a string;
///
/// a string being its u32 byte length and its UTF-8.
pub fn encode_eval_site(site: &EvalSite) -> Vec<u8> {
    let mut out = Vec::new();
    let source = |out: &mut Vec<u8>, s: EvalSource| {
        let (kind, index) = eval_source_row(s);
        out.push(kind as u8);
        out.extend_from_slice(&(index as u16).to_le_bytes());
    };
    let string = |out: &mut Vec<u8>, s: &str| {
        out.extend_from_slice(&(s.len() as u32).to_le_bytes());
        out.extend_from_slice(s.as_bytes());
    };
    let flags = u8::from(site.strict)
        | u8::from(site.receiver_strict) << 1
        | u8::from(site.class_initializer) << 2
        | u8::from(site.super_home.is_some()) << 3
        | u8::from(site.super_fields.is_some()) << 4
        | u8::from(site.super_fields.is_some_and(|(_, cell)| cell)) << 5
        | u8::from(site.new_target.is_some()) << 6
        | u8::from(site.variable_environment.is_some()) << 7;
    out.push(flags);
    source(&mut out, site.receiver);
    for s in [
        site.new_target,
        site.super_home,
        site.super_fields.map(|(s, _)| s),
        site.variable_environment,
    ]
    .into_iter()
    .flatten()
    {
        source(&mut out, s);
    }
    out.extend_from_slice(&(site.bindings.len() as u32).to_le_bytes());
    for b in &site.bindings {
        string(&mut out, &b.name);
        source(&mut out, b.source);
        out.push(match b.kind {
            EvalBindingKind::Mutable => 0,
            EvalBindingKind::Const => 1,
            EvalBindingKind::FunctionName => 2,
            EvalBindingKind::Import => 3,
        });
        out.push(u8::from(b.cell));
        out.extend_from_slice(&b.environments.to_le_bytes());
    }
    out.extend_from_slice(&(site.environments.len() as u32).to_le_bytes());
    for s in &site.environments {
        source(&mut out, *s);
    }
    for names in [&site.variable_bindings, &site.lexical_conflicts] {
        out.extend_from_slice(&(names.len() as u32).to_le_bytes());
        for name in names {
            string(&mut out, name);
        }
    }
    out
}

/// Decode [`encode_eval_site`]'s record: `None` when it is malformed or
/// breaks a rule the engine's loader checks of a site (`Executable::load`):
/// unique binding names, every mutable binding a cell, binding environments
/// within the site's, the variable environment one of them and its static
/// bindings mutable cells, a consistent super context, and every capture
/// within u16. The offset is 0: a run-time compilation needs none.
pub fn decode_eval_site(bytes: &[u8]) -> Option<EvalSite> {
    struct Reader<'a>(&'a [u8]);
    impl Reader<'_> {
        fn take(&mut self, n: usize) -> Option<&[u8]> {
            if n > self.0.len() {
                return None;
            }
            let (head, rest) = self.0.split_at(n);
            self.0 = rest;
            Some(head)
        }
        fn u8(&mut self) -> Option<u8> {
            self.take(1).map(|b| b[0])
        }
        fn u16(&mut self) -> Option<u16> {
            self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
        }
        fn u32(&mut self) -> Option<usize> {
            self.take(4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
        }
        fn source(&mut self) -> Option<EvalSource> {
            let kind = self.u8()?;
            let index = self.u16()?;
            let source = match kind {
                0 => EvalSource::Local(index),
                1 => EvalSource::Capture(index),
                2 => EvalSource::CurrentFunction,
                3 => EvalSource::This,
                4 => EvalSource::GlobalObject,
                5 => EvalSource::NewTarget,
                _ => return None,
            };
            (kind <= 1 || index == 0).then_some(source)
        }
        fn string(&mut self) -> Option<String> {
            let n = self.u32()?;
            String::from_utf8(self.take(n)?.to_vec()).ok()
        }
        fn count(&mut self) -> Option<usize> {
            // Every counted entry takes at least three bytes.
            let n = self.u32()?;
            (n <= self.0.len() / 3).then_some(n)
        }
    }
    let mut r = Reader(bytes);
    let flags = r.u8()?;
    let receiver = r.source()?;
    let new_target = if flags & 64 != 0 {
        Some(r.source()?)
    } else {
        None
    };
    let super_home = if flags & 8 != 0 {
        Some(r.source()?)
    } else {
        None
    };
    let super_fields = match (flags & 16 != 0, flags & 32 != 0) {
        (true, cell) => Some((r.source()?, cell)),
        (false, false) => None,
        (false, true) => return None,
    };
    let variable_environment = if flags & 128 != 0 {
        Some(r.source()?)
    } else {
        None
    };
    let mut bindings = Vec::new();
    for _ in 0..r.count()? {
        let name = r.string()?;
        let source = r.source()?;
        let kind = match r.u8()? {
            0 => EvalBindingKind::Mutable,
            1 => EvalBindingKind::Const,
            2 => EvalBindingKind::FunctionName,
            3 => EvalBindingKind::Import,
            _ => return None,
        };
        let cell = match r.u8()? {
            0 => false,
            1 => true,
            _ => return None,
        };
        let environments = r.u16()?;
        bindings.push(EvalBinding {
            name,
            source,
            kind,
            cell,
            environments,
        });
    }
    let mut environments = Vec::new();
    for _ in 0..r.count()? {
        environments.push(r.source()?);
    }
    let mut lists = [Vec::new(), Vec::new()];
    for list in &mut lists {
        for _ in 0..r.count()? {
            list.push(r.string()?);
        }
    }
    let [variable_bindings, lexical_conflicts] = lists;
    if !r.0.is_empty() {
        return None;
    }
    let site = EvalSite {
        offset: 0,
        strict: flags & 1 != 0,
        bindings,
        receiver,
        receiver_strict: flags & 2 != 0,
        class_initializer: flags & 4 != 0,
        super_home,
        super_fields,
        new_target,
        environments,
        variable_environment,
        variable_bindings,
        lexical_conflicts,
    };
    let mut names = BTreeSet::new();
    let invalid = eval_captures(&site).len() > usize::from(u16::MAX)
        || site.class_initializer && site.super_home.is_none()
        || site.super_fields.is_some()
            && (site.super_home.is_none() || site.new_target.is_none() || site.class_initializer)
        || site
            .variable_environment
            .is_some_and(|s| !site.environments.contains(&s))
        || site.variable_bindings.iter().any(|name| {
            !site
                .bindings
                .iter()
                .any(|b| b.name == *name && b.kind == EvalBindingKind::Mutable && b.cell)
        })
        || site.bindings.iter().any(|b| {
            !names.insert(b.name.as_str())
                || (b.kind == EvalBindingKind::Mutable && !b.cell)
                || usize::from(b.environments) > site.environments.len()
        });
    (!invalid).then_some(site)
}

/// A site's capture sources in the eval closure's capture order (the Pants
/// engine's `start_direct_eval`, `lower_direct_eval`'s capture places): each
/// with its role (an [`EVAL_ROLES`] index) and whether it is a cell.
fn eval_captures(site: &EvalSite) -> Vec<(EvalSource, u32, bool)> {
    let mut out: Vec<_> = site
        .bindings
        .iter()
        .map(|b| (b.source, 0, b.cell))
        .collect();
    out.push((site.receiver, 1, false));
    out.extend(site.new_target.map(|s| (s, 2, false)));
    out.extend(site.environments.iter().map(|s| (*s, 3, false)));
    out.extend(site.super_home.map(|s| (s, 4, false)));
    out.extend(site.super_fields.map(|(s, cell)| (s, 5, cell)));
    out
}

#[derive(Default)]
struct Tables {
    functions: Vec<[u32; FUNCTION_WORDS]>,
    constants: Vec<[u32; 3]>,
    children: Vec<u32>,
    instructions: Vec<[u32; 5]>,
    units: Vec<u16>,
    /// The eval-site section (version 6): per site its call's instruction
    /// row, source start/count and record byte start/length; per source its
    /// kind, index and role word; the records' bytes.
    sites: Vec<[u32; 5]>,
    site_sources: Vec<[u32; 3]>,
    site_records: Vec<u8>,
    operations: BTreeSet<&'static str>,
    string_constants: BTreeSet<String>,
    form_operations: BTreeSet<String>,
}

impl Tables {
    fn string(&mut self, units: &[u16], source: &str) -> Result<(u32, u32), SyntaxError> {
        let start = bounded(self.units.len(), MAX_RECORDS, source)?;
        let len = bounded(units.len(), MAX_RECORDS, source)?;
        bounded(
            self.units.len().saturating_add(units.len()),
            MAX_RECORDS,
            source,
        )?;
        self.units.extend_from_slice(units);
        Ok((start, len))
    }
    fn function(
        &mut self,
        executable: &Executable,
        source: &str,
        depth: usize,
    ) -> Result<u32, SyntaxError> {
        if depth > 256 {
            return Err(problem(
                source,
                SyntaxErrorKind::TooDeep,
                Span::new(0, 0),
                "Goose function depth budget exceeded",
            ));
        }
        let id = bounded(self.functions.len(), MAX_FUNCTIONS - 1, source)?;
        self.functions.push([0; FUNCTION_WORDS]);
        let spec = executable.to_spec();
        // Direct-eval sites export as marked calls (light-cljs-823), each with
        // its row in the eval-site section (light-cljs-858), in offset order.
        let eval_offsets: BTreeSet<u32> = spec.eval_sites.iter().map(|s| s.offset).collect();
        let mut marked = 0;
        let mut site_rows = std::collections::BTreeMap::new();
        // Children are constructed first, then their IDs form one contiguous
        // range. IDs survive all subsequent table growth.
        let mut children = Vec::new();
        for child in &spec.functions {
            children.push(self.function(child, source, depth + 1)?);
        }
        let child_start = bounded(self.children.len(), MAX_RECORDS, source)?;
        bounded(
            self.children.len().saturating_add(children.len()),
            MAX_RECORDS,
            source,
        )?;
        self.children.extend(children);
        let constant_start = bounded(self.constants.len(), MAX_RECORDS, source)?;
        for value in &spec.constants {
            let row = match value {
                Primitive::Undefined => [UNDEFINED_CONSTANT, 0, 0],
                Primitive::Null => [NULL_CONSTANT, 0, 0],
                Primitive::Bool(b) => [BOOL_CONSTANT, u32::from(*b), 0],
                Primitive::Number(n) => {
                    let b = n.to_bits();
                    [NUMBER_CONSTANT, b as u32, (b >> 32) as u32]
                }
                Primitive::String(s) => {
                    if let Ok(text) = String::from_utf16(s.code_units()) {
                        self.string_constants.insert(text);
                    }
                    let (start, len) = self.string(s.code_units(), source)?;
                    [STRING_CONSTANT, start, len]
                }
                Primitive::Symbol(symbol) => {
                    // The frontend emits only well-known symbols
                    // (`Symbol.unscopables` for `with`); a symbol has no
                    // source-stable identity otherwise.
                    let Some(index) = WELL_KNOWN_SYMBOLS
                        .iter()
                        .position(|name| *symbol == crate::value::symbol::well_known(name))
                    else {
                        return Err(problem(
                            source,
                            SyntaxErrorKind::Unsupported,
                            Span::new(0, source.len()),
                            "Goose export supports only well-known Symbol constants",
                        ));
                    };
                    [SYMBOL_CONSTANT, index as u32, 0]
                }
                Primitive::BigInt(n) => {
                    // UTF-16 pool range: the sign (0 or 1), then the magnitude
                    // in little-endian 16-bit digits without a high zero digit.
                    let (negative, limbs) = n.to_sign_magnitude();
                    let mut digits = vec![u16::from(negative)];
                    for limb in limbs {
                        digits.extend([limb as u16, (limb >> 16) as u16]);
                    }
                    while digits.len() > 1 && digits.last() == Some(&0) {
                        digits.pop();
                    }
                    let (start, len) = self.string(&digits, source)?;
                    [BIGINT_CONSTANT, start, len]
                }
            };
            bounded(self.constants.len() + 1, MAX_RECORDS, source)?;
            self.constants.push(row);
        }
        let instruction_start = bounded(self.instructions.len(), MAX_RECORDS, source)?;
        let mut offset = 0;
        let mut index = 0;
        while offset < spec.code.len() {
            // Input has already passed the engine's validator.
            let opcode = Opcode::from_byte(spec.code[offset]).expect("validated opcode");
            let location = executable
                .source_location(index)
                .expect("source-compiled instruction");
            let span = location.span;
            if source.get(span.start..span.end).is_none() {
                return Err(problem(
                    source,
                    SyntaxErrorKind::Internal,
                    Span::new(0, 0),
                    "invalid source provenance in Goose export",
                ));
            }
            let kind = opcode.operand();
            let size = kind.size();
            let mut arg = 0u32;
            for i in 0..size {
                arg |= u32::from(spec.code[offset + 1 + i]) << (8 * i);
            }
            // The kind tag lets the inspection decoder validate references
            // without binding its reader to the engine's private enum layout.
            let tag = operand_tag(kind);
            let site = if eval_offsets.contains(&(offset as u32)) {
                if !DIRECT_EVAL_CALLS.contains(&opcode) {
                    return Err(problem(
                        source,
                        SyntaxErrorKind::Internal,
                        Span::new(0, 0),
                        "direct eval site is not a call in Goose export",
                    ));
                }
                marked += 1;
                site_rows.insert(offset as u32, self.instructions.len() as u32);
                DIRECT_EVAL_SITE
            } else {
                0
            };
            bounded(self.instructions.len() + 1, MAX_RECORDS, source)?;
            self.instructions.push([
                u32::from(opcode as u8) | (tag << 8) | ((size as u32) << 16) | site,
                arg,
                bounded(span.start, MAX_SOURCE, source)?,
                bounded(span.end, MAX_SOURCE, source)?,
                bounded(offset, u32::MAX as usize, source)?,
            ]);
            self.operations.insert(opcode.name());
            if opcode == Opcode::FormOp {
                self.form_operations.insert(format!(
                    "{:?}",
                    crate::vm::instr::FormOp::from_u16(arg as u16)
                        .expect("validated form selector")
                ));
            }
            offset += 1 + size;
            index += 1;
        }
        if marked != eval_offsets.len() || marked != spec.eval_sites.len() {
            return Err(problem(
                source,
                SyntaxErrorKind::Internal,
                Span::new(0, 0),
                "direct eval site is not an instruction in Goose export",
            ));
        }
        let mut sites: Vec<&EvalSite> = spec.eval_sites.iter().collect();
        sites.sort_by_key(|s| s.offset);
        for site in sites {
            let captures = eval_captures(site);
            let record = encode_eval_site(site);
            let source_start = bounded(self.site_sources.len(), MAX_RECORDS, source)?;
            bounded(
                self.site_sources.len().saturating_add(captures.len()),
                MAX_RECORDS,
                source,
            )?;
            let record_start = bounded(self.site_records.len(), MAX_SOURCE, source)?;
            bounded(
                self.site_records.len().saturating_add(record.len()),
                MAX_SOURCE,
                source,
            )?;
            bounded(self.sites.len() + 1, MAX_RECORDS, source)?;
            self.sites.push([
                site_rows[&site.offset],
                source_start,
                captures.len() as u32,
                record_start,
                record.len() as u32,
            ]);
            for (s, role, cell) in captures {
                let (kind, index) = eval_source_row(s);
                self.site_sources
                    .push([kind, index, role | if cell { EVAL_CELL } else { 0 }]);
            }
            self.site_records.extend_from_slice(&record);
        }
        let name = spec
            .function_name
            .as_ref()
            .map_or(&[][..], |s| s.code_units());
        let (name_start, name_len) = self.string(name, source)?;
        let (text_flag, text_start, text_end) = match executable.source_text_span() {
            Some(span) => {
                if span.start >= span.end
                    || !source.is_char_boundary(span.start)
                    || !source.is_char_boundary(span.end)
                    || span.end > source.len()
                {
                    return Err(problem(
                        source,
                        SyntaxErrorKind::Internal,
                        Span::new(0, 0),
                        "invalid function source text in Goose export",
                    ));
                }
                (
                    SOURCE_TEXT,
                    bounded(span.start, MAX_SOURCE, source)?,
                    bounded(span.end, MAX_SOURCE, source)?,
                )
            }
            None => (0, 0, 0),
        };
        self.functions[id as usize] = [
            instruction_start,
            index as u32,
            constant_start,
            spec.constants.len() as u32,
            child_start,
            spec.functions.len() as u32,
            u32::from(spec.params),
            u32::from(spec.locals),
            u32::from(spec.captures),
            name_start,
            name_len,
            u32::from(spec.function_length.unwrap_or(spec.params)),
            function_flags(&spec)
                | text_flag
                | if executable.has_simple_parameters() {
                    SIMPLE_PARAMETERS
                } else {
                    0
                },
            text_start,
            text_end,
        ];
        Ok(id)
    }
}

/// Parse and lower every function, preserving exact source bytes once. This
/// accepts no values or host callbacks and cannot compute an expansion.
pub fn export_source(name: &str, source: &str) -> Result<Export, SyntaxError> {
    export_source_with(name, source, ScriptOptions::STRICT)
}

/// [`export_source`] with the script's top level in the mode `options` gives
/// (the conformance harness exports QuickJS's sloppy-mode test files).
pub fn export_source_with(
    name: &str,
    source: &str,
    options: ScriptOptions,
) -> Result<Export, SyntaxError> {
    bounded(source.len(), MAX_SOURCE, source)?;
    let executable = crate::frontend::compile_script_for(name, source, options, Backend::Goose)?;
    encode(&executable, source)
}

/// What run-time code [`export_runtime`] compiles (light-cljs-856): the
/// Goose VM's indirect eval, its Function constructor (light-cljs-857) and
/// direct eval (light-cljs-858).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeMode {
    /// Global script code of an indirect eval: sloppy unless `strict` or its
    /// own directive prologue says "use strict" (ECMAScript PerformEval).
    IndirectEval { strict: bool },
    /// The source the Function constructor assembled, `function
    /// anonymous(` + parameters + `\n) {\n` + body + `\n}`, the parameter
    /// list's closing `)` ending `parameter_units` UTF-16 units in, as the
    /// Pants engine's `DynamicFunction` places it (CreateDynamicFunction).
    DynamicFunction { parameter_units: usize },
    /// Eval code of a direct eval, compiled against its call site's scope
    /// (`compile_direct_eval`): the site the image's eval-site record
    /// encodes ([`decode_eval_site`]). Its captures are the site's sources
    /// in [`eval_captures`] order.
    DirectEval { site: EvalSite },
}

/// Compile JavaScript the Goose VM received while a job runs, given as its
/// exact UTF-16 code units (lone surrogates carried as [`push_utf16`]
/// carries them), into a GSP image as [`export_source`] encodes one; its
/// function 0 is the eval script or the dynamic function's wrapper.
///
/// The compilation runs on a helper thread with a 2 MiB stack, as the Pants
/// engine's eval does (`vm::native::eval`): the frontend's weighted nesting
/// bound is proven for that stack, and the caller's thread (a Goose worker)
/// may have less. Nothing runs here; the caller validates the image before
/// it executes. Errors: any compile error the frontend reports (the eval
/// becomes a SyntaxError with its message), the image limits, and
/// [`SyntaxErrorKind::Internal`] when the helper thread cannot start or the
/// frontend panics.
///
/// [`push_utf16`]: crate::frontend::push_utf16
pub fn export_runtime(units: &[u16], mode: RuntimeMode) -> Result<Export, SyntaxError> {
    let mut source = String::with_capacity(units.len());
    let mut surrogates = Vec::new();
    let mut next = 0;
    crate::frontend::push_utf16(units, &mut next, units.len(), &mut source, &mut surrogates);
    bounded(source.len(), MAX_SOURCE, &source)?;
    let internal = |source: &str, message: &str| {
        problem(source, SyntaxErrorKind::Internal, Span::new(0, 0), message)
    };
    let helper = std::thread::Builder::new()
        .name("goose-runtime-compiler".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let executable = match mode {
                RuntimeMode::IndirectEval { strict } => crate::frontend::compile_eval(
                    "<eval>",
                    &source,
                    &surrogates,
                    strict,
                    Backend::Goose,
                ),
                RuntimeMode::DynamicFunction { parameter_units } => {
                    crate::frontend::compile_dynamic_function(
                        &source,
                        &surrogates,
                        parameter_units,
                        Backend::Goose,
                    )
                }
                RuntimeMode::DirectEval { site } => crate::frontend::compile_direct_eval(
                    "<eval>",
                    &source,
                    &surrogates,
                    &site,
                    Backend::Goose,
                ),
            };
            executable.and_then(|executable| encode(&executable, &source))
        });
    match helper {
        Ok(handle) => handle
            .join()
            .unwrap_or_else(|_| Err(internal("", "the run-time compiler panicked"))),
        Err(_) => Err(internal("", "the run-time compiler thread did not start")),
    }
}

/// Export a function body for source smoke tests, never ordinary-call support.
pub fn export_function(source: &str) -> Result<Export, SyntaxError> {
    bounded(source.len(), MAX_SOURCE, source)?;
    let executable = crate::frontend::compile_function(source)?;
    encode(&executable, source)
}

fn encode(executable: &Executable, source: &str) -> Result<Export, SyntaxError> {
    let mut tables = Tables::default();
    assert_eq!(tables.function(executable, source, 0)?, 0);
    let mut bytes = b"GSP\0".to_vec();
    let word = |out: &mut Vec<u8>, n: u32| out.extend_from_slice(&n.to_le_bytes());
    for n in [
        VERSION,
        source.len() as u32,
        tables.functions.len() as u32,
        tables.constants.len() as u32,
        tables.children.len() as u32,
        tables.instructions.len() as u32,
        tables.units.len() as u32,
    ] {
        word(&mut bytes, n);
    }
    bytes.extend_from_slice(source.as_bytes());
    for row in &tables.functions {
        for &n in row {
            word(&mut bytes, n);
        }
    }
    for row in &tables.constants {
        for &n in row {
            word(&mut bytes, n);
        }
    }
    for &n in &tables.children {
        word(&mut bytes, n);
    }
    for row in &tables.instructions {
        for &n in row {
            word(&mut bytes, n);
        }
    }
    for n in &tables.units {
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    // The eval-site section (version 6): three u32 counts, then the site
    // rows, the source rows and the record bytes.
    for n in [
        tables.sites.len() as u32,
        tables.site_sources.len() as u32,
        tables.site_records.len() as u32,
    ] {
        word(&mut bytes, n);
    }
    for row in &tables.sites {
        for &n in row {
            word(&mut bytes, n);
        }
    }
    for row in &tables.site_sources {
        for &n in row {
            word(&mut bytes, n);
        }
    }
    bytes.extend_from_slice(&tables.site_records);
    Ok(Export {
        bytes,
        functions: tables.functions.len(),
        instructions: tables.instructions.len(),
        operations: tables.operations,
        string_constants: tables.string_constants,
        form_operations: tables.form_operations,
    })
}

/// Generated from the same checked opcode surface as the exporter. It is
/// versioned source, never interpreted code supplied by an input image.
pub fn inspection_schema() -> String {
    use crate::vm::instr::{FormOp, ThrowKind};
    let opcodes = || (0..=255).filter_map(Opcode::from_byte);
    let name = |op: Opcode| format!("opcode::{op:?}");
    let mut text = String::from(
        "// Generated by regenerate.bb from the Rust exporter; do not edit.\n\
         // Goose sources name these numbers only through the constants below\n\
         // (crates/big-boy-pants-js/AGENTS.md).\n\
         \n\
         // The byte of every opcode of the shared registry (rust/src/vm/instr.rs),\n\
         // named as its Rust `Opcode` variant.\n",
    );
    for op in opcodes() {
        text.push_str(&format!("let {} = {};\n", name(op), op as u8));
    }
    text.push_str("\n// The selector of every form operation (`FormOp`, the FormOp operand).\n");
    for op in FormOp::ALL {
        text.push_str(&format!("let form_op::{op:?} = {};\n", op as u16));
    }
    text.push_str("\n// The selector of every static throw (`ThrowKind`, ThrowError's operand).\n");
    for kind in (0..=u16::MAX).map_while(ThrowKind::from_u16) {
        text.push_str(&format!("let throw_kind::{kind:?} = {};\n", kind as u16));
    }
    text.push_str(
        "\n// The index of every well-known symbol (a Symbol constant's operand),\n\
         // capitalized: well_known::ToPrimitive is Symbol.toPrimitive.\n",
    );
    for (index, symbol) in WELL_KNOWN_SYMBOLS.iter().enumerate() {
        let (first, rest) = symbol.split_at(1);
        text.push_str(&format!(
            "let well_known::{}{rest} = {index};\n",
            first.to_uppercase()
        ));
    }
    text.push_str(
        "\n// The tag of every constant record (its first word), named as its\n\
         // `Primitive` variant. 5 is never a constant tag.\n",
    );
    for (variant, tag) in CONSTANT_TAGS {
        text.push_str(&format!("let constant::{variant} = {tag};\n"));
    }
    text.push_str("\n// A function's constructor kind (function flags bits 8..15).\n");
    for (kind, code) in CONSTRUCTOR_KINDS.iter().zip(0..) {
        assert_eq!(constructor_code(*kind), code);
        text.push_str(&format!("let constructor::{kind:?} = {code};\n"));
    }
    text.push_str("\n// An opcode's operand category (descriptor bits 8..15).\n");
    for (category, kind) in OPERAND_CATEGORIES {
        text.push_str(&format!(
            "let operand::{category} = {};\n",
            operand_tag(kind)
        ));
    }
    text.push_str("\n// What multiplies a stack effect's pops (stack-effect bits 24..27).\n");
    for (source, code) in COUNT_SOURCES.iter().zip(0..) {
        text.push_str(&format!("let count::{source} = {code};\n"));
    }
    text.push_str("\n// An opcode's control flow (stack-effect bits 28..31).\n");
    {
        use crate::vm::instr::Flow;
        for flow in [Flow::Next, Flow::Jump, Flow::Branch, Flow::Catch, Flow::End] {
            text.push_str(&format!("let flow::{flow:?} = {};\n", flow_code(flow)));
        }
    }
    text.push_str(&format!(
        "\n// A function row's bytes and its source-text flag (function flags\n\
         // bit 18, GSP version 4): words 13 and 14 hold the text's byte range.\n\
         let function_row::Bytes = {};\n\
         let function_row::SourceText = {SOURCE_TEXT};\n\
         // Function flags bit 19 (GSP version 5): a simple parameter list.\n\
         let function_row::SimpleParameters = {SIMPLE_PARAMETERS};\n",
        FUNCTION_WORDS * 4
    ));
    text.push_str(
        "\n// A direct-eval site's capture source kind (an eval-site source row's\n\
         // first word, GSP version 6), named as its Rust `EvalSource` variant.\n",
    );
    for (code, kind) in EVAL_SOURCES.iter().enumerate() {
        text.push_str(&format!("let eval_source::{kind} = {code};\n"));
    }
    text.push_str(
        "\n// What a capture source supplies (bits 0..7 of a source row's third\n\
         // word), in capture order; bit 8 (eval_role::Cell) marks a binding cell.\n",
    );
    for (code, role) in EVAL_ROLES.iter().enumerate() {
        text.push_str(&format!("let eval_role::{role} = {code};\n"));
    }
    text.push_str(&format!("let eval_role::Cell = {EVAL_CELL};\n"));
    text.push_str(
        "\n// Every opcode's tables; -1, false or \"\" for an unassigned byte.\n\
         fn export_descriptor(op: i64) -> i64 {\n    return match op {\n",
    );
    for op in opcodes() {
        let kind = op.operand();
        let descriptor =
            u32::from(op as u8) | (operand_tag(kind) << 8) | ((kind.size() as u32) << 16);
        text.push_str(&format!("        {} => {descriptor},\n", name(op)));
    }
    text.push_str("        _ => -1,\n    };\n}\n\nfn export_form_arity(op: i64) -> i64 {\n    return match op {\n");
    for op in FormOp::ALL {
        text.push_str(&format!("        form_op::{op:?} => {},\n", op.arity()));
    }
    text.push_str("        _ => -1,\n    };\n}\n\nfn export_name_operand(op: i64) -> bool {\n    return match op {\n");
    for op in opcodes().filter(|op| op.operand() == OperandKind::Name) {
        text.push_str(&format!("        {} => true,\n", name(op)));
    }
    text.push_str(
        "        _ => false,\n    };\n}\n\n\
         // The call opcodes that may carry the direct-eval site bit (descriptor\n\
         // bit 24, GSP version 3).\n\
         fn export_direct_eval_call(op: i64) -> bool {\n    return match op {\n",
    );
    for op in DIRECT_EVAL_CALLS {
        text.push_str(&format!("        {} => true,\n", name(op)));
    }
    text.push_str(
        "        _ => false,\n    };\n}\n\n\
         // Stack effect and control flow, from the engine validator's rules:\n\
         // bits 0..7 base pops, 8..15 pushes, 16..23 multiplier, 24..27 source of\n\
         // the multiplied count (0 none, 1 operand, 2 child captures, 3 form arity),\n\
         // 28..31 flow (0 next, 1 jump, 2 branch, 3 catch, 4 end).\n\
         fn export_stack_effect(op: i64) -> i64 {\n    return match op {\n",
    );
    for op in opcodes() {
        text.push_str(&format!(
            "        {} => {},\n",
            name(op),
            stack_effect_word(op)
        ));
    }
    text.push_str("        _ => -1,\n    };\n}\n\nfn export_opcode_name(op: i64) -> const u8[:] {\n    return match op {\n");
    for op in opcodes() {
        text.push_str(&format!("        {} => \"{}\",\n", name(op), op.name()));
    }
    text.push_str("        _ => \"\",\n    };\n}\n\nfn export_well_known_symbol(index: i64) -> const u8[:] {\n    return match index {\n");
    for (index, name) in WELL_KNOWN_SYMBOLS.iter().enumerate() {
        text.push_str(&format!("        {index} => \"{name}\",\n"));
    }
    text.push_str(
        "        _ => \"\",\n    };\n}\n\n\
         // Valid selectors of a ThrowKind or built-in global operand are 0..count;\n\
         // -1 for opcodes without such a selector (FormOp has export_form_arity).\n\
         fn export_selector_count(op: i64) -> i64 {\n    return match op {\n",
    );
    for op in opcodes() {
        let valid = |n: u16| match op.operand() {
            OperandKind::ThrowKind => Some(ThrowKind::from_u16(n).is_some()),
            OperandKind::BuiltinGlobal => Some(crate::realm::Global::from_u16(n).is_some()),
            _ => None,
        };
        if valid(0).is_some() {
            let count = (0..=u16::MAX)
                .take_while(|&n| valid(n) == Some(true))
                .count();
            assert!(
                (count..=usize::from(u16::MAX)).all(|n| valid(n as u16) == Some(false)),
                "{op:?}"
            );
            text.push_str(&format!("        {} => {count},\n", name(op)));
        }
    }
    text.push_str("        _ => -1,\n    };\n}\n");
    text
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whole_function_tree_and_exact_source_survive() {
        let source = "const μ = '\\ud800😀'; function f(x) { return y => x + y; } f(3);";
        let a = export_source("unicode.js", source).unwrap();
        let b = export_source("unicode.js", source).unwrap();
        assert_eq!(a.bytes, b.bytes);
        assert_eq!(a.functions, 3);
        assert_eq!(&a.bytes[32..32 + source.len()], source.as_bytes());
        assert!(a.operations.contains("get_capture"));
        assert!(a.bytes.windows(2).any(|w| w == [0, 216]));
    }
    /// Rows of one GSP table: (byte offset, record count, words per record).
    pub(super) fn rows(export: &Export, table: usize) -> Vec<Vec<u32>> {
        let word = |at: usize| u32::from_le_bytes(export.bytes[at..at + 4].try_into().unwrap());
        let widths = [FUNCTION_WORDS, 3, 1, 5];
        let mut at = 32 + word(8) as usize;
        for (t, width) in widths.iter().enumerate().take(table) {
            at += word(12 + 4 * t) as usize * width * 4;
        }
        (0..word(12 + 4 * table) as usize)
            .map(|i| {
                (0..widths[table])
                    .map(|w| word(at + (i * widths[table] + w) * 4))
                    .collect()
            })
            .collect()
    }
    fn units(export: &Export, start: u32, len: u32) -> Vec<u16> {
        let word = |at: usize| u32::from_le_bytes(export.bytes[at..at + 4].try_into().unwrap());
        let mut at = 32 + word(8) as usize;
        for (t, width) in [FUNCTION_WORDS, 3, 1, 5].into_iter().enumerate() {
            at += word(12 + 4 * t) as usize * width * 4;
        }
        (start..start + len)
            .map(|i| {
                let at = at + 2 * i as usize;
                u16::from_le_bytes([export.bytes[at], export.bytes[at + 1]])
            })
            .collect()
    }
    #[test]
    fn unimplemented_operations_on_dead_paths_export() {
        // The whole program exports; the Goose VM fails only if it runs
        // `construct`, naming it there (light-cljs-783).
        let source = "function dormant() { return new Date(); } 1;";
        let export = export_source("dead.js", source).unwrap();
        assert!(
            export.operations.contains("construct"),
            "{:?}",
            export.operations
        );
        let construct = rows(&export, 3)
            .into_iter()
            .find(|row| row[0] & 255 == Opcode::Construct as u32)
            .unwrap();
        assert!(source[construct[2] as usize..construct[3] as usize].contains("new"));
    }
    #[test]
    fn dead_direct_eval_exports_with_exactly_its_call_marked() {
        // A dead function's direct eval no longer refuses the program
        // (light-cljs-823): exactly the site's call carries the marker bit.
        let source = "function f(s) { g(s); return eval(s); } function h(s) { return eval(...s); } \
                      var o = { eval(s) { return s; } }; o.eval(1); (0, eval); 1;";
        let export = export_source("eval.js", source).unwrap();
        let marked: Vec<Vec<u32>> = rows(&export, 3)
            .into_iter()
            .filter(|row| row[0] & DIRECT_EVAL_SITE != 0)
            .collect();
        let ops: Vec<u32> = marked.iter().map(|row| row[0] & 255).collect();
        assert_eq!(
            ops,
            [Opcode::Call as u32, Opcode::CallSpread as u32],
            "{marked:?}"
        );
        for row in &marked {
            // The call's span is its `(`, right after the callee `eval`.
            let text = &source[row[2] as usize - 4..row[3] as usize];
            assert!(text.starts_with("eval("), "{text:?}");
            assert_eq!(row[0] >> 25, 0);
        }
        // No descriptor sets bits 25..31.
        assert!(rows(&export, 3).iter().all(|row| row[0] >> 24 <= 1));
        assert_eq!(VERSION, 6);
        assert_eq!(
            u32::from_le_bytes(export.bytes[4..8].try_into().unwrap()),
            6
        );
        // Version 6 (light-cljs-858): one site row per marked call, in row
        // order, its sources in capture order and its record the site.
        let (sites, sources, records) = eval_section(&export);
        let instructions = rows(&export, 3);
        let marked_rows: Vec<u32> = (0..instructions.len() as u32)
            .filter(|&i| instructions[i as usize][0] & DIRECT_EVAL_SITE != 0)
            .collect();
        assert_eq!(sites.iter().map(|s| s[0]).collect::<Vec<_>>(), marked_rows);
        let (mut next_source, mut next_record) = (0, 0);
        for site in &sites {
            assert_eq!((site[1], site[3]), (next_source, next_record));
            let record = &records[site[3] as usize..(site[3] + site[4]) as usize];
            let decoded = decode_eval_site(record).expect("the record decodes");
            assert_eq!(encode_eval_site(&decoded), record);
            let expected: Vec<[u32; 3]> = eval_captures(&decoded)
                .into_iter()
                .map(|(s, role, cell)| {
                    let (kind, index) = eval_source_row(s);
                    [kind, index, role | if cell { EVAL_CELL } else { 0 }]
                })
                .collect();
            assert_eq!(
                &sources[site[1] as usize..(site[1] + site[2]) as usize],
                &expected[..]
            );
            // `f(s)`: the binding `s` (and `f`, `g`...) resolve in the caller.
            assert!(decoded.bindings.iter().any(|b| b.name == "s"));
            next_source += site[2];
            next_record += site[4];
        }
        assert_eq!(
            (next_source as usize, next_record as usize),
            (sources.len(), records.len())
        );
    }
    /// The eval-site section after the UTF-16 pool: site rows, source rows and
    /// record bytes.
    fn eval_section(export: &Export) -> (Vec<[u32; 5]>, Vec<[u32; 3]>, Vec<u8>) {
        let bytes = &export.bytes;
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let mut at = 32 + word(8) as usize;
        for (t, width) in [FUNCTION_WORDS * 4, 12, 4, 20, 2].iter().enumerate() {
            at += word(12 + 4 * t) as usize * width;
        }
        let (ns, nsrc, nrec) = (
            word(at) as usize,
            word(at + 4) as usize,
            word(at + 8) as usize,
        );
        at += 12;
        let sites = (0..ns)
            .map(|i| std::array::from_fn(|w| word(at + (i * 5 + w) * 4)))
            .collect();
        at += ns * 20;
        let sources = (0..nsrc)
            .map(|i| std::array::from_fn(|w| word(at + (i * 3 + w) * 4)))
            .collect();
        at += nsrc * 12;
        assert_eq!(at + nrec, bytes.len());
        (sites, sources, bytes[at..].to_vec())
    }
    /// A site record round-trips; a malformed or inconsistent one is refused.
    #[test]
    fn eval_site_records_round_trip_and_refuse_malformed_bytes() {
        let source = "function f(a) { 'use strict'; let b = 1; return () => eval(a + b); } \
                      function g(x = eval('1')) { var v; with ({}) { eval(x); } }";
        let export = export_source_with("eval.js", source, ScriptOptions::SLOPPY).unwrap();
        let (sites, _, records) = eval_section(&export);
        assert_eq!(sites.len(), 3);
        for site in &sites {
            let record = &records[site[3] as usize..(site[3] + site[4]) as usize];
            let decoded = decode_eval_site(record).unwrap();
            assert_eq!(encode_eval_site(&decoded), record);
            for cut in 0..record.len() {
                assert!(decode_eval_site(&record[..cut]).is_none(), "{cut}");
            }
            let mut longer = record.to_vec();
            longer.push(0);
            assert!(decode_eval_site(&longer).is_none());
            // A site whose variable environment is not one of its
            // environments is refused, as the engine's loader refuses it.
            let mut bad = decoded.clone();
            bad.variable_environment = Some(EvalSource::Local(9));
            bad.environments.retain(|s| *s != EvalSource::Local(9));
            assert!(decode_eval_site(&encode_eval_site(&bad)).is_none());
        }
        let strict =
            decode_eval_site(&records[sites[0][3] as usize..][..sites[0][4] as usize]).unwrap();
        assert!(strict.strict && strict.bindings.iter().any(|b| b.name == "b"));
    }
    /// Function flags bit 19 marks a simple parameter list (light-cljs-854):
    /// not a default, rest or destructured parameter, nor the script.
    #[test]
    fn simple_parameter_lists_are_flagged() {
        let source = "function a(x, y) {}\nfunction b() {}\nvar m = { m(p) {} };\nvar e = (q) => q;\nfunction c(x = 1) {}\nfunction d(...r) {}\nfunction f({ k }) {}\nfunction g(x, [y]) {}";
        let export = export_source("simple.js", source).unwrap();
        let mut simple = Vec::new();
        let mut other = Vec::new();
        for row in rows(&export, 0) {
            if row[12] & SOURCE_TEXT == 0 {
                assert_eq!(row[12] & SIMPLE_PARAMETERS, 0, "the script");
                continue;
            }
            let text = &source[row[13] as usize..row[14] as usize];
            if row[12] & SIMPLE_PARAMETERS != 0 {
                simple.push(text);
            } else {
                other.push(text);
            }
        }
        simple.sort_unstable();
        other.sort_unstable();
        assert_eq!(
            simple,
            [
                "(q) => q",
                "function a(x, y) {}",
                "function b() {}",
                "m(p) {}"
            ]
        );
        assert_eq!(
            other,
            [
                "function c(x = 1) {}",
                "function d(...r) {}",
                "function f({ k }) {}",
                "function g(x, [y]) {}"
            ]
        );
    }
    /// Light-cljs-860: only the Goose lowering makes mapped arguments
    /// objects (NewMappedArguments), and only for a sloppy function with a
    /// simple, nonempty parameter list that could observe the aliasing: it
    /// assigns a parameter, lets its `arguments` escape (anything but the
    /// base of a property read) or has a direct eval. The Pants lowering of
    /// the same sloppy source keeps NewArguments everywhere.
    #[test]
    fn mapped_arguments_are_goose_only_and_observable() {
        use crate::frontend::{Backend, compile_script_for};
        fn walk(e: &Executable, source: &str, out: &mut Vec<(String, &'static str)>) {
            let spec = e.to_spec();
            let (mut offset, mut mapped, mut unmapped) = (0, false, false);
            while offset < spec.code.len() {
                let op = Opcode::from_byte(spec.code[offset]).unwrap();
                mapped |= op == Opcode::NewMappedArguments;
                unmapped |= op == Opcode::NewArguments;
                offset += 1 + op.operand().size();
            }
            if let Some(span) = e.source_text_span() {
                let text = &source[span.start..span.end];
                let name = text
                    .split(['(', ' '])
                    .find(|w| !w.is_empty() && *w != "function");
                let kind = match (mapped, unmapped) {
                    (true, false) => "mapped",
                    (false, true) => "unmapped",
                    (false, false) => "none",
                    (true, true) => "both",
                };
                out.push((name.unwrap_or("").to_string(), kind));
            }
            let mut i = 0;
            while let Some(child) = e.function(i) {
                walk(&child, source, out);
                i += 1;
            }
        }
        let source = "function a(x) { x = 2; return arguments[0]; }
function b(x) { arguments[0] = 2; return x; }
function c(x) { return arguments.length + arguments[0] + arguments.callee.name; }
function d(x) { return String(arguments); }
function e() { arguments[0] = 1; return arguments; }
function f(x) { eval('x'); return arguments.length; }
function g(x) { 'use strict'; x = 1; return arguments[0]; }
function h(x = 1) { x = 2; return arguments[0]; }
function i(x) { return () => arguments[0] + (x = 1); }
function j(x) { return x; }
function k(x, x) { x = 1; return arguments[1]; }
var o = { m(x) { x = 1; return arguments[0]; } };
function l(arguments) { arguments = 1; return arguments; }
function n(x) { var arguments; arguments[0] = 1; return x; }
function p(x) { function x() {} return arguments[0]; }
function q(x) { return arguments[0](); }
function r(x) { delete arguments[0]; return x; }";
        let mut goose = Vec::new();
        let executable =
            compile_script_for("mapped.js", source, ScriptOptions::SLOPPY, Backend::Goose).unwrap();
        walk(&executable, source, &mut goose);
        goose.sort();
        let expected: Vec<(String, &str)> = [
            ("a", "mapped"),
            ("b", "mapped"),
            ("c", "unmapped"),
            ("d", "mapped"),
            ("e", "unmapped"),
            ("f", "mapped"),
            ("g", "unmapped"),
            ("h", "unmapped"),
            ("i", "mapped"),
            ("j", "none"),
            ("k", "mapped"),
            ("l", "none"),
            ("m", "mapped"),
            ("n", "mapped"),
            ("p", "mapped"),
            ("q", "mapped"),
            ("r", "mapped"),
            ("x", "none"),
            (")", "none"),
        ]
        .iter()
        .map(|(n, k)| (n.to_string(), *k))
        .collect();
        let mut expected = expected;
        expected.sort();
        assert_eq!(goose, expected);
        let mut pants = Vec::new();
        let executable =
            crate::frontend::compile_script_with("mapped.js", source, ScriptOptions::SLOPPY)
                .unwrap();
        walk(&executable, source, &mut pants);
        assert!(pants.iter().all(|(_, kind)| *kind != "mapped"), "{pants:?}");
        // The exporter lowers for Goose and accepts the new instruction.
        let export = export_source_with("mapped.js", source, ScriptOptions::SLOPPY).unwrap();
        let mapped = Opcode::NewMappedArguments as u32;
        assert!(rows(&export, 3).iter().any(|row| row[0] & 0xff == mapped));
    }
    /// Each function's source text is QuickJS's `debug.source` (probed with
    /// qjs at 7d1c13c, light-cljs-853): declarations and expressions from
    /// `function`/`async`, methods and accessors from their property name
    /// with `get`/`set`/`async`/`*` (never `static`), arrows whole, a class
    /// constructor the whole class; the script and a class's synthesized
    /// field initializer have none.
    #[test]
    fn function_source_text_is_exported() {
        let source = r#"function  f ( a, b ) { return a; }
(function (x) { return x });
(function* g(x) { yield x });
(async function h(x) { await x });
(async function* ag(x) { });
(async (x) => x);
(x => x * 2);
((a, b) => { return a; });
var o = { m(a) { return a; }, get p() { return 1; }, set p(v) {}, *gen() {}, async am() {}, async *agm() {}, ['comp' + 1](z) {}, 'str'(q) {}, 42(n) {}, f: function() {}, a: () => 1 };
class A { constructor(x) { this.x = x; } m() {} static s() {} get g() { return 1 } static t = 1; u = 2; }
class B extends A {}
var C = class Named { };
(function /*a*/ named /*b*/ (/*c*/) /*d*/ {/*e*/});
var μ = function μ() { return "é"; };"#;
        let export = export_source("text.js", source).unwrap();
        let mut texts = Vec::new();
        let mut without = 0;
        for row in rows(&export, 0) {
            if row[12] & SOURCE_TEXT == 0 {
                assert_eq!((row[13], row[14]), (0, 0));
                without += 1;
                continue;
            }
            texts.push(&source[row[13] as usize..row[14] as usize]);
        }
        texts.sort_unstable();
        let mut expected = vec![
            "function  f ( a, b ) { return a; }",
            "function (x) { return x }",
            "function* g(x) { yield x }",
            "async function h(x) { await x }",
            "async function* ag(x) { }",
            "async (x) => x",
            "x => x * 2",
            "(a, b) => { return a; }",
            "m(a) { return a; }",
            "get p() { return 1; }",
            "set p(v) {}",
            "*gen() {}",
            "async am() {}",
            "async *agm() {}",
            "['comp' + 1](z) {}",
            "'str'(q) {}",
            "42(n) {}",
            "function() {}",
            "() => 1",
            "class A { constructor(x) { this.x = x; } m() {} static s() {} get g() { return 1 } static t = 1; u = 2; }",
            "m() {}",
            "s() {}",
            "get g() { return 1 }",
            "class B extends A {}",
            "class Named { }",
            "function /*a*/ named /*b*/ (/*c*/) /*d*/ {/*e*/}",
            "function μ() { return \"é\"; }",
        ];
        expected.sort_unstable();
        assert_eq!(texts, expected);
        // The script, A's instance and static field initializers and B's
        // instance initializer (a derived class gets one too).
        assert!(without >= 3, "{without}");
    }
    #[test]
    fn symbol_bigint_constants_and_function_kinds_are_encoded() {
        let export = export_source_with(
            "kinds.js",
            "var x; with ({}) { x; } 0n; 12345678901234567890n; class A {} class B extends A {} \
             class C extends A { constructor() { super(); } } function* g() {} \
             async function h() {} async function* k() {} function o() {}",
            ScriptOptions::SLOPPY,
        )
        .unwrap();
        let constants = rows(&export, 1);
        let unscopables = WELL_KNOWN_SYMBOLS
            .iter()
            .position(|n| *n == "unscopables")
            .unwrap();
        assert!(
            constants.contains(&vec![SYMBOL_CONSTANT, unscopables as u32, 0]),
            "{constants:?}"
        );
        let bigints: Vec<Vec<u16>> = constants
            .iter()
            .filter(|row| row[0] == BIGINT_CONSTANT)
            .map(|row| units(&export, row[1], row[2]))
            .collect();
        let big = 12345678901234567890u64;
        let digits = (0..4).map(|i| (big >> (16 * i)) as u16);
        assert!(bigints.contains(&vec![0]), "{bigints:?}");
        assert!(
            bigints.contains(&std::iter::once(0).chain(digits).collect()),
            "{bigints:?}"
        );
        let mut kinds: Vec<u32> = rows(&export, 0)
            .iter()
            .map(|f| (f[12] & !(SOURCE_TEXT | SIMPLE_PARAMETERS)) >> 8)
            .collect();
        kinds.sort();
        // Script (0); A, B, C, the default derived B's (2, 4, 3); g, h, k, o:
        // ordinary (1) only for o; generator 1 << 8, async 2 << 8 above the kind
        // (the source-text bit, 18, masked).
        for kind in [0, 1, 2, 3, 4, 0x100, 0x200, 0x300] {
            assert!(kinds.contains(&kind), "{kind:#x} in {kinds:x?}");
        }
    }
    #[test]
    fn exported_code_round_trips_through_the_engine_validator() {
        // The exporter writes `Executable::to_spec` bytes, so every operand
        // must survive it: `using` dispose counts once encoded as 0, which
        // the Goose flow checker rejected (pants/generator_return_throw).
        let source = "function* g() { try { using a = null, b = null; yield 1; } finally { 0; } } \
                      async function h() { try { await using c = null; } finally { 0; } } \
                      function k(...r) { const { x, ...y } = r; return `${x}` + [...y]; }";
        let executable =
            crate::frontend::compile_script_with("round.js", source, ScriptOptions::STRICT)
                .unwrap();
        let mut pending = vec![executable];
        let mut seen = 0;
        while let Some(e) = pending.pop() {
            let spec = e.to_spec();
            let again = Executable::load(e.to_spec())
                .unwrap_or_else(|error| panic!("{}: {error}", spec.name));
            assert_eq!(again.to_spec().code, spec.code, "{}", spec.name);
            pending.extend(spec.functions);
            seen += 1;
        }
        assert!(seen >= 4);
        let export = export_source("round.js", source).unwrap();
        assert!(
            export.operations.contains("using_dispose")
                && export.operations.contains("using_dispose_async")
        );
    }
    #[test]
    fn generated_schema_has_a_stack_effect_for_every_opcode() {
        use crate::vm::instr::{Flow, FormOp, Instr};
        let schema = inspection_schema();
        let table = schema
            .split("fn export_stack_effect(op: i64) -> i64 {")
            .nth(1)
            .unwrap()
            .split("_ => -1")
            .next()
            .unwrap();
        let byte = |name: &str| {
            (0..=255)
                .filter_map(Opcode::from_byte)
                .find(|op| format!("opcode::{op:?}") == name)
                .unwrap() as u8
        };
        let words: std::collections::BTreeMap<u8, u64> = table
            .lines()
            .filter_map(|line| line.trim().trim_end_matches(',').split_once(" => "))
            .map(|(op, word)| (byte(op), word.parse().unwrap()))
            .collect();
        assert_eq!(words.len(), 199);
        for (&byte, &word) in &words {
            let op = Opcode::from_byte(byte).unwrap();
            let (base, pushes) = ((word & 255) as usize, ((word >> 8) & 255) as usize);
            let (multiplier, source) = (((word >> 16) & 255) as usize, (word >> 24) & 15);
            for n in [0u16, 1, 3, 200] {
                let (operand, captures, count) = match source {
                    0 => (u32::from(n), n, 0),
                    1 => (u32::from(n), 0, usize::from(n)),
                    2 => (u32::from(n), n, usize::from(n)),
                    _ => {
                        let form = FormOp::ALL[usize::from(n) % FormOp::ALL.len()];
                        (form as u32, 0, form.arity())
                    }
                };
                let Some(instr) = Instr::decode(op, operand, 0, captures) else {
                    assert_eq!(op, Opcode::ThrowError);
                    continue;
                };
                assert_eq!(
                    instr.stack_effect(),
                    (base + multiplier * count, pushes),
                    "{op:?} {n}"
                );
            }
            let flow = [Flow::Next, Flow::Jump, Flow::Branch, Flow::Catch, Flow::End];
            assert_eq!(flow[(word >> 28) as usize], op.flow(), "{op:?}");
        }
    }
    #[test]
    fn operator_provenance_uses_utf8_bytes_of_the_exact_source() {
        let source = "function source(μ) { return μ + 3; }";
        let export = export_function(source).unwrap();
        let word = |offset: usize| {
            u32::from_le_bytes(export.bytes[offset..offset + 4].try_into().unwrap()) as usize
        };
        let instructions =
            32 + source.len() + word(12) * FUNCTION_WORDS * 4 + word(16) * 12 + word(20) * 4;
        let mut found = false;
        for i in 0..word(24) {
            let at = instructions + i * 20;
            let span = &source[word(at + 8)..word(at + 12)];
            if word(at) & 255 == Opcode::Plus as usize {
                assert_eq!(span, "+");
                found = true;
            }
        }
        assert!(found);
        let excessive = format!("{}0{}", "(".repeat(300), ")".repeat(300));
        assert_eq!(
            export_source("deep.js", &excessive).unwrap_err().kind,
            SyntaxErrorKind::TooDeep
        );
    }
    #[test]
    fn budgets_check_wide_inputs_before_narrowing() {
        assert!(bounded(65_536, MAX_FUNCTIONS, "").is_ok());
        assert!(bounded(65_537, MAX_FUNCTIONS, "").is_err());
        assert!(bounded(u32::MAX as usize + 1, usize::MAX, "").is_err());
        let source = " ".repeat(MAX_SOURCE + 1);
        assert_eq!(
            export_source("large.js", &source).unwrap_err().kind,
            SyntaxErrorKind::TooLarge
        );
    }
}

#[cfg(test)]
mod serial_tests {
    use super::*;

    // Independent Node/V8 and Pants expectations for original small source
    // cases. They exercise language execution, not macro expansion coverage.
    #[test]
    #[ignore = "bounded Babashka generator inputs supplied by correctness.bb"]
    fn generated_sources_for_goose() {
        let root = std::path::PathBuf::from(
            std::env::var_os("GOOSE_CORRECTNESS_INPUTS").expect("run correctness.bb"),
        );
        let mut inputs = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|e| e == "js"))
            .collect::<Vec<_>>();
        inputs.sort();
        assert_eq!(inputs.len(), 45, "the v1 bounded manifest must be complete");
        for path in inputs {
            let source = std::fs::read_to_string(&path).unwrap();
            let export = export_function(&source)
                .unwrap_or_else(|error| panic!("{}: {error}\n{source}", path.display()));
            std::fs::write(path.with_extension("gsp"), export.bytes).unwrap();
        }
    }
    /// light-cljs-856: run-time code exports from exact UTF-16 units; a lone
    /// surrogate in a literal is carried, one where a token belongs is a
    /// compile error with its position; the Function constructor's source
    /// splits at its parameter boundary.
    #[test]
    fn runtime_export_takes_utf16_and_modes() {
        let units = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        let mut literal = units("\"");
        literal.push(0xd800);
        literal.extend(units("\" + 1"));
        let export = export_runtime(&literal, RuntimeMode::IndirectEval { strict: false }).unwrap();
        assert!(export.bytes.starts_with(b"GSP\0") && export.functions == 1);
        let mut token = units("var ");
        token.push(0xdc00);
        let error =
            export_runtime(&token, RuntimeMode::IndirectEval { strict: false }).unwrap_err();
        assert!(
            error.kind != SyntaxErrorKind::Internal && (error.line, error.column) == (1, 5),
            "{error}"
        );
        let error =
            export_runtime(&units("1 +"), RuntimeMode::IndirectEval { strict: false }).unwrap_err();
        assert!(error.kind != SyntaxErrorKind::Internal, "{error}");
        // A strict eval refuses what a sloppy one accepts.
        let with = units("with ({}) {}");
        assert!(export_runtime(&with, RuntimeMode::IndirectEval { strict: false }).is_ok());
        assert!(export_runtime(&with, RuntimeMode::IndirectEval { strict: true }).is_err());
        let source = units("function anonymous(a, b\n) {\nreturn a + b\n}");
        let parameter_units = "function anonymous(a, b\n)".len();
        let export =
            export_runtime(&source, RuntimeMode::DynamicFunction { parameter_units }).unwrap();
        assert!(export.functions >= 1);
        assert!(
            export_runtime(
                &source,
                RuntimeMode::DynamicFunction {
                    parameter_units: source.len() + 1
                }
            )
            .is_err()
        );
    }
    /// Light-cljs-860: code compiled while a Goose job runs lowers for Goose
    /// too, so a sloppy function it defines that writes a parameter gets
    /// NewMappedArguments, in an indirect eval, a Function body and a direct
    /// eval alike; a strict one, or one with a default, does not.
    #[test]
    fn runtime_modes_lower_mapped_arguments_for_goose() {
        let units = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        let mapped = |export: &Export| {
            super::tests::rows(export, 3)
                .iter()
                .any(|row| row[0] & 0xff == Opcode::NewMappedArguments as u32)
        };
        let eval = |text: &str| {
            export_runtime(&units(text), RuntimeMode::IndirectEval { strict: false }).unwrap()
        };
        assert!(mapped(&eval(
            "(function (a) { a = 2; return arguments[0]; })"
        )));
        assert!(!mapped(&eval(
            "(function (a) { 'use strict'; a = 2; return arguments[0]; })"
        )));
        assert!(!mapped(&eval(
            "(function (a, b = 1) { a = 2; return arguments[0]; })"
        )));
        let body = "function anonymous(a\n) {\na = 2; return arguments[0]\n}";
        let parameter_units = "function anonymous(a\n)".len();
        let dynamic = export_runtime(
            &units(body),
            RuntimeMode::DynamicFunction { parameter_units },
        )
        .unwrap();
        assert!(mapped(&dynamic));
        let caller = crate::frontend::compile_script_with(
            "site.js",
            "function f() { return eval(s); }",
            ScriptOptions::SLOPPY,
        )
        .unwrap();
        let site = caller.function(0).unwrap().to_spec().eval_sites[0].clone();
        let direct = export_runtime(
            &units("(function (a) { arguments[0] = 2; return a; })"),
            RuntimeMode::DirectEval { site },
        )
        .unwrap();
        assert!(mapped(&direct));
    }
    #[test]
    fn source_cases_export_and_match_reference_engines() {
        use crate::{RunOutcome, Runtime, ValueView, VmErrorKind};
        fn outcome(outcome: RunOutcome) -> String {
            match outcome {
                RunOutcome::Complete(value) => match value.view() {
                    ValueView::Undefined => "undefined:undefined".into(),
                    ValueView::Bool(b) => format!("boolean:{b}"),
                    ValueView::Number(n) if n == 0.0 && n.is_sign_negative() => "number:-0".into(),
                    ValueView::Number(n) => format!("number:{n}"),
                    ValueView::String => {
                        format!("string:{}", value.string().unwrap().to_string_lossy())
                    }
                    other => panic!("unexpected {other:?}"),
                },
                RunOutcome::Failed(error) => match error.kind() {
                    VmErrorKind::ReferenceError => "exception:ReferenceError".into(),
                    VmErrorKind::TypeError => "exception:TypeError".into(),
                    other => panic!("unexpected {other:?}"),
                },
                other => panic!("unexpected {other:?}"),
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../corpus/serial");
        let regenerate = std::env::var_os("GOOSE_REGENERATE").is_some();
        let schema = root
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("src/export_schema.goose");
        if regenerate {
            std::fs::write(&schema, inspection_schema()).unwrap();
        } else {
            assert_eq!(
                std::fs::read_to_string(&schema).unwrap(),
                inspection_schema()
            );
        }
        for (name, expected) in [
            ("calls", "boolean:true"),
            ("arguments", "boolean:true"),
            ("captures", "boolean:true"),
            ("mutations", "boolean:true"),
            ("control", "boolean:true"),
            ("numbers", "boolean:true"),
            ("strings", "boolean:true"),
            ("global-undefined", "undefined:undefined"),
            ("global-this", "boolean:true"),
            ("global-missing", "exception:ReferenceError"),
            ("negative-zero", "number:-0"),
            ("nan", "number:NaN"),
            ("tdz", "exception:ReferenceError"),
            ("type-error", "exception:TypeError"),
            ("not-callable", "exception:TypeError"),
            ("unsupported-coercion", "number:3"),
            // StringToNumber in unary and binary operators, `++`, `<` and
            // `==` (light-cljs-827; its work limits in serial_tests.goose).
            ("string-to-number", "boolean:true"),
            ("unsupported-prototype", "string:[object Object]"),
            // Unimplemented operations: `new Date` on a dead path, and a
            // `using` declaration reached under a `catch` that cannot swallow
            // its fault there (serial_tests.goose).
            ("unsupported-dead", "number:7"),
            ("unsupported-reached", "number:1"),
            // [[Construct]] of ordinary functions and the realm's
            // constructors, at every budget cut (light-cljs-831), and a
            // dependency wait inside a constructor (snapshot_tests.goose).
            ("construct", "boolean:true"),
            ("construct-wait", "goose"),
            // Nor BigInt values: constants on dead paths, then one reached.
            ("dormant-constants", "number:7"),
            ("bigint-reached", "string:bigint"),
            // Direct eval (light-cljs-858): Goose runs a reached one in the
            // caller's scope, spread or not; a marked call whose `eval` is
            // not %eval% is an ordinary call; a loop in direct eval code
            // stops and resumes at every limit and cancellation inside it
            // runs the caller's finally blocks (serial_tests.goose).
            ("direct-eval-reached", "number:2"),
            ("direct-eval-spread", "number:2"),
            ("direct-eval-replaced", "number:2"),
            ("direct-eval-loop", "number:25"),
            ("cancel-direct-eval", "limit"),
            // Spread-argument limits (light-cljs-850), the Pants engine's
            // 65,535 (Node collects the whole iterator): serial_tests.goose
            // runs it under the default and a 128-value stack limit.
            ("spread-limits", "goose"),
            // Indirect eval (light-cljs-856) compiles at run time; a closure
            // it makes survives a captured snapshot and leaves with the
            // worker's reset (snapshot_tests.goose).
            ("indirect-eval", "number:2"),
            ("eval-closure", "goose"),
            // So does one a direct eval makes, with the eval sites of the
            // image and of the eval code (light-cljs-858).
            ("direct-eval-closure", "goose"),
            ("eval-loop", "number:25"),
            ("cancel-eval", "limit"),
            ("eval-source-limit", "goose"),
            // The Function constructor (light-cljs-857): conversions and
            // the new function's loop stop and resume at every limit.
            ("function-constructor", "number:25"),
            // A generator function's run-time constructor fails by name.
            ("function-kind-constructor", "goose"),
            // A dependency wait inside its conversions (snapshot_tests.goose).
            ("function-constructor-wait", "goose"),
            ("loop", "limit"),
            // Control flow (light-cljs-786): finally blocks, native frames,
            // the call depth limit, and an endless loop that only
            // cancellation ends (serial_tests.goose).
            ("finally", "boolean:true"),
            ("native-frame", "boolean:true"),
            // ToPrimitive with hints and Symbol.toPrimitive in every
            // operator family (light-cljs-828).
            ("to-primitive", "boolean:true"),
            // Array.prototype.map and forEach as native frames (light-cljs-789).
            ("array-callbacks", "boolean:true"),
            // Array.prototype.push, join and toString as native frames (light-cljs-842).
            ("array-push-join", "boolean:true"),
            // Bound functions through Forward frames (light-cljs-852).
            ("bound-functions", "boolean:true"),
            // Receivers and strings only serial_tests.goose supplies: push
            // onto rows the job did not allocate, join's work and unit limits.
            ("array-push-receiver", "goose"),
            ("array-join-strings", "goose"),
            // for-of over user iterables and generators (light-cljs-843): at
            // every budget cut, dependency waits in its protocol calls and
            // body (snapshot_tests.goose), and cancellation inside it.
            ("for-of", "boolean:true"),
            ("for-of-wait", "goose"),
            ("cancel-for-of", "limit"),
            // The built-in array and string iterators (light-cljs-844): at
            // every budget cut, dependency waits in an element getter and
            // loop bodies (snapshot_tests.goose), and sources in rows the
            // job did not allocate (serial_tests.goose).
            ("builtin-iterators", "boolean:true"),
            ("builtin-iterator-wait", "goose"),
            ("builtin-iterator-receiver", "goose"),
            // for-in over proxies (light-cljs-845): traps, handler getters
            // and descriptor fields at every budget cut, and a dependency
            // wait in every trap phase, getPrototypeOf's included, which the
            // Pants engine does not implement (snapshot_tests.goose).
            ("proxy-for-in", "boolean:true"),
            ("proxy-for-in-wait", "goose"),
            ("proxy-get", "goose"),
            ("depth", "boolean:true"),
            ("cancel", "limit"),
            ("cancel-break", "limit"),
            ("cancel-fault", "limit"),
            // The property model (light-cljs-787): accessors, prototype
            // chains, deletion and a strict assignment without a setter.
            ("accessors", "boolean:true"),
            ("prototypes", "boolean:true"),
            ("deletes", "boolean:true"),
            ("getter-only", "exception:TypeError"),
            // Key enumeration (light-cljs-804): for-in over a three-object
            // chain, object rest and spread, and ordinary objects' index keys.
            ("for-in", "boolean:true"),
            ("object-rest", "boolean:true"),
            ("index-keys", "boolean:true"),
            // Arrays' and functions' named properties, a function's lazy
            // `prototype` and ToPrimitive through getters (light-cljs-805);
            // a function's own `name` (light-cljs-806); an array method and a
            // generator's `prototype` stay explicit gaps (serial_tests.goose).
            ("array-names", "boolean:true"),
            ("function-names", "boolean:true"),
            ("getter-value-of", "boolean:true"),
            ("function-name", "string:f"),
            ("function-keys", "boolean:true"),
            ("unsupported-array-method", "number:1"),
            ("unsupported-generator-prototype", "boolean:true"),
            // Generators, async functions and promise jobs (light-cljs-790).
            // `async` checks its order in a promise job: a wrong one is an
            // unhandled rejection, which fails Node's run. The "goose" cases
            // end in ways only the Goose job defines (an async entry's
            // settled value, a promise that never settles, a dependency
            // wait, a generator left suspended; serial_tests.goose and
            // snapshot_tests.goose).
            ("generators", "boolean:true"),
            ("async", "boolean:true"),
            ("generator-loop", "boolean:true"),
            ("async-entry", "goose"),
            ("unsettled", "goose"),
            ("suspended-wait", "goose"),
            ("suspended-generator", "goose"),
            ("suspended-stack", "goose"),
            ("cancel-suspended", "goose"),
            ("generator-limit", "goose"),
            ("promise-then", "goose"),
            // Reads and calls on primitive bases through their prototypes
            // (light-cljs-833); serial_tests.goose compares node rows.
            ("primitive-methods", "number:400"),
            // A Symbol base fails by name on Goose (serial_tests.goose).
            ("symbol-base-read", "goose"),
            ("symbol-base-write", "goose"),
            // Reclamation inside a job (light-cljs-847): loops far past the
            // Goose job's row limits, live values held by generators,
            // promises, for-of and a native frame across collections, and
            // a live set over the node limit, which still ends MemoryLimit
            // (serial_tests.goose).
            ("reclaim-loops", "boolean:true"),
            ("reclaim-roots", "boolean:true"),
            ("reclaim-bursts", "boolean:true"),
            ("reclaim-observed", "number:2"),
            ("reclaim-live", "goose"),
        ] {
            let source = std::fs::read_to_string(root.join(format!("{name}.js"))).unwrap();
            let export = export_function(&source).unwrap_or_else(|error| panic!("{name}: {error}"));
            let path = root.join(format!("{name}.gsp"));
            if regenerate {
                std::fs::write(path, &export.bytes).unwrap();
            } else {
                assert_eq!(std::fs::read(path).unwrap(), export.bytes, "{name}");
            }
            if name == "loop" || name.starts_with("cancel") || expected == "goose" {
                continue;
            }
            // Node's default native stack holds fewer than the 10,000 frames
            // `depth` needs; the Pants engine below is its oracle.
            if name != "depth" {
                let js = format!(
                    "try {{ const value = ({source})(); console.log(typeof value === 'number' ? 'number:' + (Object.is(value, -0) ? '-0' : String(value)) : typeof value + ':' + String(value)); }} catch(e) {{ console.log('exception:' + e.name); }}"
                );
                let output = std::process::Command::new("node")
                    .args(["-e", &js])
                    .output()
                    .unwrap();
                assert!(output.status.success());
                assert_eq!(
                    String::from_utf8(output.stdout).unwrap().trim(),
                    expected,
                    "{name}"
                );
            }
            let executable = crate::frontend::compile_function(&source).unwrap();
            let actual = outcome(
                Runtime::new()
                    .spawn(&executable, &[])
                    .unwrap()
                    .run()
                    .unwrap(),
            );
            assert_eq!(actual, expected, "{name}");
        }
        // Scripts (light-cljs-824): top-level declarations and global names,
        // exported as the conformance harness exports a strict script. Run
        // as they are, the Pants engine gives the expectation; Goose also
        // runs them over global properties it installs first, and runs the
        // "goose" case only that way (serial_tests.goose). A "sloppy-" script
        // is exported as a sloppy one.
        for (name, expected) in [
            ("script-declarations", "boolean:true"),
            ("script-setter", "goose"),
            // A Symbol.toPrimitive getter Goose installs (light-cljs-828).
            ("to-primitive-getter", "goose"),
            ("script-reserved-function", "number:1"),
            ("script-reserved-let", "number:2"),
            ("script-reserved-var", "boolean:false"),
            ("script-throw-messages", "boolean:true"),
            ("script-suspension", "boolean:true"),
            // Primitive receivers (light-cljs-833): accessors Goose installs
            // on realm prototypes, and a sloppy `this`, which the Pants
            // engine fails by name.
            ("primitive-receiver", "goose"),
            ("sloppy-this", "goose"),
            // Getters of Symbol.hasInstance and `prototype` Goose installs,
            // run by instanceof (light-cljs-834).
            ("instanceof-hooks", "goose"),
            // Targets of bind with `length` and `name` getters and odd
            // values Goose installs (light-cljs-852).
            ("bound-metadata", "goose"),
            // Function.prototype.toString and caller/arguments, which the
            // Pants engine keeps unsupported (light-cljs-853).
            ("sloppy-function-text", "goose"),
            ("function-text-kinds", "goose"),
            // Arguments objects as ordinary objects and their `callee`, which
            // the Pants engine does not define (light-cljs-854).
            ("sloppy-arguments-objects", "goose"),
            // Mapped arguments objects (light-cljs-860), which the Pants
            // engine does not implement.
            ("sloppy-mapped-arguments", "goose"),
            // `with` statements (light-cljs-855), which strict code cannot
            // compile for the Pants reference run.
            ("sloppy-with", "goose"),
        ] {
            let source = std::fs::read_to_string(root.join(format!("{name}.js"))).unwrap();
            let options = if name.starts_with("sloppy-") {
                ScriptOptions::SLOPPY
            } else {
                ScriptOptions::STRICT
            };
            let export = export_source_with(name, &source, options)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            let path = root.join(format!("{name}.gsp"));
            if regenerate {
                std::fs::write(path, &export.bytes).unwrap();
            } else {
                assert_eq!(std::fs::read(path).unwrap(), export.bytes, "{name}");
            }
            if expected == "goose" {
                continue;
            }
            let executable =
                crate::frontend::compile_script_with(name, &source, ScriptOptions::STRICT).unwrap();
            let actual = outcome(
                Runtime::new()
                    .spawn(&executable, &[])
                    .unwrap()
                    .run()
                    .unwrap(),
            );
            assert_eq!(actual, expected, "{name}");
        }
        // Constructors and direct const writes now export (light-cljs-783):
        // the Goose VM fails `construct` by name when it runs, and runs
        // `throw_error` (light-cljs-824; serial_tests.goose). So does a
        // direct eval (light-cljs-823): its call is marked, and the VM fails
        // it as `direct_eval`.
        for (source, operation) in [
            ("function f() { return new Date(); }", "construct"),
            ("function f() { const x=1; x=2; return x; }", "throw_error"),
            ("function f(s) { return eval(s); }", "call"),
        ] {
            let export = export_function(source).unwrap();
            assert!(export.operations.contains(operation), "{source}");
        }
    }
}
