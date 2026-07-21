use jetsyntax::lexer::{Lexer, TokenKind};

fn assert_single_token(source: &str, expected: TokenKind) {
    let mut lexer = Lexer::new(source);
    let token = lexer.next_token();
    assert_eq!(token.kind, expected, "{source:?}");
    assert_eq!(lexer.source_text(token), source, "{source:?}");
    assert_eq!(token.len() as usize, source.len(), "{source:?}");
    assert_eq!(lexer.next_token().kind, TokenKind::Eof, "{source:?}");
    assert!(
        lexer.errors().is_empty(),
        "{source:?}: {:?}",
        lexer.errors()
    );
}

/// Every ECMAScript punctuator uses maximal munch and reports its complete source span.
///
/// Spec: ECMAScript lexical punctuators select the longest valid token at the current position.
#[test]
fn lexer_should_recognize_every_punctuator() {
    let cases = [
        ("{", TokenKind::LeftBrace),
        ("}", TokenKind::RightBrace),
        ("(", TokenKind::LeftParen),
        (")", TokenKind::RightParen),
        ("[", TokenKind::LeftBracket),
        ("]", TokenKind::RightBracket),
        (".", TokenKind::Dot),
        ("...", TokenKind::Ellipsis),
        (";", TokenKind::Semicolon),
        (",", TokenKind::Comma),
        (":", TokenKind::Colon),
        ("?", TokenKind::Question),
        ("?.", TokenKind::QuestionDot),
        ("??", TokenKind::QuestionQuestion),
        ("??=", TokenKind::QuestionQuestionEq),
        ("=>", TokenKind::Arrow),
        ("+", TokenKind::Plus),
        ("++", TokenKind::PlusPlus),
        ("+=", TokenKind::PlusEq),
        ("-", TokenKind::Minus),
        ("--", TokenKind::MinusMinus),
        ("-=", TokenKind::MinusEq),
        ("*", TokenKind::Star),
        ("**", TokenKind::StarStar),
        ("*=", TokenKind::StarEq),
        ("**=", TokenKind::StarStarEq),
        ("/", TokenKind::Slash),
        ("/=", TokenKind::SlashEq),
        ("%", TokenKind::Percent),
        ("%=", TokenKind::PercentEq),
        ("&", TokenKind::Amp),
        ("&&", TokenKind::AmpAmp),
        ("&=", TokenKind::AmpEq),
        ("&&=", TokenKind::AmpAmpEq),
        ("|", TokenKind::Pipe),
        ("||", TokenKind::PipePipe),
        ("|=", TokenKind::PipeEq),
        ("||=", TokenKind::PipePipeEq),
        ("^", TokenKind::Caret),
        ("^=", TokenKind::CaretEq),
        ("!", TokenKind::Bang),
        ("!=", TokenKind::BangEq),
        ("!==", TokenKind::BangEqEq),
        ("=", TokenKind::Eq),
        ("==", TokenKind::EqEq),
        ("===", TokenKind::EqEqEq),
        ("<", TokenKind::Lt),
        ("<=", TokenKind::LtEq),
        ("<<", TokenKind::ShiftLeft),
        ("<<=", TokenKind::ShiftLeftEq),
        (">", TokenKind::Gt),
        (">=", TokenKind::GtEq),
        (">>", TokenKind::ShiftRight),
        (">>=", TokenKind::ShiftRightEq),
        (">>>", TokenKind::ShiftRightUnsigned),
        (">>>=", TokenKind::ShiftRightUnsignedEq),
        ("~", TokenKind::Tilde),
        ("@", TokenKind::At),
        ("#", TokenKind::Hash),
    ];

    for (source, expected) in cases {
        assert_single_token(source, expected);
    }

    let mut lexer = Lexer::new("?.1");
    assert_eq!(lexer.next_token().kind, TokenKind::Question);
    assert_eq!(lexer.next_token().kind, TokenKind::Number);
    assert!(lexer.errors().is_empty(), "{:?}", lexer.errors());
}

