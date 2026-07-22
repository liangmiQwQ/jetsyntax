//! On-demand ECMAScript lexer used by the `JetSyntax` parser.
//!
//! The lexer does not allocate a token buffer. The parser keeps one lookahead token and asks for
//! another only when it commits to a grammar branch. Regular expressions and template continuations
//! are explicitly rescanned because their meaning depends on parser context.

/// A lexical token with byte offsets into the original UTF-8 source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: u32,
    pub end: u32,
    pub flags: TokenFlags,
}

impl Token {
    #[must_use]
    pub const fn len(self) -> u32 {
        self.end - self.start
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// Token metadata kept outside the kind discriminant.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TokenFlags(u8);

impl TokenFlags {
    const LINE_BREAK_BEFORE: u8 = 1 << 0;
    const ESCAPED: u8 = 1 << 1;
    const LEGACY_OCTAL: u8 = 1 << 2;
    const SEPARATOR: u8 = 1 << 3;
    const INVALID_TEMPLATE_ESCAPE: u8 = 1 << 4;

    #[must_use]
    pub const fn line_break_before(self) -> bool {
        self.0 & Self::LINE_BREAK_BEFORE != 0
    }

    #[must_use]
    pub const fn escaped(self) -> bool {
        self.0 & Self::ESCAPED != 0
    }

    #[must_use]
    pub const fn legacy_octal(self) -> bool {
        self.0 & Self::LEGACY_OCTAL != 0
    }

    #[must_use]
    pub const fn contains_separator(self) -> bool {
        self.0 & Self::SEPARATOR != 0
    }

    #[must_use]
    pub const fn invalid_template_escape(self) -> bool {
        self.0 & Self::INVALID_TEMPLATE_ESCAPE != 0
    }

    const fn insert(&mut self, flag: u8) {
        self.0 |= flag;
    }
}

/// ECMAScript, JSX, and TypeScript token kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum TokenKind {
    Eof,
    Invalid,
    Identifier,
    PrivateIdentifier,
    Number,
    BigInt,
    String,
    RegExp,
    NoSubstitutionTemplate,
    TemplateHead,
    TemplateMiddle,
    TemplateTail,
    JsxText,
    Break,
    Case,
    Catch,
    Class,
    Const,
    Continue,
    Debugger,
    Default,
    Delete,
    Do,
    Else,
    Export,
    Extends,
    False,
    Finally,
    For,
    Function,
    If,
    Import,
    In,
    Instanceof,
    New,
    Null,
    Return,
    Super,
    Switch,
    This,
    Throw,
    True,
    Try,
    Typeof,
    Var,
    Void,
    While,
    With,
    Yield,
    Async,
    Await,
    Let,
    Static,
    Of,
    Get,
    Set,
    As,
    Satisfies,
    Accessor,
    Using,
    Declare,
    Abstract,
    Interface,
    Type,
    Enum,
    Namespace,
    Module,
    Implements,
    Infer,
    Keyof,
    Readonly,
    Unique,
    Unknown,
    Never,
    Any,
    Boolean,
    NumberKeyword,
    StringKeyword,
    Symbol,
    Object,
    Undefined,
    Is,
    Asserts,
    Public,
    Protected,
    Private,
    Override,
    Out,
    Meta,
    From,
    Require,
    LeftBrace,
    RightBrace,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Dot,
    Ellipsis,
    Semicolon,
    Comma,
    Colon,
    Question,
    QuestionDot,
    QuestionQuestion,
    QuestionQuestionEq,
    Arrow,
    Plus,
    PlusPlus,
    PlusEq,
    Minus,
    MinusMinus,
    MinusEq,
    Star,
    StarStar,
    StarEq,
    StarStarEq,
    Slash,
    SlashEq,
    Percent,
    PercentEq,
    Amp,
    AmpAmp,
    AmpEq,
    AmpAmpEq,
    Pipe,
    PipePipe,
    PipeEq,
    PipePipeEq,
    Caret,
    CaretEq,
    Bang,
    BangEq,
    BangEqEq,
    Eq,
    EqEq,
    EqEqEq,
    Lt,
    LtEq,
    ShiftLeft,
    ShiftLeftEq,
    Gt,
    GtEq,
    ShiftRight,
    ShiftRightEq,
    ShiftRightUnsigned,
    ShiftRightUnsignedEq,
    Tilde,
    At,
    Hash,
    Backtick,
}

/// A recoverable lexical error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LexError {
    pub start: u32,
    pub end: u32,
    pub message: &'static str,
}

#[derive(Clone, Copy)]
pub(crate) struct LexerCheckpoint {
    position: usize,
    error_len: usize,
}

/// Stateful on-demand lexer.
pub struct Lexer<'s> {
    source: &'s str,
    bytes: &'s [u8],
    position: usize,
    errors: Vec<LexError>,
}

