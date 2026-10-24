//! Pattern parser. See [`parse`] for its input and error contract.

use std::collections::HashMap;
use std::fmt;

use super::ir::*;
use crate::JsString;
use crate::frontend::lexer::{is_ident_part, is_ident_start};

/// A positioned pattern SyntaxError. Offset counts UTF-16 code units from the
/// start of the pattern, or from the start of flags for a flag error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError {
    pub message: String,
    pub offset: usize,
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SyntaxError: {} at UTF-16 offset {}",
            self.message, self.offset
        )
    }
}
impl std::error::Error for SyntaxError {}

fn error(offset: usize, message: &str) -> SyntaxError {
    SyntaxError {
        message: message.into(),
        offset,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    pub has_indices: bool,
    pub global: bool,
    pub ignore_case: bool,
    pub multiline: bool,
    pub dot_all: bool,
    pub unicode: bool,
    pub unicode_sets: bool,
    pub sticky: bool,
}

impl Flags {
    pub(super) fn unicode_mode(self) -> bool {
        self.unicode || self.unicode_sets
    }

    /// Validate flag spellings, duplicates and the mutually exclusive u/v pair.
    pub fn parse(text: &JsString) -> Result<Self, SyntaxError> {
        let mut flags = Self::default();
        for (offset, &unit) in text.code_units().iter().enumerate() {
            let slot = match unit {
                100 => &mut flags.has_indices,
                103 => &mut flags.global,
                105 => &mut flags.ignore_case,
                109 => &mut flags.multiline,
                115 => &mut flags.dot_all,
                117 => &mut flags.unicode,
                118 => &mut flags.unicode_sets,
                121 => &mut flags.sticky,
                _ => return Err(error(offset, "unknown regular expression flag")),
            };
            if *slot {
                return Err(error(offset, "duplicate regular expression flag"));
            }
            *slot = true;
            if flags.unicode && flags.unicode_sets {
                return Err(error(offset, "u and v flags cannot be combined"));
            }
        }
        Ok(flags)
    }
}

/// Parse pattern text without literal delimiters. No lossy conversion to UTF-8
/// occurs. The result owns its arena and has no heap/GC edges. Both parsing and
/// dropping nested groups are iterative. Quantifier counts above u64::MAX are
/// rejected explicitly; Script and string property escapes are deferred.
pub fn parse(pattern: &JsString, flags: &JsString) -> Result<Pattern, SyntaxError> {
    let flags_value = Flags::parse(flags)?;
    Parser::new(pattern.code_units(), flags_value, false).run()
}

/// Validate literal syntax without applying matcher property-support limits.
/// The temporary arena is discarded and cannot be passed to the matcher.
pub fn validate_syntax(pattern: &JsString, flags: &JsString) -> Result<(), SyntaxError> {
    let flags_value = Flags::parse(flags)?;
    Parser::new(pattern.code_units(), flags_value, true)
        .run()
        .map(|_| ())
}

enum GroupKind {
    Root,
    Group(Option<usize>),
    Lookaround {
        behind: bool,
        negative: bool,
    },
    Modifiers {
        enable: Modifiers,
        disable: Modifiers,
    },
}

struct Frame {
    start: usize,
    context: usize,
    kind: GroupKind,
    alternatives: Vec<NodeId>,
    terms: Vec<NodeId>,
}

impl Frame {
    fn new(start: usize, context: usize, kind: GroupKind) -> Self {
        Self {
            start,
            context,
            kind,
            alternatives: Vec::new(),
            terms: Vec::new(),
        }
    }
}

/// One alternative in a disjunction, linked to its containing alternative.
/// Capture declarations retain just this index rather than copying ancestry.
struct Context {
    parent: Option<usize>,
    depth: usize,
    disjunction: usize,
}

struct NamedCapture {
    index: usize,
    context: usize,
}

#[derive(Default)]
struct SetFrame {
    start: usize,
    inverted: bool,
    count: usize,
    operator: Option<u16>,
    awaiting: bool,
    range: bool,
    may_strings: bool,
}

impl SetFrame {
    fn operand(&mut self, may_strings: bool, range: bool, out: &mut Vec<ClassSetItem>) {
        if self.count != 0 {
            out.push(match self.operator {
                Some(38) => ClassSetItem::Intersection,
                Some(45) => ClassSetItem::Subtraction,
                _ => ClassSetItem::Union,
            });
            self.may_strings = match self.operator {
                Some(38) => self.may_strings && may_strings,
                Some(45) => self.may_strings,
                _ => self.may_strings || may_strings,
            };
        } else {
            self.may_strings = may_strings;
        }
        self.range |= range;
        self.count += 1;
        self.awaiting = false;
    }
}

fn reserved_set_punctuator(c: u16) -> bool {
    matches!(
        c,
        33 | 35
            | 36
            | 37
            | 38
            | 42
            | 43
            | 44
            | 46
            | 58
            | 59
            | 60
            | 61
            | 62
            | 63
            | 64
            | 94
            | 96
            | 126
    )
}

struct Parser<'a> {
    text: &'a [u16],
    pos: usize,
    flags: Flags,
    syntax_only: bool,
    // Metadata for the last character consumed by set_character. Required for
    // v-mode complement/intersection rules even when property matching is deferred.
    property_may_strings: bool,
    total_captures: usize,
    has_named: bool,
    captures: Vec<Option<JsString>>,
    names: HashMap<JsString, Vec<NamedCapture>>,
    references: Vec<(NodeId, JsString, usize)>,
    nodes: Vec<Node>,
    frames: Vec<Frame>,
    contexts: Vec<Context>,
}

impl<'a> Parser<'a> {
    fn new(text: &'a [u16], flags: Flags, syntax_only: bool) -> Self {
        // Decimal and named escapes depend on captures anywhere in the pattern,
        // including forward references. This lexical scan never interprets a
        // parenthesis inside a class or immediately after a backslash.
        let (mut count, mut named, mut class, mut pos) = (0, false, 0usize, 0);
        while pos < text.len() {
            match text[pos] {
                92 => {
                    pos += 2;
                    continue;
                }
                91 if class == 0 || flags.unicode_sets => class += 1,
                93 if class > 0 => class -= 1,
                40 if class == 0 => {
                    if text.get(pos + 1) != Some(&63) {
                        count += 1;
                    } else if text.get(pos + 2) == Some(&60)
                        && !matches!(text.get(pos + 3), Some(61 | 33))
                    {
                        count += 1;
                        named = true;
                    }
                }
                _ => {}
            }
            pos += 1;
        }
        Self {
            text,
            pos: 0,
            flags,
            syntax_only,
            property_may_strings: false,
            total_captures: count,
            has_named: named,
            captures: Vec::new(),
            names: HashMap::new(),
            references: Vec::new(),
            nodes: Vec::new(),
            frames: vec![Frame::new(0, 0, GroupKind::Root)],
            contexts: vec![Context {
                parent: None,
                depth: 0,
                disjunction: 0,
            }],
        }
    }