/// Reserved words and contextual JavaScript/TypeScript words retain distinct token kinds.
///
/// Spec: keyword recognition applies only to an exact, unescaped IdentifierName spelling.
#[test]
fn lexer_should_recognize_keywords_and_contextual_typescript_words() {
    let cases = [
        ("break", TokenKind::Break),
        ("case", TokenKind::Case),
        ("catch", TokenKind::Catch),
        ("class", TokenKind::Class),
        ("const", TokenKind::Const),
        ("continue", TokenKind::Continue),
        ("debugger", TokenKind::Debugger),
        ("default", TokenKind::Default),
        ("delete", TokenKind::Delete),
        ("do", TokenKind::Do),
        ("else", TokenKind::Else),
        ("export", TokenKind::Export),
        ("extends", TokenKind::Extends),
        ("false", TokenKind::False),
        ("finally", TokenKind::Finally),
        ("for", TokenKind::For),
        ("function", TokenKind::Function),
        ("if", TokenKind::If),
        ("import", TokenKind::Import),
        ("in", TokenKind::In),
        ("instanceof", TokenKind::Instanceof),
        ("new", TokenKind::New),
        ("null", TokenKind::Null),
        ("return", TokenKind::Return),
        ("super", TokenKind::Super),
        ("switch", TokenKind::Switch),
        ("this", TokenKind::This),
        ("throw", TokenKind::Throw),
        ("true", TokenKind::True),
        ("try", TokenKind::Try),
        ("typeof", TokenKind::Typeof),
        ("var", TokenKind::Var),
        ("void", TokenKind::Void),
        ("while", TokenKind::While),
        ("with", TokenKind::With),
        ("yield", TokenKind::Yield),
        ("async", TokenKind::Async),
        ("await", TokenKind::Await),
        ("let", TokenKind::Let),
        ("static", TokenKind::Static),
        ("of", TokenKind::Of),
        ("get", TokenKind::Get),
        ("set", TokenKind::Set),
        ("as", TokenKind::As),
        ("satisfies", TokenKind::Satisfies),
        ("accessor", TokenKind::Accessor),
        ("using", TokenKind::Using),
        ("declare", TokenKind::Declare),
        ("abstract", TokenKind::Abstract),
        ("interface", TokenKind::Interface),
        ("type", TokenKind::Type),
        ("enum", TokenKind::Enum),
        ("namespace", TokenKind::Namespace),
        ("module", TokenKind::Module),
        ("implements", TokenKind::Implements),
        ("infer", TokenKind::Infer),
        ("keyof", TokenKind::Keyof),
        ("readonly", TokenKind::Readonly),
        ("unique", TokenKind::Unique),
        ("unknown", TokenKind::Unknown),
        ("never", TokenKind::Never),
        ("any", TokenKind::Any),
        ("boolean", TokenKind::Boolean),
        ("number", TokenKind::NumberKeyword),
        ("string", TokenKind::StringKeyword),
        ("symbol", TokenKind::Symbol),
        ("object", TokenKind::Object),
        ("undefined", TokenKind::Undefined),
        ("is", TokenKind::Is),
        ("asserts", TokenKind::Asserts),
        ("public", TokenKind::Public),
        ("protected", TokenKind::Protected),
        ("private", TokenKind::Private),
        ("override", TokenKind::Override),
        ("out", TokenKind::Out),
        ("meta", TokenKind::Meta),
        ("from", TokenKind::From),
        ("require", TokenKind::Require),
    ];

    for (source, expected) in cases {
        assert_single_token(source, expected);
    }

    assert_single_token("returning", TokenKind::Identifier);
    let mut escaped = Lexer::new(r"\u0072eturn");
    let token = escaped.next_token();
    assert_eq!(token.kind, TokenKind::Identifier);
    assert!(token.flags.escaped());
    assert!(escaped.errors().is_empty(), "{:?}", escaped.errors());
}

