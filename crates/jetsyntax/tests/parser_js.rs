use jetsyntax::{
    Language, ParseOptions, ParseResult, SourceKind, SyntaxExtensions, parse,
    tape::{FrozenTape, NodeTag, TapeValue},
};

#[derive(Clone, Copy)]
struct GrammarCase {
    name: &'static str,
    source: &'static str,
    language: Language,
    source_kind: SourceKind,
    expected_tags: &'static [NodeTag],
}

impl GrammarCase {
    const fn script(
        name: &'static str,
        source: &'static str,
        expected_tags: &'static [NodeTag],
    ) -> Self {
        Self {
            name,
            source,
            language: Language::JavaScript,
            source_kind: SourceKind::Script,
            expected_tags,
        }
    }

    const fn module(
        name: &'static str,
        source: &'static str,
        expected_tags: &'static [NodeTag],
    ) -> Self {
        Self {
            name,
            source,
            language: Language::JavaScript,
            source_kind: SourceKind::Module,
            expected_tags,
        }
    }

    fn options(self, semantic_errors: bool) -> ParseOptions {
        ParseOptions {
            language: self.language,
            source_kind: self.source_kind,
            semantic_errors,
            ..ParseOptions::default()
        }
    }
}

#[derive(Clone, Copy)]
struct ExpressionShape {
    name: &'static str,
    source: &'static str,
    root: NodeTag,
    nested_field: usize,
    nested: NodeTag,
}

/// Operator binding must preserve both precedence and left/right associativity in the emitted tree.
#[test]
fn parser_should_preserve_operator_precedence_and_associativity() {
    let cases = [
        ExpressionShape {
            name: "multiplication before addition",
            source: "a + b * c;",
            root: NodeTag::BINARY_EXPRESSION,
            nested_field: 2,
            nested: NodeTag::BINARY_EXPRESSION,
        },
        ExpressionShape {
            name: "subtraction is left associative",
            source: "a - b - c;",
            root: NodeTag::BINARY_EXPRESSION,
            nested_field: 1,
            nested: NodeTag::BINARY_EXPRESSION,
        },
        ExpressionShape {
            name: "exponentiation is right associative",
            source: "a ** b ** c;",
            root: NodeTag::BINARY_EXPRESSION,
            nested_field: 2,
            nested: NodeTag::BINARY_EXPRESSION,
        },
        ExpressionShape {
            name: "assignment is right associative",
            source: "a = b = c;",
            root: NodeTag::ASSIGNMENT_EXPRESSION,
            nested_field: 2,
            nested: NodeTag::ASSIGNMENT_EXPRESSION,
        },
        ExpressionShape {
            name: "logical and before logical or",
            source: "a || b && c;",
            root: NodeTag::LOGICAL_EXPRESSION,
            nested_field: 2,
            nested: NodeTag::LOGICAL_EXPRESSION,
        },
    ];
    let mut failures = Vec::new();

    for case in cases {
        if let Err(reason) = check_expression_shape(case) {
            failures.push(format!("{}: {reason}", case.name));
        }
    }

    assert_failures_empty(&failures);
}

/// Assignment and update operators share target classification while preserving Annex B call targets.
#[test]
fn parser_should_apply_standard_assignment_target_policies() {
    let clean = [
        "target = value; target.member += value; ++target; target--; for (target in source) {} for (target.member of source) {}",
        "factory() = value; factory() += value; ++factory(); factory()--; for (factory() in source) {} for (factory() of source) {}",
        "await = 1; yield = 2;",
        "({ value } = source); [first] = source;",
        "'use strict'; eval; arguments; await = 1;",
        "(target) = value;",
        "(factory()) = value;",
        "(target?.member).property = value;",
        "new Constructor().member = value;",
    ];
    for source in clean {
        let parsed = parse(
            source,
            ParseOptions {
                semantic_errors: true,
                source_kind: SourceKind::Script,
                ..ParseOptions::default()
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
        "value + offset = source;",
        "factory() ||= source;",
        "factory() &&= source;",
        "factory() ??= source;",
        "target?.member = source;",
        "target?.member++;",
        "++target?.member;",
        "({ value }) = source;",
        "([first]) = source;",
        "(async)(parameter) => value;",
        "'use strict'; eval = source;",
        "'use strict'; \\u0065val = source;",
        "'use strict'; arguments++;",
        "'use strict'; factory() += source;",
        "'use strict'; for (factory() in source) {}",
    ];
    for source in invalid {
        let parsed = parse(
            source,
            ParseOptions {
                semantic_errors: true,
                source_kind: SourceKind::Script,
                ..ParseOptions::default()
            },
        )
        .expect(source);
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect(source);
    }
}

#[test]
fn parser_should_apply_contextual_keyword_assignment_policies() {
    let module = parse(
        "function nested() { await = source; }",
        ParseOptions {
            semantic_errors: true,
            source_kind: SourceKind::Module,
            ..ParseOptions::default()
        },
    )
    .expect("module await restriction");
    assert!(!module.diagnostics.is_empty());
    module.tape.validate().expect("valid module recovery tape");

    let typescript_js = parse(
        "function load() { await new Promise(undefined); }",
        ParseOptions {
            source_kind: SourceKind::Script,
            semantic_errors: true,
            syntax_extensions: SyntaxExtensions {
                typescript_js_compatibility: true,
                ..SyntaxExtensions::default()
            },
            ..ParseOptions::default()
        },
    )
    .expect("TypeScript JavaScript compatibility");
    assert!(
        typescript_js.diagnostics.is_empty(),
        "{:?}",
        typescript_js.diagnostics
    );
    let standard_js = parse(
        "function load() { await new Promise(undefined); }",
        ParseOptions {
            source_kind: SourceKind::Script,
            semantic_errors: true,
            ..ParseOptions::default()
        },
    )
    .expect("standard JavaScript await grammar");
    assert!(!standard_js.diagnostics.is_empty());
}

#[test]
fn parser_should_apply_optional_chaining_assignment_policy() {
    let optional_assignment = parse(
        "target?.member = value; (target?.member) += value; target?.member ||= value; target?.member &&= value; target?.member ??= value;",
        ParseOptions {
            source_kind: SourceKind::Script,
            semantic_errors: true,
            syntax_extensions: SyntaxExtensions {
                optional_chaining_assign: true,
                ..SyntaxExtensions::default()
            },
            ..ParseOptions::default()
        },
    )
    .expect("optional chaining assignment compatibility");
    assert!(
        optional_assignment.diagnostics.is_empty(),
        "{:?}",
        optional_assignment.diagnostics
    );

    for source in [
        "target?.member++;",
        "for (target?.member in source) {}",
        "target?.() = value;",
        "({ value: target?.member } = source);",
        "[target?.member] = source;",
        "(target?.member) => value;",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                source_kind: SourceKind::Script,
                semantic_errors: true,
                syntax_extensions: SyntaxExtensions {
                    optional_chaining_assign: true,
                    ..SyntaxExtensions::default()
                },
                ..ParseOptions::default()
            },
        )
        .expect(source);
        assert!(!parsed.diagnostics.is_empty(), "{source}");
    }
}

/// Every ECMAScript statement family must produce its corresponding tape node without diagnostics.
#[test]
fn parser_should_accept_every_statement_family() {
    let cases = [
        GrammarCase::script("empty", ";", &[NodeTag::EMPTY_STATEMENT]),
        GrammarCase::script("block", "{ value; }", &[NodeTag::BLOCK_STATEMENT]),
        GrammarCase::script("expression", "value;", &[NodeTag::EXPRESSION_STATEMENT]),
        GrammarCase::script("debugger", "debugger;", &[NodeTag::DEBUGGER_STATEMENT]),
        GrammarCase::script(
            "declarations",
            "var a; let b = 1; const c = 2;",
            &[NodeTag::VARIABLE_DECLARATION, NodeTag::VARIABLE_DECLARATOR],
        ),
        GrammarCase::script(
            "if and else",
            "if (test) yes; else no;",
            &[NodeTag::IF_STATEMENT],
        ),
        GrammarCase::script(
            "switch",
            "switch (value) { case 1: hit; break; default: miss; }",
            &[
                NodeTag::SWITCH_STATEMENT,
                NodeTag::SWITCH_CASE,
                NodeTag::BREAK_STATEMENT,
            ],
        ),
        GrammarCase::script("throw", "throw value;", &[NodeTag::THROW_STATEMENT]),
        GrammarCase::script(
            "try catch finally",
            "try { work(); } catch (error) { recover(error); } finally { cleanup(); }",
            &[NodeTag::TRY_STATEMENT, NodeTag::CATCH_CLAUSE],
        ),
        GrammarCase::script(
            "loops",
            "while (ready) step(); do step(); while (ready); for (;;); for (key in object); for (value of list);",
            &[
                NodeTag::WHILE_STATEMENT,
                NodeTag::DO_WHILE_STATEMENT,
                NodeTag::FOR_STATEMENT,
                NodeTag::FOR_IN_STATEMENT,
                NodeTag::FOR_OF_STATEMENT,
            ],
        ),
        GrammarCase::script(
            "labels and jumps",
            "outer: while (ready) { if (skip) continue outer; break outer; }",
            &[
                NodeTag::LABELED_STATEMENT,
                NodeTag::WHILE_STATEMENT,
                NodeTag::CONTINUE_STATEMENT,
                NodeTag::BREAK_STATEMENT,
            ],
        ),
        GrammarCase::script("with", "with (object) value;", &[NodeTag::WITH_STATEMENT]),
        GrammarCase::script(
            "function and return",
            "function identity(value) { return value; }",
            &[NodeTag::FUNCTION_DECLARATION, NodeTag::RETURN_STATEMENT],
        ),
        GrammarCase::script(
            "class declaration",
            "class Point { constructor(x) { this.x = x; } }",
            &[NodeTag::CLASS_DECLARATION, NodeTag::METHOD_DEFINITION],
        ),
    ];

    assert_clean_cases(&cases);
}

#[test]
fn parser_should_restrict_declarations_to_statement_list_items() {
    for source in [
        "if (condition) let value;",
        "if (condition) const value = 1;",
        "if (condition) class Value {}",
        "if (condition) function* generated() {}",
        "if (condition) async function asynchronous() {}",
        "if (condition); else class Alternate {}",
        "'use strict'; if (condition) function strict() {}",
        "while (condition) function nested() {}",
        "do class Nested {} while (condition);",
        "for (;;) const value = 1;",
        "for (key in object) let value;",
        "for (value of values) class Nested {}",
        "async function run() { for await (value of values) let nested; }",
        "with (object) let value;",
        "with (object) function nested() {}",
        "label: const value = 1;",
        "label: function* generated() {}",
        "label: async function asynchronous() {}",
        "if (condition) label: function nested() {}",
        "while (condition) first: second: function nested() {}",
        "if (condition) let\n[value] = values;",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                source_kind: SourceKind::Script,
                semantic_errors: true,
                ..ParseOptions::default()
            },
        )
        .expect(source);
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect(source);
    }

    for source in [
        "label: import value from 'package';",
        "if (condition) export { value };",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                source_kind: SourceKind::Module,
                semantic_errors: true,
                ..ParseOptions::default()
            },
        )
        .expect(source);
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect(source);
    }

    for source in [
        "for (;;) interface Contract {}",
        "for (;;) type Value = number;",
        "if (condition) declare class Ambient {}",
        "while (condition) enum Choice {}",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                language: Language::TypeScript,
                source_kind: SourceKind::Script,
                semantic_errors: true,
                ..ParseOptions::default()
            },
        )
        .expect(source);
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect(source);
    }
}

#[test]
fn parser_should_preserve_valid_statement_and_annex_b_positions() {
    for source in [
        "if (condition) function annex_b() {}",
        "label: function annex_b() {}",
        "first: second: function annex_b() {}",
        "label: if (condition) function annex_b() {}",
        "while (condition) var value;",
        "if (condition) { let value; class Nested {} }",
        "for (;;) { const value = 1; function nested() {} }",
        "if (condition) letValue;",
        "if (condition) let\nvalue = 1;",
        "if (condition) let\n{}",
        "if (condition) let\n\\u0076alue = 1;",
        "if (condition) async\nfunction separated() {}",
        "label: import('package');",
        "'use strict'; function top_level() {}",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                source_kind: SourceKind::Script,
                semantic_errors: true,
                ..ParseOptions::default()
            },
        )
        .expect(source);
        assert!(
            parsed.diagnostics.is_empty(),
            "{source}: {:#?}",
            parsed.diagnostics
        );
        parsed.tape.validate().expect(source);
    }

    for source in [
        "while (condition) function nested() {}",
        "if (condition) const value = 1;",
        "if (condition) label: function nested() {}",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                source_kind: SourceKind::Script,
                semantic_errors: false,
                ..ParseOptions::default()
            },
        )
        .expect(source);
        assert!(parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect(source);
    }
}