    fn peek(&self) -> Option<u16> {
        self.text.get(self.pos).copied()
    }
    fn eat(&mut self, unit: u16) -> bool {
        if self.peek() == Some(unit) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn node(&mut self, node: Node) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(node);
        id
    }
    fn alternative(&mut self) {
        let terms = std::mem::take(&mut self.frames.last_mut().unwrap().terms);
        let id = self.node(Node::Alternative(terms));
        self.frames.last_mut().unwrap().alternatives.push(id);
    }
    fn disjunction(&mut self) -> (Frame, NodeId) {
        self.alternative();
        let mut frame = self.frames.pop().unwrap();
        let body = self.node(Node::Disjunction(std::mem::take(&mut frame.alternatives)));
        (frame, body)
    }

    fn run(mut self) -> Result<Pattern, SyntaxError> {
        let root = loop {
            let start = self.pos;
            let Some(unit) = self.peek() else {
                if self.frames.len() > 1 {
                    return Err(error(
                        self.frames.last().unwrap().start,
                        "unterminated group",
                    ));
                }
                break self.disjunction().1;
            };
            match unit {
                124 => {
                    self.pos += 1;
                    self.alternative();
                    let old = &self.contexts[self.frames.last().unwrap().context];
                    let context = Context {
                        parent: old.parent,
                        depth: old.depth,
                        disjunction: old.disjunction,
                    };
                    self.frames.last_mut().unwrap().context = self.contexts.len();
                    self.contexts.push(context);
                }
                40 => {
                    self.pos += 1;
                    self.open_group(start)?;
                }
                41 => {
                    if self.frames.len() == 1 {
                        return Err(error(start, "unmatched closing parenthesis"));
                    }
                    self.pos += 1;
                    let (frame, body) = self.disjunction();
                    let node = match frame.kind {
                        GroupKind::Group(capture) => Node::Group { body, capture },
                        GroupKind::Lookaround { behind, negative } => Node::Lookaround {
                            body,
                            behind,
                            negative,
                        },
                        GroupKind::Modifiers { enable, disable } => Node::Modifiers {
                            body,
                            enable,
                            disable,
                        },
                        GroupKind::Root => unreachable!(),
                    };
                    let id = self.node(node);
                    self.term(id)?;
                }
                42 | 43 | 63 => return Err(error(start, "nothing to repeat")),
                123 if self.quantifier()?.is_some() => {
                    return Err(error(start, "nothing to repeat"));
                }
                93 | 125 if self.flags.unicode_mode() => {
                    return Err(error(start, "unexpected syntax character"));
                }
                _ => {
                    let node = match unit {
                        94 => {
                            self.pos += 1;
                            Node::Assertion(Assertion::Start)
                        }
                        36 => {
                            self.pos += 1;
                            Node::Assertion(Assertion::End)
                        }
                        46 => {
                            self.pos += 1;
                            Node::Dot
                        }
                        91 if self.flags.unicode_sets => Node::UnicodeSet(self.unicode_set()?),
                        91 => Node::Class(self.character_class()?),
                        92 => self.escape_atom()?,
                        _ => Node::Literal(self.raw_character()),
                    };
                    let id = self.node(node);
                    self.term(id)?;
                }
            }
        };
        for (id, name, offset) in &self.references {
            let captures = self
                .names
                .get(name)
                .ok_or_else(|| error(*offset, "unknown named backreference"))?;
            self.nodes[id.0] =
                Node::Backreference(captures.iter().map(|capture| capture.index).collect());
        }
        Ok(Pattern {
            flags: self.flags,
            nodes: self.nodes,
            root,
            captures: self.captures,
        })
    }

    fn open_group(&mut self, start: usize) -> Result<(), SyntaxError> {
        let kind = if !self.eat(63) {
            self.captures.push(None);
            GroupKind::Group(Some(self.captures.len()))
        } else if self.eat(58) {
            GroupKind::Group(None)
        } else if matches!(self.peek(), Some(61 | 33)) {
            let negative = self.eat(33);
            if !negative {
                self.pos += 1;
            }
            GroupKind::Lookaround {
                behind: false,
                negative,
            }
        } else if self.eat(60) {
            if matches!(self.peek(), Some(61 | 33)) {
                let negative = self.eat(33);
                if !negative {
                    self.pos += 1;
                }
                GroupKind::Lookaround {
                    behind: true,
                    negative,
                }
            } else {
                let name = self.group_name(start)?;
                let context = self.frames.last().unwrap().context;
                if self.names.get(&name).is_some_and(|captures| {
                    captures
                        .iter()
                        .any(|capture| self.might_both_participate(capture.context, context))
                }) {
                    return Err(error(start, "duplicate group name"));
                }
                let index = self.captures.len() + 1;
                self.names
                    .entry(name.clone())
                    .or_default()
                    .push(NamedCapture { index, context });
                self.captures.push(Some(name));
                GroupKind::Group(Some(index))
            }
        } else if matches!(self.peek(), Some(105 | 109 | 115 | 45)) {
            let enable = self.modifiers(start)?;
            let minus = self.eat(45);
            let disable = if minus {
                self.modifiers(start)?
            } else {
                Modifiers::default()
            };
            if enable == Modifiers::default() && disable == Modifiers::default()
                || enable.ignore_case && disable.ignore_case
                || enable.multiline && disable.multiline
                || enable.dot_all && disable.dot_all
                || !self.eat(58)
            {
                return Err(error(start, "invalid group modifiers"));
            }
            GroupKind::Modifiers { enable, disable }
        } else {
            return Err(error(start, "invalid group prefix"));
        };
        let parent = self.frames.last().unwrap().context;
        let context = self.contexts.len();
        self.contexts.push(Context {
            parent: Some(parent),
            depth: self.contexts[parent].depth + 1,
            disjunction: context,
        });
        self.frames.push(Frame::new(start, context, kind));
        Ok(())
    }

    // ECMAScript MightBothParticipate: declarations are disjoint precisely
    // when their ancestry includes different alternatives of one disjunction.
    fn might_both_participate(&self, mut left: usize, mut right: usize) -> bool {
        while left != right {
            let a = &self.contexts[left];
            let b = &self.contexts[right];
            if a.depth > b.depth {
                left = a.parent.unwrap();
            } else if b.depth > a.depth {
                right = b.parent.unwrap();
            } else {
                if a.disjunction == b.disjunction {
                    return false;
                }
                left = a.parent.unwrap();
                right = b.parent.unwrap();
            }
        }
        true
    }