/// Numeric literals cover every radix, decimal form, BigInt, and metadata flag.
///
/// Spec: separators occur only between digits, while fractions, exponents, and legacy octal
/// spelling affect the literal's permitted suffixes.
#[test]
fn lexer_should_recognize_numeric_literal_families_and_flags() {
    let cases = [
        ("0", TokenKind::Number, false, false),
        ("42", TokenKind::Number, false, false),
        (".5", TokenKind::Number, false, false),
        ("1.", TokenKind::Number, false, false),
        ("6.02e23", TokenKind::Number, false, false),
        ("1E-9", TokenKind::Number, false, false),
        ("0xff", TokenKind::Number, false, false),
        ("0B1010_0001", TokenKind::Number, true, false),
        ("0o755", TokenKind::Number, false, false),
        ("1_000_000", TokenKind::Number, true, false),
        ("077", TokenKind::Number, false, true),
        ("0n", TokenKind::BigInt, false, false),
        ("123n", TokenKind::BigInt, false, false),
        ("0xfeedn", TokenKind::BigInt, false, false),
        ("0b1010_0001n", TokenKind::BigInt, true, false),
    ];

    for (source, expected, separator, legacy_octal) in cases {
        let mut lexer = Lexer::new(source);
        let token = lexer.next_token();
        assert_eq!(token.kind, expected, "{source:?}");
        assert_eq!(lexer.source_text(token), source, "{source:?}");
        assert_eq!(token.flags.contains_separator(), separator, "{source:?}");
        assert_eq!(token.flags.legacy_octal(), legacy_octal, "{source:?}");
        assert!(
            lexer.errors().is_empty(),
            "{source:?}: {:?}",
            lexer.errors()
        );
    }
}

/// String literals retain their raw span across escapes and line continuations.
///
/// Spec: a quoted string may contain escape sequences and escaped line terminators, but an
/// unescaped line terminator cannot occur before its closing quote.
#[test]
fn lexer_should_recognize_strings_and_line_continuations() {
    let valid = [
        "'plain'",
        r#""double quoted""#,
        r"'escaped\' quote'",
        r#""escaped\" quote""#,
        r"'unicode \u{1f600}'",
        "'continued\\\nline'",
        "\"continued\\\r\nline\"",
    ];

    for source in valid {
        assert_single_token(source, TokenKind::String);
    }
}

/// Template rescanning separates heads, middles, and tails around substitutions.
///
/// Spec: the parser resumes template scanning after the `}` that closes each substitution.
#[test]
fn lexer_should_scan_template_segments_on_demand() {
    assert_single_token("`plain`", TokenKind::NoSubstitutionTemplate);

    let source = "`before ${value} between ${other} after`";
    let mut lexer = Lexer::new(source);
    let head = lexer.next_token();
    assert_eq!(head.kind, TokenKind::TemplateHead);
    assert_eq!(lexer.source_text(head), "`before ${");
    assert_eq!(lexer.next_token().kind, TokenKind::Identifier);
    let first_brace = lexer.next_token();
    assert_eq!(first_brace.kind, TokenKind::RightBrace);
    let middle = lexer.resume_template(first_brace);
    assert_eq!(middle.kind, TokenKind::TemplateMiddle);
    assert_eq!(lexer.source_text(middle), "} between ${");
    assert_eq!(lexer.next_token().kind, TokenKind::Identifier);
    let second_brace = lexer.next_token();
    assert_eq!(second_brace.kind, TokenKind::RightBrace);
    let tail = lexer.resume_template(second_brace);
    assert_eq!(tail.kind, TokenKind::TemplateTail);
    assert_eq!(lexer.source_text(tail), "} after`");
    assert_eq!(lexer.next_token().kind, TokenKind::Eof);
    assert!(lexer.errors().is_empty(), "{:?}", lexer.errors());
}

/// A slash becomes a regular expression only after an explicit grammar-directed rescan.
///
/// Spec: regular-expression bodies honor escapes and character classes before consuming flags.
#[test]
fn lexer_should_rescan_regular_expressions_with_classes_escapes_and_flags() {
    let cases = [
        r"/answer+/giu",
        r"/[/\]]+/u",
        r"/a\/b/dv",
        r"/(?:x|y){2,4}/m",
    ];

    for source in cases {
        let mut lexer = Lexer::new(source);
        let slash = lexer.next_token();
        assert_eq!(slash.kind, TokenKind::Slash, "{source:?}");
        let regexp = lexer.scan_regexp(slash);
        assert_eq!(regexp.kind, TokenKind::RegExp, "{source:?}");
        assert_eq!(lexer.source_text(regexp), source, "{source:?}");
        assert_eq!(lexer.next_token().kind, TokenKind::Eof, "{source:?}");
        assert!(
            lexer.errors().is_empty(),
            "{source:?}: {:?}",
            lexer.errors()
        );
    }
}