impl<'s> Lexer<'s> {
    /// Create a lexer. Sources larger than four GiB are rejected by the parser API before this.
    #[must_use]
    pub const fn new(source: &'s str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            position: 0,
            errors: Vec::new(),
        }
    }

    #[must_use]
    pub const fn position(&self) -> usize {
        self.position
    }

    pub fn set_position(&mut self, position: usize) {
        self.position = position.min(self.bytes.len());
    }

    #[must_use]
    pub fn errors(&self) -> &[LexError] {
        &self.errors
    }

    #[must_use]
    pub(crate) const fn checkpoint(&self) -> LexerCheckpoint {
        LexerCheckpoint {
            position: self.position,
            error_len: self.errors.len(),
        }
    }

    pub(crate) fn rollback(&mut self, checkpoint: LexerCheckpoint) {
        debug_assert!(checkpoint.error_len <= self.errors.len());
        self.position = checkpoint.position;
        self.errors.truncate(checkpoint.error_len);
    }

    #[must_use]
    pub fn source_text(&self, token: Token) -> &'s str {
        &self.source[token.start as usize..token.end as usize]
    }

    // Keeping punctuator dispatch contiguous makes longest-match ordering auditable.
    #[allow(clippy::too_many_lines)]
    pub fn next_token(&mut self) -> Token {
        let line_break = self.skip_trivia();
        let start = self.position;
        let mut flags = TokenFlags::default();
        if line_break {
            flags.insert(TokenFlags::LINE_BREAK_BEFORE);
        }
        let Some(&byte) = self.bytes.get(start) else {
            return self.token(TokenKind::Eof, start, flags);
        };

        let kind = match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$' | b'\\' => {
                return self.identifier(start, flags);
            }
            b'0'..=b'9' => return self.number(start, flags, false),
            b'\'' | b'"' => return self.string(start, flags, byte),
            b'`' => return self.template_start(start, flags),
            b'{' => self.one(TokenKind::LeftBrace),
            b'}' => self.one(TokenKind::RightBrace),
            b'(' => self.one(TokenKind::LeftParen),
            b')' => self.one(TokenKind::RightParen),
            b'[' => self.one(TokenKind::LeftBracket),
            b']' => self.one(TokenKind::RightBracket),
            b';' => self.one(TokenKind::Semicolon),
            b',' => self.one(TokenKind::Comma),
            b':' => self.one(TokenKind::Colon),
            b'~' => self.one(TokenKind::Tilde),
            b'@' => self.one(TokenKind::At),
            b'.' if self.peek(1).is_some_and(|byte| byte.is_ascii_digit()) => {
                return self.number(start, flags, true);
            }
            b'.' if self.peek(1) == Some(b'.') && self.peek(2) == Some(b'.') => {
                self.advance(3, TokenKind::Ellipsis)
            }
            b'.' => self.one(TokenKind::Dot),
            b'?' if self.peek(1) == Some(b'.')
                && !self.peek(2).is_some_and(|byte| byte.is_ascii_digit()) =>
            {
                self.advance(2, TokenKind::QuestionDot)
            }
            b'?' if self.peek(1) == Some(b'?') && self.peek(2) == Some(b'=') => {
                self.advance(3, TokenKind::QuestionQuestionEq)
            }
            b'?' if self.peek(1) == Some(b'?') => self.advance(2, TokenKind::QuestionQuestion),
            b'?' => self.one(TokenKind::Question),
            b'+' if self.peek(1) == Some(b'+') => self.advance(2, TokenKind::PlusPlus),
            b'+' if self.peek(1) == Some(b'=') => self.advance(2, TokenKind::PlusEq),
            b'+' => self.one(TokenKind::Plus),
            b'-' if self.peek(1) == Some(b'-') => self.advance(2, TokenKind::MinusMinus),
            b'-' if self.peek(1) == Some(b'=') => self.advance(2, TokenKind::MinusEq),
            b'-' => self.one(TokenKind::Minus),
            b'*' if self.peek(1) == Some(b'*') && self.peek(2) == Some(b'=') => {
                self.advance(3, TokenKind::StarStarEq)
            }
            b'*' if self.peek(1) == Some(b'*') => self.advance(2, TokenKind::StarStar),
            b'*' if self.peek(1) == Some(b'=') => self.advance(2, TokenKind::StarEq),
            b'*' => self.one(TokenKind::Star),
            b'/' if self.peek(1) == Some(b'=') => self.advance(2, TokenKind::SlashEq),
            b'/' => self.one(TokenKind::Slash),
            b'%' if self.peek(1) == Some(b'=') => self.advance(2, TokenKind::PercentEq),
            b'%' => self.one(TokenKind::Percent),
            b'&' if self.peek(1) == Some(b'&') && self.peek(2) == Some(b'=') => {
                self.advance(3, TokenKind::AmpAmpEq)
            }
            b'&' if self.peek(1) == Some(b'&') => self.advance(2, TokenKind::AmpAmp),
            b'&' if self.peek(1) == Some(b'=') => self.advance(2, TokenKind::AmpEq),
            b'&' => self.one(TokenKind::Amp),
            b'|' if self.peek(1) == Some(b'|') && self.peek(2) == Some(b'=') => {
                self.advance(3, TokenKind::PipePipeEq)
            }
            b'|' if self.peek(1) == Some(b'|') => self.advance(2, TokenKind::PipePipe),
            b'|' if self.peek(1) == Some(b'=') => self.advance(2, TokenKind::PipeEq),
            b'|' => self.one(TokenKind::Pipe),
            b'^' if self.peek(1) == Some(b'=') => self.advance(2, TokenKind::CaretEq),
            b'^' => self.one(TokenKind::Caret),
            b'!' if self.peek(1) == Some(b'=') && self.peek(2) == Some(b'=') => {
                self.advance(3, TokenKind::BangEqEq)
            }
            b'!' if self.peek(1) == Some(b'=') => self.advance(2, TokenKind::BangEq),
            b'!' => self.one(TokenKind::Bang),
            b'=' if self.peek(1) == Some(b'=') && self.peek(2) == Some(b'=') => {
                self.advance(3, TokenKind::EqEqEq)
            }
            b'=' if self.peek(1) == Some(b'=') => self.advance(2, TokenKind::EqEq),
            b'=' if self.peek(1) == Some(b'>') => self.advance(2, TokenKind::Arrow),
            b'=' => self.one(TokenKind::Eq),
            b'<' if self.peek(1) == Some(b'<') && self.peek(2) == Some(b'=') => {
                self.advance(3, TokenKind::ShiftLeftEq)
            }
            b'<' if self.peek(1) == Some(b'<') => self.advance(2, TokenKind::ShiftLeft),
            b'<' if self.peek(1) == Some(b'=') => self.advance(2, TokenKind::LtEq),
            b'<' => self.one(TokenKind::Lt),
            b'>' if self.peek(1) == Some(b'>')
                && self.peek(2) == Some(b'>')
                && self.peek(3) == Some(b'=') =>
            {
                self.advance(4, TokenKind::ShiftRightUnsignedEq)
            }
            b'>' if self.peek(1) == Some(b'>') && self.peek(2) == Some(b'>') => {
                self.advance(3, TokenKind::ShiftRightUnsigned)
            }
            b'>' if self.peek(1) == Some(b'>') && self.peek(2) == Some(b'=') => {
                self.advance(3, TokenKind::ShiftRightEq)
            }
            b'>' if self.peek(1) == Some(b'>') => self.advance(2, TokenKind::ShiftRight),
            b'>' if self.peek(1) == Some(b'=') => self.advance(2, TokenKind::GtEq),
            b'>' => self.one(TokenKind::Gt),
            b'#' if self.peek(1).is_some_and(|next| {
                is_ascii_identifier_start(next) || next == b'\\' || next >= 0x80
            }) =>
            {
                self.position += 1;
                return self
                    .identifier(start, flags)
                    .with_kind(TokenKind::PrivateIdentifier);
            }
            b'#' => self.one(TokenKind::Hash),
            byte if byte >= 0x80 => return self.identifier(start, flags),
            _ => {
                self.position += 1;
                self.error(start, self.position, "unexpected character");
                TokenKind::Invalid
            }
        };
        self.token(kind, start, flags)
    }

    /// Rescan a `/` token as a regular-expression literal after the parser selects that grammar.
    pub fn scan_regexp(&mut self, slash: Token) -> Token {
        self.scan_regexp_with_flag_errors(slash, true)
    }

    pub(crate) fn scan_regexp_with_flag_errors(
        &mut self,
        slash: Token,
        flag_errors: bool,
    ) -> Token {
        self.position = slash.start as usize + 1;
        let mut in_class = false;
        let mut escaped = false;
        let mut terminated = false;
        while let Some(&byte) = self.bytes.get(self.position) {
            if is_line_terminator_byte(byte) || self.current_is_unicode_line_terminator() {
                break;
            }
            self.position += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'[' {
                in_class = true;
            } else if byte == b']' {
                in_class = false;
            } else if byte == b'/' && !in_class {
                terminated = true;
                break;
            }
        }
        if !terminated {
            self.error(
                slash.start as usize,
                self.position,
                "unterminated regular expression",
            );
        }
        let mut flags = 0_u8;
        while self.current_identifier_continue() {
            let start = self.position;
            let Some(&flag) = self.bytes.get(self.position) else {
                break;
            };
            self.advance_char();
            if !flag_errors {
                continue;
            }
            let bit = match flag {
                b'd' => 1 << 0,
                b'g' => 1 << 1,
                b'i' => 1 << 2,
                b'm' => 1 << 3,
                b's' => 1 << 4,
                b'u' => 1 << 5,
                b'v' => 1 << 6,
                b'y' => 1 << 7,
                _ => {
                    self.error(start, self.position, "invalid regular expression flag");
                    continue;
                }
            };
            if flags & bit != 0 {
                self.error(start, self.position, "duplicate regular expression flag");
            } else if matches!(flag, b'u' | b'v') && flags & ((1 << 5) | (1 << 6)) != 0 {
                self.error(
                    start,
                    self.position,
                    "regular expression flags `u` and `v` cannot be combined",
                );
            }
            flags |= bit;
        }
        Token {
            kind: TokenKind::RegExp,
            start: slash.start,
            end: wire_offset(self.position),
            flags: slash.flags,
        }
    }

    /// Resume a template after the parser consumes the matching `}` of a substitution.
    pub fn resume_template(&mut self, right_brace: Token) -> Token {
        let start = right_brace.start as usize;
        self.scan_template(start, right_brace.flags, false)
    }

    /// Scan raw JSX text up to `<` or `{`.
    pub fn next_jsx_text(&mut self) -> Token {
        let start = self.position;
        while let Some(&byte) = self.bytes.get(self.position) {
            if matches!(byte, b'<' | b'{') {
                break;
            }
            self.position += 1;
        }
        self.token(TokenKind::JsxText, start, TokenFlags::default())
    }

    /// Scan a JSX identifier. JSX permits hyphens after the first identifier character.
    pub fn next_jsx_identifier(&mut self) -> Token {
        let start = self.position;
        if !self.current_identifier_start() {
            self.advance_char();
            self.error(start, self.position, "invalid JSX identifier start");
            return self.token(TokenKind::Invalid, start, TokenFlags::default());
        }
        self.advance_char();
        while self.current_identifier_continue() || self.bytes.get(self.position) == Some(&b'-') {
            self.advance_char();
        }
        self.token(TokenKind::Identifier, start, TokenFlags::default())
    }

    /// Scan a quoted JSX attribute value without interpreting JavaScript escapes.
    pub fn next_jsx_string(&mut self) -> Token {
        let start = self.position;
        let Some(quote @ (b'\'' | b'"')) = self.bytes.get(self.position).copied() else {
            self.advance_char();
            self.error(
                start,
                self.position,
                "expected a quoted JSX attribute value",
            );
            return self.token(TokenKind::Invalid, start, TokenFlags::default());
        };
        self.position += 1;
        while self
            .bytes
            .get(self.position)
            .is_some_and(|byte| *byte != quote)
        {
            self.advance_char();
        }
        if self.bytes.get(self.position) == Some(&quote) {
            self.position += 1;
        } else {
            self.error(start, self.position, "unterminated JSX attribute value");
        }
        self.token(TokenKind::String, start, TokenFlags::default())
    }

    fn skip_trivia(&mut self) -> bool {
        let mut line_break = if self.position == 0 && self.bytes.starts_with(b"#!") {
            self.skip_line_comment();
            true
        } else {
            false
        };
        loop {
            while self.position < self.bytes.len() && self.bytes[self.position] == b' ' {
                self.position += 1;
            }
            match self.bytes.get(self.position).copied() {
                Some(b'\t' | 0x0B | 0x0C) => self.position += 1,
                Some(b'\n') => {
                    self.position += 1;
                    line_break = true;
                }
                Some(b'\r') => {
                    self.position += 1;
                    if self.bytes.get(self.position) == Some(&b'\n') {
                        self.position += 1;
                    }
                    line_break = true;
                }
                Some(b'/') if self.peek(1) == Some(b'/') => {
                    self.position += 2;
                    self.skip_line_comment();
                }
                Some(b'/') if self.peek(1) == Some(b'*') => {
                    self.position += 2;
                    let comment_start = self.position - 2;
                    let mut terminated = false;
                    while let Some(&byte) = self.bytes.get(self.position) {
                        if byte == b'*' && self.peek(1) == Some(b'/') {
                            self.position += 2;
                            terminated = true;
                            break;
                        }
                        if is_line_terminator_byte(byte) {
                            line_break = true;
                        }
                        if byte >= 0x80 {
                            let character = self.current_char();
                            if character.is_some_and(is_unicode_line_terminator) {
                                line_break = true;
                            }
                            self.position += character.map_or(1, char::len_utf8);
                        } else {
                            self.position += 1;
                        }
                    }
                    if !terminated {
                        self.error(comment_start, self.position, "unterminated block comment");
                    }
                }
                Some(byte) if byte >= 0x80 => {
                    let Some(character) = self.current_char() else {
                        break;
                    };
                    if is_unicode_line_terminator(character) {
                        line_break = true;
                        self.position += character.len_utf8();
                    } else if character.is_whitespace() {
                        self.position += character.len_utf8();
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        line_break
    }

    fn skip_line_comment(&mut self) {
        while let Some(&byte) = self.bytes.get(self.position) {
            if is_line_terminator_byte(byte) || self.current_is_unicode_line_terminator() {
                break;
            }
            self.position += self.current_char().map_or(1, char::len_utf8);
        }
    }

    fn identifier(&mut self, start: usize, mut flags: TokenFlags) -> Token {
        if self.position == start && self.bytes.get(start) == Some(&b'#') {
            self.position += 1;
        }

        let has_start = match self.bytes.get(self.position).copied() {
            Some(byte) if is_ascii_identifier_start(byte) => {
                self.position += 1;
                true
            }
            Some(b'\\' | 0x80..=u8::MAX) => self.identifier_slow(true, &mut flags),
            _ => false,
        };
        if !has_start {
            if self.position == start {
                self.advance_char();
            }
            self.error(start, self.position, "invalid identifier start");
            return self.token(TokenKind::Invalid, start, flags);
        }

        loop {
            while self.position < self.bytes.len()
                && is_ascii_identifier_continue(self.bytes[self.position])
            {
                self.position += 1;
            }

            match self.bytes.get(self.position).copied() {
                Some(b'\\' | 0x80..=u8::MAX) => {
                    if !self.identifier_slow(false, &mut flags) {
                        break;
                    }
                }
                _ => break,
            }
        }

        let text = &self.bytes[start..self.position];
        let kind = if flags.escaped() {
            TokenKind::Identifier
        } else {
            keyword(text)
        };
        self.token(kind, start, flags)
    }

    #[cold]
    fn identifier_slow(&mut self, first: bool, flags: &mut TokenFlags) -> bool {
        match self.bytes.get(self.position).copied() {
            Some(b'\\') => {
                flags.insert(TokenFlags::ESCAPED);
                self.identifier_escape(first)
            }
            Some(byte) if byte >= 0x80 => {
                let Some(character) = self.current_char() else {
                    return false;
                };
                let valid = if first {
                    unicode_id_start::is_id_start(character)
                } else {
                    unicode_id_start::is_id_continue(character)
                };
                if !valid && !is_identifier_joiner(character) {
                    return false;
                }
                self.position += character.len_utf8();
                true
            }
            _ => false,
        }
    }

    fn identifier_escape(&mut self, first: bool) -> bool {
        let escape_start = self.position;
        self.position += 1;
        if self.bytes.get(self.position) != Some(&b'u') {
            self.error(
                escape_start,
                self.position,
                "identifier escape must use Unicode syntax",
            );
            return false;
        }
        self.position += 1;
        let value = if self.bytes.get(self.position) == Some(&b'{') {
            self.position += 1;
            let digits_start = self.position;
            while self
                .bytes
                .get(self.position)
                .is_some_and(u8::is_ascii_hexdigit)
            {
                self.position += 1;
            }
            let value = parse_hex(&self.bytes[digits_start..self.position]);
            if self.position == digits_start || self.bytes.get(self.position) != Some(&b'}') {
                self.error(escape_start, self.position, "invalid braced Unicode escape");
                return false;
            }
            self.position += 1;
            value
        } else {
            let end = self.position.saturating_add(4);
            if end > self.bytes.len()
                || !self.bytes[self.position..end]
                    .iter()
                    .all(u8::is_ascii_hexdigit)
            {
                self.error(escape_start, self.position, "invalid Unicode escape");
                return false;
            }
            let value = parse_hex(&self.bytes[self.position..end]);
            self.position = end;
            value
        };
        let Some(character) = value.and_then(char::from_u32) else {
            self.error(
                escape_start,
                self.position,
                "Unicode escape is outside the valid range",
            );
            return false;
        };
        let valid = if first {
            is_identifier_start(character)
        } else {
            is_identifier_continue(character)
        };
        if !valid {
            self.error(
                escape_start,
                self.position,
                "escaped character is not valid in an identifier",
            );
        }
        valid
    }

    fn number(&mut self, start: usize, mut flags: TokenFlags, leading_dot: bool) -> Token {
        let mut radix = 10;
        let mut legacy_octal = false;
        let mut has_fraction = leading_dot;
        let mut has_exponent = false;
        if leading_dot {
            self.position += 1;
            self.digits(10, &mut flags);
        } else if self.bytes.get(start) == Some(&b'0') {
            self.position += 1;
            match self.bytes.get(self.position).copied() {
                Some(b'x' | b'X') => {
                    radix = 16;
                    self.position += 1;
                    self.require_digits(start, 16, &mut flags);
                }
                Some(b'b' | b'B') => {
                    radix = 2;
                    self.position += 1;
                    self.require_digits(start, 2, &mut flags);
                }
                Some(b'o' | b'O') => {
                    radix = 8;
                    self.position += 1;
                    self.require_digits(start, 8, &mut flags);
                }
                Some(b'0'..=b'7') => {
                    legacy_octal = true;
                    flags.insert(TokenFlags::LEGACY_OCTAL);
                    self.digits(10, &mut flags);
                }
                _ => {}
            }
        } else {
            self.digits(10, &mut flags);
        }

        if radix == 10 && self.bytes.get(self.position) == Some(&b'.') {
            has_fraction = true;
            self.position += 1;
            self.digits(10, &mut flags);
        }
        if radix == 10 && matches!(self.bytes.get(self.position), Some(b'e' | b'E')) {
            has_exponent = true;
            self.position += 1;
            if matches!(self.bytes.get(self.position), Some(b'+' | b'-')) {
                self.position += 1;
            }
            self.require_digits(start, 10, &mut flags);
        }
        if radix != 10
            && self
                .bytes
                .get(self.position)
                .is_some_and(u8::is_ascii_digit)
        {
            self.error(
                start,
                self.position + 1,
                "digit is not valid for this numeric radix",
            );
            self.digits(10, &mut flags);
        }

        let kind = if self.bytes.get(self.position) == Some(&b'n') {
            self.position += 1;
            if has_fraction || has_exponent || legacy_octal {
                self.error(start, self.position, "invalid BigInt literal");
            }
            TokenKind::BigInt
        } else {
            TokenKind::Number
        };
        if self.current_identifier_start() || self.bytes.get(self.position) == Some(&b'\\') {
            self.error(
                start,
                self.position,
                "identifier cannot immediately follow a number",
            );
        }
        self.token(kind, start, flags)
    }

    fn require_digits(&mut self, start: usize, radix: u8, flags: &mut TokenFlags) {
        let digits_start = self.position;
        self.digits(radix, flags);
        if self.position == digits_start {
            self.error(start, self.position, "numeric literal requires digits");
        }
    }

    fn digits(&mut self, radix: u8, flags: &mut TokenFlags) {
        let mut previous_separator = false;
        let start = self.position;
        while let Some(&byte) = self.bytes.get(self.position) {
            if digit_value(byte).is_some_and(|value| value < radix) {
                previous_separator = false;
                self.position += 1;
            } else if byte == b'_' {
                flags.insert(TokenFlags::SEPARATOR);
                if self.position == start || previous_separator {
                    self.error(
                        self.position,
                        self.position + 1,
                        "misplaced numeric separator",
                    );
                }
                previous_separator = true;
                self.position += 1;
            } else {
                break;
            }
        }
        if previous_separator {
            self.error(
                self.position - 1,
                self.position,
                "numeric separator cannot end a literal",
            );
        }
    }

    fn string(&mut self, start: usize, flags: TokenFlags, quote: u8) -> Token {
        self.position += 1;
        let mut terminated = false;
        while let Some(&byte) = self.bytes.get(self.position) {
            if byte == quote {
                self.position += 1;
                terminated = true;
                break;
            }
            if is_line_terminator_byte(byte)
                || (byte >= 0x80 && self.current_is_unicode_line_terminator())
            {
                break;
            }
            self.position += 1;
            if byte == b'\\' {
                let escape_start = self.position - 1;
                if self.scan_braced_unicode_escape(escape_start) {
                    continue;
                }
                if self.bytes.get(self.position) == Some(&b'\r') {
                    self.position += 1;
                    if self.bytes.get(self.position) == Some(&b'\n') {
                        self.position += 1;
                    }
                } else if self.bytes.get(self.position).is_some() {
                    self.advance_char();
                }
            }
        }
        if !terminated {
            self.error(start, self.position, "unterminated string literal");
        }
        self.token(TokenKind::String, start, flags)
    }

    fn template_start(&mut self, start: usize, flags: TokenFlags) -> Token {
        self.position += 1;
        self.scan_template(start, flags, true)
    }

    fn scan_template(&mut self, start: usize, mut flags: TokenFlags, first: bool) -> Token {
        while let Some(&byte) = self.bytes.get(self.position) {
            if byte == b'`' {
                self.position += 1;
                return self.token(
                    if first {
                        TokenKind::NoSubstitutionTemplate
                    } else {
                        TokenKind::TemplateTail
                    },
                    start,
                    flags,
                );
            }
            if byte == b'$' && self.peek(1) == Some(b'{') {
                self.position += 2;
                return self.token(
                    if first {
                        TokenKind::TemplateHead
                    } else {
                        TokenKind::TemplateMiddle
                    },
                    start,
                    flags,
                );
            }
            self.position += 1;
            if byte == b'\\'
                && self.bytes.get(self.position).is_some()
                && self.scan_template_escape()
            {
                flags.insert(TokenFlags::INVALID_TEMPLATE_ESCAPE);
            }
        }
        self.error(start, self.position, "unterminated template literal");
        self.token(TokenKind::TemplateTail, start, flags)
    }

    /// Scan one template escape after its leading backslash.
    ///
    /// Invalid escapes are legal inside tagged templates, so the lexer records their presence on
    /// the segment instead of reporting an unconditional lexical error. Malformed escapes stop
    /// before a template delimiter so `${` and the closing backtick retain their grammar meaning.
    fn scan_template_escape(&mut self) -> bool {
        let Some(&byte) = self.bytes.get(self.position) else {
            return true;
        };
        match byte {
            b'\r' => {
                self.position += 1;
                if self.bytes.get(self.position) == Some(&b'\n') {
                    self.position += 1;
                }
                false
            }
            b'\n' => {
                self.position += 1;
                false
            }
            b'x' => {
                self.position += 1;
                let digits_start = self.position;
                while self.position - digits_start < 2
                    && self
                        .bytes
                        .get(self.position)
                        .is_some_and(u8::is_ascii_hexdigit)
                {
                    self.position += 1;
                }
                self.position - digits_start != 2
            }
            b'u' => {
                self.position += 1;
                if self.bytes.get(self.position) == Some(&b'{') {
                    self.position += 1;
                    let digits_start = self.position;
                    while self
                        .bytes
                        .get(self.position)
                        .is_some_and(u8::is_ascii_hexdigit)
                    {
                        self.position += 1;
                    }
                    let digits_end = self.position;
                    let value = parse_hex(&self.bytes[digits_start..digits_end]);
                    if self.bytes.get(self.position) != Some(&b'}') {
                        return true;
                    }
                    self.position += 1;
                    digits_start == digits_end || value.is_none_or(|value| value > 0x10_FFFF)
                } else {
                    let digits_start = self.position;
                    while self.position - digits_start < 4
                        && self
                            .bytes
                            .get(self.position)
                            .is_some_and(u8::is_ascii_hexdigit)
                    {
                        self.position += 1;
                    }
                    self.position - digits_start != 4
                }
            }
            b'0' => {
                self.position += 1;
                self.bytes
                    .get(self.position)
                    .is_some_and(u8::is_ascii_digit)
            }
            b'1'..=b'9' => {
                self.position += 1;
                true
            }
            _ => {
                self.advance_char();
                false
            }
        }
    }

    fn scan_braced_unicode_escape(&mut self, escape_start: usize) -> bool {
        if self.bytes.get(self.position) != Some(&b'u') || self.peek(1) != Some(b'{') {
            return false;
        }

        self.position += 2;
        let digits_start = self.position;
        while self
            .bytes
            .get(self.position)
            .is_some_and(u8::is_ascii_hexdigit)
        {
            self.position += 1;
        }
        let value = parse_hex(&self.bytes[digits_start..self.position]);
        if self.position == digits_start || self.bytes.get(self.position) != Some(&b'}') {
            self.error(escape_start, self.position, "invalid braced Unicode escape");
            return true;
        }

        self.position += 1;
        if value.is_none_or(|value| value > 0x10_FFFF) {
            self.error(
                escape_start,
                self.position,
                "Unicode escape is outside the valid range",
            );
        }
        true
    }

    fn current_identifier_start(&self) -> bool {
        self.bytes.get(self.position).is_some_and(|byte| {
            is_ascii_identifier_start(*byte)
                || (*byte >= 0x80 && self.current_char().is_some_and(is_identifier_start))
        })
    }

    fn current_identifier_continue(&self) -> bool {
        self.bytes.get(self.position).is_some_and(|byte| {
            is_ascii_identifier_continue(*byte)
                || (*byte >= 0x80 && self.current_char().is_some_and(is_identifier_continue))
        })
    }

    fn current_char(&self) -> Option<char> {
        self.source.get(self.position..)?.chars().next()
    }

    fn current_is_unicode_line_terminator(&self) -> bool {
        self.current_char().is_some_and(is_unicode_line_terminator)
    }

    fn advance_char(&mut self) {
        self.position += self.current_char().map_or(1, char::len_utf8);
    }

    fn peek(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.position + offset).copied()
    }

    const fn one(&mut self, kind: TokenKind) -> TokenKind {
        self.advance(1, kind)
    }

    const fn advance(&mut self, count: usize, kind: TokenKind) -> TokenKind {
        self.position += count;
        kind
    }

    fn token(&self, kind: TokenKind, start: usize, flags: TokenFlags) -> Token {
        Token {
            kind,
            start: wire_offset(start),
            end: wire_offset(self.position),
            flags,
        }
    }

    fn error(&mut self, start: usize, end: usize, message: &'static str) {
        self.errors.push(LexError {
            start: wire_offset(start),
            end: wire_offset(end),
            message,
        });
    }
}

/// The parser rejects sources beyond the wire format's `u32` limit. Saturation keeps direct lexer
/// use bounded as well, without silently wrapping offsets for an oversized standalone input.
fn wire_offset(offset: usize) -> u32 {
    u32::try_from(offset).unwrap_or(u32::MAX)
}

impl Token {
    const fn with_kind(mut self, kind: TokenKind) -> Self {
        self.kind = kind;
        self
    }
}

const fn is_ascii_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$')
}

const fn is_ascii_identifier_continue(byte: u8) -> bool {
    is_ascii_identifier_start(byte) || byte.is_ascii_digit()
}

fn is_identifier_start(character: char) -> bool {
    matches!(character, '$' | '_') || unicode_id_start::is_id_start(character)
}

fn is_identifier_continue(character: char) -> bool {
    is_identifier_start(character)
        || unicode_id_start::is_id_continue(character)
        || is_identifier_joiner(character)
}

const fn is_identifier_joiner(character: char) -> bool {
    matches!(character, '\u{200C}' | '\u{200D}')
}

const fn is_line_terminator_byte(byte: u8) -> bool {
    matches!(byte, b'\n' | b'\r')
}

const fn is_unicode_line_terminator(character: char) -> bool {
    matches!(character, '\u{2028}' | '\u{2029}')
}

const fn digit_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn parse_hex(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0_u32, |value, byte| {
        value
            .checked_mul(16)?
            .checked_add(u32::from(digit_value(*byte)?))
    })
}

