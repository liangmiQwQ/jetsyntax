use jetsyntax::{
    Language, ParseOptions, ParseResult, parse,
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
        ("overload", "function convert(): string;"),
        ("declare", "declare function convert(): string;"),
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