    fn modifiers(&mut self, start: usize) -> Result<Modifiers, SyntaxError> {
        let mut flags = Modifiers::default();
        loop {
            let slot = match self.peek() {
                Some(105) => &mut flags.ignore_case,
                Some(109) => &mut flags.multiline,
                Some(115) => &mut flags.dot_all,
                _ => break,
            };
            if *slot {
                return Err(error(start, "duplicate group modifier"));
            }
            *slot = true;
            self.pos += 1;
        }
        Ok(flags)
    }

    fn group_name(&mut self, start: usize) -> Result<JsString, SyntaxError> {
        let mut name = String::new();
        while self.peek().is_some() && self.peek() != Some(62) {
            let cp = if self.eat(92) {
                if !self.eat(117) {
                    return Err(error(start, "invalid group name"));
                }
                self.unicode_escape(true)
                    .ok_or_else(|| error(start, "invalid group name"))?
            } else {
                self.raw_code_point()
            };
            let c = char::from_u32(cp).ok_or_else(|| error(start, "invalid group name"))?;
            if !(if name.is_empty() {
                is_ident_start(c)
            } else {
                is_ident_part(c)
            }) {
                return Err(error(start, "invalid group name"));
            }
            name.push(c);
        }
        if name.is_empty() || !self.eat(62) {
            return Err(error(start, "invalid group name"));
        }
        Ok(JsString::from(name.as_str()))
    }

    fn term(&mut self, mut id: NodeId) -> Result<(), SyntaxError> {
        let start = self.pos;
        if let Some((min, max)) = self.quantifier()? {
            if matches!(self.nodes[id.0], Node::Assertion(_))
                || matches!(self.nodes[id.0], Node::Lookaround { behind: true, .. })
                || self.flags.unicode_mode() && matches!(self.nodes[id.0], Node::Lookaround { .. })
            {
                return Err(error(start, "assertion cannot be quantified"));
            }
            let greedy = !self.eat(63);
            id = self.node(Node::Repeat {
                body: id,
                min,
                max,
                greedy,
            });
        }
        // A named reference placeholder is always the original atom, before
        // any Repeat wrapper is appended.
        self.frames.last_mut().unwrap().terms.push(id);
        Ok(())
    }

    fn quantifier(&mut self) -> Result<Option<(u64, Option<u64>)>, SyntaxError> {
        let start = self.pos;
        let simple = match self.peek() {
            Some(42) => Some((0, None)),
            Some(43) => Some((1, None)),
            Some(63) => Some((0, Some(1))),
            _ => None,
        };
        if simple.is_some() {
            self.pos += 1;
            return Ok(simple);
        }
        if !self.eat(123) {
            return Ok(None);
        }
        let min = self.count();
        let max = if self.eat(44) {
            if digit(self.peek()).is_some() {
                Some(self.count())
            } else {
                None
            }
        } else {
            Some(min)
        };
        if min.is_none() || !self.eat(125) {
            self.pos = start;
            return if self.flags.unicode_mode() {
                Err(error(start, "invalid quantifier"))
            } else {
                Ok(None)
            };
        }
        let min = min
            .unwrap()
            .ok_or_else(|| error(start, "quantifier count exceeds u64 limit"))?;
        let max = max
            .map(|n| {
                n.unwrap()
                    .ok_or_else(|| error(start, "quantifier count exceeds u64 limit"))
            })
            .transpose()?;
        if max.is_some_and(|max| min > max) {
            return Err(error(start, "quantifier range out of order"));
        }
        Ok(Some((min, max)))
    }

    // Outer None means no decimal digits, inner None means numeric overflow.
    fn count(&mut self) -> Option<Option<u64>> {
        let start = self.pos;
        let mut n = Some(0u64);
        while let Some(d) = digit(self.peek()) {
            self.pos += 1;
            n = n.and_then(|n| n.checked_mul(10)?.checked_add(u64::from(d)));
        }
        (self.pos != start).then_some(n)
    }

    fn raw_character(&mut self) -> u32 {
        if self.flags.unicode_mode() {
            self.raw_code_point()
        } else {
            let cp = u32::from(self.text[self.pos]);
            self.pos += 1;
            cp
        }
    }
    fn raw_code_point(&mut self) -> u32 {
        let high = self.text[self.pos];
        self.pos += 1;
        if (0xd800..=0xdbff).contains(&high)
            && let Some(&low @ 0xdc00..=0xdfff) = self.text.get(self.pos)
        {
            self.pos += 1;
            return pair(u32::from(high), u32::from(low));
        }
        u32::from(high)
    }

    fn escape_atom(&mut self) -> Result<Node, SyntaxError> {
        let start = self.pos;
        self.pos += 1;
        match self.peek() {
            Some(98) => {
                self.pos += 1;
                Ok(Node::Assertion(Assertion::WordBoundary))
            }
            Some(66) => {
                self.pos += 1;
                Ok(Node::Assertion(Assertion::NotWordBoundary))
            }
            Some(49..=57) => {
                let digits = self.pos;
                let value = self.count().unwrap();
                if let Some(n) = value.filter(|&n| n <= self.total_captures as u64) {
                    return Ok(Node::Backreference(vec![n as usize]));
                }
                if self.flags.unicode_mode() {
                    return Err(error(start, "invalid decimal backreference"));
                }
                self.pos = digits;
                Ok(Node::Literal(self.legacy_octal()))
            }
            Some(107) if self.flags.unicode_mode() || self.has_named => {
                self.pos += 1;
                if !self.eat(60) {
                    return Err(error(start, "invalid named backreference"));
                }
                let name = self.group_name(start)?;
                // The atom will be the next node emitted by run(). Resolve it
                // after parsing all groups, so forward names work as well.
                self.references
                    .push((NodeId(self.nodes.len()), name, start));
                Ok(Node::Backreference(Vec::new()))
            }
            _ => match self.character_escape(false, start)? {
                ClassMember::Literal(cp) => Ok(Node::Literal(cp)),
                member if self.flags.unicode_sets => {
                    Ok(Node::UnicodeSet(vec![ClassSetItem::Member(member)]))
                }
                member => Ok(Node::Class(CharacterClass {
                    inverted: false,
                    members: vec![member],
                })),
            },
        }
    }