fn keyword(text: &[u8]) -> TokenKind {
    match text {
        b"break" => TokenKind::Break,
        b"case" => TokenKind::Case,
        b"catch" => TokenKind::Catch,
        b"class" => TokenKind::Class,
        b"const" => TokenKind::Const,
        b"continue" => TokenKind::Continue,
        b"debugger" => TokenKind::Debugger,
        b"default" => TokenKind::Default,
        b"delete" => TokenKind::Delete,
        b"do" => TokenKind::Do,
        b"else" => TokenKind::Else,
        b"export" => TokenKind::Export,
        b"extends" => TokenKind::Extends,
        b"false" => TokenKind::False,
        b"finally" => TokenKind::Finally,
        b"for" => TokenKind::For,
        b"function" => TokenKind::Function,
        b"if" => TokenKind::If,
        b"import" => TokenKind::Import,
        b"in" => TokenKind::In,
        b"instanceof" => TokenKind::Instanceof,
        b"new" => TokenKind::New,
        b"null" => TokenKind::Null,
        b"return" => TokenKind::Return,
        b"super" => TokenKind::Super,
        b"switch" => TokenKind::Switch,
        b"this" => TokenKind::This,
        b"throw" => TokenKind::Throw,
        b"true" => TokenKind::True,
        b"try" => TokenKind::Try,
        b"typeof" => TokenKind::Typeof,
        b"var" => TokenKind::Var,
        b"void" => TokenKind::Void,
        b"while" => TokenKind::While,
        b"with" => TokenKind::With,
        b"yield" => TokenKind::Yield,
        b"async" => TokenKind::Async,
        b"await" => TokenKind::Await,
        b"let" => TokenKind::Let,
        b"static" => TokenKind::Static,
        b"of" => TokenKind::Of,
        b"get" => TokenKind::Get,
        b"set" => TokenKind::Set,
        b"as" => TokenKind::As,
        b"satisfies" => TokenKind::Satisfies,
        b"accessor" => TokenKind::Accessor,
        b"using" => TokenKind::Using,
        b"declare" => TokenKind::Declare,
        b"abstract" => TokenKind::Abstract,
        b"interface" => TokenKind::Interface,
        b"type" => TokenKind::Type,
        b"enum" => TokenKind::Enum,
        b"namespace" => TokenKind::Namespace,
        b"module" => TokenKind::Module,
        b"implements" => TokenKind::Implements,
        b"infer" => TokenKind::Infer,
        b"keyof" => TokenKind::Keyof,
        b"readonly" => TokenKind::Readonly,
        b"unique" => TokenKind::Unique,
        b"unknown" => TokenKind::Unknown,
        b"never" => TokenKind::Never,
        b"any" => TokenKind::Any,
        b"boolean" => TokenKind::Boolean,
        b"number" => TokenKind::NumberKeyword,
        b"string" => TokenKind::StringKeyword,
        b"symbol" => TokenKind::Symbol,
        b"object" => TokenKind::Object,
        b"undefined" => TokenKind::Undefined,
        b"is" => TokenKind::Is,
        b"asserts" => TokenKind::Asserts,
        b"public" => TokenKind::Public,
        b"protected" => TokenKind::Protected,
        b"private" => TokenKind::Private,
        b"override" => TokenKind::Override,
        b"out" => TokenKind::Out,
        b"meta" => TokenKind::Meta,
        b"from" => TokenKind::From,
        b"require" => TokenKind::Require,
        _ => TokenKind::Identifier,
    }
}

