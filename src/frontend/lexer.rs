//! On-demand tokenizer for the documented JavaScript subset
//! (`docs/grammar.md` §Lexical grammar).
//!
//! The lexer recognizes every ECMAScript punctuator by maximal munch, so that
//! an operator outside the subset (`**`, `>>>=`, `?.`, `=>`, ...) arrives at
//! the parser as one token and is rejected there with a precise diagnostic.
//! A `/` is always lexed as the division punctuator: the parser decides from
//! context whether a regular expression literal would have started, and
//! rejects it. Template literals, BigInt literals, numeric separators,
//! non-ASCII identifiers and identifier escapes are rejected here.

use super::{Span, SyntaxError, SyntaxErrorKind};

/// A lexical token.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    /// A line terminator (possibly inside a comment) separates this token
    /// from the previous one. Drives automatic semicolon insertion.
    pub newline_before: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Identifier(String),
    Keyword(Keyword),
    /// A numeric literal's value.
    Number(f64),
    /// A string literal's cooked value.
    String(String),
    Punct(Punct),
    Eof,
}

impl TokenKind {
    /// Short human-readable description for diagnostics.
    pub fn describe(&self) -> String {
        match self {
            TokenKind::Identifier(name) => format!("identifier `{name}`"),
            TokenKind::Keyword(k) => format!("keyword `{}`", k.as_str()),
            TokenKind::Number(_) => "number literal".to_string(),
            TokenKind::String(_) => "string literal".to_string(),
            TokenKind::Punct(p) => format!("`{}`", p.as_str()),
            TokenKind::Eof => "end of input".to_string(),
        }
    }
}

macro_rules! keywords {
    ($($variant:ident = $text:literal,)*) => {
        /// ECMAScript reserved words, including the strict-mode reserved
        /// words (`let`, `static`, `implements`, ...) and `await`, which this
        /// subset always reserves. All are rejected as identifiers.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Keyword { $($variant,)* }

        impl Keyword {
            pub fn as_str(self) -> &'static str {
                match self { $(Keyword::$variant => $text,)* }
            }

            pub fn from_text(text: &str) -> Option<Keyword> {
                match text { $($text => Some(Keyword::$variant),)* _ => None }
            }
        }
    };
}

keywords! {
    Await = "await",
    Break = "break",
    Case = "case",
    Catch = "catch",
    Class = "class",
    Const = "const",
    Continue = "continue",
    Debugger = "debugger",
    Default = "default",
    Delete = "delete",
    Do = "do",
    Else = "else",
    Enum = "enum",
    Export = "export",
    Extends = "extends",
    False = "false",
    Finally = "finally",
    For = "for",
    Function = "function",
    If = "if",
    Implements = "implements",
    Import = "import",
    In = "in",
    Instanceof = "instanceof",
    Interface = "interface",
    Let = "let",
    New = "new",
    Null = "null",
    Package = "package",
    Private = "private",
    Protected = "protected",
    Public = "public",
    Return = "return",
    Static = "static",
    Super = "super",
    Switch = "switch",
    This = "this",
    Throw = "throw",
    True = "true",
    Try = "try",
    Typeof = "typeof",
    Var = "var",
    Void = "void",
    While = "while",
    With = "with",
    Yield = "yield",
}

macro_rules! puncts {
    ($($variant:ident = $text:literal,)*) => {
        /// Every ECMAScript punctuator. Most are outside the subset; they are
        /// lexed so that the parser can name them in its diagnostics.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Punct { $($variant,)* }

        impl Punct {
            pub fn as_str(self) -> &'static str {
                match self { $(Punct::$variant => $text,)* }
            }
        }

        /// Longest first, for maximal munch.
        const PUNCTS: &[(&str, Punct)] = &[$(($text, Punct::$variant),)*];
    };
}