    fn unicode_set(&mut self) -> Result<Vec<ClassSetItem>, SyntaxError> {
        let mut frames = Vec::<SetFrame>::new();
        let mut out = Vec::new();
        loop {
            if self.peek() == Some(91) && frames.last().is_none_or(|f| f.awaiting) {
                let start = self.pos;
                self.pos += 1;
                let inverted = self.eat(94);
                frames.push(SetFrame {
                    start,
                    inverted,
                    awaiting: true,
                    ..SetFrame::default()
                });
            }
            let frame = frames.last_mut().unwrap();
            if self.peek() == Some(93) {
                if frame.awaiting && frame.count != 0 {
                    return Err(error(self.pos, "missing class set operand"));
                }
                if frame.count == 0 {
                    out.push(ClassSetItem::Empty);
                }
                if frame.inverted {
                    if frame.may_strings {
                        return Err(error(frame.start, "negated class may contain strings"));
                    }
                    out.push(ClassSetItem::Complement);
                }
                self.pos += 1;
                let may_strings = frame.may_strings;
                frames.pop();
                if let Some(parent) = frames.last_mut() {
                    parent.operand(may_strings, false, &mut out);
                    continue;
                }
                return Ok(out);
            }
            if self.peek().is_none() {
                return Err(error(frame.start, "unterminated character class"));
            }
            if !frame.awaiting {
                let c = self.peek().unwrap();
                if matches!(c, 38 | 45) && self.text.get(self.pos + 1) == Some(&c) {
                    if c == 38 && self.text.get(self.pos + 2) == Some(&38)
                        || frame.range
                        || frame.operator.is_none() && frame.count != 1
                        || frame.operator.is_some_and(|op| op != c)
                    {
                        return Err(error(self.pos, "invalid class set operation"));
                    }
                    frame.operator = Some(c);
                    frame.awaiting = true;
                    self.pos += 2;
                    continue;
                }
                if frame.operator.is_some() {
                    return Err(error(self.pos, "expected class set operator"));
                }
                frame.awaiting = true;
            }
            // Opening a nested operand is handled at the top of the loop.
            if self.peek() == Some(91) {
                continue;
            }
            let start = self.pos;
            let mut range = false;
            let may_strings;
            if self.peek() == Some(92) && self.text.get(self.pos + 1) == Some(&113) {
                self.pos += 2;
                if !self.eat(123) {
                    return Err(error(start, "invalid class string"));
                }
                let mut count = 0;
                let mut may = false;
                loop {
                    let mut string = Vec::new();
                    while !matches!(self.peek(), None | Some(124 | 125)) {
                        let atom = self.set_character()?;
                        let ClassMember::Literal(cp) = atom else {
                            return Err(error(self.pos, "class escape in class string"));
                        };
                        string.push(cp);
                    }
                    may |= string.len() != 1;
                    out.push(ClassSetItem::String(string));
                    if count != 0 {
                        out.push(ClassSetItem::Union);
                    }
                    count += 1;
                    if self.eat(125) {
                        break;
                    }
                    if !self.eat(124) {
                        return Err(error(start, "unterminated class string"));
                    }
                }
                may_strings = may;
            } else {
                let mut member = self.set_character()?;
                may_strings = self.property_may_strings;
                if self.peek() == Some(45) && self.text.get(self.pos + 1) != Some(&45) {
                    self.pos += 1;
                    let right = self.set_character()?;
                    let (ClassMember::Literal(lo), ClassMember::Literal(hi)) = (member, right)
                    else {
                        return Err(error(start, "class escape cannot be a range endpoint"));
                    };
                    if lo > hi {
                        return Err(error(start, "class range out of order"));
                    }
                    member = ClassMember::Range(lo, hi);
                    range = true;
                }
                out.push(ClassSetItem::Member(member));
            }
            let frame = frames.last_mut().unwrap();
            if range && frame.operator.is_some() {
                return Err(error(start, "range is not a class set operand"));
            }
            frame.operand(may_strings, range, &mut out);
        }
    }

    fn set_character(&mut self) -> Result<ClassMember, SyntaxError> {
        self.property_may_strings = false;
        let start = self.pos;
        let c = self
            .peek()
            .ok_or_else(|| error(start, "missing class character"))?;
        if self.eat(92) {
            return self.character_escape(true, start);
        }
        if matches!(c, 40 | 41 | 91 | 93 | 123 | 125 | 47 | 45 | 124)
            || reserved_set_punctuator(c) && self.text.get(self.pos + 1) == Some(&c)
        {
            return Err(error(start, "unescaped class set syntax character"));
        }
        Ok(ClassMember::Literal(self.raw_character()))
    }

    fn character_class(&mut self) -> Result<CharacterClass, SyntaxError> {
        let start = self.pos;
        self.pos += 1;
        let inverted = self.eat(94);
        let mut members = Vec::new();
        while self.peek() != Some(93) {
            if self.peek().is_none() {
                return Err(error(start, "unterminated character class"));
            }
            let left = self.class_atom()?;
            if self.peek() == Some(45) && self.text.get(self.pos + 1) != Some(&93) {
                let dash = self.pos;
                self.pos += 1;
                if self.peek().is_none() {
                    return Err(error(start, "unterminated character class"));
                }
                let right = self.class_atom()?;
                if let (ClassMember::Literal(lo), ClassMember::Literal(hi)) = (&left, &right) {
                    if lo > hi {
                        return Err(error(dash, "class range out of order"));
                    }
                    members.push(ClassMember::Range(*lo, *hi));
                } else if self.flags.unicode_mode() {
                    return Err(error(dash, "class escape cannot be a range endpoint"));
                } else {
                    members.extend([left, ClassMember::Literal(45), right]);
                }
            } else {
                members.push(left);
            }
        }
        self.pos += 1;
        Ok(CharacterClass { inverted, members })
    }

    fn class_atom(&mut self) -> Result<ClassMember, SyntaxError> {
        let start = self.pos;
        if self.eat(92) {
            self.character_escape(true, start)
        } else {
            Ok(ClassMember::Literal(self.raw_character()))
        }
    }