/// Unicode source characters and Unicode escapes follow identifier start/continue positions.
///
/// Spec: identifier escapes may use fixed or braced Unicode syntax and escaped keywords remain
/// identifiers for the parser to validate contextually.
#[test]
fn lexer_should_recognize_unicode_identifiers_and_escapes() {
    let cases = [
        ("π", false),
        ("变量", false),
        ("a\u{200c}b", false),
        (r"\u0061", true),
        (r"\u{10400}", true),
        (r"a\u{200d}", true),
        (r"\u0069f", true),
    ];

    for (source, escaped) in cases {
        let mut lexer = Lexer::new(source);
        let token = lexer.next_token();
        assert_eq!(token.kind, TokenKind::Identifier, "{source:?}");
        assert_eq!(lexer.source_text(token), source, "{source:?}");
        assert_eq!(token.flags.escaped(), escaped, "{source:?}");
        assert!(
            lexer.errors().is_empty(),
            "{source:?}: {:?}",
            lexer.errors()
        );
    }

    assert_single_token("#field", TokenKind::PrivateIdentifier);
}

/// Private identifiers accept the same Unicode starts and escapes as ordinary identifiers.
///
/// Spec: the code point after `#` is an IdentifierStart or Unicode escape, not an ASCII-only name.
#[test]
fn lexer_should_recognize_unicode_private_identifiers() {
    for source in ["#π", r"#\u0061"] {
        assert_single_token(source, TokenKind::PrivateIdentifier);
    }
}

/// Trivia is omitted while every ECMAScript line terminator reaches the next token's flags.
///
/// Spec: line comments, multiline comments, hashbangs, CRLF, LS, and PS contribute line
/// terminators; ordinary whitespace and single-line block comments do not.
#[test]
fn lexer_should_track_line_breaks_across_whitespace_and_comments() {
    let cases = [
        (" value", false),
        ("\u{a0}value", false),
        ("/* same line */value", false),
        ("// comment\nvalue", true),
        ("/* first\nsecond */value", true),
        ("\r\nvalue", true),
        ("\u{2028}value", true),
        ("\u{2029}value", true),
        ("#!/usr/bin/env node\nvalue", true),
    ];

    for (source, expected) in cases {
        let mut lexer = Lexer::new(source);
        let token = lexer.next_token();
        assert_eq!(token.kind, TokenKind::Identifier, "{source:?}");
        assert_eq!(token.flags.line_break_before(), expected, "{source:?}");
        assert!(
            lexer.errors().is_empty(),
            "{source:?}: {:?}",
            lexer.errors()
        );
    }
}

/// Unicode line and paragraph separators terminate line comments and count inside block comments.
///
/// Spec: LF, CR, LS, and PS are all ECMAScript LineTerminator code points in comment trivia.
#[test]
fn lexer_should_track_unicode_line_terminators_inside_comments() {
    for source in [
        "// comment\u{2028}value",
        "// comment\u{2029}value",
        "/* first\u{2028}second */value",
        "/* first\u{2029}second */value",
    ] {
        let mut lexer = Lexer::new(source);
        let token = lexer.next_token();
        assert_eq!(token.kind, TokenKind::Identifier, "{source:?}");
        assert!(token.flags.line_break_before(), "{source:?}");
        assert!(
            lexer.errors().is_empty(),
            "{source:?}: {:?}",
            lexer.errors()
        );
    }
}