/// Lexical loop-head bindings are isolated from surrounding and sibling loop scopes.
#[test]
fn parser_should_isolate_lexical_for_head_bindings() {
    let cases = [
        GrammarCase::script(
            "classic, sequential, outer, and nested",
            "let index = 0; for (let index = 0; index < 1; index++) { for (let index = 0; index < 1; index++) {} } for (let index = 0; index < 1; index++) {}",
            &[NodeTag::FOR_STATEMENT],
        ),
        GrammarCase::script(
            "for-in destructuring",
            "const key = 'outer'; for (const [key] in first) {} for (const [key] in second) {}",
            &[NodeTag::FOR_IN_STATEMENT, NodeTag::ARRAY_PATTERN],
        ),
        GrammarCase::script(
            "for-of destructuring",
            "const value = 'outer'; for (const { value } of first) {} for (const { value } of second) {}",
            &[NodeTag::FOR_OF_STATEMENT, NodeTag::OBJECT_PATTERN],
        ),
        GrammarCase::script(
            "for-await-of",
            "async function consume() { for await (const value of first) {} for await (const value of second) {} }",
            &[NodeTag::FUNCTION_DECLARATION, NodeTag::FOR_OF_STATEMENT],
        ),
        GrammarCase::script(
            "block function shadowing",
            "for (let value = 0; false;) { function value() {} }",
            &[NodeTag::FOR_STATEMENT, NodeTag::FUNCTION_DECLARATION],
        ),
        GrammarCase::module(
            "strict block function shadowing",
            "for (let value = 0; false;) { function value() {} }",
            &[NodeTag::FOR_STATEMENT, NodeTag::FUNCTION_DECLARATION],
        ),
        GrammarCase::script(
            "sloppy block function redeclaration",
            "{ function value() {} function value() {} }",
            &[NodeTag::BLOCK_STATEMENT, NodeTag::FUNCTION_DECLARATION],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Loop-head, same-block, and catch conflicts remain diagnostics.
#[test]
fn parser_should_diagnose_lexical_for_head_conflicts() {
    let cases = [
        GrammarCase::script(
            "duplicate head",
            "for (let value = 0, value = 1; false;) {}",
            &[NodeTag::FOR_STATEMENT],
        ),
        GrammarCase::script(
            "body var conflict",
            "for (let value = 0; false;) { var value; }",
            &[NodeTag::FOR_STATEMENT],
        ),
        GrammarCase::script(
            "same-block function conflict",
            "{ let value; function value() {} }",
            &[NodeTag::BLOCK_STATEMENT, NodeTag::FUNCTION_DECLARATION],
        ),
        GrammarCase::module(
            "strict block function redeclaration",
            "{ function value() {} function value() {} }",
            &[NodeTag::BLOCK_STATEMENT, NodeTag::FUNCTION_DECLARATION],
        ),
        GrammarCase::script(
            "catch parameter and block function conflict",
            "try {} catch (value) { function value() {} }",
            &[NodeTag::TRY_STATEMENT, NodeTag::FUNCTION_DECLARATION],
        ),
    ];

    assert_diagnostic_cases(&cases, true);
}

/// Functions, classes, and modules cover declarations and expressions that introduce grammar context.
#[test]
fn parser_should_accept_functions_classes_and_modules() {
    let cases = [
        GrammarCase::script(
            "function expression",
            "const identity = function (value) { return value; };",
            &[NodeTag::FUNCTION_EXPRESSION, NodeTag::RETURN_STATEMENT],
        ),
        GrammarCase::script(
            "arrow function",
            "const add = (left, right) => left + right;",
            &[NodeTag::ARROW_FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "derived class members",
            "class Child extends Parent { #value = 1; static count = 0; method() { return super.method(); } }",
            &[
                NodeTag::CLASS_DECLARATION,
                NodeTag::CLASS_BODY,
                NodeTag::PROPERTY_DEFINITION,
                NodeTag::METHOD_DEFINITION,
                NodeTag::PRIVATE_IDENTIFIER,
            ],
        ),
        GrammarCase::script(
            "class expression",
            "const Child = class extends Parent { static { initialize(); } };",
            &[NodeTag::CLASS_EXPRESSION, NodeTag::STATIC_BLOCK],
        ),
        GrammarCase::module(
            "imports",
            "import defaultValue, * as namespace from 'package'; import { value as renamed } from 'other';",
            &[
                NodeTag::IMPORT_DECLARATION,
                NodeTag::IMPORT_DEFAULT_SPECIFIER,
                NodeTag::IMPORT_NAMESPACE_SPECIFIER,
                NodeTag::IMPORT_SPECIFIER,
            ],
        ),
        GrammarCase::module(
            "exports",
            "export const value = 1; export { value as renamed }; export * from 'other'; export default value;",
            &[
                NodeTag::EXPORT_NAMED_DECLARATION,
                NodeTag::EXPORT_SPECIFIER,
                NodeTag::EXPORT_ALL_DECLARATION,
                NodeTag::EXPORT_DEFAULT_DECLARATION,
            ],
        ),
        GrammarCase::module(
            "dynamic import and metadata",
            "const module = import('package'); const url = import.meta.url;",
            &[NodeTag::IMPORT_EXPRESSION, NodeTag::META_PROPERTY],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Default exports permit anonymous declaration forms without weakening ordinary declarations.
#[test]
fn parser_should_allow_anonymous_default_export_declarations() {
    assert_clean_cases(&[
        GrammarCase::module(
            "anonymous default function",
            "export default function() {}",
            &[
                NodeTag::EXPORT_DEFAULT_DECLARATION,
                NodeTag::FUNCTION_DECLARATION,
            ],
        ),
        GrammarCase::module(
            "anonymous default generator",
            "export default function*() { yield value; }",
            &[
                NodeTag::EXPORT_DEFAULT_DECLARATION,
                NodeTag::FUNCTION_DECLARATION,
                NodeTag::YIELD_EXPRESSION,
            ],
        ),
        GrammarCase::module(
            "anonymous default async function",
            "export default async function() { await value; }",
            &[
                NodeTag::EXPORT_DEFAULT_DECLARATION,
                NodeTag::FUNCTION_DECLARATION,
                NodeTag::AWAIT_EXPRESSION,
            ],
        ),
        GrammarCase::module(
            "anonymous default class",
            "export default class {}",
            &[
                NodeTag::EXPORT_DEFAULT_DECLARATION,
                NodeTag::CLASS_DECLARATION,
            ],
        ),
    ]);

    for source in [
        "export default function<T>() {}",
        "export default class<T> {}",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                language: Language::TypeScript,
                source_kind: SourceKind::Module,
                semantic_errors: true,
                ..ParseOptions::default()
            },
        )
        .expect("parse anonymous TypeScript default export");
        assert!(
            parsed.diagnostics.is_empty(),
            "{source}: {:?}",
            parsed.diagnostics
        );
        parsed.tape.validate().expect("valid default export tape");
    }

    assert_diagnostic_cases(
        &[
            GrammarCase::module(
                "anonymous ordinary function declaration",
                "function() {}",
                &[NodeTag::FUNCTION_DECLARATION],
            ),
            GrammarCase::module(
                "anonymous ordinary class declaration",
                "class {}",
                &[NodeTag::CLASS_DECLARATION],
            ),
            GrammarCase::module(
                "anonymous named function export",
                "export function() {}",
                &[
                    NodeTag::EXPORT_NAMED_DECLARATION,
                    NodeTag::FUNCTION_DECLARATION,
                ],
            ),
            GrammarCase::module(
                "anonymous named class export",
                "export class {}",
                &[
                    NodeTag::EXPORT_NAMED_DECLARATION,
                    NodeTag::CLASS_DECLARATION,
                ],
            ),
        ],
        true,
    );
}

/// Module export names may be strings while imported bindings remain identifiers.
#[test]
fn parser_should_parse_arbitrary_module_namespace_names() {
    assert_clean_cases(&[
        GrammarCase::module(
            "string-named import",
            r#"import { "default" as value, if as condition } from "source";"#,
            &[NodeTag::IMPORT_DECLARATION, NodeTag::IMPORT_SPECIFIER],
        ),
        GrammarCase::module(
            "string-named local export",
            r#"const value = 1; export { value as "answer" };"#,
            &[NodeTag::EXPORT_NAMED_DECLARATION, NodeTag::EXPORT_SPECIFIER],
        ),
        GrammarCase::module(
            "string-named re-exports",
            r#"export { "source", "source" as "answer", default as "fallback" } from "module";"#,
            &[NodeTag::EXPORT_NAMED_DECLARATION, NodeTag::EXPORT_SPECIFIER],
        ),
        GrammarCase::module(
            "string-named namespace export",
            r#"export * as "namespace" from "module";"#,
            &[NodeTag::EXPORT_ALL_DECLARATION],
        ),
    ]);

    for source in [
        r#"const value = 1; export { value as "\udbff\udfff" };"#,
        r#"const value = 1; export { value as "\u{dbff}\udfff" };"#,
        r#"const value = 1; export { value as "\udbff\u{dfff}" };"#,
        r#"const value = 1; export { value as "\u{dbff}\u{dfff}" };"#,
        "const value = 1; export { value as \"\\udbff\\\n\\udfff\" };",
    ] {
        let valid_pair = parse(
            source,
            ParseOptions {
                source_kind: SourceKind::Module,
                semantic_errors: true,
                ..ParseOptions::default()
            },
        )
        .expect("parse paired surrogate export name");
        assert!(
            valid_pair.diagnostics.is_empty(),
            "{source}: {:?}",
            valid_pair.diagnostics
        );
        valid_pair
            .tape
            .validate()
            .expect("valid paired surrogate tape");
    }
}

#[test]
fn parser_should_parse_typescript_module_namespace_names() {
    for source in [
        r#"import type { "source" as Local } from "module"; export type { Local as "public" };"#,
        r#"import { type "source" as Local } from "module"; export { type Local as "public" };"#,
        r#"export type { "source" as "public" } from "module"; export type * as "namespace" from "module";"#,
        r#"import { type } from "module";"#,
        r#"import { type as } from "module";"#,
        r#"import { type as as } from "module";"#,
        r#"import { type as as as } from "module";"#,
        r#"export { type as as if } from "module";"#,
        r#"import type from "value-module"; import type from from "type-module";"#,
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                language: Language::TypeScript,
                source_kind: SourceKind::Module,
                semantic_errors: true,
                ..ParseOptions::default()
            },
        )
        .expect("parse TypeScript module export name");
        assert!(
            parsed.diagnostics.is_empty(),
            "{source}: {:?}",
            parsed.diagnostics
        );
        parsed
            .tape
            .validate()
            .expect("valid TypeScript module-name tape");
    }
}

#[test]
fn parser_should_resolve_cooked_module_bindings_and_deferred_imports() {
    assert_clean_cases(&[
        GrammarCase::module(
            "escaped local export binding",
            r"const \u0061 = 1; export { a };",
            &[NodeTag::EXPORT_NAMED_DECLARATION],
        ),
        GrammarCase::module(
            "deferred namespace import",
            r#"import defer * as namespace from "module";"#,
            &[NodeTag::PHASE_IMPORT_DECLARATION],
        ),
    ]);
}

#[test]
fn parser_should_diagnose_invalid_typescript_type_only_module_declarations() {
    for source in [
        r#"import type Default, { Named } from "module";"#,
        r#"import type { type Named } from "module";"#,
        r#"export type { type Named } from "module";"#,
        r#"import { await } from "module";"#,
        r#"import { enum } from "module";"#,
        r#"import Default { Named } from "module";"#,
        r#"import defer { Named } from "module";"#,
        r#"import defer Default from "module";"#,
        r#"import defer Default, * as Namespace from "module";"#,
        r#"import defer from from "module";"#,
        r#"import { Named } from "one"; import { Named } from "two";"#,
        r#"import type { Named } from "one"; import type { Named } from "two";"#,
        r#"import { type Named } from "one"; import { type Named } from "two";"#,
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                language: Language::TypeScript,
                source_kind: SourceKind::Module,
                semantic_errors: true,
                ..ParseOptions::default()
            },
        )
        .expect("recover invalid TypeScript type-only declaration");
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed
            .tape
            .validate()
            .expect("valid recovered TypeScript module-name tape");
    }
}

#[test]
fn parser_should_diagnose_invalid_module_namespace_names() {
    assert_diagnostic_cases(
        &[
            GrammarCase::module(
                "string import without binding",
                r#"import { "value" } from "module";"#,
                &[NodeTag::IMPORT_SPECIFIER],
            ),
            GrammarCase::module(
                "string export binding without source",
                r#"export { "value" as renamed };"#,
                &[NodeTag::EXPORT_SPECIFIER],
            ),
            GrammarCase::module(
                "lone surrogate import name",
                r#"import { "\ud800" as value } from "module";"#,
                &[NodeTag::IMPORT_SPECIFIER],
            ),
            GrammarCase::module(
                "lone surrogate export name",
                r#"export { value as "\udfff" };"#,
                &[NodeTag::EXPORT_SPECIFIER],
            ),
            GrammarCase::module(
                "escaped duplicate export name",
                r#"const first = 1, second = 2; export { first as "name", second as "n\u0061me" };"#,
                &[NodeTag::EXPORT_SPECIFIER],
            ),
            GrammarCase::module(
                "escaped identifier duplicate export name",
                r#"const first = 1, second = 2; export { first as n\u0061me, second as "name" };"#,
                &[NodeTag::EXPORT_SPECIFIER],
            ),
            GrammarCase::module(
                "duplicate declaration and string export name",
                r#"export const name = 1; export { name as "name" };"#,
                &[NodeTag::EXPORT_NAMED_DECLARATION, NodeTag::EXPORT_SPECIFIER],
            ),
            GrammarCase::module(
                "duplicate merged variable and explicit export name",
                r"var name; export var name; export { name };",
                &[NodeTag::EXPORT_NAMED_DECLARATION, NodeTag::EXPORT_SPECIFIER],
            ),
            GrammarCase::module(
                "duplicate imported binding",
                r#"import { value, value } from "module";"#,
                &[NodeTag::IMPORT_SPECIFIER],
            ),
        ],
        true,
    );
}

/// Empty arrow parameter lists accept either expression or block bodies in nested expression positions.
#[test]
fn parser_should_accept_zero_parameter_arrow_functions() {
    let cases = [
        GrammarCase::script(
            "expression and block bodies",
            "const expression = () => value; const block = () => { return value; };",
            &[
                NodeTag::ARROW_FUNCTION_EXPRESSION,
                NodeTag::BLOCK_STATEMENT,
                NodeTag::RETURN_STATEMENT,
            ],
        ),
        GrammarCase::script(
            "comment trivia",
            "const callback = (/* parameters */) /* arrow */ => value;",
            &[NodeTag::ARROW_FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "nested and parenthesized",
            "promise.then(() => value); const invoked = (() => value)();",
            &[
                NodeTag::ARROW_FUNCTION_EXPRESSION,
                NodeTag::CALL_EXPRESSION,
                NodeTag::PARENTHESIZED_EXPRESSION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Rest parameters use binding grammar while ordinary parenthesized arrows retain cover grammar.
#[test]
fn parser_should_accept_parenthesized_rest_arrow_parameters() {
    let cases = [
        GrammarCase::script(
            "direct rest parameter",
            "const variadic = (...args) => args;",
            &[NodeTag::ARROW_FUNCTION_EXPRESSION, NodeTag::REST_ELEMENT],
        ),
        GrammarCase::script(
            "preceding parameters",
            "const variadic = (first, second, third, ...rest) => rest;",
            &[NodeTag::ARROW_FUNCTION_EXPRESSION, NodeTag::REST_ELEMENT],
        ),
        GrammarCase::script(
            "destructured rest parameters",
            "const array = (...[first, second]) => first; const object = (...{ length }) => length;",
            &[
                NodeTag::ARROW_FUNCTION_EXPRESSION,
                NodeTag::REST_ELEMENT,
                NodeTag::ARRAY_PATTERN,
                NodeTag::OBJECT_PATTERN,
            ],
        ),
        GrammarCase::script(
            "async rest parameter",
            "const variadic = async (...args) => await invoke(...args);",
            &[
                NodeTag::ARROW_FUNCTION_EXPRESSION,
                NodeTag::REST_ELEMENT,
                NodeTag::AWAIT_EXPRESSION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Default and destructuring expressions must remain on the existing parenthesized cover path.
#[test]
fn parser_should_preserve_non_rest_parenthesized_expression_paths() {
    assert_clean_cases(&[GrammarCase::script(
        "default and destructuring expressions",
        "const assigned = (value = fallback); const destructured = ({ value } = source);",
        &[NodeTag::PARENTHESIZED_EXPRESSION],
    )]);
}

/// Rest parameters cannot carry defaults or a trailing comma.
#[test]
fn parser_should_diagnose_invalid_rest_arrow_parameters() {
    assert_diagnostic_cases(
        &[
            GrammarCase::script(
                "rest default",
                "const invalid = (...args = []) => args;",
                &[NodeTag::ARROW_FUNCTION_EXPRESSION, NodeTag::REST_ELEMENT],
            ),
            GrammarCase::script(
                "rest trailing comma",
                "const invalid = (...args,) => args;",
                &[NodeTag::ARROW_FUNCTION_EXPRESSION, NodeTag::REST_ELEMENT],
            ),
            GrammarCase::script(
                "rest followed by parameter",
                "const invalid = (...args, value) => args;",
                &[NodeTag::ARROW_FUNCTION_EXPRESSION, NodeTag::REST_ELEMENT],
            ),
            GrammarCase::script(
                "multiple rest parameters",
                "const invalid = (...first, ...second) => first;",
                &[NodeTag::ARROW_FUNCTION_EXPRESSION, NodeTag::REST_ELEMENT],
            ),
            GrammarCase::script(
                "await rest binding nested in async parameters",
                "async(value = (...await) => {}) => {};",
                &[NodeTag::ARROW_FUNCTION_EXPRESSION, NodeTag::REST_ELEMENT],
            ),
        ],
        true,
    );
}

/// Import calls need parentheses before they can be used as `new` callees.
#[test]
fn parser_should_restrict_import_call_new_callees() {
    assert_clean_cases(&[
        GrammarCase::script(
            "covered import call",
            "new (import('package'));",
            &[NodeTag::NEW_EXPRESSION, NodeTag::IMPORT_EXPRESSION],
        ),
        GrammarCase::module(
            "import metadata",
            "new import.meta();",
            &[NodeTag::NEW_EXPRESSION, NodeTag::META_PROPERTY],
        ),
    ]);

    assert_diagnostic_cases(
        &[
            GrammarCase::script(
                "direct import call",
                "new import('package');",
                &[NodeTag::NEW_EXPRESSION, NodeTag::IMPORT_EXPRESSION],
            ),
            GrammarCase::script(
                "direct import call property",
                "new import('package').then;",
                &[NodeTag::NEW_EXPRESSION, NodeTag::IMPORT_EXPRESSION],
            ),
            GrammarCase::script(
                "direct import call trivia",
                "new import/* comment */\n('package');",
                &[NodeTag::NEW_EXPRESSION, NodeTag::IMPORT_EXPRESSION],
            ),
        ],
        false,
    );
}

/// Statement-leading import calls retain the same postfix grammar as nested expressions.
#[test]
fn parser_should_parse_statement_leading_dynamic_import_continuations() {
    assert_clean_cases(&[
        GrammarCase::script(
            "dynamic import postfix continuations",
            "import('bare');\
             import('chain').then(handler).catch(handler);\
             import('call')();\
             import('tag')``;\
             import\n('line-break').then(handler);",
            &[
                NodeTag::IMPORT_EXPRESSION,
                NodeTag::MEMBER_EXPRESSION,
                NodeTag::CALL_EXPRESSION,
                NodeTag::TAGGED_TEMPLATE_EXPRESSION,
            ],
        ),
        GrammarCase::module(
            "static import declaration",
            "import value from 'package';",
            &[NodeTag::IMPORT_DECLARATION],
        ),
    ]);

    assert_diagnostic_cases(
        &[GrammarCase::script(
            "empty dynamic import call",
            "import();",
            &[NodeTag::IMPORT_EXPRESSION],
        )],
        false,
    );
}

/// Import calls accept the optional trailing comma after either grammar argument.
#[test]
fn parser_should_accept_dynamic_import_trailing_commas() {
    assert_clean_cases(&[
        GrammarCase::script(
            "trailing comma after source",
            "import('source',); function nested(value) { return import(value,); }",
            &[NodeTag::IMPORT_EXPRESSION],
        ),
        GrammarCase::module(
            "trailing comma after options",
            "import('data.json', { with: { type: 'json' } },);",
            &[NodeTag::IMPORT_EXPRESSION, NodeTag::OBJECT_EXPRESSION],
        ),
    ]);

    assert_diagnostic_cases(
        &[
            GrammarCase::script(
                "missing source",
                "import(,);",
                &[NodeTag::IMPORT_EXPRESSION],
            ),
            GrammarCase::script(
                "missing options between commas",
                "import('source',,);",
                &[NodeTag::IMPORT_EXPRESSION],
            ),
            GrammarCase::script(
                "extra argument after options",
                "import('source', {}, extra);",
                &[NodeTag::IMPORT_EXPRESSION],
            ),
            GrammarCase::script(
                "repeated trailing comma",
                "import('source', {},,);",
                &[NodeTag::IMPORT_EXPRESSION],
            ),
        ],
        false,
    );
}

/// Import-dot primary expressions keep their distinct grammar and postfix continuations.
#[test]
fn parser_should_parse_import_dot_primary_expressions() {
    assert_clean_cases(&[
        GrammarCase::module(
            "metadata and phased imports",
            "import.meta;\
             function nested() { return import.source('source').then(use); }\
             import.defer('defer', { with: { type: 'json' } })();\
             import/* dot */./* phase */source('tag')``;",
            &[
                NodeTag::META_PROPERTY,
                NodeTag::PHASE_IMPORT_EXPRESSION,
                NodeTag::MEMBER_EXPRESSION,
                NodeTag::CALL_EXPRESSION,
                NodeTag::TAGGED_TEMPLATE_EXPRESSION,
            ],
        ),
        GrammarCase::script(
            "phased imports in scripts",
            "import.source('source'); import.defer('defer');",
            &[NodeTag::PHASE_IMPORT_EXPRESSION],
        ),
        GrammarCase::module(
            "ordinary and static import guards",
            "import('dynamic'); import source from 'static';",
            &[NodeTag::IMPORT_EXPRESSION, NodeTag::IMPORT_DECLARATION],
        ),
    ]);
}

/// Malformed import-dot forms recover without accepting escaped contextual names.
#[test]
fn parser_should_recover_malformed_import_dot_primary_expressions() {
    assert_diagnostic_cases(
        &[
            GrammarCase::script(
                "metadata in scripts",
                "import.meta;",
                &[NodeTag::META_PROPERTY],
            ),
            GrammarCase::module(
                "unknown metadata property",
                "import.unknown;",
                &[NodeTag::META_PROPERTY],
            ),
            GrammarCase::module(
                "escaped metadata property",
                "import.m\\u0065ta;",
                &[NodeTag::META_PROPERTY],
            ),
            GrammarCase::script(
                "escaped phase name",
                "import.sour\\u0063e('source');",
                &[NodeTag::META_PROPERTY, NodeTag::CALL_EXPRESSION],
            ),
            GrammarCase::script(
                "missing phase call",
                "typeof import.source;",
                &[NodeTag::PHASE_IMPORT_EXPRESSION],
            ),
            GrammarCase::script(
                "missing phase argument",
                "import.source();",
                &[NodeTag::PHASE_IMPORT_EXPRESSION],
            ),
            GrammarCase::script(
                "extra phase argument",
                "import.defer('source', {}, extra);",
                &[NodeTag::PHASE_IMPORT_EXPRESSION],
            ),
            GrammarCase::script(
                "spread phase argument",
                "import.source(...arguments);",
                &[NodeTag::PHASE_IMPORT_EXPRESSION, NodeTag::SPREAD_ELEMENT],
            ),
            GrammarCase::module(
                "static phase declarations remain excluded",
                "import source binding from 'source'; import defer * as ns from 'defer';",
                &[NodeTag::IMPORT_DECLARATION],
            ),
        ],
        false,
    );
}

/// Phased imports are never valid assignment, rest-binding, or direct `new` targets.
#[test]
fn parser_should_restrict_import_dot_early_error_positions() {
    assert_diagnostic_cases(
        &[
            GrammarCase::script(
                "phase assignment target",
                "import.source('source') = value;",
                &[NodeTag::PHASE_IMPORT_EXPRESSION],
            ),
            GrammarCase::script(
                "phase rest binding",
                "function f(...import.defer('source')) {}",
                &[NodeTag::REST_ELEMENT],
            ),
            GrammarCase::script(
                "direct phase new callee",
                "new import.source('source');",
                &[NodeTag::NEW_EXPRESSION, NodeTag::PHASE_IMPORT_EXPRESSION],
            ),
            GrammarCase::script(
                "direct phase new callee property",
                "new import.defer('source').then;",
                &[NodeTag::NEW_EXPRESSION, NodeTag::PHASE_IMPORT_EXPRESSION],
            ),
        ],
        true,
    );
}

/// Line terminators before `=>` stay invalid, and truncated bodies retain a recoverable tape.
#[test]
fn parser_should_recover_invalid_zero_parameter_arrow_functions() {
    let cases = [
        GrammarCase::script("direct line break", "const callback = ()\n=> value;", &[]),
        GrammarCase::script(
            "comment line break",
            "const callback = ()/*\n*/=> value;",
            &[],
        ),
        GrammarCase::script(
            "truncated body",
            "const callback = () =>",
            &[NodeTag::ARROW_FUNCTION_EXPRESSION],
        ),
    ];

    assert_diagnostic_cases(&cases, false);
}

/// Parameter and `var` bindings belong to their nearest function and must not leak into siblings.
#[test]
fn parser_should_isolate_function_scopes() {
    let cases = [GrammarCase::script(
        "sibling function scopes",
        "function first(value) { var local; }\
         function second(value) { var local; }\
         class Example { first(value) { var local; } second(value) { var local; } }\
         const firstArrow = (value) => { var local; return value; };\
         const secondArrow = async (value) => { var local; return value; };",
        &[
            NodeTag::FUNCTION_DECLARATION,
            NodeTag::METHOD_DEFINITION,
            NodeTag::ARROW_FUNCTION_EXPRESSION,
        ],
    )];

    assert_clean_cases(&cases);
}

/// Binding and assignment positions must distinguish patterns from array/object expressions.
#[test]
fn parser_should_accept_binding_and_assignment_patterns() {
    let cases = [
        GrammarCase::script(
            "array binding",
            "const [first, , third = 3, ...rest] = values;",
            &[
                NodeTag::ARRAY_PATTERN,
                NodeTag::ASSIGNMENT_PATTERN,
                NodeTag::REST_ELEMENT,
            ],
        ),
        GrammarCase::script(
            "object binding",
            "const { value: renamed = 1, shorthand, ...rest } = object;",
            &[
                NodeTag::OBJECT_PATTERN,
                NodeTag::ASSIGNMENT_PATTERN,
                NodeTag::REST_ELEMENT,
            ],
        ),
        GrammarCase::script(
            "parameter patterns",
            "function read({ value }, [first], fallback = 0, ...rest) { return value; }",
            &[
                NodeTag::OBJECT_PATTERN,
                NodeTag::ARRAY_PATTERN,
                NodeTag::ASSIGNMENT_PATTERN,
                NodeTag::REST_ELEMENT,
            ],
        ),
        GrammarCase::script(
            "assignment patterns",
            "({ value, nested: [first] } = source);",
            &[
                NodeTag::OBJECT_PATTERN,
                NodeTag::ARRAY_PATTERN,
                NodeTag::ASSIGNMENT_EXPRESSION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Declaration initializers and binding-element defaults occupy distinct `ESTree` fields.
#[test]
#[allow(clippy::too_many_lines)]
fn parser_should_separate_declaration_initializers_from_binding_defaults() {
    let source = "const value = source, second = other;\
                  for (let index = 0; index < limit; index++) {}\
                  function defaults(value = fallback, { key } = object, [first] = list, ...rest) {}\
                  const [nested = 1, { item: renamed = fallback }, [inner] = list] = source;";
    let parsed = parse(
        source,
        ParseOptions {
            source_kind: SourceKind::Script,
            ..ParseOptions::default()
        },
    )
    .expect("parse binding defaults");
    inspect_tape("binding defaults", &parsed).expect("valid binding-default tape");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);

    let declarators = parsed
        .tape
        .validation()
        .filter_map(|record| match record.expect("valid record").value {
            TapeValue::Node {
                tag: NodeTag::VARIABLE_DECLARATOR,
                fields,
                ..
            } => Some(fields.to_vec()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(declarators.len(), 4);
    for (declarator, expected_init) in
        declarators[..3]
            .iter()
            .zip([NodeTag::IDENTIFIER, NodeTag::IDENTIFIER, NodeTag::LITERAL])
    {
        assert_eq!(
            node_tag(&parsed.tape, declarator[0]),
            Ok(NodeTag::IDENTIFIER)
        );
        assert_eq!(node_tag(&parsed.tape, declarator[1]), Ok(expected_init));
    }
    assert_eq!(
        node_tag(&parsed.tape, declarators[3][0]),
        Ok(NodeTag::ARRAY_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, declarators[3][1]),
        Ok(NodeTag::IDENTIFIER)
    );

    let function = parsed
        .tape
        .validation()
        .find_map(|record| match record.expect("valid record").value {
            TapeValue::Node {
                tag: NodeTag::FUNCTION_DECLARATION,
                fields,
                ..
            } => Some(fields.to_vec()),
            _ => None,
        })
        .expect("function declaration");
    let parameters = list_items(&parsed.tape, function[1]).expect("function parameters");
    assert_eq!(parameters.len(), 4);
    assert_eq!(
        node_tag(&parsed.tape, parameters[0]),
        Ok(NodeTag::ASSIGNMENT_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, parameters[1]),
        Ok(NodeTag::ASSIGNMENT_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, parameters[2]),
        Ok(NodeTag::ASSIGNMENT_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, parameters[3]),
        Ok(NodeTag::REST_ELEMENT)
    );
    let object_default = node_fields(&parsed.tape, parameters[1], NodeTag::ASSIGNMENT_PATTERN)
        .expect("object default");
    let array_default = node_fields(&parsed.tape, parameters[2], NodeTag::ASSIGNMENT_PATTERN)
        .expect("array default");
    assert_eq!(
        node_tag(&parsed.tape, object_default[0]),
        Ok(NodeTag::OBJECT_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, array_default[0]),
        Ok(NodeTag::ARRAY_PATTERN)
    );
    let rest = node_fields(&parsed.tape, parameters[3], NodeTag::REST_ELEMENT).expect("rest");
    assert_eq!(node_tag(&parsed.tape, rest[0]), Ok(NodeTag::IDENTIFIER));

    let nested = node_fields(&parsed.tape, declarators[3][0], NodeTag::ARRAY_PATTERN)
        .expect("nested pattern");
    let nested = list_items(&parsed.tape, nested[0]).expect("nested elements");
    assert_eq!(
        node_tag(&parsed.tape, nested[0]),
        Ok(NodeTag::ASSIGNMENT_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, nested[1]),
        Ok(NodeTag::OBJECT_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, nested[2]),
        Ok(NodeTag::ASSIGNMENT_PATTERN)
    );
}

/// Rest bindings reject defaults while retaining a validated recovery tree.
#[test]
fn parser_should_diagnose_rest_binding_defaults() {
    assert_diagnostic_cases(
        &[
            GrammarCase::script(
                "parameter rest default",
                "function invalid(...rest = fallback) {}",
                &[NodeTag::REST_ELEMENT, NodeTag::ASSIGNMENT_PATTERN],
            ),
            GrammarCase::script(
                "array rest default",
                "const [...rest = fallback] = source;",
                &[NodeTag::ARRAY_PATTERN, NodeTag::REST_ELEMENT],
            ),
            GrammarCase::script(
                "object rest default",
                "const { ...rest = fallback } = source;",
                &[NodeTag::OBJECT_PATTERN, NodeTag::REST_ELEMENT],
            ),
        ],
        false,
    );
}

/// Catch parameters use binding-pattern nodes without conflicting with the enclosing scope.
#[test]
fn parser_should_preserve_catch_binding_patterns_and_scope() {
    let source = "let message;\
                  try {} catch ({ message, code = 1, ...rest }) {}\
                  try {} catch ([first, , third = 3, ...tail]) {}\
                  try {} catch {}";
    let parsed = parse(
        source,
        ParseOptions {
            source_kind: SourceKind::Script,
            semantic_errors: true,
            ..ParseOptions::default()
        },
    )
    .expect("parse catch patterns");
    inspect_tape("catch patterns", &parsed).expect("valid catch-pattern tape");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);

    let program =
        node_fields(&parsed.tape, parsed.tape.header().root, NodeTag::PROGRAM).expect("Program");
    let body = list_items(&parsed.tape, program[0]).expect("Program body");

    let object_try =
        node_fields(&parsed.tape, body[1], NodeTag::TRY_STATEMENT).expect("object try");
    let object_catch =
        node_fields(&parsed.tape, object_try[1], NodeTag::CATCH_CLAUSE).expect("object catch");
    let object = node_fields(&parsed.tape, object_catch[0], NodeTag::OBJECT_PATTERN)
        .expect("object pattern");
    let properties = list_items(&parsed.tape, object[0]).expect("object properties");
    assert_eq!(properties.len(), 3);
    assert_eq!(node_tag(&parsed.tape, properties[0]), Ok(NodeTag::PROPERTY));
    let default_property =
        node_fields(&parsed.tape, properties[1], NodeTag::PROPERTY).expect("default property");
    assert_eq!(
        node_tag(&parsed.tape, default_property[1]),
        Ok(NodeTag::ASSIGNMENT_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, properties[2]),
        Ok(NodeTag::REST_ELEMENT)
    );

    let array_try = node_fields(&parsed.tape, body[2], NodeTag::TRY_STATEMENT).expect("array try");
    let array_catch =
        node_fields(&parsed.tape, array_try[1], NodeTag::CATCH_CLAUSE).expect("array catch");
    let array =
        node_fields(&parsed.tape, array_catch[0], NodeTag::ARRAY_PATTERN).expect("array pattern");
    let elements = list_items(&parsed.tape, array[0]).expect("array elements");
    assert_eq!(elements.len(), 4);
    assert!(matches!(
        parsed.tape.value_at(elements[1]),
        Ok(TapeValue::Null)
    ));
    assert_eq!(
        node_tag(&parsed.tape, elements[2]),
        Ok(NodeTag::ASSIGNMENT_PATTERN)
    );
    assert_eq!(
        node_tag(&parsed.tape, elements[3]),
        Ok(NodeTag::REST_ELEMENT)
    );

    let optional_try =
        node_fields(&parsed.tape, body[3], NodeTag::TRY_STATEMENT).expect("optional try");
    let optional_catch =
        node_fields(&parsed.tape, optional_try[1], NodeTag::CATCH_CLAUSE).expect("optional catch");
    assert!(matches!(
        parsed.tape.value_at(optional_catch[0]),
        Ok(TapeValue::Null)
    ));
}

/// Catch parameters reject initializers and commas following rest bindings while recovering a tree.
#[test]
fn parser_should_diagnose_invalid_catch_bindings() {
    let cases = [
        (
            "try {} catch (error = fallback) {}",
            "expected RightParen, found Eq",
        ),
        (
            "try {} catch ([...rest, tail]) {}",
            "rest element must be last",
        ),
        ("try {} catch ([...rest,]) {}", "rest element must be last"),
        (
            "try {} catch ({ ...rest, }) {}",
            "rest property must be last",
        ),
    ];

    for (source, expected) in cases {
        let parsed = parse(source, ParseOptions::default()).expect("recover invalid catch");
        inspect_tape("invalid catch", &parsed).expect("valid recovered catch tape");
        assert_eq!(
            parsed
                .diagnostics
                .first()
                .map(|diagnostic| &*diagnostic.message),
            Some(expected)
        );
    }
}

/// Regular expressions and template literals require parser-directed rescanning after `/`, `}`, and tags.
#[test]
fn parser_should_accept_regular_expressions_and_templates() {
    let cases = [
        GrammarCase::script(
            "regular expression",
            "const matcher = /a(?:b|c)+/giu;",
            &[NodeTag::LITERAL],
        ),
        GrammarCase::script(
            "plain template",
            "const message = `plain text`;",
            &[NodeTag::TEMPLATE_LITERAL, NodeTag::TEMPLATE_ELEMENT],
        ),
        GrammarCase::script(
            "interpolated template",
            "const message = `hello ${name}, ${count + 1}`;",
            &[NodeTag::TEMPLATE_LITERAL, NodeTag::TEMPLATE_ELEMENT],
        ),
        GrammarCase::script(
            "tagged template",
            "html`<p>${content}</p>`;",
            &[
                NodeTag::TAGGED_TEMPLATE_EXPRESSION,
                NodeTag::TEMPLATE_LITERAL,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Invalid template escapes are syntax errors only for untagged literals. Tagged templates keep
/// parsing and mark each affected quasi so bindings can materialize a null cooked value.
#[test]
fn parser_should_distinguish_tagged_template_invalid_escapes() {
    assert_eq!(NodeTag::INVALID_TEMPLATE_ELEMENT.get(), 588);
    let source = r"tag`\01`; tag`\xg`; tag`\u0`; tag`\u{}`; tag`\u{110000}`; tag`\xg${value}\u0${other}\u{g}`;";
    let expected_quasis = [
        r"\01",
        r"\xg",
        r"\u0",
        r"\u{}",
        r"\u{110000}",
        r"\xg",
        r"\u0",
        r"\u{g}",
    ];

    for language in [
        Language::JavaScript,
        Language::JavaScriptJsx,
        Language::TypeScript,
        Language::TypeScriptJsx,
    ] {
        let options = ParseOptions {
            language,
            ..ParseOptions::default()
        };
        let parsed = parse(source, options).expect("parse tagged invalid escapes");
        assert!(
            parsed.diagnostics.is_empty(),
            "{language:?}: {:?}",
            parsed.diagnostics
        );
        let tags = inspect_tape("tagged invalid escapes", &parsed).expect("valid tagged tape");
        assert_eq!(
            tags.iter()
                .filter(|&&tag| tag == NodeTag::INVALID_TEMPLATE_ELEMENT)
                .count(),
            8,
            "{language:?}"
        );
        let quasis = parsed
            .tape
            .validation()
            .filter_map(
                |record| match record.expect("valid template record").value {
                    TapeValue::Node {
                        tag: NodeTag::INVALID_TEMPLATE_ELEMENT,
                        span,
                        ..
                    } => Some(&source[span.start as usize..span.end as usize]),
                    _ => None,
                },
            )
            .collect::<Vec<_>>();
        assert_eq!(quasis, expected_quasis, "{language:?}");
    }
}

/// Untagged invalid escapes diagnose in syntax-only and semantic modes while preserving all
/// quasis and following statements on the recovered tape.
#[test]
fn parser_should_recover_untagged_template_invalid_escapes_in_all_modes() {
    let source = r"`\01`; `\xg`; `\u0`; `\u{}`; `\u{110000}`; `\xg${value}\u0${other}\u{g}`;";
    let expected_quasis = [
        r"\01",
        r"\xg",
        r"\u0",
        r"\u{}",
        r"\u{110000}",
        r"\xg",
        r"\u0",
        r"\u{g}",
    ];

    for language in [
        Language::JavaScript,
        Language::JavaScriptJsx,
        Language::TypeScript,
        Language::TypeScriptJsx,
    ] {
        for semantic_errors in [false, true] {
            let recovered = parse(
                source,
                ParseOptions {
                    language,
                    semantic_errors,
                    ..ParseOptions::default()
                },
            )
            .expect("recover untagged invalid escapes");
            assert_eq!(recovered.diagnostics.len(), 8, "{language:?}");
            assert!(recovered.diagnostics.iter().all(|diagnostic| {
                let Some(span_text) =
                    source.get(diagnostic.span.start as usize..diagnostic.span.end as usize)
                else {
                    return false;
                };
                diagnostic.message == "invalid escape sequence in template literal"
                    && diagnostic.span.start <= diagnostic.span.end
                    && diagnostic.span.end as usize <= source.len()
                    && expected_quasis.contains(&span_text)
            }));
            let tags = inspect_tape("untagged invalid escapes", &recovered)
                .expect("valid recovered untagged tape");
            assert_eq!(
                tags.iter()
                    .filter(|&&tag| tag == NodeTag::INVALID_TEMPLATE_ELEMENT)
                    .count(),
                8,
                "{language:?}"
            );
        }
    }
}

/// Generic TypeScript tags share tagged semantics, while every valid escape keeps the legacy
/// `TemplateElement` wire tag.
#[test]
fn parser_should_preserve_generic_tags_and_valid_template_escapes() {
    for language in [Language::TypeScript, Language::TypeScriptJsx] {
        let parsed = parse(
            r"tag<Result>`\xg`;",
            ParseOptions {
                language,
                ..ParseOptions::default()
            },
        )
        .expect("parse a generic tagged template with an invalid escape");
        assert!(parsed.diagnostics.is_empty(), "{language:?}");
        let tags = inspect_tape("generic tagged invalid escape", &parsed).expect("valid tape");
        assert!(tags.contains(&NodeTag::TS_TAGGED_TEMPLATE_EXPRESSION));
        assert!(tags.contains(&NodeTag::INVALID_TEMPLATE_ELEMENT));
    }

    let valid = parse(
        r"tag`\0\x61\u0061\u{000000000061}\z`; `\0\x61\u0061\u{61}\z`;",
        ParseOptions::default(),
    )
    .expect("parse valid template escapes");
    assert!(valid.diagnostics.is_empty(), "{:?}", valid.diagnostics);
    let tags = inspect_tape("valid escapes", &valid).expect("valid template tape");
    assert!(!tags.contains(&NodeTag::INVALID_TEMPLATE_ELEMENT));
}

/// Resource declarations reuse the lexical-declaration tape shape in every language mode and
/// remain independent from async iteration in all four for-of combinations.
#[test]
fn parser_should_parse_explicit_resource_declarations_in_every_language_mode() {
    let source = concat!(
        "{ using first = acquire(), second = acquire(); }\n",
        "for (using classic = acquire(); ready; advance()) {}\n",
        "for (using syncResource of syncResources) {}\n",
        "async function consume() {\n",
        "  await using asyncResource = acquire();\n",
        "  for (await using asyncDisposal of syncResources) {}\n",
        "  for await (using syncDisposal of asyncResources) {}\n",
        "  for await (await using bothAsync of asyncResources) {}\n",
        "}\n",
    );

    for language in [
        Language::JavaScript,
        Language::JavaScriptJsx,
        Language::TypeScript,
        Language::TypeScriptJsx,
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                language,
                semantic_errors: true,
                ..ParseOptions::default()
            },
        )
        .expect("parse resource declarations");
        assert!(
            parsed.diagnostics.is_empty(),
            "{language:?}: {:?}",
            parsed.diagnostics
        );
        let tags = inspect_tape("resource declarations", &parsed).expect("valid resource tape");
        assert_eq!(
            tags.iter()
                .filter(|&&tag| tag == NodeTag::VARIABLE_DECLARATION)
                .count(),
            7,
            "{language:?}",
        );
    }
}

/// Contextual `using` lookahead must preserve existing identifier expressions while reporting
/// resource-declaration placement, binding, and for-head early errors.
#[test]
fn parser_should_disambiguate_and_validate_resource_declarations() {
    let expressions = GrammarCase::script(
        "using expressions",
        concat!(
            "var using, value, index, object, Type, let;\n",
            "using; using(value); using[index] = value;\n",
            "using in object; using instanceof Type;\n",
            "let in object; let instanceof Type; for (let in object) {}\n",
            "using\nlet = value;\n",
            "using: { break using; }\n",
            "async function f() { await using[index]; await using\nlet in object; }\n",
        ),
        &[
            NodeTag::LABELED_STATEMENT,
            NodeTag::AWAIT_EXPRESSION,
            NodeTag::FOR_IN_STATEMENT,
        ],
    );
    let parsed = parse(expressions.source, expressions.options(true)).expect("parse expressions");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    inspect_tape(expressions.name, &parsed).expect("valid expression tape");

    let contextual_yield = GrammarCase::script(
        "contextual yield resource bindings",
        concat!(
            "function sync() { using yield = null; for (using yield of values) {} }\n",
            "async function asynchronous() { await using yield = null; }\n",
        ),
        &[NodeTag::VARIABLE_DECLARATION, NodeTag::FOR_OF_STATEMENT],
    );
    let parsed = parse(contextual_yield.source, contextual_yield.options(true))
        .expect("parse contextual yield resource bindings");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    inspect_tape(contextual_yield.name, &parsed).expect("valid contextual yield resource tape");

    let invalid = [
        GrammarCase::script(
            "script top-level using",
            "using resource = acquire();",
            &[NodeTag::VARIABLE_DECLARATION],
        ),
        GrammarCase::module(
            "sync await using",
            "function f() { await using resource = acquire(); }",
            &[NodeTag::VARIABLE_DECLARATION],
        ),
        GrammarCase::module(
            "resource binding restrictions",
            concat!(
                "{ using missing; using { value } = source; using let = source; using enum = null; }",
                "async function f() { await using enum = null; }",
            ),
            &[NodeTag::VARIABLE_DECLARATION],
        ),
        GrammarCase::module(
            "resource for-head restrictions",
            "for (using item in source) {} for (using item = value of source) {}",
            &[NodeTag::FOR_IN_STATEMENT, NodeTag::FOR_OF_STATEMENT],
        ),
        GrammarCase::module(
            "for await requires for-of",
            "async function f() { for await (using item = value;;) {} }",
            &[NodeTag::FOR_STATEMENT],
        ),
        GrammarCase::module(
            "resource statement placement",
            "if (ready) using item = value; switch (key) { case 0: using other = value; }",
            &[NodeTag::IF_STATEMENT, NodeTag::SWITCH_STATEMENT],
        ),
        GrammarCase::module(
            "exported resource declaration",
            "export using item = value;",
            &[NodeTag::EXPORT_NAMED_DECLARATION],
        ),
        GrammarCase::script(
            "strict contextual yield resource binding",
            "'use strict'; function f() { using yield = null; } function g(yield) {}",
            &[NodeTag::VARIABLE_DECLARATION],
        ),
        GrammarCase::script(
            "strict body contextual yield parameter binding",
            "function f(yield) { 'use strict'; }",
            &[NodeTag::FUNCTION_DECLARATION],
        ),
        GrammarCase::script(
            "generator contextual yield resource binding",
            "function* f() { using yield = null; }",
            &[NodeTag::VARIABLE_DECLARATION],
        ),
    ];
    assert_diagnostic_cases(&invalid, true);
}

#[test]
fn parser_should_validate_for_await_grammar_and_contexts() {
    assert_diagnostic_cases(
        &[
            GrammarCase::module(
                "for await requires an async or module context",
                concat!(
                    "function f() {",
                    " for await (item of source) {} for await (using resource of source) {}",
                    " for await (await using asyncResource of source) {}",
                    "}",
                ),
                &[NodeTag::FOR_OF_STATEMENT],
            ),
            GrammarCase::script(
                "for-of expression head cannot start with let",
                "for (let.member of source) {} for (let().member of source) {} for (let``.member of source) {}",
                &[NodeTag::FOR_OF_STATEMENT],
            ),
            GrammarCase::module(
                "class static blocks do not inherit async statement context",
                concat!(
                    "async function outer() { class C { static {",
                    " await using resource = acquire();",
                    " await using\nitem = source;",
                    " for await (item of source) {}",
                    " for await (using other of source) {}",
                    "} } }",
                ),
                &[NodeTag::VARIABLE_DECLARATION, NodeTag::FOR_OF_STATEMENT],
            ),
        ],
        true,
    );

    for source in [
        "async function f() { for await (item in source) {} }",
        "async function f() { for await (using item in source) {} }",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                source_kind: SourceKind::Module,
                semantic_errors: false,
                ..ParseOptions::default()
            },
        )
        .expect("recover invalid for-await-in statement");
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect("valid for-await-in tape");
    }

    for source_kind in [SourceKind::Script, SourceKind::CommonJs] {
        let parsed = parse(
            "for await (item of source) {}",
            ParseOptions {
                source_kind,
                semantic_errors: true,
                ..ParseOptions::default()
            },
        )
        .expect("recover invalid top-level for-await statement");
        assert!(!parsed.diagnostics.is_empty(), "{source_kind:?}");
    }

    let module_block = parse(
        concat!(
            "{ await using resource = acquire(); for await (using item of source) {} }",
            "class C { async method() { await using resource = acquire();",
            " for await (using item of source) {} } }",
            "async function wrapper() { class AwaitKey { [await key]() {} } }",
        ),
        ParseOptions {
            source_kind: SourceKind::Module,
            semantic_errors: true,
            ..ParseOptions::default()
        },
    )
    .expect("parse module-level await syntax in a nested block");
    assert!(
        module_block.diagnostics.is_empty(),
        "{:?}",
        module_block.diagnostics
    );

    let typescript_semantic_free = parse(
        concat!(
            "function f() {",
            " for await (item of source) {}",
            " for await (using resource of source) {}",
            " for await (await using asyncResource of source) {}",
            "}",
        ),
        ParseOptions {
            language: Language::TypeScript,
            source_kind: SourceKind::Script,
            semantic_errors: false,
            ..ParseOptions::default()
        },
    )
    .expect("parse TypeScript for-await contexts without semantic errors");
    assert!(
        typescript_semantic_free.diagnostics.is_empty(),
        "{:?}",
        typescript_semantic_free.diagnostics
    );
}

/// Invalid literal flags are diagnosed while runtime `RegExp` arguments remain ordinary strings.
///
/// Spec: regular-expression literal flag validation is a parse-time check, unlike construction.
#[test]
fn parser_should_validate_regular_expression_literal_flags() {
    assert_diagnostic_cases(
        &[
            GrammarCase::script("invalid flag", "/./G;", &[NodeTag::LITERAL]),
            GrammarCase::script("duplicate flag", "/./gig;", &[NodeTag::LITERAL]),
            GrammarCase::script("incompatible flags", "/./uv;", &[NodeTag::LITERAL]),
        ],
        true,
    );
    assert_clean_cases(&[
        GrammarCase::script(
            "runtime constructor validation",
            r#"new RegExp(".", "uv"); RegExp("\\p{Unknown}", "u");"#,
            &[NodeTag::NEW_EXPRESSION, NodeTag::CALL_EXPRESSION],
        ),
        GrammarCase {
            name: "TypeScript scanner compatibility",
            source: "/foo/visualstudiocode; /./uv; /(?𝘴𝘪-𝘮:^𝘧𝘰𝘰.)/𝘨𝘮𝘶;",
            language: Language::TypeScript,
            source_kind: SourceKind::Script,
            expected_tags: &[NodeTag::LITERAL],
        },
    ]);
}

/// Optional member, element, and call chains must be wrapped once in a chain expression.
#[test]
fn parser_should_accept_optional_chaining() {
    let cases = [
        GrammarCase::script(
            "optional member",
            "object?.value;",
            &[NodeTag::CHAIN_EXPRESSION, NodeTag::MEMBER_EXPRESSION],
        ),
        GrammarCase::script(
            "optional element",
            "object?.[key];",
            &[NodeTag::CHAIN_EXPRESSION, NodeTag::MEMBER_EXPRESSION],
        ),
        GrammarCase::script(
            "optional call",
            "callback?.(argument);",
            &[NodeTag::CHAIN_EXPRESSION, NodeTag::CALL_EXPRESSION],
        ),
        GrammarCase::script(
            "mixed optional chain",
            "object?.method?.(argument)?.result;",
            &[
                NodeTag::CHAIN_EXPRESSION,
                NodeTag::MEMBER_EXPRESSION,
                NodeTag::CALL_EXPRESSION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Dot member properties accept `IdentifierName` keywords and declared private names.
#[test]
fn parser_should_accept_keyword_and_private_member_names() {
    let cases = [
        GrammarCase::script(
            "every keyword IdentifierName",
            "object.break.case.catch.class.const.continue.debugger.default.delete.do.else.export.extends.false.finally.for.function.if.import.in.instanceof.new.null.return.super.switch.this.throw.true.try.typeof.var.void.while.with.yield.async.await.let.static.of.get.set.as.satisfies.accessor.using.declare.abstract.interface.type.enum.namespace.module.implements.infer.keyof.readonly.unique.unknown.never.any.boolean.number.string.symbol.object.undefined.is.asserts.public.protected.private.override.out.meta.from.require;",
            &[NodeTag::MEMBER_EXPRESSION],
        ),
        GrammarCase::script(
            "optional keyword IdentifierName",
            "iterator?.return;",
            &[NodeTag::CHAIN_EXPRESSION, NodeTag::MEMBER_EXPRESSION],
        ),
        GrammarCase::script(
            "declared private member",
            "class C { read(object) { return object.#field; } optional(object) { return object?.#field; } #field = 1; }",
            &[
                NodeTag::CLASS_DECLARATION,
                NodeTag::PRIVATE_IDENTIFIER,
                NodeTag::CHAIN_EXPRESSION,
                NodeTag::MEMBER_EXPRESSION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Private member names retain class-scope declaration diagnostics.
#[test]
fn parser_should_diagnose_invalid_private_member_scope() {
    let cases = [
        (
            "object.#field;",
            "private name is only valid inside a class",
        ),
        (
            "class C { read(object) { return object.#missing; } }",
            "private name `missing` is not declared",
        ),
    ];

    for (source, expected_message) in cases {
        let parsed = parse(source, ParseOptions::default()).expect("recover parse");
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == expected_message),
            "missing `{expected_message}` in {:?}",
            parsed.diagnostics
        );
    }
}

/// Async and generator context controls whether `await` and `yield` are expressions or identifiers.
#[test]
fn parser_should_accept_async_functions_and_generators() {
    let cases = [
        GrammarCase::script(
            "generator",
            "function* sequence() { yield 1; yield* other(); }",
            &[NodeTag::FUNCTION_DECLARATION, NodeTag::YIELD_EXPRESSION],
        ),
        GrammarCase::script(
            "async function",
            "async function load() { return await fetchValue(); }",
            &[NodeTag::FUNCTION_DECLARATION, NodeTag::AWAIT_EXPRESSION],
        ),
        GrammarCase::script(
            "async function expressions",
            "const load = async function (value) { return await fetchValue(value); }; const stream = async function* named() { yield await next(); };",
            &[
                NodeTag::FUNCTION_EXPRESSION,
                NodeTag::AWAIT_EXPRESSION,
                NodeTag::YIELD_EXPRESSION,
            ],
        ),
        GrammarCase::script(
            "async arrow",
            "const load = async (value) => await transform(value);",
            &[
                NodeTag::ARROW_FUNCTION_EXPRESSION,
                NodeTag::AWAIT_EXPRESSION,
            ],
        ),
        GrammarCase::module(
            "top-level await",
            "const value = await load();",
            &[NodeTag::AWAIT_EXPRESSION],
        ),
        GrammarCase::module(
            "for await",
            "async function consume() { for await (const value of stream) { await use(value); } }",
            &[
                NodeTag::FUNCTION_DECLARATION,
                NodeTag::FOR_OF_STATEMENT,
                NodeTag::AWAIT_EXPRESSION,
            ],
        ),
        GrammarCase::module(
            "exported async functions",
            "export async function load() { return await fetchValue(); } export default async function* stream() { yield await next(); }",
            &[
                NodeTag::EXPORT_NAMED_DECLARATION,
                NodeTag::EXPORT_DEFAULT_DECLARATION,
                NodeTag::FUNCTION_DECLARATION,
                NodeTag::AWAIT_EXPRESSION,
                NodeTag::YIELD_EXPRESSION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// A bare yield ends before enclosing punctuation, while another yield can begin its argument.
#[test]
fn parser_should_end_bare_yield_at_expression_boundaries() {
    assert_clean_cases(&[
        GrammarCase::script(
            "bare yield expression boundaries",
            "function* sequence(value) { const array = [yield, yield yield]; const object = { key: yield, ...yield }; consume(yield); switch (value) { case yield: break; } }",
            &[
                NodeTag::FUNCTION_DECLARATION,
                NodeTag::YIELD_EXPRESSION,
                NodeTag::ARRAY_EXPRESSION,
                NodeTag::OBJECT_EXPRESSION,
            ],
        ),
        GrammarCase::script(
            "nested generator yield outside arrow parameters",
            "function* outer() { (value = function* () { yield; }) => value; }",
            &[
                NodeTag::ARROW_FUNCTION_EXPRESSION,
                NodeTag::FUNCTION_EXPRESSION,
                NodeTag::YIELD_EXPRESSION,
            ],
        ),
    ]);

    let delegated = parse(
        "function* sequence() { consume(yield*); }",
        ParseOptions {
            semantic_errors: true,
            ..ParseOptions::default()
        },
    )
    .expect("recover missing delegated yield argument");
    assert!(
        delegated
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "yield delegation requires an expression"),
        "{:?}",
        delegated.diagnostics
    );
    delegated.tape.validate().expect("valid recovery tape");

    for source in [
        "function* sequence() { yield ? one : two; }",
        "function* sequence(source) { yield in source; }",
        "function* sequence() { (value = yield) => value; }",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                semantic_errors: true,
                ..ParseOptions::default()
            },
        )
        .expect("recover invalid yield continuation");
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect("valid recovery tape");
    }
}

/// Escapes and line terminators keep `async` from introducing a function expression.
#[test]
fn parser_should_respect_async_function_expression_introducer_boundaries() {
    assert_clean_cases(&[GrammarCase::script(
        "line break",
        "const value = async\nfunction split() {}",
        &[NodeTag::IDENTIFIER, NodeTag::FUNCTION_DECLARATION],
    )]);
    assert_diagnostic_cases(
        &[GrammarCase::script(
            "escaped async",
            "const value = \\u0061sync function split() {}",
            &[NodeTag::IDENTIFIER, NodeTag::FUNCTION_DECLARATION],
        )],
        false,
    );
}

/// Async function expressions enforce their contextual and parameter-list early errors.
#[test]
fn parser_should_diagnose_async_function_expression_early_errors() {
    let allowed_declaration = parse(
        "async function await() {}",
        ParseOptions {
            semantic_errors: true,
            source_kind: SourceKind::Script,
            ..ParseOptions::default()
        },
    )
    .expect("async declaration named await");
    assert!(
        allowed_declaration.diagnostics.is_empty(),
        "{:?}",
        allowed_declaration.diagnostics
    );

    let cases = [
        GrammarCase::script(
            "await async function expression name",
            "const value = async function await() {};",
            &[NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "await async generator name",
            "const value = async function* await() {};",
            &[NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "escaped await binding",
            "const value = async function() { var \\u0061wait; };",
            &[NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "escaped await reference",
            "const value = async function() { void \\u0061wait; };",
            &[NodeTag::FUNCTION_EXPRESSION, NodeTag::UNARY_EXPRESSION],
        ),
        GrammarCase::script(
            "yield generator name",
            "const value = async function* yield() {};",
            &[NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "escaped yield binding",
            "const value = async function*() { var \\u0079ield; };",
            &[NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "escaped yield reference",
            "const value = async function*() { void \\u0079ield; };",
            &[NodeTag::FUNCTION_EXPRESSION, NodeTag::UNARY_EXPRESSION],
        ),
        GrammarCase::script(
            "await parameter expression",
            "const value = async function(input = await source) {};",
            &[NodeTag::FUNCTION_EXPRESSION, NodeTag::AWAIT_EXPRESSION],
        ),
        GrammarCase::script(
            "yield parameter expression",
            "const value = async function*(input = yield source) {};",
            &[NodeTag::FUNCTION_EXPRESSION, NodeTag::YIELD_EXPRESSION],
        ),
        GrammarCase::script(
            "non-simple strict parameters",
            "const value = async function(input = source) { 'use strict'; };",
            &[NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "rest trailing comma",
            "const value = async function(...inputs,) {};",
            &[NodeTag::FUNCTION_EXPRESSION, NodeTag::REST_ELEMENT],
        ),
        GrammarCase::script(
            "parameter body collision",
            "const value = async function(input) { let input; };",
            &[NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "super property",
            "const value = async function*() { super.value; };",
            &[NodeTag::FUNCTION_EXPRESSION, NodeTag::SUPER],
        ),
        GrammarCase::script(
            "assignment target",
            "(async function() {}) = value;",
            &[NodeTag::FUNCTION_EXPRESSION, NodeTag::ASSIGNMENT_EXPRESSION],
        ),
        GrammarCase::script(
            "yield star line break",
            "const value = async function*() { yield\n* source; };",
            &[NodeTag::FUNCTION_EXPRESSION, NodeTag::YIELD_EXPRESSION],
        ),
    ];

    assert_diagnostic_cases(&cases, true);
}

/// Arrow functions inherit a method's `super` permission, while ordinary functions reset it.
#[test]
fn parser_should_track_super_permission_across_nested_functions() {
    for source in [
        "const object = { method() { return () => super.value; } };",
        "class Child extends Parent { method() { return () => super.method(); } }",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                semantic_errors: true,
                ..ParseOptions::default()
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

    assert_diagnostic_cases(
        &[GrammarCase::script(
            "ordinary nested function",
            "class Child extends Parent { method() { return function() { return super.method(); }; } }",
            &[NodeTag::FUNCTION_EXPRESSION, NodeTag::SUPER],
        )],
        true,
    );
}

/// `async function` export forms may not cross a line terminator.
#[test]
fn parser_should_respect_line_breaks_in_exported_async_functions() {
    assert_diagnostic_cases(
        &[GrammarCase::module(
            "named export line break",
            "export async/*\n*/function split() {}",
            &[
                NodeTag::EXPORT_NAMED_DECLARATION,
                NodeTag::FUNCTION_DECLARATION,
            ],
        )],
        false,
    );
    assert_clean_cases(&[GrammarCase::module(
        "default export line break",
        "export default async\nfunction split() {}",
        &[
            NodeTag::EXPORT_DEFAULT_DECLARATION,
            NodeTag::FUNCTION_DECLARATION,
        ],
    )]);
}

/// Malformed exported async functions recover inside their original export wrappers.
#[test]
fn parser_should_recover_malformed_exported_async_functions() {
    assert_diagnostic_cases(
        &[
            GrammarCase::module(
                "malformed named async function export",
                "export async function broken(",
                &[
                    NodeTag::EXPORT_NAMED_DECLARATION,
                    NodeTag::FUNCTION_DECLARATION,
                ],
            ),
            GrammarCase::module(
                "malformed default async generator export",
                "export default async function* broken(",
                &[
                    NodeTag::EXPORT_DEFAULT_DECLARATION,
                    NodeTag::FUNCTION_DECLARATION,
                ],
            ),
        ],
        false,
    );
}

/// Generator methods preserve prefixes, computed names, and isolated function scopes.
#[test]
fn parser_should_accept_object_and_class_generator_methods() {
    let cases = [
        GrammarCase::script(
            "object generator methods",
            "const methods = { *plain(value) { yield value; }, *[key](value) { yield value; }, async *stream(value) { await load(value); yield value; }, async: asyncValue, get: getValue, set: setValue, static: staticValue };",
            &[
                NodeTag::OBJECT_EXPRESSION,
                NodeTag::PROPERTY,
                NodeTag::FUNCTION_EXPRESSION,
                NodeTag::YIELD_EXPRESSION,
                NodeTag::AWAIT_EXPRESSION,
            ],
        ),
        GrammarCase::script(
            "class generator methods",
            "class Methods { *plain(value) { yield value; } *[key](value) { yield value; } async *stream(value) { await load(value); yield value; } static *values(value) { yield value; } static async *entries(value) { await load(value); yield value; } static() {} async() {} get() {} set() {} }",
            &[
                NodeTag::CLASS_DECLARATION,
                NodeTag::METHOD_DEFINITION,
                NodeTag::FUNCTION_EXPRESSION,
                NodeTag::YIELD_EXPRESSION,
                NodeTag::AWAIT_EXPRESSION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Async ordinary methods preserve public, private, computed, and static method forms.
#[test]
fn parser_should_accept_object_and_class_async_methods() {
    let cases = [
        GrammarCase::script(
            "object async methods",
            "const methods = { async plain(value) { await load(value); }, async [key]() {}, async 'named'() {}, async() {} };",
            &[
                NodeTag::OBJECT_EXPRESSION,
                NodeTag::PROPERTY,
                NodeTag::FUNCTION_EXPRESSION,
                NodeTag::AWAIT_EXPRESSION,
            ],
        ),
        GrammarCase::script(
            "class async methods",
            "class Methods { async plain(value) { await load(value); } static async [key]() {} async #private() {} static async #privateStatic() {} async() {} }",
            &[
                NodeTag::CLASS_DECLARATION,
                NodeTag::METHOD_DEFINITION,
                NodeTag::FUNCTION_EXPRESSION,
                NodeTag::PRIVATE_IDENTIFIER,
                NodeTag::AWAIT_EXPRESSION,
            ],
        ),
        GrammarCase::script(
            "line-broken and escaped async keys",
            "class Methods { async\nplain() {} \\u0061sync() {} }",
            &[NodeTag::PROPERTY_DEFINITION, NodeTag::METHOD_DEFINITION],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Escaped or line-broken `async` tokens do not introduce async methods.
#[test]
fn parser_should_respect_async_method_introducer_boundaries() {
    let cases = [
        GrammarCase::script(
            "escaped class async modifier",
            "class Methods { \\u0061sync method() {} }",
            &[NodeTag::PROPERTY_DEFINITION, NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "line-broken object async modifier",
            "const methods = { async\nmethod() {} };",
            &[NodeTag::OBJECT_EXPRESSION, NodeTag::PROPERTY],
        ),
    ];

    assert_diagnostic_cases(&cases, true);
}

#[test]
fn parser_should_reject_arguments_in_class_initializers_but_stop_at_function_boundaries() {
    for source in [
        "function outer() { class C { field = arguments; } }",
        "class C { field = () => arguments; }",
        "class C { field = async () => ({ arguments }); }",
        "class C { #field = () => { const nested = () => arguments; }; }",
        "class C { field = ({ [arguments]() {} }); }",
        "class C { static field = typeof arguments; }",
        "class C { static { arguments; } }",
        r"class C { static { class Nested { [argument\u0073]() {} } } }",
    ] {
        let parsed = parse(
            source,
            ParseOptions {
                source_kind: SourceKind::Script,
                semantic_errors: true,
                ..ParseOptions::default()
            },
        )
        .expect("recover forbidden class arguments reference");
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed
            .tape
            .validate()
            .expect("valid class arguments recovery tape");
    }

    let allowed = parse(
        "function outer() { class C { [arguments] = 1; member = object.arguments; named = ({ arguments: 1, arguments() {} }); field = function(value = arguments) { return arguments; }; generator = function*(value = arguments) { return arguments; }; asynchronous = async function(value = arguments) { return arguments; }; arrow = () => function() { return arguments; }; static { function nested(value = arguments) { return arguments; } class Nested { method() { return arguments; } } } } }",
        ParseOptions {
            source_kind: SourceKind::Script,
            semantic_errors: true,
            ..ParseOptions::default()
        },
    )
    .expect("parse arguments beyond class initializer boundaries");
    assert!(allowed.diagnostics.is_empty(), "{:#?}", allowed.diagnostics);
    allowed
        .tape
        .validate()
        .expect("valid class arguments boundary tape");

    let syntax_only = parse(
        "class C { field = arguments; static { arguments; } }",
        ParseOptions {
            semantic_errors: false,
            ..ParseOptions::default()
        },
    )
    .expect("parse class arguments references without semantic errors");
    assert!(
        syntax_only.diagnostics.is_empty(),
        "{:#?}",
        syntax_only.diagnostics
    );
}

/// Async methods retain contextual, parameter-list, and nested-function early errors.
#[test]
fn parser_should_diagnose_async_method_early_errors() {
    let cases = [
        GrammarCase::script(
            "await parameter name",
            "const methods = { async invalid(await) {} };",
            &[NodeTag::PROPERTY, NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "escaped await binding",
            "class Methods { async invalid() { var \\u0061wait; } }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "escaped await reference",
            "class Methods { static async invalid() { void \\u0061wait; } }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::UNARY_EXPRESSION],
        ),
        GrammarCase::script(
            "non-simple strict parameters",
            "const methods = { async invalid(value = fallback) { 'use strict'; } };",
            &[NodeTag::PROPERTY, NodeTag::ASSIGNMENT_PATTERN],
        ),
        GrammarCase::script(
            "rest trailing comma",
            "class Methods { async invalid(...values,) {} }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::REST_ELEMENT],
        ),
        GrammarCase::script(
            "duplicate parameters",
            "const methods = { async invalid(value, value) {} };",
            &[NodeTag::PROPERTY, NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "direct super call",
            "const methods = { async invalid() { super(); } };",
            &[NodeTag::PROPERTY, NodeTag::SUPER],
        ),
        GrammarCase::script(
            "nested ordinary function super",
            "class Methods extends Base { async invalid() { return function() { return super.value; }; } }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::SUPER],
        ),
        GrammarCase::script(
            "nested class static block super call",
            "class Outer extends Base { constructor() { class Inner { static { super(); } } } }",
            &[
                NodeTag::CLASS_DECLARATION,
                NodeTag::STATIC_BLOCK,
                NodeTag::SUPER,
            ],
        ),
    ];

    assert_diagnostic_cases(&cases, true);
}

/// Public class accessors reuse method functions while preserving ordinary `get` and `set` names.
#[test]
fn parser_should_accept_public_class_accessors() {
    let cases = [
        GrammarCase::script(
            "public class accessors",
            "class Accessors { get value() { return this._value; } set value({ next } = fallback) { this._value = next; } static get [key]() { return value; } static set \"named\"(value) {} get static() {} static get static() {} }",
            &[
                NodeTag::CLASS_DECLARATION,
                NodeTag::METHOD_DEFINITION,
                NodeTag::FUNCTION_EXPRESSION,
                NodeTag::ASSIGNMENT_PATTERN,
                NodeTag::OBJECT_PATTERN,
            ],
        ),
        GrammarCase::script(
            "ordinary get and set members",
            "class Names { get() {} set(value) {} get; set = value; static get() {} static set(value) {} }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::PROPERTY_DEFINITION],
        ),
        GrammarCase::script(
            "computed special accessor names",
            "class Special { get ['constructor']() {} set ['constructor'](value) {} static get ['prototype']() {} static set ['prototype'](value) {} }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "accessor super property and nested constructor",
            "class Outer extends Base { get inherited() { return super.value; } set inherited(value) { super.value = value; } get Nested() { return class Inner extends Base { constructor() { super(); } }; } }",
            &[
                NodeTag::METHOD_DEFINITION,
                NodeTag::MEMBER_EXPRESSION,
                NodeTag::CLASS_EXPRESSION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Private accessors share one canonical private name while preserving static and Unicode keys.
#[test]
fn parser_should_accept_private_class_accessors() {
    let cases = [
        GrammarCase::script(
            "paired escaped private accessors",
            "class Accessors { get #\\u0076alue() { return this.#value; } set #value({ next } = fallback) { this.#value = next; } }",
            &[
                NodeTag::CLASS_DECLARATION,
                NodeTag::METHOD_DEFINITION,
                NodeTag::PRIVATE_IDENTIFIER,
                NodeTag::FUNCTION_EXPRESSION,
                NodeTag::ASSIGNMENT_PATTERN,
                NodeTag::OBJECT_PATTERN,
            ],
        ),
        GrammarCase::script(
            "static and Unicode private accessors",
            "class Accessors { static get #π() { return this.#π; } static set #π(value) { this.#π = value; } get #only() {} set #write(value) {} }",
            &[
                NodeTag::METHOD_DEFINITION,
                NodeTag::PRIVATE_IDENTIFIER,
                NodeTag::MEMBER_EXPRESSION,
            ],
        ),
        GrammarCase::script(
            "nested private accessor scopes",
            "class Outer { get #value() { return class Inner { get #value() { return this.#value; } set #value(next) {} }; } set #value(next) {} }",
            &[
                NodeTag::CLASS_DECLARATION,
                NodeTag::CLASS_EXPRESSION,
                NodeTag::METHOD_DEFINITION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Accessor arity and noncomputed class special-name restrictions are early errors.
#[test]
fn parser_should_diagnose_public_class_accessor_early_errors() {
    let cases = [
        GrammarCase::script(
            "getter parameter",
            "class C { get value(parameter) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "setter without parameter",
            "class C { set value() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "setter with two parameters",
            "class C { set value(first, second) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "setter rest parameter",
            "class C { set value(...values) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "setter trailing comma",
            "class C { set value(parameter,) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "getter constructor",
            "class C { get constructor() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "quoted setter constructor",
            "class C { set 'constructor'(value) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "escaped quoted getter constructor",
            "class C { get \"constr\\u0075ctor\"() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "escaped identifier setter constructor",
            "class C { set constr\\u0075ctor(value) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "static getter prototype",
            "class C { static get prototype() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "quoted static setter prototype",
            "class C { static set 'prototype'(value) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "escaped static getter prototype",
            "class C { static get prot\\u006ftype() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "direct super in getter",
            "class C extends Base { get value() { super(); } }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "direct super in static setter",
            "class C extends Base { static set value(next) { super(); } }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "strict with in getter",
            "class C { get value() { with (object) statement; } }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::WITH_STATEMENT],
        ),
        GrammarCase::script(
            "strict delete in setter",
            "class C { set value(next) { delete identifier; } }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::UNARY_EXPRESSION],
        ),
        GrammarCase::script(
            "escaped get introducer",
            "class C { g\\u0065t value() {} }",
            &[NodeTag::PROPERTY_DEFINITION, NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "escaped set introducer",
            "class C { s\\u0065t value(next) {} }",
            &[NodeTag::PROPERTY_DEFINITION, NodeTag::METHOD_DEFINITION],
        ),
    ];

    assert_diagnostic_cases(&cases, true);
}

/// Private accessor arity, private-name collisions, and class-only syntax are early errors.
#[test]
fn parser_should_diagnose_private_class_accessor_early_errors() {
    let cases = [
        GrammarCase::script(
            "private getter parameter",
            "class C { get #value(parameter) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "private setter without parameter",
            "class C { set #value() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "private setter rest parameter",
            "class C { set #value(...values) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "private setter trailing comma",
            "class C { set #value(parameter,) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "duplicate private getters",
            "class C { get #value() {} get #value() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "duplicate private setters",
            "class C { set #value(next) {} set #value(next) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "private getter after field",
            "class C { #value; get #value() {} }",
            &[NodeTag::PROPERTY_DEFINITION, NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "private field after setter",
            "class C { set #value(next) {} #value; }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::PROPERTY_DEFINITION],
        ),
        GrammarCase::script(
            "private setter after method",
            "class C { #value() {} set #value(next) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "private method after getter",
            "class C { get #value() {} #value() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "mixed static private accessors",
            "class C { get #value() {} static set #value(next) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "mixed escaped static private accessors",
            "class C { static get #\\u0076alue() {} set #value(next) {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "private constructor accessor",
            "class C { get #constructor() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "direct super in private getter",
            "class C extends Base { get #value() { super(); } }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "strict private setter body",
            "class C { set #value(next) { with (object) statement; } }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::WITH_STATEMENT],
        ),
        GrammarCase::script(
            "escaped private get introducer",
            "class C { g\\u0065t #value() {} }",
            &[NodeTag::PROPERTY_DEFINITION, NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "split private name",
            "class C { get # value() {} }",
            &[NodeTag::METHOD_DEFINITION],
        ),
        GrammarCase::script(
            "object private getter",
            "const object = { get #value() {} };",
            &[NodeTag::OBJECT_EXPRESSION],
        ),
        GrammarCase::script(
            "object private setter inside class",
            "class C { method() { return { set #value(next) {} }; } }",
            &[NodeTag::OBJECT_EXPRESSION, NodeTag::METHOD_DEFINITION],
        ),
    ];

    assert_diagnostic_cases(&cases, true);
}

/// Object accessors preserve literal and computed names without changing ordinary property roles.
#[test]
fn parser_should_accept_public_object_accessors() {
    let cases = [
        GrammarCase::script(
            "public object accessors",
            "const accessors = { get value() { return this._value; }, set value({ next } = fallback) { this._value = next; }, get [key]() { return value; }, set 'named'(value) {}, get 0() {}, set 1n(value) {}, get return() {}, set async(value) {} };",
            &[
                NodeTag::OBJECT_EXPRESSION,
                NodeTag::PROPERTY,
                NodeTag::FUNCTION_EXPRESSION,
                NodeTag::ASSIGNMENT_PATTERN,
                NodeTag::OBJECT_PATTERN,
            ],
        ),
        GrammarCase::script(
            "ordinary get and set properties",
            "const names = { get() {}, set(value) {}, get, set, get: getter, set: setter, *generator() {}, *get() {}, async *set() {} };",
            &[
                NodeTag::OBJECT_EXPRESSION,
                NodeTag::PROPERTY,
                NodeTag::FUNCTION_EXPRESSION,
            ],
        ),
        GrammarCase::script(
            "object accessor super property and sloppy body",
            "const inherited = { get value() { with (object) return super.value; }, set value(next) { super.value = next; delete identifier; } };",
            &[
                NodeTag::PROPERTY,
                NodeTag::MEMBER_EXPRESSION,
                NodeTag::WITH_STATEMENT,
                NodeTag::UNARY_EXPRESSION,
            ],
        ),
        GrammarCase::script(
            "object accessor strict directive does not leak",
            "const strict = { get value() { 'use strict'; return this.value; }, set value(next) { 'use strict'; this.value = next; } }; with (target) statement;",
            &[
                NodeTag::PROPERTY,
                NodeTag::FUNCTION_EXPRESSION,
                NodeTag::WITH_STATEMENT,
            ],
        ),
        GrammarCase::module(
            "strict property and member identifier names",
            "const names = { static: 1, interface: 2, get public() { delete target.static; return target.static + target.interface; }, set private(value) { target.protected = value; } };",
            &[
                NodeTag::PROPERTY,
                NodeTag::MEMBER_EXPRESSION,
                NodeTag::UNARY_EXPRESSION,
            ],
        ),
        GrammarCase::script(
            "strict delete computed member containing private access",
            "class C { #key; get value() { delete target[this.#key]; return target[this.#key]; } }",
            &[
                NodeTag::METHOD_DEFINITION,
                NodeTag::MEMBER_EXPRESSION,
                NodeTag::UNARY_EXPRESSION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Object accessor arity, direct-super, and contextual introducer restrictions are early errors.
#[test]
fn parser_should_diagnose_public_object_accessor_early_errors() {
    let cases = [
        GrammarCase::script(
            "getter parameter",
            "const object = { get value(parameter) {} };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "setter without parameter",
            "const object = { set value() {} };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "setter with two parameters",
            "const object = { set value(first, second) {} };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "setter rest parameter",
            "const object = { set value(...values) {} };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "setter trailing comma",
            "const object = { set value(parameter,) {} };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "direct super in getter",
            "const object = { get value() { super(); } };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "direct super in setter",
            "const object = { set value(next) { super(); } };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "escaped get introducer",
            "const object = { g\\u0065t value() {} };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "escaped set introducer",
            "const object = { s\\u0065t value(next) {} };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "generator after get",
            "const object = { get *value() {} };",
            &[NodeTag::PROPERTY],
        ),
    ];

    assert_diagnostic_cases(&cases, true);
}

/// Cover grammar and strict accessor bodies retain their surrounding early errors.
#[test]
fn parser_should_diagnose_object_accessor_context_errors() {
    let cases = [
        GrammarCase::script(
            "accessor in assignment pattern",
            "0, [{ get value() {} }] = [{}];",
            &[NodeTag::ARRAY_PATTERN, NodeTag::OBJECT_PATTERN],
        ),
        GrammarCase::script(
            "accessor in for-of assignment pattern",
            "for ([{ set value(next) {} }] of source) {}",
            &[NodeTag::FOR_OF_STATEMENT, NodeTag::OBJECT_PATTERN],
        ),
        GrammarCase::script(
            "object accessor optional chain assignment",
            "[{ set value(next) {} }?.value = 1] = [2];",
            &[NodeTag::CHAIN_EXPRESSION, NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "use strict getter body",
            "const object = { get value() { 'use strict'; public = 1; } };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "use strict setter parameter",
            "const object = { set value(eval) { 'use strict'; } };",
            &[NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "use strict setter with default",
            "const object = { set value(next = 0) { 'use strict'; } };",
            &[NodeTag::PROPERTY, NodeTag::ASSIGNMENT_PATTERN],
        ),
        GrammarCase::module(
            "export declaration in getter",
            "const object = { get value() { export default null; } };",
            &[NodeTag::PROPERTY, NodeTag::EXPORT_DEFAULT_DECLARATION],
        ),
        GrammarCase::module(
            "import declaration in setter",
            "const object = { set value(next) { import value from './value.js'; } };",
            &[NodeTag::PROPERTY, NodeTag::IMPORT_DECLARATION],
        ),
        GrammarCase::module(
            "strict with in getter",
            "const object = { get value() { with (target) statement; } };",
            &[NodeTag::PROPERTY, NodeTag::WITH_STATEMENT],
        ),
        GrammarCase::module(
            "strict delete in setter",
            "const object = { set value(next) { delete identifier; } };",
            &[NodeTag::PROPERTY, NodeTag::UNARY_EXPRESSION],
        ),
        GrammarCase::script(
            "strict delete private member",
            "class C { #value; method() { delete this.#value; } }",
            &[NodeTag::CLASS_DECLARATION, NodeTag::UNARY_EXPRESSION],
        ),
        GrammarCase::script(
            "hashbang before strict directive",
            "#!/usr/bin/env node\n'use strict'; with (target) statement;",
            &[NodeTag::WITH_STATEMENT],
        ),
    ];

    assert_diagnostic_cases(&cases, true);
}

/// Strict reserved identifiers use one decoded spelling check without rejecting `IdentifierName`.
#[test]
fn parser_should_diagnose_strict_reserved_identifier_references() {
    let options = ParseOptions {
        language: Language::JavaScript,
        source_kind: SourceKind::Module,
        semantic_errors: true,
        ..ParseOptions::default()
    };
    for source in [
        "[implements];",
        "[interface];",
        "[let];",
        "[package];",
        "[private];",
        "[protected];",
        "[public];",
        "[static];",
        "[yield];",
        "[impl\\u0065ments];",
        "[st\\u0061tic];",
        "[yi\\u0065ld];",
    ] {
        let parsed = parse(source, options).expect(source);
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect(source);
    }

    let allowed = parse(
        "[fo\\u006f]; const names = { static: 1, interface: 2 }; names.static;",
        options,
    )
    .expect("allowed strict IdentifierName spellings");
    assert!(allowed.diagnostics.is_empty(), "{:#?}", allowed.diagnostics);
    allowed.tape.validate().expect("valid allowed tape");
}

/// Direct bindings reuse lexer escape metadata while object shorthands retain span-based checks.
#[test]
fn parser_should_diagnose_strict_reserved_bindings_with_and_without_escapes() {
    let options = ParseOptions {
        language: Language::JavaScript,
        source_kind: SourceKind::Module,
        semantic_errors: true,
        ..ParseOptions::default()
    };
    for source in [
        "let implements;",
        "let impl\\u0065ments;",
        "const { static } = source;",
        "const { st\\u0061tic } = source;",
    ] {
        let parsed = parse(source, options).expect(source);
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect(source);
    }

    for source in [
        "let ordinary;",
        "let ord\\u0069nary;",
        "const { ordinary } = source;",
        "const { ord\\u0069nary } = source;",
    ] {
        let parsed = parse(source, options).expect(source);
        assert!(
            parsed.diagnostics.is_empty(),
            "{source}: {:#?}",
            parsed.diagnostics
        );
        parsed.tape.validate().expect(source);
    }
}

/// Escaped reserved spellings remain lexical identifiers until their reference or binding context
/// applies ECMAScript early errors.
#[test]
fn parser_should_diagnose_escaped_reserved_identifiers_contextually() {
    let options = ParseOptions {
        language: Language::JavaScript,
        source_kind: SourceKind::Script,
        semantic_errors: true,
        ..ParseOptions::default()
    };
    for source in [
        "br\\u0065ak;",
        "var br\\u{65}ak;",
        "tru\\u0065: statement;",
        "({ br\\u0065ak } = source);",
        "({ br\\u0065ak }) => {};",
        "'use strict'; ({ impl\\u0065ments });",
        "async function f() { ({ aw\\u0061it }); }",
        "function* f() { ({ yi\\u0065ld }); }",
        "class C { static field = { await }; }",
        "function f() { n\\u0065w.target; }",
    ] {
        let parsed = parse(source, options).expect(source);
        assert!(!parsed.diagnostics.is_empty(), "{source}");
        parsed.tape.validate().expect(source);
    }

    let allowed = parse(
        "const object = { br\\u0065ak: 1 }; object.br\\u0065ak; class C { br\\u0065ak() {} } const { br\\u0065ak: value } = object;",
        options,
    )
    .expect("allowed escaped IdentifierName positions");
    assert!(allowed.diagnostics.is_empty(), "{:#?}", allowed.diagnostics);
    allowed.tape.validate().expect("valid allowed tape");

    let syntax_only = parse(
        "let br\\u0065ak = 1; ({ br\\u0065ak });",
        ParseOptions {
            language: Language::TypeScript,
            source_kind: SourceKind::Script,
            semantic_errors: false,
            ..ParseOptions::default()
        },
    )
    .expect("syntax-only TypeScript parsing");
    assert!(
        syntax_only.diagnostics.is_empty(),
        "{:#?}",
        syntax_only.diagnostics
    );
    syntax_only.tape.validate().expect("valid syntax-only tape");
}

/// Generator methods retain binding and constructor early errors.
#[test]
fn parser_should_diagnose_generator_method_early_errors() {
    let cases = [
        GrammarCase::script(
            "duplicate object generator parameter",
            "const methods = { *invalid(value, value) {} };",
            &[NodeTag::PROPERTY, NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "yield class generator parameter",
            "class Methods { *invalid(yield) {} }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "generator constructor",
            "class Methods { *constructor() {} }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "static generator prototype",
            "class Methods { static *prototype() {} }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::FUNCTION_EXPRESSION],
        ),
        GrammarCase::script(
            "private generator constructor",
            "class Methods { static async *#constructor() {} }",
            &[NodeTag::METHOD_DEFINITION, NodeTag::FUNCTION_EXPRESSION],
        ),
    ];

    assert_diagnostic_cases(&cases, true);
}

/// Property names retain their key form and method, field, or shorthand role.
#[test]
fn parser_should_accept_property_names_across_objects_patterns_and_classes() {
    let cases = [
        GrammarCase::script(
            "object property names",
            "const object = { [key]: value, return: keyword, \"text\": stringValue, 0: numeric, 1n: bigint, shorthand, [method]() { return value; }, default() { return value; } };",
            &[
                NodeTag::OBJECT_EXPRESSION,
                NodeTag::PROPERTY,
                NodeTag::FUNCTION_EXPRESSION,
            ],
        ),
        GrammarCase::script(
            "binding pattern property names",
            "const { [key]: computed, return: keyword, \"text\": stringValue, 0: numeric, 1n: bigint, shorthand = fallback } = source;",
            &[
                NodeTag::OBJECT_PATTERN,
                NodeTag::PROPERTY,
                NodeTag::ASSIGNMENT_PATTERN,
            ],
        ),
        GrammarCase::script(
            "assignment pattern property names",
            "({ [key]: computed, return: keyword, \"text\": stringValue, 0: numeric, 1n: bigint } = source);",
            &[NodeTag::OBJECT_PATTERN, NodeTag::PROPERTY],
        ),
        GrammarCase::script(
            "class property names",
            "class Properties { [field] = value; [method]() {} return() {} \"text\" = value; 0() {} 1n = value; }",
            &[
                NodeTag::CLASS_DECLARATION,
                NodeTag::PROPERTY_DEFINITION,
                NodeTag::METHOD_DEFINITION,
            ],
        ),
    ];

    assert_clean_cases(&cases);
}

/// Recoverable syntax errors must still return a validated program tape.
#[test]
fn parser_should_recover_with_a_valid_tape() {
    let cases = [
        GrammarCase::script("missing binding", "const = 1; next();", &[]),
        GrammarCase::script("missing condition", "if () { next(); }", &[]),
        GrammarCase::script("missing operand", "value + ; next();", &[]),
        GrammarCase::script("unterminated block", "function run() { return 1;", &[]),
        GrammarCase::script("throw line break", "throw\nvalue;", &[]),
        GrammarCase::script("unterminated literal", "const value = 'text", &[]),
        GrammarCase::script(
            "non-identifier label target",
            "this.property: value;",
            &[NodeTag::LABELED_STATEMENT],
        ),
        GrammarCase::script(
            "parenthesized label target",
            "(label): value;",
            &[NodeTag::LABELED_STATEMENT],
        ),
    ];

    assert_diagnostic_cases(&cases, false);
}

/// Static-semantics checks must diagnose invalid programs after the grammar has produced valid nodes.
#[test]
fn parser_should_report_javascript_early_errors() {
    let cases = [
        GrammarCase::module(
            "duplicate lexical binding",
            "let value; let value;",
            &[NodeTag::VARIABLE_DECLARATION],
        ),
        GrammarCase::module(
            "return outside function",
            "return value;",
            &[NodeTag::RETURN_STATEMENT],
        ),
        GrammarCase::module(
            "break outside target",
            "break;",
            &[NodeTag::BREAK_STATEMENT],
        ),
        GrammarCase::module(
            "continue to non-loop label",
            "label: { continue label; }",
            &[NodeTag::LABELED_STATEMENT, NodeTag::CONTINUE_STATEMENT],
        ),
        GrammarCase::module(
            "with in strict code",
            "with (object) value;",
            &[NodeTag::WITH_STATEMENT],
        ),
        GrammarCase::module(
            "duplicate label",
            "label: label: while (true) break label;",
            &[NodeTag::LABELED_STATEMENT],
        ),
        GrammarCase::module(
            "delete identifier in strict code",
            "delete value;",
            &[NodeTag::UNARY_EXPRESSION],
        ),
        GrammarCase::module(
            "duplicate export",
            "const value = 1; export { value }; export { value };",
            &[NodeTag::EXPORT_NAMED_DECLARATION],
        ),
        GrammarCase::module(
            "optional call arrow parameter",
            "value?.() => result;",
            &[
                NodeTag::CHAIN_EXPRESSION,
                NodeTag::ARROW_FUNCTION_EXPRESSION,
            ],
        ),
    ];

    assert_diagnostic_cases(&cases, true);
}

fn assert_clean_cases(cases: &[GrammarCase]) {
    let mut failures = Vec::new();
    for &case in cases {
        match parse(case.source, case.options(false)) {
            Err(error) => failures.push(format!("{}: parse failed: {error}", case.name)),
            Ok(parsed) => {
                let tags = match inspect_tape(case.name, &parsed) {
                    Ok(tags) => tags,
                    Err(reason) => {
                        failures.push(reason);
                        continue;
                    }
                };
                if !parsed.diagnostics.is_empty() {
                    failures.push(format!(
                        "{}: diagnostics: {:?}",
                        case.name, parsed.diagnostics
                    ));
                }
                for &tag in case.expected_tags {
                    if !tags.contains(&tag) {
                        failures.push(format!("{}: missing node tag {tag:?}", case.name));
                    }
                }
            }
        }
    }
    assert_failures_empty(&failures);
}

fn assert_diagnostic_cases(cases: &[GrammarCase], semantic_errors: bool) {
    let mut failures = Vec::new();
    for &case in cases {
        match parse(case.source, case.options(semantic_errors)) {
            Err(error) => failures.push(format!(
                "{}: parse failed instead of recovering: {error}",
                case.name
            )),
            Ok(parsed) => {
                let tags = match inspect_tape(case.name, &parsed) {
                    Ok(tags) => tags,
                    Err(reason) => {
                        failures.push(reason);
                        continue;
                    }
                };
                if parsed.diagnostics.is_empty() {
                    failures.push(format!("{}: expected a diagnostic", case.name));
                }
                for &tag in case.expected_tags {
                    if !tags.contains(&tag) {
                        failures.push(format!("{}: missing recovered node tag {tag:?}", case.name));
                    }
                }
            }
        }
    }
    assert_failures_empty(&failures);
}

fn inspect_tape(name: &str, parsed: &ParseResult) -> Result<Vec<NodeTag>, String> {
    parsed
        .tape
        .validate()
        .map_err(|error| format!("{name}: invalid tape: {error}"))?;
    match parsed.tape.value_at(parsed.tape.header().root) {
        Ok(TapeValue::Node {
            tag: NodeTag::PROGRAM,
            ..
        }) => {}
        Ok(value) => return Err(format!("{name}: root is not Program: {value:?}")),
        Err(error) => return Err(format!("{name}: root lookup failed: {error}")),
    }

    let mut tags = Vec::new();
    for record in parsed.tape.validation() {
        let record =
            record.map_err(|error| format!("{name}: record validation failed: {error}"))?;
        if let TapeValue::Node { tag, .. } = record.value {
            tags.push(tag);
        }
    }
    Ok(tags)
}

fn check_expression_shape(case: ExpressionShape) -> Result<(), String> {
    let parsed = parse(
        case.source,
        ParseOptions {
            source_kind: SourceKind::Script,
            ..ParseOptions::default()
        },
    )
    .map_err(|error| format!("parse failed: {error}"))?;
    inspect_tape(case.name, &parsed)?;
    if !parsed.diagnostics.is_empty() {
        return Err(format!("diagnostics: {:?}", parsed.diagnostics));
    }

    let expression = first_expression_offset(&parsed.tape)?;
    let root_fields = node_fields(&parsed.tape, expression, case.root)?;
    let nested = root_fields
        .get(case.nested_field)
        .copied()
        .ok_or_else(|| format!("{:?} lacks field {}", case.root, case.nested_field))?;
    let nested_tag = node_tag(&parsed.tape, nested)?;
    if nested_tag != case.nested {
        return Err(format!(
            "field {} of {:?} is {nested_tag:?}, expected {:?}",
            case.nested_field, case.root, case.nested
        ));
    }
    Ok(())
}

fn first_expression_offset(tape: &FrozenTape) -> Result<u32, String> {
    let program = node_fields(tape, tape.header().root, NodeTag::PROGRAM)?;
    let body_offset = program.first().copied().ok_or("Program lacks body")?;
    let statements = match tape
        .value_at(body_offset)
        .map_err(|error| error.to_string())?
    {
        TapeValue::List { items, .. } => items,
        value => return Err(format!("Program body is not a list: {value:?}")),
    };
    let statement = statements.first().copied().ok_or("Program body is empty")?;
    let statement = node_fields(tape, statement, NodeTag::EXPRESSION_STATEMENT)?;
    statement
        .first()
        .copied()
        .ok_or_else(|| "ExpressionStatement lacks expression".to_owned())
}

fn node_fields(tape: &FrozenTape, offset: u32, expected: NodeTag) -> Result<&[u32], String> {
    match tape.value_at(offset).map_err(|error| error.to_string())? {
        TapeValue::Node { tag, fields, .. } if tag == expected => Ok(fields),
        TapeValue::Node { tag, .. } => Err(format!("node is {tag:?}, expected {expected:?}")),
        value => Err(format!("record is not a node: {value:?}")),
    }
}

fn list_items(tape: &FrozenTape, offset: u32) -> Result<&[u32], String> {
    match tape.value_at(offset).map_err(|error| error.to_string())? {
        TapeValue::List { items, .. } => Ok(items),
        value => Err(format!("record is not a list: {value:?}")),
    }
}

fn node_tag(tape: &FrozenTape, offset: u32) -> Result<NodeTag, String> {
    match tape.value_at(offset).map_err(|error| error.to_string())? {
        TapeValue::Node { tag, .. } => Ok(tag),
        value => Err(format!("record is not a node: {value:?}")),
    }
}

fn assert_failures_empty(failures: &[String]) {
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