    // Entry position is immediately after the backslash. On an Annex B
    // fallback only the identity letter is consumed, except invalid \c,
    // which emits a backslash and leaves c to be parsed as the next atom.
    fn character_escape(
        &mut self,
        in_class: bool,
        start: usize,
    ) -> Result<ClassMember, SyntaxError> {
        let unit = self
            .peek()
            .ok_or_else(|| error(start, "trailing backslash"))?;
        self.pos += 1;
        let set = match unit {
            100 => Some(ClassEscape::Digit),
            68 => Some(ClassEscape::NotDigit),
            119 => Some(ClassEscape::Word),
            87 => Some(ClassEscape::NotWord),
            115 => Some(ClassEscape::Space),
            83 => Some(ClassEscape::NotSpace),
            _ => None,
        };
        if let Some(set) = set {
            return Ok(ClassMember::Escape(set));
        }
        let cp = match unit {
            116 => 9,
            110 => 10,
            118 => 11,
            102 => 12,
            114 => 13,
            98 if in_class => 8,
            99 => {
                let next = self.peek();
                if next.is_some_and(|n| {
                    matches!(n,65..=90 | 97..=122)
                        || in_class && !self.flags.unicode_mode() && matches!(n, 48..=57 | 95)
                }) {
                    self.pos += 1;
                    u32::from(next.unwrap() & 31)
                } else if self.flags.unicode_mode() {
                    return Err(error(start, "invalid control escape"));
                } else {
                    self.pos -= 1;
                    92
                }
            }
            120 => {
                if let Some(cp) = self.hex(2) {
                    cp
                } else if self.flags.unicode_mode() {
                    return Err(error(start, "invalid hexadecimal escape"));
                } else {
                    120
                }
            }
            117 => {
                let saved = self.pos;
                if let Some(cp) = self.unicode_escape(self.flags.unicode_mode()) {
                    cp
                } else if self.flags.unicode_mode() {
                    return Err(error(start, "invalid Unicode escape"));
                } else {
                    self.pos = saved;
                    117
                }
            }
            48..=57 => {
                if self.flags.unicode_mode() {
                    if unit != 48 || digit(self.peek()).is_some() {
                        return Err(error(start, "invalid decimal escape"));
                    }
                    0
                } else {
                    self.pos -= 1;
                    self.legacy_octal()
                }
            }
            112 | 80 if self.flags.unicode_mode() => {
                if !self.eat(123) {
                    return Err(error(start, "invalid Unicode property escape"));
                }
                let mut name = String::new();
                while let Some(c @ (48..=57 | 65..=90 | 97..=122 | 95 | 61)) = self.peek() {
                    name.push(char::from_u32(u32::from(c)).unwrap());
                    self.pos += 1;
                }
                if !self.eat(125) {
                    return Err(error(start, "invalid Unicode property escape"));
                }
                if self.syntax_only {
                    let kind = super::property::syntax_kind(&name)
                        .ok_or_else(|| error(start, "unknown Unicode property"))?;
                    self.property_may_strings = kind == super::property::PropertyKind::String;
                    if self.property_may_strings && (!self.flags.unicode_sets || unit == 80) {
                        return Err(error(start, "invalid string property escape"));
                    }
                    // A nonliteral placeholder keeps range-endpoint checks
                    // intact. Only validate_syntax uses and discards this arena.
                    return Ok(ClassMember::Escape(ClassEscape::Digit));
                }
                let property = super::property::lookup(&name)
                    .ok_or_else(|| error(start, "unknown or unsupported Unicode property"))?;
                return Ok(ClassMember::Property {
                    property,
                    inverted: unit == 80,
                });
            }
            _ if !self.flags.unicode_mode()
                || matches!(
                    unit,
                    94 | 36 | 92 | 46 | 42 | 43 | 63 | 40 | 41 | 91 | 93 | 123 | 125 | 124 | 47
                )
                || in_class
                    && (unit == 45 || self.flags.unicode_sets && reserved_set_punctuator(unit)) =>
            {
                u32::from(unit)
            }
            _ => return Err(error(start, "invalid identity escape in u mode")),
        };
        Ok(ClassMember::Literal(cp))
    }

    fn legacy_octal(&mut self) -> u32 {
        let first = self.text[self.pos];
        self.pos += 1;
        if first >= 56 {
            return u32::from(first);
        }
        let mut cp = u32::from(first - 48);
        let remaining = if first <= 51 { 2 } else { 1 };
        for _ in 0..remaining {
            if let Some(unit @ 48..=55) = self.peek() {
                self.pos += 1;
                cp = cp * 8 + u32::from(unit - 48);
            } else {
                break;
            }
        }
        cp
    }

    /// Parse after u. With Unicode enabled, allow braces and combine a pair
    /// of fixed-width surrogate escapes. Never combine raw/escaped mixtures.
    fn unicode_escape(&mut self, unicode: bool) -> Option<u32> {
        if unicode && self.eat(123) {
            let start = self.pos;
            let mut cp = 0u32;
            while let Some(d) = self.peek().and_then(hex_digit) {
                self.pos += 1;
                cp = cp.checked_mul(16)?.checked_add(d)?;
            }
            if self.pos == start || !self.eat(125) || cp > 0x10ffff {
                return None;
            }
            return Some(cp);
        }
        let high = self.hex(4)?;
        if unicode && (0xd800..=0xdbff).contains(&high) {
            let saved = self.pos;
            if self.eat(92)
                && self.eat(117)
                && let Some(low @ 0xdc00..=0xdfff) = self.hex(4)
            {
                return Some(pair(high, low));
            }
            self.pos = saved;
        }
        Some(high)
    }

    fn hex(&mut self, len: usize) -> Option<u32> {
        let mut cp = 0;
        for &unit in self.text.get(self.pos..self.pos + len)? {
            cp = cp * 16 + hex_digit(unit)?;
        }
        self.pos += len;
        Some(cp)
    }
}