puncts! {
    UShrAssign = ">>>=",
    Ellipsis = "...",
    StrictEq = "===",
    StrictNotEq = "!==",
    StarStarAssign = "**=",
    ShlAssign = "<<=",
    ShrAssign = ">>=",
    UShr = ">>>",
    AmpAmpAssign = "&&=",
    PipePipeAssign = "||=",
    NullishAssign = "??=",
    Arrow = "=>",
    Eq = "==",
    NotEq = "!=",
    Le = "<=",
    Ge = ">=",
    AmpAmp = "&&",
    PipePipe = "||",
    Nullish = "??",
    OptionalChain = "?.",
    PlusPlus = "++",
    MinusMinus = "--",
    StarStar = "**",
    Shl = "<<",
    Shr = ">>",
    PlusAssign = "+=",
    MinusAssign = "-=",
    StarAssign = "*=",
    SlashAssign = "/=",
    PercentAssign = "%=",
    AmpAssign = "&=",
    PipeAssign = "|=",
    CaretAssign = "^=",
    LBrace = "{",
    RBrace = "}",
    LParen = "(",
    RParen = ")",
    LBracket = "[",
    RBracket = "]",
    Semicolon = ";",
    Comma = ",",
    Dot = ".",
    Question = "?",
    Colon = ":",
    Assign = "=",
    Lt = "<",
    Gt = ">",
    Plus = "+",
    Minus = "-",
    Star = "*",
    Slash = "/",
    Percent = "%",
    Bang = "!",
    Tilde = "~",
    Amp = "&",
    Pipe = "|",
    Caret = "^",
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// WhiteSpace (ECMAScript §12.2): TAB, VT, FF, ZWNBSP and Unicode `Zs`.
fn is_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\u{0B}' | '\u{0C}' | ' ' | '\u{A0}' | '\u{FEFF}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '$' || c == '_'
}

fn is_ident_part(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '$' || c == '_'
}

