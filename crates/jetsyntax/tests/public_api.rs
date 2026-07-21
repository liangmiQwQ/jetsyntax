use jetsyntax::{
    Language, ParseOptions, SourceKind, parse,
    tape::{NodeTag, TapeValue},
};

/// The native API owns a language-neutral tape and accepts every required source mode.
#[test]
fn parser_should_accept_all_required_languages() {
    let cases = [
        (Language::JavaScript, "const answer = 42;"),
        (Language::TypeScript, "const answer: number = 42;"),
        (
            Language::JavaScriptJsx,
            "const answer = <output>42</output>;",
        ),
        (
            Language::TypeScriptJsx,
            "const answer: JSX.Element = <output>42</output>;",
        ),
    ];

    for (language, source) in cases {
        let parsed = parse(
            source,
            ParseOptions {
                language,
                source_kind: SourceKind::Module,
                ..ParseOptions::default()
            },
        )
        .expect("parse should fit the wire format");
        assert!(
            parsed.diagnostics.is_empty(),
            "{language:?}: {:?}",
            parsed.diagnostics
        );
        let root = parsed
            .tape
            .value_at(parsed.tape.header().root)
            .expect("program root");
        assert!(matches!(
            root,
            TapeValue::Node {
                tag: NodeTag::PROGRAM,
                ..
            }
        ));
    }
}

/// Syntax diagnostics never prevent callers from inspecting a structurally valid recovery tree.
#[test]
fn parser_should_return_a_valid_tape_after_recovery() {
    let parsed = parse("const = ;", ParseOptions::default()).expect("recover parse");
    assert!(!parsed.diagnostics.is_empty());
    parsed.tape.validate().expect("recovery tape remains valid");
}

/// Source type is part of the native tape rather than hidden binding state.
#[test]
fn parser_should_distinguish_script_and_module_programs() {
    for source_kind in [SourceKind::Script, SourceKind::Module, SourceKind::CommonJs] {
        let parsed = parse(
            "0;",
            ParseOptions {
                source_kind,
                ..ParseOptions::default()
            },
        )
        .expect("parse");
        assert_eq!(parsed.tape.header().source_bytes, 2);
    }
}