fn digit(unit: Option<u16>) -> Option<u32> {
    match unit {
        Some(n @ 48..=57) => Some(u32::from(n - 48)),
        _ => None,
    }
}
fn hex_digit(unit: u16) -> Option<u32> {
    match unit {
        48..=57 => Some(u32::from(unit - 48)),
        65..=70 => Some(u32::from(unit - 65 + 10)),
        97..=102 => Some(u32::from(unit - 97 + 10)),
        _ => None,
    }
}
fn pair(high: u32, low: u32) -> u32 {
    0x10000 + ((high - 0xd800) << 10) + low - 0xdc00
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(text: &str, flags: &str) -> Result<Pattern, SyntaxError> {
        parse(&JsString::from(text), &JsString::from(flags))
    }

    fn terms(p: &Pattern) -> Vec<&Node> {
        let Node::Disjunction(alts) = &p.nodes[p.root.0] else {
            panic!()
        };
        assert_eq!(alts.len(), 1);
        let Node::Alternative(terms) = &p.nodes[alts[0].0] else {
            panic!()
        };
        terms.iter().map(|id| &p.nodes[id.0]).collect()
    }

    // Adapted parse-level patterns from QuickJS tests/test_builtin.js::test_regexp.
    // Copyright (c) 2017-2021 Fabrice Bellard
    // Copyright (c) 2017-2021 Charlie Gordon
    //
    // Permission is hereby granted, free of charge, to any person obtaining a copy
    // of this software and associated documentation files (the "Software"), to deal
    // in the Software without restriction, including without limitation the rights
    // to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
    // copies of the Software, and to permit persons to whom the Software is
    // furnished to do so, subject to the following conditions:
    //
    // The above copyright notice and this permission notice shall be included in
    // all copies or substantial portions of the Software.
    //
    // THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
    // IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
    // FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
    // THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
    // LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
    // OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
    // THE SOFTWARE.
    #[test]
    fn annex_b_and_escapes() {
        for (source, expected) in [
            (r"\x61", vec![97]),
            (r"\u0061", vec![97]),
            (r"\ca", vec![1]),
            (r"\\a", vec![92, 97]),
            (r"\c0", vec![92, 99, 48]),
            ("\0a", vec![0, 97]),
            (r"{1a}", vec![123, 49, 97, 125]),
        ] {
            let p = pattern(source, "").unwrap();
            assert_eq!(
                terms(&p),
                expected
                    .into_iter()
                    .map(Node::Literal)
                    .collect::<Vec<_>>()
                    .iter()
                    .collect::<Vec<_>>(),
                "{source}"
            );
        }
        let p = pattern("a{1+", "").unwrap();
        let t = terms(&p);
        assert_eq!(t[..2], [&Node::Literal(97), &Node::Literal(123)]);
        let Node::Repeat {
            body,
            min: 1,
            max: None,
            greedy: true,
        } = t[2]
        else {
            panic!("{t:?}")
        };
        assert_eq!(p.nodes[body.0], Node::Literal(49));
        for (text, expected) in [
            (r"[\q{a\b}]", vec![113, 123, 97, 8, 125]),
            (r"[\b]", vec![8]),
        ] {
            let p = pattern(text, "").unwrap();
            assert_eq!(
                terms(&p),
                [&Node::Class(CharacterClass {
                    inverted: false,
                    members: expected.into_iter().map(ClassMember::Literal).collect()
                })]
            );
        }
    }

    #[test]
    fn syntax_errors() {
        for (source, flags, offset, message) in [
            ("(", "", 0, "unterminated group"),
            ("a**", "", 2, "nothing to repeat"),
            ("a{2,1}", "", 1, "quantifier range out of order"),
            ("[b-a]", "", 2, "class range out of order"),
            (r"\u{110000}", "u", 0, "invalid Unicode escape"),
            ("(?<n>a)(?<n>b)", "", 7, "duplicate group name"),
            ("]", "u", 0, "unexpected syntax character"),
            ("{1a}", "u", 0, "invalid quantifier"),
            (r"\c0", "u", 0, "invalid control escape"),
            ("", "gg", 1, "duplicate regular expression flag"),
            ("", "x", 0, "unknown regular expression flag"),
            ("", "uv", 1, "u and v flags cannot be combined"),
        ] {
            let err = pattern(source, flags).unwrap_err();
            assert_eq!(
                (err.offset, err.message.as_str()),
                (offset, message),
                "{source:?} {flags}"
            );
        }
        for (source, flags) in [(r"(?<n>a)\k<n>", "u"), (r"[\]]", "u"), ("]", "")] {
            pattern(source, flags).unwrap();
        }
    }

    #[test]
    fn alternatives_groups_and_lookarounds() {
        let p = pattern("^a|(?:b)(c)(?=d)(?!e)(?<=f)(?<!g)$\\b\\B", "").unwrap();
        assert_eq!(p.captures, [None]);
        let Node::Disjunction(alts) = &p.nodes[p.root.0] else {
            panic!()
        };
        assert_eq!(alts.len(), 2);
        let Node::Alternative(first) = &p.nodes[alts[0].0] else {
            panic!()
        };
        assert_eq!(p.nodes[first[0].0], Node::Assertion(Assertion::Start));
        let Node::Alternative(second) = &p.nodes[alts[1].0] else {
            panic!()
        };
        assert!(matches!(
            p.nodes[second[0].0],
            Node::Group { capture: None, .. }
        ));
        assert!(matches!(
            p.nodes[second[1].0],
            Node::Group {
                capture: Some(1),
                ..
            }
        ));
        for (id, (behind, negative)) in
            second[2..6]
                .iter()
                .zip([(false, false), (false, true), (true, false), (true, true)])
        {
            assert!(
                matches!(p.nodes[id.0], Node::Lookaround { behind: b, negative: n, .. } if b == behind && n == negative)
            );
        }
        assert_eq!(p.nodes[second[6].0], Node::Assertion(Assertion::End));
        assert_eq!(
            p.nodes[second[7].0],
            Node::Assertion(Assertion::WordBoundary)
        );
        assert_eq!(
            p.nodes[second[8].0],
            Node::Assertion(Assertion::NotWordBoundary)
        );
        let p = pattern("|a||", "u").unwrap();
        let Node::Disjunction(alts) = &p.nodes[p.root.0] else {
            panic!()
        };
        assert_eq!(alts.len(), 4);
        assert_eq!(p.nodes[alts[0].0], Node::Alternative(vec![]));
    }

    #[test]
    fn quantifiers_and_assertion_restrictions() {
        for (source, min, max, greedy) in [
            ("a*", 0, None, true),
            ("a+?", 1, None, false),
            ("a?", 0, Some(1), true),
            ("a{0}", 0, Some(0), true),
            ("a{12,}", 12, None, true),
            ("a{2,10}?", 2, Some(10), false),
            ("a{0002}", 2, Some(2), true),
        ] {
            let p = pattern(source, "u").unwrap();
            let [
                Node::Repeat {
                    body,
                    min: actual_min,
                    max: actual_max,
                    greedy: actual_greedy,
                },
            ] = terms(&p).as_slice()
            else {
                panic!("{source}")
            };
            assert_eq!(
                (*actual_min, *actual_max, *actual_greedy),
                (min, max, greedy)
            );
            assert_eq!(p.nodes[body.0], Node::Literal(97));
        }
        pattern("(?=a)+", "").unwrap();
        for (source, flags) in [
            ("(?=a)+", "u"),
            ("(?<=a)+", ""),
            ("^*", ""),
            (r"\b?", ""),
            ("a{1,2}{3}", ""),
            ("{2}", ""),
            ("a{1,2", "u"),
        ] {
            assert!(pattern(source, flags).is_err(), "{source}");
        }
        assert_eq!(
            pattern("a{18446744073709551616}", "").unwrap_err().message,
            "quantifier count exceeds u64 limit"
        );
    }

    #[test]
    fn forward_references_and_legacy_decimal_escapes() {
        for flags in ["", "u"] {
            let p = pattern(r"\2(a)(b)\1", flags).unwrap();
            assert_eq!(terms(&p)[0], &Node::Backreference(vec![2]));
            assert_eq!(terms(&p)[3], &Node::Backreference(vec![1]));
            let p = pattern(r"\k<n>(?<n>a)\k<\u006e>", flags).unwrap();
            assert_eq!(p.captures, [Some(JsString::from("n"))]);
            assert_eq!(terms(&p)[0], &Node::Backreference(vec![1]));
            assert_eq!(terms(&p)[2], &Node::Backreference(vec![1]));
        }
        for (source, expected) in [
            (r"\1", vec![1]),
            (r"\18", vec![1, 56]),
            (r"\400", vec![32, 48]),
            (r"\377", vec![255]),
            (r"\8\9", vec![56, 57]),
            (r"\08", vec![0, 56]),
            (r"\k<a>", vec![107, 60, 97, 62]),
        ] {
            let p = pattern(source, "").unwrap();
            assert_eq!(
                terms(&p),
                expected
                    .into_iter()
                    .map(Node::Literal)
                    .collect::<Vec<_>>()
                    .iter()
                    .collect::<Vec<_>>(),
                "{source}"
            );
        }
        // Parentheses in classes/escapes never increase the capture count.
        assert_eq!(
            terms(&pattern(r"\1[(]\(", "").unwrap())[0],
            &Node::Literal(1)
        );
        for (source, flags) in [
            (r"\1", "u"),
            (r"\2(a)", "u"),
            (r"\k<x>", "u"),
            (r"(?<n>a)\k<x>", ""),
            (r"(?<n>a)\k", ""),
            (r"(?<1>a)", ""),
            (r"(?<a-b>a)", "u"),
        ] {
            assert!(pattern(source, flags).is_err(), "{source}");
        }
        let p = pattern(r"(?<\u{10400}>a)\k<𐐀>", "").unwrap();
        assert_eq!(p.captures, [Some(JsString::from("𐐀"))]);
        assert_eq!(terms(&p)[1], &Node::Backreference(vec![1]));
    }

    #[test]
    fn unicode_code_points_and_lone_surrogates() {
        for source in [r"\uD83D\uDE00", r"\u{1F600}", "😀"] {
            let p = pattern(source, "u").unwrap();
            assert_eq!(terms(&p), [&Node::Literal(0x1f600)]);
        }
        for source in [r"\uD83D\uDE00", "😀"] {
            let p = pattern(source, "").unwrap();
            assert_eq!(terms(&p), [&Node::Literal(0xd83d), &Node::Literal(0xde00)]);
        }
        for flags in ["", "u"] {
            let raw = JsString::from_code_units(vec![0xd800, 0, 0xdc00]).unwrap();
            let p = parse(&raw, &JsString::from(flags)).unwrap();
            assert_eq!(
                terms(&p),
                [
                    &Node::Literal(0xd800),
                    &Node::Literal(0),
                    &Node::Literal(0xdc00)
                ]
            );
            assert_eq!(
                terms(&pattern(r"\uD800", flags).unwrap()),
                [&Node::Literal(0xd800)]
            );
        }
        let mixed = JsString::from_code_units(vec![0xd83d, 92, 117, 100, 101, 48, 48]).unwrap();
        assert_eq!(
            terms(&parse(&mixed, &JsString::from("u")).unwrap()),
            [&Node::Literal(0xd83d), &Node::Literal(0xde00)]
        );
        // The offset is measured in code units rather than Unicode scalars.
        assert_eq!(pattern("😀]", "u").unwrap_err().offset, 2);
        for source in [
            r"\q",
            r"\-",
            r"\xG0",
            r"\u123",
            r"\u{}",
            r"\u{FFFFFFFFF}",
            r"\01",
        ] {
            assert!(pattern(source, "u").is_err(), "{source}");
        }
        let p = pattern(r"\xG0\uZ", "").unwrap();
        assert_eq!(
            terms(&p),
            [120, 71, 48, 117, 90]
                .map(Node::Literal)
                .iter()
                .collect::<Vec<_>>()
        );
        assert_eq!(
            terms(&pattern(r"\t\n\v\f\r\0", "u").unwrap()),
            [9, 10, 11, 12, 13, 0]
                .map(Node::Literal)
                .iter()
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn character_classes_and_ranges() {
        let p = pattern(r"[^a-z\d\D\w\W\s\S\-]", "u").unwrap();
        assert_eq!(
            terms(&p),
            [&Node::Class(CharacterClass {
                inverted: true,
                members: vec![
                    ClassMember::Range(97, 122),
                    ClassMember::Escape(ClassEscape::Digit),
                    ClassMember::Escape(ClassEscape::NotDigit),
                    ClassMember::Escape(ClassEscape::Word),
                    ClassMember::Escape(ClassEscape::NotWord),
                    ClassMember::Escape(ClassEscape::Space),
                    ClassMember::Escape(ClassEscape::NotSpace),
                    ClassMember::Literal(45),
                ]
            })]
        );
        for source in [r"[]", r"[^]", r"[-a]", r"[a-]", r"[\uD83D\uDE00-\u{1F601}]"] {
            pattern(source, "u").unwrap();
        }
        for source in [r"[\d-a]", r"[a-\w]", r"[\1]", r"[\B]", r"[\k<n>]"] {
            assert!(pattern(source, "u").is_err(), "{source}");
            pattern(source, "").unwrap();
        }
        let p = pattern(r"[\c0\c_\1\8]", "").unwrap();
        assert_eq!(
            terms(&p),
            [&Node::Class(CharacterClass {
                inverted: false,
                members: [16, 31, 1, 56].map(ClassMember::Literal).to_vec()
            })]
        );
        let p = pattern(r"[a-\d]", "").unwrap();
        assert_eq!(
            terms(&p),
            [&Node::Class(CharacterClass {
                inverted: false,
                members: vec![
                    ClassMember::Literal(97),
                    ClassMember::Literal(45),
                    ClassMember::Escape(ClassEscape::Digit)
                ]
            })]
        );
    }

    #[test]
    fn flags_modifiers_and_deferred_syntax() {
        let f = Flags::parse(&JsString::from("dgimsuy")).unwrap();
        assert!(
            f.has_indices
                && f.global
                && f.ignore_case
                && f.multiline
                && f.dot_all
                && f.unicode
                && f.sticky
        );
        assert!(Flags::parse(&JsString::from("v")).unwrap().unicode_sets);
        assert!(pattern("[a]", "v").is_ok());
        assert!(pattern(r"\p{ASCII}", "u").is_ok());
        assert_eq!(pattern(r"\p{sc=Greek}", "u").unwrap_err().offset, 0);
        let p = pattern("(?im-s:a)", "").unwrap();
        let [
            Node::Modifiers {
                enable, disable, ..
            },
        ] = terms(&p).as_slice()
        else {
            panic!()
        };
        assert_eq!(
            *enable,
            Modifiers {
                ignore_case: true,
                multiline: true,
                dot_all: false
            }
        );
        assert_eq!(
            *disable,
            Modifiers {
                dot_all: true,
                ..Modifiers::default()
            }
        );
        for flags in ["", "u"] {
            let p = pattern("(?i-:a)", flags).unwrap();
            let [
                Node::Modifiers {
                    enable, disable, ..
                },
            ] = terms(&p).as_slice()
            else {
                panic!()
            };
            assert_eq!(
                *enable,
                Modifiers {
                    ignore_case: true,
                    ..Modifiers::default()
                }
            );
            assert_eq!(*disable, Modifiers::default());
        }
        for source in ["(?ii:a)", "(?i-i:a)", "(?-:a)", "(?x:a)", "(?i)", "(?a)"] {
            assert!(pattern(source, "").is_err(), "{source}");
        }
    }

    #[test]
    fn duplicate_names_in_disjoint_alternatives() {
        for flags in ["", "u"] {
            for source in [
                r"(?:(?<n>a)|(?<n>b))\k<n>",
                r"\k<n>(?:(?<n>a)|(?:(?<n>b)|(?<n>c)))",
                r"(?:(?<n>a)|(?<n>b))+\k<n>",
            ] {
                let p = pattern(source, flags).unwrap();
                assert!(
                    p.captures
                        .iter()
                        .all(|name| name.as_ref() == Some(&JsString::from("n")))
                );
                let references: Vec<_> = p
                    .nodes
                    .iter()
                    .filter_map(|node| match node {
                        Node::Backreference(indices) => Some(indices),
                        _ => None,
                    })
                    .collect();
                let expected: Vec<_> = (1..=p.captures.len()).collect();
                assert_eq!(references, [&expected]);
            }
            for source in [
                r"(?<n>a)(?<n>b)",
                r"(?<n>(?<n>a)|b)",
                r"(?:(?<n>a)|b)(?:c|(?<n>d))",
                r"(?:(?<n>a)|(?<n>b))(?<n>c)",
                r"(?:(?<n>a)(?<n>b)|c)",
                r"(?<n>a)(?<\u006e>b)",
            ] {
                assert_eq!(
                    pattern(source, flags).unwrap_err().message,
                    "duplicate group name",
                    "{source}"
                );
            }
            let p = pattern(r"(?:(?<n>a)|(?<n>b))\1\k<n>+", flags).unwrap();
            let t = terms(&p);
            assert_eq!(t[1], &Node::Backreference(vec![1]));
            let Node::Repeat {
                body,
                min: 1,
                max: None,
                ..
            } = t[2]
            else {
                panic!()
            };
            assert_eq!(p.nodes[body.0], Node::Backreference(vec![1, 2]));
            let p = pattern(r"(?:x(?<a>a)|(?<b>b)(?<a>c))\k<a>\k<b>", flags).unwrap();
            let t = terms(&p);
            assert_eq!(t[1], &Node::Backreference(vec![1, 3]));
            assert_eq!(t[2], &Node::Backreference(vec![2]));
        }
    }

    fn branch(p: &Pattern, disjunction: NodeId, index: usize) -> &[NodeId] {
        let Node::Disjunction(alts) = &p.nodes[disjunction.0] else {
            panic!()
        };
        let Node::Alternative(terms) = &p.nodes[alts[index].0] else {
            panic!()
        };
        terms
    }

    #[test]
    fn nested_ir_preserves_atoms_and_branch_contents() {
        for flags in ["", "u"] {
            let p = pattern(r"a(?:b.|c(d|e))(?=f|g)(?i:h|i)j", flags).unwrap();
            let t = terms(&p);
            assert_eq!(t.len(), 5);
            assert_eq!(t[0], &Node::Literal(97));
            assert_eq!(t[4], &Node::Literal(106));
            let Node::Group {
                body,
                capture: None,
            } = t[1]
            else {
                panic!()
            };
            let first = branch(&p, *body, 0);
            assert_eq!(first.len(), 2);
            assert_eq!(p.nodes[first[0].0], Node::Literal(98));
            assert_eq!(p.nodes[first[1].0], Node::Dot);
            let second = branch(&p, *body, 1);
            assert_eq!(second.len(), 2);
            assert_eq!(p.nodes[second[0].0], Node::Literal(99));
            let Node::Group {
                body: capture_body,
                capture: Some(1),
            } = p.nodes[second[1].0]
            else {
                panic!()
            };
            for (index, cp) in [100, 101].into_iter().enumerate() {
                let b = branch(&p, capture_body, index);
                assert_eq!(b.len(), 1);
                assert_eq!(p.nodes[b[0].0], Node::Literal(cp));
            }
            assert_eq!(p.captures, [None]);
            let Node::Lookaround {
                body,
                behind: false,
                negative: false,
            } = t[2]
            else {
                panic!()
            };
            for (index, cp) in [102, 103].into_iter().enumerate() {
                let b = branch(&p, *body, index);
                assert_eq!(b.len(), 1);
                assert_eq!(p.nodes[b[0].0], Node::Literal(cp));
            }
            let Node::Modifiers {
                body,
                enable,
                disable,
            } = t[3]
            else {
                panic!()
            };
            assert_eq!(
                *enable,
                Modifiers {
                    ignore_case: true,
                    ..Modifiers::default()
                }
            );
            assert_eq!(*disable, Modifiers::default());
            for (index, cp) in [104, 105].into_iter().enumerate() {
                let b = branch(&p, *body, index);
                assert_eq!(b.len(), 1);
                assert_eq!(p.nodes[b[0].0], Node::Literal(cp));
            }
            let p = pattern(r"\. [.]", flags).unwrap();
            assert_eq!(
                terms(&p),
                [
                    &Node::Literal(46),
                    &Node::Literal(32),
                    &Node::Class(CharacterClass {
                        inverted: false,
                        members: vec![ClassMember::Literal(46)]
                    })
                ]
            );
        }
    }

    #[test]
    fn nested_patterns_use_an_arena() {
        let source = format!("{}a{}", "(?:".repeat(20_000), ")".repeat(20_000));
        let p = pattern(&source, "u").unwrap();
        assert_eq!(p.nodes.len(), 60_003);
        // Every edge goes backwards, including captures and repetitions.
        for (index, node) in p.nodes.iter().enumerate() {
            match node {
                Node::Alternative(ids) | Node::Disjunction(ids) => {
                    assert!(ids.iter().all(|id| id.0 < index))
                }
                Node::Group { body, .. }
                | Node::Modifiers { body, .. }
                | Node::Lookaround { body, .. }
                | Node::Repeat { body, .. } => assert!(body.0 < index),
                _ => {}
            }
        }
        drop(p);
        for source in ["\\", "[", "[a-", ")", "(?", "(?<", "(?<=", "((a)"] {
            assert!(pattern(source, "").is_err(), "{source}");
        }
    }
}