/// The tokenizer. Cloning it is cheap and gives an independent cursor, which
/// the parser uses for bounded lookahead.
#[derive(Clone, Debug)]
pub struct Lexer<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Lexer<'a> {
        Lexer { src, pos: 0 }
    }

    pub fn source(&self) -> &'a str {
        self.src
    }

    fn peek_char(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn peek_char_at(&self, n: usize) -> Option<char> {
        self.src[self.pos..].chars().nth(n)
    }

    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek_char()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn error(&self, kind: SyntaxErrorKind, span: Span, message: impl Into<String>) -> SyntaxError {
        SyntaxError::new(self.src, kind, span, message)
    }

    /// Error spanning the single character at `pos` (or empty at the end).
    fn error_at(
        &self,
        pos: usize,
        kind: SyntaxErrorKind,
        message: impl Into<String>,
    ) -> SyntaxError {
        let end = self.src[pos..]
            .chars()
            .next()
            .map_or(pos, |c| pos + c.len_utf8());
        self.error(kind, Span::new(pos, end), message)
    }

    /// Skip whitespace and comments; report whether a line terminator was
    /// crossed.
    fn skip_trivia(&mut self) -> Result<bool, SyntaxError> {
        let mut newline = false;
        loop {
            let Some(c) = self.peek_char() else {
                return Ok(newline);
            };
            if is_whitespace(c) {
                self.bump();
            } else if is_line_terminator(c) {
                newline = true;
                self.bump();
            } else if self.rest().starts_with("//") {
                while let Some(c) = self.peek_char() {
                    if is_line_terminator(c) {
                        break;
                    }
                    self.bump();
                }
            } else if self.rest().starts_with("/*") {
                let start = self.pos;
                self.pos += 2;
                match self.rest().find("*/") {
                    Some(len) => {
                        let body = &self.rest()[..len];
                        if body.chars().any(is_line_terminator) {
                            newline = true;
                        }
                        self.pos += len + 2;
                    }
                    None => {
                        return Err(self.error(
                            SyntaxErrorKind::UnexpectedEnd,
                            Span::new(start, start + 2),
                            "unterminated block comment",
                        ));
                    }
                }
            } else {
                return Ok(newline);
            }
        }
    }

    /// Lex the next token. At the end of input this returns an `Eof` token
    /// (repeatedly).
    pub fn next_token(&mut self) -> Result<Token, SyntaxError> {
        let newline_before = self.skip_trivia()?;
        let start = self.pos;
        let kind = match self.peek_char() {
            None => TokenKind::Eof,
            Some(c) if is_ident_start(c) => self.lex_identifier(),
            Some(c) if c.is_ascii_digit() => self.lex_number()?,
            Some('.') if self.peek_char_at(1).is_some_and(|c| c.is_ascii_digit()) => {
                self.lex_number()?
            }
            Some(q @ ('"' | '\'')) => self.lex_string(q)?,
            Some('`') => {
                return Err(self.error_at(
                    start,
                    SyntaxErrorKind::Unsupported,
                    "template literals are not supported",
                ));
            }
            Some('\\') => {
                return Err(self.error_at(
                    start,
                    SyntaxErrorKind::Unsupported,
                    "Unicode escape sequences in identifiers are not supported",
                ));
            }
            Some(c) if !c.is_ascii() && c.is_alphabetic() => {
                return Err(self.error_at(
                    start,
                    SyntaxErrorKind::UnknownToken,
                    format!("non-ASCII identifier character {c:?} is not supported"),
                ));
            }
            Some(c) => match self.lex_punct() {
                Some(p) => TokenKind::Punct(p),
                None => {
                    return Err(self.error_at(
                        start,
                        SyntaxErrorKind::UnknownToken,
                        format!("unexpected character {c:?}"),
                    ));
                }
            },
        };
        Ok(Token {
            kind,
            span: Span::new(start, self.pos),
            newline_before,
        })
    }

    fn lex_identifier(&mut self) -> TokenKind {
        let start = self.pos;
        while self.peek_char().is_some_and(is_ident_part) {
            self.bump();
        }
        let text = &self.src[start..self.pos];
        match Keyword::from_text(text) {
            Some(k) => TokenKind::Keyword(k),
            None => TokenKind::Identifier(text.to_string()),
        }
    }

    fn lex_punct(&mut self) -> Option<Punct> {
        let rest = self.rest();
        for &(text, punct) in PUNCTS {
            if rest.starts_with(text) {
                // `a?.5:b` is `a ? .5 : b`, not optional chaining.
                if punct == Punct::OptionalChain
                    && rest[2..].starts_with(|c: char| c.is_ascii_digit())
                {
                    continue;
                }
                self.pos += text.len();
                return Some(punct);
            }
        }
        None
    }

    fn eat_digits(&mut self, radix: u32) -> Result<usize, SyntaxError> {
        let mut count = 0;
        loop {
            match self.peek_char() {
                Some(c) if c.is_digit(radix) => {
                    self.bump();
                    count += 1;
                }
                Some('_') => {
                    return Err(self.error_at(
                        self.pos,
                        SyntaxErrorKind::Unsupported,
                        "numeric separators are not supported",
                    ));
                }
                _ => return Ok(count),
            }
        }
    }

    fn lex_number(&mut self) -> Result<TokenKind, SyntaxError> {
        let start = self.pos;
        let rest = self.rest();
        let radix = match rest.get(..2) {
            Some("0x" | "0X") => 16,
            Some("0o" | "0O") => 8,
            Some("0b" | "0B") => 2,
            _ => 10,
        };
        let mut integral = true;
        let value = if radix != 10 {
            self.pos += 2;
            let digits_start = self.pos;
            if self.eat_digits(radix)? == 0 {
                return Err(self.error(
                    SyntaxErrorKind::InvalidToken,
                    Span::new(start, self.pos),
                    "missing digits after radix prefix",
                ));
            }
            radix_value(&self.src[digits_start..self.pos], radix)
        } else {
            if rest.starts_with('0')
                && rest[1..].starts_with(|c: char| c.is_ascii_digit() || c == '_')
            {
                return Err(self.error(
                    SyntaxErrorKind::InvalidToken,
                    Span::new(start, start + 1),
                    "legacy octal and leading-zero number literals are not allowed in strict mode",
                ));
            }
            self.eat_digits(10)?;
            if self.peek_char() == Some('.') {
                integral = false;
                self.bump();
                self.eat_digits(10)?;
            }
            if matches!(self.peek_char(), Some('e' | 'E')) {
                integral = false;
                self.bump();
                if matches!(self.peek_char(), Some('+' | '-')) {
                    self.bump();
                }
                if self.eat_digits(10)? == 0 {
                    return Err(self.error(
                        SyntaxErrorKind::InvalidToken,
                        Span::new(start, self.pos),
                        "missing exponent digits in number literal",
                    ));
                }
            }
            // Rust's float parser is correctly rounded, overflows to
            // infinity, and accepts every form lexed above ("1.", ".5",
            // "1.e3"). A failure here would be a lexer bug; report it.
            self.src[start..self.pos].parse::<f64>().map_err(|_| {
                self.error(
                    SyntaxErrorKind::InvalidToken,
                    Span::new(start, self.pos),
                    "malformed number literal",
                )
            })?
        };
        match self.peek_char() {
            Some('n') if integral => Err(self.error(
                SyntaxErrorKind::Unsupported,
                Span::new(start, self.pos + 1),
                "BigInt literals are not supported",
            )),
            Some(c) if is_ident_start(c) || c.is_ascii_digit() || c == '\\' => Err(self.error_at(
                self.pos,
                SyntaxErrorKind::InvalidToken,
                "identifier or digit starts immediately after number literal",
            )),
            _ => Ok(TokenKind::Number(value)),
        }
    }

    fn lex_string(&mut self, quote: char) -> Result<TokenKind, SyntaxError> {
        let start = self.pos;
        self.bump();
        let mut out = String::new();
        loop {
            let char_pos = self.pos;
            let Some(c) = self.bump() else {
                return Err(self.error(
                    SyntaxErrorKind::UnexpectedEnd,
                    Span::new(start, self.pos),
                    "unterminated string literal",
                ));
            };
            match c {
                c if c == quote => break,
                '\n' | '\r' => {
                    return Err(self.error(
                        SyntaxErrorKind::InvalidToken,
                        Span::new(start, char_pos),
                        "unterminated string literal (line break before closing quote)",
                    ));
                }
                '\\' => self.lex_escape(char_pos, &mut out)?,
                c => out.push(c),
            }
        }
        Ok(TokenKind::String(out))
    }

    fn lex_escape(&mut self, backslash: usize, out: &mut String) -> Result<(), SyntaxError> {
        let Some(c) = self.bump() else {
            return Err(self.error(
                SyntaxErrorKind::UnexpectedEnd,
                Span::new(backslash, self.pos),
                "unterminated string literal",
            ));
        };
        let escape_error = |lexer: &Self, kind, message: &str| {
            lexer.error(kind, Span::new(backslash, lexer.pos), message.to_string())
        };
        match c {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'b' => out.push('\u{08}'),
            'f' => out.push('\u{0C}'),
            'v' => out.push('\u{0B}'),
            '0' if !self.peek_char().is_some_and(|c| c.is_ascii_digit()) => out.push('\0'),
            '0'..='7' => {
                return Err(escape_error(
                    self,
                    SyntaxErrorKind::InvalidToken,
                    "octal escape sequences are not allowed in strict mode",
                ));
            }
            '8' | '9' => {
                return Err(escape_error(
                    self,
                    SyntaxErrorKind::InvalidToken,
                    "\\8 and \\9 escapes are not allowed in strict mode",
                ));
            }
            '\r' => {
                // Line continuation; CRLF is one terminator.
                if self.peek_char() == Some('\n') {
                    self.bump();
                }
            }
            '\n' | '\u{2028}' | '\u{2029}' => {}
            'x' => {
                let value = self.hex_digits(2).ok_or_else(|| {
                    escape_error(
                        self,
                        SyntaxErrorKind::InvalidToken,
                        "invalid hexadecimal escape sequence",
                    )
                })?;
                out.push(char::from_u32(value).unwrap_or('\u{FFFD}'));
            }
            'u' => {
                let unit = self.unicode_escape_value().ok_or_else(|| {
                    escape_error(
                        self,
                        SyntaxErrorKind::InvalidToken,
                        "invalid Unicode escape sequence",
                    )
                })?;
                let scalar = if (0xD800..0xDC00).contains(&unit) {
                    // A high surrogate must be followed by a `\u` low
                    // surrogate escape to form one scalar value.
                    let mut ahead = self.clone();
                    let low = if ahead.rest().starts_with("\\u") {
                        ahead.pos += 2;
                        ahead.unicode_escape_value()
                    } else {
                        None
                    };
                    match low {
                        Some(low @ 0xDC00..0xE000) => {
                            *self = ahead;
                            0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00)
                        }
                        _ => unit,
                    }
                } else {
                    unit
                };
                match char::from_u32(scalar) {
                    Some(ch) => out.push(ch),
                    None => {
                        return Err(escape_error(
                            self,
                            SyntaxErrorKind::Unsupported,
                            "lone surrogate escapes are not supported (strings hold Unicode scalar values)",
                        ));
                    }
                }
            }
            other => out.push(other),
        }
        Ok(())
    }

    /// Exactly `n` hex digits.
    fn hex_digits(&mut self, n: usize) -> Option<u32> {
        let text = self.rest().get(..n)?;
        if !text.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        self.pos += n;
        u32::from_str_radix(text, 16).ok()
    }

    /// The part of a `\u` escape after the `u`: `HHHH` or `{H...}` (at most
    /// U+10FFFF). Returns the code point (possibly a surrogate).
    fn unicode_escape_value(&mut self) -> Option<u32> {
        if self.peek_char() == Some('{') {
            let close = self.rest().find('}')?;
            let digits = &self.rest()[1..close];
            if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            let trimmed = digits.trim_start_matches('0');
            if trimmed.len() > 6 {
                return None;
            }
            let value = u32::from_str_radix(if trimmed.is_empty() { "0" } else { trimmed }, 16)
                .ok()
                .filter(|v| *v <= 0x10FFFF)?;
            self.pos += close + 1;
            Some(value)
        } else {
            self.hex_digits(4)
        }
    }
}

/// Correctly rounded value of a non-empty digit string in a power-of-two
/// radix (2, 8 or 16).
fn radix_value(digits: &str, radix: u32) -> f64 {
    let bits_per_digit = radix.trailing_zeros();
    let digits = digits.trim_start_matches('0');
    // Keep the leading 128 bits exactly; fold every later nonzero bit into a
    // sticky low bit. 128 bits far exceed f64's 53, so the `u128 -> f64`
    // conversion (round to nearest, ties to even) then rounds correctly.
    let mut mantissa: u128 = 0;
    let mut shift: i32 = 0;
    let mut sticky = false;
    for c in digits.chars() {
        let d = c.to_digit(radix).unwrap_or(0) as u128;
        if mantissa.leading_zeros() >= bits_per_digit {
            mantissa = (mantissa << bits_per_digit) | d;
        } else {
            shift += bits_per_digit as i32;
            sticky |= d != 0;
        }
    }
    if sticky {
        mantissa |= 1;
    }
    let value = mantissa as f64;
    if shift == 0 {
        value
    } else {
        // Exact power-of-two scaling; overflow to infinity is correct because
        // the mantissa is already at least 2^120 here.
        value * 2f64.powi(shift)
    }
}
