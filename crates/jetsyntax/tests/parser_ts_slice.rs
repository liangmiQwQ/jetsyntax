use jetsyntax::{
    Language, ParseOptions, ParseResult, SourceKind, SyntaxExtensions, parse,
    tape::{NodeTag, TapeValue},
};

#[test]
fn parses_typescript_type_families_without_diagnostics() {
    let cases = [
        (
            "keyword types",
            "let text: string; let count: number; let flag: boolean; let token: symbol; let large: bigint; let anything: any; let mystery: unknown; let impossible: never; let objectValue: object; let absent: undefined; let empty: null; let none: void;",
            &[
                NodeTag::TS_STRING_KEYWORD,
                NodeTag::TS_NUMBER_KEYWORD,
                NodeTag::TS_BOOLEAN_KEYWORD,
                NodeTag::TS_SYMBOL_KEYWORD,
                NodeTag::TS_BIGINT_KEYWORD,
                NodeTag::TS_ANY_KEYWORD,
                NodeTag::TS_UNKNOWN_KEYWORD,
                NodeTag::TS_NEVER_KEYWORD,
                NodeTag::TS_OBJECT_KEYWORD,
                NodeTag::TS_UNDEFINED_KEYWORD,
                NodeTag::TS_NULL_KEYWORD,
                NodeTag::TS_VOID_KEYWORD,
            ][..],
        ),
        (
            "qualified reference",
            "const value: Namespace.Model = input;",
            &[
                NodeTag::TS_TYPE_ANNOTATION,
                NodeTag::TS_TYPE_REFERENCE,
                NodeTag::TS_QUALIFIED_NAME,
            ][..],
        ),
        (
            "union and intersection",
            "let value: (Text | Number) & Serializable;",
            &[
                NodeTag::TS_PARENTHESIZED_TYPE,
                NodeTag::TS_UNION_TYPE,
                NodeTag::TS_INTERSECTION_TYPE,
            ][..],
        ),
        (
            "literal types",
            "let state: 'ready' | 'waiting' | 0 | true;",
            &[NodeTag::TS_LITERAL_TYPE, NodeTag::TS_UNION_TYPE][..],
        ),
        (
            "array and tuple types",
            "let list: readonly string[]; let tuple: [name: string, count?: number, ...rest: boolean[]];",
            &[
                NodeTag::TS_TYPE_OPERATOR,
                NodeTag::TS_ARRAY_TYPE,
                NodeTag::TS_TUPLE_TYPE,
                NodeTag::TS_NAMED_TUPLE_MEMBER,
            ][..],
        ),
        (
            "function type",
            "let callback: <T>(value: T, index?: number) => Promise<T>;",
            &[
                NodeTag::TS_FUNCTION_TYPE,
                NodeTag::TS_TYPE_PARAMETER_DECLARATION,
                NodeTag::TS_TYPE_PARAMETER_INSTANTIATION,
            ][..],
        ),
        (
            "conditional and indexed access",
            "let selected: T extends readonly unknown[] ? T[number] : never;",
            &[
                NodeTag::TS_CONDITIONAL_TYPE,
                NodeTag::TS_INDEXED_ACCESS_TYPE,
            ][..],
        ),
        (
            "infer type",
            "let element: T extends (infer Element)[] ? Element : never;",
            &[NodeTag::TS_INFER_TYPE, NodeTag::TS_TYPE_PARAMETER][..],
        ),
        (
            "mapped type",
            "let clone: { readonly [Key in keyof Source]?: Source[Key] };",
            &[NodeTag::TS_MAPPED_TYPE, NodeTag::TS_TYPE_OPERATOR][..],
        ),
        (
            "object type",
            "let service: { readonly name: string; method(input: number): void; new (): Service };",
            &[
                NodeTag::TS_TYPE_LITERAL,
                NodeTag::TS_PROPERTY_SIGNATURE,
                NodeTag::TS_METHOD_SIGNATURE,
            ][..],
        ),
    ];

    for (name, source, expected_tags) in cases {
        assert_clean_with_tags(name, source, expected_tags);
    }
}

#[test]
fn parses_named_typescript_declarations_and_nested_generics() {
    assert_clean_with_tags(
        "nested generic reference",
        "const value: Promise<ReadonlyArray<Map<string, number>>> = input;",
        &[
            NodeTag::TS_TYPE_ANNOTATION,
            NodeTag::TS_TYPE_REFERENCE,
            NodeTag::TS_TYPE_PARAMETER_INSTANTIATION,
        ],
    );
    assert_clean_with_tags(
        "type alias",
        "type Result<Value, Error = unknown> = { ok: true; value: Value } | { ok: false; error: Error };",
        &[
            NodeTag::TS_TYPE_ALIAS_DECLARATION,
            NodeTag::TS_TYPE_PARAMETER,
            NodeTag::TS_TYPE_PARAMETER_DECLARATION,
            NodeTag::TS_UNION_TYPE,
        ],
    );
    assert_clean_with_tags(
        "interface",
        "interface Repository<T> extends Base<T> { readonly value: T; get<Key extends keyof T>(key: Key): T[Key]; }",
        &[
            NodeTag::TS_INTERFACE_DECLARATION,
            NodeTag::TS_INTERFACE_BODY,
            NodeTag::TS_INTERFACE_HERITAGE,
            NodeTag::TS_PROPERTY_SIGNATURE,
            NodeTag::TS_METHOD_SIGNATURE,
        ],
    );
    assert_clean_with_tags(
        "enums",
        "enum Direction { Up, Down = 4 } const enum Flag { Read = 1, Write = 2 }",
        &[
            NodeTag::TS_ENUM_DECLARATION,
            NodeTag::TS_ENUM_BODY,
            NodeTag::TS_ENUM_MEMBER,
        ],
    );
    assert_clean_with_tags(
        "namespace",
        "namespace Library.Core { export interface Item { id: string } export const version = '1'; }",
        &[
            NodeTag::TS_MODULE_DECLARATION,
            NodeTag::TS_MODULE_BLOCK,
            NodeTag::TS_INTERFACE_DECLARATION,
            NodeTag::EXPORT_NAMED_DECLARATION,
        ],
    );
}

#[test]
fn keeps_type_bindings_separate_from_parent_value_scopes() {
    let source = "function convert(value) { type value = number; } try {} catch (error) { type error = unknown; }";
    let parsed = parse(
        source,
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("parse separate type and value bindings");

    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed.tape.validate().expect("valid type-binding tape");
    assert_eq!(
        node_fields(&parsed, NodeTag::TS_TYPE_ALIAS_DECLARATION).count(),
        2
    );
}

#[test]
fn parses_block_function_return_annotations() {
    let source = [
        "function convert(value: Input): Namespace.Output { return value; }",
        "const convertLater = function (value: Input): Output | undefined { return value; };",
        "async function load(): Promise<Result> { return await request(); }",
        "function* values(): Iterable<Result> { yield result; }",
        "function plain() {}",
    ]
    .join("\n");
    let parsed = parse(&source, typescript_options()).expect("parse function return annotations");
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed
        .tape
        .validate()
        .expect("valid return-annotation tape");

    let declarations = node_fields(&parsed, NodeTag::FUNCTION_DECLARATION).collect::<Vec<_>>();
    assert_eq!(declarations.len(), 4);
    assert_eq!(declarations[0].len(), 6);
    assert_eq!(declarations[1].len(), 6);
    assert_eq!(declarations[2].len(), 6);
    assert_eq!(declarations[3].len(), 5);

    let expressions = node_fields(&parsed, NodeTag::FUNCTION_EXPRESSION).collect::<Vec<_>>();
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].len(), 6);

    for fields in declarations[..3].iter().chain(&expressions) {
        let annotation = parsed.tape.value_at(fields[5]).expect("return annotation");
        assert!(matches!(
            annotation,
            TapeValue::Node {
                tag: NodeTag::TS_TYPE_ANNOTATION,
                ..
            }
        ));
    }

    let TapeValue::Node { span, .. } = parsed
        .tape
        .value_at(declarations[0][5])
        .expect("declaration return annotation")
    else {
        panic!("return annotation is not a node");
    };
    assert_eq!(
        span.start as usize,
        source.find(": Namespace.Output").unwrap()
    );
    assert_eq!(
        &source[span.start as usize..span.end as usize],
        ": Namespace.Output"
    );

    let definition = parse(
        "function typed(): string {}",
        ParseOptions {
            language: Language::TypeScriptDefinition,
            semantic_errors: true,
            ..ParseOptions::default()
        },
    )
    .expect("parse definition-file typed function");
    assert!(!definition.diagnostics.is_empty());
    assert_node_field_count(&definition, NodeTag::FUNCTION_DECLARATION, 6);

    let tsx = parse(
        "function typed(): string {}",
        ParseOptions {
            language: Language::TypeScriptJsx,
            ..ParseOptions::default()
        },
    )
    .expect("parse TSX typed function");
    assert!(tsx.diagnostics.is_empty());
    assert_node_field_count(&tsx, NodeTag::FUNCTION_DECLARATION, 6);
}

#[test]
fn parses_runtime_function_type_parameters() {
    let source = [
        "function convert<T extends Input, U = T>(value: T): U { return value; }",
        "async function load<T>(value: T) { return value; }",
        "function* values<T>() { yield value; }",
        "const later = function<const T>(value: T) { return value; };",
    ]
    .join("\n");
    let parsed = parse(&source, typescript_options()).expect("parse function type parameters");
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed
        .tape
        .validate()
        .expect("valid function type-parameter tape");

    let declarations = node_fields(&parsed, NodeTag::FUNCTION_DECLARATION).collect::<Vec<_>>();
    assert_eq!(declarations.len(), 3);
    assert!(declarations.iter().all(|fields| fields.len() == 7));
    assert!(matches!(
        parsed.tape.value_at(declarations[0][5]),
        Ok(TapeValue::Node {
            tag: NodeTag::TS_TYPE_ANNOTATION,
            ..
        })
    ));
    assert!(matches!(
        parsed.tape.value_at(declarations[1][5]),
        Ok(TapeValue::Null)
    ));
    assert!(matches!(
        parsed.tape.value_at(declarations[2][3]),
        Ok(TapeValue::Bool(true))
    ));

    let expressions = node_fields(&parsed, NodeTag::FUNCTION_EXPRESSION).collect::<Vec<_>>();
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].len(), 7);
    assert!(matches!(
        parsed.tape.value_at(expressions[0][5]),
        Ok(TapeValue::Null)
    ));
    for fields in declarations.iter().chain(&expressions) {
        assert!(matches!(
            parsed.tape.value_at(fields[6]),
            Ok(TapeValue::Node {
                tag: NodeTag::TS_TYPE_PARAMETER_DECLARATION,
                ..
            })
        ));
    }
}

#[test]
fn parses_top_level_typescript_function_signatures_and_restores_context() {
    let source = [
        "export function overloaded<T>(value: T): T;",
        "export function overloaded(value: string): string;",
        "function overloaded(value) { return value; }",
        "function following(): void {}",
        "export { overloaded };",
    ]
    .join("\n");
    let parsed = parse(
        &source,
        ParseOptions {
            source_kind: SourceKind::Module,
            ..typescript_options()
        },
    )
    .expect("parse top-level function signatures");

    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed
        .tape
        .validate()
        .expect("valid function-signature tape");
    assert_eq!(NodeTag::TS_DECLARE_FUNCTION.get(), 572);
    let signatures = node_fields(&parsed, NodeTag::TS_DECLARE_FUNCTION).collect::<Vec<_>>();
    assert_eq!(signatures.len(), 2);
    assert!(signatures.iter().all(|fields| fields.len() == 6));
    assert!(matches!(
        parsed.tape.value_at(signatures[0][5]),
        Ok(TapeValue::Node {
            tag: NodeTag::TS_TYPE_PARAMETER_DECLARATION,
            ..
        })
    ));
    assert!(matches!(
        parsed.tape.value_at(signatures[1][5]),
        Ok(TapeValue::Null)
    ));
    let TapeValue::Node { span, .. } = parsed
        .tape
        .value_at(signatures[0][4])
        .expect("signature return annotation")
    else {
        panic!("signature return annotation is not a node");
    };
    assert_eq!(&source[span.start as usize..span.end as usize], ": T");
    assert_eq!(
        node_fields(&parsed, NodeTag::FUNCTION_DECLARATION).count(),
        2
    );
    assert_eq!(
        node_fields(&parsed, NodeTag::EXPORT_NAMED_DECLARATION).count(),
        3
    );
}

#[test]
fn parses_nested_generator_async_and_asi_function_signatures() {
    let source = [
        "async function asynchronous(): Promise<void>;",
        "function* generator(): Iterable<void>;",
        "function outer() { function nested(): void; }",
        "function lineBreak(): void",
        "function following() {}",
        "function eof(): void",
    ]
    .join("\n");
    let parsed = parse(&source, typescript_options()).expect("parse extended function signatures");
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed
        .tape
        .validate()
        .expect("valid extended-signature tape");

    let signatures = node_fields(&parsed, NodeTag::TS_DECLARE_FUNCTION).collect::<Vec<_>>();
    assert_eq!(signatures.len(), 5);
    assert!(
        signatures
            .iter()
            .any(|fields| matches!(parsed.tape.value_at(fields[3]), Ok(TapeValue::Bool(true))))
    );
    assert!(
        signatures
            .iter()
            .any(|fields| matches!(parsed.tape.value_at(fields[2]), Ok(TapeValue::Bool(true))))
    );
}