#[cfg(test)]
mod tests {
    use super::{Lexer, TokenKind};

    fn kinds(source: &str) -> Vec<TokenKind> {
        let mut lexer = Lexer::new(source);
        let mut kinds = Vec::new();
        loop {
            let token = lexer.next_token();
            kinds.push(token.kind);
            if token.kind == TokenKind::Eof {
                break;
            }
        }
        assert!(lexer.errors().is_empty(), "{:?}", lexer.errors());
        kinds
    }

    #[test]
    fn lexes_operators_with_maximal_munch() {
        assert_eq!(
            kinds("a?.b ??= c **= 2 >>>= 1"),
            [
                TokenKind::Identifier,
                TokenKind::QuestionDot,
                TokenKind::Identifier,
                TokenKind::QuestionQuestionEq,
                TokenKind::Identifier,
                TokenKind::StarStarEq,
                TokenKind::Number,
                TokenKind::ShiftRightUnsignedEq,
                TokenKind::Number,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_unicode_and_escaped_identifiers() {
        let mut lexer = Lexer::new("π \\u0061");
        assert_eq!(lexer.next_token().kind, TokenKind::Identifier);
        let escaped = lexer.next_token();
        assert_eq!(escaped.kind, TokenKind::Identifier);
        assert!(escaped.flags.escaped());
        assert!(lexer.errors().is_empty());
    }

    #[test]
    fn lexes_numeric_families() {
        assert_eq!(
            kinds("0xff 0b10_01 0o77 1.5e+2 99n"),
            [
                TokenKind::Number,
                TokenKind::Number,
                TokenKind::Number,
                TokenKind::Number,
                TokenKind::BigInt,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn rescans_regular_expressions() {
        let mut lexer = Lexer::new("/[/\\]]+/giu");
        let slash = lexer.next_token();
        let regexp = lexer.scan_regexp(slash);
        assert_eq!(regexp.kind, TokenKind::RegExp);
        assert_eq!(lexer.source_text(regexp), "/[/\\]]+/giu");
        assert!(lexer.errors().is_empty());
    }

    #[test]
    fn tracks_line_breaks_through_comments() {
        let mut lexer = Lexer::new("return /* first\nsecond */ value");
        assert_eq!(lexer.next_token().kind, TokenKind::Return);
        let value = lexer.next_token();
        assert!(value.flags.line_break_before());
    }
}