/// Invalid lexical forms recover with a token and at least one bounded diagnostic.
///
/// Spec: malformed literals, identifier escapes, comments, and source characters are lexical
/// errors even when tokenization continues for parser recovery.
#[test]
fn lexer_should_report_invalid_lexical_forms() {
    let cases = [
        ("\0", TokenKind::Invalid),
        ("🙂", TokenKind::Invalid),
        ("'unterminated", TokenKind::String),
        ("'line\nbreak'", TokenKind::String),
        ("`unterminated", TokenKind::TemplateTail),
        ("0x", TokenKind::Number),
        ("0x_ff", TokenKind::Number),
        ("1__0", TokenKind::Number),
        ("1_", TokenKind::Number),
        ("1e+", TokenKind::Number),
        ("1.0n", TokenKind::BigInt),
        ("077n", TokenKind::BigInt),
        ("123abc", TokenKind::Number),
        (r"\x", TokenKind::Invalid),
        (r"\u{}", TokenKind::Invalid),
        (r"\u0030name", TokenKind::Invalid),
        ("/* unterminated", TokenKind::Eof),
    ];

    for (source, expected) in cases {
        let mut lexer = Lexer::new(source);
        let token = lexer.next_token();
        assert_eq!(token.kind, expected, "{source:?}");
        assert!(!lexer.errors().is_empty(), "{source:?}");
        assert!(
            lexer.errors().iter().all(|error| {
                error.start <= error.end
                    && error.end as usize <= source.len()
                    && !error.message.is_empty()
            }),
            "{source:?}: {:?}",
            lexer.errors()
        );
    }

    let mut lexer = Lexer::new("/[unterminated");
    let slash = lexer.next_token();
    assert_eq!(lexer.scan_regexp(slash).kind, TokenKind::RegExp);
    assert!(!lexer.errors().is_empty());
}

/// Unescaped Unicode line terminators cannot occur in strings or regular-expression bodies.
///
/// Spec: LS and PS terminate these lexical goals just like LF and CR.
#[test]
fn lexer_should_reject_unicode_line_terminators_inside_literals() {
    for source in ["'before\u{2028}after'", "\"before\u{2029}after\""] {
        let mut lexer = Lexer::new(source);
        assert_eq!(lexer.next_token().kind, TokenKind::String, "{source:?}");
        assert!(!lexer.errors().is_empty(), "{source:?}");
    }

    for source in ["/before\u{2028}after/", "/before\u{2029}after/"] {
        let mut lexer = Lexer::new(source);
        let slash = lexer.next_token();
        assert_eq!(lexer.scan_regexp(slash).kind, TokenKind::RegExp);
        assert!(!lexer.errors().is_empty(), "{source:?}");
    }
}

/// Digits outside a prefixed literal's radix cannot silently begin another numeric token.
///
/// Spec: binary and octal integer literals reject decimal digits that are not in their radix.
#[test]
fn lexer_should_reject_digits_outside_the_numeric_radix() {
    for source in ["0b102", "0o78"] {
        let mut lexer = Lexer::new(source);
        assert_eq!(lexer.next_token().kind, TokenKind::Number, "{source:?}");
        assert!(!lexer.errors().is_empty(), "{source:?}");
    }
}

/// Recovery tokens and diagnostics never point beyond the original source.
///
/// Spec: an out-of-range Unicode code point is invalid, and recovery spans remain bounded by the
/// source text so downstream diagnostic rendering is safe.
#[test]
fn lexer_should_bound_out_of_range_unicode_escape_diagnostics() {
    let source = r"\u{110000}";
    let mut lexer = Lexer::new(source);
    let token = lexer.next_token();
    assert_eq!(token.kind, TokenKind::Invalid);
    assert!(token.end as usize <= source.len(), "{token:?}");
    assert!(!lexer.errors().is_empty());
    assert!(
        lexer
            .errors()
            .iter()
            .all(|error| error.end as usize <= source.len()),
        "{:?}",
        lexer.errors()
    );
}

/// JSX text remains raw until a tag or expression delimiter is encountered.
///
/// Spec: JSX child text includes whitespace and entity spellings and stops before `<` or `{`.
#[test]
fn lexer_should_scan_jsx_text_without_interpreting_entities() {
    let source = "hello &amp;\nworld<div>child {value}";
    let mut lexer = Lexer::new(source);
    let first = lexer.next_jsx_text();
    assert_eq!(first.kind, TokenKind::JsxText);
    assert_eq!(lexer.source_text(first), "hello &amp;\nworld");
    assert_eq!(lexer.next_token().kind, TokenKind::Lt);
    assert_eq!(lexer.next_token().kind, TokenKind::Identifier);
    assert_eq!(lexer.next_token().kind, TokenKind::Gt);
    let child = lexer.next_jsx_text();
    assert_eq!(child.kind, TokenKind::JsxText);
    assert_eq!(lexer.source_text(child), "child ");
    assert_eq!(lexer.next_token().kind, TokenKind::LeftBrace);
    assert!(lexer.errors().is_empty(), "{:?}", lexer.errors());
}