#[test]
fn parses_explicit_declared_functions_on_cold_signature_records() {
    let source = [
        "declare function plain(value: string): void;",
        "declare function* generated<T>(...values: T[],): Iterable<T>",
        "declare async function asynchronous(): Promise<void>;",
        "declare async function* asynchronousGenerator<T>(): AsyncIterable<T>;",
        "function outer() { declare function nested(): void; }",
        "export declare function exported<T>(): T;",
        "function overload(): void;",
        "declare function eof(): void",
    ]
    .join("\n");
    let parsed = parse(
        &source,
        ParseOptions {
            source_kind: SourceKind::Module,
            ..typescript_options()
        },
    )
    .expect("parse explicit declared functions");

    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed
        .tape
        .validate()
        .expect("valid explicit-declare-function tape");
    let declarations = parsed
        .tape
        .validation()
        .map(|record| record.expect("valid record").value)
        .filter_map(|value| match value {
            TapeValue::Node {
                tag: NodeTag::TS_EXPLICIT_DECLARE_FUNCTION,
                flags: 0,
                span,
                fields,
                ..
            } => Some((span, fields.to_vec())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(declarations.len(), 7);
    assert_eq!(NodeTag::TS_EXPLICIT_DECLARE_FUNCTION.get(), 578);
    assert!(declarations.iter().all(|(_, fields)| fields.len() == 7));
    assert!(
        declarations
            .iter()
            .all(|(_, fields)| { matches!(parsed.tape.value_at(fields[6]), Ok(TapeValue::Null)) })
    );
    assert!(declarations.iter().all(|(span, _)| {
        source[span.start as usize..span.end as usize].starts_with("declare")
    }));
    assert!(parsed.tape.validation().any(|record| {
        matches!(
            record.expect("valid record").value,
            TapeValue::Node {
                tag: NodeTag::TS_DECLARE_FUNCTION,
                flags: 0,
                ..
            }
        )
    }));
    let export = first_node_fields(&parsed, NodeTag::EXPORT_NAMED_DECLARATION);
    assert!(matches!(
        parsed.tape.value_at(export[4]),
        Ok(TapeValue::U32(1))
    ));
}

#[test]
fn keeps_explicit_declare_function_contextual_and_restores_ambient_grammar() {
    for source in [
        "declare\nfunction separated(): void;",
        "declare async\nfunction separated(): void;",
        "declar\\u0065 function escaped(): void;",
        "declare f\\u0075nction escaped(): void;",
        "declare as\\u0079nc function escaped(): void;",
        "export declare\nfunction separated(): void;",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                source_kind: SourceKind::Module,
                ..typescript_options()
            },
        )
        .expect("recover contextual declare function");
        parsed.tape.validate().expect("valid contextual tape");
        assert!(
            parsed.tape.validation().all(|record| {
                !matches!(
                    record.expect("valid record").value,
                    TapeValue::Node {
                        tag: NodeTag::TS_EXPLICIT_DECLARE_FUNCTION,
                        ..
                    }
                )
            }),
            "{source}"
        );
    }

    let recovered = parse(
        "declare function initialized(value = 1): void; declare function implemented() {} function ordinary() {}",
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("recover invalid explicit declared functions");
    recovered.tape.validate().expect("valid recovered tape");
    assert_eq!(
        recovered
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message.contains("ambient contexts"))
            .count(),
        2,
        "{:#?}",
        recovered.diagnostics
    );
    let explicit =
        node_fields(&recovered, NodeTag::TS_EXPLICIT_DECLARE_FUNCTION).collect::<Vec<_>>();
    assert_eq!(explicit.len(), 2);
    assert!(matches!(
        recovered.tape.value_at(explicit[0][6]),
        Ok(TapeValue::Null)
    ));
    assert!(matches!(
        recovered.tape.value_at(explicit[1][6]),
        Ok(TapeValue::Node {
            tag: NodeTag::BLOCK_STATEMENT,
            ..
        })
    ));
    assert_eq!(
        node_fields(&recovered, NodeTag::FUNCTION_DECLARATION).count(),
        1
    );

    let restoration_source =
        "declare function eval(arguments: unknown): void; function arguments() {}";
    let ordinary_start = restoration_source
        .find("function arguments")
        .expect("ordinary function offset");
    let restoration = parse(
        restoration_source,
        ParseOptions {
            source_kind: SourceKind::Module,
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("restore strict module grammar after explicit declaration");
    assert!(!restoration.diagnostics.is_empty());
    assert!(
        restoration
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.span.start as usize >= ordinary_start)
    );
}

#[test]
fn diagnoses_async_and_generator_modifiers_in_ambient_functions() {
    let parsed = parse(
        "declare async function asynchronous(): void; declare function* generated(): void; declare async function* both(): void;",
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("recover invalid ambient function modifiers");
    assert_eq!(parsed.diagnostics.len(), 4);
    assert_eq!(
        parsed
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message.contains("async functions"))
            .count(),
        2
    );
    assert_eq!(
        parsed
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message.contains("generators"))
            .count(),
        2
    );
}

#[test]
fn permits_rest_trailing_commas_only_in_typescript_signatures() {
    for source in [
        "declare function explicit(...values: unknown[], );",
        "function overload(...values: unknown[], ): void;",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                semantic_errors: true,
                ..typescript_options()
            },
        )
        .expect("parse TypeScript signature trailing comma");
        assert!(
            parsed.diagnostics.is_empty(),
            "{source}: {:#?}",
            parsed.diagnostics
        );
    }

    for (source, options) in [
        (
            "function runtime(...values: unknown[], ) {}",
            ParseOptions {
                semantic_errors: true,
                ..typescript_options()
            },
        ),
        (
            "function javascript(...values, ) {}",
            ParseOptions {
                semantic_errors: true,
                ..ParseOptions::default()
            },
        ),
        (
            "class C { method(...values: unknown[], ): void; }",
            ParseOptions {
                semantic_errors: true,
                ..typescript_options()
            },
        ),
    ] {
        let parsed = parse(source, options).expect("recover runtime rest trailing comma");
        assert!(!parsed.diagnostics.is_empty(), "{source}");
    }

    for source in [
        "declare function explicit(...values: unknown[], ) {}",
        "declare namespace N { class C { method(...values: unknown[], ) {} } }",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                semantic_errors: true,
                ..typescript_options()
            },
        )
        .expect("recover an ambient implementation with a rest trailing comma");
        assert_eq!(
            parsed.diagnostics.len(),
            1,
            "{source}: {:#?}",
            parsed.diagnostics
        );
        assert!(parsed.diagnostics[0].message.contains("implementation"));
    }
}

#[test]
fn parses_explicit_declared_enums_with_existing_enum_records() {
    let source = [
        "declare enum Direction { Up, Down = 2 }",
        "declare const\nenum ConstantDirection { Up = calculate() }",
        "export declare enum ExportedDirection { Up }",
        "export declare const enum ExportedConstantDirection { Up }",
        "enum OrdinaryDirection { Up }",
    ]
    .join("\n");
    let parsed = parse(
        &source,
        ParseOptions {
            source_kind: SourceKind::Module,
            ..typescript_options()
        },
    )
    .expect("parse explicit declared enums");

    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed
        .tape
        .validate()
        .expect("valid explicit-declare-enum tape");
    let enums = parsed
        .tape
        .validation()
        .filter_map(|record| match record.expect("valid record").value {
            TapeValue::Node {
                tag: NodeTag::TS_ENUM_DECLARATION,
                span,
                fields,
                ..
            } => Some((span, fields.to_vec())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(enums.len(), 5);
    assert!(enums.iter().all(|(_, fields)| fields.len() == 4));
    for (index, (span, fields)) in enums.iter().enumerate() {
        assert!(matches!(
            parsed.tape.value_at(fields[2]),
            Ok(TapeValue::Bool(value)) if value == matches!(index, 1 | 3)
        ));
        assert!(matches!(
            parsed.tape.value_at(fields[3]),
            Ok(TapeValue::Bool(value)) if value == (index < 4)
        ));
        let prefix = if index < 4 { "declare" } else { "enum" };
        assert!(source[span.start as usize..span.end as usize].starts_with(prefix));
    }
    assert!(node_fields(&parsed, NodeTag::TS_ENUM_MEMBER).any(|fields| {
        matches!(
            parsed.tape.value_at(fields[1]),
            Ok(TapeValue::Node {
                tag: NodeTag::CALL_EXPRESSION,
                ..
            })
        )
    }));

    let exports = node_fields(&parsed, NodeTag::EXPORT_NAMED_DECLARATION).collect::<Vec<_>>();
    assert_eq!(exports.len(), 2);
    for fields in exports {
        assert!(matches!(
            parsed.tape.value_at(fields[0]),
            Ok(TapeValue::Node {
                tag: NodeTag::TS_ENUM_DECLARATION,
                span,
                ..
            }) if source[span.start as usize..span.end as usize].starts_with("declare")
        ));
        assert!(matches!(
            parsed.tape.value_at(fields[4]),
            Ok(TapeValue::U32(1))
        ));
    }
}

#[test]
fn keeps_explicit_declared_enums_contextual_and_typescript_only() {
    for language in [
        Language::TypeScript,
        Language::TypeScriptJsx,
        Language::TypeScriptDefinition,
    ] {
        let parsed = parse(
            "declare enum Choice { First }",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("parse TypeScript declared enum");
        assert!(
            parsed.diagnostics.is_empty(),
            "{language:?}: {:#?}",
            parsed.diagnostics
        );
        let fields = first_node_fields(&parsed, NodeTag::TS_ENUM_DECLARATION);
        assert!(matches!(
            parsed.tape.value_at(fields[3]),
            Ok(TapeValue::Bool(true))
        ));
    }

    for source in [
        "declare\nenum Choice {}",
        "declar\\u0065 enum Choice {}",
        "declare en\\u0075m Choice {}",
        "declare c\\u006fnst enum Choice {}",
        "declare const en\\u0075m Choice {}",
        "export declare\nenum Choice {}",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                source_kind: SourceKind::Module,
                ..typescript_options()
            },
        )
        .expect("recover contextual declare enum");
        parsed.tape.validate().expect("valid contextual enum tape");
        assert!(
            node_fields(&parsed, NodeTag::TS_ENUM_DECLARATION).all(|fields| {
                matches!(parsed.tape.value_at(fields[3]), Ok(TapeValue::Bool(false)))
            }),
            "{source}"
        );
    }

    for options in [
        ParseOptions {
            language: Language::JavaScript,
            ..ParseOptions::default()
        },
        ParseOptions {
            language: Language::JavaScriptJsx,
            ..ParseOptions::default()
        },
        ParseOptions {
            syntax_extensions: SyntaxExtensions {
                typescript_js_compatibility: true,
                ..SyntaxExtensions::default()
            },
            ..ParseOptions::default()
        },
    ] {
        let parsed = parse("declare enum Choice {}", options).expect("recover excluded enum");
        assert!(!parsed.diagnostics.is_empty());
        assert!(
            node_fields(&parsed, NodeTag::TS_ENUM_DECLARATION).all(|fields| {
                matches!(parsed.tape.value_at(fields[3]), Ok(TapeValue::Bool(false)))
            })
        );
    }
}

#[test]
fn parses_explicit_declared_namespaces_with_nested_and_qualified_names() {
    let source = [
        r"declare namespace N\u0061me.default { namespace Inner { const value: number; } declare namespace Explicit {} }",
        "export\ndeclare namespace Public {}",
        "namespace Ordinary {}",
    ]
    .join("\n");
    let parsed = parse(
        &source,
        ParseOptions {
            source_kind: SourceKind::Module,
            ..typescript_options()
        },
    )
    .expect("parse explicit declared namespaces");

    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed
        .tape
        .validate()
        .expect("valid explicit namespace tape");
    let modules = parsed
        .tape
        .validation()
        .filter_map(|record| match record.expect("valid record").value {
            TapeValue::Node {
                tag: NodeTag::TS_MODULE_DECLARATION,
                span,
                fields,
                ..
            } => Some((span, fields.to_vec())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(modules.len(), 5);
    for (span, fields) in &modules {
        assert_eq!(fields.len(), 4);
        assert!(matches!(
            parsed.tape.value_at(fields[3]),
            Ok(TapeValue::U32(0))
        ));
        let text = &source[span.start as usize..span.end as usize];
        let expected_declare = text.starts_with("declare");
        assert!(matches!(
            parsed.tape.value_at(fields[2]),
            Ok(TapeValue::Bool(declare)) if declare == expected_declare
        ));
    }
    assert!(modules.iter().any(|(span, fields)| {
        source[span.start as usize..span.end as usize].starts_with("namespace Inner")
            && matches!(parsed.tape.value_at(fields[2]), Ok(TapeValue::Bool(false)))
    }));
    assert!(modules.iter().any(|(span, fields)| {
        source[span.start as usize..span.end as usize].starts_with("declare namespace Explicit")
            && matches!(parsed.tape.value_at(fields[2]), Ok(TapeValue::Bool(true)))
    }));

    let escaped_identifier = parsed.tape.validation().find_map(|record| {
        let TapeValue::Node {
            tag: NodeTag::IDENTIFIER,
            span,
            fields,
            ..
        } = record.expect("valid record").value
        else {
            return None;
        };
        (&source[span.start as usize..span.end as usize] == r"N\u0061me").then_some(fields[0])
    });
    let TapeValue::PoolString { start, len } = parsed
        .tape
        .value_at(escaped_identifier.expect("escaped namespace identifier"))
        .expect("escaped identifier name")
    else {
        panic!("escaped namespace name is not decoded into the tape pool");
    };
    assert_eq!(
        parsed
            .tape
            .string_view(start, len)
            .expect("decoded namespace name"),
        "Name"
    );

    let export = first_node_fields(&parsed, NodeTag::EXPORT_NAMED_DECLARATION);
    assert!(matches!(
        parsed.tape.value_at(export[4]),
        Ok(TapeValue::U32(1))
    ));
}

#[test]
fn keeps_declared_namespaces_contextual_and_typescript_only() {
    for language in [
        Language::TypeScript,
        Language::TypeScriptJsx,
        Language::TypeScriptDefinition,
    ] {
        let parsed = parse(
            "declare namespace Included {}",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("parse TypeScript declared namespace");
        let fields = first_node_fields(&parsed, NodeTag::TS_MODULE_DECLARATION);
        assert!(matches!(
            parsed.tape.value_at(fields[2]),
            Ok(TapeValue::Bool(true))
        ));
    }

    for source in [
        "declare namespace\nSeparated {}",
        "declar\\u0065 namespace Escaped {}",
        "declare namesp\\u0061ce Escaped {}",
        "declare namespace default.Name {}",
        "declare namespace enum.Name {}",
        "declare namespace {}",
        "declare.namespace;",
        "declare: namespace;",
    ] {
        let parsed = parse(source, typescript_options()).expect("recover contextual namespace");
        parsed.tape.validate().expect("valid contextual tape");
        assert!(
            node_fields(&parsed, NodeTag::TS_MODULE_DECLARATION).all(|fields| {
                matches!(parsed.tape.value_at(fields[2]), Ok(TapeValue::Bool(false)))
            }),
            "{source}"
        );
    }

    let separated = parse("declare\nnamespace Ordinary {}", typescript_options())
        .expect("parse expression followed by ordinary namespace");
    let fields = first_node_fields(&separated, NodeTag::TS_MODULE_DECLARATION);
    assert!(matches!(
        separated.tape.value_at(fields[2]),
        Ok(TapeValue::Bool(false))
    ));
    for source in ["namespace\nName {}", "module\nName {}"] {
        let parsed = parse(source, typescript_options()).expect("recover newline module keyword");
        assert_eq!(
            node_fields(&parsed, NodeTag::TS_MODULE_DECLARATION).count(),
            0,
            "{source}"
        );
    }

    for options in [
        ParseOptions::default(),
        ParseOptions {
            language: Language::JavaScriptJsx,
            ..ParseOptions::default()
        },
        ParseOptions {
            syntax_extensions: SyntaxExtensions {
                typescript_js_compatibility: true,
                ..SyntaxExtensions::default()
            },
            ..ParseOptions::default()
        },
    ] {
        let parsed =
            parse("declare namespace Excluded {}", options).expect("recover excluded namespace");
        assert_eq!(
            node_fields(&parsed, NodeTag::TS_MODULE_DECLARATION).count(),
            0
        );
    }
}

#[test]
fn parses_explicit_ambient_external_modules_and_global_augmentations() {
    let source = [
        r#"declare module "pack\u0061ge" { import value from "dependency"; import Alias = require("dependency"); export = Alias; export as namespace Alias; export default Alias; export { Alias } from "dependency"; export * from "dependency"; namespace Nested {} }"#,
        r#"declare module "empty";"#,
        "declare global\n{ function eval(): void; let shared: number; }",
        r#"export declare module "exported" {}"#,
        "export declare global {}",
    ]
    .join("\n");
    let parsed = parse(
        &source,
        ParseOptions {
            source_kind: SourceKind::Module,
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("parse ambient external modules and global augmentations");

    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed.tape.validate().expect("valid ambient module tape");
    let modules = parsed
        .tape
        .validation()
        .filter_map(|record| match record.expect("valid record").value {
            TapeValue::Node {
                tag: NodeTag::TS_MODULE_DECLARATION,
                span,
                fields,
                ..
            } => Some((span, fields.to_vec())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(modules.len(), 6);
    let shorthand = modules
        .iter()
        .find(|(span, _)| {
            &source[span.start as usize..span.end as usize] == r#"declare module "empty";"#
        })
        .expect("shorthand ambient module");
    assert!(matches!(
        parsed.tape.value_at(shorthand.1[1]),
        Ok(TapeValue::Null)
    ));
    assert!(matches!(
        parsed.tape.value_at(shorthand.1[3]),
        Ok(TapeValue::U32(1))
    ));
    let globals = modules
        .iter()
        .filter(|(_, fields)| matches!(parsed.tape.value_at(fields[3]), Ok(TapeValue::U32(2))))
        .count();
    assert_eq!(globals, 2);
    assert_eq!(
        modules
            .iter()
            .filter(|(_, fields)| {
                matches!(parsed.tape.value_at(fields[3]), Ok(TapeValue::U32(1)))
            })
            .count(),
        3
    );
}

#[test]
fn contextual_global_augmentations_preserve_their_ambient_grammar() {
    let source = [
        "let topLevel: string; global\n{ let topLevel: number; function topImplementation() {} class Top { method() {} field = 1; } let topInitializer = 1; }",
        r#"declare module "ambient" { let nested: string; global { let nested: number; function ambientImplementation() {} class Ambient { method() {} field = 1; } let ambientInitializer = 1; } }"#,
        "namespace Ordinary { global { function namespaceImplementation() {} class Nested { method() {} field = 1; } let namespaceInitializer = 1; } }",
        "declare namespace Ambient { global {} }",
    ]
    .join("\n");
    let parsed = parse(
        &source,
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("parse contextual global augmentations");

    for message in ["duplicate binding `topLevel`", "duplicate binding `nested`"] {
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == message),
            "{message}: {:#?}",
            parsed.diagnostics
        );
    }
    let ambient_messages = [
        "function implementations are not allowed in ambient contexts",
        "class method implementations are not allowed in ambient contexts",
        "class property initializers are not allowed in ambient contexts",
        "initializers are not allowed in ambient contexts",
    ];
    for message in ambient_messages {
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == message),
            "{message}: {:#?}",
            parsed.diagnostics
        );
    }
    assert_eq!(
        parsed
            .diagnostics
            .iter()
            .filter(|diagnostic| ambient_messages.contains(&diagnostic.message.as_ref()))
            .count(),
        ambient_messages.len(),
        "{:#?}",
        parsed.diagnostics
    );
    parsed
        .tape
        .validate()
        .expect("valid contextual global tape");
    let globals = parsed
        .tape
        .validation()
        .filter_map(|record| match record.expect("valid record").value {
            TapeValue::Node {
                tag: NodeTag::TS_MODULE_DECLARATION,
                fields,
                ..
            } if matches!(parsed.tape.value_at(fields[3]), Ok(TapeValue::U32(2))) => {
                Some(fields.to_vec())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(globals.len(), 4);
    for fields in globals {
        assert!(matches!(
            parsed.tape.value_at(fields[2]),
            Ok(TapeValue::Bool(false))
        ));
        assert!(matches!(
            parsed.tape.value_at(fields[0]),
            Ok(TapeValue::Node {
                tag: NodeTag::IDENTIFIER,
                ..
            })
        ));
        assert!(matches!(
            parsed.tape.value_at(fields[1]),
            Ok(TapeValue::Node {
                tag: NodeTag::TS_MODULE_BLOCK,
                ..
            })
        ));
    }
}

#[test]
fn parses_contextual_global_augmentations_in_all_typescript_statement_scopes() {
    let placed = parse(
        "function f() { global {} } { global {} } declare global { global {} }",
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("parse contextual global in nested statement scopes");
    assert!(placed.diagnostics.is_empty(), "{:#?}", placed.diagnostics);
    let global_declare_fields = node_fields(&placed, NodeTag::TS_MODULE_DECLARATION)
        .filter_map(|fields| {
            matches!(placed.tape.value_at(fields[3]), Ok(TapeValue::U32(2))).then_some(fields[2])
        })
        .collect::<Vec<_>>();
    assert_eq!(global_declare_fields.len(), 4);
    assert_eq!(
        global_declare_fields
            .iter()
            .filter(|field| matches!(placed.tape.value_at(**field), Ok(TapeValue::Bool(true))))
            .count(),
        1
    );

    for source in [r"gl\u006fbal {}", "global;", "global\n;"] {
        let recovered = parse(source, typescript_options()).expect("recover contextual global");
        assert_eq!(
            node_fields(&recovered, NodeTag::TS_MODULE_DECLARATION).count(),
            0,
            "{source}"
        );
    }

    for options in [
        ParseOptions::default(),
        ParseOptions {
            language: Language::JavaScriptJsx,
            ..ParseOptions::default()
        },
        ParseOptions {
            syntax_extensions: SyntaxExtensions {
                typescript_js_compatibility: true,
                ..SyntaxExtensions::default()
            },
            ..ParseOptions::default()
        },
    ] {
        let recovered = parse("global {}", options).expect("recover excluded contextual global");
        assert_eq!(
            node_fields(&recovered, NodeTag::TS_MODULE_DECLARATION).count(),
            0
        );
    }
}

#[test]
fn recovers_ambient_module_heads_without_broadening_contextual_syntax() {
    let legacy = parse("declare module Legacy.Deep {}", typescript_options())
        .expect("parse legacy ambient internal module");
    assert!(legacy.diagnostics.is_empty(), "{:#?}", legacy.diagnostics);
    let fields = first_node_fields(&legacy, NodeTag::TS_MODULE_DECLARATION);
    assert!(matches!(
        legacy.tape.value_at(fields[3]),
        Ok(TapeValue::U32(1))
    ));

    let semantic_legacy = parse(
        "declare module Legacy.Deep {}",
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("recover a legacy ambient internal module in semantic mode");
    assert_eq!(semantic_legacy.diagnostics.len(), 1);
    assert_eq!(
        semantic_legacy.diagnostics[0].message,
        "ambient external module name must be a string literal"
    );
    for source in ["declare module 42 {}", "declare module {}"] {
        let parsed = parse(source, typescript_options()).expect("recover invalid ambient module");
        assert!(parsed.diagnostics.iter().any(|diagnostic| {
            diagnostic.message == "ambient module name must be a string literal or identifier"
        }));
        let fields = first_node_fields(&parsed, NodeTag::TS_MODULE_DECLARATION);
        assert!(matches!(
            parsed.tape.value_at(fields[2]),
            Ok(TapeValue::Bool(true))
        ));
        assert!(matches!(
            parsed.tape.value_at(fields[3]),
            Ok(TapeValue::U32(1))
        ));
        if source == "declare module {}" {
            assert!(matches!(
                parsed.tape.value_at(fields[0]),
                Ok(TapeValue::Null)
            ));
        }
    }

    let bodyless_global =
        parse("declare global;", typescript_options()).expect("recover bodyless global");
    assert_eq!(bodyless_global.diagnostics.len(), 1);
    assert_eq!(
        bodyless_global.diagnostics[0].message,
        "global augmentation requires a module block"
    );
    let fields = first_node_fields(&bodyless_global, NodeTag::TS_MODULE_DECLARATION);
    assert!(matches!(
        bodyless_global.tape.value_at(fields[3]),
        Ok(TapeValue::U32(2))
    ));

    for source in [
        "declare\nmodule \"split\" {}",
        "declare module\n\"split\" {}",
        "declar\\u0065 module \"escaped\" {}",
        "declare mod\\u0075le \"escaped\" {}",
        "declare gl\\u006fbal {}",
        "global {}",
    ] {
        let parsed = parse(source, typescript_options()).expect("recover contextual syntax");
        assert!(
            node_fields(&parsed, NodeTag::TS_MODULE_DECLARATION).all(|fields| {
                !matches!(parsed.tape.value_at(fields[2]), Ok(TapeValue::Bool(true)))
            }),
            "{source}"
        );
    }

    for options in [
        ParseOptions::default(),
        ParseOptions {
            language: Language::JavaScriptJsx,
            ..ParseOptions::default()
        },
        ParseOptions {
            syntax_extensions: SyntaxExtensions {
                typescript_js_compatibility: true,
                ..SyntaxExtensions::default()
            },
            ..ParseOptions::default()
        },
    ] {
        let parsed = parse(r#"declare module "excluded" {}"#, options)
            .expect("recover excluded ambient module");
        assert_eq!(
            node_fields(&parsed, NodeTag::TS_MODULE_DECLARATION).count(),
            0
        );
    }
}

#[test]
fn merges_ambient_call_and_constructor_overloads() {
    let ambient_constructor_overloads = parse(
        "declare namespace M { export function RegExp(pattern: string): RegExp; export class RegExp { constructor(pattern: string); } }",
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("parse ambient call and constructor overloads");
    assert!(
        ambient_constructor_overloads.diagnostics.is_empty(),
        "{:#?}",
        ambient_constructor_overloads.diagnostics
    );

    for source in [
        "declare namespace M { function C(): C; class C {} class C {} }",
        "declare namespace M { class C {} function C(): C; class C {} }",
    ] {
        let duplicate = parse(
            source,
            ParseOptions {
                semantic_errors: true,
                ..typescript_options()
            },
        )
        .expect("recover a duplicate ambient class around a function overload");
        assert_eq!(duplicate.diagnostics.len(), 1, "{source}");
        assert_eq!(duplicate.diagnostics[0].message, "duplicate binding `C`");
    }
}

#[test]
fn separates_external_module_semantics_from_internal_and_global_scopes() {
    let external = parse(
        r#"declare module "one" { import value from "dependency"; import Alias = require("dependency"); export = Alias; export as namespace Alias; export default Alias; export { value } from "dependency"; export * from "dependency"; let shared: number; } declare module "two" { import value from "dependency"; export { value } from "dependency"; let shared: number; }"#,
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("parse isolated external module scopes");
    assert!(
        external.diagnostics.is_empty(),
        "{:#?}",
        external.diagnostics
    );

    let nested_internal = parse(
        r#"declare module "outer" { namespace Inner { import value from "dependency"; export * from "dependency"; } }"#,
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("recover external forms in a nested internal namespace");
    assert_eq!(nested_internal.diagnostics.len(), 2);
    assert!(nested_internal.diagnostics.iter().any(|diagnostic| {
        diagnostic.message == "import declarations in a namespace cannot reference a module"
    }));
    assert!(nested_internal.diagnostics.iter().any(|diagnostic| {
        diagnostic.message == "export-all declarations are not allowed in internal namespaces"
    }));

    let global_collision = parse(
        "declare global { let shared: number; function implemented() {} class C { method() {} field = 1; } } let shared: number;",
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("recover invalid global augmentation declarations");
    for message in [
        "duplicate binding `shared`",
        "function implementations are not allowed in ambient contexts",
        "class method implementations are not allowed in ambient contexts",
        "class property initializers are not allowed in ambient contexts",
    ] {
        assert!(
            global_collision
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == message),
            "{message}: {:#?}",
            global_collision.diagnostics
        );
    }

    let nested_global = parse(
        "declare global { declare global {} }",
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("recover a nested global augmentation");
    assert!(nested_global.diagnostics.iter().any(|diagnostic| {
        diagnostic.message
            == "global augmentations are only allowed at the top level of a namespace or module"
    }));

    let namespace_export_collision = parse(
        "export as namespace exportedGlobal; declare global { export let exportedGlobal; }",
        ParseOptions {
            source_kind: SourceKind::Module,
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("recover a namespace export redeclared by a global augmentation");
    assert_eq!(namespace_export_collision.diagnostics.len(), 1);
    assert_eq!(
        namespace_export_collision.diagnostics[0].message,
        "duplicate binding `exportedGlobal`"
    );

    for prefix in [r#"declare module "ambient" {}"#, "declare global {}"] {
        let parsed = parse(
            &format!("{prefix} function eval() {{}}"),
            ParseOptions {
                source_kind: SourceKind::Module,
                semantic_errors: true,
                ..typescript_options()
            },
        )
        .expect("restore strict grammar after an ambient module declaration");
        assert!(!parsed.diagnostics.is_empty(), "{prefix}");
    }
}

#[test]
fn scopes_declared_namespace_semantics_and_strictness() {
    let semantic_free = parse(
        "\"use strict\"; namespace public {}",
        ParseOptions {
            semantic_errors: false,
            ..typescript_options()
        },
    )
    .expect("parse a strict-reserved namespace name without semantic diagnostics");
    assert!(
        semantic_free.diagnostics.is_empty(),
        "{:#?}",
        semantic_free.diagnostics
    );
    assert_eq!(
        node_fields(&semantic_free, NodeTag::TS_MODULE_DECLARATION).count(),
        1
    );

    let ambient_sloppy = parse(
        "declare namespace N { function eval(): void; function arguments(): void; class C { method(eval: unknown): void; method2(arguments: unknown): void; } }",
        ParseOptions {
            source_kind: SourceKind::Module,
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("parse ambient namespace bindings with strict grammar suspended");
    assert!(
        ambient_sloppy.diagnostics.is_empty(),
        "{:#?}",
        ambient_sloppy.diagnostics
    );
    let restored = parse(
        "declare namespace N {} function eval() {}",
        ParseOptions {
            source_kind: SourceKind::Module,
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("restore strict grammar after an ambient namespace");
    assert!(!restored.diagnostics.is_empty());

    for source in [
        "function f() { declare namespace N {} }",
        "if (condition) { declare namespace N {} }",
    ] {
        let misplaced = parse(
            source,
            ParseOptions {
                semantic_errors: true,
                ..typescript_options()
            },
        )
        .expect("recover a misplaced ambient namespace");
        assert!(!misplaced.diagnostics.is_empty(), "{source}");
    }
}

#[test]
fn enforces_ambient_namespace_class_and_variable_rules() {
    let valid = parse(
        "declare namespace Valid { function signature(): void; class C { method(): void; rest(...items: any[],): void; get value(): string; field: string; readonly inferred = Symbol(); } var value: number; let later: string; const text = 'value'; const truth = true; const count = -1; const large = -1n; const template = `value`; const member = Enum.Member; const keyword = Enum.default; const indexed = Enum['Member']; const templated = Namespace.Enum[`Member`]; namespace Nested { const value: number; } } function outside() {} class Outside { field = 1; method() {} } const runtime = 1 + 2;",
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("parse valid ambient namespace declarations");
    assert!(valid.diagnostics.is_empty(), "{:#?}", valid.diagnostics);

    for source in [
        "declare namespace N { function implemented() {} }",
        "declare namespace N { class C { method() {} } }",
        "declare namespace N { class C { get value() { return 1; } } }",
        "declare namespace N { class C { constructor() {} } }",
        "declare namespace N { class C { static {} } }",
        "declare namespace N { class C { field = 1; } }",
        "declare namespace N { class C { readonly typed: number = 1; } }",
        "declare namespace N { var value = 1; }",
        "declare namespace N { let value: number = 1; }",
        "declare namespace N { const value: number = 1; }",
        "declare namespace N { const value = (1); }",
        "declare namespace N { const value = null; }",
        "declare namespace N { const value = 1 + 2; }",
        "declare namespace N { const value = Namespace[member]; }",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                semantic_errors: true,
                ..typescript_options()
            },
        )
        .expect("recover invalid ambient namespace declaration");
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed
            .tape
            .validate()
            .expect("valid recovered ambient tape");
    }
}

#[test]
fn restricts_only_external_export_forms_in_ambient_namespaces() {
    let valid = parse(
        "declare namespace N { export interface Item {} export const value: number; export { value }; } export default runtime;",
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("parse valid internal namespace exports");
    assert!(valid.diagnostics.is_empty(), "{:#?}", valid.diagnostics);

    for source in [
        "declare namespace N { export = N; }",
        "declare namespace N { export as namespace N; }",
        "declare namespace N { export default value; }",
        "declare namespace N { export { value } from 'module'; }",
        "declare namespace N { export * from 'module'; }",
        "declare namespace N { export * as values from 'module'; }",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                semantic_errors: true,
                ..typescript_options()
            },
        )
        .expect("recover invalid internal namespace export");
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect("valid recovered export tape");
    }
}

#[test]
fn gates_typescript_function_signatures() {
    for language in [
        Language::TypeScript,
        Language::TypeScriptJsx,
        Language::TypeScriptDefinition,
    ] {
        let parsed = parse(
            "function signature(value: Input): Output;",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("parse TypeScript function signature");
        assert!(
            parsed.diagnostics.is_empty(),
            "{language:?}: {:#?}",
            parsed.diagnostics
        );
        assert_node_field_count(&parsed, NodeTag::TS_DECLARE_FUNCTION, 6);
    }

    let compatibility = parse(
        "function signature();",
        ParseOptions {
            syntax_extensions: SyntaxExtensions {
                typescript_js_compatibility: true,
                ..SyntaxExtensions::default()
            },
            ..ParseOptions::default()
        },
    )
    .expect("parse compatibility function signature");
    assert!(
        compatibility.diagnostics.is_empty(),
        "{:#?}",
        compatibility.diagnostics
    );
    assert_node_field_count(&compatibility, NodeTag::TS_DECLARE_FUNCTION, 6);

    for (source, options) in [
        (
            "function signature();",
            ParseOptions {
                language: Language::JavaScript,
                ..ParseOptions::default()
            },
        ),
        (
            "function signature();",
            ParseOptions {
                language: Language::JavaScriptJsx,
                ..ParseOptions::default()
            },
        ),
        (
            "const expression = function named(): void;",
            typescript_options(),
        ),
        (
            "function signature(): void const value = 1;",
            typescript_options(),
        ),
    ] {
        let parsed = parse(source, options).expect("recover excluded function signature");
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed
            .tape
            .validate()
            .expect("valid excluded-signature tape");
        assert_eq!(
            node_fields(&parsed, NodeTag::TS_DECLARE_FUNCTION).count(),
            0,
            "{source}"
        );
    }
}

#[test]
fn parses_declared_variables_and_type_only_exports() {
    let source = [
        "declare var first;",
        "declare let second: string;",
        "declare const third: number;",
        "export declare var exportedFirst;",
        "export declare let exportedSecond: string;",
        "export declare const exportedThird: number;",
    ]
    .join("\n");
    let parsed = parse(
        &source,
        ParseOptions {
            source_kind: SourceKind::Module,
            ..typescript_options()
        },
    )
    .expect("parse declared variables");
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed
        .tape
        .validate()
        .expect("valid declared-variable tape");

    assert_eq!(NodeTag::TS_DECLARE_VARIABLE_DECLARATION.get(), 575);
    let declarations =
        node_fields(&parsed, NodeTag::TS_DECLARE_VARIABLE_DECLARATION).collect::<Vec<_>>();
    assert_eq!(declarations.len(), 6);
    assert!(declarations.iter().all(|fields| fields.len() == 2));

    let exports = node_fields(&parsed, NodeTag::EXPORT_NAMED_DECLARATION).collect::<Vec<_>>();
    assert_eq!(exports.len(), 3);
    for fields in exports {
        assert!(matches!(
            parsed.tape.value_at(fields[0]),
            Ok(TapeValue::Node {
                tag: NodeTag::TS_DECLARE_VARIABLE_DECLARATION,
                span,
                ..
            }) if source[span.start as usize..span.end as usize].starts_with("declare ")
        ));
        assert!(matches!(
            parsed.tape.value_at(fields[4]),
            Ok(TapeValue::U32(1))
        ));
    }
}

#[test]
fn keeps_declare_variable_contextual_and_typescript_only() {
    for language in [
        Language::TypeScript,
        Language::TypeScriptJsx,
        Language::TypeScriptDefinition,
    ] {
        let parsed = parse(
            "declare var value;",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("parse TypeScript declared variable");
        assert!(
            parsed.diagnostics.is_empty(),
            "{language:?}: {:#?}",
            parsed.diagnostics
        );
        assert_node_field_count(&parsed, NodeTag::TS_DECLARE_VARIABLE_DECLARATION, 2);
    }

    for source in [
        "declare; var value;",
        "declare\nvar value;",
        "declar\\u0065 var value;",
        "declare v\\u0061r value;",
        "export declare\nvar value;",
    ] {
        let parsed = parse(source, typescript_options()).expect("recover contextual declare");
        parsed
            .tape
            .validate()
            .expect("valid contextual recovery tape");
        assert_eq!(
            node_fields(&parsed, NodeTag::TS_DECLARE_VARIABLE_DECLARATION).count(),
            0,
            "{source}"
        );
    }

    let exported = parse(
        "export\ndeclare var value;",
        ParseOptions {
            source_kind: SourceKind::Module,
            ..typescript_options()
        },
    )
    .expect("parse line break before declare");
    assert!(
        exported.diagnostics.is_empty(),
        "{:#?}",
        exported.diagnostics
    );
    assert_node_field_count(&exported, NodeTag::TS_DECLARE_VARIABLE_DECLARATION, 2);

    for language in [Language::JavaScript, Language::JavaScriptJsx] {
        let parsed = parse(
            "declare var value;",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("recover JavaScript declare expression");
        assert!(!parsed.diagnostics.is_empty(), "{language:?}");
        assert_eq!(
            node_fields(&parsed, NodeTag::TS_DECLARE_VARIABLE_DECLARATION).count(),
            0
        );
    }

    let compatibility = parse(
        "declare var value;",
        ParseOptions {
            syntax_extensions: SyntaxExtensions {
                typescript_js_compatibility: true,
                ..SyntaxExtensions::default()
            },
            ..ParseOptions::default()
        },
    )
    .expect("recover compatibility declare expression");
    assert!(!compatibility.diagnostics.is_empty());
    assert_eq!(
        node_fields(&compatibility, NodeTag::TS_DECLARE_VARIABLE_DECLARATION).count(),
        0
    );
}

#[test]
fn declared_variables_do_not_mask_typescript_syntax_diagnostics() {
    for source in [
        "declare var value: number; ++value++;",
        "declare var value: number; value++++;",
        "declare var values: number[]; values[01];",
    ] {
        let parsed = parse(source, typescript_options()).expect("parse invalid TypeScript source");
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        assert_node_field_count(&parsed, NodeTag::TS_DECLARE_VARIABLE_DECLARATION, 2);
        parsed.tape.validate().expect("valid diagnostic tape");
    }

    for preserve_parentheses in [false, true] {
        let parenthesized = parse(
            "declare var value: number; ++(value++); (value++)++;",
            ParseOptions {
                preserve_parentheses,
                ..typescript_options()
            },
        )
        .expect("parse parenthesized updates");
        assert!(
            parenthesized.diagnostics.is_empty(),
            "preserve_parentheses={preserve_parentheses}: {:#?}",
            parenthesized.diagnostics
        );
    }

    let sloppy_octal = parse(
        "077;",
        ParseOptions {
            semantic_errors: true,
            source_kind: SourceKind::Script,
            ..ParseOptions::default()
        },
    )
    .expect("parse sloppy legacy octal");
    assert!(sloppy_octal.diagnostics.is_empty());

    let strict_octal = parse(
        "'use strict'; 077;",
        ParseOptions {
            semantic_errors: true,
            source_kind: SourceKind::Script,
            ..ParseOptions::default()
        },
    )
    .expect("parse strict legacy octal");
    assert!(!strict_octal.diagnostics.is_empty());
}

#[test]
fn keeps_runtime_function_type_parameters_out_of_javascript() {
    for language in [Language::JavaScript, Language::JavaScriptJsx] {
        let parsed = parse(
            "function invalid<T>(value) {}",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("recoverable JavaScript parse");
        assert!(!parsed.diagnostics.is_empty(), "{language:?}");
        parsed.tape.validate().expect("valid recovery tape");
    }
}

#[test]
fn diagnoses_empty_runtime_function_type_parameters() {
    let parsed = parse("function invalid<>() {}", typescript_options())
        .expect("recoverable empty type-parameter parse");
    assert!(!parsed.diagnostics.is_empty());
    parsed.tape.validate().expect("valid recovery tape");

    let fields = first_node_fields(&parsed, NodeTag::TS_TYPE_PARAMETER_DECLARATION);
    assert!(matches!(
        parsed.tape.value_at(fields[0]),
        Ok(TapeValue::List { items, .. }) if items.is_empty()
    ));
}

#[test]
fn parses_optional_typed_value_parameters() {
    let source = [
        "function declaration(required: Input, optional?: Input, inferred?) {}",
        "const expression = function (required: Input, optional?: Input) {};",
        "class Service { method(required: Input, optional?: Input) {} }",
        "const arrow = (required: Input, optional?: Input) => optional;",
        "const asyncArrow = async (required: Input, optional?: Input) => optional;",
    ]
    .join("\n");
    let parsed = parse(&source, typescript_options()).expect("parse optional value parameters");

    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed
        .tape
        .validate()
        .expect("valid optional-parameter tape");

    let identifiers = node_fields(&parsed, NodeTag::IDENTIFIER)
        .filter(|fields| fields.len() == 3)
        .collect::<Vec<_>>();
    assert_eq!(identifiers.len(), 11);
    assert_eq!(
        identifiers
            .iter()
            .filter(|fields| {
                matches!(parsed.tape.value_at(fields[2]), Ok(TapeValue::Bool(true)))
            })
            .count(),
        6
    );
    assert_eq!(
        identifiers
            .iter()
            .filter(|fields| {
                matches!(parsed.tape.value_at(fields[2]), Ok(TapeValue::Bool(false)))
            })
            .count(),
        5
    );
    assert_eq!(
        identifiers
            .iter()
            .filter(|fields| matches!(parsed.tape.value_at(fields[1]), Ok(TapeValue::Null)))
            .count(),
        1
    );
    assert_eq!(
        node_fields(&parsed, NodeTag::ARROW_FUNCTION_EXPRESSION).count(),
        2
    );
}

#[test]
fn keeps_optional_parameter_syntax_out_of_javascript() {
    let parsed = parse(
        "function invalid(value?: Input) {}",
        ParseOptions {
            language: Language::JavaScript,
            ..ParseOptions::default()
        },
    )
    .expect("recover from optional JavaScript parameter");

    assert!(!parsed.diagnostics.is_empty());
    parsed
        .tape
        .validate()
        .expect("valid recovered JavaScript tape");
    assert!(
        node_fields(&parsed, NodeTag::IDENTIFIER).all(|fields| fields.len() != 3),
        "JavaScript identifiers must not gain TypeScript parameter fields"
    );
}

#[test]
fn does_not_apply_parameter_optionality_to_other_typescript_bindings() {
    for source in [
        "let value?: Input;",
        "import { value? } from 'package';",
        "type value? = Input;",
        "function destructured({ value? }: Input) {}",
        "function rest(...values?: Input[]) {}",
    ] {
        let parsed = parse(source, typescript_options()).expect("recover invalid optional binding");

        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect("valid recovered tape");
    }
}

#[test]
fn limits_function_return_annotations_to_supported_typescript_bodies() {
    let cases = [
        (
            "predicate",
            "function isText(value: unknown): value is string { return true; }",
        ),
        (
            "assertion",
            "function assertText(value: unknown): asserts value { }",
        ),
        ("missing type", "function missing(): ; {}"),
    ];
    for (name, source) in cases {
        let parsed = parse(source, typescript_options()).expect(name);
        assert!(!parsed.diagnostics.is_empty(), "{name}");
        parsed.tape.validate().expect(name);
    }

    for language in [Language::JavaScript, Language::JavaScriptJsx] {
        let parsed = parse(
            "function convert(): string { return ''; }",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("recoverable JavaScript parse");
        assert!(!parsed.diagnostics.is_empty(), "{language:?}");
        parsed
            .tape
            .validate()
            .expect("valid JavaScript recovery tape");
    }
}

#[test]
fn parses_method_return_annotations_without_widening_plain_method_records() {
    let source = [
        "class Service {",
        "  method(): Namespace.Output {}",
        "  static [key](): Promise<Result> {}",
        "  #private(): Hidden {}",
        "  *values(): Iterable<Result> {}",
        "  async load(): Promise<Result> {}",
        "  get value(): Result {}",
        "  get #secret(): Hidden {}",
        "  set value(next): void {}",
        "  plain() {}",
        "}",
        "const service = {",
        "  method(): Result {},",
        "  [key](): Result {},",
        "  *values(): Iterable<Result> {},",
        "  async load(): Promise<Result> {},",
        "  get value(): Result {},",
        "  set value(next): void {},",
        "  plain() {},",
        "};",
    ]
    .join("\n");
    let parsed = parse(&source, typescript_options()).expect("parse method return annotations");
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed
        .tape
        .validate()
        .expect("valid method annotation tape");

    let methods = node_fields(&parsed, NodeTag::FUNCTION_EXPRESSION).collect::<Vec<_>>();
    assert_eq!(methods.len(), 16);
    assert_eq!(
        methods.iter().filter(|fields| fields.len() == 6).count(),
        14
    );
    assert_eq!(methods.iter().filter(|fields| fields.len() == 5).count(), 2);

    for fields in methods.iter().filter(|fields| fields.len() == 6) {
        assert!(matches!(
            parsed.tape.value_at(fields[5]),
            Ok(TapeValue::Node {
                tag: NodeTag::TS_TYPE_ANNOTATION,
                ..
            })
        ));
    }
    let TapeValue::Node { span, .. } = parsed
        .tape
        .value_at(methods[0][5])
        .expect("method return annotation")
    else {
        panic!("method return annotation is not a node");
    };
    assert_eq!(
        &source[span.start as usize..span.end as usize],
        ": Namespace.Output"
    );
}

#[test]
fn parses_bodyless_typescript_class_signatures_and_constructor_kinds() {
    let source = [
        "class Service extends Base {",
        "  constructor(value: Input);",
        "  constructor(value: Input) { super(); }",
        "  method(required: Input, fallback = value, ...rest): Result;",
        "  static [key](value = fallback): Result;",
        "  'literal'();",
        "  0();",
        "  #private(value: Input): Output;",
        "  implemented() {}",
        "  static constructor() {}",
        "  ['constructor']() {}",
        "}",
        "function following(value: Input): Output { return value; }",
        "import { value } from 'module';",
    ]
    .join("\n");
    let parsed = parse(&source, typescript_options()).expect("parse bodyless class signatures");
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed
        .tape
        .validate()
        .expect("valid bodyless-signature tape");

    assert_eq!(NodeTag::TS_EMPTY_BODY_FUNCTION_EXPRESSION.get(), 571);
    assert!(
        node_fields(&parsed, NodeTag::TS_EMPTY_BODY_FUNCTION_EXPRESSION)
            .all(|fields| fields.len() == 5)
    );
    assert_eq!(
        node_fields(&parsed, NodeTag::TS_EMPTY_BODY_FUNCTION_EXPRESSION).count(),
        6
    );
    assert_eq!(
        node_fields(&parsed, NodeTag::FUNCTION_EXPRESSION).count(),
        4
    );

    let constructor_count = node_fields(&parsed, NodeTag::METHOD_DEFINITION)
        .filter(|fields| matches!(parsed.tape.value_at(fields[2]), Ok(TapeValue::U32(3))))
        .count();
    assert_eq!(constructor_count, 2);
    assert_child_tag(
        &parsed,
        NodeTag::METHOD_DEFINITION,
        1,
        NodeTag::TS_EMPTY_BODY_FUNCTION_EXPRESSION,
    );

    let fields = node_fields(&parsed, NodeTag::TS_EMPTY_BODY_FUNCTION_EXPRESSION)
        .find(|fields| {
            matches!(
                parsed.tape.value_at(fields[4]),
                Ok(TapeValue::Node {
                    tag: NodeTag::TS_TYPE_ANNOTATION,
                    ..
                })
            )
        })
        .expect("annotated bodyless method");
    let TapeValue::Node { span, .. } = parsed
        .tape
        .value_at(fields[4])
        .expect("bodyless return annotation")
    else {
        panic!("bodyless return annotation is not a node");
    };
    assert_eq!(&source[span.start as usize..span.end as usize], ": Result");
}

#[test]
fn parses_typescript_class_member_modifiers_on_cold_tape_tags() {
    let source = [
        "class Base {}",
        "class Derived extends Base {",
        "  public constructor() { super(); }",
        "  protected static declared(): Output;",
        "  private field: Input;",
        "  readonly value = initial;",
        "  public override method() {}",
        "  override readonly size = 1;",
        "  protected get item() { return this.field; }",
        "}",
    ]
    .join("\n");
    let parsed = parse(&source, typescript_options()).expect("parse modified class members");
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed.tape.validate().expect("valid modified-member tape");

    assert_eq!(NodeTag::TS_MODIFIED_METHOD_DEFINITION.get(), 573);
    assert_eq!(NodeTag::TS_MODIFIED_PROPERTY_DEFINITION.get(), 574);
    let method_flags: Vec<_> = parsed
        .tape
        .validation()
        .map(|record| record.expect("valid record").value)
        .filter_map(|value| match value {
            TapeValue::Node {
                tag: NodeTag::TS_MODIFIED_METHOD_DEFINITION,
                flags,
                fields,
                ..
            } => {
                assert_eq!(fields.len(), 5);
                Some(flags)
            }
            _ => None,
        })
        .collect();
    let property_flags: Vec<_> = parsed
        .tape
        .validation()
        .map(|record| record.expect("valid record").value)
        .filter_map(|value| match value {
            TapeValue::Node {
                tag: NodeTag::TS_MODIFIED_PROPERTY_DEFINITION,
                flags,
                fields,
                ..
            } => {
                assert_eq!(fields.len(), 5);
                Some(flags)
            }
            _ => None,
        })
        .collect();
    assert_eq!(method_flags, [1, 2, 9, 2]);
    assert_eq!(property_flags, [3, 4, 12]);
}

#[test]
fn keeps_typescript_modifier_words_ambiguous_and_decodes_escaped_spellings() {
    let source = [
        "class Base {}",
        "class Names extends Base {",
        "  public() {}",
        "  private() {}",
        "  protected;",
        "  readonly = 0;",
        "  override() {}",
        "  public static() {}",
        "  override readonly() {}",
        "  static static() {}",
        "  p\\u0075blic escapedField;",
        "  r\\u0065adonly escapedReadonly;",
        "  ov\\u0065rride escapedOverride() {}",
        "  public",
        "  private() {}",
        "  static",
        "  readonly",
        "  protected() {}",
        "  async = 1;",
        "  async() {}",
        "}",
    ]
    .join("\n");
    let parsed = parse(&source, typescript_options()).expect("parse modifier ambiguities");
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed.tape.validate().expect("valid ambiguity tape");

    let modified_methods = node_fields(&parsed, NodeTag::TS_MODIFIED_METHOD_DEFINITION).count();
    let modified_properties =
        node_fields(&parsed, NodeTag::TS_MODIFIED_PROPERTY_DEFINITION).count();
    assert_eq!(modified_methods, 3);
    assert_eq!(modified_properties, 2);
    assert_eq!(node_fields(&parsed, NodeTag::METHOD_DEFINITION).count(), 7);
    assert_eq!(
        node_fields(&parsed, NodeTag::PROPERTY_DEFINITION).count(),
        5
    );

    for source in [
        "class C { public<T>() {} }",
        "class C { private?: number }",
        "class C { protected!: number }",
        "class C { readonly: number }",
        "class C { override() {} }",
        "class C { static<T>() {} }",
    ] {
        let parsed = parse(source, typescript_options()).expect("recover excluded ambiguity form");
        parsed
            .tape
            .validate()
            .expect("valid excluded ambiguity tape");
        assert_eq!(
            node_fields(&parsed, NodeTag::TS_MODIFIED_METHOD_DEFINITION).count()
                + node_fields(&parsed, NodeTag::TS_MODIFIED_PROPERTY_DEFINITION).count(),
            0,
            "{source}"
        );
    }
}

#[test]
fn gates_class_member_modifiers_and_preserves_ordinary_compatibility_tapes() {
    let source = "class C { static method() {} field = 1; readonly() {} async = 2; }";
    let javascript = parse(source, ParseOptions::default()).expect("parse JavaScript class");
    let compatibility = parse(
        source,
        ParseOptions {
            syntax_extensions: SyntaxExtensions {
                typescript_js_compatibility: true,
                ..SyntaxExtensions::default()
            },
            ..ParseOptions::default()
        },
    )
    .expect("parse compatibility class");
    assert_eq!(javascript.tape.words(), compatibility.tape.words());

    for language in [Language::JavaScript, Language::JavaScriptJsx] {
        let parsed = parse(
            "class C { public field; protected method() {} }",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("recover gated modifiers");
        assert!(!parsed.diagnostics.is_empty(), "{language:?}");
        assert_eq!(
            node_fields(&parsed, NodeTag::TS_MODIFIED_METHOD_DEFINITION).count()
                + node_fields(&parsed, NodeTag::TS_MODIFIED_PROPERTY_DEFINITION).count(),
            0
        );
    }

    let compatibility = parse(
        "class C { public field; protected method() {} }",
        ParseOptions {
            syntax_extensions: SyntaxExtensions {
                typescript_js_compatibility: true,
                ..SyntaxExtensions::default()
            },
            ..ParseOptions::default()
        },
    )
    .expect("parse compatibility modifiers");
    assert!(
        compatibility.diagnostics.is_empty(),
        "{:#?}",
        compatibility.diagnostics
    );
    assert_eq!(
        node_fields(&compatibility, NodeTag::TS_MODIFIED_METHOD_DEFINITION).count(),
        1
    );
    assert_eq!(
        node_fields(&compatibility, NodeTag::TS_MODIFIED_PROPERTY_DEFINITION).count(),
        1
    );
}

#[test]
fn diagnoses_invalid_typescript_class_member_modifier_combinations() {
    for source in [
        "class C { readonly method() {} }",
        "class C extends B { override constructor() {} }",
        "class C { override method() {} }",
        "class C { public #field; }",
        "class C { public static {} }",
        "class C { readonly public field; }",
        "class C { public constructor; }",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                semantic_errors: true,
                ..typescript_options()
            },
        )
        .expect("recover invalid modifiers");
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed
            .tape
            .validate()
            .expect("valid modifier recovery tape");
    }

    for source in [
        "class C { constructor; }",
        "class C { public constructor; }",
        "class C { public static constructor; }",
        "class C { constructor: number; }",
        "class C { constructor = 1; }",
        "class C { public constr\\u0075ctor; }",
    ] {
        let syntax_only = parse(source, typescript_options()).expect("recover constructor field");
        assert!(!syntax_only.diagnostics.is_empty(), "{source}");
    }
    for source in [
        "class C { public 'constructor'; }",
        "class C { public ['constructor']; }",
    ] {
        let syntax_only =
            parse(source, typescript_options()).expect("parse constructor-like field");
        assert!(
            syntax_only.diagnostics.is_empty(),
            "{source}: {:#?}",
            syntax_only.diagnostics
        );
    }
}

#[test]
fn parses_abstract_classes_and_members_on_cold_tape_records() {
    let source = [
        "abstract class Plain {",
        "  public abstract method(value: Input): Output;",
        "  abstract readonly field: Input;",
        "  abstract #privateMethod(): void;",
        "}",
        "export abstract class Derived<T> extends Base implements Contract<T> {",
        "  abstract override inherited(): T;",
        "  protected abstract property: T;",
        "}",
        "export default abstract class {}",
    ]
    .join("\n");
    let parsed = parse(
        &source,
        ParseOptions {
            language: Language::TypeScript,
            semantic_errors: true,
            source_kind: SourceKind::Module,
            ..ParseOptions::default()
        },
    )
    .expect("parse abstract classes");
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed.tape.validate().expect("valid abstract-class tape");

    assert_eq!(NodeTag::TS_ABSTRACT_METHOD_DEFINITION.get(), 576);
    assert_eq!(NodeTag::TS_ABSTRACT_PROPERTY_DEFINITION.get(), 577);
    let abstract_class_flags: Vec<_> = parsed
        .tape
        .validation()
        .map(|record| record.expect("valid record").value)
        .filter_map(|value| match value {
            TapeValue::Node {
                tag: NodeTag::CLASS_DECLARATION | NodeTag::TS_GENERIC_CLASS_DECLARATION,
                flags,
                ..
            } => (flags != 0).then_some(flags),
            _ => None,
        })
        .collect();
    assert_eq!(abstract_class_flags, [1, 1, 1]);

    let method_flags: Vec<_> = parsed
        .tape
        .validation()
        .map(|record| record.expect("valid record").value)
        .filter_map(|value| match value {
            TapeValue::Node {
                tag: NodeTag::TS_ABSTRACT_METHOD_DEFINITION,
                flags,
                fields,
                ..
            } => {
                assert_eq!(fields.len(), 5);
                Some(flags)
            }
            _ => None,
        })
        .collect();
    let property_flags: Vec<_> = parsed
        .tape
        .validation()
        .map(|record| record.expect("valid record").value)
        .filter_map(|value| match value {
            TapeValue::Node {
                tag: NodeTag::TS_ABSTRACT_PROPERTY_DEFINITION,
                flags,
                fields,
                ..
            } => {
                assert_eq!(fields.len(), 5);
                Some(flags)
            }
            _ => None,
        })
        .collect();
    assert_eq!(method_flags, [1, 0, 8]);
    assert_eq!(property_flags, [4, 2]);
    assert!(
        node_fields(&parsed, NodeTag::TS_ABSTRACT_METHOD_DEFINITION).all(|fields| {
            matches!(
                parsed.tape.value_at(fields[1]),
                Ok(TapeValue::Node {
                    tag: NodeTag::TS_EMPTY_BODY_FUNCTION_EXPRESSION,
                    ..
                })
            )
        })
    );
}

#[test]
fn parses_abstract_accessor_async_and_generator_signatures_without_swallowing_members() {
    let source = [
        "abstract class Signatures {",
        "  abstract get value(): string;",
        "  abstract set value(next: string);",
        "  abstract async load(): Promise<string>;",
        "  abstract *values(): IterableIterator<string>;",
        "  after: string;",
        "}",
    ]
    .join("\n");
    let parsed = parse(
        &source,
        ParseOptions {
            language: Language::TypeScript,
            semantic_errors: true,
            ..ParseOptions::default()
        },
    )
    .expect("parse abstract method signature forms");
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed.tape.validate().expect("valid abstract method tape");
    assert_eq!(
        node_fields(&parsed, NodeTag::TS_ABSTRACT_METHOD_DEFINITION).count(),
        4
    );
    assert_eq!(
        node_fields(&parsed, NodeTag::TS_EMPTY_BODY_FUNCTION_EXPRESSION).count(),
        4
    );
    assert_eq!(
        node_fields(&parsed, NodeTag::PROPERTY_DEFINITION).count(),
        1,
        "the member after the signatures must remain a separate field"
    );
    assert!(
        node_fields(&parsed, NodeTag::TS_EMPTY_BODY_FUNCTION_EXPRESSION)
            .any(|fields| matches!(parsed.tape.value_at(fields[2]), Ok(TapeValue::Bool(true))))
    );
    assert!(
        node_fields(&parsed, NodeTag::TS_EMPTY_BODY_FUNCTION_EXPRESSION)
            .any(|fields| matches!(parsed.tape.value_at(fields[3]), Ok(TapeValue::Bool(true))))
    );
}

#[test]
fn keeps_abstract_contextual_at_class_and_member_boundaries() {
    let source = [
        "abstract",
        "class Ordinary {}",
        "abstract as Type;",
        "abstract satisfies Type;",
        "abstract in value;",
        "abstract instanceof Constructor;",
        "export default abstract;",
        "class Names {",
        "  abstract();",
        "  abstract!: void;",
        "  abstract",
        "  method();",
        "}",
    ]
    .join("\n");
    let parsed = parse(
        &source,
        ParseOptions {
            language: Language::TypeScript,
            source_kind: SourceKind::Module,
            ..ParseOptions::default()
        },
    )
    .expect("parse abstract ambiguities");
    parsed.tape.validate().expect("valid ambiguity tape");
    assert_eq!(
        node_fields(&parsed, NodeTag::TS_ABSTRACT_METHOD_DEFINITION).count()
            + node_fields(&parsed, NodeTag::TS_ABSTRACT_PROPERTY_DEFINITION).count(),
        0
    );
    assert!(
        parsed
            .tape
            .validation()
            .map(|record| record.expect("valid record").value)
            .all(|value| !matches!(value, TapeValue::Node { flags: 1, tag, .. } if matches!(tag, NodeTag::CLASS_DECLARATION | NodeTag::TS_CLASS_DECLARATION | NodeTag::TS_GENERIC_CLASS_DECLARATION)))
    );

    let compatibility = parse(
        "abstract class Compatible { abstract member(); }",
        ParseOptions {
            syntax_extensions: SyntaxExtensions {
                typescript_js_compatibility: true,
                ..SyntaxExtensions::default()
            },
            ..ParseOptions::default()
        },
    )
    .expect("parse compatibility abstract class");
    assert!(
        compatibility.diagnostics.is_empty(),
        "{:#?}",
        compatibility.diagnostics
    );
    assert_eq!(
        node_fields(&compatibility, NodeTag::TS_ABSTRACT_METHOD_DEFINITION).count(),
        1
    );

    for language in [Language::JavaScript, Language::JavaScriptJsx] {
        let javascript = parse(
            "abstract class Gated { abstract member(); }",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("recover gated abstract class");
        assert!(!javascript.diagnostics.is_empty(), "{language:?}");
        assert_eq!(
            node_fields(&javascript, NodeTag::TS_ABSTRACT_METHOD_DEFINITION).count()
                + node_fields(&javascript, NodeTag::TS_ABSTRACT_PROPERTY_DEFINITION).count(),
            0
        );
    }
}

#[test]
fn diagnoses_invalid_abstract_class_member_combinations() {
    for source in [
        "class C { abstract method(); }",
        "abstract class C { abstract method() {} }",
        "abstract class C { abstract property = 1; }",
        "abstract class C { static abstract method(); }",
        "abstract class C { abstract constructor(); }",
        "abstract class C { override abstract method(); }",
        "abstract class C { abstract abstract method(); }",
        "abstract class C { abstract static {} }",
        "abstract class C { abstract #field: number; }",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                semantic_errors: true,
                ..typescript_options()
            },
        )
        .expect("recover invalid abstract member");
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed
            .tape
            .validate()
            .expect("valid abstract recovery tape");
    }

    let private_method = parse(
        "abstract class C { abstract #method(): void; }",
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("parse private abstract method");
    assert!(
        private_method.diagnostics.is_empty(),
        "{:#?}",
        private_method.diagnostics
    );
}

#[test]
fn gates_bodyless_class_signatures_and_requires_explicit_semicolons() {
    for language in [
        Language::TypeScript,
        Language::TypeScriptJsx,
        Language::TypeScriptDefinition,
    ] {
        let parsed = parse(
            "class C { method(value: Input): Output; }",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("parse TypeScript-capable bodyless method");
        assert!(
            parsed.diagnostics.is_empty(),
            "{language:?}: {:#?}",
            parsed.diagnostics
        );
        assert_node_field_count(&parsed, NodeTag::TS_EMPTY_BODY_FUNCTION_EXPRESSION, 5);
    }

    let compatibility = parse(
        "class C { method(); }",
        ParseOptions {
            syntax_extensions: SyntaxExtensions {
                typescript_js_compatibility: true,
                ..SyntaxExtensions::default()
            },
            ..ParseOptions::default()
        },
    )
    .expect("parse compatibility bodyless method");
    assert!(
        compatibility.diagnostics.is_empty(),
        "{:#?}",
        compatibility.diagnostics
    );

    for language in [Language::JavaScript, Language::JavaScriptJsx] {
        let parsed = parse(
            "class C { method(); }",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("recover JavaScript bodyless method");
        assert!(!parsed.diagnostics.is_empty(), "{language:?}");
        parsed
            .tape
            .validate()
            .expect("valid JavaScript recovery tape");
        assert_eq!(
            node_fields(&parsed, NodeTag::TS_EMPTY_BODY_FUNCTION_EXPRESSION).count(),
            0
        );
    }

    for source in [
        "class C { method(): Output\nnext() {} }",
        "class C { async method(): Promise<Output>; }",
        "class C { *method(): Iterable<Output>; }",
        "class C { get value(): Output; }",
        "const value = { method(); };",
    ] {
        let parsed = parse(source, typescript_options()).expect("recover excluded signature form");
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect("valid excluded-form tape");
    }
}

#[test]
fn diagnoses_unsupported_method_return_forms_and_setter_semantics() {
    for source in [
        "class C { predicate(value): value is string {} }",
        "class C { assertion(value): asserts value {} }",
        "class C { constructor(): string {} }",
    ] {
        let parsed = parse(source, typescript_options()).expect("recoverable method parse");
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect("valid recovery tape");
    }

    for language in [Language::JavaScript, Language::JavaScriptJsx] {
        let parsed = parse(
            "class C { method(): string {} }",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("recoverable JavaScript method parse");
        assert!(!parsed.diagnostics.is_empty(), "{language:?}");
        parsed
            .tape
            .validate()
            .expect("valid JavaScript recovery tape");
    }

    let source =
        "class C { set value(next): void {} } const object = { set value(next): void {} };";
    let syntax_only = parse(source, typescript_options()).expect("syntax-only setter parse");
    assert!(syntax_only.diagnostics.is_empty());
    assert!(
        node_fields(&syntax_only, NodeTag::FUNCTION_EXPRESSION).all(|fields| fields.len() == 6)
    );

    let semantic = parse(
        source,
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("semantic setter parse");
    assert_eq!(semantic.diagnostics.len(), 2, "{:#?}", semantic.diagnostics);
    assert!(node_fields(&semantic, NodeTag::FUNCTION_EXPRESSION).all(|fields| fields.len() == 6));
}

#[test]
fn requires_super_to_continue_as_a_call_or_property() {
    let invalid = parse(
        "class C extends Base { method(): void { super; } }",
        typescript_options(),
    )
    .expect("recoverable bare-super parse");
    assert!(!invalid.diagnostics.is_empty());
    invalid.tape.validate().expect("valid bare-super tape");

    let valid = parse(
        "class C extends Base { constructor() { super(); } method() { super.value; super[key]; } }",
        typescript_options(),
    )
    .expect("parse valid super continuations");
    assert!(valid.diagnostics.is_empty(), "{:#?}", valid.diagnostics);
}

#[test]
fn parses_typescript_export_assignment_and_namespace_export() {
    let source = "export = Namespace.factory; export as namespace JetSyntax;";
    assert_clean_with_tags(
        "TypeScript export forms",
        source,
        &[
            NodeTag::TS_EXPORT_ASSIGNMENT,
            NodeTag::TS_NAMESPACE_EXPORT_DECLARATION,
        ],
    );

    let parsed = parse(source, typescript_options()).expect("parse TypeScript export forms");
    assert_child_tag(
        &parsed,
        NodeTag::TS_EXPORT_ASSIGNMENT,
        0,
        NodeTag::MEMBER_EXPRESSION,
    );
    assert_child_tag(
        &parsed,
        NodeTag::TS_NAMESPACE_EXPORT_DECLARATION,
        0,
        NodeTag::IDENTIFIER,
    );
    assert_node_field_count(&parsed, NodeTag::TS_EXPORT_ASSIGNMENT, 1);
    assert_node_field_count(&parsed, NodeTag::TS_NAMESPACE_EXPORT_DECLARATION, 1);
}

#[test]
fn malformed_typescript_exports_recover_to_valid_tapes() {
    for source in ["export = ;", "export as value;"] {
        let parsed = parse(source, typescript_options()).expect("recoverable TypeScript parse");
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect("valid recovery tape");
    }
}

#[test]
fn keeps_typescript_export_forms_out_of_javascript() {
    let parsed = parse(
        "export = value; export as namespace Library;",
        ParseOptions {
            language: Language::JavaScript,
            ..ParseOptions::default()
        },
    )
    .expect("recoverable JavaScript parse");
    assert!(!parsed.diagnostics.is_empty());
    parsed.tape.validate().expect("valid recovery tape");

    let tags: Vec<_> = parsed
        .tape
        .validation()
        .map(|record| record.expect("valid record").value)
        .filter_map(|value| match value {
            TapeValue::Node { tag, .. } => Some(tag),
            _ => None,
        })
        .collect();
    assert!(!tags.contains(&NodeTag::TS_EXPORT_ASSIGNMENT));
    assert!(!tags.contains(&NodeTag::TS_NAMESPACE_EXPORT_DECLARATION));
}

#[test]
fn parses_typescript_expression_wrappers_without_diagnostics() {
    let cases = [
        (
            "as expression",
            "const value = input as Namespace.Model;",
            &[NodeTag::TS_AS_EXPRESSION, NodeTag::TS_QUALIFIED_NAME][..],
        ),
        (
            "const assertion",
            "const value = { state: 'ready' } as const;",
            &[
                NodeTag::TS_AS_EXPRESSION,
                NodeTag::OBJECT_EXPRESSION,
                NodeTag::TS_TYPE_REFERENCE,
            ][..],
        ),
        (
            "satisfies expression",
            "const value = { state: 'ready' } satisfies Model;",
            &[NodeTag::TS_SATISFIES_EXPRESSION][..],
        ),
        (
            "postfix non-null expression",
            "const value = optional!.member!;",
            &[NodeTag::TS_NON_NULL_EXPRESSION, NodeTag::MEMBER_EXPRESSION][..],
        ),
        (
            "angle-bracket type assertion",
            "const value = <Namespace.Model>input;",
            &[NodeTag::TS_TYPE_ASSERTION, NodeTag::TS_QUALIFIED_NAME][..],
        ),
        (
            "chained expression wrappers",
            "const value = input! as Model satisfies Constraint;",
            &[
                NodeTag::TS_NON_NULL_EXPRESSION,
                NodeTag::TS_AS_EXPRESSION,
                NodeTag::TS_SATISFIES_EXPRESSION,
            ][..],
        ),
    ];

    for (name, source, expected_tags) in cases {
        assert_clean_with_tags(name, source, expected_tags);
    }

    let parsed = parse(
        "const value = input! as Model satisfies Constraint;",
        typescript_options(),
    )
    .expect("parse expression wrappers");
    assert_child_tag(
        &parsed,
        NodeTag::TS_AS_EXPRESSION,
        0,
        NodeTag::TS_NON_NULL_EXPRESSION,
    );
    assert_child_tag(
        &parsed,
        NodeTag::TS_AS_EXPRESSION,
        1,
        NodeTag::TS_TYPE_REFERENCE,
    );
    assert_child_tag(
        &parsed,
        NodeTag::TS_SATISFIES_EXPRESSION,
        0,
        NodeTag::TS_AS_EXPRESSION,
    );
    assert_child_tag(
        &parsed,
        NodeTag::TS_SATISFIES_EXPRESSION,
        1,
        NodeTag::TS_TYPE_REFERENCE,
    );

    let parsed = parse("const value = left + right as Model;", typescript_options())
        .expect("parse assertion after additive expression");
    assert_child_tag(
        &parsed,
        NodeTag::TS_AS_EXPRESSION,
        0,
        NodeTag::BINARY_EXPRESSION,
    );

    let parsed = parse("const value = left as Model + right;", typescript_options())
        .expect("parse assertion before additive expression");
    assert_child_tag(
        &parsed,
        NodeTag::BINARY_EXPRESSION,
        1,
        NodeTag::TS_AS_EXPRESSION,
    );

    let parsed =
        parse("const value = <number>input;", typescript_options()).expect("parse type assertion");
    assert_child_tag(
        &parsed,
        NodeTag::TS_TYPE_ASSERTION,
        0,
        NodeTag::TS_NUMBER_KEYWORD,
    );
    assert_child_tag(&parsed, NodeTag::TS_TYPE_ASSERTION, 1, NodeTag::IDENTIFIER);
}

/// TypeScript expression-only wrappers preserve the assignment target of their operand.
#[test]
fn validates_assignment_targets_through_typescript_wrappers() {
    let valid = [
        "value as unknown = source;",
        "value satisfies unknown = source;",
        "value! += source;",
        "(value as unknown)++;",
        "(<unknown>value)++;",
    ];
    for source in valid {
        let parsed = parse(
            source,
            ParseOptions {
                semantic_errors: true,
                ..typescript_options()
            },
        )
        .expect(source);
        assert!(
            parsed.diagnostics.is_empty(),
            "{source}: {:?}",
            parsed.diagnostics
        );
        parsed.tape.validate().expect(source);
    }

    let invalid = [
        "(value + offset) as unknown = source;",
        "factory() as unknown ||= source;",
        "optional?.member! = source;",
        "'use strict'; (eval as unknown) = source;",
    ];
    for source in invalid {
        let parsed = parse(
            source,
            ParseOptions {
                semantic_errors: true,
                ..typescript_options()
            },
        )
        .expect(source);
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect(source);
    }
}

#[test]
fn keeps_angle_bracket_type_assertions_out_of_tsx() {
    let parsed = parse(
        "const value = <number>input;",
        ParseOptions {
            language: Language::TypeScriptJsx,
            ..ParseOptions::default()
        },
    )
    .expect("recoverable TSX parse");
    assert!(!parsed.diagnostics.is_empty());
    parsed.tape.validate().expect("valid recovery tape");

    let has_type_assertion = parsed
        .tape
        .validation()
        .map(|record| record.expect("valid record").value)
        .any(|value| {
            matches!(
                value,
                TapeValue::Node {
                    tag: NodeTag::TS_TYPE_ASSERTION,
                    ..
                }
            )
        });
    assert!(!has_type_assertion);
}

#[test]
fn parses_typescript_import_equals_declarations() {
    let source = [
        "import Alias = Namespace.Deep.Member;",
        "import external = require(\"package\");",
        "import type types = require(\"types\");",
        "import type = require(\"type-name\");",
        "export import Public = Namespace.Member;",
        "export import type PublicTypes = require(\"public-types\");",
        "namespace Local { import Inner = Namespace.Member; }",
        "import type\nAcrossLines = require(\"line-break\");",
    ]
    .join("\n");
    let parsed = parse(&source, typescript_options()).expect("parse import aliases");
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed.tape.validate().expect("valid tape");

    assert_eq!(NodeTag::TS_IMPORT_EQUALS_DECLARATION.get(), 563);
    assert_eq!(NodeTag::TS_EXTERNAL_MODULE_REFERENCE.get(), 564);
    assert_child_tag(
        &parsed,
        NodeTag::TS_IMPORT_EQUALS_DECLARATION,
        0,
        NodeTag::IDENTIFIER,
    );
    assert_child_tag(
        &parsed,
        NodeTag::TS_IMPORT_EQUALS_DECLARATION,
        1,
        NodeTag::TS_QUALIFIED_NAME,
    );
    assert_child_tag(
        &parsed,
        NodeTag::TS_IMPORT_EQUALS_DECLARATION,
        1,
        NodeTag::TS_EXTERNAL_MODULE_REFERENCE,
    );
    assert_child_tag(
        &parsed,
        NodeTag::TS_EXTERNAL_MODULE_REFERENCE,
        0,
        NodeTag::LITERAL,
    );
    assert_child_tag(
        &parsed,
        NodeTag::EXPORT_NAMED_DECLARATION,
        0,
        NodeTag::TS_IMPORT_EQUALS_DECLARATION,
    );
    assert_list_child_tag(
        &parsed,
        NodeTag::TS_MODULE_BLOCK,
        0,
        NodeTag::TS_IMPORT_EQUALS_DECLARATION,
    );
    assert_node_field_count(&parsed, NodeTag::TS_IMPORT_EQUALS_DECLARATION, 3);
    assert_node_field_count(&parsed, NodeTag::TS_EXTERNAL_MODULE_REFERENCE, 1);
}

#[test]
fn distinguishes_import_equals_contextual_tokens_and_bindings() {
    let source = [
        "namespace M {}",
        "import alias = M;",
        "var alias;",
        "var reverse;",
        "import reverse = M;",
        "import type from \"ordinary\";",
    ]
    .join("\n");
    let parsed = parse(
        &source,
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("parse contextual import aliases");
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed.tape.validate().expect("valid tape");
    assert_clean_with_tags(
        "type identifier alias",
        "import type = require(\"type-name\");",
        &[
            NodeTag::TS_IMPORT_EQUALS_DECLARATION,
            NodeTag::TS_EXTERNAL_MODULE_REFERENCE,
        ],
    );
    assert_clean_with_tags(
        "ordinary type default import",
        "import type from \"ordinary\";",
        &[
            NodeTag::IMPORT_DECLARATION,
            NodeTag::IMPORT_DEFAULT_SPECIFIER,
        ],
    );
}

#[test]
fn malformed_import_equals_declarations_recover_to_valid_tapes() {
    for source in [
        "import Alias = ;",
        "import Alias = require(name);",
        "import Alias = require(\"a\", \"b\");",
        "import Alias = require(\"a\";",
        "import Alias = r\\u0065quire(\"a\");",
        "import \\u0074ype Alias = require(\"a\");",
        "import type Alias = Namespace.Member;",
        "namespace Local { import Alias = require(\"a\"); }",
    ] {
        let parsed = parse(source, typescript_options())
            .unwrap_or_else(|error| panic!("{source}: {error:?}"));
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect("valid recovery tape");
    }

    let parsed = parse("import Alias = Namespace.Member;", ParseOptions::default())
        .expect("recoverable JavaScript parse");
    assert!(!parsed.diagnostics.is_empty());
    parsed
        .tape
        .validate()
        .expect("valid JavaScript recovery tape");
    assert!(
        node_fields(&parsed, NodeTag::TS_IMPORT_EQUALS_DECLARATION)
            .next()
            .is_none()
    );

    let parsed = parse(
        "import Alias = Namespace.Member;",
        ParseOptions {
            source_kind: jetsyntax::SourceKind::Script,
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("recoverable script parse");
    assert!(!parsed.diagnostics.is_empty());
    parsed.tape.validate().expect("valid script recovery tape");
}

#[test]
fn malformed_typescript_declarations_recover_to_valid_tapes() {
    for source in [
        "type Missing = ;",
        "interface Broken<T { value: T }",
        "enum Broken { = 1, Valid }",
    ] {
        let parsed = parse(source, typescript_options()).expect("recoverable parse");
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect("valid recovery tape");
    }
}

#[test]
fn parses_direct_generic_new_expressions_without_widening_javascript_records() {
    let source = [
        "new Plain();",
        "new Factory<Input>(value);",
        "new Namespace.Factory<Map<Key, Value>>;",
        "new Factory<<T>(value: T) => void>();",
    ]
    .join("\n");
    let parsed = parse(&source, typescript_options()).expect("parse generic new expressions");
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed.tape.validate().expect("valid generic-new tape");

    assert_eq!(first_node_fields(&parsed, NodeTag::NEW_EXPRESSION).len(), 2);
    let expressions = node_fields(&parsed, NodeTag::TS_NEW_EXPRESSION).collect::<Vec<_>>();
    assert_eq!(expressions.len(), 3);
    assert!(expressions.iter().all(|fields| fields.len() == 3));
    for fields in &expressions {
        assert!(matches!(
            parsed.tape.value_at(fields[2]),
            Ok(TapeValue::Node {
                tag: NodeTag::TS_TYPE_PARAMETER_INSTANTIATION,
                ..
            })
        ));
    }

    let TapeValue::Node { span, .. } = parsed
        .tape
        .value_at(expressions[0][2])
        .expect("type arguments")
    else {
        panic!("type arguments are not a node");
    };
    assert_eq!(&source[span.start as usize..span.end as usize], "<Input>");
}

#[test]
#[allow(clippy::too_many_lines)]
fn disambiguates_direct_generic_new_expressions_from_relational_expressions() {
    for source in [
        "new A<T>();",
        "new A<T>;",
        "new A < B >\nC;",
        "new A<T> * value;",
    ] {
        let parsed = parse(source, typescript_options()).expect("parse generic new expression");
        assert!(
            parsed.diagnostics.is_empty(),
            "{source}: {:#?}",
            parsed.diagnostics
        );
        parsed.tape.validate().expect("valid generic-new tape");
        assert_eq!(
            first_node_fields(&parsed, NodeTag::TS_NEW_EXPRESSION).len(),
            3
        );
    }

    for source in ["new A < B > C;", "new A<T> + value;"] {
        let parsed = parse(source, typescript_options()).expect("parse relational expression");
        assert!(
            parsed.diagnostics.is_empty(),
            "{source}: {:#?}",
            parsed.diagnostics
        );
        parsed.tape.validate().expect("valid relational tape");
        assert_eq!(first_node_fields(&parsed, NodeTag::NEW_EXPRESSION).len(), 2);
        assert_eq!(
            node_fields(&parsed, NodeTag::TS_TYPE_PARAMETER_INSTANTIATION).count(),
            0
        );
        assert_eq!(node_fields(&parsed, NodeTag::TS_NEW_EXPRESSION).count(), 0);
    }

    let greater_equal = parse("new A<T>=value;", typescript_options())
        .expect("parse greater-than-or-equal relational expression");
    assert!(
        greater_equal.diagnostics.is_empty(),
        "{:#?}",
        greater_equal.diagnostics
    );
    greater_equal
        .tape
        .validate()
        .expect("valid relational tape");
    assert_eq!(
        first_node_fields(&greater_equal, NodeTag::NEW_EXPRESSION).len(),
        2
    );
    assert_eq!(
        node_fields(&greater_equal, NodeTag::TS_TYPE_PARAMETER_INSTANTIATION).count(),
        0
    );
    assert_eq!(
        node_fields(&greater_equal, NodeTag::TS_NEW_EXPRESSION).count(),
        0
    );

    let shift_assign = parse("new A<T>>=value;", typescript_options())
        .expect("recover shift assignment expression");
    assert!(!shift_assign.diagnostics.is_empty());
    shift_assign
        .tape
        .validate()
        .expect("valid shift-assignment recovery tape");
    assert_eq!(
        first_node_fields(&shift_assign, NodeTag::NEW_EXPRESSION).len(),
        2
    );
    assert_eq!(
        node_fields(&shift_assign, NodeTag::TS_TYPE_PARAMETER_INSTANTIATION).count(),
        0
    );
    assert_eq!(
        node_fields(&shift_assign, NodeTag::TS_NEW_EXPRESSION).count(),
        0
    );

    let tsx = parse(
        "new A<T>();",
        ParseOptions {
            language: Language::TypeScriptJsx,
            ..ParseOptions::default()
        },
    )
    .expect("parse TSX generic new expression");
    assert!(tsx.diagnostics.is_empty(), "{:#?}", tsx.diagnostics);
    assert_eq!(first_node_fields(&tsx, NodeTag::TS_NEW_EXPRESSION).len(), 3);

    for language in [Language::JavaScript, Language::JavaScriptJsx] {
        let parsed = parse(
            "new A<T>();",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("recover JavaScript angle expression");
        parsed
            .tape
            .validate()
            .expect("valid JavaScript recovery tape");
        assert!(node_fields(&parsed, NodeTag::NEW_EXPRESSION).all(|fields| fields.len() == 2));
        assert_eq!(node_fields(&parsed, NodeTag::TS_NEW_EXPRESSION).count(), 0);
        assert_eq!(
            node_fields(&parsed, NodeTag::TS_TYPE_PARAMETER_INSTANTIATION).count(),
            0
        );
    }
}

#[test]
fn rolls_back_malformed_generic_new_speculation_without_stale_records() {
    let malformed = parse(r"new A<\x + value;", typescript_options())
        .expect("recover malformed relational expression");
    assert!(!malformed.diagnostics.is_empty());
    malformed.tape.validate().expect("valid rollback tape");
    assert_eq!(
        malformed
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message == "identifier escape must use Unicode syntax")
            .count(),
        1,
        "lexical errors must not leak from the speculative branch"
    );
    assert_eq!(
        node_fields(&malformed, NodeTag::TS_TYPE_PARAMETER_INSTANTIATION).count(),
        0,
        "missing `>` must discard speculative type nodes"
    );

    for source in ["new A<>();", "new A<T>.value;", "new A<T>?.value;"] {
        let parsed = parse(source, typescript_options()).expect("recover invalid generic new");
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect("valid recovery tape");
        assert_eq!(
            first_node_fields(&parsed, NodeTag::TS_NEW_EXPRESSION).len(),
            3
        );
    }
}

#[test]
fn parses_typescript_class_implements_clauses_and_preserves_legacy_class_records() {
    let source = [
        "class Plain {}",
        "class Derived extends Base implements One, Namespace.Two<Map<Key, Value>>, Constructor<<T>(value: T) => T> {}",
        "(class implements Anonymous<Inner<Value>> {});",
        "class Reordered implements First extends Base implements Second {}",
        "class Repeated extends Base extends Discarded implements Third {}",
    ]
    .join("\n");
    let parsed = parse(&source, typescript_options()).expect("parse class implements clauses");
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed.tape.validate().expect("valid class-implements tape");

    assert_node_field_count(&parsed, NodeTag::CLASS_DECLARATION, 3);
    assert!(node_fields(&parsed, NodeTag::TS_CLASS_DECLARATION).all(|fields| fields.len() == 4));
    assert_node_field_count(&parsed, NodeTag::TS_CLASS_EXPRESSION, 4);
    assert_eq!(
        node_fields(&parsed, NodeTag::TS_CLASS_IMPLEMENTS).count(),
        7
    );
    assert_child_tag(
        &parsed,
        NodeTag::TS_CLASS_IMPLEMENTS,
        0,
        NodeTag::MEMBER_EXPRESSION,
    );
    assert_child_tag(
        &parsed,
        NodeTag::TS_CLASS_IMPLEMENTS,
        1,
        NodeTag::TS_TYPE_PARAMETER_INSTANTIATION,
    );

    let spans = parsed
        .tape
        .validation()
        .map(|record| record.expect("valid record").value)
        .filter_map(|value| match value {
            TapeValue::Node {
                tag: NodeTag::TS_CLASS_IMPLEMENTS,
                span,
                ..
            } => Some(&source[span.start as usize..span.end as usize]),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        spans,
        [
            "One",
            "Namespace.Two<Map<Key, Value>>",
            "Constructor<<T>(value: T) => T>",
            "Anonymous<Inner<Value>>",
            "First",
            "Second",
            "Third",
        ]
    );

    let super_classes = node_fields(&parsed, NodeTag::TS_CLASS_DECLARATION)
        .map(|fields| {
            let TapeValue::Node { span, .. } = parsed
                .tape
                .value_at(fields[1])
                .expect("implemented class super class")
            else {
                panic!("implemented class super class is not a node");
            };
            &source[span.start as usize..span.end as usize]
        })
        .collect::<Vec<_>>();
    assert_eq!(super_classes, ["Base", "Base", "Base"]);
}

#[test]
fn parses_typescript_generic_classes_without_widening_existing_class_records() {
    let source = [
        "class Plain {}",
        "class Generic<T extends Constraint = Fallback> {}",
        "class Derived<Key, Value = Key> extends Base implements Repository<Key, Value> {}",
        "(class<Item> {});",
    ]
    .join("\n");
    let parsed = parse(&source, typescript_options()).expect("parse generic classes");
    assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
    parsed.tape.validate().expect("valid generic-class tape");

    assert_eq!(NodeTag::TS_GENERIC_CLASS_DECLARATION.get(), 569);
    assert_eq!(NodeTag::TS_GENERIC_CLASS_EXPRESSION.get(), 570);
    assert_node_field_count(&parsed, NodeTag::CLASS_DECLARATION, 3);
    assert!(
        node_fields(&parsed, NodeTag::TS_GENERIC_CLASS_DECLARATION).all(|fields| fields.len() == 5)
    );
    assert_node_field_count(&parsed, NodeTag::TS_GENERIC_CLASS_EXPRESSION, 5);
    assert_eq!(
        node_fields(&parsed, NodeTag::TS_GENERIC_CLASS_DECLARATION).count(),
        2
    );
    assert_eq!(
        node_fields(&parsed, NodeTag::TS_TYPE_PARAMETER_DECLARATION).count(),
        3
    );

    let generic_classes =
        node_fields(&parsed, NodeTag::TS_GENERIC_CLASS_DECLARATION).collect::<Vec<_>>();
    assert!(matches!(
        parsed.tape.value_at(generic_classes[0][3]),
        Ok(TapeValue::Null)
    ));
    assert!(matches!(
        parsed.tape.value_at(generic_classes[1][3]),
        Ok(TapeValue::List { items, .. }) if items.len() == 1
    ));
    assert_child_tag(
        &parsed,
        NodeTag::TS_GENERIC_CLASS_DECLARATION,
        4,
        NodeTag::TS_TYPE_PARAMETER_DECLARATION,
    );
    assert_child_tag(
        &parsed,
        NodeTag::TS_GENERIC_CLASS_EXPRESSION,
        4,
        NodeTag::TS_TYPE_PARAMETER_DECLARATION,
    );

    let anonymous = first_node_fields(&parsed, NodeTag::TS_GENERIC_CLASS_EXPRESSION);
    assert!(matches!(
        parsed.tape.value_at(anonymous[0]),
        Ok(TapeValue::Null)
    ));
}

#[test]
fn gates_and_recovers_typescript_generic_classes() {
    for language in [
        Language::TypeScript,
        Language::TypeScriptJsx,
        Language::TypeScriptDefinition,
    ] {
        let parsed = parse(
            "class Generic<T> {}",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("parse TypeScript-capable generic class");
        assert!(
            parsed.diagnostics.is_empty(),
            "{language:?}: {:#?}",
            parsed.diagnostics
        );
        assert_node_field_count(&parsed, NodeTag::TS_GENERIC_CLASS_DECLARATION, 5);
    }

    let compatibility = parse(
        "class Generic<T> {}",
        ParseOptions {
            syntax_extensions: SyntaxExtensions {
                typescript_js_compatibility: true,
                ..SyntaxExtensions::default()
            },
            ..ParseOptions::default()
        },
    )
    .expect("parse TypeScript JavaScript compatibility generic class");
    assert!(
        compatibility.diagnostics.is_empty(),
        "{:#?}",
        compatibility.diagnostics
    );
    assert_node_field_count(&compatibility, NodeTag::TS_GENERIC_CLASS_DECLARATION, 5);

    for language in [Language::JavaScript, Language::JavaScriptJsx] {
        let parsed = parse(
            "class Generic<T> {}",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("recover standard JavaScript class");
        assert!(!parsed.diagnostics.is_empty(), "{language:?}");
        parsed
            .tape
            .validate()
            .expect("valid JavaScript recovery tape");
        assert_eq!(
            node_fields(&parsed, NodeTag::TS_GENERIC_CLASS_DECLARATION).count(),
            0
        );
    }

    let empty = parse("class Empty<> {}", typescript_options())
        .expect("recover empty class type parameters");
    assert!(!empty.diagnostics.is_empty());
    empty.tape.validate().expect("valid empty-parameter tape");
    assert_node_field_count(&empty, NodeTag::TS_GENERIC_CLASS_DECLARATION, 5);
    let fields = first_node_fields(&empty, NodeTag::TS_TYPE_PARAMETER_DECLARATION);
    assert!(matches!(
        empty.tape.value_at(fields[0]),
        Ok(TapeValue::List { items, .. }) if items.is_empty()
    ));
}

#[test]
fn recovers_typescript_class_heritage_ambiguities_by_semantic_mode() {
    let source = "class Empty implements {} class Generic implements Box<> {} class Repeat implements A implements B extends Base {}";
    let syntax_only = parse(source, typescript_options()).expect("syntax-only heritage parse");
    assert!(
        syntax_only.diagnostics.is_empty(),
        "{:#?}",
        syntax_only.diagnostics
    );
    syntax_only.tape.validate().expect("valid syntax-only tape");
    assert_eq!(
        node_fields(&syntax_only, NodeTag::TS_CLASS_DECLARATION).count(),
        3
    );

    let semantic = parse(
        source,
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("semantic heritage parse");
    assert!(
        semantic
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "implements list cannot be empty")
    );
    assert!(
        semantic
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "type argument list cannot be empty")
    );
    semantic.tape.validate().expect("valid semantic tape");

    let object_heritage = parse("class C2 extends { foo: string; } {}", typescript_options())
        .expect("recover nonempty object heritage");
    assert!(!object_heritage.diagnostics.is_empty());
    object_heritage
        .tape
        .validate()
        .expect("valid object-heritage recovery tape");
    assert_child_tag(
        &object_heritage,
        NodeTag::CLASS_DECLARATION,
        1,
        NodeTag::OBJECT_EXPRESSION,
    );

    let named = parse(
        "class implements {}",
        ParseOptions {
            source_kind: SourceKind::Script,
            ..typescript_options()
        },
    )
    .expect("parse class named implements");
    assert!(named.diagnostics.is_empty(), "{:#?}", named.diagnostics);
    assert_node_field_count(&named, NodeTag::CLASS_DECLARATION, 3);
    assert_eq!(
        node_fields(&named, NodeTag::TS_CLASS_DECLARATION).count(),
        0
    );

    let anonymous = parse("(class implements Interface {});", typescript_options())
        .expect("parse anonymous class implements clause");
    assert!(
        anonymous.diagnostics.is_empty(),
        "{:#?}",
        anonymous.diagnostics
    );
    let fields = first_node_fields(&anonymous, NodeTag::TS_CLASS_EXPRESSION);
    assert!(matches!(
        anonymous.tape.value_at(fields[0]),
        Ok(TapeValue::Null { .. })
    ));

    let escaped = parse(
        r"class C impl\u0065ments Interface {}",
        typescript_options(),
    )
    .expect("reject escaped implements clause");
    assert!(!escaped.diagnostics.is_empty());
    escaped
        .tape
        .validate()
        .expect("valid escaped recovery tape");
    assert_eq!(
        node_fields(&escaped, NodeTag::TS_CLASS_IMPLEMENTS).count(),
        0
    );

    let malformed = parse(
        "class Malformed implements , Namespace.Interface<> {}",
        typescript_options(),
    )
    .expect("recover malformed heritage");
    assert!(!malformed.diagnostics.is_empty());
    malformed
        .tape
        .validate()
        .expect("valid malformed recovery tape");
    assert_node_field_count(&malformed, NodeTag::TS_CLASS_DECLARATION, 4);
}

#[test]
fn gates_class_implements_to_typescript_capable_dialects() {
    for language in [
        Language::TypeScript,
        Language::TypeScriptJsx,
        Language::TypeScriptDefinition,
    ] {
        let parsed = parse(
            "class C implements Interface {}",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("parse TypeScript-capable class");
        assert!(
            parsed.diagnostics.is_empty(),
            "{language:?}: {:#?}",
            parsed.diagnostics
        );
        assert_node_field_count(&parsed, NodeTag::TS_CLASS_DECLARATION, 4);
    }

    let compatibility = parse(
        "class C implements Interface {}",
        ParseOptions {
            syntax_extensions: SyntaxExtensions {
                typescript_js_compatibility: true,
                ..SyntaxExtensions::default()
            },
            ..ParseOptions::default()
        },
    )
    .expect("parse TypeScript JavaScript compatibility class");
    assert!(
        compatibility.diagnostics.is_empty(),
        "{:#?}",
        compatibility.diagnostics
    );
    assert_node_field_count(&compatibility, NodeTag::TS_CLASS_DECLARATION, 4);

    for language in [Language::JavaScript, Language::JavaScriptJsx] {
        let parsed = parse(
            "class C implements Interface {}",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("recover standard JavaScript class");
        assert!(!parsed.diagnostics.is_empty(), "{language:?}");
        parsed
            .tape
            .validate()
            .expect("valid JavaScript recovery tape");
        assert_eq!(
            node_fields(&parsed, NodeTag::TS_CLASS_IMPLEMENTS).count(),
            0
        );
        assert_eq!(
            node_fields(&parsed, NodeTag::TS_CLASS_DECLARATION).count(),
            0
        );
    }
}

#[test]
fn emits_babel_8_typescript_schema_wrappers() {
    let source = "type Box<T> = Promise<T>; type Text = string; type Flags<S> = { readonly [K in keyof S]?: S[K] }; interface Repository<T> extends Base<T> {} enum Choice { First } namespace Library.Core {}";
    let parsed = parse(source, typescript_options()).expect("parse");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    parsed.tape.validate().expect("valid tape");

    assert_child_tag(
        &parsed,
        NodeTag::TS_TYPE_ALIAS_DECLARATION,
        1,
        NodeTag::TS_TYPE_PARAMETER_DECLARATION,
    );
    assert_child_tag(&parsed, NodeTag::TS_TYPE_PARAMETER, 0, NodeTag::IDENTIFIER);
    assert_child_tag(
        &parsed,
        NodeTag::TS_TYPE_ALIAS_DECLARATION,
        2,
        NodeTag::TS_TYPE_REFERENCE,
    );
    assert_child_tag(
        &parsed,
        NodeTag::TS_TYPE_REFERENCE,
        1,
        NodeTag::TS_TYPE_PARAMETER_INSTANTIATION,
    );
    assert_list_child_tag(
        &parsed,
        NodeTag::TS_INTERFACE_DECLARATION,
        2,
        NodeTag::TS_INTERFACE_HERITAGE,
    );
    assert_child_tag(
        &parsed,
        NodeTag::TS_ENUM_DECLARATION,
        1,
        NodeTag::TS_ENUM_BODY,
    );
    assert_child_tag(
        &parsed,
        NodeTag::TS_MODULE_DECLARATION,
        0,
        NodeTag::TS_QUALIFIED_NAME,
    );
    assert_child_tag(
        &parsed,
        NodeTag::TS_MODULE_DECLARATION,
        1,
        NodeTag::TS_MODULE_BLOCK,
    );
    assert_node_field_count(&parsed, NodeTag::TS_MODULE_DECLARATION, 4);
    assert_node_field_count(&parsed, NodeTag::TS_MAPPED_TYPE, 6);
    assert_node_field_count(&parsed, NodeTag::TS_STRING_KEYWORD, 0);
}

#[test]
fn typescript_private_early_errors_follow_the_semantic_error_option() {
    let source = "class A { #constructor() {} #value = 1; method() { delete this.#value; } }";
    let syntax_only = parse(source, typescript_options()).expect("syntax-only parse");
    assert!(
        syntax_only.diagnostics.is_empty(),
        "{:#?}",
        syntax_only.diagnostics
    );
    syntax_only.tape.validate().expect("valid syntax-only tape");

    let semantic = parse(
        source,
        ParseOptions {
            semantic_errors: true,
            ..typescript_options()
        },
    )
    .expect("semantic parse");
    assert_eq!(semantic.diagnostics.len(), 2, "{:#?}", semantic.diagnostics);
    semantic.tape.validate().expect("valid semantic tape");
}

fn assert_clean_with_tags(name: &str, source: &str, expected_tags: &[NodeTag]) {
    let parsed = parse(source, typescript_options()).expect("parse");
    assert!(
        parsed.diagnostics.is_empty(),
        "{name}: {:?}",
        parsed.diagnostics
    );
    parsed.tape.validate().expect("valid tape");

    let tags: Vec<_> = parsed
        .tape
        .validation()
        .map(|record| record.expect("valid record").value)
        .filter_map(|value| match value {
            TapeValue::Node { tag, .. } => Some(tag),
            _ => None,
        })
        .collect();
    for &tag in expected_tags {
        assert!(tags.contains(&tag), "{name}: missing {tag:?}");
    }
}

fn typescript_options() -> ParseOptions {
    ParseOptions {
        language: Language::TypeScript,
        ..ParseOptions::default()
    }
}

fn assert_child_tag(parsed: &ParseResult, parent: NodeTag, field: usize, expected: NodeTag) {
    for fields in node_fields(parsed, parent) {
        let child = parsed.tape.value_at(fields[field]).expect("child node");
        if matches!(child, TapeValue::Node { tag, .. } if tag == expected) {
            return;
        }
    }
    panic!("no {parent:?} field {field} contained {expected:?}");
}

fn assert_list_child_tag(parsed: &ParseResult, parent: NodeTag, field: usize, expected: NodeTag) {
    for fields in node_fields(parsed, parent) {
        let list = parsed.tape.value_at(fields[field]).expect("child list");
        let TapeValue::List { items, .. } = list else {
            continue;
        };
        if items.iter().any(|&item| {
            matches!(
                parsed.tape.value_at(item),
                Ok(TapeValue::Node { tag, .. }) if tag == expected
            )
        }) {
            return;
        }
    }
    panic!("no {parent:?} list field {field} contained {expected:?}");
}

fn assert_node_field_count(parsed: &ParseResult, tag: NodeTag, expected: usize) {
    let fields = first_node_fields(parsed, tag);
    assert_eq!(fields.len(), expected, "{tag:?}");
}

fn first_node_fields(parsed: &ParseResult, expected: NodeTag) -> Vec<u32> {
    node_fields(parsed, expected)
        .next()
        .unwrap_or_else(|| panic!("missing {expected:?}"))
}

fn node_fields(parsed: &ParseResult, expected: NodeTag) -> impl Iterator<Item = Vec<u32>> + '_ {
    parsed
        .tape
        .validation()
        .map(|record| record.expect("valid record").value)
        .filter_map(move |value| match value {
            TapeValue::Node { tag, fields, .. } if tag == expected => Some(fields.to_vec()),
            _ => None,
        })
}
